//! Formatting methods due to absence of core::fmt and heap allocations.
//! There are also convenience methods, such as for writing to file descriptors, and methods for
//! writing to stderr and stdout.

/// Allocate a fixed size string on the stack.
#[derive(Clone, Copy)]
pub struct StringOnStack<const N: usize> {
  buf: [u8; N],
  len: usize,
}
impl<const N: usize> StringOnStack<N> {
  pub fn new() -> Self {
    StringOnStack {
      buf: [0; N],
      len: 0,
    }
  }

  /// Returns the stored string as bytes.
  pub fn as_bytes(&self) -> &[u8] {
    &self.buf[..self.len]
  }

  /// Returns the string as a string slice.
  pub fn as_str(&self) -> &str {
    core::str::from_utf8(self.as_bytes()).unwrap_or_default()
  }

  /// Returns the string as a c_str. Returns an empty c_str if not null_terminated.
  pub fn as_cstr(&self) -> &core::ffi::CStr {
    if self.len < N && self.buf[self.len] == 0 {
      core::ffi::CStr::from_bytes_with_nul(&self.buf[..=self.len]).unwrap_or(c"")
    } else {
      c""
    }
  }

  /// Returns a pointer to the string's data.
  pub fn as_ptr(&self) -> *const u8 {
    self.buf.as_ptr()
  }

  /// Returns the length of the strings.
  pub fn len(&self) -> usize {
    self.len
  }

  /// Check if the string is empty.
  pub fn is_empty(&self) -> bool {
    self.len == 0
  }

  /// Clears the string.
  pub fn clear(&mut self) {
    self.len = 0;
  }

  /// Push arbitrary bytes onto the string.
  pub fn push_bytes(&mut self, bytes: &[u8]) -> &mut Self {
    let n = bytes.len().min(N.saturating_sub(self.len));
    self.buf[self.len..self.len + n].copy_from_slice(&bytes[..n]);
    self.len += n;
    self
  }

  /// Push a string literal onto the string.
  pub fn push_str(&mut self, s: &str) -> &mut Self {
    self.push_bytes(s.as_bytes());
    self
  }

  /// Push a u32 onto the string. At least 10 bytes are allocated, since u32::MAX is 10 digits long.
  pub fn push_u32(&mut self, num: u32) -> &mut Self {
    let mut tmp = [0u8; 10];
    let n = u32_to_decimal(num, &mut tmp);
    self.push_bytes(&tmp[..n]);
    self
  }

  /// Push an i32 onto the string, with the negative sign.
  pub fn push_i32(&mut self, num: i32) -> &mut Self {
    if num < 0 {
      self.push_bytes(b"-");
      self.push_u32(num.unsigned_abs())
    } else {
      self.push_u32(num as u32)
    }
  }

  /// Null terminates the string.
  /// Note: For a full string (string with length `N`), this will overwrite the last byte.
  pub fn null_terminate(&mut self) -> &mut Self {
    if self.len < N {
      self.buf[self.len] = 0;
    }
    self
  }
}

impl<const N: usize> Default for StringOnStack<N> {
  fn default() -> Self {
    Self::new()
  }
}

/// Converts a u32 to it's decimal representation, stores it in the given byte slice, and returns
/// the bytes written. The array must be at least 10 bytes long, since a u32::MAX is 10 digits long.
pub fn u32_to_decimal(mut num: u32, buf: &mut [u8]) -> usize {
  if num == 0 {
    if !buf.is_empty() {
      buf[0] = b'0';
      return 1;
    }
    return 0;
  }
  let mut tmp = [0u8; 10];
  let mut idx = 10;
  while num > 0 {
    idx -= 1;
    tmp[idx] = b'0' + (num % 10) as u8;
    num /= 10;
  }
  let n = (10 - idx).min(buf.len());
  buf[..n].copy_from_slice(&tmp[idx..idx + n]);
  n
}

/// Writes the buffer to the file descriptor.
pub fn write_fd(fd: libc::c_int, mut msg: &[u8]) -> Result<(), crate::error::SysError> {
  while !msg.is_empty() {
    let res = unsafe { libc::write(fd, msg.as_ptr() as *const libc::c_void, msg.len()) };
    if res > 0 {
      msg = &msg[res as usize..];
    } else if res < 0 {
      let err = crate::error::SysError::last("write");
      if err.errno == libc::EINTR {
        continue;
      }
      return Err(err);
    } else {
      break;
    }
  }
  Ok(())
}

/// Writes the buffer to stderr.
pub fn write_stderr(msg: &[u8]) {
  let _ = write_fd(2, msg);
}

/// Writes the buffer to stdout.
pub fn write_stdout(msg: &[u8]) {
  let _ = write_fd(1, msg);
}
