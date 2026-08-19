//! Named opcode constants for every Wayland interface this project speaks, split into
//! `request`/`event` sub-modules (they're separate numbering spaces per interface — mixing them
//! up was the single most common bug during initial development of this project).
//!
//! Every constant here was cross-checked against the upstream protocol XML rather than
//! transcribed from memory:
//! - core interfaces: <https://github.com/wayland-mirror/wayland> `protocol/wayland.xml`
//! - wlr-* extensions (wlr-layer-shell, wlr-gamma-control, wlr-data-control):
//!   <https://github.com/swaywm/wlr-protocols> `unstable/*.xml`
//! - ext-*/xdg-output staging protocols: <https://github.com/wayland-mirror/wayland-protocols>
//!   (`xdg_output` verified against this system's own
//!   `/usr/share/wayland-protocols/unstable/xdg-output/xdg-output-unstable-v1.xml`, which matches)
//!
//! Adding support for another interface (e.g. `xdg_shell`, `wl_seat`) means adding one small file
//! here, cross-checked the same way, rather than hand-writing opcodes at each call site.

pub mod ext_idle_notification_v1;
pub mod ext_idle_notifier_v1;
pub mod wl_callback;
pub mod wl_compositor;
pub mod wl_display;
pub mod wl_output;
pub mod wl_registry;
pub mod wl_seat;
pub mod wl_shm;
pub mod wl_shm_pool;
pub mod wl_surface;
pub mod zwlr_data_control_device_v1;
pub mod zwlr_data_control_manager_v1;
pub mod zwlr_data_control_offer_v1;
pub mod zwlr_data_control_source_v1;
pub mod zwlr_gamma_control_manager_v1;
pub mod zwlr_gamma_control_v1;
pub mod zwlr_layer_shell_v1;
pub mod zwlr_layer_surface_v1;
pub mod zxdg_output_manager_v1;
pub mod zxdg_output_v1;
