#![no_std]
#![no_main]

mod state;

use state::State;
use wllib::dispatch::dispatch_once;
use wllib::registry::crawl;
use wllib::transport::Connection;

#[link(name = "c", kind = "static")]
unsafe extern "C" {}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn main(_argc: isize, _argv: *const *mut libc::c_char) -> libc::c_int {
  let mut conn = match Connection::connect() {
    Ok(c) => c,
    Err(e) => {
      e.write_diagnostic();
      unsafe { libc::exit(1) };
    }
  };
  let mut state = State::init();
  if let Err(e) = crawl(&mut conn, &mut state) {
    e.write_diagnostic();
    unsafe { libc::exit(1) };
  }
  state.request_xdg_outputs(&mut conn);

  // dispatch until every bound output has reported its `done` event. exiting after the first
  // successful call could print anywhere from zero to all outputs depending on how the compositor
  // happened to batch its writes, so we loop through them.
  while state.done_count < state.output_len {
    if let Err(e) = dispatch_once(&mut conn, &mut state) {
      e.write_diagnostic();
      unsafe { libc::exit(1) };
    }
  }
  0
}
