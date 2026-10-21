//! The "files" save handler (ext/session/mod_files.c).

use std::ffi::CString;
use std::fs::File;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::os::unix::io::AsRawFd;

use goro_core::vm::Vm;

use crate::{err, err_plain, ps, strtol, valid_key, E_NOTICE, E_WARNING};

const FILE_PREFIX: &[u8] = b"sess_";
const MAXPATHLEN: usize = 4096;

pub(crate) struct FilesData {
    basedir: Vec<u8>,
    dirdepth: usize,
    filemode: u32,
    fd: Option<File>,
    last_key: Option<Vec<u8>>,
    st_size: u64,
}

fn strerror(code: i32) -> String {
    unsafe {
        let p = libc::strerror(code);
        if p.is_null() {
            return format!("Unknown error {}", code);
        }
        std::ffi::CStr::from_ptr(p).to_string_lossy().into_owned()
    }
}

fn lossy(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

fn path_create(data: &FilesData, key: &[u8]) -> Option<Vec<u8>> {
    if key.len() <= data.dirdepth
        || MAXPATHLEN < data.basedir.len() + 2 * data.dirdepth + key.len() + 5 + FILE_PREFIX.len() + 1
    {
        return None;
    }
    let mut buf = data.basedir.clone();
    buf.push(b'/');
    for &c in key.iter().take(data.dirdepth) {
        buf.push(c);
        buf.push(b'/');
    }
    buf.extend_from_slice(FILE_PREFIX);
    buf.extend_from_slice(key);
    Some(buf)
}

fn take() -> Option<FilesData> {
    ps(|p| p.mod_data.take())
}

fn put(d: FilesData) {
    ps(|p| p.mod_data = Some(d));
}

/// ps_files_open
fn files_open(vm: &mut Vm, data: &mut FilesData, key: &[u8]) {
    if data.fd.is_some() && data.last_key.as_deref() == Some(key) {
        return;
    }
    data.last_key = None;
    data.fd = None;
    if !valid_key(key) {
        err(vm, E_WARNING, "Session ID is too long or contains illegal characters. Only the A-Z, a-z, 0-9, \"-\", and \",\" characters are allowed");
        return;
    }
    let path = match path_create(data, key) {
        Some(p) => p,
        None => {
            err(vm, E_WARNING, &format!(
                "Failed to create session data file path. Too short session ID, invalid save_path or path length exceeds {} characters",
                MAXPATHLEN
            ));
            return;
        }
    };
    data.last_key = Some(key.to_vec());
    let r = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .mode(data.filemode)
        .custom_flags(libc::O_NOFOLLOW)
        .open(lossy(&path));
    match r {
        Ok(f) => {
            if let Ok(md) = f.metadata() {
                let (uid, euid) = unsafe { (libc::getuid(), libc::geteuid()) };
                if md.uid() != 0 && md.uid() != uid && md.uid() != euid && uid != 0 {
                    err(vm, E_WARNING, "Session data file is not created by your uid");
                    return;
                }
            }
            unsafe {
                while libc::flock(f.as_raw_fd(), libc::LOCK_EX) == -1 && *libc::__errno_location() == libc::EINTR {}
            }
            data.fd = Some(f);
        }
        Err(e) => {
            let code = e.raw_os_error().unwrap_or(0);
            err(vm, E_WARNING, &format!("open({}, O_RDWR) failed: {} ({})", lossy(&path), strerror(code), code));
        }
    }
}

fn files_write(vm: &mut Vm, data: &mut FilesData, key: &[u8], val: &[u8]) -> bool {
    files_open(vm, data, key);
    let f = match &data.fd {
        Some(f) => f,
        None => return false,
    };
    if (val.len() as u64) < data.st_size {
        let _ = f.set_len(0);
    }
    use std::os::unix::fs::FileExt;
    match f.write_at(val, 0) {
        Ok(n) if n == val.len() => true,
        Ok(_) => {
            err(vm, E_WARNING, "Write wrote less bytes than requested");
            false
        }
        Err(e) => {
            let code = e.raw_os_error().unwrap_or(0);
            err(vm, E_WARNING, &format!("Write failed: {} ({})", strerror(code), code));
            false
        }
    }
}

/// PS_OPEN_FUNC(files)
pub(crate) fn open(vm: &mut Vm, save_path: &[u8]) -> bool {
    let tmp;
    let mut save_path = save_path;
    if save_path.is_empty() {
        tmp = temp_dir(vm);
        save_path = &tmp;
        if !crate::basedir_ok(vm, save_path) {
            return false;
        }
    }
    let mut argv: Vec<&[u8]> = Vec::new();
    let mut last = save_path;
    while let Some(p) = last.iter().position(|&c| c == b';') {
        argv.push(&last[..p]);
        last = &last[p + 1..];
        if argv.len() > 1 {
            break;
        }
    }
    argv.push(last);
    let mut dirdepth = 0usize;
    let mut filemode = 0o600u32;
    if argv.len() > 1 {
        let (v, _, overflow) = strtol(argv[0], 10);
        if overflow {
            err_plain(vm, E_WARNING, "The first parameter in session.save_path is invalid");
            return false;
        }
        dirdepth = v as usize;
    }
    if argv.len() > 2 {
        let (v, _, overflow) = strtol(argv[1], 8);
        if overflow || !(0..=0o7777).contains(&v) {
            err_plain(vm, E_WARNING, "The second parameter in session.save_path is invalid");
            return false;
        }
        filemode = v as u32;
    }
    let base = argv[argv.len() - 1];
    let base = match base.iter().position(|&c| c == 0) {
        Some(p) => &base[..p],
        None => base,
    };
    put(FilesData {
        basedir: base.to_vec(),
        dirdepth,
        filemode,
        fd: None,
        last_key: None,
        st_size: 0,
    });
    true
}

fn temp_dir(vm: &Vm) -> Vec<u8> {
    if let Some(v) = vm.constants.get(b"sys_temp_dir".as_slice()) {
        let s = v.to_php_string().as_bytes().to_vec();
        if !s.is_empty() {
            let mut s = s;
            while s.len() >= 2 && s.ends_with(b"/") {
                s.pop();
            }
            return s;
        }
    }
    if let Ok(t) = std::env::var("TMPDIR") {
        if !t.is_empty() {
            let mut t = t.into_bytes();
            while t.len() >= 2 && t.ends_with(b"/") {
                t.pop();
            }
            return t;
        }
    }
    b"/tmp".to_vec()
}

/// PS_CLOSE_FUNC(files)
pub(crate) fn close() {
    take();
}

/// PS_READ_FUNC(files)
pub(crate) fn read(vm: &mut Vm, key: &[u8]) -> Option<Vec<u8>> {
    let mut data = take()?;
    files_open(vm, &mut data, key);
    let r = (|| {
        let f = data.fd.as_ref()?;
        let md = f.metadata().ok()?;
        data.st_size = md.len();
        if md.len() == 0 {
            return Some(Vec::new());
        }
        let mut buf = vec![0u8; md.len() as usize];
        use std::os::unix::fs::FileExt;
        match f.read_at(&mut buf, 0) {
            Ok(n) if n == buf.len() => Some(buf),
            Ok(_) => {
                err(vm, E_WARNING, "Read returned less bytes than requested");
                None
            }
            Err(e) => {
                let code = e.raw_os_error().unwrap_or(0);
                err(vm, E_WARNING, &format!("Read failed: {} ({})", strerror(code), code));
                None
            }
        }
    })();
    put(data);
    r
}

/// PS_WRITE_FUNC(files)
pub(crate) fn write(vm: &mut Vm, key: &[u8], val: &[u8]) -> bool {
    let mut data = match take() {
        Some(d) => d,
        None => return false,
    };
    let r = files_write(vm, &mut data, key, val);
    put(data);
    r
}

/// PS_UPDATE_TIMESTAMP_FUNC(files)
pub(crate) fn update_timestamp(vm: &mut Vm, key: &[u8], val: &[u8]) -> bool {
    let mut data = match take() {
        Some(d) => d,
        None => return false,
    };
    let r = match path_create(&data, key) {
        None => false,
        Some(path) => {
            let c = CString::new(path).unwrap_or_default();
            let ret = unsafe { libc::utime(c.as_ptr(), std::ptr::null()) };
            if ret == -1 {
                files_write(vm, &mut data, key, val)
            } else {
                true
            }
        }
    };
    put(data);
    r
}

/// PS_DESTROY_FUNC(files)
pub(crate) fn destroy(key: &[u8]) -> bool {
    let mut data = match take() {
        Some(d) => d,
        None => return false,
    };
    let r = match path_create(&data, key) {
        None => false,
        Some(path) => {
            if data.fd.is_some() {
                data.fd = None;
                let p = lossy(&path);
                if std::fs::remove_file(&p).is_err() && std::path::Path::new(&p).exists() {
                    false
                } else {
                    true
                }
            } else {
                true
            }
        }
    };
    put(data);
    r
}

/// PS_GC_FUNC(files): returns nrdels (-1 on failure).
pub(crate) fn gc(vm: &mut Vm, maxlifetime: i64) -> i64 {
    let data = match take() {
        Some(d) => d,
        None => return -1,
    };
    let r = if data.dirdepth == 0 { cleanup_dir(vm, &data.basedir, maxlifetime) } else { -1 };
    put(data);
    r
}

fn cleanup_dir(vm: &mut Vm, dirname: &[u8], maxlifetime: i64) -> i64 {
    let d = lossy(dirname);
    let rd = match std::fs::read_dir(&d) {
        Ok(r) => r,
        Err(e) => {
            let code = e.raw_os_error().unwrap_or(0);
            err(vm, E_NOTICE, &format!("ps_files_cleanup_dir: opendir({}) failed: {} ({})", d, strerror(code), code));
            return -1;
        }
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let mut n = 0;
    for e in rd.flatten() {
        let name = e.file_name();
        let nb = name.as_encoded_bytes();
        if nb.starts_with(FILE_PREFIX) {
            let p = e.path();
            if let Ok(md) = std::fs::metadata(&p) {
                if now - md.mtime() > maxlifetime {
                    let _ = std::fs::remove_file(&p);
                    n += 1;
                }
            }
        }
    }
    n
}

fn key_exists_in(data: &FilesData, key: &[u8]) -> bool {
    match path_create(data, key) {
        Some(p) => std::fs::metadata(lossy(&p)).is_ok(),
        None => false,
    }
}

/// PS_VALIDATE_SID_FUNC(files): true when the session exists.
pub(crate) fn key_exists(key: &[u8]) -> bool {
    ps(|p| p.mod_data.as_ref().map(|d| key_exists_in(d, key)).unwrap_or(false))
}

/// PS_CREATE_SID_FUNC(files)
pub(crate) fn create_sid() -> Option<Vec<u8>> {
    let mut maxfail = 3;
    loop {
        let sid = crate::create_id();
        if key_exists(&sid) {
            maxfail -= 1;
            if maxfail < 0 {
                return None;
            }
            continue;
        }
        return Some(sid);
    }
}
