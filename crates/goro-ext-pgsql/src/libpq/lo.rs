//! Large objects (fe-lobj.c): the server-side lo_* functions called with
//! the fastpath interface.

use super::conn::PgConn;
use super::exec::FnArg;
use super::*;

pub const INV_WRITE: i32 = 0x0002_0000;
pub const INV_READ: i32 = 0x0004_0000;

const LO_BUFSIZE: usize = 8192;

/// PGlobjfuncs
#[derive(Clone, Copy, Debug, Default)]
pub struct LoFuncs {
    pub open: Oid,
    pub close: Oid,
    pub creat: Oid,
    pub create: Oid,
    pub unlink: Oid,
    pub lseek: Oid,
    pub lseek64: Oid,
    pub tell: Oid,
    pub tell64: Oid,
    pub truncate: Oid,
    pub truncate64: Oid,
    pub read: Oid,
    pub write: Oid,
}

impl PgConn {
    /// lo_initialize
    fn lo_initialize(&mut self) -> Option<LoFuncs> {
        if let Some(f) = self.lobj_funcs {
            self.clear_error_state();
            return Some(f);
        }
        let q = b"select proname, oid from pg_catalog.pg_proc where proname in ('lo_open', 'lo_close', 'lo_creat', 'lo_create', 'lo_unlink', 'lo_lseek', 'lo_lseek64', 'lo_tell', 'lo_tell64', 'lo_truncate', 'lo_truncate64', 'loread', 'lowrite') and pronamespace = (select oid from pg_catalog.pg_namespace where nspname = 'pg_catalog')";
        let res = self.exec(q);
        let res = match res {
            Some(r) if r.status == ExecStatus::TuplesOk => r,
            _ => {
                self.append_error("query to initialize large object functions did not return data");
                return None;
            }
        };
        let mut f = LoFuncs::default();
        for i in 0..res.ntuples() {
            let name = res.value(i, 0);
            let oid: Oid = std::str::from_utf8(res.value(i, 1))
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);
            match name {
                b"lo_open" => f.open = oid,
                b"lo_close" => f.close = oid,
                b"lo_creat" => f.creat = oid,
                b"lo_create" => f.create = oid,
                b"lo_unlink" => f.unlink = oid,
                b"lo_lseek" => f.lseek = oid,
                b"lo_lseek64" => f.lseek64 = oid,
                b"lo_tell" => f.tell = oid,
                b"lo_tell64" => f.tell64 = oid,
                b"lo_truncate" => f.truncate = oid,
                b"lo_truncate64" => f.truncate64 = oid,
                b"loread" => f.read = oid,
                b"lowrite" => f.write = oid,
                _ => {}
            }
        }
        for (oid, name) in [
            (f.open, "lo_open"),
            (f.close, "lo_close"),
            (f.creat, "lo_creat"),
            (f.unlink, "lo_unlink"),
            (f.lseek, "lo_lseek"),
            (f.tell, "lo_tell"),
            (f.read, "loread"),
            (f.write, "lowrite"),
        ] {
            if oid == 0 {
                self.append_error(&format!("cannot determine OID of function {}", name));
                return None;
            }
        }
        self.lobj_funcs = Some(f);
        Some(f)
    }

    fn lo_fn_int(&mut self, fnid: Oid, args: &[FnArg]) -> Option<i32> {
        let (res, val) = self.fn_call(fnid, args);
        match res {
            Some(r) if r.status == ExecStatus::CommandOk => {
                let v = val?;
                if v.len() >= 4 {
                    Some(i32::from_be_bytes([v[0], v[1], v[2], v[3]]))
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    fn lo_fn_int64(&mut self, fnid: Oid, args: &[FnArg]) -> Option<i64> {
        let (res, val) = self.fn_call(fnid, args);
        match res {
            Some(r) if r.status == ExecStatus::CommandOk => {
                let v = val?;
                if v.len() >= 8 {
                    let mut b = [0u8; 8];
                    b.copy_from_slice(&v[..8]);
                    Some(i64::from_be_bytes(b))
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// lo_open
    pub fn lo_open(&mut self, lobj: Oid, mode: i32) -> i32 {
        let Some(f) = self.lo_initialize() else {
            return -1;
        };
        self.lo_fn_int(f.open, &[FnArg::Int(lobj as i32), FnArg::Int(mode)])
            .unwrap_or(-1)
    }

    /// lo_close
    pub fn lo_close(&mut self, fd: i32) -> i32 {
        let Some(f) = self.lo_initialize() else {
            return -1;
        };
        self.lo_fn_int(f.close, &[FnArg::Int(fd)]).unwrap_or(-1)
    }

    /// lo_truncate / lo_truncate64
    pub fn lo_truncate(&mut self, fd: i32, len: i64) -> i32 {
        let Some(f) = self.lo_initialize() else {
            return -1;
        };
        if f.truncate64 != 0 {
            return self
                .lo_fn_int(f.truncate64, &[FnArg::Int(fd), FnArg::Int64(len)])
                .unwrap_or(-1);
        }
        if f.truncate == 0 {
            self.append_error("cannot determine OID of function lo_truncate");
            return -1;
        }
        if len > i32::MAX as i64 {
            self.append_error("argument of lo_truncate exceeds integer range");
            return -1;
        }
        self.lo_fn_int(f.truncate, &[FnArg::Int(fd), FnArg::Int(len as i32)])
            .unwrap_or(-1)
    }

    /// lo_read: the bytes read, or None
    pub fn lo_read(&mut self, fd: i32, len: usize) -> Option<Vec<u8>> {
        let f = self.lo_initialize()?;
        if len > i32::MAX as usize {
            self.append_error("argument of lo_read exceeds integer range");
            return None;
        }
        let (res, val) = self.fn_call(f.read, &[FnArg::Int(fd), FnArg::Int(len as i32)]);
        match res {
            Some(r) if r.status == ExecStatus::CommandOk => Some(val.unwrap_or_default()),
            _ => None,
        }
    }

    /// lo_write: the count written, or -1
    pub fn lo_write(&mut self, fd: i32, buf: &[u8]) -> i32 {
        let Some(f) = self.lo_initialize() else {
            return -1;
        };
        if buf.len() > i32::MAX as usize {
            self.append_error("argument of lo_write exceeds integer range");
            return -1;
        }
        self.lo_fn_int(f.write, &[FnArg::Int(fd), FnArg::Bytes(buf.to_vec())])
            .unwrap_or(-1)
    }

    /// lo_lseek
    pub fn lo_lseek(&mut self, fd: i32, offset: i32, whence: i32) -> i32 {
        let Some(f) = self.lo_initialize() else {
            return -1;
        };
        self.lo_fn_int(
            f.lseek,
            &[FnArg::Int(fd), FnArg::Int(offset), FnArg::Int(whence)],
        )
        .unwrap_or(-1)
    }

    /// lo_lseek64
    pub fn lo_lseek64(&mut self, fd: i32, offset: i64, whence: i32) -> i64 {
        let Some(f) = self.lo_initialize() else {
            return -1;
        };
        if f.lseek64 == 0 {
            self.append_error("cannot determine OID of function lo_lseek64");
            return -1;
        }
        self.lo_fn_int64(
            f.lseek64,
            &[FnArg::Int(fd), FnArg::Int64(offset), FnArg::Int(whence)],
        )
        .unwrap_or(-1)
    }

    /// lo_creat
    pub fn lo_creat(&mut self, mode: i32) -> Oid {
        let Some(f) = self.lo_initialize() else {
            return 0;
        };
        self.lo_fn_int(f.creat, &[FnArg::Int(mode)])
            .map(|v| v as Oid)
            .unwrap_or(0)
    }

    /// lo_create
    pub fn lo_create(&mut self, lobj: Oid) -> Oid {
        let Some(f) = self.lo_initialize() else {
            return 0;
        };
        if f.create == 0 {
            self.append_error("cannot determine OID of function lo_create");
            return 0;
        }
        self.lo_fn_int(f.create, &[FnArg::Int(lobj as i32)])
            .map(|v| v as Oid)
            .unwrap_or(0)
    }

    /// lo_tell
    pub fn lo_tell(&mut self, fd: i32) -> i32 {
        let Some(f) = self.lo_initialize() else {
            return -1;
        };
        self.lo_fn_int(f.tell, &[FnArg::Int(fd)]).unwrap_or(-1)
    }

    /// lo_tell64
    pub fn lo_tell64(&mut self, fd: i32) -> i64 {
        let Some(f) = self.lo_initialize() else {
            return -1;
        };
        if f.tell64 == 0 {
            self.append_error("cannot determine OID of function lo_tell64");
            return -1;
        }
        self.lo_fn_int64(f.tell64, &[FnArg::Int(fd)]).unwrap_or(-1)
    }

    /// lo_unlink
    pub fn lo_unlink(&mut self, lobj: Oid) -> i32 {
        let Some(f) = self.lo_initialize() else {
            return -1;
        };
        self.lo_fn_int(f.unlink, &[FnArg::Int(lobj as i32)])
            .unwrap_or(-1)
    }

    /// lo_import / lo_import_with_oid
    pub fn lo_import(&mut self, filename: &str, oid: Oid) -> Oid {
        let data = match std::fs::read(filename) {
            Ok(d) => d,
            Err(e) => {
                let errno = e.raw_os_error().unwrap_or(0);
                self.append_error(&format!(
                    "could not open file \"{}\": {}",
                    filename,
                    super::conn::strerror(errno)
                ));
                return 0;
            }
        };
        let lobj = if oid == 0 {
            self.lo_creat(INV_READ | INV_WRITE)
        } else {
            self.lo_create(oid)
        };
        if lobj == 0 {
            return 0;
        }
        let fd = self.lo_open(lobj, INV_WRITE);
        if fd < 0 {
            return 0;
        }
        for chunk in data.chunks(LO_BUFSIZE) {
            let n = self.lo_write(fd, chunk);
            if n < chunk.len() as i32 {
                let _ = self.lo_close(fd);
                return 0;
            }
        }
        if self.lo_close(fd) != 0 {
            return 0;
        }
        lobj
    }

    /// lo_export: 1 on success, -1 on failure
    pub fn lo_export(&mut self, lobj: Oid, filename: &str) -> i32 {
        let fd = self.lo_open(lobj, INV_READ);
        if fd < 0 {
            self.append_error(&format!("could not open large object {}", lobj));
            return -1;
        }
        let mut out = match std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(filename)
        {
            Ok(f) => f,
            Err(e) => {
                let errno = e.raw_os_error().unwrap_or(0);
                let _ = self.lo_close(fd);
                self.append_error(&format!(
                    "could not open file \"{}\": {}",
                    filename,
                    super::conn::strerror(errno)
                ));
                return -1;
            }
        };
        use std::io::Write;
        loop {
            let Some(buf) = self.lo_read(fd, LO_BUFSIZE) else {
                let _ = self.lo_close(fd);
                return -1;
            };
            if buf.is_empty() {
                break;
            }
            if let Err(e) = out.write_all(&buf) {
                let errno = e.raw_os_error().unwrap_or(0);
                let _ = self.lo_close(fd);
                self.append_error(&format!(
                    "could not write to file \"{}\": {}",
                    filename,
                    super::conn::strerror(errno)
                ));
                return -1;
            }
        }
        if self.lo_close(fd) != 0 {
            return -1;
        }
        1
    }
}
