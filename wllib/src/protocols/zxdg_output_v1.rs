//! `zxdg_output_v1`.
pub mod request {
  pub const DESTROY: u16 = 0;
}

pub mod event {
  pub const LOGICAL_POSITION: u16 = 0;
  pub const LOGICAL_SIZE: u16 = 1;
  pub const DONE: u16 = 2; // deprecated since v3, wl_output.done is sent instead
  pub const NAME: u16 = 3; // since v2, deprecated in favor of wl_output.name
  pub const DESCRIPTION: u16 = 4; // since v2, deprecated in favor of wl_output.description
}
