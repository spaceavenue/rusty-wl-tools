//! `zwlr_data_control_offer_v1` — announced via `zwlr_data_control_device_v1::data_offer`.
//! This is the *requesting* side (used by `wl-paste`/`wl-watch`): it receives one `offer` event per
//! mime type the current selection supports, then `receive` is used to ask for the bytes for one
//! of them. The requester creates a pipe locally and sends the *write* end as an argument,
//! no fd reception needed on this side.
pub mod request {
  pub const RECEIVE: u16 = 0;
  pub const DESTROY: u16 = 1;
}

pub mod event {
  pub const OFFER: u16 = 0;
}
