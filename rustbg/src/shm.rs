use crate::state::State;
use crate::{AppError, image_load};

// create a memory-backed file, aka memfd, containing the scaled bgra pixel data and return its file
// descriptor to the compositor. the memfd is mmap'ed to our virtual address space, and we pass it
// as a byte slice to our image loading function. we then unmap it and send the fd back. the wayland
// side turns it into a buffer
pub fn get_image_fd(out_width: u32, out_height: u32, state: &mut State) -> Result<i32, AppError> {
    if state.config.image_path.is_none() {
        return Err(AppError::FileOpenError);
    };
    let stride = out_width * 4; // 4 bytes per pixel (BGRA)
    let size = (stride * out_height) as usize;

    // create an anonymous, in-memory file.
    let fd = unsafe {
        libc::memfd_create(
            c"rustbg-wayland-shm".as_ptr() as *const libc::c_char,
            libc::MFD_CLOEXEC | libc::MFD_ALLOW_SEALING | libc::MFD_NOEXEC_SEAL,
        )
    };
    if fd < 0 {
        return Err(AppError::FileOpenError);
    }

    unsafe {
        // set the file's size, equal to our output size
        if { libc::ftruncate(fd, size as libc::off_t) } < 0 {
            libc::close(fd);
            return Err(AppError::FileOpenError);
        }

        // map the in-memory file into our address space
        let mmap_ptr = libc::mmap(
            core::ptr::null_mut(),
            size,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_SHARED,
            fd,
            0,
        );
        if mmap_ptr == libc::MAP_FAILED {
            libc::close(fd);
            return Err(AppError::SHMError);
        }

        let mmap_slice = core::slice::from_raw_parts_mut(mmap_ptr as *mut u8, size);

        // spawn ffmpeg, fill the mmap'd region directly
        image_load::load_and_scale(out_width, out_height, mmap_slice, state)?;

        // unmap the address space. the data remains in the memfd
        libc::munmap(mmap_ptr, size);
    };

    Ok(fd)
}
