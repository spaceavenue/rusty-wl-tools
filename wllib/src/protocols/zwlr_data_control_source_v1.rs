//! `zwlr_data_control_source_v1` — created via `zwlr_data_control_manager_v1::create_data_source`.
//! This is the *offering* side (used by `wl-copy`): after `offer`-ing every mime type it supports
//! and being installed via `set_selection`, it receives a `send` event carrying a file descriptor
//! as ancillary data every time some other client asks for the data.
pub mod request {
  pub const OFFER: u16 = 0;
  pub const DESTROY: u16 = 1;
}

pub mod event {
  pub const SEND: u16 = 0;
  pub const CANCELLED: u16 = 1;
}
