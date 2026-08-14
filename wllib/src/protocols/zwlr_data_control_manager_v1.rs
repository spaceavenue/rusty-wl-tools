//! `zwlr_data_control_manager_v1` — <https://wayland.app/protocols/wlr-data-control-unstable-v1>
pub mod request {
  pub const CREATE_DATA_SOURCE: u16 = 0;
  pub const GET_DATA_DEVICE: u16 = 1;
  pub const DESTROY: u16 = 2;
}
