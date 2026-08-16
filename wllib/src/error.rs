//! Error types for the transport/protocol layer.

use crate::fmt_lite::{StringOnStack, write_stderr};
use crate::wire::{read_str, read_u32};

/// A failed libc call, captured with `errno` at the point of failure.
///
/// `call` is a fixed `&'static str` naming the syscall (e.g. `"mmap"`).
#[derive(Clone, Copy)]
pub struct SysError {
  pub call: &'static str,
  pub errno: i32,
}
impl SysError {
  /// Capture the current `errno`, tagged with the name of the call that just failed. Called
  /// immediately after the failing libc call, before any other libc calls can clobber errno.
  pub fn last(call: &'static str) -> Self {
    let errno = unsafe { *libc::__errno_location() };
    Self { call, errno }
  }
}

/// Max length of a `wl_display::error` message body we'll keep for diagnostics. Longer messages
/// are truncated rather than allocated.
pub const PROTOCOL_MESSAGE_CAP: usize = 128;

/// A parsed `wl_display::error` event: which object misbehaved, the protocol-defined error code,
/// and the compositor's human-readable explanation.
#[derive(Clone, Copy)]
pub struct ProtocolError {
  pub object_id: u32,
  pub code: u32,
  pub message: StringOnStack<PROTOCOL_MESSAGE_CAP>,
}
impl ProtocolError {
  pub fn from(buf: &[u8], idx: usize) -> Self {
    let object_id = read_u32(buf, idx + 8);
    let code = read_u32(buf, idx + 12);
    let mut message: StringOnStack<PROTOCOL_MESSAGE_CAP> = StringOnStack::new();
    if let Some((text, _)) = read_str(buf, idx + 16) {
      message.push_str(text);
    }
    Self {
      object_id,
      code,
      message,
    }
  }
}

/// Errors that can occur in the transport/protocol layer.
#[derive(Clone, Copy)]
pub enum WireError {
  /// A libc syscall used by the transport failed.
  Sys(SysError),
  /// The compositor sent a `wl_display::error` event, a fatal protocol violation. The
  /// connection should be treated as dead once this is observed.
  Protocol(ProtocolError),
  /// `WAYLAND_DISPLAY` or `XDG_RUNTIME_DIR` wasn't set (or didn't fit the socket path buffer).
  Environment,
  /// The compositor closed the connection (`recv` returned EOF).
  ConnectionClosed,
}
impl WireError {
  /// Print a one-line, human-readable diagnostic to stderr.
  pub fn write_diagnostic(&self) {
    match self {
      WireError::Sys(e) => {
        let mut s = StringOnStack::<PROTOCOL_MESSAGE_CAP>::new();
        s.push_str("[wllib] syscall failed: ")
          .push_str(e.call)
          .push_str(" (errno ")
          .push_i32(e.errno)
          .push_str(")");
        write_stderr(s.as_bytes());
        write_stderr(b"\n");
      }
      WireError::Protocol(p) => {
        let mut s = StringOnStack::<PROTOCOL_MESSAGE_CAP>::new();
        s.push_str("[wllib] wayland protocol error: object ")
          .push_u32(p.object_id)
          .push_str(", code ")
          .push_u32(p.code)
          .push_str(", message: ")
          .push_str(p.message.as_str());
        write_stderr(s.as_bytes());
        write_stderr(b"\n");
      }
      WireError::Environment => {
        write_stderr(b"[wllib] WAYLAND_DISPLAY or XDG_RUNTIME_DIR not set\n");
      }
      WireError::ConnectionClosed => {
        write_stderr(b"[wllib] connection closed by compositor\n");
      }
    }
  }
}
