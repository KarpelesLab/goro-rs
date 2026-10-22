//! php-src ext/phar/tar.c: tar-based phar archives.

use std::cell::RefCell;
use std::rc::Rc;

use goro_core::vm::Vm;

use crate::archive::*;
use crate::util;

// tar_header field offsets
const NAME: usize = 0;
const MODE: usize = 100;
const SIZE: usize = 124;
const MTIME: usize = 136;
const CHECKSUM: usize = 148;
const TYPEFLAG: usize = 156;
const LINKNAME: usize = 157;
const MAGIC: usize = 257;
const VERSION: usize = 263;
const PREFIX: usize = 345;
const OLD_HEADER_LEN: usize = 257;
const HEADER_LEN: usize = 512;

/// phar_tar_number()
fn tar_number(buf: &[u8]) -> u32 {
    let mut i = 0;
    let mut num: u32 = 0;
    while i < buf.len() && buf[i] == b' ' {
        i += 1;
    }
    while i < buf.len() && (b'0'..=b'7').contains(&buf[i]) {
        num = num.wrapping_mul(8).wrapping_add((buf[i] - b'0') as u32);
        i += 1;
    }
    num
}

/// phar_tar_octal(): Err when the value does not fit.
fn tar_octal(buf: &mut [u8], mut val: u32) -> Result<(), ()> {
    let len = buf.len();
    for i in (0..len).rev() {
        buf[i] = b'0' + (val & 7) as u8;
        val >>= 3;
    }
    if val == 0 {
        return Ok(());
    }
    for b in buf.iter_mut() {
        *b = b'7';
    }
    Err(())
}

/// phar_tar_checksum()
fn tar_checksum(buf: &[u8]) -> u32 {
    buf.iter().map(|&b| b as u32).sum()
}

fn strnlen(b: &[u8]) -> usize {
    b.iter().position(|&c| c == 0).unwrap_or(b.len())
}

/// phar_is_tar()
pub fn is_tar(buf: &[u8], fname: &[u8]) -> bool {
    if buf.starts_with(b"<?php") {
        return false;
    }
    let checksum = tar_number(&buf[CHECKSUM..CHECKSUM + 8]);
    let mut h = buf[..512].to_vec();
    h[CHECKSUM..CHECKSUM + 8].fill(b' ');
    let is_tar = checksum == tar_checksum(&h);
    let bname = match fname.iter().rposition(|&c| c == b'/') {
        Some(i) => &fname[i..],
        None => fname,
    };
    if !is_tar {
        if let Some(p) = memmem(bname, b".tar") {
            let c = bname.get(p + 4).copied().unwrap_or(0);
            if c == 0 || c == b'.' {
                // probably a corrupted tar - so we will pretend it is one
                return true;
            }
        }
    }
    is_tar
}

/// phar_open_or_create_tar()
pub fn open_or_create_tar(
    vm: &mut Vm,
    fname: &[u8],
    alias: Option<&[u8]>,
    is_data: bool,
    options: i64,
) -> Res<ArcRef> {
    let phar = create_or_parse_filename(vm, fname, alias, is_data, options)?;
    let mut p = phar.borrow_mut();
    p.is_data = is_data;
    if p.is_tar {
        drop(p);
        return Ok(phar);
    }
    if p.is_brandnew {
        p.is_tar = true;
        p.is_zip = false;
        drop(p);
        return Ok(phar);
    }
    fail(format!(
        "phar tar error: \"{}\" already exists as a regular phar and must be deleted from disk prior to creating as a tar-based phar",
        l(fname)
    ))
}

/// phar_tar_process_metadata()
fn process_metadata(phar: &ArcRef, entry: &EntryRef, data: &[u8]) -> Result<(), ()> {
    let size = entry.borrow().uncompressed_filesize as usize;
    if data.len() < size {
        return Err(());
    }
    entry.borrow_mut().metadata.parse_lazy(&data[..size]);
    let name = entry.borrow().filename.clone();
    if name == b".phar/.metadata.bin" {
        if phar.borrow().metadata.has_data() {
            return Err(());
        }
        let m = std::mem::take(&mut entry.borrow_mut().metadata);
        phar.borrow_mut().metadata = m;
    } else {
        const PRE: &[u8] = b".phar/.metadata/";
        const POST: &[u8] = b"/.metadata.bin";
        if name.len() >= PRE.len() + 1 + POST.len() {
            let target = &name[PRE.len()..name.len() - POST.len()];
            let mentry = phar.borrow().find(target);
            if let Some(mentry) = mentry {
                if mentry.borrow().metadata.has_data() {
                    return Err(());
                }
                let m = std::mem::take(&mut entry.borrow_mut().metadata);
                mentry.borrow_mut().metadata = m;
            }
        }
    }
    Ok(())
}

/// phar_parse_tarfile()
pub fn parse_tarfile(
    vm: &mut Vm,
    fp: Vec<u8>,
    fname: &[u8],
    alias: Option<&[u8]>,
    compression: u32,
) -> Res<ArcRef> {
    let f = l(fname);
    let totalsize = fp.len();
    let truncated = || {
        fail(format!(
            "phar error: \"{}\" is a corrupted tar file (truncated)",
            f
        ))
    };
    if totalsize < 512 {
        return fail(format!(
            "phar error: \"{}\" is not a tar file or is truncated",
            f
        ));
    }
    let mut cur = 0usize; // stream position
    let mut buf = fp[0..512].to_vec();
    cur += 512;
    let mut old = &buf[MAGIC..MAGIC + 5] != b"ustar";
    let myphar = Rc::new(RefCell::new(Archive {
        is_tar: true,
        flags: compression,
        ..Default::default()
    }));
    let mut actual_alias: Option<Vec<u8>> = None;
    let mut last_was_longlink = false;
    let mut longlink_name: Vec<u8> = Vec::new();
    loop {
        let pos = cur;
        let sum1 = tar_number(&buf[CHECKSUM..CHECKSUM + 8]);
        if sum1 == 0 && tar_checksum(&buf) == 0 {
            break;
        }
        buf[CHECKSUM..CHECKSUM + 8].fill(b' ');
        let mut sum2 = tar_checksum(&buf[..if old { OLD_HEADER_LEN } else { HEADER_LEN }]);
        if old && sum2 != sum1 {
            let sum3 = tar_checksum(&buf);
            if sum3 == sum1 {
                sum2 = sum3;
                old = false;
            }
        }
        let fsize = tar_number(&buf[SIZE..SIZE + 12]);
        let mut size = fsize as usize;
        let typeflag = buf[TYPEFLAG];
        let mut skip_to_next = false;
        if !old && (typeflag == TAR_GLOBAL_HDR || typeflag == TAR_FILE_HDR) {
            size = (size + 511) & !511;
            skip_to_next = true;
        }
        if !skip_to_next {
            let name_len = strnlen(&buf[NAME..NAME + 100]);
            if ((!old && buf[PREFIX] == 0) || old)
                && name_len == b".phar/signature.bin".len()
                && &buf[NAME..NAME + name_len] == b".phar/signature.bin"
            {
                if size > 511 {
                    return fail(format!(
                        "phar error: tar-based phar \"{}\" has signature that is larger than 511 bytes, cannot process",
                        f
                    ));
                }
                let curloc = cur;
                let read = size.min(totalsize.saturating_sub(cur));
                if read != size || read <= 8 {
                    return fail(format!(
                        "phar error: tar-based phar \"{}\" signature cannot be read",
                        f
                    ));
                }
                let sigbuf = fp[cur..cur + size].to_vec();
                cur += size;
                let sig_flags = u32::from_le_bytes([sigbuf[0], sigbuf[1], sigbuf[2], sigbuf[3]]);
                myphar.borrow_mut().sig_flags = sig_flags;
                let end_of_phar = cur - size - 512;
                match util::verify_signature(vm, &fp, end_of_phar, sig_flags, &sigbuf[8..], fname) {
                    Ok(s) => myphar.borrow_mut().signature = Some(s),
                    Err(e) => {
                        return fail(format!(
                            "phar error: tar-based phar \"{}\" signature cannot be verified: {}",
                            f,
                            e.unwrap_or_default()
                        ));
                    }
                }
                cur = curloc + 512;
                if (typeflag == 0 || typeflag == TAR_FILE) && size > 0 {
                    cur += 512;
                    if cur > totalsize {
                        return truncated();
                    }
                }
                if cur + 512 > totalsize {
                    return truncated();
                }
                buf = fp[cur..cur + 512].to_vec();
                let s1 = tar_number(&buf[CHECKSUM..CHECKSUM + 8]);
                if s1 == 0 && tar_checksum(&buf) == 0 {
                    break;
                }
                return fail(format!(
                    "phar error: \"{}\" has entries after signature, invalid phar",
                    f
                ));
            }
            if !last_was_longlink && typeflag == b'L' {
                last_was_longlink = true;
                if fsize == u32::MAX || fsize == 0 {
                    return fail(format!(
                        "phar error: \"{}\" is a corrupted tar file (invalid entry size)",
                        f
                    ));
                }
                let n = fsize as usize;
                if cur + n > totalsize {
                    return truncated();
                }
                longlink_name = fp[cur..cur + n].to_vec();
                // the name is a C string
                longlink_name.truncate(strnlen(&longlink_name));
                cur += n;
                let pad = ((n + 511) & !511) - n;
                cur += pad;
                if cur > totalsize {
                    return truncated();
                }
                if cur + 512 > totalsize {
                    return truncated();
                }
                buf = fp[cur..cur + 512].to_vec();
                cur += 512;
                continue;
            }
            let filename: Vec<u8> = if !last_was_longlink && !old && buf[PREFIX] != 0 {
                let mut name = Vec::with_capacity(256);
                let pl = strnlen(&buf[PREFIX..PREFIX + 155]);
                name.extend_from_slice(&buf[PREFIX..PREFIX + pl]);
                name.push(b'/');
                let nl = strnlen(&buf[NAME..NAME + 100]);
                name.extend_from_slice(&buf[NAME..NAME + nl]);
                if name.last() == Some(&b'/') {
                    name.pop();
                }
                name
            } else if !last_was_longlink {
                let mut nl = strnlen(&buf[NAME..NAME + 100]);
                if nl > 0 && buf[NAME + nl - 1] == b'/' {
                    nl -= 1;
                }
                buf[NAME..NAME + nl].to_vec()
            } else {
                std::mem::take(&mut longlink_name)
            };
            last_was_longlink = false;
            util::add_virtual_dirs(&mut myphar.borrow_mut(), &filename);
            if sum1 != sum2 {
                return fail(format!(
                    "phar error: \"{}\" is a corrupted tar file (checksum mismatch of file \"{}\")",
                    f,
                    l(&filename)
                ));
            }
            let entry_mode = tar_number(&buf[MODE..MODE + 8]);
            let mut entry = Entry {
                is_tar: true,
                is_crc_checked: true,
                phar: Rc::downgrade(&myphar),
                uncompressed_filesize: fsize,
                compressed_filesize: fsize,
                filename,
                ..Default::default()
            };
            entry.tar_type = if old && typeflag == 0 {
                TAR_FILE
            } else {
                typeflag
            };
            entry.offset = pos as i64;
            entry.offset_abs = pos as i64;
            entry.fp_type = FpType::Fp;
            entry.flags = entry_mode & PHAR_ENT_PERM_MASK;
            entry.timestamp = tar_number(&buf[MTIME..MTIME + 12]);
            if old && entry.tar_type == TAR_FILE && entry_mode & 0o170000 == 0o040000 {
                entry.tar_type = TAR_DIR;
            }
            entry.is_dir = entry.tar_type == TAR_DIR;
            let linkname_len = strnlen(&buf[LINKNAME..LINKNAME + 100]);
            let linkname = buf[LINKNAME..LINKNAME + linkname_len].to_vec();
            if entry.tar_type == TAR_LINK {
                if !myphar.borrow().manifest.contains_key(&linkname) {
                    return fail(format!(
                        "phar error: \"{}\" is a corrupted tar file - hard link to non-existent file \"{}\"",
                        f,
                        l(&linkname)
                    ));
                }
                entry.link = Some(linkname);
            } else if entry.tar_type == TAR_SYMLINK {
                entry.link = Some(linkname);
            }
            set_inode(&mut entry, fname);
            let name = entry.filename.clone();
            let newentry = entry.into_ref();
            myphar
                .borrow_mut()
                .manifest
                .insert(name.clone(), newentry.clone());
            if name.starts_with(b".phar/.metadata") {
                let data = &fp[cur.min(totalsize)..];
                if process_metadata(&myphar, &newentry, data).is_err() {
                    return fail(format!(
                        "phar error: tar-based phar \"{}\" has invalid metadata in magic file \"{}\"",
                        f,
                        l(&name)
                    ));
                }
            }
            if actual_alias.is_none() && name == b".phar/alias.txt" {
                if size > 511 {
                    return fail(format!(
                        "phar error: tar-based phar \"{}\" has alias that is larger than 511 bytes, cannot process",
                        f
                    ));
                }
                if cur + size <= totalsize {
                    let a = fp[cur..cur + size].to_vec();
                    if !validate_alias(&a) {
                        let mut shown = a.clone();
                        if size > 50 {
                            shown.truncate(50);
                            shown.extend_from_slice(b"...");
                        }
                        // (the alias is a C string)
                        shown.truncate(strnlen(&shown));
                        return fail(format!(
                            "phar error: invalid alias \"{}\" in tar-based phar \"{}\"",
                            l(&shown),
                            f
                        ));
                    }
                    myphar.borrow_mut().alias = Some(a.clone());
                    actual_alias = Some(a);
                } else {
                    return fail(format!(
                        "phar error: Unable to read alias from tar-based phar \"{}\"",
                        f
                    ));
                }
            }
            size = (size + 511) & !511;
        }
        if skip_to_next || ((typeflag == 0 || typeflag == TAR_FILE) && size > 0) {
            cur += size;
            if cur > totalsize {
                return truncated();
            }
        }
        if cur == totalsize {
            break;
        }
        if cur + 512 > totalsize {
            return truncated();
        }
        buf = fp[cur..cur + 512].to_vec();
        cur += 512;
    }
    let is_data = !myphar
        .borrow()
        .manifest
        .contains_key(b".phar/stub.php".as_slice());
    myphar.borrow_mut().is_data = is_data;
    if !is_data && require_hash() && myphar.borrow().signature.is_none() {
        return fail(format!(
            "tar-based phar \"{}\" does not have a signature",
            f
        ));
    }
    {
        let mut p = myphar.borrow_mut();
        p.fname = fname.to_vec();
        p.fp = Some(Rc::new(RefCell::new(fp)));
        p.set_ext_from_fname();
    }
    request_initialize();
    let added = g(|g| {
        if g.fname_map.contains_key(fname) {
            false
        } else {
            g.fname_map.insert(fname.to_vec(), myphar.clone());
            true
        }
    });
    if !added {
        return fail(format!(
            "phar error: Unable to add tar-based phar \"{}\" to phar registry",
            f
        ));
    }
    if let Some(a) = actual_alias {
        myphar.borrow_mut().is_temporary_alias = false;
        if let Some(fd) = find_alias(&a) {
            if util::free_alias(&fd).is_err() {
                fname_map_del(fname);
                return fail(format!(
                    "phar error: Unable to add tar-based phar \"{}\", alias is already in use",
                    f
                ));
            }
        }
        g(|g| {
            if !g.alias_map.contains_key(&a) {
                g.alias_map.insert(a, myphar.clone());
            }
        });
    } else {
        match alias.filter(|a| !a.is_empty()) {
            Some(a) => {
                if let Some(fd) = find_alias(a) {
                    if util::free_alias(&fd).is_err() {
                        fname_map_del(fname);
                        return fail(format!(
                            "phar error: Unable to add tar-based phar \"{}\", alias is already in use",
                            f
                        ));
                    }
                }
                g(|g| {
                    if !g.alias_map.contains_key(a) {
                        g.alias_map.insert(a.to_vec(), myphar.clone());
                    }
                });
                myphar.borrow_mut().alias = Some(a.to_vec());
            }
            None => {
                myphar.borrow_mut().alias = Some(fname.to_vec());
            }
        }
        myphar.borrow_mut().is_temporary_alias = true;
    }
    Ok(myphar)
}

struct Pass {
    new: Vec<u8>,
    error: PErr,
}

/// phar_tar_writeheaders_int(): false to stop.
fn writeheaders_int(vm: &mut Vm, phar: &ArcRef, entry: &EntryRef, pass: &mut Pass) -> bool {
    let pf = l(&phar.borrow().fname);
    if entry.borrow().is_mounted {
        return true;
    }
    if entry.borrow().is_deleted {
        let (refc, locks, name) = {
            let e = entry.borrow();
            (e.fp_refcount, e.fileinfo_lock_count, e.filename.clone())
        };
        if refc <= 0 && locks == 0 {
            phar.borrow_mut().manifest.shift_remove(&name);
        }
        return true;
    }
    let name = entry.borrow().filename.clone();
    util::add_virtual_dirs(&mut phar.borrow_mut(), &name);
    let mut header = [0u8; 512];
    let flen = name.len();
    if flen > 100 {
        if flen > 256 {
            pass.error = Some(format!(
                "tar-based phar \"{}\" cannot be created, filename \"{}\" is too long for tar file format",
                pf,
                l(&name)
            ));
            return false;
        }
        let mut boundary = flen - 101;
        while boundary < flen && name[boundary] != b'/' {
            boundary += 1;
        }
        if boundary >= flen || boundary > 155 {
            pass.error = Some(format!(
                "tar-based phar \"{}\" cannot be created, filename \"{}\" is too long for tar file format",
                pf,
                l(&name)
            ));
            return false;
        }
        header[PREFIX..PREFIX + boundary].copy_from_slice(&name[..boundary]);
        let rest = &name[boundary + 1..];
        header[NAME..NAME + rest.len()].copy_from_slice(rest);
    } else {
        header[NAME..NAME + flen].copy_from_slice(&name);
    }
    let (flags, usize_, ts, tar_type, link) = {
        let e = entry.borrow();
        (
            e.flags,
            e.uncompressed_filesize,
            e.timestamp,
            e.tar_type,
            e.link.clone(),
        )
    };
    let _ = tar_octal(&mut header[MODE..MODE + 7], flags & PHAR_ENT_PERM_MASK);
    if tar_octal(&mut header[SIZE..SIZE + 11], usize_).is_err() {
        pass.error = Some(format!(
            "tar-based phar \"{}\" cannot be created, filename \"{}\" is too large for tar file format",
            pf,
            l(&name)
        ));
        return false;
    }
    if tar_octal(&mut header[MTIME..MTIME + 11], ts).is_err() {
        pass.error = Some(format!(
            "tar-based phar \"{}\" cannot be created, file modification time of file \"{}\" is too large for tar file format",
            pf,
            l(&name)
        ));
        return false;
    }
    header[TYPEFLAG] = tar_type;
    if let Some(link) = &link {
        let ll = strnlen(link);
        if ll >= 100 {
            pass.error = Some(format!(
                "tar-based phar \"{}\" cannot be created, link \"{}\" is too long for format",
                pf,
                l(link)
            ));
            return false;
        }
        header[LINKNAME..LINKNAME + ll].copy_from_slice(&link[..ll]);
    }
    header[MAGIC..MAGIC + 5].copy_from_slice(b"ustar");
    header[VERSION..VERSION + 2].copy_from_slice(b"00");
    header[CHECKSUM..CHECKSUM + 8].copy_from_slice(b"        ");
    let crc = tar_checksum(&header);
    entry.borrow_mut().crc32 = crc;
    if tar_octal(&mut header[CHECKSUM..CHECKSUM + 7], crc).is_err() {
        pass.error = Some(format!(
            "tar-based phar \"{}\" cannot be created, checksum of file \"{}\" is too large for tar file format",
            pf,
            l(&name)
        ));
        return false;
    }
    entry.borrow_mut().header_offset = pass.new.len() as i64;
    pass.new.extend_from_slice(&header);
    let pos = pass.new.len();
    if usize_ != 0 {
        if let Err(e) = util::open_entry_fp(vm, entry, false) {
            pass.error = e;
            if pass.error.is_none() {
                pass.error = Some(String::new());
            }
            return false;
        }
        let Some(data) = util::read_entry_contents(vm, entry, false) else {
            pass.error = Some(format!(
                "tar-based phar \"{}\" cannot be created, contents of file \"{}\" could not be written, seek failed",
                pf,
                l(&name)
            ));
            return false;
        };
        if data.len() != usize_ as usize {
            pass.error = Some(format!(
                "tar-based phar \"{}\" cannot be created, contents of file \"{}\" could not be written",
                pf,
                l(&name)
            ));
            return false;
        }
        pass.new.extend_from_slice(&data);
        let pad = ((usize_ as usize + 511) & !511) - usize_ as usize;
        pass.new.extend(std::iter::repeat_n(0u8, pad));
    }
    let mut e = entry.borrow_mut();
    e.is_modified = false;
    if e.fp_type == FpType::Mod {
        e.fp = None;
    }
    e.fp_type = FpType::Fp;
    e.offset = pos as i64;
    e.offset_abs = pos as i64;
    true
}

/// phar_tar_setmetadata()
fn setmetadata(vm: &mut Vm, tracker: &Meta, entry: &EntryRef) {
    let mut m = tracker.clone();
    if m.str.is_none() {
        if let Some(v) = &m.val {
            m.str = goro_ext_standard::serialize::php_serialize(vm, v);
        }
    }
    let ser = m.str.clone();
    let mut e = entry.borrow_mut();
    e.metadata = m;
    let n = ser.as_ref().map(|s| s.len()).unwrap_or(0) as u32;
    e.uncompressed_filesize = n;
    e.compressed_filesize = n;
    e.fp_type = FpType::Mod;
    e.is_modified = true;
    e.fp = Some(Rc::new(RefCell::new(ser.unwrap_or_default())));
    e.offset = 0;
    e.offset_abs = 0;
}

fn magic_entry(phar: &ArcRef, name: &[u8]) -> EntryRef {
    Entry {
        filename: name.to_vec(),
        phar: Rc::downgrade(phar),
        tar_type: TAR_FILE,
        is_tar: true,
        ..Default::default()
    }
    .into_ref()
}

/// phar_tar_setupmetadata(): false to stop.
fn setupmetadata(vm: &mut Vm, phar: &ArcRef, entry: &EntryRef) {
    let name = entry.borrow().filename.clone();
    if name.starts_with(b".phar/.metadata") {
        if name == b".phar/.metadata.bin" {
            let m = phar.borrow().metadata.clone();
            setmetadata(vm, &m, entry);
            return;
        }
        const PRE: &[u8] = b".phar/.metadata/";
        const POST: &[u8] = b"/.metadata.bin";
        if name.len() >= PRE.len() + 1 + POST.len()
            && !phar
                .borrow()
                .manifest
                .contains_key(&name[PRE.len()..name.len() - POST.len()])
        {
            // orphaned metadata
            phar.borrow_mut().manifest.shift_remove(&name);
        }
        return;
    }
    if !entry.borrow().is_modified {
        return;
    }
    let mut lookfor = b".phar/.metadata/".to_vec();
    lookfor.extend_from_slice(&name);
    lookfor.extend_from_slice(b"/.metadata.bin");
    if !entry.borrow().metadata.has_data() {
        phar.borrow_mut().manifest.shift_remove(&lookfor);
        return;
    }
    let m = entry.borrow().metadata.clone();
    let existing = phar.borrow().find(&lookfor);
    if let Some(meta) = existing {
        setmetadata(vm, &m, &meta);
        return;
    }
    let meta = magic_entry(phar, &lookfor);
    phar.borrow_mut().manifest.insert(lookfor, meta.clone());
    setmetadata(vm, &m, &meta);
}

/// phar_tar_flush()
pub fn tar_flush(
    vm: &mut Vm,
    phar: &ArcRef,
    user_stub: Option<&[u8]>,
    is_default_stub: bool,
) -> PErr {
    const NEWSTUB: &[u8] = b"<?php // tar-based phar archive stub file\n__HALT_COMPILER();";
    let fname = phar.borrow().fname.clone();
    let f = l(&fname);
    let mk = |data: Vec<u8>, name: &[u8]| -> EntryRef {
        Entry {
            flags: PHAR_ENT_PERM_DEF_FILE,
            timestamp: now(),
            is_modified: true,
            is_crc_checked: true,
            is_tar: true,
            tar_type: b'0',
            phar: Rc::downgrade(phar),
            fp_type: FpType::Mod,
            uncompressed_filesize: data.len() as u32,
            compressed_filesize: 0,
            fp: Some(Rc::new(RefCell::new(data))),
            filename: name.to_vec(),
            ..Default::default()
        }
        .into_ref()
    };
    if !phar.borrow().is_data {
        // alias
        let (temp, alias) = {
            let p = phar.borrow();
            (p.is_temporary_alias, p.alias.clone().unwrap_or_default())
        };
        if !temp && !alias.is_empty() {
            let e = mk(alias, b".phar/alias.txt");
            phar.borrow_mut()
                .manifest
                .insert(b".phar/alias.txt".to_vec(), e);
        } else {
            phar.borrow_mut()
                .manifest
                .shift_remove(b".phar/alias.txt".as_slice());
        }
        // stub
        if let Some(stub) = user_stub.filter(|_| !is_default_stub) {
            let Some(pos) = stristr(stub, b"__HALT_COMPILER();") else {
                return Some(format!("illegal stub for tar-based phar \"{}\"", f));
            };
            let len = pos + "__HALT_COMPILER();".len();
            let mut data = stub[..len].to_vec();
            data.extend_from_slice(b" ?>\r\n");
            let e = mk(data, b".phar/stub.php");
            phar.borrow_mut()
                .manifest
                .insert(b".phar/stub.php".to_vec(), e);
        } else {
            let e = mk(NEWSTUB.to_vec(), b".phar/stub.php");
            e.borrow_mut().compressed_filesize = NEWSTUB.len() as u32;
            if !is_default_stub {
                let mut p = phar.borrow_mut();
                if !p.manifest.contains_key(b".phar/stub.php".as_slice()) {
                    p.manifest.insert(b".phar/stub.php".to_vec(), e);
                }
            } else {
                phar.borrow_mut()
                    .manifest
                    .insert(b".phar/stub.php".to_vec(), e);
            }
        }
    }
    let mut pass = Pass {
        new: Vec::new(),
        error: None,
    };
    if phar.borrow().metadata.has_data() {
        let existing = phar.borrow().find(b".phar/.metadata.bin");
        let m = phar.borrow().metadata.clone();
        match existing {
            Some(me) => setmetadata(vm, &m, &me),
            None => {
                let me = magic_entry(phar, b".phar/.metadata.bin");
                phar.borrow_mut()
                    .manifest
                    .insert(b".phar/.metadata.bin".to_vec(), me.clone());
                setmetadata(vm, &m, &me);
            }
        }
    }
    let entries: Vec<EntryRef> = phar.borrow().manifest.values().cloned().collect();
    for e in &entries {
        setupmetadata(vm, phar, e);
    }
    let entries: Vec<EntryRef> = phar.borrow().manifest.values().cloned().collect();
    for e in &entries {
        if !writeheaders_int(vm, phar, e, &mut pass) {
            break;
        }
    }
    if let Some(e) = pass.error {
        return Some(e);
    }
    let (is_data, sig_flags) = {
        let p = phar.borrow();
        (p.is_data, p.sig_flags)
    };
    if !is_data || sig_flags != 0 {
        let signature = match util::create_signature(vm, phar, &pass.new) {
            Ok(s) => s,
            Err(e) => {
                return Some(format!(
                    "phar error: unable to write signature to tar-based phar: {}",
                    e.unwrap_or_default()
                ));
            }
        };
        let sig_flags = phar.borrow().sig_flags;
        let mut data = Vec::new();
        data.extend_from_slice(&sig_flags.to_le_bytes());
        data.extend_from_slice(&(signature.len() as u32).to_le_bytes());
        data.extend_from_slice(&signature);
        let len = data.len() as u32;
        let e = mk(data, b".phar/signature.bin");
        {
            let mut eb = e.borrow_mut();
            eb.uncompressed_filesize = len;
            eb.compressed_filesize = len;
        }
        writeheaders_int(vm, phar, &e, &mut pass);
        if let Some(err) = pass.error {
            return Some(err);
        }
    }
    pass.new.extend(std::iter::repeat_n(0u8, 1024));
    {
        let mut p = phar.borrow_mut();
        p.ufp = None;
        p.is_brandnew = false;
    }
    let newbuf = Rc::new(RefCell::new(pass.new));
    if !phar.borrow().donotflush {
        let flags = phar.borrow().flags;
        let out = if flags & PHAR_FILE_COMPRESSED_GZ != 0 {
            util::zlib_deflate(&newbuf.borrow(), 15 + 16)
        } else if flags & PHAR_FILE_COMPRESSED_BZ2 != 0 {
            util::bz2_compress(vm, &newbuf.borrow())
        } else {
            Some(newbuf.borrow().clone())
        };
        if std::fs::write(l(&fname), out.unwrap_or_default()).is_err() {
            phar.borrow_mut().fp = Some(newbuf);
            crate::open_failed_warning(vm, &fname);
            return Some(format!("unable to open new phar \"{}\" for writing", f));
        }
    }
    phar.borrow_mut().fp = Some(newbuf);
    None
}
