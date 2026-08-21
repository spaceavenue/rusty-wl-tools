//! File-descriptor write helpers.

use crate::error::SysError;

/// Writes `msg` to the file descriptor, retrying on a partial write and on `EINTR` until either
/// every requested byte has been written or a real error occurs.
///
/// `count` optionally caps how many bytes of `msg` get written, clamped to `[1, msg.len()]`
/// rather than allowed to be 0, since a 0-byte "write" every caller actually wants is better
/// expressed as not calling this at all, and treating 0 as "write everything" (i.e. falling
/// through to `None`'s behavior) would make a caller's off-by-one bug silently write the whole
/// buffer instead of failing loudly.
pub fn write_fd(
  fd: libc::c_int,
  msg: impl AsRef<[u8]>,
  count: Option<usize>,
) -> Result<(), SysError> {
  let msg = msg.as_ref();
  let len = match count {
    Some(c) => c.clamp(1, msg.len()),
    None => msg.len(),
  };
  // truncate once up front so every iteration below can just trust `msg.len()`. truncating `msg`
  // itself here means "how much is left" and "what `write()` should see" can never disagree.
  let mut msg = &msg[..len];
  while !msg.is_empty() {
    let res = unsafe { libc::write(fd, msg.as_ptr().cast::<libc::c_void>(), msg.len()) };
    match res {
      // a `write()` of a nonzero-length buffer returning exactly 0 isn't specified to happen for
      // regular files/pipes/sockets, but treating it as "nothing more to do" rather than looping
      // forever is the safe reading if it ever does.
      0 => break,
      res if res > 0 => {
        // a "short write". the kernel accepted fewer bytes than asked (e.g. a full pipe buffer,
        // or a signal interrupting a partial transfer). isn't an error, just unfinished, so retry
        // with whatever's left.
        msg = &msg[res as usize..];
      }
      _ => {
        let err = SysError::last("write");
        if err.errno == libc::EINTR {
          continue;
        }
        return Err(err);
      }
    }
  }
  Ok(())
}

/// Writes the buffer to stderr.
pub fn write_stderr(msg: impl AsRef<[u8]>) {
  let _ = write_fd(2, msg, None);
}

/// Writes the buffer to stdout.
pub fn write_stdout(msg: impl AsRef<[u8]>) {
  let _ = write_fd(1, msg, None);
}
