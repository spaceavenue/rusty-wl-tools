use wllib::dispatch::EventHandler;
use wllib::protocols::{wl_shm, wl_shm_pool, wl_surface, zwlr_layer_surface_v1};
use wllib::registry::{GlobalHandler, bind, clamp_version};
use wllib::transport::Connection;
use wllib::wire::{Message, read_u32};

use crate::shm;

// tracks registered object ids
#[derive(Default)]
pub struct Global {
  // wl_compositor global id
  pub compositor_id: u32,
  // wl_shm global id
  pub shm_id: u32,
  // zwlr_layer_shell_v1 global id
  pub layer_shell_id: u32,
}

// abstration around wl_output
pub struct Output {
  pub global_name: u32,
  pub output_id: u32,
  pub wl_surface_id: u32,
  pub layer_surface_id: u32,
  pub buffer_id: u32,
}

// config
pub struct Config {
  pub image_path: Option<*const libc::c_char>,
  pub namespace: *const libc::c_char,
  pub fill: bool,
}
// default config
impl Default for Config {
  fn default() -> Self {
    Self {
      image_path: Some(c"image.png".as_ptr() as *const libc::c_char),
      namespace: c"wallpaper".as_ptr() as *const libc::c_char,
      fill: false,
    }
  }
}

pub struct State {
  pub global: Global,
  pub outputs: [Option<Output>; 4],
  pub output_len: usize,
  pub config: Config,
}
impl State {
  pub fn init(config: Config) -> Self {
    Self {
      global: Global::default(),
      outputs: core::array::from_fn(|_| None),
      output_len: 0,
      config,
    }
  }

  // zwlr_layer_surface_v1::configure(serial, width, height): the compositor telling us the
  // size to render at. We ack it, render the wallpaper at that size, and attach/commit it.
  fn handle_layer_surface_configure(
    &mut self,
    conn: &mut Connection,
    opcode: u16,
    data: &[u8],
    i: usize,
  ) {
    // event matches a bound layer surface object
    if opcode != zwlr_layer_surface_v1::event::CONFIGURE {
      return;
    }
    let serial = read_u32(data, 0);
    let width = read_u32(data, 4);
    let height = read_u32(data, 8);

    // acknowledge the layer surface configuration
    let layer_surface_id = self.outputs[i].as_ref().unwrap().layer_surface_id;
    let mut ack = Message::new(
      layer_surface_id,
      zwlr_layer_surface_v1::request::ACK_CONFIGURE,
    );
    ack.write_u32(serial);
    conn.send_logged(&ack, None);

    if width == 0 || height == 0 {
      return;
    }

    // get the fd containing the scaled image data
    let fd = match shm::get_image_fd(width, height, &self.config) {
      Ok(fd) => fd,
      Err(e) => {
        e.write_diagnostic();
        return;
      }
    };

    let pool_id = conn.alloc_id();
    let buffer_id = conn.alloc_id();
    let stride = (width * 4) as i32;

    // create a shared memory pool
    let mut pool_msg = Message::new(self.global.shm_id, wl_shm::request::CREATE_POOL);
    pool_msg.write_u32(pool_id);
    pool_msg.write_i32((width * height * 4) as i32);
    conn.send_logged(&pool_msg, Some(fd));
    unsafe { libc::close(fd) };

    // create a wl_buffer from the pool
    let mut buf_msg = Message::new(pool_id, wl_shm_pool::request::CREATE_BUFFER);
    buf_msg.write_u32(buffer_id);
    buf_msg.write_i32(0); // offset
    buf_msg.write_i32(width as i32);
    buf_msg.write_i32(height as i32);
    buf_msg.write_i32(stride);
    buf_msg.write_u32(wl_shm::format::ARGB8888);
    conn.send_logged(&buf_msg, None);

    // destroy the pool object, buffer remains valid tho
    conn.send_logged(&Message::new(pool_id, wl_shm_pool::request::DESTROY), None);

    let Some(ref mut out) = self.outputs[i] else {
      return;
    };
    out.buffer_id = buffer_id;
    let wl_surface_id = out.wl_surface_id;

    // destroy the pool object, buffer remains valid tho
    let mut attach = Message::new(wl_surface_id, wl_surface::request::ATTACH);
    attach.write_u32(buffer_id);
    attach.write_i32(0);
    attach.write_i32(0);
    conn.send_logged(&attach, None);

    // mark the entire surface as damaged
    let mut dmg = Message::new(wl_surface_id, wl_surface::request::DAMAGE_BUFFER);
    dmg.write_i32(0);
    dmg.write_i32(0);
    dmg.write_i32(width as i32);
    dmg.write_i32(height as i32);
    conn.send_logged(&dmg, None);

    // commit the state changes to the surface
    conn.send_logged(
      &Message::new(wl_surface_id, wl_surface::request::COMMIT),
      None,
    );
  }
}
impl GlobalHandler for State {
  // bind globals matching interfaces we want. we bind to the minimum of the client's wanted
  // version and the server's advertised version
  fn on_global(&mut self, conn: &mut Connection, name: u32, interface: &[u8], version: u32) {
    match interface {
      b"wl_compositor" => {
        let id = conn.alloc_id();
        match bind(conn, name, interface, clamp_version(7, version), id) {
          Ok(()) => self.global.compositor_id = id,
          Err(e) => e.write_diagnostic(),
        }
      }
      b"wl_shm" => {
        let id = conn.alloc_id();
        match bind(conn, name, interface, clamp_version(2, version), id) {
          Ok(()) => self.global.shm_id = id,
          Err(e) => e.write_diagnostic(),
        }
      }
      b"zwlr_layer_shell_v1" => {
        let id = conn.alloc_id();
        match bind(conn, name, interface, clamp_version(5, version), id) {
          Ok(()) => self.global.layer_shell_id = id,
          Err(e) => e.write_diagnostic(),
        }
      }
      b"wl_output" => {
        if self.output_len >= 4 {
          return;
        }
        let id = conn.alloc_id();
        match bind(conn, name, interface, clamp_version(4, version), id) {
          Ok(()) => {
            self.outputs[self.output_len] = Some(Output {
              global_name: name,
              output_id: id,
              wl_surface_id: 0,
              layer_surface_id: 0,
              buffer_id: 0,
            });
            self.output_len += 1;
          }
          Err(e) => e.write_diagnostic(),
        }
      }
      _ => (),
    }
  }
}
impl EventHandler for State {
  fn handle_event(&mut self, conn: &mut Connection, sender: u32, opcode: u16, data: &[u8]) {
    let mut out_idx = None;
    for i in 0..self.output_len {
      if let Some(ref o) = self.outputs[i] {
        if o.layer_surface_id == sender {
          out_idx = Some(i);
          break;
        }
      }
    }
    let Some(i) = out_idx else { return };
    {
      let this = &mut *self;
      let Some(ref out) = this.outputs[i] else {
        return;
      };
      let is_layer_surface = out.layer_surface_id == sender;
      if is_layer_surface {
        this.handle_layer_surface_configure(conn, opcode, data, i);
      }
    };
  }
}
