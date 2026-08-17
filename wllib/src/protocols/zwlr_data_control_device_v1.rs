//! `zwlr_data_control_device_v1` — created via `zwlr_data_control_manager_v1::get_data_device`.
pub mod request {
  pub const SET_SELECTION: u16 = 0;
  pub const DESTROY: u16 = 1;
  pub const SET_PRIMARY_SELECTION: u16 = 2; // since v2
}

pub mod event {
  pub const DATA_OFFER: u16 = 0;
  // `selection`'s `id` arg is allow-null: 0 means the clipboard was cleared, not an error. The
  // first `selection` event fires immediately upon binding the device, so a fresh client learns
  // the current clipboard state (or its absence) without needing to request it.
  pub const SELECTION: u16 = 1;
  pub const FINISHED: u16 = 2;
  pub const PRIMARY_SELECTION: u16 = 3; // since v2, same allow-null behavior as SELECTION
}
