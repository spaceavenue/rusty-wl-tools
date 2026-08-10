#![no_std]
#![no_main]

pub mod error;
pub mod image_load;
pub mod remove_self;
pub mod shm;
pub mod state;

use wllib::dispatch::dispatch_once;
use wllib::error::WireError::ConnectionClosed;
use wllib::fmt_lite::write_stderr;
use wllib::protocols::{wl_compositor, wl_surface, zwlr_layer_shell_v1, zwlr_layer_surface_v1};
use wllib::registry::crawl;
use wllib::transport::Connection;
use wllib::wire::Message;

use crate::state::{Config, State};

unsafe extern "C" {
  static optarg: *const libc::c_char;
  static mut optind: libc::c_int;
}
#[link(name = "c", kind = "static")]
unsafe extern "C" {}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn main(argc: isize, argv: *const *mut libc::c_char) -> libc::c_int {
  let mut config = Config::default();
  loop {
    let c = unsafe { libc::getopt(argc as i32, argv, c"fn:t:".as_ptr()) };
    if c == -1 {
      break;
    }
    match c as u8 as char {
      'f' => config.fill = true,
      'n' => {
        if unsafe { optarg.is_null() } {
          break;
        }
        config.namespace = unsafe { optarg }
      }
      _ => (),
    }
  }
  let optind_val = unsafe { optind };
  if optind_val < argc as libc::c_int {
    config.image_path = Some(unsafe { *argv.add((optind_val) as usize) });
  }

  if config.image_path.is_none() {
    write_stderr(b"Usage: rustbg [-f | --fill] [-n | --namespace <name>] <image path>\n");
    unsafe { libc::exit(1) };
  }

  let mut conn = match Connection::connect() {
    Ok(c) => c,
    Err(e) => {
      e.write_diagnostic();
      unsafe { libc::exit(1) };
    }
  };

  let mut state = State::init(config);

  if let Err(e) = crawl(&mut conn, &mut state) {
    e.write_diagnostic();
    unsafe { libc::exit(1) };
  }

  // setup layer surfaces and gamma control for all monitors.
  for i in 0..state.output_len {
    // alloc ids for the new wl_surface (from compositor), layer surface, and gamma control
    // objects.
    let surf_id = conn.alloc_id();
    let layer_surf_id = conn.alloc_id();
    let output_id = state.outputs[i].output_id;

    // this is a multi step process
    //
    // 1. creates a wl_surface
    let mut surf_msg = Message::new(
      state.global.compositor_id,
      wl_compositor::request::CREATE_SURFACE,
    );
    surf_msg.write_u32(surf_id);
    conn.send_logged(&surf_msg, None);

    // 2. wrap the wl_surface as a layer surface
    let mut ls_msg = Message::new(
      state.global.layer_shell_id,
      zwlr_layer_shell_v1::request::GET_LAYER_SURFACE,
    );
    ls_msg.write_u32(layer_surf_id);
    ls_msg.write_u32(surf_id);
    ls_msg.write_u32(output_id);
    ls_msg.write_u32(zwlr_layer_shell_v1::layer::BACKGROUND);
    ls_msg.write_cstr(unsafe { core::ffi::CStr::from_ptr(state.config.namespace) });
    conn.send_logged(&ls_msg, None);

    // anchor to all edges for full screen
    let mut anchor_msg = Message::new(layer_surf_id, zwlr_layer_surface_v1::request::SET_ANCHOR);
    anchor_msg.write_u32(zwlr_layer_surface_v1::anchor::ALL);
    conn.send_logged(&anchor_msg, None);

    // configure exclusive zone to go behind other layer surfaces, like panels or status bars
    let mut ex_msg = Message::new(
      layer_surf_id,
      zwlr_layer_surface_v1::request::SET_EXCLUSIVE_ZONE,
    );
    // -1 -> do not request any exclusive zone.
    ex_msg.write_i32(-1);
    conn.send_logged(&ex_msg, None);

    // initial attach
    let mut attach_msg = Message::new(surf_id, wl_surface::request::ATTACH);
    // NULL -> map surface
    attach_msg.write_u32(0);
    attach_msg.write_i32(0);
    attach_msg.write_i32(0);
    conn.send_logged(&attach_msg, None);
    // commit initial surface
    conn.send_logged(&Message::new(surf_id, wl_surface::request::COMMIT), None);

    // store the allocated ids back into our Output instance
    state.outputs[i].wl_surface_id = surf_id;
    state.outputs[i].layer_surface_id = layer_surf_id;
  }

  // NULL -> map surface
  // probably doesnt even do anything atp
  // (not like it did before either, madvise is just a "strong suggestion")
  // but still, for the illusion ig :3
  // also the name is kinda funny
  remove_self::evict_self_from_ram();

  // main wayland event dispatch loop
  loop {
    match dispatch_once(&mut conn, &mut state) {
      Ok(_) => (),
      Err(ConnectionClosed) => break,
      Err(e) => {
        e.write_diagnostic();
        break;
      }
    }
  }
  0
}
