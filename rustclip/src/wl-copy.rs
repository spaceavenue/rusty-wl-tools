#![no_std]
#![no_main]

// `wl-copy` doesn't use `wllib::dispatch`/`EventHandler` for its runtime loop, because
// `zwlr_data_control_source_v1::send` hands this process a file descriptor as ancillary data which
// `dispatch_once`'s loop has no way to carry back to a handler. it wasn't designed around any event
// needing more than the plain argument bytes. so, this binary runs its own loop directly on top of
// `Connection::recv_with_fd`, and only uses `crawl`/`GlobalHandler` for the registry phase.

use rustclip::error::AppError;
use rustclip::mime::{GENERIC_TEXT_OFFERS, MimeType, infer_mime_type_from_fd, is_text_mime};
use wllib::cli;
use wllib::dispatch::EventHandler;
use wllib::error::{SysError, WireError};
use wllib::fmt_lite::write_stderr;
use wllib::protocols::{
  zwlr_data_control_device_v1, zwlr_data_control_manager_v1, zwlr_data_control_source_v1,
};
use wllib::registry::{GlobalHandler, bind, clamp_version, crawl};
use wllib::transport::Connection;
use wllib::wire::{Message, parse_header};

#[derive(Default)]
struct Global {
  seat_id: u32,
  manager_id: u32,
}

struct State {
  global: Global,
}

impl GlobalHandler for State {
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
        match bind(conn, name, interface, clamp_version(2, version), id) {
          Ok(()) => self.global.manager_id = id,
          Err(e) => e.write_diagnostic(),
        }
      }
      _ => (),
    }
  }
}

// stub for satisfying `crawl`'s EventHandler trait bound.
impl EventHandler for State {
  fn handle_event(&mut self, _conn: &mut Connection, _sender: u32, _opcode: u16, _data: &[u8]) {}
}

#[link(name = "c", kind = "static")]
unsafe extern "C" {}

unsafe extern "C" {
  static optarg: *const libc::c_char;
  static mut optind: libc::c_int;
}

const OPTSTRING: *const i8 = c"pont:".as_ptr();

const LONGOPTS: [cli::LongOption; 7] = [
  cli::LongOption::new(c"use-primary", cli::NO_ARGUMENT, 'p'),
  cli::LongOption::new(c"primary", cli::NO_ARGUMENT, 'p'),
  cli::LongOption::new(c"paste-once", cli::NO_ARGUMENT, 'o'),
  cli::LongOption::new(c"trim-newline", cli::NO_ARGUMENT, 'n'),
  cli::LongOption::new(c"foreground", cli::NO_ARGUMENT, 'f'),
  cli::LongOption::new(c"type", cli::REQUIRED_ARGUMENT, 't'),
  cli::LONG_OPTION_TERMINATOR,
];

#[unsafe(no_mangle)]
pub unsafe extern "C" fn main(argc: isize, argv: *const *mut libc::c_char) -> libc::c_int {
  let mut use_primary = false;
  let mut paste_once = false;
  let mut trim_newline = false;
  let mut foreground = false;
  let mut wanted_mime: Option<MimeType> = None;
  let mut longindex: libc::c_int = 0;
  unsafe { optind = 1 };

  loop {
    let c = unsafe {
      cli::getopt_long(
        argc as _,
        argv,
        OPTSTRING,
        LONGOPTS.as_ptr(),
        &mut longindex,
      )
    };
    if c == -1 {
      break;
    }
    match c as u8 as char {
      'p' => use_primary = true,
      'o' => paste_once = true,
      'n' => trim_newline = true,
      'f' => foreground = true,
      't' if !unsafe { optarg.is_null() } => {
        wanted_mime = Some(MimeType::from(unsafe { core::ffi::CStr::from_ptr(optarg) }));
      }
      _ => (),
    }
  }

  // Create an anonymous file descriptor in RAM to hold the clipboard data
  let optind_val = unsafe { optind };
  let memfd = unsafe { libc::memfd_create(c"wl-clip".as_ptr(), libc::MFD_CLOEXEC) };
  if memfd < 0 {
    AppError::Sys(SysError::last("memfd_create")).write_diagnostic();
    unsafe { libc::exit(1) };
  }
  let is_stdin = optind_val >= argc as libc::c_int;
  if is_stdin {
    if let Err(e) = write_stdin_to_fd(memfd) {
      e.write_diagnostic();
      unsafe { libc::exit(1) };
    }
  } else if let Err(e) = write_args_to_fd(memfd, argv, optind_val as usize, argc as usize) {
    e.write_diagnostic();
    unsafe { libc::exit(1) };
  }

  trim_trailing_newline_if_requested(memfd, trim_newline);

  // only stdin content is inferred from — an explicit argument list is already text the user
  // typed on the command line, and `-t` always takes priority over either.
  let inferred_mime = if is_stdin && wanted_mime.is_none() {
    infer_mime_type_from_fd(memfd)
  } else {
    None
  };
  let fd_to_copy = memfd;

  let mut conn = match Connection::connect() {
    Ok(c) => c,
    Err(e) => {
      e.write_diagnostic();
      unsafe { libc::exit(1) };
    }
  };

  let mut state = State {
    global: Global::default(),
  };
  if let Err(e) = crawl(&mut conn, &mut state) {
    e.write_diagnostic();
    unsafe { libc::exit(1) };
  }
  if state.global.manager_id == 0 {
    write_stderr("[wl-copy]: compositor does not support wlr-data-control\n");
    unsafe { libc::exit(1) };
  }
  if state.global.seat_id == 0 {
    write_stderr("[wl-copy]: no wl_seat found\n");
    unsafe { libc::exit(1) };
  }
  let manager_id = state.global.manager_id;
  let seat_id = state.global.seat_id;

  let source_id = conn.alloc_id();
  let mut create_msg = Message::new(
    manager_id,
    zwlr_data_control_manager_v1::request::CREATE_DATA_SOURCE,
  );
  create_msg.write_u32(source_id);
  conn.send_logged(&create_msg, None);

  let primary_mime: Option<&str> = wanted_mime
    .as_ref()
    .map(|m| m.as_str())
    .or(inferred_mime.as_ref().map(|m| m.as_str()));

  if let Some(m) = primary_mime {
    let mut offer_msg = Message::new(source_id, zwlr_data_control_source_v1::request::OFFER);
    offer_msg.write_str(m);
    conn.send_logged(&offer_msg, None);
  }
  // can unwrap() here since we are already checking if it's None. if that check doesnt pass it is
  // bound to be Some.
  if primary_mime.is_none() || is_text_mime(primary_mime.unwrap()) {
    for &pref in &GENERIC_TEXT_OFFERS {
      if primary_mime != Some(pref) {
        let mut offer_msg = Message::new(source_id, zwlr_data_control_source_v1::request::OFFER);
        offer_msg.write_str(pref);
        conn.send_logged(&offer_msg, None);
      }
    }
  }

  let device_id = conn.alloc_id();
  let mut device_msg = Message::new(
    manager_id,
    zwlr_data_control_manager_v1::request::GET_DATA_DEVICE,
  );
  device_msg.write_u32(device_id);
  device_msg.write_u32(seat_id);
  conn.send_logged(&device_msg, None);

  let set_selection_opcode = if use_primary {
    zwlr_data_control_device_v1::request::SET_PRIMARY_SELECTION
  } else {
    zwlr_data_control_device_v1::request::SET_SELECTION
  };
  let mut select_msg = Message::new(device_id, set_selection_opcode);
  select_msg.write_u32(source_id);
  conn.send_logged(&select_msg, None);

  if !foreground {
    let pid = unsafe { libc::fork() };
    if pid < 0 {
      write_stderr("[wl-copy]: failed to fork process\n");
      unsafe { libc::exit(1) };
    } else if pid > 0 {
      unsafe { libc::exit(0) };
    }
  }

  // serve `send`/`cancelled` events on the child's source until someone else takes over the
  // selection (or with -o, until we've served exactly one request).
  loop {
    let mut buf = [0u8; 4096];
    let (result, fd) = conn.recv_with_fd(&mut buf);
    let data = match result {
      Ok(items) => items,
      Err(WireError::ConnectionClosed) => break,
      Err(e) => {
        e.write_diagnostic();
        break;
      }
    };

    let mut pending_fd = fd;
    let mut idx = 0;
    while let Some(header) = parse_header(data, idx) {
      if header.sender == source_id {
        match header.opcode {
          zwlr_data_control_source_v1::event::SEND => {
            if let Some(target_fd) = pending_fd.take() {
              write_content_to_fd(fd_to_copy, target_fd);
              if paste_once {
                unsafe { libc::exit(0) };
              }
            } else {
              write_stderr("[wl-copy]: got a send event with no accompanying fd, ignoring\n");
            }
          }
          zwlr_data_control_source_v1::event::CANCELLED => {
            unsafe { libc::exit(0) };
          }
          _ => (),
        }
      }
      idx += header.size as usize;
    }
    // prevent leaking an fd that arrived but wasn't claimed by any SEND event.
    if let Some(unclaimed_fd) = pending_fd {
      unsafe { libc::close(unclaimed_fd) };
    }
  }
  unsafe { libc::close(fd_to_copy) };
  0
}

fn write_content_to_fd(source_fd: libc::c_int, target_fd: libc::c_int) {
  // reset the file offset of the memfd to the beginning before copying
  unsafe { libc::lseek(source_fd, 0, libc::SEEK_SET) };
  let mut buf = [0u8; 4096];
  loop {
    let r = unsafe { libc::read(source_fd, buf.as_mut_ptr() as _, buf.len()) };
    if r <= 0 {
      break;
    }
    let mut written = 0;
    while written < r {
      let w = unsafe {
        libc::write(
          target_fd,
          buf.as_ptr().add(written as _) as _,
          (r - written) as _,
        )
      };
      if w <= 0 {
        break;
      }
      written += w;
    }
  }
  unsafe { libc::close(target_fd) };
}

fn write_stdin_to_fd(memfd: libc::c_int) -> Result<(), AppError> {
  let mut chunk = [0u8; 4096];
  loop {
    let n = unsafe { libc::read(0, chunk.as_mut_ptr() as *mut _, chunk.len()) };
    match n {
      n if n > 0 => {
        let mut written = 0;
        while written < n {
          let w = unsafe {
            libc::write(
              memfd,
              chunk.as_ptr().add(written as _) as _,
              (n - written) as _,
            )
          };
          match w {
            w if w > 0 => written += w,
            0 => break,
            _ => return Err(AppError::Sys(SysError::last("write"))),
          }
        }
      }
      0 => break,
      _ => return Err(AppError::Sys(SysError::last("read"))),
    }
  }
  Ok(())
}

// trims a single trailing '\n' from `fd`'s content in place, if `trim_newline` was requested.
fn trim_trailing_newline_if_requested(fd: libc::c_int, trim_newline: bool) {
  if !trim_newline {
    return;
  }
  let size = unsafe { libc::lseek(fd, 0, libc::SEEK_END) };
  if size > 0 {
    let mut last_byte = [0u8; 1];
    unsafe { libc::lseek(fd, size - 1, libc::SEEK_SET) };
    if unsafe { libc::read(fd, last_byte.as_mut_ptr() as _, 1) == 1 } && last_byte[0] == b'\n' {
      unsafe { libc::ftruncate(fd, size - 1) };
    }
  }
}

fn write_args_to_fd(
  memfd: libc::c_int,
  argv: *const *mut libc::c_char,
  start: usize,
  argc: usize,
) -> Result<(), AppError> {
  for i in start..argc {
    if i > start {
      let w = unsafe { libc::write(memfd, b" ".as_ptr() as _, 1) };
      match w {
        w if w > 0 => (),
        0 => break,
        _ => return Err(AppError::Sys(SysError::last("write"))),
      }
    }
    let arg = unsafe { *argv.add(i) };
    let bytes = unsafe { core::ffi::CStr::from_ptr(arg) }.to_bytes();
    let mut written = 0;
    while written < bytes.len() {
      let w = unsafe {
        libc::write(
          memfd,
          bytes.as_ptr().add(written as _) as _,
          (bytes.len() - written) as _,
        )
      };
      match w {
        w if w > 0 => written += w as usize,
        0 => break,
        _ => return Err(AppError::Sys(SysError::last("write"))),
      }
    }
  }
  Ok(())
}
