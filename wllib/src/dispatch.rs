use crate::error::{PROTOCOL_MESSAGE_CAP, ProtocolError, WireError};
use crate::fmt_lite::StringOnStack;
use crate::protocols::wl_display::DISPLAY_ID;
use crate::transport::Connection;
use crate::wire::{parse_header, read_string, read_u32};

/// Implement to receive protocol events once the registry crawl is done.
///
/// `data` is the bytes immediately following the 8-byte header, up to the next message. Use
/// [`crate::wire::read_u32`] etc. with offsets relative to the start of `data` to read.
pub trait EventHandler {
  fn handle_event(&mut self, conn: &mut Connection, sender: u32, opcode: u16, data: &[u8]);
}

/// Block on a `recv()` call and dispatch every complete message found in it to `handler`.
pub fn dispatch_once<H: EventHandler>(
  conn: &mut Connection,
  handler: &mut H,
) -> Result<(), WireError> {
  let mut buf = [0u8; 4096];
  let data = conn.recv(&mut buf)?;
  let mut idx = 0;
  while let Some(header) = parse_header(data, idx) {
    if header.sender == DISPLAY_ID && header.opcode == crate::protocols::wl_display::event::ERROR {
      let object_id = read_u32(data, idx + 8);
      let code = read_u32(data, idx + 12);
      let mut message: StringOnStack<PROTOCOL_MESSAGE_CAP> = StringOnStack::new();
      if let Some((text, _)) = read_string(data, idx + 16) {
        message.push_bytes(text);
      }
      return Err(WireError::Protocol(ProtocolError {
        object_id,
        code,
        message,
      }));
    }
    let event_data = &data[idx + 8..idx + header.size as usize];
    handler.handle_event(conn, header.sender, header.opcode, event_data);
    idx += header.size as usize;
  }
  Ok(())
}
