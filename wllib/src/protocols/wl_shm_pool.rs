//! `wl_shm_pool` — created via `wl_shm::create_pool`.
pub mod request {
  pub const CREATE_BUFFER: u16 = 0;
  pub const DESTROY: u16 = 1;
  pub const RESIZE: u16 = 2;
}
