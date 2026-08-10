use wllib::error::{SysError, WireError};
use wllib::fmt_lite;

// Application-level errors.
//
// Wraps [`wlcore::WireError`] for anything related to the wayland connection/protocol. Each
// variant carries enough context to render a real diagnostic via `write_diagnostic`.
pub enum AppError {
  // A failure from the wayland transport/protocol layer.
  Wire(WireError),
  // A libc syscall outside the wayland socket failed.
  Sys(SysError),
  // dump-bgra produced fewer bytes than the expected `width * height * 4`.
  ImageDecodeError,
  // Attempted an operation that needs `config.image_path` before one was set.
  MissingImagePath,
}

impl AppError {
  pub fn write_diagnostic(&self) {
    match self {
      AppError::Wire(e) => e.write_diagnostic(),
      AppError::Sys(e) => WireError::Sys(*e).write_diagnostic(),
      AppError::ImageDecodeError => fmt_lite::write_stderr(
        b"[rustbg] image decode error: dump-bgra produced fewer bytes than expected\n",
      ),
      AppError::MissingImagePath => fmt_lite::write_stderr(b"[rustbg] no image path configured\n"),
    }
  }
}

impl From<WireError> for AppError {
  fn from(e: WireError) -> Self {
    AppError::Wire(e)
  }
}
