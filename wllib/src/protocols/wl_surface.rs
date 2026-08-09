//! `wl_surface`
pub mod request {
  pub const DESTROY: u16 = 0;
  pub const ATTACH: u16 = 1;
  pub const DAMAGE: u16 = 2;
  pub const FRAME: u16 = 3;
  pub const SET_OPAQUE_REGION: u16 = 4;
  pub const SET_INPUT_REGION: u16 = 5;
  pub const COMMIT: u16 = 6;
  pub const SET_BUFFER_TRANSFORM: u16 = 7; // since v2
  pub const SET_BUFFER_SCALE: u16 = 8; // since v3
  pub const DAMAGE_BUFFER: u16 = 9; // since v4
  pub const OFFSET: u16 = 10; // since v5
}

pub mod event {
  pub const ENTER: u16 = 0;
  pub const LEAVE: u16 = 1;
  pub const PREFERRED_BUFFER_SCALE: u16 = 2; // since v6
  pub const PREFERRED_BUFFER_TRANSFORM: u16 = 3; // since v6
}
