//! `wl_shm`
pub mod request {
  pub const CREATE_POOL: u16 = 0;
  pub const RELEASE: u16 = 1; // since v2
}

pub mod event {
  pub const FORMAT: u16 = 0;
}

/// The `wl_shm.format` enum values this project cares about.
pub mod format {
  pub const ARGB8888: u32 = 0;
  pub const XRGB8888: u32 = 1;
}
