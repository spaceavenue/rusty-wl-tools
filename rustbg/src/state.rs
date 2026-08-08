use wllib::wire::{Message, parse_header, read_u16, read_u32};

use crate::wayland::Wayland;
use crate::{AppError, shm, wayland, write_err, write_u32_err};

// abstration around wl_output
pub struct Output {
    pub global_name: u32,
    pub output_id: u32,
    pub wl_surface_id: u32,
    pub layer_surface_id: u32,
    pub buffer_id: u32,
    pub width: u32,
    pub height: u32,
}

// config
pub struct Config {
    pub image_path: Option<*const libc::c_char>,
    pub namespace: *const libc::c_char,
    pub fill: bool,
}
// default config
impl Default for Config {
    fn default() -> Self {
        Self {
            image_path: Some(c"image.png".as_ptr() as *const libc::c_char),
            namespace: c"wallpaper".as_ptr() as *const libc::c_char,
            fill: false,
        }
    }
}
pub struct State {
    pub wayland: Wayland,
    pub outputs: [Option<Output>; 4],
    pub output_len: usize,
    pub config: Config,
}

impl State {
    pub fn init(config: Config) -> Result<State, AppError> {
        let Ok(wayland) = wayland::Wayland::init().map_err(|e| write_err(e.message())) else {
            unsafe { libc::exit(1) }
        };
        let state = State {
            wayland: wayland,
            outputs: core::array::from_fn(|_| None),
            output_len: 0,
            config,
        };
        Ok(state)
    }

    // read messages from socket, parse compositor registry. we loop until the sync callback event
    // (sender ID 3) is received
    pub fn read_and_parse_registry(&mut self) {
        let mut done = false;
        while !done {
            let mut buf = [0u8; 4096];
            let bytes = unsafe {
                libc::recv(
                    self.wayland.socket_fd,
                    buf.as_mut_ptr() as *mut libc::c_void,
                    buf.len(),
                    0,
                )
            };
            if bytes <= 0 {
                return;
            }

            let mut idx = 0;
            // iterate over all protocol messages received in this packet.
            while idx < bytes as usize {
                let Some(msg_hdr) = parse_header(&buf, idx) else {
                    return;
                };
                let sender = msg_hdr.sender;
                let opcode = msg_hdr.opcode;
                let size = msg_hdr.size as usize;
                if size == 0 {
                    break;
                }

                // sender 2 -> wl_registry, opcode 0 -> wl_registry::global
                // arguments: name (uint), interface (string), version (uint).
                if sender == 2 && opcode == 0 {
                    let name = read_u32(&buf, idx + 8);
                    let str_len = read_u32(&buf, idx + 12) as usize; // length including null terminator
                    if str_len > 1 && idx + 16 + str_len <= bytes as usize {
                        let interface = &buf[idx + 16..idx + 16 + (str_len - 1)];
                        let padded_str_len = (str_len + 3) & !3; // string is padded to 4 bytes/32-bit boundary
                        let advertised_version = read_u32(&buf, idx + 16 + padded_str_len);
                        self.match_and_bind_global(interface, name, advertised_version);
                    }
                }
                // sender 1 -> wl_display. opcode 0 -> wl_display::error. so we're handling protocol
                // errors basically
                if sender == 1 && opcode == 0 {
                    write_err(b"parse registry: wayland protocol error\n");
                    return;
                }
                // sender 3 -> sync callback. eeceiving this means the server has processed our
                // get_registry request and sent all globals
                if sender == 3 {
                    done = true;
                    break;
                }
                idx += size;
            }
        }
    }

    // bind globals matching interfaces we support. we bind to the minimum of the client's
    // wanted version and the server's advertised version
    fn match_and_bind_global(&mut self, interface: &[u8], name: u32, advertised_version: u32) {
        let bind = |wanted: u32| -> u32 {
            if advertised_version < wanted {
                advertised_version
            } else {
                wanted
            }
        };
        match interface {
            b"wl_compositor" => {
                self.wayland.compositor_id = self.wayland.alloc_id();
                let mut msg = Message::new(2, 0);
                msg.write_u32(name);
                msg.write_string(b"wl_compositor");
                msg.write_u32(bind(7));
                msg.write_u32(self.wayland.compositor_id);
                self.wayland.send(&msg, None);
            }
            b"wl_shm" => {
                self.wayland.shm_id = self.wayland.alloc_id();
                let mut msg = Message::new(2, 0);
                msg.write_u32(name);
                msg.write_string(b"wl_shm");
                msg.write_u32(bind(2));
                msg.write_u32(self.wayland.shm_id);
                self.wayland.send(&msg, None);
            }
            b"zwlr_layer_shell_v1" => {
                self.wayland.layer_shell_id = self.wayland.alloc_id();
                let mut msg = Message::new(2, 0);
                msg.write_u32(name);
                msg.write_string(b"zwlr_layer_shell_v1");
                msg.write_u32(bind(5));
                msg.write_u32(self.wayland.layer_shell_id);
                self.wayland.send(&msg, None);
            }
            b"wl_output" => {
                if self.output_len >= 4 {
                    return;
                }
                let out_id = self.wayland.alloc_id();
                let mut msg = Message::new(2, 0);
                msg.write_u32(name);
                msg.write_string(b"wl_output");
                msg.write_u32(bind(4));
                msg.write_u32(out_id);
                self.wayland.send(&msg, None);

                self.outputs[self.output_len] = Some(Output {
                    global_name: name,
                    output_id: out_id,
                    wl_surface_id: 0,
                    layer_surface_id: 0,
                    buffer_id: 0,
                    width: 0,
                    height: 0,
                });
                self.output_len += 1;
            }
            _ => (),
        }
    }

    // main runtime loop event parser. read incoming packets from the socket, parse protocol
    // messages, detect protocol errors, match them to active outputs. basically eveything fun :)
    pub fn process_runtime_events(&mut self) -> bool {
        let mut buf = [0u8; 4096];
        let bytes = unsafe {
            libc::recv(
                self.wayland.socket_fd,
                buf.as_mut_ptr() as *mut libc::c_void,
                buf.len(),
                0,
            )
        };
        // connection closed by compositor (EOF)
        if bytes == 0 {
            return false;
        }
        // socket error
        if bytes < 0 {
            write_err(b"Wayland socket read error\n");
            return false;
        }

        let mut idx = 0;
        // parse messages in the received buffer
        while idx < bytes as usize {
            let sender = read_u32(&buf, idx);
            let opcode = read_u16(&buf, idx + 4);
            let size = read_u16(&buf, idx + 6) as usize;
            if size == 0 {
                break;
            }

            // sender 1 -> wl_display. opcode 0 -> wl_display::error. so we're handling protocol
            // errors basically, a bit more informative this time
            if sender == 1 && opcode == 0 {
                let failing_object_id = read_u32(&buf, idx + 8);
                let error_code = read_u32(&buf, idx + 12);
                let error_string_len = read_u32(&buf, idx + 16) as usize;
                write_err(b"[wayland protocol error] object: ");
                write_u32_err(failing_object_id);
                write_err(b", error code: ");
                write_u32_err(error_code);
                write_err(b", message: ");
                if idx + 20 + error_string_len <= buf.len() {
                    write_err(&buf[(idx + 20)..(idx + 20 + error_string_len)]);
                }
                write_err(b"\n");
                return false;
            }

            // check if this event belongs to any of our bound layer surface objects
            let mut match_found = false;
            let mut out_idx = 0;
            for i in 0..4 {
                let Some(ref output) = self.outputs[i] else {
                    break;
                };
                if (output.layer_surface_id) == sender {
                    match_found = true;
                    out_idx = i;
                    break;
                }
            }

            // if it what we want, dispatch.
            if match_found {
                self.dispatch_single_event(&buf, idx, sender, opcode, out_idx);
            }
            idx += size;
        }
        true
    }

    // dispatch individual events for a specific output.handles surface configuration events like
    // setting the wallpaper
    fn dispatch_single_event(
        &mut self,
        buf: &[u8],
        idx: usize,
        sender: u32,
        opcode: u16,
        i: usize,
    ) {
        let Some(ref mut out) = self.outputs[i] else {
            return;
        };

        match sender {
            // event matches a bound layer surface object
            val if val == out.layer_surface_id => {
                if opcode != 0 {
                    // opcode 0 is zwlr_layer_surface_v1::configure
                    return;
                }
                let serial = read_u32(buf, idx + 8);
                let width = read_u32(buf, idx + 12);
                let height = read_u32(buf, idx + 16);

                // acknowledge the layer surface configuration
                // zwlr_layer_surface_v1 (ID) -> request opcode 6 (ack_configure)
                let mut ack = Message::new(out.layer_surface_id, 6);
                ack.write_u32(serial);
                self.wayland.send(&ack, None);

                // get the fd containing the scaled image data
                let Ok(fd) = shm::get_image_fd(width, height, self) else {
                    return;
                };
                let Some(ref mut target_out) = self.outputs[i] else {
                    return;
                };
                let pool_id = self.wayland.alloc_id();
                let buffer_id = self.wayland.alloc_id();

                // create a shared memory pool
                // wl_shm (ID) -> request opcode 0 (create_pool)
                let mut pool_msg = Message::new(self.wayland.shm_id, 0);
                pool_msg.write_u32(pool_id);
                pool_msg.write_i32((width * height * 4) as i32); // size = w * h * 4 bytes
                self.wayland.send(&pool_msg, Some(fd));
                unsafe {
                    libc::close(fd); // client no longer needs the fd after sending
                }

                // create a wl_buffer from the pool
                // wl_shm_pool (ID) -> request opcode 0 (create_buffer)
                let mut buf_msg = Message::new(pool_id, 0);
                buf_msg.write_u32(buffer_id);
                buf_msg.write_i32(0); // offset
                buf_msg.write_i32(width as i32);
                buf_msg.write_i32(height as i32);
                buf_msg.write_i32((width * 4) as i32); // stride
                buf_msg.write_u32(0); // 0 -> format: WL_SHM_FORMAT_ARGB8888
                self.wayland.send(&buf_msg, None);

                // destroy the pool object, buffer remains valid tho
                // wl_shm_pool (ID) -> request opcode 1 (destroy)
                let destroy_pool = Message::new(pool_id, 1);
                self.wayland.send(&destroy_pool, None);

                target_out.buffer_id = buffer_id;

                // attach the buffer to the surface
                // wl_surface (ID) -> request opcode 1 (attach)
                let mut attach = Message::new(target_out.wl_surface_id, 1);
                attach.write_u32(buffer_id as u32);
                attach.write_i32(0); // x offset
                attach.write_i32(0); // y offset
                self.wayland.send(&attach, None);

                // mark the entire surface as damaged
                // wl_surface (ID) -> request opcode 9 (damage_buffer)
                let mut dmg = Message::new(target_out.wl_surface_id, 9);
                dmg.write_i32(0);
                dmg.write_i32(0);
                dmg.write_i32(width as i32);
                dmg.write_i32(height as i32);
                self.wayland.send(&dmg, None);

                // commit the state changes to the surface
                // wl_surface (ID) -> request opcode 6 (commit)
                self.wayland
                    .send(&Message::new(target_out.wl_surface_id, 6), None);
            }
            _ => return,
        }
    }
}
