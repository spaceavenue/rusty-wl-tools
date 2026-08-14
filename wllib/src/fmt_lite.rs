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

  pub fn as_bytes(&self) -> &[u8] {
    &self.buf[..self.len]
  }

  pub fn as_ptr(&self) -> *const u8 {
    self.buf.as_ptr()
  }

  pub fn len(&self) -> usize {
    self.len
  }

  pub fn is_empty(&self) -> bool {
    self.len == 0
  }

  pub fn clear(&mut self) {
    self.len = 0;
  }

  /// Push arbitrary bytes onto the string.
  pub fn push_bytes(&mut self, bytes: &[u8]) {
    let space = N.saturating_sub(self.len);
    let n = bytes.len().min(space);
    self.buf[self.len..self.len + n].copy_from_slice(&bytes[..n]);
    self.len += n;
  }

  /// Push a string literal onto the string.
  pub fn push_str(&mut self, s: &str) {
    self.push_bytes(s.as_bytes());
  }

  /// Push a u32 onto the string. At least 10 bytes are allocated, since u32::MAX is 10 digits long.
  pub fn push_u32(&mut self, num: u32) {
    let mut tmp = [0u8; 10];
    let n = u32_to_decimal(num, &mut tmp);
    self.push_bytes(&tmp[..n]);
  }

  /// Push an i32 onto the string, with the negative sign.
  pub fn push_i32(&mut self, num: i32) {
    if num < 0 {
      self.push_bytes(b"-");
      self.push_u32(num.unsigned_abs());
    } else {
      self.push_u32(num as u32);
    }
  }

  /// Null terminates the string.
  pub fn null_terminate(&mut self) {
    if self.len < N {
      self.buf[self.len] = 0;
    }
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
pub fn write_fd(fd: libc::c_int, msg: &[u8]) {
  unsafe {
    libc::write(fd, msg.as_ptr() as *const libc::c_void, msg.len());
  }
}

/// Writes the buffer to stderr.
pub fn write_stderr(msg: &[u8]) {
  write_fd(2, msg);
}

/// Writes the buffer to stdout.
pub fn write_stdout(msg: &[u8]) {
  write_fd(1, msg);
}
