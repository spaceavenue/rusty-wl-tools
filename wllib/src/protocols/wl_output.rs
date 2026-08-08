//! `wl_output`. This project only binds it to learn `output_id`s and doesn't currently use any of
//! its own events, but the constants are here for when that changes (e.g. reacting to
//! hotplug/mode-change events).
pub mod request {
    pub const RELEASE: u16 = 0; // since v3
}

pub mod event {
    pub const GEOMETRY: u16 = 0;
    pub const MODE: u16 = 1;
    pub const DONE: u16 = 2; // since v2
    pub const SCALE: u16 = 3; // since v2
    pub const NAME: u16 = 4; // since v4
    pub const DESCRIPTION: u16 = 5; // since v4
}
