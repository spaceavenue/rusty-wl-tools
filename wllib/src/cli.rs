//! `getopt_long` bindings.
//!
//! `optarg`/`optind` already needed hand-declaring directly (as raw `extern "C" { static ... }`
//! items) in every binary that uses plain `getopt`, rather than being reliably available through
//! the `libc` crate for the musl target. Declaring `getopt_long` and its `option` struct the same
//! way sidesteps that same uncertainty for this function, rather than hoping it happens to be
//! bound.
//!
//! `getopt_long` is used with `flag` always `NULL` and `val` set to the short-option-equivalent
//! character, so it returns the exact same code for `-f` and `--fill` alike. Thus, a binary's
//! existing `match c as u8 as char { ... }` block handles both forms completely unchanged and only
//! the call itself and a small options table need to be added.

#[repr(C)]
pub struct LongOption {
  pub name: *const libc::c_char,
  pub has_arg: libc::c_int,
  pub flag: *mut libc::c_int,
  pub val: libc::c_int,
}
impl LongOption {
  pub const fn new(name: &str, has_arg: libc::c_int, val: char) -> Self {
    Self {
      name: name.as_ptr() as _,
      has_arg,
      flag: core::ptr::null_mut(),
      val: val as _,
    }
  }
}

pub const NO_ARGUMENT: libc::c_int = 0;
pub const REQUIRED_ARGUMENT: libc::c_int = 1;
pub const OPTIONAL_ARGUMENT: libc::c_int = 2;

/// Every `LongOption` array passed to `getopt_long` must end with an all-zero entry marking where
/// the array ends, since the array itself carries no separate length.
pub const LONG_OPTION_TERMINATOR: LongOption = LongOption {
  name: core::ptr::null(),
  has_arg: 0,
  flag: core::ptr::null_mut(),
  val: 0,
};

unsafe extern "C" {
  pub fn getopt_long(
    argc: libc::c_int,
    argv: *const *mut libc::c_char,
    optstring: *const libc::c_char,
    longopts: *const LongOption,
    longindex: *mut libc::c_int,
  ) -> libc::c_int;
}
