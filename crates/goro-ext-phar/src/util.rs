//! php-src ext/phar/util.c: entry lookup and creation, entry file pointers,
//! signatures, archive lookup.

use std::cell::RefCell;
use std::rc::Rc;

use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::Vm;

use crate::archive::*;
use crate::pathcheck::{Pcr, path_check};

// ---------------------------------------------------------------------------
// compression

/// zlib.inflate / zlib.deflate with the given window bits (raw deflate for
/// -15, gzip for 31, auto-detect for 47).
pub fn zlib_inflate(data: &[u8], window_bits: i32) -> Option<Vec<u8>> {
    let mut inf = goro_zlib::Inflate::new(window_bits).ok()?;
    let mut out = Vec::with_capacity(data.len() * 2 + 64);
    let mut pos = 0usize;
    let mut chunk = vec![0u8; 65536];
    loop {
        let mut io = goro_zlib::Io::new(&data[pos..], &mut chunk);
        let st = inf.inflate(&mut io, goro_zlib::Z_NO_FLUSH);
        let consumed = io.in_pos;
        let produced = io.out_pos;
        pos += consumed;
        out.extend_from_slice(&chunk[..produced]);
        match st {
            goro_zlib::Z_STREAM_END => return Some(out),
            goro_zlib::Z_OK => {
                if consumed == 0 && produced == 0 {
                    // truncated input
                    return if pos >= data.len() { Some(out) } else { None };
                }
            }
            goro_zlib::Z_BUF_ERROR => {
                if pos >= data.len() {
                    return Some(out);
                }
                if consumed == 0 && produced == 0 {
                    return None;
                }
            }
            _ => return None,
        }
    }
}

pub fn zlib_deflate(data: &[u8], window_bits: i32) -> Option<Vec<u8>> {
    let mut d = goro_zlib::Deflate::new(
        goro_zlib::Z_DEFAULT_COMPRESSION,
        goro_zlib::Z_DEFLATED,
        window_bits,
        goro_zlib::MAX_MEM_LEVEL,
        goro_zlib::Z_DEFAULT_STRATEGY,
    )
    .ok()?;
    let mut out = Vec::new();
    let mut chunk = vec![0u8; 65536];
    let mut pos = 0usize;
    loop {
        let mut io = goro_zlib::Io::new(&data[pos..], &mut chunk);
        let st = d.deflate(&mut io, goro_zlib::Z_FINISH);
        pos += io.in_pos;
        let produced = io.out_pos;
        out.extend_from_slice(&chunk[..produced]);
        match st {
            goro_zlib::Z_STREAM_END => return Some(out),
            goro_zlib::Z_OK | goro_zlib::Z_BUF_ERROR => {
                if produced == 0 && io_done(pos, data.len()) && st == goro_zlib::Z_BUF_ERROR {
                    return None;
                }
            }
            _ => return None,
        }
    }
}

fn io_done(pos: usize, len: usize) -> bool {
    pos >= len
}

fn call_builtin(vm: &mut Vm, name: &[u8], args: &[Value]) -> Option<Value> {
    let f = vm.functions.get(name).copied()?;
    f(vm, args).ok()
}

pub fn bz2_compress(vm: &mut Vm, data: &[u8]) -> Option<Vec<u8>> {
    match call_builtin(
        vm,
        b"bzcompress",
        &[
            Value::String(PhpString::from_bytes(data)),
            Value::Long(9),
            Value::Long(0),
        ],
    )? {
        Value::String(s) => Some(s.as_bytes().to_vec()),
        _ => None,
    }
}

pub fn bz2_decompress(vm: &mut Vm, data: &[u8]) -> Option<Vec<u8>> {
    match call_builtin(
        vm,
        b"bzdecompress",
        &[Value::String(PhpString::from_bytes(data))],
    )? {
        Value::String(s) => Some(s.as_bytes().to_vec()),
        _ => None,
    }
}

/// The filter of phar_compress_filter() applied to `data`.
pub fn compress(vm: &mut Vm, flags: u32, data: &[u8]) -> Option<Vec<u8>> {
    match flags & PHAR_ENT_COMPRESSION_MASK {
        PHAR_ENT_COMPRESSED_GZ => zlib_deflate(data, -15),
        PHAR_ENT_COMPRESSED_BZ2 => bz2_compress(vm, data),
        _ => Some(data.to_vec()),
    }
}

/// The filter of phar_decompress_filter() applied to `data`.
pub fn decompress(vm: &mut Vm, flags: u32, data: &[u8]) -> Option<Vec<u8>> {
    match flags & PHAR_ENT_COMPRESSION_MASK {
        PHAR_ENT_COMPRESSED_GZ => zlib_inflate(data, -15),
        PHAR_ENT_COMPRESSED_BZ2 => bz2_decompress(vm, data),
        _ => Some(data.to_vec()),
    }
}

/// phar_decompress_filter(entry, 1)
pub fn decompress_filter_name(e: &Entry) -> &'static str {
    let flags = if e.is_modified { e.old_flags } else { e.flags };
    match flags & PHAR_ENT_COMPRESSION_MASK {
        PHAR_ENT_COMPRESSED_GZ => "zlib.inflate",
        PHAR_ENT_COMPRESSED_BZ2 => "bzip2.decompress",
        _ => "unknown",
    }
}

// ---------------------------------------------------------------------------
// links

/// phar_get_link_location()
fn get_link_location(entry: &EntryRef) -> Option<Vec<u8>> {
    let mut e = entry.borrow_mut();
    let link = e.link.clone()?;
    if link.first() == Some(&b'/') {
        return Some(link[1..].to_vec());
    }
    if let Some(p) = e.filename.iter().rposition(|&c| c == b'/') {
        let new_name = e.filename[..p].to_vec();
        let mut ret = new_name.clone();
        ret.push(b'/');
        ret.extend_from_slice(&link);
        // (php-src replaces the entry's file name with its directory here)
        e.filename = new_name;
        return Some(ret);
    }
    Some(link)
}

/// phar_get_link_source()
pub fn get_link_source(entry: &EntryRef) -> Option<EntryRef> {
    get_link_source_depth(entry, 0)
}

fn get_link_source_depth(entry: &EntryRef, depth: usize) -> Option<EntryRef> {
    let link = entry.borrow().link.clone();
    let Some(link) = link else {
        return Some(entry.clone());
    };
    if depth > 64 {
        return None;
    }
    let loc = get_link_location(entry).unwrap_or_default();
    let phar = entry.borrow().phar();
    let found = {
        let p = phar.borrow();
        p.find(&link).or_else(|| p.find(&loc))
    };
    found.and_then(|le| get_link_source_depth(&le, depth + 1))
}

// ---------------------------------------------------------------------------
// entry file pointers

/// phar_open_archive_fp()
pub fn open_archive_fp(_vm: &mut Vm, phar: &ArcRef) -> Res<()> {
    if phar.borrow().fp.is_some() {
        return Ok(());
    }
    let fname = phar.borrow().fname.clone();
    match read_file(&fname) {
        Some(d) => {
            phar.borrow_mut().fp = Some(Rc::new(RefCell::new(d)));
            Ok(())
        }
        None => Err(None),
    }
}

/// phar_get_efp(): the stream holding the entry's contents.
pub fn get_efp(vm: &mut Vm, entry: &EntryRef, follow_links: bool) -> Option<Buf> {
    if follow_links && entry.borrow().link.is_some() {
        if let Some(le) = get_link_source(entry) {
            if !Rc::ptr_eq(&le, entry) {
                return get_efp(vm, &le, true);
            }
        }
    }
    let fp_type = entry.borrow().fp_type;
    match fp_type {
        FpType::Fp => {
            let phar = entry.borrow().phar();
            if phar.borrow().fp.is_none() {
                let _ = open_archive_fp(vm, &phar);
            }
            phar.borrow().fp.clone()
        }
        FpType::Ufp => {
            let phar = entry.borrow().phar();
            phar.borrow().ufp.clone()
        }
        FpType::Mod => entry.borrow().fp.clone(),
        FpType::Tmp => {
            if entry.borrow().fp.is_none() {
                let tmp = entry.borrow().tmp.clone().unwrap_or_default();
                let data = if has_scheme(&tmp) {
                    goro_ext_standard::native_streams::read_whole_url(vm, &tmp, false)
                        .ok()
                        .flatten()
                } else {
                    read_file(&tmp)
                };
                if let Some(d) = data {
                    entry.borrow_mut().fp = Some(Rc::new(RefCell::new(d)));
                }
            }
            entry.borrow().fp.clone()
        }
    }
}

/// Read `len` bytes of an entry's stream from its offset (phar_seek_efp(0)
/// + php_stream_read()).
fn read_efp(vm: &mut Vm, entry: &EntryRef, follow_links: bool, len: usize) -> Option<Vec<u8>> {
    let fp = get_efp(vm, entry, follow_links)?;
    let src = if follow_links {
        get_link_source(entry).unwrap_or_else(|| entry.clone())
    } else {
        entry.clone()
    };
    if src.borrow().is_dir {
        return Some(Vec::new());
    }
    let off = src.borrow().offset.max(0) as usize;
    let data = fp.borrow();
    if off > data.len() {
        // the archive's own file can be sought past its end (it may have
        // been rewritten in place since it was opened): nothing to read
        if src.borrow().fp_type == FpType::Fp {
            return Some(Vec::new());
        }
        return None;
    }
    let end = (off + len).min(data.len());
    Some(data[off..end].to_vec())
}

/// The uncompressed contents of an entry whose fp holds them (after
/// phar_open_entry_fp()).
pub fn read_entry_contents(vm: &mut Vm, entry: &EntryRef, follow_links: bool) -> Option<Vec<u8>> {
    let src = if follow_links {
        get_link_source(entry).unwrap_or_else(|| entry.clone())
    } else {
        entry.clone()
    };
    let len = src.borrow().uncompressed_filesize as usize;
    read_efp(vm, entry, follow_links, len)
}

/// compressed_filesize bytes of the entry's stream (the raw stored data).
pub fn read_entry_raw(vm: &mut Vm, entry: &EntryRef, _oldfile: Option<&Buf>) -> Option<Vec<u8>> {
    let len = entry.borrow().compressed_filesize as usize;
    read_efp(vm, entry, false, len)
}

/// phar_create_writeable_entry()
fn create_writeable_entry(phar: &ArcRef, entry: &EntryRef) -> Res<()> {
    let mut e = entry.borrow_mut();
    if e.fp_type == FpType::Mod {
        if let Some(fp) = &e.fp {
            fp.borrow_mut().clear();
        }
    } else {
        if e.link.is_some() {
            e.link = None;
            e.tar_type = if e.is_tar { TAR_FILE } else { 0 };
        }
        e.fp = Some(new_buf());
    }
    e.old_flags = e.flags;
    e.is_modified = true;
    phar.borrow_mut().is_modified = true;
    e.uncompressed_filesize = 0;
    e.compressed_filesize = 0;
    e.crc32 = 0;
    e.flags = PHAR_ENT_PERM_DEF_FILE;
    e.fp_type = FpType::Mod;
    e.offset = 0;
    Ok(())
}

/// phar_separate_entry_fp()
fn separate_entry_fp(vm: &mut Vm, entry: &EntryRef) -> Res<()> {
    open_entry_fp(vm, entry, true)?;
    if entry.borrow().fp_type == FpType::Mod {
        return Ok(());
    }
    let link = get_link_source(entry).unwrap_or_else(|| entry.clone());
    let Some(data) = read_entry_contents(vm, &link, false) else {
        let e = entry.borrow();
        return fail(format!(
            "phar error: cannot separate entry file \"{}\" contents in phar archive \"{}\" for write access",
            l(&e.filename),
            l(&e.phar().borrow().fname)
        ));
    };
    let mut e = entry.borrow_mut();
    if e.link.is_some() {
        e.link = None;
        e.tar_type = if e.is_tar { TAR_FILE } else { 0 };
    }
    e.offset = 0;
    e.fp = Some(Rc::new(RefCell::new(data)));
    e.fp_type = FpType::Mod;
    e.is_modified = true;
    Ok(())
}

/// phar_get_entry_data(): Ok(None) when the entry does not exist but may
/// be created.
#[allow(clippy::too_many_arguments)]
pub fn get_entry_data(
    vm: &mut Vm,
    fname: &[u8],
    path: &[u8],
    mode: &[u8],
    allow_dir: u8,
    security: bool,
) -> Res<Option<EntryData>> {
    let m0 = mode.first().copied().unwrap_or(b'r');
    let m1 = mode.get(1).copied().unwrap_or(0);
    let for_write = m0 != b'r' || m1 == b'+';
    let for_append = m0 == b'a';
    let for_create = m0 != b'r';
    let for_trunc = m0 == b'w';
    let phar = get_archive(fname, None)?;
    if for_write && readonly() && !phar.borrow().is_data {
        return fail(format!(
            "phar error: file \"{}\" in phar \"{}\" cannot be opened for writing, disabled by ini setting",
            l(path),
            l(fname)
        ));
    }
    if path.is_empty() {
        return fail(format!(
            "phar error: file \"\" in phar \"{}\" must not be empty",
            l(fname)
        ));
    }
    let creating = for_create && (!readonly() || phar.borrow().is_data);
    let entry = if allow_dir != 0 {
        get_entry_info_dir(vm, &phar, path, allow_dir, security)
    } else {
        get_entry_info(vm, &phar, path, security)
    };
    let entry = match entry {
        Ok(e) => e,
        Err(err) => {
            if creating {
                return Ok(None);
            }
            return Err(err);
        }
    };
    let (is_modified, fp_refcount, is_deleted) = {
        let e = entry.borrow();
        (e.is_modified, e.fp_refcount, e.is_deleted)
    };
    if is_modified && !for_write {
        return fail(format!(
            "phar error: file \"{}\" in phar \"{}\" cannot be opened for reading, writable file pointers are open",
            l(path),
            l(fname)
        ));
    }
    if fp_refcount != 0 && for_write {
        return fail(format!(
            "phar error: file \"{}\" in phar \"{}\" cannot be opened for writing, readable file pointers are open",
            l(path),
            l(fname)
        ));
    }
    if is_deleted {
        if !for_create {
            return Err(None);
        }
        entry.borrow_mut().is_deleted = false;
    }
    if entry.borrow().is_dir {
        let ret = EntryData {
            phar: phar.clone(),
            fp: None,
            position: 0,
            zero: 0,
            internal_file: entry.clone(),
        };
        phar.borrow_mut().refcount += 1;
        entry.borrow_mut().fp_refcount += 1;
        return Ok(Some(ret));
    }
    if entry.borrow().fp_type == FpType::Mod {
        if for_trunc {
            create_writeable_entry(&phar, &entry)?;
        }
        // (for_append: the position starts at the end)
    } else if for_write {
        {
            let mut e = entry.borrow_mut();
            if e.link.is_some() {
                e.link = None;
                e.tar_type = if e.is_tar { TAR_FILE } else { 0 };
            }
        }
        if for_trunc {
            create_writeable_entry(&phar, &entry)?;
        } else {
            separate_entry_fp(vm, &entry)?;
        }
    } else {
        open_entry_fp(vm, &entry, true)?;
    }
    let fp = get_efp(vm, &entry, true);
    let zero = if entry.borrow().link.is_some() {
        let Some(link) = get_link_source(&entry) else {
            return Err(None);
        };
        link.borrow().offset
    } else {
        entry.borrow().offset
    };
    let position = if for_append && entry.borrow().fp_type == FpType::Mod {
        entry.borrow().uncompressed_filesize as i64
    } else {
        0
    };
    entry.borrow_mut().fp_refcount += 1;
    phar.borrow_mut().refcount += 1;
    let _ = position;
    Ok(Some(EntryData {
        phar,
        fp,
        position: 0,
        zero,
        internal_file: entry,
    }))
}

/// phar_get_or_create_entry_data()
pub fn get_or_create_entry_data(
    vm: &mut Vm,
    fname: &[u8],
    path: &[u8],
    mode: &[u8],
    allow_dir: u8,
    security: bool,
) -> Res<EntryData> {
    let is_dir = path.last() == Some(&b'/');
    let phar = get_archive(fname, None)?;
    if let Some(ret) = get_entry_data(vm, fname, path, mode, allow_dir, security)? {
        return Ok(ret);
    }
    let chk = path_check(path);
    if chk.result.is_err() {
        return fail(format!(
            "phar error: invalid path \"{}\" contains {}",
            l(path),
            chk.error
        ));
    }
    let mut path = chk.path(path).to_vec();
    let mut etemp = Entry {
        fp_type: FpType::Mod,
        fp: Some(new_buf()),
        fp_refcount: 1,
        ..Default::default()
    };
    if allow_dir == 2 {
        etemp.is_dir = true;
        etemp.flags = PHAR_ENT_PERM_DEF_DIR;
        etemp.old_flags = PHAR_ENT_PERM_DEF_DIR;
    } else {
        etemp.flags = PHAR_ENT_PERM_DEF_FILE;
        etemp.old_flags = PHAR_ENT_PERM_DEF_FILE;
    }
    if is_dir && !path.is_empty() {
        path.pop();
    }
    add_virtual_dirs(&mut phar.borrow_mut(), &path);
    etemp.is_modified = true;
    etemp.timestamp = now();
    etemp.is_crc_checked = true;
    etemp.phar = Rc::downgrade(&phar);
    etemp.filename = path.clone();
    etemp.is_zip = phar.borrow().is_zip;
    if phar.borrow().is_tar {
        etemp.is_tar = true;
        etemp.tar_type = if etemp.is_dir { TAR_DIR } else { TAR_FILE };
    }
    let fp = etemp.fp.clone();
    let entry = etemp.into_ref();
    {
        let mut p = phar.borrow_mut();
        if p.manifest.contains_key(&path) {
            return fail(format!(
                "phar error: unable to add new entry \"{}\" to phar \"{}\"",
                l(&path),
                l(&p.fname)
            ));
        }
        p.manifest.insert(path, entry.clone());
        p.refcount += 1;
    }
    Ok(EntryData {
        phar,
        fp,
        position: 0,
        zero: 0,
        internal_file: entry,
    })
}

/// phar_copy_entry_fp(): copy the contents of `source` into `dest`.
pub fn copy_entry_fp(vm: &mut Vm, source: &EntryRef, dest: &mut Entry) -> Res<()> {
    open_entry_fp(vm, source, true)?;
    if dest.link.is_some() {
        dest.link = None;
        dest.tar_type = if dest.is_tar { TAR_FILE } else { 0 };
    }
    dest.fp_type = FpType::Mod;
    dest.offset = 0;
    dest.is_modified = true;
    let link = get_link_source(source).unwrap_or_else(|| source.clone());
    match read_entry_contents(vm, &link, false) {
        Some(d) => {
            dest.fp = Some(Rc::new(RefCell::new(d)));
            Ok(())
        }
        None => {
            dest.fp = None;
            dest.fp_type = FpType::Fp;
            let s = source.borrow();
            fail(format!(
                "phar error: unable to copy contents of file \"{}\" to \"{}\" in phar archive \"{}\"",
                l(&s.filename),
                l(&dest.filename),
                l(&s.phar().borrow().fname)
            ))
        }
    }
}

/// phar_open_entry_fp(): decompress a compressed entry into the archive's
/// ufp (and check its crc32).
pub fn open_entry_fp(vm: &mut Vm, entry: &EntryRef, follow_links: bool) -> Res<()> {
    if follow_links && entry.borrow().link.is_some() {
        if let Some(le) = get_link_source(entry) {
            if !Rc::ptr_eq(&le, entry) {
                return open_entry_fp(vm, &le, true);
            }
        }
    }
    if entry.borrow().is_modified {
        return Ok(());
    }
    let fp_type = entry.borrow().fp_type;
    if fp_type == FpType::Tmp {
        get_efp(vm, entry, false);
        return Ok(());
    }
    if fp_type != FpType::Fp {
        return Ok(());
    }
    let phar = entry.borrow().phar();
    if phar.borrow().fp.is_none() && open_archive_fp(vm, &phar).is_err() {
        return fail(format!(
            "phar error: Cannot open phar archive \"{}\" for reading",
            l(&phar.borrow().fname)
        ));
    }
    let (old_flags, flags) = {
        let e = entry.borrow();
        (e.old_flags, e.flags)
    };
    let pfp = phar.borrow().fp.clone().unwrap();
    let mut zero = entry.borrow().offset;
    let crc = entry.borrow().crc32;
    if (old_flags != 0 && old_flags & PHAR_ENT_COMPRESSION_MASK == 0)
        || flags & PHAR_ENT_COMPRESSION_MASK == 0
    {
        return postprocess_file(vm, &phar, entry, &pfp, &mut zero, crc, 1);
    }
    if phar.borrow().ufp.is_none() {
        phar.borrow_mut().ufp = Some(new_buf());
    }
    postprocess_file(vm, &phar, entry, &pfp, &mut zero, crc, 1)?;
    let ufp = phar.borrow().ufp.clone().unwrap();
    let (offset, csize, usize_, eflags) = {
        let e = entry.borrow();
        let fl = if e.is_modified { e.old_flags } else { e.flags };
        (
            e.offset.max(0) as usize,
            e.compressed_filesize as usize,
            e.uncompressed_filesize as usize,
            fl,
        )
    };
    let mismatch = || {
        let e = entry.borrow();
        fail(format!(
            "phar error: internal corruption of phar \"{}\" (actual filesize mismatch on file \"{}\")",
            l(&phar.borrow().fname),
            l(&e.filename)
        ))
    };
    let raw = {
        let d = pfp.borrow();
        if offset + csize > d.len() {
            if usize_ == 0 {
                Vec::new()
            } else {
                return mismatch();
            }
        } else {
            d[offset..offset + csize].to_vec()
        }
    };
    let out = if usize_ == 0 {
        Some(Vec::new())
    } else {
        decompress(vm, eflags, &raw)
    };
    let Some(out) = out else {
        return mismatch();
    };
    if out.len() != usize_ {
        return mismatch();
    }
    let loc = {
        let mut u = ufp.borrow_mut();
        let loc = u.len();
        u.extend_from_slice(&out);
        loc as i64
    };
    {
        let mut e = entry.borrow_mut();
        e.old_flags = e.flags;
        e.fp_type = FpType::Ufp;
        e.offset = loc;
    }
    let mut zero = loc;
    postprocess_file(vm, &phar, entry, &ufp, &mut zero, crc, 0)
}

/// phar_open_jit()
pub fn open_jit(vm: &mut Vm, phar: &ArcRef, entry: &EntryRef) -> Res<()> {
    open_entry_fp(vm, entry, true)?;
    if get_efp(vm, entry, true).is_none() {
        return fail(format!(
            "phar error: cannot seek to start of file \"{}\" in phar \"{}\"",
            l(&entry.borrow().filename),
            l(&phar.borrow().fname)
        ));
    }
    Ok(())
}

/// phar_free_alias()
#[allow(clippy::result_unit_err)]
pub fn free_alias(phar: &ArcRef) -> Result<(), ()> {
    if phar.borrow().refcount != 0 {
        return Err(());
    }
    let fname = phar.borrow().fname.clone();
    if !fname_map_del(&fname) {
        return Err(());
    }
    invalidate_last();
    Ok(())
}

fn set_last(fd: &ArcRef, alias: Option<Vec<u8>>) {
    let (name, a) = {
        let p = fd.borrow();
        (p.fname.clone(), p.alias.clone())
    };
    g(|g| {
        g.last_phar = Some(fd.clone());
        g.last_phar_name = name;
        g.last_alias = alias.or(a);
    });
}

/// phar_get_archive(): look up an archive by file name or alias.
pub fn get_archive(fname: &[u8], alias: Option<&[u8]>) -> Res<ArcRef> {
    request_initialize();
    let alias = alias.filter(|a| !a.is_empty());
    let (last, last_name, last_alias) = g(|g| {
        (
            g.last_phar.clone(),
            g.last_phar_name.clone(),
            g.last_alias.clone(),
        )
    });
    if let Some(lp) = &last {
        if fname == last_name.as_slice() {
            if let Some(a) = alias {
                let (temp, cur_alias, lfname) = {
                    let p = lp.borrow();
                    (p.is_temporary_alias, p.alias.clone(), p.fname.clone())
                };
                if !temp && cur_alias.as_deref() != Some(a) {
                    return fail(format!(
                        "alias \"{}\" is already used for archive \"{}\" cannot be overloaded with \"{}\"",
                        l(a),
                        l(&lfname),
                        l(fname)
                    ));
                }
                if let Some(ca) = cur_alias.filter(|c| !c.is_empty()) {
                    g(|g| g.alias_map.shift_remove(&ca));
                }
                g(|g| {
                    if !g.alias_map.contains_key(a) {
                        g.alias_map.insert(a.to_vec(), lp.clone());
                    }
                    g.last_alias = Some(a.to_vec());
                });
            }
            return Ok(lp.clone());
        }
        if let Some(a) = alias {
            if last_alias.as_deref() == Some(a) {
                return alias_success(lp, a, fname);
            }
        }
    }
    if let Some(a) = alias {
        if let Some(fd) = find_alias(a) {
            return alias_success(&fd, a, fname);
        }
    }
    if !fname.is_empty() {
        if let Some(fd) = find_fname(fname) {
            if let Some(a) = alias {
                let (temp, cur_alias, ffname) = {
                    let p = fd.borrow();
                    (p.is_temporary_alias, p.alias.clone(), p.fname.clone())
                };
                if !temp && cur_alias.as_deref() != Some(a) {
                    return fail(format!(
                        "alias \"{}\" is already used for archive \"{}\" cannot be overloaded with \"{}\"",
                        l(a),
                        l(&ffname),
                        l(fname)
                    ));
                }
                if let Some(ca) = cur_alias.filter(|c| !c.is_empty()) {
                    g(|g| g.alias_map.shift_remove(&ca));
                }
                g(|g| {
                    if !g.alias_map.contains_key(a) {
                        g.alias_map.insert(a.to_vec(), fd.clone());
                    }
                });
            }
            set_last(&fd, None);
            return Ok(fd);
        }
        if let Some(fd) = find_alias(fname) {
            set_last(&fd, None);
            return Ok(fd);
        }
        let Some(rp) = expand_filepath(fname) else {
            return Err(None);
        };
        if let Some(fd) = find_fname(&rp) {
            if let Some(a) = alias {
                g(|g| {
                    if !g.alias_map.contains_key(a) {
                        g.alias_map.insert(a.to_vec(), fd.clone());
                    }
                });
            }
            set_last(&fd, None);
            return Ok(fd);
        }
    }
    Err(None)
}

fn alias_success(fd: &ArcRef, alias: &[u8], fname: &[u8]) -> Res<ArcRef> {
    let ffname = fd.borrow().fname.clone();
    if !fname.is_empty() && fname != ffname.as_slice() {
        let msg = format!(
            "alias \"{}\" is already used for archive \"{}\" cannot be overloaded with \"{}\"",
            l(alias),
            l(&ffname),
            l(fname)
        );
        if free_alias(fd).is_ok() {
            return Err(None);
        }
        return fail(msg);
    }
    set_last(fd, Some(alias.to_vec()));
    Ok(fd.clone())
}

/// phar_get_entry_info()
pub fn get_entry_info(vm: &mut Vm, phar: &ArcRef, path: &[u8], security: bool) -> Res<EntryRef> {
    get_entry_info_dir(vm, phar, path, 0, security)
}

/// phar_get_entry_info_dir(): `dir` 0 for files only, 1 for files and
/// (temporary) directories, 2 for pre-existing directories only.
pub fn get_entry_info_dir(
    vm: &mut Vm,
    phar: &ArcRef,
    path: &[u8],
    dir: u8,
    security: bool,
) -> Res<EntryRef> {
    let is_dir = path.last() == Some(&b'/');
    if security && path.starts_with(b".phar") {
        return fail(
            "phar error: cannot directly access magic \".phar\" directory or files within it"
                .to_string(),
        );
    }
    if path.is_empty() && dir == 0 {
        return fail(format!(
            "phar error: invalid path \"{}\" must not be empty",
            l(path)
        ));
    }
    let chk = path_check(path);
    if chk.result.is_err() {
        return fail(format!(
            "phar error: invalid path \"{}\" contains {}",
            l(path),
            chk.error
        ));
    }
    let mut path = chk.path(path).to_vec();
    if is_dir {
        if path.len() <= 1 {
            return Err(None);
        }
        path.pop();
    }
    let found = phar.borrow().find(&path);
    if let Some(entry) = found {
        let e = entry.borrow();
        if e.is_deleted {
            return Err(None);
        }
        if e.is_dir && dir == 0 {
            return fail(format!("phar error: path \"{}\" is a directory", l(&path)));
        }
        if !e.is_dir && dir == 2 {
            return fail(format!(
                "phar error: path \"{}\" exists and is a not a directory",
                l(&path)
            ));
        }
        drop(e);
        return Ok(entry);
    }
    if dir != 0 && phar.borrow().virtual_dirs.contains(&path) {
        let entry = Entry {
            is_temp_dir: true,
            is_dir: true,
            filename: path.clone(),
            phar: Rc::downgrade(phar),
            ..Default::default()
        };
        return Ok(entry.into_ref());
    }
    let mounted: Vec<Vec<u8>> = phar.borrow().mounted_dirs.iter().cloned().collect();
    for key in mounted {
        if key.len() >= path.len() || !path.starts_with(&key) {
            continue;
        }
        let Some(entry) = phar.borrow().find(&key) else {
            return fail(format!(
                "phar internal error: mounted path \"{}\" could not be retrieved from manifest",
                l(&key)
            ));
        };
        let (tmp, is_mounted) = {
            let e = entry.borrow();
            (e.tmp.clone(), e.is_mounted)
        };
        let Some(tmp) = tmp.filter(|_| is_mounted) else {
            return fail(format!(
                "phar internal error: mounted path \"{}\" is not properly initialized as a mounted path",
                l(&key)
            ));
        };
        let mut test = tmp.clone();
        test.extend_from_slice(&path[key.len()..]);
        let Some(test_is_dir) = stat_is_dir(vm, &test) else {
            return Err(None);
        };
        if test_is_dir && dir == 0 {
            return fail(format!("phar error: path \"{}\" is a directory", l(&path)));
        }
        if !test_is_dir && dir != 0 {
            return fail(format!(
                "phar error: path \"{}\" exists and is a not a directory",
                l(&path)
            ));
        }
        if mount_entry(vm, phar, &test, &path).is_err() {
            return fail(format!(
                "phar error: path \"{}\" exists as file \"{}\" and could not be mounted",
                l(&path),
                l(&test)
            ));
        }
        let Some(e) = phar.borrow().find(&path) else {
            return fail(format!(
                "phar error: path \"{}\" exists as file \"{}\" and could not be retrieved after being mounted",
                l(&path),
                l(&test)
            ));
        };
        return Ok(e);
    }
    Err(None)
}

/// phar_mount_entry(): map `filename` (outside the phar) to `path`.
#[allow(clippy::result_unit_err)]
pub fn mount_entry(vm: &mut Vm, phar: &ArcRef, filename: &[u8], path: &[u8]) -> Result<(), ()> {
    let chk = path_check(path);
    if chk.result > Pcr::IsOk {
        return Err(());
    }
    let path = chk.path(path).to_vec();
    if path.starts_with(b".phar") {
        return Err(());
    }
    let is_phar = filename.len() > 7 && filename.starts_with(b"phar://");
    let tmp = if is_phar {
        filename.to_vec()
    } else {
        expand_filepath(filename).unwrap_or_else(|| filename.to_vec())
    };
    let sb = if has_scheme(&tmp) {
        goro_ext_standard::native_streams::stat_path(vm, &tmp, PHP_STREAM_URL_STAT_QUIET)
            .ok()
            .flatten()
            .map(|s| (s.mode, s.size))
    } else {
        use std::os::unix::fs::MetadataExt;
        std::fs::metadata(l(&tmp))
            .ok()
            .map(|m| (m.mode() as i64, m.size() as i64))
    };
    let Some((mode, size)) = sb else {
        return Err(());
    };
    let mut entry = Entry {
        phar: Rc::downgrade(phar),
        tmp: Some(tmp),
        is_mounted: true,
        is_crc_checked: true,
        fp_type: FpType::Tmp,
        filename: path.clone(),
        ..Default::default()
    };
    if mode & 0o170000 == 0o040000 {
        entry.is_dir = true;
        let mut p = phar.borrow_mut();
        if p.mounted_dirs.contains(&path) {
            return Err(());
        }
        p.mounted_dirs.insert(path.clone());
    } else {
        entry.uncompressed_filesize = size as u32;
        entry.compressed_filesize = size as u32;
    }
    entry.flags = mode as u32;
    let mut p = phar.borrow_mut();
    if p.manifest.contains_key(&path) {
        return Err(());
    }
    p.manifest.insert(path, entry.into_ref());
    Ok(())
}

/// phar_add_virtual_dirs()
pub fn add_virtual_dirs(phar: &mut Archive, filename: &[u8]) {
    let mut len = filename.len();
    while let Some(s) = filename[..len].iter().rposition(|&c| c == b'/') {
        len = s;
        if len == 0 {
            break;
        }
        if !phar.virtual_dirs.insert(filename[..len].to_vec()) {
            break;
        }
    }
}

/// phar_hex_str()
pub fn hex_str(digest: &[u8]) -> Vec<u8> {
    const HEX: &[u8] = b"0123456789ABCDEF";
    let mut out = Vec::with_capacity(digest.len() * 2);
    for &b in digest {
        out.push(HEX[(b >> 4) as usize]);
        out.push(HEX[(b & 0xF) as usize]);
    }
    out
}

fn hash(vm: &mut Vm, algo: &str, data: &[u8]) -> Vec<u8> {
    match call_builtin(
        vm,
        b"hash",
        &[
            Value::String(PhpString::from_string(algo.to_string())),
            Value::String(PhpString::from_bytes(data)),
            Value::True,
        ],
    ) {
        Some(Value::String(s)) => s.as_bytes().to_vec(),
        _ => Vec::new(),
    }
}

/// phar_call_openssl_signverify(): openssl_sign()/openssl_verify().
fn openssl_signverify(
    vm: &mut Vm,
    is_sign: bool,
    data: &[u8],
    key: &[u8],
    signature: &[u8],
    sig_type: u32,
) -> Result<Vec<u8>, ()> {
    let fname: &[u8] = if is_sign {
        b"openssl_sign"
    } else {
        b"openssl_verify"
    };
    let Some(f) = vm.functions.get(fname).copied() else {
        return Err(());
    };
    let algo = match sig_type {
        PHAR_SIG_OPENSSL_SHA512 => 9,
        PHAR_SIG_OPENSSL_SHA256 => 7,
        _ => 1,
    };
    let sig_ref = Rc::new(RefCell::new(Value::String(PhpString::from_bytes(
        signature,
    ))));
    let sig_arg = if is_sign {
        Value::Reference(sig_ref.clone())
    } else {
        Value::String(PhpString::from_bytes(signature))
    };
    let args = [
        Value::String(PhpString::from_bytes(data)),
        sig_arg,
        Value::String(PhpString::from_bytes(key)),
        Value::Long(algo),
    ];
    let r = f(vm, &args).map_err(|_| ())?;
    match r {
        Value::Long(1) => Ok(Vec::new()),
        Value::Long(_) => Err(()),
        Value::True => Ok(sig_ref.borrow().to_php_string().as_bytes().to_vec()),
        _ => Err(()),
    }
}

/// Whether PEM_read_bio_PrivateKey() reads `key` (a PEM private key, not
/// a file:// path; no passphrase), without the warnings openssl_*() give.
fn private_key_reads(vm: &mut Vm, key: &[u8]) -> bool {
    if key.starts_with(b"file://") {
        return false;
    }
    let Some(f) = vm.functions.get(b"openssl_pkey_get_private".as_slice()).copied() else {
        return false;
    };
    let saved = vm.warning_capture.replace(Vec::new());
    let r = f(vm, &[Value::String(PhpString::from_bytes(key))]);
    vm.warning_capture = saved;
    matches!(r, Ok(Value::Object(_)))
}

/// phar_verify_signature(): the hex signature, or the error.
pub fn verify_signature(
    vm: &mut Vm,
    fp: &[u8],
    end_of_phar: usize,
    sig_type: u32,
    sig: &[u8],
    fname: &[u8],
) -> Res<Vec<u8>> {
    let data = &fp[..end_of_phar.min(fp.len())];
    let check = |vm: &mut Vm, algo: &str, n: usize| -> Res<Vec<u8>> {
        if sig.len() < n {
            return fail("broken signature".to_string());
        }
        let digest = hash(vm, algo, data);
        if digest.len() != n || digest != sig[..n] {
            return fail("broken signature".to_string());
        }
        Ok(hex_str(&digest))
    };
    match sig_type {
        PHAR_SIG_OPENSSL_SHA512 | PHAR_SIG_OPENSSL_SHA256 | PHAR_SIG_OPENSSL => {
            if !vm.loaded_extensions.contains(b"openssl".as_slice()) {
                return fail("openssl not loaded".to_string());
            }
            let mut pfile = fname.to_vec();
            pfile.extend_from_slice(b".pubkey");
            let pubkey = read_file(&pfile).filter(|k| !k.is_empty());
            let Some(pubkey) = pubkey else {
                return fail("openssl public key could not be read".to_string());
            };
            if openssl_signverify(vm, false, data, &pubkey, sig, sig_type).is_err() {
                return fail("openssl signature could not be verified".to_string());
            }
            Ok(hex_str(sig))
        }
        PHAR_SIG_SHA512 => check(vm, "sha512", 64),
        PHAR_SIG_SHA256 => check(vm, "sha256", 32),
        PHAR_SIG_SHA1 => check(vm, "sha1", 20),
        PHAR_SIG_MD5 => check(vm, "md5", 16),
        _ => fail("broken or unsupported signature".to_string()),
    }
}

/// phar_create_signature(): the binary signature of `data`; sets
/// phar->signature (hex).
pub fn create_signature(vm: &mut Vm, phar: &ArcRef, data: &[u8]) -> Res<Vec<u8>> {
    phar.borrow_mut().signature = None;
    let sig_flags = phar.borrow().sig_flags;
    let sig = match sig_flags {
        PHAR_SIG_SHA512 => hash(vm, "sha512", data),
        PHAR_SIG_OPENSSL_SHA512 | PHAR_SIG_OPENSSL_SHA256 | PHAR_SIG_OPENSSL => {
            // (php-src links libssl itself when ext/openssl is there: no
            // key at all fails making the BIO, a key
            // PEM_read_bio_PrivateKey() cannot read fails quietly, before
            // signing; without it, openssl_sign() is called)
            let key = g(|g| g.openssl_privatekey.clone());
            if vm.loaded_extensions.contains(b"openssl".as_slice()) {
                let Some(k) = &key else {
                    return fail(format!(
                        "unable to write to phar \"{}\" with requested openssl signature",
                        l(&phar.borrow().fname)
                    ));
                };
                if !private_key_reads(vm, k) {
                    return fail("unable to process private key".to_string());
                }
            }
            let key = key.unwrap_or_default();
            match openssl_signverify(vm, true, data, &key, b"", sig_flags) {
                Ok(s) => s,
                Err(_) => {
                    return fail(format!(
                        "unable to write phar \"{}\" with requested openssl signature",
                        l(&phar.borrow().fname)
                    ));
                }
            }
        }
        PHAR_SIG_SHA1 => hash(vm, "sha1", data),
        PHAR_SIG_MD5 => hash(vm, "md5", data),
        PHAR_SIG_SHA256 => hash(vm, "sha256", data),
        _ => {
            phar.borrow_mut().sig_flags = PHAR_SIG_SHA256;
            hash(vm, "sha256", data)
        }
    };
    phar.borrow_mut().signature = Some(hex_str(&sig));
    Ok(sig)
}

/// phar_find_in_include_path(): the phar:// URL of `filename` (relative)
/// inside the executing phar, if any.
pub fn find_in_include_path(vm: &mut Vm, filename: &[u8]) -> Option<(Vec<u8>, Option<ArcRef>)> {
    let cwd = g(|g| g.cwd.clone());
    let cwd = cwd?;
    let fname = executed_filename(vm)?;
    let is_phar_url = starts_with_ci(&fname, b"phar://");
    let (last, last_name) = g(|g| (g.last_phar.clone(), g.last_phar_name.clone()));
    let mut arch: Vec<u8>;
    let mut phar: Option<ArcRef> = None;
    let mut splitted = false;
    if let Some(lp) = last {
        if is_phar_url && fname.len() - 7 >= last_name.len() && fname[7..].starts_with(&last_name) {
            arch = last_name.clone();
            phar = Some(lp);
            splitted = true;
        } else {
            arch = Vec::new();
        }
    } else {
        arch = Vec::new();
    }
    if !splitted {
        if !is_phar_url {
            return None;
        }
        let (a, _) = split_fname(vm, &fname, 1, 0).ok()?;
        arch = a;
    }
    if splitted || filename.first() == Some(&b'.') {
        if !splitted {
            phar = Some(get_archive(&arch, None).ok()?);
        }
        let p = phar.clone().unwrap();
        let test = fix_filepath(filename.to_vec(), true);
        if test.first() == Some(&b'/') {
            if p.borrow().manifest.contains_key(&test[1..]) {
                let mut r = b"phar://".to_vec();
                r.extend_from_slice(&arch);
                r.extend_from_slice(&test);
                return Some((r, Some(p)));
            }
        } else if p.borrow().manifest.contains_key(&test) {
            let mut r = b"phar://".to_vec();
            r.extend_from_slice(&arch);
            r.push(b'/');
            r.extend_from_slice(&test);
            return Some((r, Some(p)));
        }
    }
    // php_resolve_path(filename, "phar://<arch>/<cwd>:<include_path>")
    let include_path = vm
        .constants
        .get(b"include_path".as_slice())
        .map(|v| v.to_php_string().as_bytes().to_vec())
        .unwrap_or_else(|| b".".to_vec());
    let mut path = b"phar://".to_vec();
    path.extend_from_slice(&arch);
    path.push(b'/');
    path.extend_from_slice(&cwd);
    path.push(b':');
    path.extend_from_slice(&include_path);
    let ret = resolve_path(vm, filename, &path)?;
    let mut pphar = None;
    if starts_with_ci(&ret, b"phar://") {
        if let Ok((a, _)) = split_fname(vm, &ret, 1, 0) {
            pphar = find_fname(&a);
        }
    }
    Some((ret, pphar))
}

/// php_resolve_path() over an explicit path list (stream wrapper entries
/// are checked with url_stat).
pub fn resolve_path(vm: &mut Vm, filename: &[u8], path: &[u8]) -> Option<Vec<u8>> {
    if filename.is_empty() {
        return None;
    }
    // absolute or explicitly relative paths, and URLs, are not searched
    if has_scheme(filename) {
        return Some(filename.to_vec());
    }
    if filename[0] == b'/' || filename.starts_with(b"./") || filename.starts_with(b"../") {
        let rp = expand_filepath(filename)?;
        return std::path::Path::new(&l(&rp)).exists().then_some(rp);
    }
    let mut ptr = 0usize;
    while ptr < path.len() {
        let rest = &path[ptr..];
        let mut p = 0;
        while p < rest.len()
            && (rest[p].is_ascii_alphanumeric() || matches!(rest[p], b'+' | b'-' | b'.'))
        {
            p += 1;
        }
        let mut search_from = 0;
        let mut is_wrapper = false;
        if p < rest.len() && rest[p] == b':' && p > 1 && rest[p + 1..].starts_with(b"//") {
            is_wrapper = true;
            search_from = p + 3;
        }
        let end = rest[search_from.min(rest.len())..]
            .iter()
            .position(|&c| c == b':')
            .map(|e| e + search_from);
        let entry = match end {
            Some(e) => &rest[..e],
            None => rest,
        };
        let mut trypath = entry.to_vec();
        trypath.push(b'/');
        trypath.extend_from_slice(filename);
        ptr = match end {
            Some(e) => ptr + e + 1,
            None => path.len(),
        };
        if is_wrapper {
            if goro_ext_standard::native_streams::stat_path(vm, &trypath, PHP_STREAM_URL_STAT_QUIET)
                .ok()
                .flatten()
                .is_some()
            {
                return Some(trypath);
            }
            continue;
        }
        if let Some(rp) = expand_filepath(&trypath) {
            if std::path::Path::new(&l(&rp)).exists() {
                return Some(rp);
            }
        }
    }
    None
}
