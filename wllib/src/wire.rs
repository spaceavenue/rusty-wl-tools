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
pub fn read_i32(buf: &[u8], idx: usize) -> i32 {
    read_u32(buf, idx) as i32
}

/// Read a `u16` argument at `idx`.
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
/// caller can find the next argument at `idx + consumed`. Returns `None` if the buffer doesn't
/// hold the full string.
pub fn read_string(buf: &[u8], idx: usize) -> Option<(&[u8], usize)> {
    let str_len = read_u32(buf, idx) as usize; // includes NUL terminator
    if str_len == 0 {
        return Some((&buf[0..0], 4));
    }
    let start = idx + 4;
    if start + str_len > buf.len() {
        return None;
    }
    let content = &buf[start..start + str_len - 1]; // exclude NUL
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
/// or if `size` would run past the end of `buf`.
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

/// Outgoing wire message with a fixed 256-byte capacity. Writes past capacity are silently dropped
/// rather than panicking.
pub struct Message {
    header: MessageHeader,
    data: [u8; 256],
    // len: usize,
}

impl Message {
    /// Create a new message targeting `obj_id` with request/event `opcode`
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

    /// Write a `string` argument from a byte slice. Accepts either a NUL-terminated or bare byte
    /// slice.
    pub fn write_string(&mut self, s: &[u8]) {
        let mut s_len = s.len() as u32;
        if s.last() != Some(&0) {
            s_len += 1;
        }
        self.write_u32(s_len);
        let actual_len = if s.last() == Some(&0) {
            s.len() - 1
        } else {
            s.len()
        } as u16;
        if self.header.size + actual_len < self.data.len() as u16 {
            self.data[self.header.size as usize..(self.header.size + actual_len) as usize]
                .copy_from_slice(&s[..actual_len as usize]);
            self.data[(self.header.size + actual_len) as usize] = 0;
            self.header.size += actual_len + 1;
            self.header.size = (self.header.size + 3) & !3; // pad to 4-byte boundary
            self.sync_size();
        }
    }

    /// Write a `string` argument from a raw NUL-terminated C string pointer (e.g. `optarg`, or a
    /// `c"..."` literal).
    pub fn write_cstr(&mut self, s: *const libc::c_char) {
        unsafe {
            let mut len = 0u16;
            while *s.add(len as usize) != 0 {
                len += 1;
            }
            self.write_u32((len + 1) as u32);
            if self.header.size + len < self.data.len() as u16 {
                core::ptr::copy_nonoverlapping(
                    s as *const u8,
                    self.data.as_mut_ptr().add(self.header.size as usize),
                    len as usize,
                );
                self.data[(self.header.size + len) as usize] = 0;
                self.header.size += len + 1;
                self.header.size = (self.header.size + 3) & !3;
                self.sync_size();
            }
        }
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.data[..self.header.size as usize]
    }
}
