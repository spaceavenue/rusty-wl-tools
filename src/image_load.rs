use crate::state::State;
use crate::{AppError, file_err, image_err};

// build strings dynamically on the stack, no heap allocation needed
struct StringOnStack {
    buffer: [u8; 8],
    len: usize,
}
impl StringOnStack {
    fn new() -> Self {
        StringOnStack {
            buffer: [0; 8],
            len: 0,
        }
    }
    // push an integer converted to ASCII representation
    fn push_usize(&mut self, mut num: usize) {
        if num == 0 {
            self.push_str(b"0");
            return;
        }
        let mut tmp = [0u8; 10];
        let mut idx = 10;
        while num > 0 {
            idx -= 1;
            tmp[idx] = b'0' + (num % 10) as u8;
            num /= 10;
        }
        self.push_str(&tmp[idx..10]);
    }
    // push a byte slice/string
    fn push_str(&mut self, str: &[u8]) {
        let new_len = self.len + str.len();
        if new_len < 95 {
            self.buffer[self.len..new_len].copy_from_slice(str);
            self.len = new_len;
        }
    }
}

// this was a bit hard to follow so im documenting for my own reference
// we:
// 1. create a unix pipe
// 2. spawn the dump-bgra child process (fork + execvp)
// 3. connect it's write end (stdout) to the write end of the pipe
// 4. then read the raw bgra pixels from our read end, directly into the mmap_slice
pub fn load_and_scale(
    out_width: u32,
    out_height: u32,
    buffer: &mut [u8],
    state: &mut State,
) -> Result<(), AppError> {
    let Some(path) = state.config.image_path else {
        return Err(file_err());
    };
    let mut w_str = StringOnStack::new();
    w_str.push_usize(out_width as usize);
    let mut h_str = StringOnStack::new();
    h_str.push_usize(out_height as usize);
    let mut mode_str = StringOnStack::new();
    if state.config.fill {
        mode_str.push_str(b"fill\0");
    } else {
        mode_str.push_str(b"fit\0");
    }

    unsafe {
        let mut pipe = [0i32; 2];
        // create an unidirectional pipe to read data from child process
        // O_CLOEXEC closes the read/write ends in any other spawned children
        if libc::pipe2(pipe.as_mut_ptr(), libc::O_CLOEXEC) != 0 {
            return Err(file_err());
        }

        let pid = libc::fork();
        if pid < 0 {
            libc::close(pipe[0]);
            libc::close(pipe[1]);
            return Err(file_err());
        }
        // child process context
        if pid == 0 {
            libc::close(pipe[0]);
            libc::dup2(pipe[1], 1); // redirect stdout to the write-end of the pipe
            libc::close(pipe[1]);
            libc::signal(libc::SIGPIPE, libc::SIG_DFL); // eestore default SIGPIPE handler before exec

            // redirect stderr to /dev/null
            let devnull = libc::open(c"/dev/null".as_ptr(), libc::O_WRONLY);
            if devnull >= 0 {
                libc::dup2(devnull, 2);
                libc::close(devnull);
            }

            // build ffmpeg argument vector: scale image to raw bgra pixels and stream to stdout
            let argv: [*const libc::c_char; 7] = [
                c"dump-bgra".as_ptr(),
                w_str.buffer.as_ptr() as _,
                h_str.buffer.as_ptr() as _,
                mode_str.buffer.as_ptr() as _,
                path,
                c"-".as_ptr(),
                core::ptr::null(),
            ];
            libc::execvp(c"dump-bgra".as_ptr(), argv.as_ptr());
            libc::_exit(1); // if execvp failed, kill child >:)
        }

        // parent process context
        libc::close(pipe[1]); // close the write end in parent
        let mut offset = 0usize;
        // read raw bytes from the pipe directly into buffer
        loop {
            let remaining = buffer.len() - offset;
            if remaining == 0 {
                break;
            }
            let num_bytes = libc::read(pipe[0], buffer.as_mut_ptr().add(offset) as _, remaining);
            match num_bytes {
                num_bytes if num_bytes > 0 => {
                    offset += num_bytes as usize;
                }
                0 => break, // EOF reached
                _ => {
                    libc::close(pipe[0]);
                    libc::waitpid(pid, core::ptr::null_mut(), 0);
                    return Err(image_err());
                }
            }
        }
        libc::close(pipe[0]);
        libc::waitpid(pid, core::ptr::null_mut(), 0);

        if offset != buffer.len() {
            return Err(image_err()); // failed to read complete frame size
        }
    }
    Ok(())
}
