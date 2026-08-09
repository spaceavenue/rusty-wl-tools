//! `zwlr_layer_surface_v1` — created via `zwlr_layer_shell_v1::get_layer_surface`.
pub mod request {
  pub const SET_SIZE: u16 = 0;
  pub const SET_ANCHOR: u16 = 1;
  pub const SET_EXCLUSIVE_ZONE: u16 = 2;
  pub const SET_MARGIN: u16 = 3;
  pub const SET_KEYBOARD_INTERACTIVITY: u16 = 4;
  pub const GET_POPUP: u16 = 5;
  pub const ACK_CONFIGURE: u16 = 6;
  pub const DESTROY: u16 = 7;
  pub const SET_LAYER: u16 = 8; // since v2
}

pub mod event {
  pub const CONFIGURE: u16 = 0;
  pub const CLOSED: u16 = 1;
}

/// `zwlr_layer_surface_v1.anchor` bitfield values.
pub mod anchor {
  pub const TOP: u32 = 1;
  pub const BOTTOM: u32 = 2;
  pub const LEFT: u32 = 4;
  pub const RIGHT: u32 = 8;
  pub const ALL: u32 = TOP | BOTTOM | LEFT | RIGHT;
}
