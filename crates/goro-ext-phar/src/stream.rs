//! php-src ext/phar/stream.c and dirstream.c: the phar:// stream wrapper.

use std::cell::RefCell;
use std::rc::Rc;

use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};
use goro_ext_standard::native_streams::{
    NativeOpened, NativeStream, NativeWrapperOps, StatBuf, wrapper_log_error,
};

use crate::archive::*;
use crate::util;

/// (PHP_STREAM_URL_STAT_QUIET, == IGNORE_URL for open options)
const QUIET: i64 = 2;

fn log(vm: &mut Vm, options: i64, msg: String) {
    wrapper_log_error(vm, options, msg);
}

/// php_url of a phar URL: (host = archive, path = "/entry")
pub struct PharUrl {
    pub host: Vec<u8>,
    pub path: Vec<u8>,
}

/// phar_parse_url()
pub fn parse_url(vm: &mut Vm, filename: &[u8], mode: &[u8], options: i64) -> Option<PharUrl> {
    if !starts_with_ci(filename, b"phar://") {
        return None;
    }
    let m0 = mode.first().copied().unwrap_or(b'r');
    let m1 = mode.get(1).copied().unwrap_or(0);
    if m0 == b'a' {
        if options & QUIET == 0 {
            log(
                vm,
                options,
                "phar error: open mode append not supported".to_string(),
            );
        }
        return None;
    }
    let (arch, entry) = match split_fname(vm, filename, 2, if m0 == b'w' { 2 } else { 0 }) {
        Ok(x) => x,
        Err(arch) => {
            if options & QUIET == 0 {
                match arch {
                    Some(a) => log(
                        vm,
                        options,
                        format!(
                            "phar error: no directory in \"{}\", must have at least phar://{}/ for root directory (always use full path to a new phar)",
                            l(filename),
                            l(&a)
                        ),
                    ),
                    None => log(
                        vm,
                        options,
                        format!(
                            "phar error: invalid url or non-existent phar \"{}\"",
                            l(filename)
                        ),
                    ),
                }
            }
            return None;
        }
    };
    let resource = PharUrl {
        host: arch,
        path: entry,
    };
    if m0 == b'w' || (m0 == b'r' && m1 == b'+') {
        let pphar = find_fname(&resource.host);
        if readonly() && pphar.as_ref().is_none_or(|p| !p.borrow().is_data) {
            if options & QUIET == 0 {
                log(
                    vm,
                    options,
                    "phar error: write operations disabled by the php.ini setting phar.readonly"
                        .to_string(),
                );
            }
            return None;
        }
        if let Err(e) = open_or_create_filename(vm, &resource.host, None, false, options) {
            if let Some(e) = e {
                if options & QUIET == 0 {
                    log(vm, options, e);
                }
            }
            return None;
        }
    } else if let Err(e) = open_from_filename(vm, &resource.host, None, options) {
        if let Some(e) = e {
            if options & QUIET == 0 {
                log(vm, options, e);
            }
        }
        return None;
    }
    Some(resource)
}

/// The stream of an open phar entry (phar_ops).
pub struct PharStream {
    idata: Option<EntryData>,
}

fn entry_of(idata: &EntryData) -> EntryRef {
    if idata.internal_file.borrow().link.is_some() {
        util::get_link_source(&idata.internal_file).unwrap_or_else(|| idata.internal_file.clone())
    } else {
        idata.internal_file.clone()
    }
}

/// phar_dostat()
pub fn dostat(phar: &Archive, data: Option<&Entry>, is_temp_dir: bool) -> StatBuf {
    let mut sb = StatBuf::default();
    match data {
        Some(d) if !is_temp_dir && !d.is_dir => {
            sb.size = d.uncompressed_filesize as i64;
            sb.mode = (d.flags & PHAR_ENT_PERM_MASK) as i64 | 0o100000;
            sb.mtime = d.timestamp as i64;
            sb.atime = d.timestamp as i64;
            sb.ctime = d.timestamp as i64;
        }
        Some(d) if !is_temp_dir => {
            sb.size = 0;
            sb.mode = (d.flags & PHAR_ENT_PERM_MASK) as i64 | 0o040000;
            sb.mtime = d.timestamp as i64;
            sb.atime = d.timestamp as i64;
            sb.ctime = d.timestamp as i64;
        }
        _ => {
            sb.size = 0;
            sb.mode = 0o777 | 0o040000;
            sb.mtime = phar.max_timestamp as i64;
            sb.atime = phar.max_timestamp as i64;
            sb.ctime = phar.max_timestamp as i64;
        }
    }
    if !phar.is_writeable {
        sb.mode = (sb.mode & 0o555) | (sb.mode & !0o777);
    }
    sb.nlink = 1;
    sb.rdev = -1;
    sb.dev = 0xc;
    if !is_temp_dir {
        if let Some(d) = data {
            sb.ino = d.inode as i64;
        }
    }
    sb.blksize = -1;
    sb.blocks = -1;
    sb
}

impl NativeStream for PharStream {
    fn read(&mut self, _vm: &mut Vm, count: usize, eof: &mut bool) -> Option<Vec<u8>> {
        let idata = self.idata.as_mut()?;
        let entry = entry_of(idata);
        let (deleted, size) = {
            let e = entry.borrow();
            (e.is_deleted, e.uncompressed_filesize as i64)
        };
        if deleted {
            *eof = true;
            return None;
        }
        let fp = idata.fp.clone()?;
        let data = fp.borrow();
        let start = (idata.position + idata.zero).max(0) as usize;
        let avail = (size - idata.position).max(0) as usize;
        let n = count.min(avail);
        let end = (start + n).min(data.len());
        let got = if start < data.len() {
            data[start..end].to_vec()
        } else {
            Vec::new()
        };
        idata.position += got.len() as i64;
        *eof = idata.position == size;
        Some(got)
    }

    fn write(&mut self, _vm: &mut Vm, buf: &[u8]) -> i64 {
        let Some(idata) = self.idata.as_mut() else {
            return -1;
        };
        let Some(fp) = idata.fp.clone() else {
            return -1;
        };
        {
            let mut data = fp.borrow_mut();
            let start = (idata.position + idata.zero).max(0) as usize;
            if data.len() < start {
                data.resize(start, 0);
            }
            let end = start + buf.len();
            if data.len() < end {
                data.resize(end, 0);
            }
            data[start..end].copy_from_slice(buf);
        }
        idata.position += buf.len() as i64;
        let mut e = idata.internal_file.borrow_mut();
        if idata.position > e.uncompressed_filesize as i64 {
            e.uncompressed_filesize = idata.position as u32;
        }
        e.compressed_filesize = e.uncompressed_filesize;
        e.old_flags = e.flags;
        e.is_modified = true;
        buf.len() as i64
    }

    fn close(&mut self, vm: &mut Vm) {
        self.flush(vm);
        if let Some(idata) = self.idata.take() {
            entry_delref(idata);
        }
    }

    fn flush(&mut self, vm: &mut Vm) -> i32 {
        let Some(idata) = self.idata.as_ref() else {
            return -1;
        };
        if idata.internal_file.borrow().is_modified {
            idata.internal_file.borrow_mut().timestamp = now();
            let phar = idata.phar.clone();
            let err = flush(vm, &phar);
            if let Some(e) = err {
                log(vm, REPORT_ERRORS, e);
                return -1;
            }
            0
        } else {
            -1
        }
    }

    fn seek(&mut self, _vm: &mut Vm, offset: i64, whence: i64) -> Option<i64> {
        let idata = self.idata.as_mut()?;
        let entry = entry_of(idata);
        let size = entry.borrow().uncompressed_filesize as i64;
        let temp = match whence {
            2 => idata.zero.wrapping_add(size).wrapping_add(offset),
            1 => idata.zero.wrapping_add(idata.position).wrapping_add(offset),
            0 => idata.zero.wrapping_add(offset),
            _ => 0,
        };
        if temp > idata.zero + size {
            return None;
        }
        if temp < idata.zero {
            return None;
        }
        idata.position = temp - idata.zero;
        Some(idata.position)
    }

    fn lock(&mut self, _vm: &mut Vm, _value: i64) -> i32 {
        -1
    }

    fn label(&self) -> &'static str {
        "phar stream"
    }

    fn stat(&mut self, _vm: &mut Vm) -> Option<StatBuf> {
        let idata = self.idata.as_ref()?;
        let p = idata.phar.borrow();
        let e = idata.internal_file.borrow();
        Some(dostat(&p, Some(&e), false))
    }
}

/// A directory stream (phar_dir_ops).
pub struct PharDir {
    entries: Vec<Vec<u8>>,
    pos: usize,
}

impl NativeStream for PharDir {
    fn read(&mut self, _vm: &mut Vm, _count: usize, _eof: &mut bool) -> Option<Vec<u8>> {
        None
    }
    fn write(&mut self, _vm: &mut Vm, _buf: &[u8]) -> i64 {
        -1
    }
    fn close(&mut self, _vm: &mut Vm) {}
    fn flush(&mut self, _vm: &mut Vm) -> i32 {
        -1
    }
    fn seek(&mut self, _vm: &mut Vm, offset: i64, whence: i64) -> Option<i64> {
        let mut offset = offset;
        if whence == 2 {
            offset += self.entries.len() as i64;
        }
        if whence == 0 || whence == 2 {
            self.pos = 0;
        }
        if offset < 0 {
            return None;
        }
        let mut n = 0;
        while n < offset && self.pos < self.entries.len() {
            self.pos += 1;
            n += 1;
        }
        Some(n)
    }
    fn lock(&mut self, _vm: &mut Vm, _value: i64) -> i32 {
        -1
    }
    fn label(&self) -> &'static str {
        "phar dir"
    }
    fn readdir(&mut self, _vm: &mut Vm) -> Option<Vec<u8>> {
        let e = self.entries.get(self.pos)?.clone();
        self.pos += 1;
        if e.len() >= 256 {
            return None;
        }
        Some(e)
    }
    fn rewinddir(&mut self, _vm: &mut Vm) {
        self.pos = 0;
    }
}

/// phar_make_dirstream()
fn make_dirstream(dir: &[u8], manifest: &[Vec<u8>]) -> PharDir {
    let mut data: Vec<Vec<u8>> = Vec::new();
    let dirlen = dir.len();
    let empty = || PharDir {
        entries: Vec::new(),
        pos: 0,
    };
    if (dir == b"/" && manifest.is_empty()) || (dirlen >= 5 && dir.starts_with(b".phar")) {
        return empty();
    }
    let add = |e: &[u8], data: &mut Vec<Vec<u8>>| {
        if !e.is_empty() && !data.iter().any(|x| x == e) {
            data.push(e.to_vec());
        }
    };
    for key in manifest {
        let keylen = key.len();
        if keylen <= dirlen && (keylen == 0 || keylen < dirlen || key.starts_with(dir)) {
            continue;
        }
        if dir.first() == Some(&b'/') {
            // root directory
            if key.starts_with(b".phar") {
                continue;
            }
            let n = key.iter().position(|&c| c == b'/').unwrap_or(keylen);
            add(&key[..n], &mut data);
            continue;
        }
        if !key.starts_with(dir) || key[dirlen] != b'/' {
            continue;
        }
        let rest = &key[dirlen + 1..];
        let n = rest.iter().position(|&c| c == b'/').unwrap_or(rest.len());
        add(&rest[..n], &mut data);
    }
    data.sort();
    PharDir {
        entries: data,
        pos: 0,
    }
}

/// php_stream_phar_wrapper
pub struct PharWrapper;

fn manifest_keys(phar: &ArcRef) -> Vec<Vec<u8>> {
    phar.borrow().manifest.keys().cloned().collect()
}

impl NativeWrapperOps for PharWrapper {
    fn label(&self) -> &'static str {
        "phar"
    }

    /// phar_wrapper_open_url()
    fn open(
        &self,
        vm: &mut Vm,
        path: &[u8],
        mode: &str,
        options: i64,
        context: Option<i64>,
    ) -> Result<Option<NativeOpened>, VmError> {
        let mode_b = mode.as_bytes();
        let Some(resource) = parse_url(vm, path, mode_b, options) else {
            return Ok(None);
        };
        request_initialize();
        let internal_file = resource.path[1..].to_vec();
        let m0 = mode_b.first().copied().unwrap_or(b'r');
        let m1 = mode_b.get(1).copied().unwrap_or(0);
        if m0 == b'w' || (m0 == b'r' && m1 == b'+') {
            let idata = match util::get_or_create_entry_data(
                vm,
                &resource.host,
                &internal_file,
                mode_b,
                0,
                true,
            ) {
                Ok(d) => d,
                Err(e) => {
                    let msg = e.unwrap_or_else(|| {
                        format!(
                            "phar error: file \"{}\" could not be created in phar \"{}\"",
                            l(&internal_file),
                            l(&resource.host)
                        )
                    });
                    log(vm, options, msg);
                    return Ok(None);
                }
            };
            if let Some(Value::Long(c)) =
                goro_ext_standard::native_streams::context_option(context, b"phar", b"compress")
                    .map(|v| v.deref())
            {
                let mut e = idata.internal_file.borrow_mut();
                if e.uncompressed_filesize == 0
                    && e.compressed_filesize == 0
                    && (c as u32 & !PHAR_ENT_COMPRESSION_MASK) == 0
                    && c >= 0
                {
                    e.flags &= !PHAR_ENT_COMPRESSION_MASK;
                    e.flags |= c as u32;
                }
            }
            if let Some(m) =
                goro_ext_standard::native_streams::context_option(context, b"phar", b"metadata")
            {
                let mut e = idata.internal_file.borrow_mut();
                e.metadata.free();
                e.metadata.val = Some(m.deref());
                drop(e);
                idata.phar.borrow_mut().is_modified = true;
            }
            let opened = {
                let mut o = b"phar://".to_vec();
                o.extend_from_slice(&idata.phar.borrow().fname);
                o.push(b'/');
                o.extend_from_slice(&idata.internal_file.borrow().filename);
                o
            };
            return Ok(Some(NativeOpened {
                stream: Rc::new(RefCell::new(PharStream { idata: Some(idata) })),
                opened_path: Some(opened),
            }));
        }
        let idata;
        let opened_path: Option<Vec<u8>>;
        if internal_file.is_empty() && options & STREAM_OPEN_FOR_INCLUDE != 0 {
            // retrieve the stub
            let phar = match util::get_archive(&resource.host, None) {
                Ok(p) => p,
                Err(_) => {
                    log(
                        vm,
                        options,
                        format!("file {} is not a valid phar archive", l(&resource.host)),
                    );
                    return Ok(None);
                }
            };
            let (is_tar, is_zip, fname) = {
                let p = phar.borrow();
                (p.is_tar, p.is_zip, p.fname.clone())
            };
            if is_tar || is_zip {
                match util::get_entry_data(vm, &resource.host, b".phar/stub.php", b"r", 0, false) {
                    Ok(Some(d)) => idata = d,
                    Ok(None) | Err(None) => {
                        log(
                            vm,
                            options,
                            format!(
                                "phar error: \"{}\" is not a file in phar \"{}\"",
                                l(&internal_file),
                                l(&resource.host)
                            ),
                        );
                        return Ok(None);
                    }
                    Err(Some(e)) => {
                        log(vm, options, e);
                        return Ok(None);
                    }
                }
                opened_path = Some(fname);
            } else {
                if phar.borrow().fp.is_none() && util::open_archive_fp(vm, &phar).is_err() {
                    log(
                        vm,
                        options,
                        format!(
                            "phar error: could not reopen phar \"{}\"",
                            l(&resource.host)
                        ),
                    );
                    return Ok(None);
                }
                let fp = phar.borrow().fp.clone();
                let halt = phar.borrow().halt_offset as u32;
                let entry = Entry {
                    is_temp_dir: true,
                    phar: Rc::downgrade(&phar),
                    compressed_filesize: halt,
                    uncompressed_filesize: halt,
                    is_crc_checked: true,
                    fp_refcount: 1,
                    ..Default::default()
                }
                .into_ref();
                phar.borrow_mut().refcount += 1;
                idata = EntryData {
                    phar: phar.clone(),
                    fp,
                    position: 0,
                    zero: 0,
                    internal_file: entry,
                };
                opened_path = Some(fname);
            }
            return Ok(Some(NativeOpened {
                stream: Rc::new(RefCell::new(PharStream { idata: Some(idata) })),
                opened_path,
            }));
        }
        // read-only access is allowed to magic files in .phar directory
        match util::get_entry_data(vm, &resource.host, &internal_file, b"r", 0, false) {
            Ok(Some(d)) => idata = d,
            Ok(None) | Err(None) => {
                log(
                    vm,
                    options,
                    format!(
                        "phar error: \"{}\" is not a file in phar \"{}\"",
                        l(&internal_file),
                        l(&resource.host)
                    ),
                );
                return Ok(None);
            }
            Err(Some(e)) => {
                log(vm, options, e);
                return Ok(None);
            }
        }
        // check length, crc32
        let (checked, crc) = {
            let e = idata.internal_file.borrow();
            (e.is_crc_checked, e.crc32)
        };
        if !checked {
            let fp = idata.fp.clone().unwrap_or_else(new_buf);
            let mut zero = idata.zero;
            let phar = idata.phar.clone();
            let entry = idata.internal_file.clone();
            if let Err(e) = postprocess_file(vm, &phar, &entry, &fp, &mut zero, crc, 2) {
                log(vm, options, e.unwrap_or_default());
                entry_delref(idata);
                return Ok(None);
            }
        }
        let cwd_init = g(|g| g.cwd_init);
        if !cwd_init && options & STREAM_OPEN_FOR_INCLUDE != 0 {
            let (is_tar, is_zip) = {
                let p = idata.phar.borrow();
                (p.is_tar, p.is_zip)
            };
            let entry = idata.internal_file.borrow().filename.clone();
            g(|g| {
                g.cwd_init = true;
                if (is_tar || is_zip) && entry == b".phar/stub.php" {
                    g.cwd_init = false;
                } else if let Some(p) = entry.iter().rposition(|&c| c == b'/') {
                    g.cwd = Some(entry[..p].to_vec());
                } else {
                    g.cwd = None;
                }
            });
        }
        let opened = {
            let mut o = b"phar://".to_vec();
            o.extend_from_slice(&idata.phar.borrow().fname);
            o.push(b'/');
            o.extend_from_slice(&idata.internal_file.borrow().filename);
            o
        };
        Ok(Some(NativeOpened {
            stream: Rc::new(RefCell::new(PharStream { idata: Some(idata) })),
            opened_path: Some(opened),
        }))
    }

    /// phar_wrapper_open_dir()
    fn opendir(
        &self,
        vm: &mut Vm,
        path: &[u8],
        options: i64,
        context: Option<i64>,
    ) -> Result<Option<Rc<RefCell<dyn NativeStream>>>, VmError> {
        let Some(resource) = parse_url(vm, path, b"r", options) else {
            log(vm, options, format!("phar url \"{}\" is unknown", l(path)));
            return Ok(None);
        };
        request_initialize();
        let phar = match util::get_archive(&resource.host, None) {
            Ok(p) => p,
            Err(e) => {
                let msg =
                    e.unwrap_or_else(|| format!("phar file \"{}\" is unknown", l(&resource.host)));
                log(vm, options, msg);
                return Ok(None);
            }
        };
        if resource.path == b"/" {
            let keys = manifest_keys(&phar);
            return Ok(Some(Rc::new(RefCell::new(make_dirstream(b"/", &keys)))));
        }
        let internal_file = &resource.path[1..];
        let entry = phar.borrow().find(internal_file);
        match entry {
            Some(e) if !e.borrow().is_dir => Ok(None),
            Some(e) => {
                let (mounted, tmp) = {
                    let eb = e.borrow();
                    (eb.is_mounted, eb.tmp.clone())
                };
                if mounted {
                    // php_stream_opendir(entry->tmp)
                    let tmp = tmp.unwrap_or_default();
                    let _ = context;
                    let list = list_plain_dir(&tmp);
                    return Ok(list.map(|entries| {
                        Rc::new(RefCell::new(PharDir { entries, pos: 0 }))
                            as Rc<RefCell<dyn NativeStream>>
                    }));
                }
                let keys = manifest_keys(&phar);
                Ok(Some(Rc::new(RefCell::new(make_dirstream(
                    internal_file,
                    &keys,
                )))))
            }
            None => {
                let keys = manifest_keys(&phar);
                if keys.iter().any(|k| k.starts_with(internal_file)) {
                    return Ok(Some(Rc::new(RefCell::new(make_dirstream(
                        internal_file,
                        &keys,
                    )))));
                }
                Ok(None)
            }
        }
    }

    /// phar_wrapper_stat()
    fn url_stat(&self, vm: &mut Vm, url: &[u8], flags: i64) -> Result<Option<StatBuf>, VmError> {
        let Some(resource) = parse_url(vm, url, b"r", flags | QUIET) else {
            return Ok(None);
        };
        request_initialize();
        let internal_file = resource.path[1..].to_vec();
        let Ok(phar) = util::get_archive(&resource.host, None) else {
            return Ok(None);
        };
        if internal_file.is_empty() {
            return Ok(Some(dostat(&phar.borrow(), None, true)));
        }
        let entry = phar.borrow().find(&internal_file);
        if let Some(e) = entry {
            return Ok(Some(dostat(&phar.borrow(), Some(&e.borrow()), false)));
        }
        if phar.borrow().virtual_dirs.contains(&internal_file) {
            return Ok(Some(dostat(&phar.borrow(), None, true)));
        }
        let mounted: Vec<Vec<u8>> = phar.borrow().mounted_dirs.iter().cloned().collect();
        for key in mounted {
            if key.len() >= internal_file.len() || !internal_file.starts_with(&key) {
                continue;
            }
            let Some(e) = phar.borrow().find(&key) else {
                return Ok(None);
            };
            let (tmp, is_mounted) = {
                let eb = e.borrow();
                (eb.tmp.clone(), eb.is_mounted)
            };
            let Some(tmp) = tmp.filter(|_| is_mounted) else {
                return Ok(None);
            };
            let mut test = tmp;
            test.extend_from_slice(&internal_file[key.len()..]);
            if stat_is_dir(vm, &test).is_none() {
                continue;
            }
            if util::mount_entry(vm, &phar, &test, &internal_file).is_err() {
                return Ok(None);
            }
            let Some(e) = phar.borrow().find(&internal_file) else {
                return Ok(None);
            };
            return Ok(Some(dostat(&phar.borrow(), Some(&e.borrow()), false)));
        }
        Ok(None)
    }

    /// phar_wrapper_unlink()
    fn unlink(
        &self,
        vm: &mut Vm,
        url: &[u8],
        options: i64,
        _context: Option<i64>,
    ) -> Result<bool, VmError> {
        let Some(resource) = parse_url(vm, url, b"rb", options) else {
            log(vm, options, "phar error: unlink failed".to_string());
            return Ok(false);
        };
        request_initialize();
        let pphar = find_fname(&resource.host);
        if readonly() && pphar.as_ref().is_none_or(|p| !p.borrow().is_data) {
            log(
                vm,
                options,
                "phar error: write operations disabled by the php.ini setting phar.readonly"
                    .to_string(),
            );
            return Ok(false);
        }
        let internal_file = resource.path[1..].to_vec();
        let idata = match util::get_entry_data(vm, &resource.host, &internal_file, b"r", 0, true) {
            Ok(Some(d)) => d,
            Ok(None) | Err(None) => {
                log(
                    vm,
                    options,
                    format!("unlink of \"{}\" failed, file does not exist", l(url)),
                );
                return Ok(false);
            }
            Err(Some(e)) => {
                log(
                    vm,
                    options,
                    format!("unlink of \"{}\" failed: {}", l(url), e),
                );
                return Ok(false);
            }
        };
        if idata.internal_file.borrow().fp_refcount > 1 {
            log(
                vm,
                options,
                format!(
                    "phar error: \"{}\" in phar \"{}\", has open file pointers, cannot unlink",
                    l(&internal_file),
                    l(&resource.host)
                ),
            );
            entry_delref(idata);
            return Ok(false);
        }
        if let Some(e) = entry_remove(vm, idata) {
            log(vm, options, e);
        }
        Ok(true)
    }

    /// phar_wrapper_rename()
    fn rename(
        &self,
        vm: &mut Vm,
        url_from: &[u8],
        url_to: &[u8],
        options: i64,
        _context: Option<i64>,
    ) -> Result<bool, VmError> {
        let (uf, ut) = (l(url_from), l(url_to));
        let warn = |vm: &mut Vm, msg: String| {
            let f = vm.current_builtin_name().unwrap_or_default();
            vm.emit_warning(&format!("{}(): {}", f, msg));
        };
        let Some(resource_from) = parse_url(vm, url_from, b"wb", options | QUIET) else {
            warn(
                vm,
                format!(
                    "phar error: cannot rename \"{}\" to \"{}\": invalid or non-writable url \"{}\"",
                    uf, ut, uf
                ),
            );
            return Ok(false);
        };
        let pfrom = util::get_archive(&resource_from.host, None).ok();
        if readonly() && pfrom.as_ref().is_none_or(|p| !p.borrow().is_data) {
            warn(
                vm,
                "phar error: Write operations disabled by the php.ini setting phar.readonly"
                    .to_string(),
            );
            return Ok(false);
        }
        let Some(resource_to) = parse_url(vm, url_to, b"wb", options | QUIET) else {
            warn(
                vm,
                format!(
                    "phar error: cannot rename \"{}\" to \"{}\": invalid or non-writable url \"{}\"",
                    uf, ut, ut
                ),
            );
            return Ok(false);
        };
        let pto = util::get_archive(&resource_to.host, None).ok();
        if readonly() && pto.as_ref().is_none_or(|p| !p.borrow().is_data) {
            warn(
                vm,
                "phar error: Write operations disabled by the php.ini setting phar.readonly"
                    .to_string(),
            );
            return Ok(false);
        }
        if resource_from.host != resource_to.host {
            warn(
                vm,
                format!(
                    "phar error: cannot rename \"{}\" to \"{}\", not within the same phar archive",
                    uf, ut
                ),
            );
            return Ok(false);
        }
        let phar = match util::get_archive(&resource_from.host, None) {
            Ok(p) => p,
            Err(e) => {
                warn(
                    vm,
                    format!(
                        "phar error: cannot rename \"{}\" to \"{}\": {}",
                        uf,
                        ut,
                        e.unwrap_or_default()
                    ),
                );
                return Ok(false);
            }
        };
        let from = resource_from.path[1..].to_vec();
        let to = resource_to.path[1..].to_vec();
        let mut is_modified = false;
        let is_dir;
        let entry = phar.borrow().find(&from);
        if let Some(entry) = entry {
            if entry.borrow().is_deleted {
                warn(
                    vm,
                    format!(
                        "phar error: cannot rename \"{}\" to \"{}\" from extracted phar archive, source has been deleted",
                        uf, ut
                    ),
                );
                return Ok(false);
            }
            let mut new = entry.borrow().clone();
            {
                let mut e = entry.borrow_mut();
                e.is_deleted = true;
                e.fp = None;
                e.metadata.val = None;
                e.link = None;
                e.tmp = None;
            }
            new.filename = to.clone();
            if let Err(e) = util::copy_entry_fp(vm, &entry, &mut new) {
                warn(
                    vm,
                    format!(
                        "phar error: cannot rename \"{}\" to \"{}\": {}",
                        uf,
                        ut,
                        e.unwrap_or_default()
                    ),
                );
                return Ok(false);
            }
            is_modified = true;
            new.is_modified = true;
            is_dir = new.is_dir;
            phar.borrow_mut()
                .manifest
                .insert(to.clone(), new.into_ref());
        } else {
            is_dir = phar.borrow().virtual_dirs.contains(&from);
            if !is_dir {
                warn(
                    vm,
                    format!(
                        "phar error: cannot rename \"{}\" to \"{}\" from extracted phar archive, source does not exist",
                        uf, ut
                    ),
                );
                return Ok(false);
            }
        }
        if is_dir {
            let from_len = from.len();
            let rekey = |k: &[u8]| -> Vec<u8> {
                let mut n = to.clone();
                n.extend_from_slice(&k[from_len..]);
                n
            };
            {
                let mut p = phar.borrow_mut();
                let old = std::mem::take(&mut p.manifest);
                for (k, e) in old {
                    let deleted = e.borrow().is_deleted;
                    if !deleted && k.len() > from_len && k.starts_with(&from) && k[from_len] == b'/'
                    {
                        let nk = rekey(&k);
                        is_modified = true;
                        {
                            let mut eb = e.borrow_mut();
                            eb.is_modified = true;
                            eb.filename = nk.clone();
                        }
                        p.manifest.insert(nk, e);
                    } else {
                        p.manifest.insert(k, e);
                    }
                }
                let matches =
                    |k: &[u8]| k.starts_with(&from) && (k.len() == from_len || k[from_len] == b'/');
                let vd = std::mem::take(&mut p.virtual_dirs);
                for k in vd {
                    if matches(&k) {
                        p.virtual_dirs.insert(rekey(&k));
                    } else {
                        p.virtual_dirs.insert(k);
                    }
                }
                let md = std::mem::take(&mut p.mounted_dirs);
                for k in md {
                    if matches(&k) {
                        p.mounted_dirs.insert(rekey(&k));
                    } else {
                        p.mounted_dirs.insert(k);
                    }
                }
            }
        }
        if is_modified {
            if let Some(e) = flush(vm, &phar) {
                warn(
                    vm,
                    format!("phar error: cannot rename \"{}\" to \"{}\": {}", uf, ut, e),
                );
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// phar_wrapper_mkdir()
    fn mkdir(
        &self,
        vm: &mut Vm,
        url_from: &[u8],
        _mode: i64,
        options: i64,
        _context: Option<i64>,
    ) -> Result<bool, VmError> {
        let (arch, _) = match split_fname(vm, url_from, 2, 2) {
            Ok(x) => x,
            Err(_) => {
                log(
                    vm,
                    options,
                    format!(
                        "phar error: cannot create directory \"{}\", no phar archive specified",
                        l(url_from)
                    ),
                );
                return Ok(false);
            }
        };
        let phar = util::get_archive(&arch, None).ok();
        if readonly() && phar.as_ref().is_none_or(|p| !p.borrow().is_data) {
            log(
                vm,
                options,
                format!(
                    "phar error: cannot create directory \"{}\", write operations disabled",
                    l(url_from)
                ),
            );
            return Ok(false);
        }
        let Some(resource) = parse_url(vm, url_from, b"w", options) else {
            return Ok(false);
        };
        let rpath = resource.path[1..].to_vec();
        let host = resource.host.clone();
        let phar = match util::get_archive(&host, None) {
            Ok(p) => p,
            Err(e) => {
                log(
                    vm,
                    options,
                    format!(
                        "phar error: cannot create directory \"{}\" in phar \"{}\", error retrieving phar information: {}",
                        l(&rpath),
                        l(&host),
                        e.unwrap_or_default()
                    ),
                );
                return Ok(false);
            }
        };
        match util::get_entry_info_dir(vm, &phar, &rpath, 2, true) {
            Ok(_) => {
                log(
                    vm,
                    options,
                    format!(
                        "phar error: cannot create directory \"{}\" in phar \"{}\", directory already exists",
                        l(&rpath),
                        l(&host)
                    ),
                );
                return Ok(false);
            }
            Err(Some(e)) => {
                log(
                    vm,
                    options,
                    format!(
                        "phar error: cannot create directory \"{}\" in phar \"{}\", {}",
                        l(&rpath),
                        l(&host),
                        e
                    ),
                );
                return Ok(false);
            }
            Err(None) => {}
        }
        match util::get_entry_info_dir(vm, &phar, &rpath, 0, true) {
            Ok(_) => {
                log(
                    vm,
                    options,
                    format!(
                        "phar error: cannot create directory \"{}\" in phar \"{}\", file already exists",
                        l(&rpath),
                        l(&host)
                    ),
                );
                return Ok(false);
            }
            Err(Some(e)) => {
                log(
                    vm,
                    options,
                    format!(
                        "phar error: cannot create directory \"{}\" in phar \"{}\", {}",
                        l(&rpath),
                        l(&host),
                        e
                    ),
                );
                return Ok(false);
            }
            Err(None) => {}
        }
        let (is_zip, is_tar) = {
            let p = phar.borrow();
            (p.is_zip, p.is_tar)
        };
        let entry = Entry {
            is_zip,
            filename: rpath.clone(),
            is_tar,
            tar_type: if is_tar { TAR_DIR } else { 0 },
            is_dir: true,
            phar: Rc::downgrade(&phar),
            is_modified: true,
            is_crc_checked: true,
            flags: PHAR_ENT_PERM_DEF_DIR,
            old_flags: PHAR_ENT_PERM_DEF_DIR,
            ..Default::default()
        };
        {
            let mut p = phar.borrow_mut();
            if p.manifest.contains_key(&rpath) {
                let fname = l(&p.fname);
                drop(p);
                log(
                    vm,
                    options,
                    format!(
                        "phar error: cannot create directory \"{}\" in phar \"{}\", adding to manifest failed",
                        l(&rpath),
                        fname
                    ),
                );
                return Ok(false);
            }
            p.manifest.insert(rpath.clone(), entry.into_ref());
        }
        if let Some(e) = flush(vm, &phar) {
            let fname = l(&phar.borrow().fname);
            log(
                vm,
                options,
                format!(
                    "phar error: cannot create directory \"{}\" in phar \"{}\", {}",
                    l(&rpath),
                    fname,
                    e
                ),
            );
            phar.borrow_mut().manifest.shift_remove(&rpath);
            return Ok(false);
        }
        util::add_virtual_dirs(&mut phar.borrow_mut(), &rpath);
        Ok(true)
    }

    /// phar_wrapper_rmdir()
    fn rmdir(
        &self,
        vm: &mut Vm,
        url: &[u8],
        options: i64,
        _context: Option<i64>,
    ) -> Result<bool, VmError> {
        let (arch, _) = match split_fname(vm, url, 2, 2) {
            Ok(x) => x,
            Err(_) => {
                log(
                    vm,
                    options,
                    format!(
                        "phar error: cannot remove directory \"{}\", no phar archive specified, or phar archive does not exist",
                        l(url)
                    ),
                );
                return Ok(false);
            }
        };
        let phar = util::get_archive(&arch, None).ok();
        if readonly() && phar.as_ref().is_none_or(|p| !p.borrow().is_data) {
            log(
                vm,
                options,
                format!(
                    "phar error: cannot rmdir directory \"{}\", write operations disabled",
                    l(url)
                ),
            );
            return Ok(false);
        }
        let Some(resource) = parse_url(vm, url, b"w", options) else {
            return Ok(false);
        };
        let rpath = resource.path[1..].to_vec();
        let host = resource.host.clone();
        let phar = match util::get_archive(&host, None) {
            Ok(p) => p,
            Err(e) => {
                log(
                    vm,
                    options,
                    format!(
                        "phar error: cannot remove directory \"{}\" in phar \"{}\", error retrieving phar information: {}",
                        l(&rpath),
                        l(&host),
                        e.unwrap_or_default()
                    ),
                );
                return Ok(false);
            }
        };
        let entry = match util::get_entry_info_dir(vm, &phar, &rpath, 2, true) {
            Ok(e) => e,
            Err(Some(e)) => {
                log(
                    vm,
                    options,
                    format!(
                        "phar error: cannot remove directory \"{}\" in phar \"{}\", {}",
                        l(&rpath),
                        l(&host),
                        e
                    ),
                );
                return Ok(false);
            }
            Err(None) => {
                log(
                    vm,
                    options,
                    format!(
                        "phar error: cannot remove directory \"{}\" in phar \"{}\", directory does not exist",
                        l(&rpath),
                        l(&host)
                    ),
                );
                return Ok(false);
            }
        };
        let path_len = rpath.len();
        if !entry.borrow().is_deleted {
            let p = phar.borrow();
            let nonempty = p
                .manifest
                .keys()
                .any(|k| k.starts_with(&rpath) && k.get(path_len) == Some(&b'/'))
                || p.virtual_dirs
                    .iter()
                    .any(|k| k.starts_with(&rpath) && k.get(path_len) == Some(&b'/'));
            drop(p);
            if nonempty {
                log(vm, options, "phar error: Directory not empty".to_string());
                return Ok(false);
            }
        }
        if entry.borrow().is_temp_dir {
            phar.borrow_mut().virtual_dirs.shift_remove(&rpath);
        } else {
            {
                let mut e = entry.borrow_mut();
                e.is_deleted = true;
                e.is_modified = true;
            }
            if let Some(err) = flush(vm, &phar) {
                let fname = l(&entry.borrow().filename);
                log(
                    vm,
                    options,
                    format!(
                        "phar error: cannot remove directory \"{}\" in phar \"{}\", {}",
                        fname,
                        l(&phar.borrow().fname),
                        err
                    ),
                );
                return Ok(false);
            }
        }
        Ok(true)
    }
}

/// php_stream_opendir() on a plain directory (mounted directories).
fn list_plain_dir(path: &[u8]) -> Option<Vec<Vec<u8>>> {
    let rd = std::fs::read_dir(l(path)).ok()?;
    let mut v: Vec<Vec<u8>> = vec![b".".to_vec(), b"..".to_vec()];
    for e in rd.flatten() {
        v.push(e.file_name().to_string_lossy().as_bytes().to_vec());
    }
    Some(v)
}
