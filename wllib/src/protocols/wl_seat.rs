//! `wl_seat`: advertises available input devices.
pub mod request {
  pub const GET_POINTER: u16 = 0;
  pub const GET_KEYBOARD: u16 = 1;
  pub const GET_TOUCH: u16 = 2;
  pub const RELEASE: u16 = 3; // since v5
}

pub mod event {
  pub const CAPABILITIES: u16 = 0;
  pub const NAME: u16 = 1; // since v2
}
