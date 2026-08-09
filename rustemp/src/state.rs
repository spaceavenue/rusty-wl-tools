use wllib::dispatch::EventHandler;
use wllib::protocols::zwlr_gamma_control_v1;
use wllib::registry::{GlobalHandler, bind, clamp_version};
use wllib::transport::Connection;
use wllib::wire::{Message, read_u32};

use crate::gamma;

// tracks registered object ids
#[derive(Default)]
pub struct Global {
  // wl_compositor global id
  pub compositor_id: u32,
  // wl_shm global id
  pub shm_id: u32,
  // zwlr_layer_shell_v1 global id
  pub gamma_manager_id: u32,
}

// abstration around wl_output
pub struct Output {
  pub global_name: u32,
  pub output_id: u32,
  pub gamma_control_id: u32,
}

// config
pub struct Config {
  pub temp: Option<f64>,
}
// default config
impl Default for Config {
  fn default() -> Self {
    Self { temp: None }
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

  // zwlr_gamma_control_v1::gamma_size(size): the compositor telling us how many entries its
  // gamma ramp expects. We generate ramps for the configured color temperature and hand them
  // over via set_gamma.
  fn handle_gamma_size(&mut self, conn: &mut Connection, opcode: u16, data: &[u8], i: usize) {
    if opcode != zwlr_gamma_control_v1::event::FAILED {
      return;
    }
    let size = read_u32(data, 0) as usize;
    let Some(temp) = self.config.temp else { return };

    // get the fd containing the scaled image data
    let g_fd = match gamma::get_gamma_table_fd(size, temp) {
      Ok(fd) => fd,
      Err(e) => {
        e.write_diagnostic();
        return;
      }
    };
    let gamma_control_id = self.outputs[i].as_ref().unwrap().gamma_control_id;
    // set gamma table
    conn.send_logged(
      &Message::new(gamma_control_id, zwlr_gamma_control_v1::request::SET_GAMMA),
      Some(g_fd),
    );
    unsafe { libc::close(g_fd) };
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
      b"zwlr_gamma_control_manager_v1" => {
        let id = conn.alloc_id();
        match bind(conn, name, interface, clamp_version(1, version), id) {
          Ok(()) => self.global.gamma_manager_id = id,
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
              gamma_control_id: 0,
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
        if o.gamma_control_id == sender {
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
      if out.gamma_control_id == sender {
        this.handle_gamma_size(conn, opcode, data, i);
      }
    };
  }
}
