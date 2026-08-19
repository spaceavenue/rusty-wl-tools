use wllib::dispatch::EventHandler;
use wllib::fmt_lite::{StringOnStack, write_stderr, write_stdout};
use wllib::protocols::{wl_output, zxdg_output_manager_v1, zxdg_output_v1};
use wllib::registry::{GlobalHandler, bind, clamp_version};
use wllib::transport::Connection;
use wllib::wire::{Message, read_i32, read_str, read_u32};

pub const MAX_OUTPUTS: usize = 8;

// `wl_output::geometry`'s `transform` arg, decoded from `wayland.xml`'s `wl_output.transform`
// enum (0-7).
fn transform_name(t: u32) -> &'static str {
  match t {
    0 => "normal",
    1 => "90",
    2 => "180",
    3 => "270",
    4 => "flipped",
    5 => "flipped-90",
    6 => "flipped-180",
    7 => "flipped-270",
    _ => "unknown",
  }
}

// `wl_output::geometry`'s `subpixel` arg, decoded from `wayland.xml`'s `wl_output.subpixel`
// enum (0-5).
fn subpixel_name(s: i32) -> &'static str {
  match s {
    0 => "unknown",
    1 => "none",
    2 => "horizontal-rgb",
    3 => "horizontal-bgr",
    4 => "vertical-rgb",
    5 => "vertical-bgr",
    _ => "unknown",
  }
}

// tracks registered object ids
#[derive(Default)]
pub struct Global {
  // wl_compositor global id
  pub compositor_id: u32,
  // zxdg_output_manager_v1 global id, 0 if the compositor doesn't advertise it. when absent,
  // outputs are printed with physical-only geometry from `wl_output`, no logical position/size.
  pub xdg_output_manager_id: u32,
}

#[derive(Default)]
pub struct Mode {
  pub flags: u32,
  pub width: i32,
  pub height: i32,
  pub refresh: i32,
}

#[derive(Default)]
pub struct Geometry {
  pub x: i32,
  pub y: i32,
  pub physical_width: i32,
  pub physical_height: i32,
  pub subpixel: i32,
  pub make: StringOnStack<64>,
  pub model: StringOnStack<64>,
  pub transform: u32,
}

// wl_output's physical geometry can differ from the output's actual position/size in the
// compositor's layout once scaling/rotation/fractional-scale are involved (see
// xdg-output-unstable-v1.xml's `logical_size` docs). `logical_*` here is that compositor-space
// geometry.  zero/unset when `xdg_output_id` is 0.
#[derive(Default)]
pub struct Logical {
  pub x: i32,
  pub y: i32,
  pub width: i32,
  pub height: i32,
}

// abstration around wl_output
#[derive(Default)]
pub struct Output {
  pub output_id: u32,
  pub name: StringOnStack<64>,
  pub desc: StringOnStack<64>,
  pub geo: Geometry,
  pub mode: Mode,
  pub scale: i32,
  // id of this output's zxdg_output_v1 object, or 0 if the compositor has no
  // zxdg_output_manager_v1 global or `request_xdg_outputs` hasn't run yet.
  pub xdg_output_id: u32,
  pub logical: Logical,
  // set once wl_output::done has fired for this output.
  wl_done: bool,
  // set once zxdg_output_v1::logical_size has fired for this output. `is_complete` ignores it when
  // `xdg_output_id` is 0.
  logical_done: bool,
  // guard against printing/counting the same output twice.
  printed: bool,
}
impl Output {
  // an output is ready to print once wl_output has finished reporting, and once zxdg_output_v1 has
  // finished reporting, if one was actually requested for it. a compositor with no
  // zxdg_output_manager_v1 global never gets `xdg_output_id` set, so those outputs are ready as
  // soon as wl_output alone is done.
  fn is_complete(&self) -> bool {
    self.wl_done && (self.xdg_output_id == 0 || self.logical_done)
  }
}

pub struct State {
  pub global: Global,
  pub outputs: [Output; MAX_OUTPUTS],
  pub output_len: usize,
  // count of outputs that have printed.
  pub done_count: usize,
}

impl State {
  pub fn init() -> Self {
    Self {
      global: Global::default(),
      outputs: core::array::from_fn(|_| Output::default()),
      output_len: 0,
      done_count: 0,
    }
  }

  // send `get_xdg_output(new_id, wl_output_id)` for every bound output, once the manager global
  // (if any) is known. called once after `crawl()` returns. `zxdg_output_manager_v1` and
  // `wl_output` globals can arrive from the registry in either order, so there's no point trying
  // to request this reactively from inside `on_global`. by the time crawl finishes, every
  // `wl_output` has been bound and the manager global (if any) has definitely been seen.
  pub fn request_xdg_outputs(&mut self, conn: &mut Connection) {
    if self.global.xdg_output_manager_id == 0 {
      return;
    }
    for out in &mut self.outputs[..self.output_len] {
      let id = conn.alloc_id();
      let mut msg = Message::new(
        self.global.xdg_output_manager_id,
        zxdg_output_manager_v1::request::GET_XDG_OUTPUT,
      );
      msg.write_u32(id);
      msg.write_u32(out.output_id);
      conn.send_logged(&msg, None);
      out.xdg_output_id = id;
    }
  }

  // returns `true` once this call has made the output ready to print (see `Output::is_complete`),
  // so the caller can print it and count it toward `done_count` exactly once.
  fn handle_output_event(out: &mut Output, opcode: u16, data: &[u8]) -> bool {
    match opcode {
      wl_output::event::NAME => {
        if let Some((content, _)) = read_str(data, 0) {
          out.name.push(content);
        }
      }
      wl_output::event::DESCRIPTION => {
        if let Some((content, _)) = read_str(data, 0) {
          out.desc.push(content);
        }
      }
      wl_output::event::GEOMETRY => {
        let mut make_len = 0;
        let mut mode_len = 0;
        out.geo.x = read_i32(data, 0);
        out.geo.y = read_i32(data, 4);
        out.geo.physical_width = read_i32(data, 8);
        out.geo.physical_height = read_i32(data, 12);
        out.geo.subpixel = read_i32(data, 16);
        if let Some((make, consumed)) = read_str(data, 20) {
          out.geo.make.push(make);
          make_len = consumed;
        }
        if let Some((model, consumed)) = read_str(data, 20 + make_len) {
          out.geo.model.push(model);
          mode_len = consumed;
        }
        out.geo.transform = read_u32(data, 20 + make_len + mode_len);
      }
      wl_output::event::MODE => {
        out.mode.flags = read_u32(data, 0);
        out.mode.width = read_i32(data, 4);
        out.mode.height = read_i32(data, 8);
        out.mode.refresh = read_i32(data, 12);
      }
      wl_output::event::SCALE => out.scale = read_i32(data, 0),
      wl_output::event::DONE => out.wl_done = true,
      _ => (),
    }
    Self::finish_if_complete(out)
  }

  fn handle_xdg_output_event(out: &mut Output, opcode: u16, data: &[u8]) -> bool {
    match opcode {
      zxdg_output_v1::event::LOGICAL_POSITION => {
        out.logical.x = read_i32(data, 0);
        out.logical.y = read_i32(data, 4);
      }
      zxdg_output_v1::event::LOGICAL_SIZE => {
        out.logical.width = read_i32(data, 0);
        out.logical.height = read_i32(data, 4);
        // only ever sent once per property change and always accompanies a full update, so it's a
        // safe, version-independent "logical info is current" signal without needing to also handle
        // the deprecated `done` event.
        out.logical_done = true;
      }
      _ => (),
    }
    Self::finish_if_complete(out)
  }

  fn finish_if_complete(out: &mut Output) -> bool {
    if out.printed || !out.is_complete() {
      return false;
    }
    out.printed = true;

    let m = &out.mode;
    let g = &out.geo;
    let mut s = StringOnStack::<512>::new();
    s.push("Name: ")
      .push(out.name.as_str())
      .push("\nDescription: ")
      .push(out.desc)
      .push("\nCurrent Mode: ")
      .push(m.width)
      .push("x")
      .push(m.height)
      .push("@")
      .push(m.refresh / 1000)
      .push("\nPhysical Size: ")
      .push(g.physical_width)
      .push("x")
      .push(g.physical_height)
      .push("mm")
      .push("\nPosition: ")
      .push(g.x)
      .push(",")
      .push(g.y);
    if out.xdg_output_id != 0 {
      s.push("\nLogical Position: ")
        .push(out.logical.x)
        .push(",")
        .push(out.logical.y)
        .push("\nLogical Size: ")
        .push(out.logical.width)
        .push("x")
        .push(out.logical.height);
    }

    s.push("\nScale: ")
      .push(out.scale)
      .push("\nTransform: ")
      .push(transform_name(g.transform))
      .push("\nSubpixel: ")
      .push(subpixel_name(g.subpixel));

    write_stdout(s);
    true
  }
}

impl GlobalHandler for State {
  // bind globals matching interfaces we want. we bind to the minimum of the client's wanted
  // version and the server's advertised version
  fn on_global(&mut self, conn: &mut Connection, name: u32, interface: &str, version: u32) {
    match interface {
      "wl_compositor" => {
        let id = conn.alloc_id();
        match bind(conn, name, interface, clamp_version(7, version), id) {
          Ok(()) => self.global.compositor_id = id,
          Err(e) => e.write_diagnostic(),
        }
      }
      "wl_output" => {
        if self.output_len >= MAX_OUTPUTS {
          write_stderr("Maximum outputs limit reached\n");
          return;
        }
        let id = conn.alloc_id();
        match bind(conn, name, interface, clamp_version(4, version), id) {
          Ok(()) => {
            self.outputs[self.output_len] = Output {
              output_id: id,
              ..Default::default()
            };
            self.output_len += 1;
          }
          Err(e) => e.write_diagnostic(),
        }
      }
      "zxdg_output_manager_v1" => {
        let id = conn.alloc_id();
        // only `logical_position`/`logical_size` are wanted.
        match bind(conn, name, interface, clamp_version(1, version), id) {
          Ok(()) => self.global.xdg_output_manager_id = id,
          Err(e) => e.write_diagnostic(),
        }
      }
      _ => (),
    }
  }
}

impl EventHandler for State {
  fn handle_event(&mut self, _conn: &mut Connection, sender: u32, opcode: u16, data: &[u8]) {
    let Some(out) = self.outputs[..self.output_len]
      .iter_mut()
      .find(|out| out.output_id == sender || out.xdg_output_id == sender)
    else {
      return;
    };
    let completed = if out.output_id == sender {
      State::handle_output_event(out, opcode, data)
    } else {
      State::handle_xdg_output_event(out, opcode, data)
    };
    if completed {
      self.done_count += 1;
    }
  }
}
