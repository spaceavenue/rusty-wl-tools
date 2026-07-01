#![no_std]
#![no_main]

use rustbg::state::{Config, State};
use rustbg::wayland::Message;
use rustbg::{remove_self, write_err};

unsafe extern "C" {
    static optarg: *const libc::c_char;
    static mut optind: libc::c_int;
}
#[link(name = "c", kind = "static")]
unsafe extern "C" {}

#[unsafe(no_mangle)]
pub extern "C" fn main(argc: isize, argv: *const *mut libc::c_char) -> libc::c_int {
    let mut config = Config::default();
    loop {
        let c = unsafe { libc::getopt(argc as i32, argv, c"fn:t:".as_ptr()) };
        if c == -1 {
            break;
        }
        match c as u8 as char {
            'f' => config.fill = true,
            'n' => {
                if unsafe { optarg.is_null() } {
                    break;
                }
                config.namespace = unsafe { optarg }
            }
            _ => (),
        }
    }
    let optind_val = unsafe { optind };
    if optind_val < argc as libc::c_int {
        config.image_path = Some(unsafe { *argv.add((optind_val) as usize) });
    }

    if config.image_path.is_none() {
        write_err(b"Usage: rustbg [-f | --fill] [-n | --namespace <name>] <image path>\n");
        unsafe { libc::exit(1) };
    }

    // initialize the state. connection to the wayland compositor and registry init all happens
    // here.
    let Ok(mut state) = State::init(config) else {
        write_err(b"failed to init state\n");
        unsafe { libc::exit(1) };
    };

    state.read_and_parse_registry();

    // setup layer surfaces and gamma control for all monitors.
    for i in 0..4 {
        if state.outputs[i].is_none() {
            continue;
        }

        // alloc ids for the new wl_surface (from compositor), layer surface, and gamma control
        // objects.
        let surf_id = state.wayland.alloc_id();
        let layer_surf_id = state.wayland.alloc_id();

        let output_id = match state.outputs[i] {
            Some(ref o) => o.output_id,
            None => continue,
        };

        // this is a multi step process
        //
        // 1. creates a wl_surface
        // wl_compositor (ID) -> request opcode 0 (create_surface)
        let mut surf_msg = Message::new(state.wayland.compositor_id, 0);
        surf_msg.write_u32(surf_id);
        state.wayland.send(&surf_msg.finalize(), None);

        // 2. wrap the wl_surface as a layer surface
        // zwlr_layer_shell_v1 (ID) -> request opcode 0 (get_layer_surface)
        let mut ls_msg = Message::new(state.wayland.layer_shell_id, 0);
        ls_msg.write_u32(layer_surf_id);
        ls_msg.write_u32(surf_id);
        ls_msg.write_u32(output_id);
        ls_msg.write_u32(0); // 0 -> background layer
        ls_msg.write_cstr(state.config.namespace);
        state.wayland.send(&ls_msg.finalize(), None);

        // anchor to all edges for full screen
        // zwlr_layer_surface_v1 (ID) -> request opcode 1 (set_anchor)
        let mut anchor_msg = Message::new(layer_surf_id, 1);
        anchor_msg.write_u32(15); // 15 -> achor to: top | bottom | left | right (1 | 2 | 4 | 8 = 15)
        state.wayland.send(&anchor_msg.finalize(), None);

        // configure exclusive zone to go behind other layer surfaces, like panels or status bars
        // zwlr_layer_surface_v1 (ID) -> request opcode 2 (set_exclusive_zone)
        let mut ex_msg = Message::new(layer_surf_id, 2);
        ex_msg.write_i32(-1); // -1 means do not request any exclusive zone.
        state.wayland.send(&ex_msg.finalize(), None);

        // initial attach (attaching 0 or NULL tells compositor to map surface)
        // wl_surface (ID) -> request opcode 1 (attach)
        let mut attach_msg = Message::new(surf_id, 1);
        attach_msg.write_u32(0); // NULL -> map surface
        attach_msg.write_i32(0);
        attach_msg.write_i32(0);
        state.wayland.send(&attach_msg.finalize(), None);

        // commit initial surface
        // wl_surface (ID) -> request opcode 6 (commit)
        state
            .wayland
            .send(&Message::new(surf_id, 6).finalize(), None);

        // store the allocated ids back into our Output instance
        if let Some(ref mut out) = state.outputs[i] {
            out.wl_surface_id = surf_id;
            out.layer_surface_id = layer_surf_id;
        }
    }

    // probably doesnt even do anything atp
    // (not like it did before either, madvise is just a "strong suggestion")
    // but still, for the illusion ig :3
    // also the name is kinda funny
    remove_self::evict_self_from_ram();

    // main wayland event dispatch loop
    while state.process_runtime_events() {}
    0
}
