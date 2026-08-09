//! `wl_display` — the core global object, always object id 1. Frozen at version 1 forever.
pub mod request {
  pub const SYNC: u16 = 0;
  pub const GET_REGISTRY: u16 = 1;
}

pub mod event {
  pub const ERROR: u16 = 0;
  pub const DELETE_ID: u16 = 1;
}

pub const DISPLAY_ID: u32 = 1;
