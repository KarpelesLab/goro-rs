//! Directory listings in the order readdir(3) returns them (PHP's
//! opendir()/readdir(), DirectoryIterator and scandir(SCANDIR_SORT_NONE) keep
//! that order; "." and ".." come wherever the filesystem puts them).

/// d_type values (DT_UNKNOWN when the filesystem does not report one).
pub const DT_UNKNOWN: u8 = 0;
pub const DT_DIR: u8 = 4;
pub const DT_REG: u8 = 8;
pub const DT_LNK: u8 = 10;

/// Every entry of `path` (names as raw bytes, with their d_type), "." and ".."
/// included, in readdir order.
#[inline(never)]
pub fn readdir_entries(path: &[u8]) -> std::io::Result<Vec<(Vec<u8>, u8)>> {
    let c = std::ffi::CString::new(path)
        .map_err(|_| std::io::Error::from_raw_os_error(libc::ENOENT))?;
    // SAFETY: opendir/readdir/closedir on a handle owned by this function.
    unsafe {
        let d = libc::opendir(c.as_ptr());
        if d.is_null() {
            return Err(std::io::Error::last_os_error());
        }
        let mut out = Vec::new();
        loop {
            let e = libc::readdir(d);
            if e.is_null() {
                break;
            }
            let name = std::ffi::CStr::from_ptr((*e).d_name.as_ptr());
            out.push((name.to_bytes().to_vec(), (*e).d_type));
        }
        libc::closedir(d);
        Ok(out)
    }
}

/// The names only, lossily decoded (callers that keep `String` names).
#[inline(never)]
pub fn readdir_names(path: &str) -> std::io::Result<Vec<String>> {
    Ok(readdir_entries(path.as_bytes())?
        .into_iter()
        .map(|(n, _)| crate::string::lossy(&n).into_owned())
        .collect())
}
