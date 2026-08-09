use wllib::error::SysError;

use crate::error::AppError;

#[link(name = "c")]
unsafe extern "C" {
  fn pow(base: f64, exponent: f64) -> f64;
  fn log(x: f64) -> f64;
}

fn powf(base: f64, exponent: f64) -> f64 {
  unsafe { pow(base, exponent) }
}

fn ln(x: f64) -> f64 {
  unsafe { log(x) }
}

// convert temperature in kelvin to rgb data
pub fn kelvin_to_rgb(kelvin: f64) -> (f64, f64, f64) {
  let kelvin = kelvin.clamp(1000.0, 40000.0);
  let temp = kelvin / 100.0;

  let r = if temp <= 66.0 {
    1.0
  } else {
    let r = (powf(329.698727446 * (temp - 60.0), -0.1332047592)) / 255.0;
    r.clamp(0.0, 1.0)
  };

  let g = if temp <= 66.0 {
    let g = (99.4708025861 * ln(temp) - 161.1195636025) / 255.0;
    g.clamp(0.0, 1.0)
  } else {
    let g = (powf(288.1221695283 * (temp - 60.0), -0.0755148492)) / 255.0;
    g.clamp(0.0, 1.0)
  };

  let b = if temp >= 66.0 {
    1.0
  } else if temp <= 19.0 {
    0.0
  } else {
    let b = (138.5177312231 * ln(temp - 10.0) - 305.0447927307) / 255.0;
    b.clamp(0.0, 1.0)
  };

  (r, g, b)
}

pub fn get_gamma_table_fd(size: usize, temp_kelvin: f64) -> Result<i32, AppError> {
  let (r_factor, g_factor, b_factor) = kelvin_to_rgb(temp_kelvin);
  unsafe {
    let fd = libc::memfd_create(c"rustemp-memfd".as_ptr(), libc::MFD_ALLOW_SEALING);
    if fd < 0 {
      return Err(AppError::Sys(SysError::last("memfd_create")));
    }

    // gamma tables contain three ramps (R, G, B), each with `size` elements of 16-bit values (2
    // bytes)
    let fd_size = size * 3 * 2;
    if libc::ftruncate(fd, fd_size as libc::off_t) < 0 {
      let err = SysError::last("ftruncate");
      libc::close(fd);
      return Err(AppError::Sys(err));
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
      let err = SysError::last("mmap");
      libc::close(fd);
      return Err(AppError::Sys(err));
    }

    let slice = core::slice::from_raw_parts_mut(ptr as *mut u16, size * 3);

    // generate gamma curves scaled by the RGB color temperature factors
    for i in 0..size {
      let t = i as f64 / (size - 1) as f64;
      // red
      slice[i] = (t * r_factor * 65535.0) as u16;
      // greerg
      slice[size + i] = (t * g_factor * 65535.0) as u16;
      // blue
      slice[2 * size + i] = (t * b_factor * 65535.0) as u16;
    }

    libc::munmap(ptr, fd_size);
    Ok(fd)
  }
}
