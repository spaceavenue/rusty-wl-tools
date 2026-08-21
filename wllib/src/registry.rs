use crate::dispatch::EventHandler;
use crate::error::{ProtocolError, WireError};
use crate::protocols::wl_callback::SYNC_CALLBACK_ID;
use crate::protocols::wl_display::{self, DISPLAY_ID};
use crate::protocols::wl_registry::{self, REGISTRY_ID};
use crate::transport::Connection;
use crate::wire::{Message, parse_header, read_str, read_u32};

/// Implement to receive each advertised global during [`crawl`]. For e.g., match on `interface` and
/// for the ones you want, call `conn.alloc_id()` and [`bind`] to claim it.
pub trait GlobalHandler {
  fn on_global(&mut self, conn: &mut Connection, name: u32, interface: &str, version: u32);
}

/// Build and send a `wl_registry::bind(name, interface, version, new_id)` request.
///
/// `version` should already be clamped to what the compositor advertised — see
/// [`clamp_version`].
pub fn bind(
  conn: &Connection,
  name: u32,
  interface: &str,
  version: u32,
  new_id: u32,
) -> Result<(), WireError> {
  let mut msg = Message::new(REGISTRY_ID, crate::protocols::wl_registry::request::BIND);
  msg.write_u32(name);
  msg.write_str(interface);
  msg.write_u32(version);
  msg.write_u32(new_id);
  conn.send(&msg, None)
}

/// Clamp a desired bind version down to what the compositor actually advertised.
#[must_use]
pub fn clamp_version(wanted: u32, advertised: u32) -> u32 {
  if advertised < wanted {
    advertised
  } else {
    wanted
  }
}

pub fn crawl<H: GlobalHandler + EventHandler>(
  conn: &mut Connection,
  handler: &mut H,
) -> Result<(), WireError> {
  // create registry object with id 2
  let mut reg_msg = Message::new(DISPLAY_ID, wl_display::request::GET_REGISTRY);
  reg_msg.write_u32(REGISTRY_ID);
  conn.send(&reg_msg, None)?;

  // sync call to make sure registry globals are sent
  let mut sync_msg = Message::new(DISPLAY_ID, wl_display::request::SYNC);
  sync_msg.write_u32(SYNC_CALLBACK_ID);
  conn.send(&sync_msg, None)?;

  loop {
    let mut buf = [0u8; 4096];
    let data = conn.recv_framed(&mut buf)?;

    let mut idx = 0;
    // iterate over all protocol messages received in this packet.
    while let Some(header) = parse_header(data, idx) {
      if header.sender == REGISTRY_ID && header.opcode == wl_registry::event::GLOBAL {
        let name = read_u32(data, idx + 8);
        if let Some((interface, consumed)) = read_str(data, idx + 12) {
          let version = read_u32(data, idx + 12 + consumed);
          handler.on_global(conn, name, interface, version);
        }
      } else if header.sender == DISPLAY_ID
        && header.opcode == crate::protocols::wl_display::event::ERROR
      {
        return Err(WireError::Protocol(ProtocolError::from(data, idx)));
      } else if header.sender == SYNC_CALLBACK_ID {
        return Ok(());
      } else {
        // any other event can show up here interleaved with registry/sync traffic in the same
        // recv() batch: `on_global` binds reactively as each `wl_registry::global` is parsed, and
        // some interfaces (like `wl_output`) fire a burst of events unprompted right after bind
        // with no request needed to trigger them. `sync` only orders requests sent *before* it, and
        // these bind() calls are sent *after* sync already went out, so there's no guarantee the
        // compositor's reply to a reactive bind lands after the sync `done` marker. forward it to
        // the same `EventHandler` the caller already needs to define for post-crawl runtime
        // dispatch.
        let event_data = &data[idx + 8..idx + header.size as usize];
        handler.handle_event(conn, header.sender, header.opcode, event_data);
      }
      idx += header.size as usize;
    }
  }
}
