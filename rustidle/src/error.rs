use wllib::error::{SysError, WireError};
use wllib::io::write_stderr;

pub enum AppError {
  // A failure from the wayland transport/protocol layer.
  Wire(WireError),
  // A libc syscall outside the wayland socket failed (memfd_create, mmap, pipe2, fork, ...).
  Sys(SysError),
  // A `timeout <s> ...` config line parsed a value of 0 seconds.
  InvalidTimeout,
}

impl AppError {
  pub fn write_diagnostic(&self) {
    match self {
      AppError::Wire(e) => e.write_diagnostic(),
      AppError::Sys(e) => WireError::Sys(*e).write_diagnostic(),
      AppError::InvalidTimeout => write_stderr("Timeout must be at least 1s\n"),
    }
  }
}

impl From<WireError> for AppError {
  fn from(e: WireError) -> Self {
    AppError::Wire(e)
  }
}
