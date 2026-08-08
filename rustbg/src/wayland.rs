use wllib::protocols::wl_callback::SYNC_CALLBACK_ID;
use wllib::protocols::wl_display;
use wllib::protocols::wl_display::DISPLAY_ID;
use wllib::protocols::wl_registry::REGISTRY_ID;
use wllib::wire::Message;

use crate::{AppError, file_err};

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
            iov_base: msg.as_bytes().as_ptr() as *mut libc::c_void,
            iov_len: msg.header().size as usize,
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
        let mut reg_msg = Message::new(DISPLAY_ID, wl_display::request::GET_REGISTRY);
        reg_msg.write_u32(REGISTRY_ID);
        wayland.send(&reg_msg, None);

        // sync call to make sure registry globals are sent
        let mut sync_msg = Message::new(DISPLAY_ID, wl_display::request::SYNC);
        sync_msg.write_u32(SYNC_CALLBACK_ID);
        wayland.send(&sync_msg, None);

        Ok(wayland)
    }
}
