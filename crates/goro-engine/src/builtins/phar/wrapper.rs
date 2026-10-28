//! The `phar://` stream wrapper (PHP manual: Phar, "Using Phar Archives:
//! the phar stream wrapper"): reading and writing entries, stat, opendir,
//! unlink, rename, mkdir, rmdir, chmod/touch and include. Messages from
//! ext/phar/tests.

use goro_ext_phar::Entry;
use goro_runtime::{Key, PhpArray, Value};

use super::{Shared, flush, load, lossy, name_info, readonly, split_url};
use crate::builtins::file::{Backing, Stream};
use crate::builtins::streams::{NativeStream, NativeWrapper};
use crate::func::Args;
use crate::vm::{R, Vm};

pub static WRAPPER: NativeWrapper = NativeWrapper {
    open,
    url_stat: Some(url_stat),
    unlink: Some(unlink),
    rename: Some(rename),
    mkdir: Some(mkdir),
    rmdir: Some(rmdir),
    opendir: Some(opendir),
    metadata: Some(metadata),
    include: Some(include),
};

/// Whether an archive is opened as data (no ".phar" in its name) when it
/// isn't loaded yet.
fn data_by_name(vm: &mut Vm, abs: &[u8]) -> bool {
    match super::state(vm).archives.get(abs) {
        Some(s) => s.borrow().is_data,
        None => !name_info(abs).exec,
    }
}

/// The contents of an entry (or of a mounted file), or the message.
pub(crate) fn read_entry(vm: &mut Vm, s: &Shared, inner: &[u8]) -> Result<Vec<u8>, String> {
    let mut l = s.borrow_mut();
    if let Some(ext) = mounted(&l.mounts, inner) {
        return std::fs::read(super::os_path(&ext))
            .map_err(|_| format!("phar error: Cannot open mounted file \"{}\"", lossy(&ext)));
    }
    let fname = l.a.fname.clone();
    let _ = vm;
    let Some(e) = l.a.get_mut(inner) else {
        if l.a.is_dir(inner) {
            return Err(format!(
                "phar error: Cannot retrieve contents, \"{}\" in phar \"{}\" is a directory",
                lossy(inner),
                lossy(&fname)
            ));
        }
        return Err(format!(
            "phar error: \"{}\" is not a file in phar \"{}\"",
            lossy(inner),
            lossy(&fname)
        ));
    };
    if e.is_dir {
        return Err(format!(
            "phar error: Cannot retrieve contents, \"{}\" in phar \"{}\" is a directory",
            lossy(inner),
            lossy(&fname)
        ));
    }
    e.contents(&fname)
        .map(|d| d.to_vec())
        .map_err(|m| format!("phar error: {m}"))
}

/// The external file mounted at `inner` (or under a mounted directory).
fn mounted(mounts: &[(Vec<u8>, Vec<u8>)], inner: &[u8]) -> Option<Vec<u8>> {
    for (m, ext) in mounts {
        if inner == &m[..] {
            return Some(ext.clone());
        }
        if inner.len() > m.len() && inner.starts_with(m) && inner[m.len()] == b'/' {
            let mut p = ext.clone();
            p.extend_from_slice(&inner[m.len()..]);
            return Some(p);
        }
    }
    None
}

fn fail(vm: &mut Vm, a: &Args, url: &[u8], quiet: bool, msg: &str) -> R<Option<Stream>> {
    if !quiet {
        vm.warn(&format!(
            "{}({}): Failed to open stream: {}",
            a.fname(),
            lossy(url),
            msg
        ))?;
    }
    Ok(None)
}

fn open(vm: &mut Vm, a: &Args, url: &[u8], mode: &[u8], quiet: bool) -> R<Option<Stream>> {
    let (abs, inner) = match split_url(vm, url) {
        Ok(x) => x,
        Err(m) => return fail(vm, a, url, quiet, &m),
    };
    let write = mode
        .iter()
        .any(|&c| matches!(c, b'w' | b'a' | b'x' | b'c' | b'+'));
    if !write {
        let d = data_by_name(vm, &abs);
        let s = match load(vm, &abs, false, d) {
            Ok(s) => s,
            Err(m) => return fail(vm, a, url, quiet, &m),
        };
        if inner.is_empty() {
            return fail(
                vm,
                a,
                url,
                quiet,
                &format!(
                    "phar error: no directory in \"{}\", must have at least phar://{}/ for root directory (always use full path to a new phar)",
                    lossy(url),
                    lossy(&abs)
                ),
            );
        }
        return match read_entry(vm, &s, &inner) {
            Ok(data) => Ok(Some(Stream::new(
                Backing::Native(Box::new(PharRead { data, pos: 0 })),
                mode,
                url,
                "phar",
                "phar stream",
            ))),
            Err(m) => fail(vm, a, url, quiet, &m),
        };
    }
    let is_data = data_by_name(vm, &abs);
    if !is_data && readonly(vm) {
        return fail(
            vm,
            a,
            url,
            quiet,
            "phar error: write operations disabled by the php.ini setting phar.readonly",
        );
    }
    if mode.first() == Some(&b'a') {
        return fail(
            vm,
            a,
            url,
            quiet,
            "phar error: open mode append not supported",
        );
    }
    let s = match load(vm, &abs, true, is_data) {
        Ok(s) => s,
        Err(m) => return fail(vm, a, url, quiet, &m),
    };
    if inner.is_empty() || goro_ext_phar::is_meta_name(&inner) {
        let m = if inner.is_empty() {
            format!(
                "phar error: file \"\" in phar \"{}\" cannot be empty",
                lossy(&abs)
            )
        } else {
            "phar error: cannot create any files in magic \".phar\" directory".to_string()
        };
        return fail(vm, a, url, quiet, &m);
    }
    let existing = {
        let l = s.borrow();
        l.a.get(&inner).map(|e| e.is_dir)
    };
    if existing == Some(true) {
        return fail(
            vm,
            a,
            url,
            quiet,
            &format!(
                "phar error: file \"{}\" in phar \"{}\" is a directory",
                lossy(&inner),
                lossy(&abs)
            ),
        );
    }
    if mode.first() == Some(&b'x') && existing.is_some() {
        return fail(vm, a, url, quiet, "phar error: file exists");
    }
    let initial = if mode.first() == Some(&b'r') || mode.first() == Some(&b'c') {
        match existing {
            Some(_) => read_entry(vm, &s, &inner).unwrap_or_default(),
            None => Vec::new(),
        }
    } else {
        Vec::new()
    };
    s.borrow_mut().open_writes += 1;
    let w = PharWrite {
        s: s.clone(),
        name: inner,
        buf: initial,
        pos: 0,
        closed: false,
    };
    Ok(Some(Stream::new(
        Backing::Native(Box::new(w)),
        mode,
        url,
        "phar",
        "phar stream",
    )))
}

/// A stream reading an entry: seeks stay inside it (test
/// fseek_outside_bounds.phpt).
struct PharRead {
    data: Vec<u8>,
    pos: usize,
}

impl NativeStream for PharRead {
    fn read(&mut self, _vm: &mut Vm, out: &mut [u8]) -> std::io::Result<usize> {
        let n = out.len().min(self.data.len().saturating_sub(self.pos));
        out[..n].copy_from_slice(&self.data[self.pos..self.pos + n]);
        self.pos += n;
        Ok(n)
    }
    fn seek(&mut self, _vm: &mut Vm, pos: std::io::SeekFrom) -> Option<std::io::Result<u64>> {
        let n = match pos {
            std::io::SeekFrom::Start(n) => n as i64,
            std::io::SeekFrom::Current(n) => self.pos as i64 + n,
            std::io::SeekFrom::End(n) => self.data.len() as i64 + n,
        };
        if n < 0 || n as usize > self.data.len() {
            return Some(Err(std::io::Error::other("seek outside the entry")));
        }
        self.pos = n as usize;
        Some(Ok(n as u64))
    }
    fn tell(&mut self) -> Option<u64> {
        Some(self.pos as u64)
    }
    fn eof(&self) -> bool {
        self.pos >= self.data.len()
    }
}

/// A stream writing an entry: the contents go in when it is closed.
struct PharWrite {
    s: Shared,
    name: Vec<u8>,
    buf: Vec<u8>,
    pos: usize,
    closed: bool,
}

impl PharWrite {
    fn commit(&mut self, vm: &mut Vm) -> Result<(), String> {
        if self.closed {
            return Ok(());
        }
        self.closed = true;
        {
            let mut l = self.s.borrow_mut();
            l.open_writes = l.open_writes.saturating_sub(1);
            let data = std::mem::take(&mut self.buf);
            match l.a.get_mut(&self.name) {
                Some(e) => {
                    let comp = e.comp;
                    e.set_contents(data);
                    let _ = comp;
                }
                None => {
                    let e = Entry::file(&self.name, data, 0, super::classes::new_perms());
                    l.a.put(e);
                }
            }
        }
        flush(vm, &self.s)
    }
}

impl NativeStream for PharWrite {
    fn read(&mut self, _vm: &mut Vm, out: &mut [u8]) -> std::io::Result<usize> {
        let n = out.len().min(self.buf.len().saturating_sub(self.pos));
        out[..n].copy_from_slice(&self.buf[self.pos..self.pos + n]);
        self.pos += n;
        Ok(n)
    }
    fn write(&mut self, _vm: &mut Vm, data: &[u8]) -> std::io::Result<usize> {
        let end = self.pos + data.len();
        if self.buf.len() < end {
            self.buf.resize(end, 0);
        }
        self.buf[self.pos..end].copy_from_slice(data);
        self.pos = end;
        Ok(data.len())
    }
    fn seek(&mut self, _vm: &mut Vm, pos: std::io::SeekFrom) -> Option<std::io::Result<u64>> {
        let n = match pos {
            std::io::SeekFrom::Start(n) => n as i64,
            std::io::SeekFrom::Current(n) => self.pos as i64 + n,
            std::io::SeekFrom::End(n) => self.buf.len() as i64 + n,
        };
        if n < 0 {
            return Some(Err(std::io::Error::other("seek failed")));
        }
        self.pos = n as usize;
        Some(Ok(n as u64))
    }
    fn tell(&mut self) -> Option<u64> {
        Some(self.pos as u64)
    }
    fn eof(&self) -> bool {
        self.pos >= self.buf.len()
    }
    fn close(&mut self, vm: &mut Vm) -> R<()> {
        if let Err(m) = self.commit(vm) {
            vm.warn(&format!("fclose(): {m}"))?;
        }
        Ok(())
    }
}

impl Drop for PharWrite {
    fn drop(&mut self) {
        if !self.closed {
            let mut l = self.s.borrow_mut();
            l.open_writes = l.open_writes.saturating_sub(1);
        }
    }
}

/// A stat array as PHP's phar wrapper gives (observed on PHP 8.5: device
/// 12, no owner, times of the entry).
fn stat_arr(dir: bool, perms: u32, size: u32, mtime: u32, ino: u32) -> PhpArray {
    let mut a = PhpArray::new();
    let mode = if dir {
        0o40000 | (perms & 0o777)
    } else {
        0o100000 | (perms & 0o777)
    };
    for (k, v) in [
        ("dev", 12i64),
        ("ino", ino as i64),
        ("mode", mode as i64),
        ("nlink", 1),
        ("uid", 0),
        ("gid", 0),
        ("rdev", -1),
        ("size", size as i64),
        ("atime", mtime as i64),
        ("mtime", mtime as i64),
        ("ctime", mtime as i64),
        ("blksize", -1),
        ("blocks", -1),
    ] {
        a.insert(Key::from(k), Value::Int(v));
    }
    a
}

/// stat() of a phar URL.
pub(crate) fn stat_url(vm: &mut Vm, url: &[u8]) -> Option<PhpArray> {
    let (abs, inner) = split_url(vm, url).ok()?;
    let is_data = data_by_name(vm, &abs);
    let s = load(vm, &abs, false, is_data).ok()?;
    let ro = !is_data && readonly(vm);
    let l = s.borrow();
    let mut st = stat_entry(&l, &inner)?;
    if ro {
        // no write permission while phar.readonly (test phar_oo_002.phpt)
        if let Some(Value::Int(m)) = st.get_bytes(b"mode").cloned() {
            st.insert(Key::from("mode"), Value::Int(m & !0o222));
        }
    }
    Some(st)
}

fn stat_entry(l: &super::Loaded, inner: &[u8]) -> Option<PhpArray> {
    if let Some(ext) = mounted(&l.mounts, inner) {
        let m = std::fs::metadata(super::os_path(&ext)).ok()?;
        use std::os::unix::fs::MetadataExt;
        return Some(stat_arr(
            m.is_dir(),
            m.mode(),
            m.size() as u32,
            m.mtime() as u32,
            0,
        ));
    }
    if inner.is_empty() {
        return Some(stat_arr(true, 0o777, 0, 0, 0));
    }
    match l.a.get(inner) {
        Some(e) if e.is_dir => Some(stat_arr(true, e.perms(), 0, e.mtime, 0)),
        Some(e) => Some(stat_arr(
            false,
            e.perms(),
            e.size,
            e.mtime,
            goro_ext_phar::crc32(inner) & 0xffff,
        )),
        None if l.a.is_dir(inner) => Some(stat_arr(true, 0o777, 0, 0, 0)),
        None => None,
    }
}

fn url_stat(vm: &mut Vm, url: &[u8], _flags: i64) -> R<Option<PhpArray>> {
    Ok(stat_url(vm, url))
}

fn unlink(vm: &mut Vm, a: &Args, url: &[u8]) -> R<bool> {
    let (abs, inner) = match split_url(vm, url) {
        Ok(x) => x,
        Err(m) => {
            vm.fwarn(a, &m)?;
            return Ok(false);
        }
    };
    let is_data = data_by_name(vm, &abs);
    if !is_data && readonly(vm) {
        vm.fwarn(
            a,
            "phar error: write operations disabled by the php.ini setting phar.readonly",
        )?;
        return Ok(false);
    }
    let s = match load(vm, &abs, false, is_data) {
        Ok(s) => s,
        Err(m) => {
            vm.fwarn(a, &m)?;
            return Ok(false);
        }
    };
    {
        let mut l = s.borrow_mut();
        match l.a.get(&inner) {
            Some(e) if !e.is_dir => {}
            _ => {
                drop(l);
                vm.fwarn(
                    a,
                    &format!("unlink of \"{}\" failed, file does not exist", lossy(url)),
                )?;
                return Ok(false);
            }
        }
        if l.open_writes > 0 {
            drop(l);
            vm.fwarn(
                a,
                &format!(
                    "phar error: \"{}\" in phar \"{}\", has open file pointers, cannot unlink",
                    lossy(&inner),
                    lossy(&abs)
                ),
            )?;
            return Ok(false);
        }
        l.a.remove(&inner);
    }
    if let Err(m) = flush(vm, &s) {
        vm.fwarn(a, &format!("phar error: {m}"))?;
        return Ok(false);
    }
    Ok(true)
}

fn rename(vm: &mut Vm, a: &Args, from: &[u8], to: &[u8]) -> R<bool> {
    let bad = |vm: &mut Vm, a: &Args, why: &str| -> R<bool> {
        vm.fwarn(
            a,
            &format!(
                "phar error: cannot rename \"{}\" to \"{}\"{}",
                lossy(from),
                lossy(to),
                why
            ),
        )?;
        Ok(false)
    };
    if !to.starts_with(b"phar://") {
        return bad(
            vm,
            a,
            &format!(": invalid or non-writable url \"{}\"", lossy(to)),
        );
    }
    let (Ok((fa, fi)), Ok((ta, ti))) = (split_url(vm, from), split_url(vm, to)) else {
        return bad(
            vm,
            a,
            &format!(": invalid or non-writable url \"{}\"", lossy(to)),
        );
    };
    if fa != ta {
        return bad(vm, a, ", not within the same phar archive");
    }
    let is_data = data_by_name(vm, &fa);
    if !is_data && readonly(vm) {
        vm.fwarn(
            a,
            "phar error: write operations disabled by the php.ini setting phar.readonly",
        )?;
        return Ok(false);
    }
    let s = match load(vm, &fa, false, is_data) {
        Ok(s) => s,
        Err(_) => {
            return bad(vm, a, ": invalid or non-writable url");
        }
    };
    {
        let mut l = s.borrow_mut();
        if l.a.get(&fi).is_some() {
            l.a.rename(&fi, &ti);
        } else if l.a.is_dir(&fi) {
            // a directory: everything under it
            let mut p = fi.clone();
            p.push(b'/');
            let names: Vec<Vec<u8>> =
                l.a.entries()
                    .iter()
                    .filter(|e| e.name.starts_with(&p))
                    .map(|e| e.name.clone())
                    .collect();
            for n in names {
                let mut t = ti.clone();
                t.extend_from_slice(&n[fi.len()..]);
                l.a.rename(&n, &t);
            }
        } else {
            drop(l);
            return bad(vm, a, " from extracted phar archive, source does not exist");
        }
    }
    if let Err(m) = flush(vm, &s) {
        vm.fwarn(a, &format!("phar error: {m}"))?;
        return Ok(false);
    }
    Ok(true)
}

fn mkdir(vm: &mut Vm, a: &Args, url: &[u8], _mode: i64, _opts: i64) -> R<bool> {
    let (abs, inner) = match split_url(vm, url) {
        Ok(x) => x,
        Err(_) => {
            vm.fwarn(
                a,
                &format!(
                    "phar error: cannot create directory \"{}\", no phar archive specified",
                    lossy(url)
                ),
            )?;
            return Ok(false);
        }
    };
    let is_data = data_by_name(vm, &abs);
    if !is_data && readonly(vm) {
        vm.fwarn(
            a,
            &format!(
                "phar error: cannot create directory \"{}\", write operations disabled",
                lossy(&inner)
            ),
        )?;
        return Ok(false);
    }
    let s = match load(vm, &abs, true, is_data) {
        Ok(s) => s,
        Err(m) => {
            vm.fwarn(a, &m)?;
            return Ok(false);
        }
    };
    let err = {
        let l = s.borrow();
        if inner.is_empty() {
            Some(format!(
                "phar error: cannot create directory \"\" in phar \"{}\", phar error: invalid path \"\" must not be empty",
                lossy(&abs)
            ))
        } else {
            match l.a.get(&inner) {
                Some(e) if e.is_dir => Some(format!(
                    "phar error: cannot create directory \"{}\" in phar \"{}\", directory already exists",
                    lossy(&inner),
                    lossy(&abs)
                )),
                Some(_) => Some(format!(
                    "phar error: cannot create directory \"{}\" in phar \"{}\", phar error: path \"{}\" exists and is a not a directory",
                    lossy(&inner),
                    lossy(&abs),
                    lossy(&inner)
                )),
                None if l.a.is_dir(&inner) => Some(format!(
                    "phar error: cannot create directory \"{}\" in phar \"{}\", directory already exists",
                    lossy(&inner),
                    lossy(&abs)
                )),
                None => None,
            }
        }
    };
    if let Some(m) = err {
        vm.fwarn(a, &m)?;
        return Ok(false);
    }
    s.borrow_mut().a.put(Entry::dir(&inner, 0));
    if let Err(m) = flush(vm, &s) {
        vm.fwarn(a, &format!("phar error: {m}"))?;
        return Ok(false);
    }
    Ok(true)
}

fn rmdir(vm: &mut Vm, a: &Args, url: &[u8]) -> R<bool> {
    let (abs, inner) = match split_url(vm, url) {
        Ok(x) => x,
        Err(_) => {
            vm.fwarn(
                a,
                &format!(
                    "phar error: cannot remove directory \"{}\", no phar archive specified, or phar archive does not exist",
                    lossy(url)
                ),
            )?;
            return Ok(false);
        }
    };
    let is_data = data_by_name(vm, &abs);
    if !is_data && readonly(vm) {
        vm.fwarn(
            a,
            &format!(
                "phar error: cannot rmdir directory \"{}\", write operations disabled",
                lossy(&inner)
            ),
        )?;
        return Ok(false);
    }
    let s = match load(vm, &abs, false, is_data) {
        Ok(s) => s,
        Err(_) => {
            vm.fwarn(
                a,
                &format!(
                    "phar error: cannot remove directory \"{}\", no phar archive specified, or phar archive does not exist",
                    lossy(url)
                ),
            )?;
            return Ok(false);
        }
    };
    let err = {
        let l = s.borrow();
        match l.a.get(&inner) {
            Some(e) if !e.is_dir => Some(format!(
                "phar error: cannot remove directory \"{}\" in phar \"{}\", phar error: path \"{}\" exists and is a not a directory",
                lossy(&inner),
                lossy(&abs),
                lossy(&inner)
            )),
            _ if !l.a.is_dir(&inner) || inner.is_empty() => Some(format!(
                "phar error: cannot remove directory \"{}\" in phar \"{}\", directory does not exist",
                lossy(&inner),
                lossy(&abs)
            )),
            _ if !l.a.list_dir(&inner).is_empty() => Some("phar error: Directory not empty".into()),
            _ => None,
        }
    };
    if let Some(m) = err {
        vm.fwarn(a, &m)?;
        return Ok(false);
    }
    s.borrow_mut().a.remove(&inner);
    if let Err(m) = flush(vm, &s) {
        vm.fwarn(a, &format!("phar error: {m}"))?;
        return Ok(false);
    }
    Ok(true)
}

fn opendir(vm: &mut Vm, a: &Args, url: &[u8]) -> R<Option<Vec<Vec<u8>>>> {
    let dfail = |vm: &mut Vm, m: &str| -> R<Option<Vec<Vec<u8>>>> {
        vm.warn(&format!(
            "{}({}): Failed to open directory: {}",
            a.fname(),
            lossy(url),
            m
        ))?;
        Ok(None)
    };
    // "phar://alias" without the root slash (tests 017.phpt, 027.phpt)
    let rest = &url[7.min(url.len())..];
    let unknown = format!(
        "phar error: invalid url or non-existent phar \"{}\"\nphar url \"{}\" is unknown",
        lossy(url),
        lossy(url)
    );
    if rest.is_empty() || (!rest.contains(&b'/') && super::state(vm).aliases.contains_key(rest)) {
        return dfail(
            vm,
            &format!(
                "phar error: no directory in \"{}\", must have at least {}/ for root directory (always use full path to a new phar)\nphar url \"{}\" is unknown",
                lossy(url),
                lossy(url),
                lossy(url)
            ),
        );
    }
    let (abs, inner) = match split_url(vm, url) {
        Ok(x) => x,
        Err(_) => return dfail(vm, &unknown),
    };
    let is_data = data_by_name(vm, &abs);
    let s = match load(vm, &abs, false, is_data) {
        Ok(s) => s,
        Err(m) if m.starts_with("phar error: invalid url") => return dfail(vm, &unknown),
        Err(m) => return dfail(vm, &m),
    };
    let l = s.borrow();
    if !l.a.is_dir(&inner) || l.a.get(&inner).is_some_and(|e| !e.is_dir) {
        drop(l);
        return dfail(vm, &unknown);
    }
    let mut names = l.a.list_dir(&inner);
    // mounted directories show at the root
    for (m, _) in &l.mounts {
        let parent = match m.iter().rposition(|&c| c == b'/') {
            Some(p) => &m[..p],
            None => &b""[..],
        };
        if parent == &inner[..] {
            let n = m[parent.len()..]
                .strip_prefix(b"/")
                .unwrap_or(&m[parent.len()..]);
            if !names.iter().any(|x| x == n) {
                names.push(n.to_vec());
            }
        }
    }
    names.sort();
    Ok(Some(names))
}

fn metadata(vm: &mut Vm, a: &Args, url: &[u8], option: i64, value: &Value) -> R<bool> {
    let (abs, inner) = match split_url(vm, url) {
        Ok(x) => x,
        Err(m) => {
            vm.fwarn(a, &m)?;
            return Ok(false);
        }
    };
    let is_data = data_by_name(vm, &abs);
    if !is_data && readonly(vm) {
        return Ok(false);
    }
    let Ok(s) = load(vm, &abs, option == 1, is_data) else {
        return Ok(false);
    };
    {
        let mut l = s.borrow_mut();
        match option {
            6 => {
                let m = match value {
                    Value::Int(n) => *n as u32,
                    _ => 0o644,
                };
                let Some(e) = l.a.get_mut(&inner) else {
                    return Ok(false);
                };
                e.flags = (e.flags & !0o777) | (m & 0o777);
            }
            _ => {
                if l.a.get(&inner).is_none() {
                    l.a.put(Entry::file(&inner, Vec::new(), 0, 0o666));
                }
            }
        }
    }
    Ok(flush(vm, &s).is_ok())
}

fn include(vm: &mut Vm, fname: &str, url: &[u8], quiet: bool) -> R<Option<(Vec<u8>, Vec<u8>)>> {
    let (abs, inner) = match split_url(vm, url) {
        Ok(x) => x,
        Err(m) => {
            if !quiet {
                vm.warn(&format!(
                    "{fname}({}): Failed to open stream: {m}",
                    lossy(url)
                ))?;
            }
            return Ok(None);
        }
    };
    let is_data = data_by_name(vm, &abs);
    let s = match load(vm, &abs, false, is_data) {
        Ok(s) => s,
        Err(m) => {
            if !quiet {
                vm.warn(&format!(
                    "{fname}({}): Failed to open stream: {m}",
                    lossy(url)
                ))?;
            }
            return Ok(None);
        }
    };
    // the archive itself: its stub runs (test 017.phpt)
    if inner.is_empty() {
        let Ok(src) = std::fs::read(super::os_path(&abs)) else {
            return Ok(None);
        };
        let src = super::include_filter(vm, &abs, &src).unwrap_or(src);
        return Ok(Some((src, abs)));
    }
    match read_entry(vm, &s, &inner) {
        Ok(src) => {
            let mut name = b"phar://".to_vec();
            name.extend_from_slice(&abs);
            name.push(b'/');
            name.extend_from_slice(&inner);
            Ok(Some((src, name)))
        }
        Err(m) => {
            if !quiet {
                vm.warn(&format!(
                    "{fname}({}): Failed to open stream: {m}",
                    lossy(url)
                ))?;
            }
            Ok(None)
        }
    }
}
