use wllib::error::{SysError, WireError};
use wllib::io;

pub enum AppError {
  Wire(WireError),
  Sys(SysError),
  InvalidTemp,
}

impl AppError {
  pub fn write_diagnostic(&self) {
    match self {
      AppError::Wire(e) => e.write_diagnostic(),
      AppError::Sys(e) => WireError::Sys(*e).write_diagnostic(),
      AppError::InvalidTemp => io::write_stderr("[rustemp] invalid temperature\n"),
    }
  }
}

impl From<WireError> for AppError {
  fn from(e: WireError) -> Self {
    AppError::Wire(e)
  }
}
