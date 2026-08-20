use wllib::error::{SysError, WireError};
use wllib::fmt_lite::write_stderr;

pub enum AppError {
  Wire(WireError),
  Sys(SysError),
  // no offer/selection currently available when data was requested
  NothingCopied,
  // the mime type requested with -t isn't offered by the current selection
  MimeNotAvailable,
}

impl AppError {
  pub fn write_diagnostic(&self) {
    match self {
      AppError::Wire(e) => e.write_diagnostic(),
      AppError::Sys(e) => WireError::Sys(*e).write_diagnostic(),
      AppError::NothingCopied => write_stderr(b"rustclip: nothing is currently copied\n"),
      AppError::MimeNotAvailable => {
        write_stderr("rustclip: requested mime type not offered by the current selection\n");
      }
    }
  }
}

impl From<WireError> for AppError {
  fn from(e: WireError) -> Self {
    AppError::Wire(e)
  }
}
