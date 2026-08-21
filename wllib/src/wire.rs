//! Wire-format encoding/decoding for the Wayland protocol.
//!
//! Message format:
//!     4-byte sender/target object id
//!     2-byte opcode
//!     2-byte message size, including 8-byte header
//!     argument data, padded to 4-byte alignment
//! All integers are native-endian.

/// Read a `u32` argument at `idx`. Returns 0 if `idx` is out of range rather than panicking, since
/// wire data is untrusted input and this crate has no allocator to build a proper error path around
/// every 4-byte read.
#[must_use]
pub fn read_u32(buf: &[u8], idx: usize) -> u32 {
  if idx + 4 <= buf.len() {
    let mut bytes = [0u8; 4];
    bytes.copy_from_slice(&buf[idx..idx + 4]);
    u32::from_ne_bytes(bytes)
  } else {
    0
  }
}

/// Reads a u32 and returns it as an i32.
#[must_use]
pub fn read_i32(buf: &[u8], idx: usize) -> i32 {
  read_u32(buf, idx) as i32
}

/// Read a `u16` argument at `idx`.
#[must_use]
pub fn read_u16(buf: &[u8], idx: usize) -> u16 {
  if idx + 2 <= buf.len() {
    let mut bytes = [0u8; 2];
    bytes.copy_from_slice(&buf[idx..idx + 2]);
    u16::from_ne_bytes(bytes)
  } else {
    0
  }
}

/// Read a wire `string` argument at `idx`: a `u32` length prefix (including the NUL terminator),
/// the string bytes, then padding to a 4-byte boundary.
///
/// Returns `(content, consumed)` where `content` excludes the NUL terminator and `consumed` is
/// the total number of bytes occupied by this argument (length prefix + padded string), so the
/// caller can find the next argument at `idx + consumed`. Returns `None` if the string lenfth is 0
/// or buffer doesn't hold the full string.
#[must_use]
pub fn read_str(buf: &[u8], idx: usize) -> Option<(&str, usize)> {
  let str_len = read_u32(buf, idx) as usize; // includes NUL terminator
  if str_len == 0 {
    return None;
  }
  let start = idx + 4;
  if start + str_len > buf.len() {
    return None;
  }
  let content = core::str::from_utf8(&buf[start..start + str_len - 1]).ok()?;
  let padded = (str_len + 3) & !3;
  Some((content, 4 + padded))
}

/// 8-byte message header.
#[derive(Clone, Copy)]
pub struct MessageHeader {
  pub sender: u32,
  pub opcode: u16,
  pub size: u16,
}

impl MessageHeader {
  #[must_use]
  pub fn new(sender: u32, opcode: u16, size: u16) -> Self {
    Self {
      sender,
      opcode,
      size,
    }
  }
}

/// Parse message header at `idx`.
///
/// Returns `None` if header doesn't fit, if `size` is smaller than header (malformed message),
/// or if `size` would run past the end of `buf`. That last case doesn't distinguish "this is a
/// malformed message" from "this message just hasn't fully arrived yet", which is exactly why
/// callers walking a live socket use [`complete_message_len`] rather than calling this directly in
/// a loop and treating `None` as "done for this batch": a `None` here can mean "stop, nothing more
/// to parse" *or* "stop, and remember these trailing bytes for next time," and only the caller
/// knows which.
#[must_use]
pub fn parse_header(buf: &[u8], idx: usize) -> Option<MessageHeader> {
  if idx + 8 > buf.len() {
    return None;
  }
  let sender = read_u32(buf, idx);
  let opcode = read_u16(buf, idx + 4);
  let size = read_u16(buf, idx + 6);
  if size < 8 || idx + (size as usize) > buf.len() {
    return None;
  }
  Some(MessageHeader::new(sender, opcode, size))
}

/// Given `total` valid bytes at the front of `buf`, return how many of them form zero or more
/// *complete* wire messages laid back-to-back from the start. `total - result` is whatever's left
/// over: either 0 (everything parsed cleanly) or the leading bytes of one more message that
/// hasn't fully arrived over the wire yet.
///
/// This exists as its own pure function for two reasons: it's the one piece of that carry-over
/// logic that doesn't touch a real socket, so it can be unit tested directly; and it's genuinely
/// just `parse_header` applied repeatedly, which belongs next to `parse_header` rather than off in
/// the transport layer.
#[must_use]
pub fn complete_message_len(buf: &[u8], total: usize) -> usize {
  let mut end = 0;
  // `parse_header` bounds itself to `buf[..total]` on every call (not the caller-supplied `buf`'s
  // full length) specifically so any stale bytes sitting past `total` never get misread as part of
  // this batch.
  while let Some(header) = parse_header(&buf[..total], end) {
    end += header.size as usize;
  }
  end
}

/// Outgoing wire message with a fixed 256-byte capacity. Writes past capacity are silently dropped
/// rather than panicking.
pub struct Message {
  header: MessageHeader,
  data: [u8; 256],
}

impl Message {
  /// Create a new message targeting `obj_id` with request/event `opcode`
  #[must_use]
  pub fn new(obj_id: u32, opcode: u16) -> Self {
    let header = MessageHeader::new(obj_id, opcode, 8);
    let mut msg = Self {
      header,
      data: [0; 256],
    };
    msg.data[0..4].copy_from_slice(&obj_id.to_ne_bytes());
    msg.data[4..6].copy_from_slice(&opcode.to_ne_bytes());
    msg.sync_size();
    msg
  }

  fn sync_size(&mut self) {
    self.data[6..8].copy_from_slice(&self.header.size.to_ne_bytes());
  }

  pub fn write_u32(&mut self, val: u32) {
    if self.header.size + 4 <= self.data.len() as u16 {
      self.data[self.header.size as usize..self.header.size as usize + 4]
        .copy_from_slice(&val.to_ne_bytes());
      self.header.size += 4;
      self.sync_size();
    }
  }

  pub fn write_i32(&mut self, val: i32) {
    self.write_u32(val as u32);
  }

  /// Write a `string` argument from a `&str`.
  ///
  /// Wire format (see the module doc comment): a `u32` length prefix that counts the trailing NUL
  /// as part of the string's length, then that many content bytes including the NUL, then padding
  /// out to a 4-byte boundary. `s` (a `&str`) doesn't carry its own NUL, so the `+1` on `s_len`
  /// accounts for the one this method adds itself just below.
  pub fn write_str(&mut self, s: &str) {
    let s_len = (s.len() + 1) as u32;

    self.write_u32(s_len);

    let actual_len = s.len();
    let start = self.header.size as usize;
    // `<` rather than `<=`: `actual_len` bytes of content need one more byte after them for the
    // NUL this writes at `start + actual_len`, so the check must leave room for content *and*
    // that terminator, not just the content.
    if self.header.size + (actual_len as u16) < self.data.len() as u16 {
      self.data[start..start + actual_len].copy_from_slice(s.as_bytes());
      self.data[start + actual_len] = 0;
      // `+ 1` here for the NUL just written.
      self.header.size += actual_len as u16 + 1;
      // pad to 4-byte boundary.
      self.header.size = (self.header.size + 3) & !3;
      self.sync_size();
    }
  }

  /// Write a `string` argument from a raw NUL-terminated C string pointer (e.g. `optarg`, or a
  /// `c"..."` literal).
  pub fn write_cstr(&mut self, s: &core::ffi::CStr) {
    // unlike `write_str`, `to_bytes_with_nul()` already includes the terminator in both the slice
    // and its length. so, nothing here needs a manual `+1` anywhere. `len` is already the full
    // on-wire byte count for the length prefix.
    let bytes = s.to_bytes_with_nul();
    let len = bytes.len() as u16;

    self.write_u32(u32::from(len));

    // `<` here has the same room-for-everything reasoning as `write_str`'s check, except `len`
    // already covers the NUL (see above), so there's no separate `+1` needed on this side either.
    if self.header.size + len < self.data.len() as u16 {
      self.data[self.header.size as usize..(self.header.size + len) as usize]
        .copy_from_slice(bytes);
      self.header.size += len;
      // pad to 4-byte boundary, same as write_str.
      self.header.size = (self.header.size + 3) & !3;
      self.sync_size();
    }
  }

  #[must_use]
  pub fn as_bytes(&self) -> &[u8] {
    &self.data[..self.header.size as usize]
  }
}
