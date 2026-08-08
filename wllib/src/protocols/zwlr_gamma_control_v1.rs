//! `zwlr_gamma_control_v1` — created via `zwlr_gamma_control_manager_v1::get_gamma_control`.
pub mod request {
    pub const SET_GAMMA: u16 = 0;
    pub const DESTROY: u16 = 1;
}

pub mod event {
    pub const GAMMA_SIZE: u16 = 0;
    pub const FAILED: u16 = 1;
}
