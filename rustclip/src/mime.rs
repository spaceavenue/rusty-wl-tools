// Fixed-capacity mime-type storage and matching.
//
// Each mime type a selection offers arrives as the argument of its own separate `offer` event. So,
// it has to be copied out into owned storage the moment it arrives since the event payload it came
// from doesn't outlive the `dispatch_once` call that delivered it. Storage is a fixed
// `[u8; MAX_MIME_LEN]` per entry because no allocator.

use wllib::fmt_lite::StringOnStack;

pub const MAX_MIME_LEN: usize = 128;
pub const MAX_MIME_TYPES: usize = 128;

pub type MimeType = StringOnStack<MAX_MIME_LEN>;

// Preference order used when the caller doesn't request a specific mime type: the first of
// these actually offered by the current selection wins. Also the set `wl-copy` offers by
// default (unless `-t` pins it to exactly one), so two copies of this tool interoperate cleanly
// even without relying on any particular preference matching some other tool's request exactly.
pub const PREFERRED_TEXT_MIMES: [&str; 4] = [
  "text/plain;charset=utf-8",
  "text/plain",
  "UTF8_STRING",
  "STRING",
];

// Pick the best matching mime type out of `available[..available_len]`. Prefers `wanted` if
// given and actually present; otherwise walks [`PREFERRED_TEXT_MIMES`] in order; otherwise
// falls back to whatever was offered first.
pub fn pick_mime(
  available: &[MimeType; MAX_MIME_TYPES],
  available_len: usize,
  wanted: Option<&str>,
) -> Option<MimeType> {
  let slice = &available[..available_len];
  if let Some(w) = wanted {
    return slice.iter().find(|m| **m == *w).copied();
  }
  for pref in PREFERRED_TEXT_MIMES.iter() {
    if let Some(m) = slice.iter().find(|m| m == pref) {
      return Some(*m);
    }
  }
  slice.first().copied()
}
