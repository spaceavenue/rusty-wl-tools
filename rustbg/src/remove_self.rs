// convert a hex byte slice to usize, for parsing memory addresses
fn hex_bytes_to_usize(bytes: &[u8]) -> usize {
  let mut value = 0;
  for &b in bytes {
    value = (value << 4)
      | match b {
        b'0'..=b'9' => (b - b'0') as usize,
        b'a'..=b'f' => (b - b'a' + 10) as usize,
        b'A'..=b'F' => (b - b'A' + 10) as usize,
        _ => return value,
      };
  }
  value
}

// basically tells the kernel to take our pages and page them out to swap
// this can be acknowledged or ignored, we can only really suggest
// (almost like we're advising it- oooohhhhh)
pub fn evict_self_from_ram() {
  const MADV_PAGEOUT: libc::c_int = 21;

  let mut buf = [0u8; 4096];
  // read the process' memory map
  let fd = unsafe { libc::open(c"/proc/self/maps".as_ptr(), libc::O_RDONLY) };
  if fd < 0 {
    return;
  }
  let num_bytes = unsafe { libc::read(fd, buf.as_mut_ptr() as *mut _, buf.len()) };
  unsafe {
    libc::close(fd);
  }

  // parse lines in `/proc/self/maps` to find mapping ranges
  for line in buf[..num_bytes as usize].split(|&b| b == b'\n') {
    if line.is_empty() {
      continue;
    }

    let mut parts = line.split(|&b| b == b'-');
    let (Some(start_bytes), Some(end_bytes)) = (
      parts.next(),
      parts.next().and_then(|o| o.split(|&b| b == b' ').next()),
    ) else {
      return;
    };
    let start = hex_bytes_to_usize(start_bytes);
    let end = hex_bytes_to_usize(end_bytes);

    (start < end).then(|| unsafe {
      libc::madvise(start as *mut libc::c_void, end - start, MADV_PAGEOUT);
    });
  }
}
