#![no_std]
#![no_main]

pub mod config;
pub mod error;
pub mod state;

use wllib::dispatch::dispatch_once;
use wllib::error::WireError::ConnectionClosed;
use wllib::fmt_lite::{write_stderr, write_stdout};
use wllib::protocols::ext_idle_notifier_v1;
use wllib::registry::crawl;
use wllib::transport::Connection;
use wllib::wire::Message;

use crate::config::Config;
use crate::state::State;

#[link(name = "c", kind = "static")]
unsafe extern "C" {}

// Global flag to track the suspended state
static IS_SUSPENDED: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

extern "C" fn handle_sigusr1(_sig: libc::c_int) {
  // fetch_xor with true inverts the current boolean value
  IS_SUSPENDED.fetch_xor(true, core::sync::atomic::Ordering::Relaxed);
}

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

  // tells kernel to reap child processes automatically, avoiding watipid
  unsafe { libc::signal(libc::SIGCHLD, libc::SIG_IGN) };
  // handle SIGUSR1
  unsafe { libc::signal(libc::SIGUSR1, handle_sigusr1 as libc::sighandler_t) };

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
    while IS_SUSPENDED.load(core::sync::atomic::Ordering::Relaxed) {
      unsafe { libc::pause() };
    }
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
