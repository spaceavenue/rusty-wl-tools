//! Formatting methods due to absence of core::fmt and heap allocations.
//! There are also convenience methods, such as for writing to file descriptors, and methods for
//! writing to stderr and stdout.

/// Lightweight formatter trait in absence of core::fmt.
pub trait FmtLite {
  fn format_into<const N: usize>(self, buf: &mut StringOnStack<N>);
}

impl FmtLite for &str {
  fn format_into<const N: usize>(self, buf: &mut StringOnStack<N>) {
    buf.push_str(self);
  }
}

impl FmtLite for char {
  fn format_into<const N: usize>(self, buf: &mut StringOnStack<N>) {
    buf.push_char(self);
  }
}

impl FmtLite for u32 {
  fn format_into<const N: usize>(self, buf: &mut StringOnStack<N>) {
    buf.push_u32(self);
  }
}

impl FmtLite for i32 {
  fn format_into<const N: usize>(self, buf: &mut StringOnStack<N>) {
    buf.push_i32(self);
  }
}

impl FmtLite for u64 {
  fn format_into<const N: usize>(self, buf: &mut StringOnStack<N>) {
    buf.push_u64(self);
  }
}

impl FmtLite for i64 {
  fn format_into<const N: usize>(self, buf: &mut StringOnStack<N>) {
    buf.push_i64(self);
  }
}

/// Allocate a fixed size string on the stack.
#[derive(Clone, Copy)]
pub struct StringOnStack<const N: usize> {
  buf: [u8; N],
  len: usize,
}
impl<const N: usize> StringOnStack<N> {
  /// Create an empty string.
  pub const fn new() -> Self {
    Self {
      buf: [0; N],
      len: 0,
    }
  }

  /// Returns the stored string as bytes.
  pub const fn capacity(&self) -> usize {
    N.saturating_sub(1)
  }

  /// Returns remaining capacity in bytes.
  pub const fn remaining_capacity(&self) -> usize {
    self.capacity().saturating_sub(self.len)
  }

  /// Current length in bytes.
  pub const fn len(&self) -> usize {
    self.len
  }

  /// Returns true if empty.
  pub const fn is_empty(&self) -> bool {
    self.len == 0
  }

  /// Clear the string.
  pub fn clear(&mut self) {
    self.len = 0;
    if N > 0 {
      self.buf[0] = 0;
    }
  }

  /// View as byte slice.
  pub fn as_bytes(&self) -> &[u8] {
    &self.buf[..self.len]
  }

  /// View as a string slice.
  pub fn as_str(&self) -> &str {
    // SAFETY: All push methods enforce UTF-8 validity.
    unsafe { str::from_utf8_unchecked(&self.buf[..self.len]) }
  }

  /// View as a mutable string slice.
  pub fn as_mut_str(&mut self) -> &mut str {
    // SAFETY: All push methods enforce UTF-8 validity.
    unsafe { str::from_utf8_unchecked_mut(&mut self.buf[..self.len]) }
  }

  /// Return a raw pointer to the C string representation.
  /// Guaranteed to be NUL-terminated at all times.
  pub fn as_ptr(&self) -> *const libc::c_char {
    self.buf.as_ptr() as *const libc::c_char
  }

  /// View as a borrowed CStr. Always safe and O(1) due to the trailing NUL invariant.
  pub fn as_c_str(&self) -> &core::ffi::CStr {
    if N == 0 {
      c""
    } else {
      // SAFETY: self.buf is of length N and self.buf[self.len] is guaranteed to be 0.
      unsafe { core::ffi::CStr::from_bytes_with_nul_unchecked(&self.buf[..=self.len]) }
    }
  }

  /// Push a string slice, silently truncating at character boundaries if capacity is exceeded.
  pub fn push_str(&mut self, s: &str) -> &mut Self {
    let space = self.remaining_capacity();
    let n = if space >= s.len() {
      s.len()
    } else if space > 0 {
      // Find the last valid UTF-8 character boundary that fits
      let mut valid_len = space;
      while !s.is_char_boundary(valid_len) {
        valid_len -= 1;
      }
      valid_len
    } else {
      0
    };

    if n > 0 {
      self.buf[self.len..self.len + n].copy_from_slice(&s.as_bytes()[..n]);
      self.len += n;
      if self.len < N {
        self.buf[self.len] = 0; // Maintain NUL invariant
      }
    }

    self
  }

  /// Push a single character.
  pub fn push_char(&mut self, c: char) -> &mut Self {
    let mut encode_buf = [0u8; 4];
    let s = c.encode_utf8(&mut encode_buf);
    self.push_str(s)
  }

  /// Push an unsigned 64-bit integer formatted in base 10.
  pub fn push_u64(&mut self, mut num: u64) -> &mut Self {
    if num == 0 {
      return self.push_str("0");
    }
    let mut tmp = [0u8; 20]; // u64::MAX is 20 decimal digits
    let mut idx = 20;
    while num > 0 {
      idx -= 1;
      tmp[idx] = b'0' + (num % 10) as u8;
      num /= 10;
    }
    // SAFETY: tmp contains only ASCII digits '0'-'9'
    let s = unsafe { str::from_utf8_unchecked(&tmp[idx..]) };
    self.push_str(s)
  }

  /// Push a signed 64-bit integer formatted in base 10.
  pub fn push_i64(&mut self, num: i64) -> &mut Self {
    if num < 0 {
      self.push_str("-");
      self.push_u64(num.unsigned_abs())
    } else {
      self.push_u64(num as u64)
    }
  }

  /// Push an unsigned 32-bit integer formatted in base 10.
  pub fn push_u32(&mut self, num: u32) -> &mut Self {
    self.push_u64(num as u64)
  }

  /// Push a signed 32-bit integer formatted in base 10.
  pub fn push_i32(&mut self, num: i32) -> &mut Self {
    self.push_i64(num as i64)
  }

  /// Push anything that implements `FmtLite` (currently: &str, char, u32, i32)
  pub fn push<T: FmtLite>(&mut self, value: T) -> &mut Self {
    value.format_into(self);
    self
  }
}

impl<const N: usize> Default for StringOnStack<N> {
  fn default() -> Self {
    Self::new()
  }
}

impl<const N: usize> AsRef<[u8]> for StringOnStack<N> {
  fn as_ref(&self) -> &[u8] {
    self.as_bytes()
  }
}

/// Writes the buffer to the file descriptor.
pub fn write_fd(fd: libc::c_int, msg: impl AsRef<[u8]>) -> Result<(), crate::error::SysError> {
  let mut msg = msg.as_ref();
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
pub fn write_stderr(msg: impl AsRef<[u8]>) {
  let _ = write_fd(2, msg);
}

/// Writes the buffer to stdout.
pub fn write_stdout(msg: impl AsRef<[u8]>) {
  let _ = write_fd(1, msg);
}
