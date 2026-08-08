# What
This is a small and simple Rust-based client for setting the wallpaper on wayland, using the wlr-layer-shell protocol. Image loading and decoding is handled by [dump-bgra](https://codeberg.org/spaceavenue/dump-bgra) (a tool i created specifically to dump bgra pixels as fast as possible), so you need to have it installed on your system for the wallpaper to work.

It is statically linked with musl (or at least I do it that way), and uses `#![no_std]` to completely eliminate the stdlib from the binary. Thus it also uses no external crates like `wayland-client` or even `wayland-sys`, and relies solely on reading from and writing to the wayland socket to communicate with the server.

This is a personal project so don't expect stability or the best code quality :P In fact, because it doesn't uses stdlib, it extensively uses unsafe Rust and libc function calls and interfaces everywhere. To be completely honest, you probably shouldn't use this lmao. Stick to something stable and tested, with more features like [swaybg](https://github.com/swaywm/swaybg), or for a more minimal option, [wbg](https://codeberg.org/dnkl/wbg).

# Why And How
Fun :3  
But also I kinda just went down a rabbit hole of trying to make a wallpaper daemon that consumes as little memory as possible.  

I started out by trying to call `malloc_trim(0)` to remove unused memory, to then linking with `musl` statically, to then trying to remove `core::fmt` and free myself from Rust's (excellent) formatting engine, just so I could make the binary itself as small as possible. 
I then shifted to removing any and all formatting done by Rust at `panic!` by enabling `panic-immediate-abort` and, well, immediately aborting on panic. The binary is gutted of location details and debug formatting architecture too, via `-Z location-detail=none` and `-Z fmt-debug=none`.  

The stdlib is built at compile-time and with `panic_abort` and the feature `optimize-for-size`. I then took on a major architectural shift by removing all external crates and relying only on corelib and libc, and communicating with the server only by reading from and writing to the wayland socket.
