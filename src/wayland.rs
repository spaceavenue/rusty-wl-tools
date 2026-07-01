use crate::{AppError, file_err};

// structured wayland protocol message
// wayland uses a format consisting of 32-bit aligned words
pub struct Message {
    // fixed 256 buffer bytes for raw message payload
    pub data: [u8; 256],
    // current length of the message in bytes
    pub len: usize,
}

impl Message {
    // create a new message targeting `obj_id` with request/event `opcode`
    // header structure:
    //   - bytes 0-3: 32-bit sender/destination object id
    //   - bytes 4-5: 16-bit opcode (request/event identifier)
    //   - bytes 6-7: 16-bit total message size in bytes (written by finalize)
    pub fn new(obj_id: u32, opcode: u16) -> Self {
        let mut msg = Self {
            data: [0; 256],
            len: 8, // header occupies first 8 bytes
        };
        msg.data[0..4].copy_from_slice(&obj_id.to_ne_bytes());
        msg.data[4..6].copy_from_slice(&opcode.to_ne_bytes());
        msg
    }

    // write a 32-bit unsigned integer to the message args
    pub fn write_u32(&mut self, val: u32) {
        if self.len + 4 > 256 {
            return;
        }
        self.data[self.len..self.len + 4].copy_from_slice(&val.to_ne_bytes());
        self.len += 4;
    }

    // write a 32-bit signed integer to the message args
    pub fn write_i32(&mut self, val: i32) {
        if self.len + 4 > 256 {
            return;
        }
        self.data[self.len..self.len + 4].copy_from_slice(&val.to_ne_bytes());
        self.len += 4;
    }

    // write a string to the args
    // strings are serialized as:
    //   - 32-bit length prefix (including the null terminator)
    //   - string bytes (null-terminated)
    //   - 0 byte padding to align the next arg to a 32-bit boundary
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
        };

        if self.len + actual_len + 1 <= 256 {
            self.data[self.len..self.len + actual_len].copy_from_slice(&s[..actual_len]);
            self.data[self.len + actual_len] = b'\0';
            self.len += actual_len + 1;
            // pad message length to multiple of 4 bytes/32-bits
            self.len = (self.len + 3) & !3;
        }
    }

    // write a raw c-string to the payload (mostly used by c ptrs in cli args)
    pub fn write_cstr(&mut self, s: *const libc::c_char) {
        unsafe {
            let mut len = 0usize;
            while *s.add(len) != 0 {
                len += 1;
            }
            self.write_u32((len + 1) as u32);
            if self.len + len + 1 <= 256 {
                core::ptr::copy_nonoverlapping(
                    s as *const u8,
                    self.data.as_mut_ptr().add(self.len),
                    len,
                );
            }
            self.data[self.len + len] = b'\0';
            self.len += len + 1;
            // pad message length to multiple of 4 bytes/32-bits
            self.len = (self.len + 3) & !3;
        }
    }

    // write the final size to bytes 6-7 (heh) of the header, return the Message
    pub fn finalize(mut self) -> Self {
        let total_size = self.len as u16;
        self.data[6..8].copy_from_slice(&total_size.to_ne_bytes());
        self
    }
}

// qanages the Unix socket connection, tracks registered object ids
pub struct Wayland {
    // raw socket file descriptor connected to the wayland server
    pub socket_fd: libc::c_int,
    // wl_compositor global id
    pub compositor_id: u32,
    // wl_shm global id
    pub shm_id: u32,
    // zwlr_layer_shell_v1 global id
    pub layer_shell_id: u32,
    // next object id to assign
    next_id: u32,
}

impl Wayland {
    // allocate a unique client-side object id sequentially.
    pub fn alloc_id(&mut self) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    // send a protocol message over the socket. optionally transmit fd along with the message using
    // unix domain socket ancillary data aka `SCM_RIGHTS`
    pub fn send(&self, msg: &Message, fd: Option<libc::c_int>) {
        let mut iov = libc::iovec {
            iov_base: msg.data.as_ptr() as *mut libc::c_void,
            iov_len: msg.len,
        };
        let mut msghdr = unsafe { core::mem::zeroed::<libc::msghdr>() };
        msghdr.msg_iov = &mut iov;
        msghdr.msg_iovlen = 1 as _;

        // if no file descriptor needs to be passed, send a simple message
        let Some(f) = fd else {
            unsafe {
                libc::sendmsg(self.socket_fd, &msghdr, 0);
            }
            return;
        };

        // otherwise, configure ancillary control buffer to pass the fd
        let mut cmsg_buf = [0u8; 24];
        msghdr.msg_control = cmsg_buf.as_mut_ptr() as *mut libc::c_void;
        msghdr.msg_controllen =
            unsafe { libc::CMSG_SPACE(core::mem::size_of::<libc::c_int>() as u32) as u32 };

        let cmsg = unsafe { &mut *(libc::CMSG_FIRSTHDR(&msghdr)) };
        cmsg.cmsg_level = libc::SOL_SOCKET;
        cmsg.cmsg_type = libc::SCM_RIGHTS;
        cmsg.cmsg_len =
            unsafe { libc::CMSG_LEN(core::mem::size_of::<libc::c_int>() as u32) as libc::c_uint };
        unsafe {
            core::ptr::write(libc::CMSG_DATA(cmsg) as *mut libc::c_int, f);
        }
        unsafe {
            libc::sendmsg(self.socket_fd, &msghdr, 0);
        }
    }

    // initialize the wayland connection by connecting to the server socket.
    // the socket path is constructed from `XDG_RUNTIME_DIR` and `WAYLAND_DISPLAY`
    // also binds the initial registry object and perform a display sync callback
    pub fn init() -> Result<Self, AppError> {
        let display = unsafe { libc::getenv(b"WAYLAND_DISPLAY\0".as_ptr() as *const libc::c_char) };
        let runtime = unsafe { libc::getenv(b"XDG_RUNTIME_DIR\0".as_ptr() as *const libc::c_char) };
        if display.is_null() || runtime.is_null() {
            return Err(AppError::WaylandError);
        }

        // open the Unix domain socket
        let socket_fd =
            unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_STREAM | libc::SOCK_CLOEXEC, 0) };
        if socket_fd < 0 {
            return Err(file_err());
        }

        let mut sun = unsafe { core::mem::zeroed::<libc::sockaddr_un>() };
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
                core::mem::size_of::<libc::sockaddr_un>() as u32,
            )
        };
        if connect_res < 0 {
            unsafe {
                libc::close(socket_fd);
            }
            return Err(file_err());
        }

        let wayland = Self {
            socket_fd,
            compositor_id: 0,
            shm_id: 0,
            layer_shell_id: 0,
            // IDs 1 -> display, 2 -> registry, 3 -> sync callback. so next id is 4
            // for some reason this not being sequential was causing errors even though the protocol
            // says the ids can have gaps in them. maybe compositor specific
            next_id: 4,
        };

        // create registry object with id 2
        // wl_display (ID 1) -> request opcode 1 (get_registry)
        let mut reg_msg = Message::new(1, 1);
        reg_msg.write_u32(2);
        wayland.send(&reg_msg.finalize(), None);

        // sync call to make sure registry globals are sent
        // wl_display (ID 1) -> request opcode 0 (sync)
        let mut sync_msg = Message::new(1, 0);
        sync_msg.write_u32(3); // 3 -> callback object ID
        wayland.send(&sync_msg.finalize(), None);

        Ok(wayland)
    }
}
