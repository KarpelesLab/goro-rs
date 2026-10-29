//! The files save handler (PHP manual: session.save_path): one
//! `sess_ID` file per session in the save directory (or N levels of
//! subdirectories named after the ID's first characters, with
//! "N;[MODE;]/path"), locked with flock() while the session is open,
//! created with MODE (default 0600) under the umask.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};

use super::{st, warn};
use crate::vm::{R, Vm};

#[derive(Default)]
pub(crate) struct Files {
    /// The save directory, once opened.
    dir: Option<PathBuf>,
    depth: usize,
    mode: u32,
    /// The open, locked session file and its ID.
    file: Option<(Vec<u8>, File)>,
}

fn temp_dir(vm: &Vm) -> PathBuf {
    if let Some(v) = vm.get_ini(b"sys_temp_dir")
        && !v.is_empty()
    {
        return PathBuf::from(std::ffi::OsStr::from_bytes(&v));
    }
    std::env::var_os("TMPDIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"))
}

/// Parses "N;MODE;/path" (PHP manual: session.save_path).
pub(super) fn open(vm: &mut Vm, save_path: &[u8]) -> R<bool> {
    let parts: Vec<&[u8]> = save_path.split(|&c| c == b';').collect();
    let (depth, mode, dir) = match parts.len() {
        1 => (0, 0o600, parts[0]),
        2 => (
            goro_runtime::convert::str_to_int(parts[0]).max(0) as usize,
            0o600,
            parts[1],
        ),
        _ => {
            let m = std::str::from_utf8(parts[1])
                .ok()
                .and_then(|s| u32::from_str_radix(s.trim(), 8).ok())
                .unwrap_or(0o600);
            (
                goro_runtime::convert::str_to_int(parts[0]).max(0) as usize,
                m,
                *parts.last().unwrap(),
            )
        }
    };
    let dir_b = if dir.is_empty() {
        temp_dir(vm).as_os_str().as_bytes().to_vec()
    } else {
        dir.to_vec()
    };
    if !vm.basedir_allows(&dir_b) {
        let bd = vm
            .get_ini(b"open_basedir")
            .map(|v| v.to_string_lossy())
            .unwrap_or_default();
        warn(
            vm,
            &format!(
                "open_basedir restriction in effect. File({}) is not within the allowed path(s): ({})",
                String::from_utf8_lossy(&dir_b),
                bd
            ),
        )?;
        return Ok(false);
    }
    let dir = if dir.is_empty() {
        temp_dir(vm)
    } else {
        let p = PathBuf::from(std::ffi::OsStr::from_bytes(dir));
        if p.is_absolute() { p } else { vm.cwd.join(p) }
    };
    let f = &mut st(vm).files;
    f.dir = Some(dir);
    f.depth = depth;
    f.mode = mode;
    Ok(true)
}

pub(super) fn close(vm: &mut Vm) -> bool {
    let f = &mut st(vm).files;
    // Closing the file releases its lock.
    f.file = None;
    true
}

/// The file of session `id`; None (reported) when there is none.
fn path_of(vm: &mut Vm, id: &[u8]) -> R<Option<PathBuf>> {
    let f = &st(vm).files;
    let Some(dir) = f.dir.clone() else {
        return Ok(None);
    };
    let depth = f.depth;
    if !super::valid_id(id) {
        warn(
            vm,
            "Session ID is too long or contains illegal characters. Only the A-Z, a-z, 0-9, \"-\", and \",\" characters are allowed",
        )?;
        return Ok(None);
    }
    if id.len() <= depth || dir.as_os_str().len() + id.len() + depth * 2 + 6 >= 4096 {
        warn(
            vm,
            "Failed to create session data file path. Too short session ID, invalid save_path or path length exceeds 4096 characters",
        )?;
        return Ok(None);
    }
    let mut p = dir;
    for &c in &id[..depth] {
        p.push(std::ffi::OsStr::from_bytes(&[c]));
    }
    let mut name = b"sess_".to_vec();
    name.extend_from_slice(id);
    p.push(std::ffi::OsStr::from_bytes(&name));
    Ok(Some(p))
}

/// Opens and locks the file of `id` (kept open until close()).
fn open_file(vm: &mut Vm, id: &[u8]) -> R<bool> {
    if st(vm).files.file.as_ref().is_some_and(|(k, _)| k == id) {
        return Ok(true);
    }
    st(vm).files.file = None;
    let Some(p) = path_of(vm, id)? else {
        return Ok(false);
    };
    let mode = st(vm).files.mode;
    let r = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(mode)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&p);
    match r {
        Ok(file) => {
            // SAFETY: flock on an open descriptor.
            unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) };
            st(vm).files.file = Some((id.to_vec(), file));
            Ok(true)
        }
        Err(e) => {
            let code = e.raw_os_error().unwrap_or(0);
            warn(
                vm,
                &format!(
                    "open({}, O_RDWR) failed: {} ({})",
                    p.display(),
                    crate::builtins::exec::strerror(code),
                    code
                ),
            )?;
            Ok(false)
        }
    }
}

pub(super) fn read(vm: &mut Vm, id: &[u8]) -> R<Option<Vec<u8>>> {
    if !open_file(vm, id)? {
        return Ok(None);
    }
    let Some((_, f)) = st(vm).files.file.as_mut() else {
        return Ok(None);
    };
    let mut data = Vec::new();
    let _ = f.seek(SeekFrom::Start(0));
    if f.read_to_end(&mut data).is_err() {
        return Ok(None);
    }
    Ok(Some(data))
}

pub(super) fn write(vm: &mut Vm, id: &[u8], data: &[u8]) -> R<bool> {
    if !open_file(vm, id)? {
        return Ok(false);
    }
    let Some((_, f)) = st(vm).files.file.as_mut() else {
        return Ok(false);
    };
    let ok = f.set_len(0).is_ok()
        && f.seek(SeekFrom::Start(0)).is_ok()
        && f.write_all(data).is_ok();
    if !ok {
        let e = std::io::Error::last_os_error();
        let code = e.raw_os_error().unwrap_or(0);
        warn(
            vm,
            &format!(
                "write failed: {} ({})",
                crate::builtins::exec::strerror(code),
                code
            ),
        )?;
    }
    Ok(ok)
}

/// Lazy write: the file's modification time is updated.
pub(super) fn update_timestamp(vm: &mut Vm, id: &[u8]) -> bool {
    let Ok(Some(p)) = path_of(vm, id) else {
        return false;
    };
    let c = std::ffi::CString::new(p.as_os_str().as_bytes()).unwrap_or_default();
    // SAFETY: a NUL-terminated path; NULL times means now.
    unsafe { libc::utime(c.as_ptr(), std::ptr::null()) == 0 }
}

pub(super) fn destroy(vm: &mut Vm, id: &[u8]) -> bool {
    if st(vm).files.file.as_ref().is_some_and(|(k, _)| k == id) {
        st(vm).files.file = None;
    }
    let Ok(Some(p)) = path_of(vm, id) else {
        return false;
    };
    match std::fs::remove_file(&p) {
        Ok(()) => true,
        Err(e) => e.kind() == std::io::ErrorKind::NotFound,
    }
}

/// Whether session `id` has a file (use_strict_mode).
pub(super) fn exists(vm: &mut Vm, id: &[u8]) -> bool {
    if !super::valid_id(id) {
        return false;
    }
    let f = &st(vm).files;
    let Some(dir) = f.dir.clone() else {
        return false;
    };
    let depth = f.depth;
    if id.len() <= depth {
        return false;
    }
    let mut p = dir;
    for &c in &id[..depth] {
        p.push(std::ffi::OsStr::from_bytes(&[c]));
    }
    let mut name = b"sess_".to_vec();
    name.extend_from_slice(id);
    p.push(std::ffi::OsStr::from_bytes(&name));
    p.exists()
}

/// Deletes the session files not modified for `maxlifetime` seconds
/// (only in a flat save directory: PHP manual, session.save_path).
pub(super) fn gc(vm: &mut Vm, maxlifetime: i64) -> Option<i64> {
    let f = &st(vm).files;
    let dir = f.dir.clone()?;
    if f.depth > 0 {
        return Some(0);
    }
    let now = std::time::SystemTime::now();
    let mut n = 0;
    let Ok(rd) = std::fs::read_dir(Path::new(&dir)) else {
        return Some(0);
    };
    for e in rd.flatten() {
        if !e.file_name().as_bytes().starts_with(b"sess_") {
            continue;
        }
        let Ok(md) = e.metadata() else { continue };
        let Ok(m) = md.modified() else { continue };
        let age = now.duration_since(m).map(|d| d.as_secs() as i64).unwrap_or(0);
        if age > maxlifetime && std::fs::remove_file(e.path()).is_ok() {
            n += 1;
        }
    }
    Some(n)
}
