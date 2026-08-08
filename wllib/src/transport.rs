//! Socket connection and id allocation.

use core::mem;

use crate::error::{SysError, WireError};
use crate::wire::Message;

/// Result of a `recv()` call on the wayland socket.
pub enum RecvResult<'a> {
    /// Data was read into the buffer.
    Data(&'a [u8]),
    /// The compositor closed the connection.
    Closed,
    /// The underlying `recv(2)` call failed.
    Error(SysError),
}

pub struct Connection {
    socket_fd: libc::c_int,
    next_id: u32,
}
impl Connection {
    /// Connect to the Wayland socket.
    pub fn connect() -> Result<Self, WireError> {
        let display = unsafe { libc::getenv(c"WAYLAND_DISPLAY".as_ptr() as *const libc::c_char) };
        let runtime = unsafe { libc::getenv(c"XDG_RUNTIME_DIR".as_ptr() as *const libc::c_char) };
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
            }
            let mut src = display;
            while *src != 0 && dest_idx < 107 {
                sun.sun_path[dest_idx] = *src;
                dest_idx += 1;
                src = src.add(1);
            }
        }

        let connect_res = unsafe {
            libc::connect(
                socket_fd,
                &sun as *const libc::sockaddr_un as *const libc::sockaddr,
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
        msghdr.msg_iov = &mut iov;
        msghdr.msg_iovlen = 1 as _;

        // if no file descriptor needs to be passed, send a simple message
        let result = if let Some(f) = fd {
            // configure ancillary control buffer to pass the fd
            let mut cmsg_buf = [0u8; 24];
            msghdr.msg_control = cmsg_buf.as_mut_ptr() as *mut libc::c_void;
            msghdr.msg_controllen =
                unsafe { libc::CMSG_SPACE(mem::size_of::<libc::c_int>() as u32) as u32 };

            let cmsg = unsafe { &mut *(libc::CMSG_FIRSTHDR(&msghdr)) };
            cmsg.cmsg_level = libc::SOL_SOCKET;
            cmsg.cmsg_type = libc::SCM_RIGHTS;
            cmsg.cmsg_len =
                unsafe { libc::CMSG_LEN(mem::size_of::<libc::c_int>() as u32) as libc::c_uint };
            unsafe { core::ptr::write(libc::CMSG_DATA(cmsg) as *mut libc::c_int, f) };
            unsafe { libc::sendmsg(self.socket_fd, &msghdr, 0) }
        } else {
            unsafe { libc::sendmsg(self.socket_fd, &msghdr, 0) }
        };

        if result < 0 {
            Err(WireError::Sys(SysError::last("sendmsg")))
        } else {
            Ok(())
        }
    }

    /// Convenience wrapper around [`Self::send`] that logs failures via
    /// [`WireError::write_diagnostic`] instead of requiring every call site to handle socket errors
    /// individually.
    pub fn send_logged(&self, msg: &Message, fd: Option<libc::c_int>) {
        if let Err(e) = self.send(msg, fd) {
            e.write_diagnostic();
        }
    }

    /// Read protocol data into `buf` and return what was received.
    pub fn recv<'a>(&self, buf: &'a mut [u8]) -> RecvResult<'a> {
        let bytes = unsafe {
            libc::recv(
                self.socket_fd,
                buf.as_mut_ptr() as *mut libc::c_void,
                buf.len(),
                0,
            )
        };
        if bytes > 0 {
            RecvResult::Data(&buf[..bytes as usize])
        } else if bytes == 0 {
            RecvResult::Closed
        } else {
            RecvResult::Error(SysError::last("recv"))
        }
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        unsafe { libc::close(self.socket_fd) };
    }
}
