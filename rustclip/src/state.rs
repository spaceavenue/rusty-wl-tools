use wllib::dispatch::EventHandler;
use wllib::error::{SysError, WireError};
use wllib::fmt_lite::write_stdout;
use wllib::protocols::{zwlr_data_control_device_v1, zwlr_data_control_offer_v1};
use wllib::registry::{GlobalHandler, bind, clamp_version};
use wllib::transport::Connection;
use wllib::wire::{Message, read_str, read_u32};

use crate::error::AppError;
use crate::mime::{MAX_MIME_TYPES, MimeType, pick_mime};

// tracks registered object ids
#[derive(Default)]
pub struct Global {
  // wl_seat global id
  pub seat_id: u32,
  // zwlr_data_control_manager global id
  pub manager_id: u32,
}

pub enum Action {
  PrintAndExit,
  ListTypes,
  PipeToCommand {
    argv: *const *mut libc::c_char,
    argc: usize,
  },
}

pub struct State {
  pub global: Global,
  pub device_id: u32,
  pub use_primary: bool,
  pub wanted_mime: Option<MimeType>,
  pub action: Action,
  // set when a null offer id (cleared/emtpy clipboard) is received, so that a caller like
  // `wl-paste` can differentiate between "nothing was selected" and "still waiting..."
  pub had_empty_selection: bool,
  // the offer object currently being *built* via data_offer + offer events, before a `selection`
  // event confirms it as the active one. `building_id` is 0 until data_offer names one.
  building_id: u32,
  building_mimes: [MimeType; MAX_MIME_TYPES],
  building_mime_len: usize,
}
impl State {
  pub fn init(use_primary: bool, wanted_mime: Option<MimeType>, action: Action) -> Self {
    Self {
      global: Global::default(),
      device_id: 0,
      use_primary,
      wanted_mime,
      action,
      had_empty_selection: false,
      building_id: 0,
      building_mimes: [MimeType::default(); MAX_MIME_TYPES],
      building_mime_len: 0,
    }
  }

  // if we get a data offer, we update building the building id to whatever the data offer sets.
  // otherwise, we check which clipboard we're operating on and handle the selection.
  fn handle_device_event(&mut self, conn: &mut Connection, opcode: u16, data: &[u8]) {
    if opcode == zwlr_data_control_device_v1::event::DATA_OFFER {
      self.building_id = read_u32(data, 0);
      self.building_mime_len = 0;
      return;
    }
    let is_target_event = if self.use_primary {
      opcode == zwlr_data_control_device_v1::event::PRIMARY_SELECTION
    } else {
      opcode == zwlr_data_control_device_v1::event::SELECTION
    };
    if is_target_event {
      self.handle_selection(conn, read_u32(data, 0));
    }
  }

  fn handle_selection(&mut self, conn: &mut Connection, offer_id: u32) {
    // offer id 0 means empty clipboard or that it was cleared.
    if offer_id == 0 {
      self.had_empty_selection = true;
      return;
    }

    // by protocol ordering, data_offer + its offer events always immediately precede the
    // selection event naming it, so the offer just confirmed is whichever one we were building.
    if offer_id != self.building_id {
      return;
    }

    // handle listing the mime types
    if let Action::ListTypes = &self.action {
      for i in 0..self.building_mime_len {
        write_stdout(self.building_mimes[i].as_bytes());
        write_stdout(b"\n");
      }
      unsafe { libc::exit(0) };
    }

    // owned copy, see note in fetch_offer below
    let mimes = self.building_mimes;
    let mime_len = self.building_mime_len;
    let wanted = self.wanted_mime;
    let Some(mime) = pick_mime(&mimes, mime_len, wanted.as_ref().map(|m| m.as_str())) else {
      AppError::MimeNotAvailable.write_diagnostic();
      unsafe { libc::exit(1) };
    };

    match fetch_offer(conn, offer_id, &mime) {
      Ok(read_fd) => {
        match &self.action {
          Action::PrintAndExit => {
            stream_fd_to_stdout(read_fd);
            unsafe { libc::close(read_fd) };
            unsafe { libc::exit(0) };
          }
          Action::PipeToCommand { argv, argc } => {
            run_with_stdin(read_fd, *argv, *argc);
            unsafe { libc::close(read_fd) };
          }
          // handled above, before fetch_offer is called
          Action::ListTypes => unreachable!(),
        }
      }
      Err(e) => e.write_diagnostic(),
    }
  }
}
impl GlobalHandler for State {
  // bind globals matching interfaces we want. we bind to the minimum of the client's wanted
  // version and the server's advertised version
  fn on_global(&mut self, conn: &mut Connection, name: u32, interface: &str, version: u32) {
    match interface {
      "wl_seat" => {
        if self.global.seat_id != 0 {
          return;
        }
        let id = conn.alloc_id();
        match bind(conn, name, interface, clamp_version(1, version), id) {
          Ok(()) => self.global.seat_id = id,
          Err(e) => e.write_diagnostic(),
        }
      }
      "zwlr_data_control_manager_v1" => {
        let id = conn.alloc_id();
        match bind(conn, name, interface, clamp_version(1, version), id) {
          Ok(()) => self.global.manager_id = id,
          Err(e) => e.write_diagnostic(),
        }
      }
      _ => (),
    }
  }
}
impl EventHandler for State {
  fn handle_event(&mut self, conn: &mut Connection, sender: u32, opcode: u16, data: &[u8]) {
    if sender == self.device_id {
      self.handle_device_event(conn, opcode, data);
    }
    if sender == self.building_id
      && opcode == zwlr_data_control_offer_v1::event::OFFER
      && let Some((mime_str, _)) = read_str(data, 0)
      && self.building_mime_len < MAX_MIME_TYPES
    {
      self.building_mimes[self.building_mime_len] = MimeType::from(mime_str);
      self.building_mime_len += 1;
    }
  }
}

// send `receive(mime, write_fd)` on `offer_id` using a locally-created pipe, then return the read
// end. we close our own copy of the write end immediately after sending it. the *other* copy (now
// held by whichever process is providing the data) still needs to close its own copy too before the
// reader sees EOF, but if we kept ours open as well, EOF would never arrive at all even after the
// real writer finishes.
fn fetch_offer(
  conn: &mut Connection,
  offer_id: u32,
  mime: &MimeType,
) -> Result<libc::c_int, AppError> {
  let mut pipe_fds = [0i32; 2];
  if unsafe { libc::pipe2(pipe_fds.as_mut_ptr(), libc::O_CLOEXEC) } != 0 {
    return Err(AppError::Sys(SysError::last("pipe2")));
  }
  let [read_fd, write_fd] = pipe_fds;

  let mut msg = Message::new(offer_id, zwlr_data_control_offer_v1::request::RECEIVE);
  msg.write_str(mime.as_str());
  conn.send_logged(&msg, Some(write_fd));

  unsafe { libc::close(write_fd) };

  Ok(read_fd)
}

// stream `read_fd` to stdout until EOF in fixed-size chunks. clipboard content isn't assumed to
// fit in memory all at once.
fn stream_fd_to_stdout(read_fd: libc::c_int) {
  let mut buf = [0u8; 8192];
  loop {
    let n = unsafe { libc::read(read_fd, buf.as_mut_ptr() as *mut _, buf.len()) };
    if n <= 0 {
      break;
    }
    let mut written = 0usize;
    while written < n as usize {
      let w = unsafe {
        libc::write(
          1,
          buf[written..n as usize].as_ptr() as *const _,
          n as usize - written,
        )
      };
      if w <= 0 {
        break;
      }
      written += w as usize;
    }
  }
}

// fork, wire `read_fd` up as the child's stdin, and exec `argv`. the caller is expected to have set
// `SIGCHLD` to `SIG_IGN` (like in rustidle) so the kernel reaps the child automatically.
fn run_with_stdin(read_fd: libc::c_int, argv: *const *mut libc::c_char, argc: usize) {
  if argc == 0 {
    return;
  }
  unsafe {
    let pid = libc::fork();
    if pid < 0 {
      WireError::Sys(SysError::last("fork")).write_diagnostic();
      return;
    }
    if pid == 0 {
      libc::dup2(read_fd, 0);
      // parent's copy of read_fd is no longer needed once the child has its own via dup2.
      libc::close(read_fd);
      // `argv` came straight from `main`'s own argv, which the OS guarantees is NUL-terminated at
      // argv[argc]. so, a pointer offset into it is still validly NUL-terminated and this can be
      // handed to execvp directly without rebuilding it.
      libc::execvp(*argv as _, argv as _);
      // only reached if exec itself failed
      libc::_exit(1);
    }
  }
}
