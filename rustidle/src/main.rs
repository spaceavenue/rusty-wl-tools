#![no_std]
#![no_main]

pub mod config;
pub mod error;
pub mod state;

use wllib::dispatch::dispatch_once;
use wllib::error::WireError::ConnectionClosed;
use wllib::fmt_lite::write_stderr;
use wllib::protocols::ext_idle_notifier_v1;
use wllib::registry::crawl;
use wllib::transport::Connection;
use wllib::wire::Message;

use crate::config::Config;
use crate::state::State;

#[link(name = "c", kind = "static")]
unsafe extern "C" {}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn main(argc: isize, argv: *const *mut libc::c_char) -> libc::c_int {
  if argc != 2 {
    write_stderr(b"Usage: rustidle <config-file>\n");
    unsafe { libc::exit(1) };
  }
  // idk why clippy says its not marked unsafe when it i
  // #[allow(clippy::not_unsafe_ptr_arg_deref)]
  let config_path = unsafe { *argv.add(1) };
  if config_path.is_null() {
    write_stderr(b"Usage: rustidle <config-file>\n");
    unsafe { libc::exit(1) };
  }

  unsafe { libc::signal(libc::SIGCHLD, libc::SIG_IGN) };

  let mut conn = match Connection::connect() {
    Ok(c) => c,
    Err(e) => {
      e.write_diagnostic();
      unsafe { libc::exit(1) };
    }
  };
  let config = match Config::load(config_path) {
    Ok(c) => c,
    Err(e) => {
      e.write_diagnostic();
      unsafe { libc::exit(1) }
    }
  };
  let mut state = State::init(config);

  if let Err(e) = crawl(&mut conn, &mut state) {
    e.write_diagnostic();
    unsafe { libc::exit(1) };
  }

  for i in 0..state.config.entry_len {
    let notif = &mut state.notifications[i];
    notif.id = conn.alloc_id();
    let mut msg = Message::new(
      state.global.idle_notifier_id,
      ext_idle_notifier_v1::request::GET_IDLE_NOTIFICATION,
    );
    msg.write_u32(notif.id);
    msg.write_u32(notif.entry.timeout_ms);
    msg.write_u32(state.global.seat_id);

    conn.send_logged(&msg, None);
  }

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
