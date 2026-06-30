pub mod image_load;
pub mod remove_self;
pub mod shm;
pub mod state;
pub mod wayland;

pub enum AppError {
    FileOpenError,
    ImageDecodeError,
    SHMError,
    WaylandError,
}
impl AppError {
    pub fn message(&self) -> &'static [u8] {
        match self {
            Self::FileOpenError => b"file open error\n",
            Self::ImageDecodeError => b"image decode error\n",
            Self::SHMError => b"shm error\n",
            Self::WaylandError => b"wayland error",
        }
    }
}
pub fn write_err(msg: &[u8]) {
    unsafe { libc::write(2, msg.as_ptr() as *const libc::c_void, msg.len()) };
}
pub fn write_u32_err(mut num: u32) {
    if num == 0 {
        write_err(b"0");
        return;
    }
    let mut buf = [0u8; 10]; // u32 max is 10 digits
    let mut idx = 10;
    while num > 0 {
        idx -= 1;
        buf[idx] = b'0' + (num % 10) as u8;
        num /= 10;
    }
    write_err(&buf[idx..10]);
}
pub fn write_out(msg: &[u8]) {
    unsafe { libc::write(1, msg.as_ptr() as *const _, msg.len()) };
}
pub fn file_err() -> AppError {
    return AppError::FileOpenError;
}
pub fn image_err() -> AppError {
    return AppError::ImageDecodeError;
}

pub fn read_u32(buf: &[u8], idx: usize) -> u32 {
    if idx + 4 <= buf.len() {
        let mut bytes = [0u8; 4];
        bytes.copy_from_slice(&buf[idx..idx + 4]);
        u32::from_ne_bytes(bytes)
    } else {
        0
    }
}

pub fn read_u16(buf: &[u8], idx: usize) -> u16 {
    if idx + 2 <= buf.len() {
        let mut bytes = [0u8; 2];
        bytes.copy_from_slice(&buf[idx..idx + 2]);
        u16::from_ne_bytes(bytes)
    } else {
        0
    }
}
