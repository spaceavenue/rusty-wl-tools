//! `wl_callback` — returned by `wl_display::sync` and `wl_surface::frame`. No requests; frozen at
//! version 1.
pub mod event {
    pub const DONE: u16 = 0;
}

pub const SYNC_CALLBACK_ID: u32 = 3;
