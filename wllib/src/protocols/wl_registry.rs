//! `wl_registry` — created via `wl_display::get_registry`.
pub mod request {
    pub const BIND: u16 = 0;
}

pub mod event {
    pub const GLOBAL: u16 = 0;
    pub const GLOBAL_REMOVE: u16 = 1;
}

pub const REGISTRY_ID: u32 = 2;
