//! Socket connection and id allocation.

use core::mem;

use crate::error::{SysError, WireError};
use crate::wire::{Message, complete_message_len};

/// Size of the leftover-message carry buffer `Connection` keeps for [`Connection::recv_framed`].
/// Must be at least as large as any buffer a caller passes to `recv_framed`; a leftover can
/// never exceed what was read into that buffer in the first place.
const RECV_CARRY_CAP: usize = 4096;

pub struct Connection {
  socket_fd: libc::c_int,
  next_id: u32,
  // Trailing bytes from the previous `recv_framed()` call that didn't form a complete message —
  // re-prepended to the front of the caller's buffer on the next call. See `recv_framed` for why
  // this has to live here rather than in a caller-local buffer the way `recv`'s `buf` does.
  carry: [u8; RECV_CARRY_CAP],
  carry_len: usize,
}

impl Connection {
  /// Connect to the Wayland socket.
  pub fn connect() -> Result<Self, WireError> {
    let display = unsafe { libc::getenv(c"WAYLAND_DISPLAY".as_ptr().cast::<libc::c_char>()) };
    let runtime = unsafe { libc::getenv(c"XDG_RUNTIME_DIR".as_ptr().cast::<libc::c_char>()) };
    if display.is_null() || runtime.is_null() {
      return Err(WireError::Environment);
    }

    let socket_fd =
      unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_STREAM | libc::SOCK_CLOEXEC, 0) };
    if socket_fd < 0 {
      return Err(WireError::Sys(SysError::last("socket")));
    }

    let mut sun = unsafe { mem::zeroed::<libc::sockaddr_un>() };
    sun.sun_family = libc::AF_UNIX as libc::sa_family_t;

    // construct the unix socket path
    let mut dest_idx = 0;
    unsafe {
      let mut src = runtime;
      while *src != 0 && dest_idx < 100 {
        sun.sun_path[dest_idx] = *src;
        dest_idx += 1;
        src = src.add(1);
      }
      if dest_idx < 107 {
        sun.sun_path[dest_idx] = b'/' as libc::c_char;
        dest_idx += 1;
      } else {
        return Err(WireError::Environment);
      }
      let mut src = display;
      while *src != 0 && dest_idx < 107 {
        sun.sun_path[dest_idx] = *src;
        dest_idx += 1;
        src = src.add(1);
      }
      if dest_idx >= 107 {
        return Err(WireError::Environment);
      }
    }

    let connect_res = unsafe {
      libc::connect(
        socket_fd,
        (&raw const sun).cast::<libc::sockaddr>(),
        mem::size_of::<libc::sockaddr_un>() as u32,
      )
    };
    if connect_res < 0 {
      let err = SysError::last("connect");
      unsafe { libc::close(socket_fd) };
      return Err(WireError::Sys(err));
    }

    Ok(Self {
      socket_fd,
      next_id: 4,
      carry: [0; RECV_CARRY_CAP],
      carry_len: 0,
    })
  }

  /// Allocate a unique client-side object id sequentially.
  pub fn alloc_id(&mut self) -> u32 {
    let id = self.next_id;
    self.next_id += 1;
    id
  }

  /// Send a protocol message over the socket. Optionally transmit fd along with the message using
  /// unix domain socket ancillary `SCM_RIGHTS` data.
  pub fn send(&self, msg: &Message, fd: Option<libc::c_int>) -> Result<(), WireError> {
    let msg_bytes = msg.as_bytes();
    let mut iov = libc::iovec {
      iov_base: msg_bytes.as_ptr() as *mut libc::c_void,
      iov_len: msg_bytes.len(),
    };
    let mut msghdr = unsafe { mem::zeroed::<libc::msghdr>() };
    msghdr.msg_iov = &raw mut iov;
    msghdr.msg_iovlen = 1 as _;

    // if no file descriptor needs to be passed, send a simple message
    let result = if let Some(f) = fd {
      // `msg_control` is a separate side channel from `msg_iov` (the actual message bytes).
      // this is the buffer the kernel will read the SCM_RIGHTS ancillary data out of. 24 bytes is
      // enough for one cmsghdr plus one padded `c_int` payload. a second fd would
      // need a bigger buffer, but nothing in this project ever sends more than one at a time.
      let mut cmsg_buf = [0u8; 24];
      msghdr.msg_control = cmsg_buf.as_mut_ptr().cast::<libc::c_void>();
      // `CMSG_SPACE` (unlike `CMSG_LEN` below) includes the alignment padding after the payload,
      // not just the header+payload themselves. `msg_controllen` describes how much of
      // `cmsg_buf` the kernel is allowed to touch, so it needs the padded figure.
      msghdr.msg_controllen = unsafe { libc::CMSG_SPACE(mem::size_of::<libc::c_int>() as _) };

      let cmsg = unsafe { &mut *(libc::CMSG_FIRSTHDR(&raw const msghdr)) };
      cmsg.cmsg_level = libc::SOL_SOCKET;
      cmsg.cmsg_type = libc::SCM_RIGHTS;
      cmsg.cmsg_len =
        unsafe { libc::CMSG_LEN(mem::size_of::<libc::c_int>() as u32) as libc::c_uint };
      // `CMSG_DATA` points just past the header to where its payload starts, so we write the actual
      // fd number there. this is the one line that hands `f` over to the kernel; everything
      // else in this branch is just describing the shape of that handoff.
      unsafe { core::ptr::write(libc::CMSG_DATA(cmsg).cast::<libc::c_int>(), f) };
      unsafe { libc::sendmsg(self.socket_fd, &raw const msghdr, 0) }
    } else {
      unsafe { libc::sendmsg(self.socket_fd, &raw const msghdr, 0) }
    };

    if result < 0 {
      Err(WireError::Sys(SysError::last("sendmsg")))
    } else {
      Ok(())
    }
  }

  /// Convenience wrapper around [`Self::send`] that logs failures via
  /// [`WireError::write_diagnostic`].
  pub fn send_logged(&self, msg: &Message, fd: Option<libc::c_int>) {
    if let Err(e) = self.send(msg, fd) {
      e.write_diagnostic();
    }
  }

  /// Read protocol data into `buf` and return what was received.
  pub fn recv<'a>(&self, buf: &'a mut [u8]) -> Result<&'a [u8], WireError> {
    let bytes = unsafe {
      libc::recv(
        self.socket_fd,
        buf.as_mut_ptr().cast::<libc::c_void>(),
        buf.len(),
        0,
      )
    };
    if bytes > 0 {
      Ok(&buf[..bytes as usize])
    } else if bytes == 0 {
      Err(WireError::ConnectionClosed)
    } else {
      Err(WireError::Sys(SysError::last("recv")))
    }
  }

  /// Like [`Self::recv`], but only ever hands back *complete* wire messages. A message that
  /// arrives split across two `recv()` calls is carried over internally and re-prepended to the
  /// front of `buf` on the next call, instead of being handed to a caller half-formed.
  ///
  /// Callers walking the returned slice with [`crate::wire::parse_header`] can assume every
  /// header they see is backed by its complete argument bytes; no partial trailing message will
  /// ever appear in what's returned here.
  ///
  /// Doesn't handle a *single* message larger than `buf`. This only fixes messages that fit in
  /// `buf` but got split across a `recv()` boundary, not messages too big for `buf` at all.
  pub fn recv_framed<'a>(&mut self, buf: &'a mut [u8]) -> Result<&'a [u8], WireError> {
    debug_assert!(
      buf.len() >= RECV_CARRY_CAP,
      "recv_framed: buf must be at least RECV_CARRY_CAP bytes, or a leftover carry might not fit"
    );

    // re-prime the buffer with whatever didn't form a complete message last time, then read new
    // bytes in after it. from this point on, `buf[..carry_len]` and the freshly-received bytes
    // are indistinguishable, both are just "data that arrived, not yet fully parsed."
    let carry_len = self.carry_len;
    buf[..carry_len].copy_from_slice(&self.carry[..carry_len]);

    let bytes = unsafe {
      libc::recv(
        self.socket_fd,
        buf[carry_len..].as_mut_ptr().cast::<libc::c_void>(),
        buf.len() - carry_len,
        0,
      )
    };
    if bytes == 0 {
      return Err(WireError::ConnectionClosed);
    }
    if bytes < 0 {
      return Err(WireError::Sys(SysError::last("recv")));
    }

    let total = carry_len + bytes as usize;
    let complete_end = complete_message_len(buf, total);

    // whatever's left after the last complete message (0 bytes if everything parsed cleanly, or
    // the leading bytes of one more message still waiting on the rest of its content) moves into
    // `self.carry` so it survives until the next call. `leftover <= total <= buf.len()` always
    // (this call only ever wrote at most `buf.len()` bytes into `buf`), and `buf.len() <=
    // RECV_CARRY_CAP` per the assert above, so this copy can never overrun `self.carry`.
    let leftover = total - complete_end;
    self.carry[..leftover].copy_from_slice(&buf[complete_end..total]);
    self.carry_len = leftover;

    Ok(&buf[..complete_end])
  }

  /// Read protocol data into `buf` and return what was received, along with the first fd if one was
  /// received.
  pub fn recv_with_fd<'a>(
    &self,
    buf: &'a mut [u8],
  ) -> (Result<&'a [u8], WireError>, Option<libc::c_int>) {
    let mut iov = libc::iovec {
      iov_base: buf.as_mut_ptr().cast::<libc::c_void>(),
      iov_len: buf.len(),
    };

    // configure ancillary control buffer to receive the fd
    let mut cmsg_buf = [0u8; 24];
    let mut msghdr = unsafe { mem::zeroed::<libc::msghdr>() };
    msghdr.msg_iov = &raw mut iov;
    msghdr.msg_iovlen = 1 as _;
    msghdr.msg_control = cmsg_buf.as_mut_ptr().cast::<libc::c_void>();
    msghdr.msg_controllen = unsafe { libc::CMSG_SPACE(mem::size_of::<libc::c_int>() as _) };

    let bytes = unsafe { libc::recvmsg(self.socket_fd, &raw mut msghdr, 0) };
    let mut received_fd = None;

    let res = if bytes > 0 {
      // the kernel decides whether any ancillary data actually arrived. a `recvmsg` that got a
      // plain data-only message returns a null cmsg pointer, not a header describing zero bytes of
      // payload.
      let cmsg = unsafe { libc::CMSG_FIRSTHDR(&raw const msghdr) };
      if !cmsg.is_null() {
        let cmsg_ref = unsafe { &*cmsg };
        // confirm this is actually the SCM_RIGHTS ancillary data this method expects before
        // trusting its payload as an fd.
        if cmsg_ref.cmsg_level == libc::SOL_SOCKET && cmsg_ref.cmsg_type == libc::SCM_RIGHTS {
          // `CMSG_DATA`'s returned pointer is only guaranteed aligned to the cmsg header's own
          // alignment, not necessarily to `c_int`'s. reading through an ordinary `*const
          // libc::c_int` reference would be UB if the two don't happen to coincide.
          let fd_ptr = unsafe { libc::CMSG_DATA(cmsg) } as *const libc::c_int;
          received_fd = Some(unsafe { core::ptr::read_unaligned(fd_ptr) });
        }
      }
      Ok(&buf[..bytes as usize])
    } else if bytes == 0 {
      Err(WireError::ConnectionClosed)
    } else {
      Err(WireError::Sys(SysError::last("recv")))
    };
    (res, received_fd)
  }
}

impl Drop for Connection {
  fn drop(&mut self) {
    unsafe { libc::close(self.socket_fd) };
  }
}
