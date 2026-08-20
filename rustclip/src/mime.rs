// Fixed-capacity mime-type storage and matching.
//
// Each mime type a selection offers arrives as the argument of its own separate `offer` event. So,
// it has to be copied out into owned storage the moment it arrives since the event payload it came
// from doesn't outlive the `dispatch_once` call that delivered it. Storage is a fixed
// `[u8; MAX_MIME_LEN]` per entry because no allocator.
//
// Mime inference has two independent halves:
// - Content-based (wl-copy, reading stdin): magic-byte sniffing first (`sniff_mime`), then
//   `xdg-mime query filetype` as a fallback, pointed at `/proc/self/fd/<n>` of the memfd holding
//   the content. xdg-mime just needs a path it can `stat`/`open`, and a procfs fd entry is one.
// - Name-based (wl-paste, writing to a redirected file): a small built-in extension table, not a
//   port of the freedesktop shared-mime-info glob database — see `EXTENSION_MIME_TABLE`.
//
// Offer classification and request selection (`classify_offer_types`/`mime_type_to_request`)
// prioritize explicit requested types, then inferred types, then UTF-8 / plain text fallbacks —
// matching wl-clipboard's behavior of the same name.

use wllib::fmt_lite::StringOnStack;

pub const MAX_MIME_LEN: usize = 128;
pub const MAX_MIME_TYPES: usize = 128;

pub type MimeType = StringOnStack<MAX_MIME_LEN>;

/// Generic plain text formats offered by wl-copy by default or alongside text MIME types.
pub const GENERIC_TEXT_OFFERS: [&str; 5] = [
  "text/plain",
  "text/plain;charset=utf-8",
  "TEXT",
  "STRING",
  "UTF8_STRING",
];

/// A heuristic to detect if a MIME type is plain text or text-compatible.
pub fn is_text_mime(mime: &str) -> bool {
  // Types that explicitly declare they're textual
  mime.starts_with("text/")
    || mime == "TEXT"
    || mime == "STRING"
    || mime == "UTF8_STRING"
    // Common script and markup types
    || mime.contains("json")
    || mime.ends_with("script")
    || mime.ends_with("xml")
    || mime.ends_with("yaml")
    || mime.ends_with("csv")
    || mime.ends_with("ini")
    // Special-case PGP and SSH keys
    || mime.contains("application/vnd.ms-publisher")
    || mime.ends_with("pgp-keys")
}

/// Infer common MIME types from the initial bytes (magic numbers) of the data.
pub fn sniff_mime(buf: &[u8]) -> Option<&'static str> {
  // PNG: \x89PNG\r\n\x1a\n
  if buf.len() >= 8 && &buf[..8] == b"\x89PNG\r\n\x1a\n" {
    return Some("image/png");
  }
  // JPEG: \xFF\xD8\xFF
  if buf.len() >= 3 && &buf[..3] == b"\xff\xd8\xff" {
    return Some("image/jpeg");
  }
  // GIF: GIF87a or GIF89a
  if buf.len() >= 6 && (&buf[..6] == b"GIF87a" || &buf[..6] == b"GIF89a") {
    return Some("image/gif");
  }
  // WebP: RIFF....WEBP
  if buf.len() >= 12 && &buf[..4] == b"RIFF" && &buf[8..12] == b"WEBP" {
    return Some("image/webp");
  }
  // PDF: %PDF-
  if buf.len() >= 5 && &buf[..5] == b"%PDF-" {
    return Some("application/pdf");
  }
  // SVG / XML
  let trimmed = buf.trim_ascii_start();
  if trimmed.starts_with(b"<?xml") || trimmed.starts_with(b"<svg") {
    return Some("image/svg+xml");
  }
  None
}

/// Return the path of the file linked to a file descriptor.
pub fn path_for_fd(fd: libc::c_int) -> Option<StringOnStack<256>> {
  let mut proc_path = StringOnStack::<64>::new();
  proc_path.push_str("/proc/self/fd/").push_i32(fd);

  let mut buf = [0u8; 256];
  let n = unsafe {
    libc::readlink(
      proc_path.as_ptr(),
      buf.as_mut_ptr() as *mut libc::c_char,
      buf.len() - 1,
    )
  };
  if n > 0 {
    let mut res = StringOnStack::<256>::new();
    if let Ok(s) = core::str::from_utf8(&buf[..n as usize]) {
      res.push_str(s);
      return Some(res);
    }
  }
  None
}

/// Common filename extension -> MIME type mappings, covering the formats most likely to show up
/// in a `wl-paste > file.ext` redirection. Will expand as needed.
const EXTENSION_MIME_TABLE: &[(&str, &str)] = &[
  ("png", "image/png"),
  ("jpg", "image/jpeg"),
  ("jpeg", "image/jpeg"),
  ("gif", "image/gif"),
  ("bmp", "image/bmp"),
  ("webp", "image/webp"),
  ("ico", "image/x-icon"),
  ("tif", "image/tiff"),
  ("tiff", "image/tiff"),
  ("svg", "image/svg+xml"),
  ("pdf", "application/pdf"),
  ("txt", "text/plain"),
  ("md", "text/markdown"),
  ("json", "application/json"),
  ("html", "text/html"),
  ("htm", "text/html"),
  ("xml", "text/xml"),
  ("csv", "text/csv"),
  ("yaml", "text/yaml"),
  ("yml", "text/yaml"),
  ("zip", "application/zip"),
  ("tar", "application/x-tar"),
  ("gz", "application/gzip"),
  ("mp3", "audio/mpeg"),
  ("mp4", "video/mp4"),
  ("wav", "audio/wav"),
  ("ogg", "audio/ogg"),
];

/// Infer a MIME type from a file path's extension via `EXTENSION_MIME_TABLE`. Returns `None` for
/// extensions it doesn't recognize.
pub fn infer_mime_type_from_name(file_path: &str) -> Option<MimeType> {
  let filename = match file_path.rfind('/') {
    Some(idx) => &file_path[idx + 1..],
    None => file_path,
  };
  let ext = match filename.rfind('.') {
    Some(idx) if idx > 0 && idx + 1 < filename.len() => &filename[idx + 1..],
    _ => return None,
  };

  EXTENSION_MIME_TABLE
    .iter()
    .find(|(e, _)| e.eq_ignore_ascii_case(ext))
    .map(|(_, mime)| MimeType::from(*mime))
}

/// Infer a MIME type for an open fd's content: magic-byte sniffing first, then `xdg-mime query
/// filetype` against the fd's own `/proc/self/fd` entry as a fallback for content sniffing doesn't
/// recognize.
pub fn infer_mime_type_from_fd(fd: libc::c_int) -> Option<MimeType> {
  let mut header = [0u8; 64];
  unsafe { libc::lseek(fd, 0, libc::SEEK_SET) };
  let n = unsafe { libc::read(fd, header.as_mut_ptr() as *mut _, header.len()) };
  unsafe { libc::lseek(fd, 0, libc::SEEK_SET) };
  if n > 0
    && let Some(sniffed) = sniff_mime(&header[..n as usize])
  {
    return Some(MimeType::from(sniffed));
  }
  query_xdg_mime_for_fd(fd)
}

fn query_xdg_mime_for_fd(fd: libc::c_int) -> Option<MimeType> {
  let mut pipe_fds = [0i32; 2];
  if unsafe { libc::pipe2(pipe_fds.as_mut_ptr(), libc::O_CLOEXEC) } != 0 {
    return None;
  }

  let pid = unsafe { libc::fork() };
  if pid < 0 {
    unsafe {
      libc::close(pipe_fds[0]);
      libc::close(pipe_fds[1]);
    }
    return None;
  }

  if pid == 0 {
    unsafe {
      libc::dup2(pipe_fds[1], 1);
      libc::close(pipe_fds[0]);
      libc::close(pipe_fds[1]);

      let devnull = libc::open(c"/dev/null".as_ptr(), libc::O_RDONLY);
      if devnull >= 0 {
        libc::dup2(devnull, 0);
        libc::close(devnull);
      } else {
        libc::close(0);
      }

      libc::signal(libc::SIGHUP, libc::SIG_DFL);
      libc::signal(libc::SIGPIPE, libc::SIG_DFL);

      // clear FD_CLOEXEC on this process's own copy of `fd` so it survives the exec below and
      // `/proc/self/fd/<fd>` still resolves inside the exec'd xdg-mime process.
      libc::fcntl(fd, libc::F_SETFD, 0);
      let mut path = StringOnStack::<40>::new();
      path.push_str("/proc/self/fd/").push_i32(fd);

      libc::execlp(
        c"xdg-mime".as_ptr(),
        c"xdg-mime".as_ptr(),
        c"query".as_ptr(),
        c"filetype".as_ptr(),
        path.as_ptr(),
        core::ptr::null::<libc::c_char>(),
      );
      libc::_exit(1);
    }
  }

  unsafe { libc::close(pipe_fds[1]) };

  let mut wstatus = 0;
  unsafe { libc::waitpid(pid, &mut wstatus, 0) };
  if !libc::WIFEXITED(wstatus) || libc::WEXITSTATUS(wstatus) != 0 {
    unsafe { libc::close(pipe_fds[0]) };
    return None;
  }

  let mut buf = [0u8; 256];
  let n = unsafe {
    libc::read(
      pipe_fds[0],
      buf.as_mut_ptr() as *mut libc::c_void,
      buf.len(),
    )
  };
  unsafe { libc::close(pipe_fds[0]) };

  if n <= 0 {
    return None;
  }

  let slice = &buf[..n as usize];
  let trimmed = match core::str::from_utf8(slice) {
    Ok(s) => s.trim_ascii(),
    Err(e) => match core::str::from_utf8(&slice[..e.valid_up_to()]) {
      Ok(s) => s.trim_ascii(),
      Err(_) => return None,
    },
  };

  // `file`/`xdg-mime` sometimes print a failure message to stdout instead of stderr (e.g. `file`
  // printing "cannot open `...' (No such file or directory)" with exit status 0 when given a path
  // it can't actually stat, which happens for every memfd since `xdg-mime` resolves
  // `/proc/self/fd/<n>` to the memfd's cosmetic, non-openable "/memfd:name (deleted)" name — a
  // string that itself contains slashes, so a bare `contains('/')` check isn't enough to reject
  // it). A real mime type is exactly `type/subtype` with no whitespace and non-empty halves, so
  // require that shape instead of trusting exit status alone.
  let looks_like_mime = !trimmed.contains(char::is_whitespace)
    && match trimmed.split_once('/') {
      Some((ty, subty)) => !ty.is_empty() && !subty.is_empty() && !subty.contains('/'),
      None => false,
    };
  if trimmed.is_empty() || trimmed.starts_with("inode/") || !looks_like_mime {
    return None;
  }

  Some(MimeType::from(trimmed))
}

#[derive(Default)]
pub struct ClassifiedTypes {
  pub explicit_available: bool,
  pub inferred_available: bool,
  pub plain_text_utf8_available: bool,
  pub plain_text_available: bool,
  pub has_sensitive_hint: bool,
  pub having_explicit_as_prefix: Option<MimeType>,
  pub any_text: Option<MimeType>,
  pub any: Option<MimeType>,
}

/// Classify offer types.
pub fn classify_offer_types(
  available: &[MimeType],
  wanted_explicit: Option<&str>,
  inferred: Option<&str>,
) -> ClassifiedTypes {
  let mut types = ClassifiedTypes::default();

  for m in available {
    let s = m.as_str();
    if let Some(exp) = wanted_explicit {
      if s == exp {
        types.explicit_available = true;
      }
      if types.having_explicit_as_prefix.is_none() && s.starts_with(exp) {
        types.having_explicit_as_prefix = Some(*m);
      }
    }
    if inferred == Some(s) {
      types.inferred_available = true;
    }
    if s == "text/plain;charset=utf-8" {
      types.plain_text_utf8_available = true;
    }
    if s == "text/plain" {
      types.plain_text_available = true;
    }
    if types.any_text.is_none() && is_text_mime(s) {
      types.any_text = Some(*m);
    }
    if types.any.is_none() {
      types.any = Some(*m);
    }
    if s == "x-kde-passwordManagerHint" {
      types.has_sensitive_hint = true;
    }
  }

  types
}

/// Select the best matching MIME type to request.
pub fn mime_type_to_request(
  types: &ClassifiedTypes,
  wanted_explicit: Option<&str>,
  inferred: Option<&str>,
) -> Option<MimeType> {
  if let Some(exp) = wanted_explicit {
    if exp == "text" {
      if types.plain_text_utf8_available {
        return Some(MimeType::from("text/plain;charset=utf-8"));
      }
      if types.plain_text_available {
        return Some(MimeType::from("text/plain"));
      }
      if let Some(any_txt) = types.any_text {
        return Some(any_txt);
      }
    } else if exp.contains('/') {
      // a fully qualified mime type only ever matches exactly, no prefix fallback.
      if types.explicit_available {
        return Some(MimeType::from(exp));
      }
    } else {
      if types.explicit_available {
        return Some(MimeType::from(exp));
      }
      if let Some(prefixed) = types.having_explicit_as_prefix {
        return Some(prefixed);
      }
    }
  } else {
    match inferred {
      None => {
        if types.plain_text_utf8_available {
          return Some(MimeType::from("text/plain;charset=utf-8"));
        }
        if types.plain_text_available {
          return Some(MimeType::from("text/plain"));
        }
        if let Some(any_txt) = types.any_text {
          return Some(any_txt);
        }
        if let Some(any) = types.any {
          return Some(any);
        }
      }
      Some(inf) if is_text_mime(inf) => {
        if types.inferred_available {
          return Some(MimeType::from(inf));
        }
        if types.plain_text_utf8_available {
          return Some(MimeType::from("text/plain;charset=utf-8"));
        }
        if types.plain_text_available {
          return Some(MimeType::from("text/plain"));
        }
        if let Some(any_txt) = types.any_text {
          return Some(any_txt);
        }
      }
      Some(inf) => {
        if types.inferred_available {
          return Some(MimeType::from(inf));
        }
      }
    }
  }

  None
}
