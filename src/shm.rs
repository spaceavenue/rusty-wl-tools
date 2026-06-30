use crate::state::State;
use crate::{AppError, image_load};

// convert temperature in kelvin to rgb data
pub fn kelvin_to_rgb(kelvin: f64) -> (f64, f64, f64) {
    let kelvin = kelvin.clamp(1000.0, 40000.0);
    let temp = kelvin / 100.0;

    let r = if temp <= 66.0 {
        1.0
    } else {
        let r = (329.698727446 * (temp - 60.0).powf(-0.1332047592)) / 255.0;
        r.clamp(0.0, 1.0)
    };

    let g = if temp <= 66.0 {
        let g = (99.4708025861 * temp.ln() - 161.1195636025) / 255.0;
        g.clamp(0.0, 1.0)
    } else {
        let g = (288.1221695283 * (temp - 60.0).powf(-0.0755148492)) / 255.0;
        g.clamp(0.0, 1.0)
    };

    let b = if temp >= 66.0 {
        1.0
    } else if temp <= 19.0 {
        0.0
    } else {
        let b = (138.5177312231 * (temp - 10.0).ln() - 305.0447927307) / 255.0;
        b.clamp(0.0, 1.0)
    };

    (r, g, b)
}

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

// same thing as above, this time with calculated gamma table ramps matching the target color
// temperature. theres also no shm or buffer involved with this on the wayland side
pub fn get_gamma_table_fd(size: usize, temp_kelvin: f64) -> Result<i32, AppError> {
    let (r_factor, g_factor, b_factor) = kelvin_to_rgb(temp_kelvin);
    unsafe {
        let fd = libc::memfd_create(c"rustemp-memfd".as_ptr(), libc::MFD_ALLOW_SEALING);
        if fd < 0 {
            return Err(AppError::FileOpenError);
        }

        // gamma tables contain three ramps (R, G, B), each with `size` elements of 16-bit values (2
        // bytes)
        let fd_size = size * 3 * 2;
        if libc::ftruncate(fd, fd_size as libc::off_t) < 0 {
            libc::close(fd);
            return Err(AppError::FileOpenError);
        }

        // map the file to fill it
        let ptr = libc::mmap(
            core::ptr::null_mut(),
            fd_size,
            libc::PROT_WRITE,
            libc::MAP_SHARED,
            fd,
            0,
        );
        if ptr == libc::MAP_FAILED {
            libc::close(fd);
            return Err(AppError::SHMError);
        }

        let slice = core::slice::from_raw_parts_mut(ptr as *mut u16, size * 3);

        // generate gamma curves scaled by the RGB color temperature factors
        for i in 0..size {
            let t = i as f64 / (size - 1) as f64;
            slice[i] = (t * r_factor * 65535.0) as u16; // red
            slice[size + i] = (t * g_factor * 65535.0) as u16; // greerg
            slice[2 * size + i] = (t * b_factor * 65535.0) as u16; // blue
        }

        libc::munmap(ptr, fd_size);
        Ok(fd)
    }
}
