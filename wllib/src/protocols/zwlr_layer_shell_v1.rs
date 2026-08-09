//! `zwlr_layer_shell_v1` — <https://wayland.app/protocols/wlr-layer-shell-unstable-v1>
pub mod request {
  pub const GET_LAYER_SURFACE: u16 = 0;
  pub const DESTROY: u16 = 1;
}

/// `zwlr_layer_shell_v1.layer` enum values.
pub mod layer {
  pub const BACKGROUND: u32 = 0;
  pub const BOTTOM: u32 = 1;
  pub const TOP: u32 = 2;
  pub const OVERLAY: u32 = 3;
}
