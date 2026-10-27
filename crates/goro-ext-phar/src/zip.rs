//! php-src ext/phar/zip.c: zip-based phar archives.

use std::cell::RefCell;
use std::rc::Rc;

use goro_core::vm::Vm;

use crate::archive::*;
use crate::util;

const PHAR_ZIP_COMP_NONE: u16 = 0;
const PHAR_ZIP_COMP_DEFLATE: u16 = 8;
const PHAR_ZIP_COMP_BZIP2: u16 = 12;
const PHAR_ZIP_FLAG_ENCRYPTED: u16 = 0x0001;

/// sizeof(phar_zip_file_header)
const LOCAL_LEN: usize = 30;
/// sizeof(phar_zip_central_dir_file)
const CENTRAL_LEN: usize = 46;
/// sizeof(phar_zip_dir_end)
const EOCD_LEN: usize = 22;
/// sizeof(phar_zip_unix3)
const UNIX3_LEN: usize = 18;

fn g16(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b[o], b[o + 1]])
}

fn g32(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

fn s16(b: &mut [u8], o: usize, v: u16) {
    b[o..o + 2].copy_from_slice(&v.to_le_bytes());
}

fn s32(b: &mut [u8], o: usize, v: u32) {
    b[o..o + 4].copy_from_slice(&v.to_le_bytes());
}

/// phar_zip_d2u_time()
fn d2u_time(dtime: u16, ddate: u16) -> u32 {
    unsafe {
        let now = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&now, &mut tm);
        tm.tm_year = ((ddate as i32 >> 9) & 127) + 1980 - 1900;
        tm.tm_mon = ((ddate as i32 >> 5) & 15) - 1;
        tm.tm_mday = ddate as i32 & 31;
        tm.tm_hour = (dtime as i32 >> 11) & 31;
        tm.tm_min = (dtime as i32 >> 5) & 63;
        tm.tm_sec = (dtime as i32) << 1 & 62;
        libc::mktime(&mut tm) as u32
    }
}

/// phar_zip_u2d_time(): (time, date)
fn u2d_time(time: u32) -> (u16, u16) {
    unsafe {
        let t = time as libc::time_t;
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&t, &mut tm);
        if tm.tm_year >= 80 {
            let cdate =
                (((tm.tm_year + 1900 - 1980) << 9) + ((tm.tm_mon + 1) << 5) + tm.tm_mday) as u16;
            let ctime = ((tm.tm_hour << 11) + (tm.tm_min << 5) + (tm.tm_sec >> 1)) as u16;
            (ctime, cdate)
        } else {
            (0, (1 << 5) + 1)
        }
    }
}

/// phar_zip_process_extra()
fn process_extra(buf: &[u8], mut pos: usize, entry: &mut Entry, len: u16) -> Result<(), ()> {
    let mut len = len as i32;
    loop {
        if pos + 4 > buf.len() {
            return Err(());
        }
        let tag = [buf[pos], buf[pos + 1]];
        let hsize = g16(buf, pos + 2);
        pos += 4;
        if tag == *b"UT" && hsize >= 5 {
            if pos + 5 > buf.len() {
                return Err(());
            }
            let flags = buf[pos];
            if flags & 1 != 0 {
                entry.timestamp = g32(buf, pos + 1);
            }
            len -= hsize as i32 + 4;
            pos += hsize as usize;
            if len == 0 {
                break;
            }
            continue;
        }
        if tag != *b"nu" {
            pos += hsize as usize;
            len -= hsize as i32 + 4;
            if len == 0 {
                break;
            }
            continue;
        }
        // unix3 header
        let n = UNIX3_LEN - 4;
        if pos + n > buf.len() {
            return Err(());
        }
        let perms = g16(buf, pos + 4);
        pos += n;
        len -= n as i32 + 4;
        if hsize as usize > UNIX3_LEN - 4 {
            pos += hsize as usize - 2;
        }
        entry.flags &= PHAR_ENT_COMPRESSION_MASK;
        entry.flags |= perms as u32 & PHAR_ENT_PERM_MASK;
        if len == 0 {
            break;
        }
    }
    Ok(())
}

/// phar_find_eocd()
fn find_eocd(s: &[u8]) -> Option<usize> {
    let n = s.len();
    if n < EOCD_LEN {
        return None;
    }
    let mut end = n + 4 - EOCD_LEN;
    loop {
        // zend_memnrstr(s, "PK\5\6", 4, end): last occurrence starting
        // before end - 4 + 1
        let hay = &s[..end.min(n)];
        let start = hay.windows(4).rposition(|w| w == b"PK\x05\x06")?;
        let comment_len = g16(s, start + 20) as usize;
        if start + EOCD_LEN + comment_len == n {
            return Some(start);
        }
        end = start;
        if end == 0 {
            return None;
        }
    }
}

/// phar_parse_zipfile()
pub fn parse_zipfile(vm: &mut Vm, fp: Vec<u8>, fname: &[u8], alias: Option<&[u8]>) -> Res<ArcRef> {
    let f = l(fname);
    let total = fp.len();
    let max = EOCD_LEN + 65536;
    let base = total.saturating_sub(max);
    let buf = &fp[base..];
    if buf.is_empty() {
        return fail(format!(
            "phar error: unable to read in data to search for end of central directory in zip-based phar \"{}\"",
            f
        ));
    }
    let Some(p) = find_eocd(buf) else {
        return fail(format!(
            "phar error: end of central directory not found in zip-based phar \"{}\"",
            f
        ));
    };
    let locator = &buf[p..p + EOCD_LEN];
    if g16(locator, 6) != 0 || g16(locator, 4) != 0 {
        return fail(format!(
            "phar error: split archives spanning multiple zips cannot be processed in zip-based phar \"{}\"",
            f
        ));
    }
    if g16(locator, 8) != g16(locator, 10) {
        return fail(format!(
            "phar error: corrupt zip archive, conflicting file count in end of central directory record in zip-based phar \"{}\"",
            f
        ));
    }
    let mydata = Rc::new(RefCell::new(Archive::default()));
    let comment_len = g16(locator, 20) as usize;
    let mut metadata: Option<Vec<u8>> = None;
    if comment_len > 0 {
        let mstart = p + EOCD_LEN;
        if comment_len != buf.len() - mstart {
            return fail(format!(
                "phar error: corrupt zip archive, zip file comment truncated in zip-based phar \"{}\"",
                f
            ));
        }
        let m = buf[mstart..mstart + comment_len].to_vec();
        mydata.borrow_mut().metadata.parse_lazy(&m);
        metadata = Some(m);
    }
    {
        let mut d = mydata.borrow_mut();
        d.fname = fname.to_vec();
        d.is_zip = true;
        d.set_ext_from_fname();
    }
    let zfail = |msg: &str| -> Res<ArcRef> {
        fail(format!("phar error: {} in zip-based phar \"{}\"", msg, f))
    };
    let cdir_offset = g32(locator, 16) as usize;
    let count = g16(locator, 10);
    let mut cur = cdir_offset;
    let mut actual_alias: Option<Vec<u8>> = None;
    for i in 0..count {
        let beforeus = cur;
        if cur + CENTRAL_LEN > total {
            return zfail("unable to read central directory entry, truncated");
        }
        let ze = fp[cur..cur + CENTRAL_LEN].to_vec();
        cur += CENTRAL_LEN;
        if &ze[0..4] != b"PK\x01\x02" {
            return zfail("corrupted central directory entry, no magic signature");
        }
        let mut entry = Entry {
            phar: Rc::downgrade(&mydata),
            is_zip: true,
            fp_type: FpType::Fp,
            compressed_filesize: g32(&ze, 20),
            uncompressed_filesize: g32(&ze, 24),
            crc32: g32(&ze, 16),
            timestamp: d2u_time(g16(&ze, 12), g16(&ze, 14)),
            flags: PHAR_ENT_PERM_DEF_FILE,
            header_offset: g32(&ze, 42) as i64,
            ..Default::default()
        };
        if g16(&ze, 8) & PHAR_ZIP_FLAG_ENCRYPTED != 0 {
            return zfail("Cannot process encrypted zip files");
        }
        let filename_len = g16(&ze, 28) as usize;
        if filename_len == 0 {
            return zfail("Cannot process zips created from stdin (zero-length filename)");
        }
        if cur + filename_len > total {
            return zfail("unable to read in filename from central directory, truncated");
        }
        let mut filename = fp[cur..cur + filename_len].to_vec();
        cur += filename_len;
        if filename.last() == Some(&b'/') {
            entry.is_dir = true;
            if filename.len() > 1 {
                filename.pop();
            }
            entry.flags |= PHAR_ENT_PERM_DEF_DIR;
        }
        entry.filename = filename;
        let extra_len = g16(&ze, 30);
        let mut local: Option<Vec<u8>> = None;
        if !entry.is_dir {
            let ho = entry.header_offset as usize;
            if ho + LOCAL_LEN > total {
                return zfail("phar error: internal corruption (cannot read local file header)");
            }
            let lh = fp[ho..ho + LOCAL_LEN].to_vec();
            if entry.filename.len() != g16(&lh, 26) as usize {
                return zfail(
                    "phar error: internal corruption (local file header does not match central directory)",
                );
            }
            let off = entry.header_offset
                + LOCAL_LEN as i64
                + entry.filename.len() as i64
                + g16(&lh, 28) as i64;
            entry.offset = off;
            entry.offset_abs = off;
            local = Some(lh);
        } else {
            let off = entry.header_offset
                + LOCAL_LEN as i64
                + entry.filename.len() as i64
                + extra_len as i64;
            entry.offset = off;
            entry.offset_abs = off;
        }
        if entry.filename == b".phar/signature.bin" {
            if entry.uncompressed_filesize > 0x10000 {
                return zfail("signatures larger than 64 KiB are not supported");
            }
            let mut sigfile = fp[..(entry.header_offset as usize).min(total)].to_vec();
            sigfile.extend_from_slice(&fp[cdir_offset.min(total)..beforeus.min(total)]);
            if let Some(m) = &metadata {
                sigfile.extend_from_slice(m);
            }
            let off = entry.offset as usize;
            let n = entry.uncompressed_filesize as usize;
            if off + n > total || n <= 8 {
                return zfail("signature cannot be read");
            }
            let sig = fp[off..off + n].to_vec();
            let sig_flags = g32(&sig, 0);
            mydata.borrow_mut().sig_flags = sig_flags;
            let slen = sigfile.len();
            match util::verify_signature(vm, &sigfile, slen, sig_flags, &sig[8..], fname) {
                Ok(s) => mydata.borrow_mut().signature = Some(s),
                Err(e) => {
                    let mut m = format!("signature cannot be verified: {}", e.unwrap_or_default());
                    m.truncate(127);
                    return zfail(&m);
                }
            }
            if i != count - 1 {
                return zfail("entries exist after signature, invalid phar");
            }
            continue;
        }
        util::add_virtual_dirs(&mut mydata.borrow_mut(), &entry.filename.clone());
        if extra_len > 0 {
            if process_extra(&fp, cur, &mut entry, extra_len).is_err() {
                return zfail("Unable to process extra field header for file in central directory");
            }
            cur += extra_len as usize;
        }
        match g16(&ze, 10) {
            PHAR_ZIP_COMP_NONE => {}
            PHAR_ZIP_COMP_DEFLATE => entry.flags |= PHAR_ENT_COMPRESSED_GZ,
            PHAR_ZIP_COMP_BZIP2 => entry.flags |= PHAR_ENT_COMPRESSED_BZ2,
            1 => return zfail("unsupported compression method (Shrunk) used in this zip"),
            2..=5 => return zfail("unsupported compression method (Reduce) used in this zip"),
            6 => return zfail("unsupported compression method (Implode) used in this zip"),
            7 => return zfail("unsupported compression method (Tokenize) used in this zip"),
            9 => return zfail("unsupported compression method (Deflate64) used in this zip"),
            10 => {
                return zfail(
                    "unsupported compression method (PKWare Implode/old IBM TERSE) used in this zip",
                );
            }
            14 => return zfail("unsupported compression method (LZMA) used in this zip"),
            18 => return zfail("unsupported compression method (IBM TERSE) used in this zip"),
            19 => return zfail("unsupported compression method (IBM LZ77) used in this zip"),
            97 => return zfail("unsupported compression method (WavPack) used in this zip"),
            98 => return zfail("unsupported compression method (PPMd) used in this zip"),
            _ => return zfail("unsupported compression method (unknown) used in this zip"),
        }
        let file_comment_len = g16(&ze, 32) as usize;
        if file_comment_len > 0 {
            if cur + file_comment_len > total {
                return zfail("unable to read in file comment, truncated");
            }
            let c = fp[cur..cur + file_comment_len].to_vec();
            cur += file_comment_len;
            entry.metadata.parse_lazy(&c);
        }
        if actual_alias.is_none() && entry.filename == b".phar/alias.txt" {
            let lh = local.as_ref().unwrap();
            if entry.crc32 != g32(lh, 14)
                || entry.uncompressed_filesize != g32(lh, 22)
                || entry.compressed_filesize != g32(lh, 18)
            {
                return zfail(
                    "phar error: internal corruption of zip-based phar (local header of alias does not match central directory)",
                );
            }
            let off = entry.offset as usize;
            let n = entry.uncompressed_filesize as usize;
            let raw = &fp[off.min(total)..];
            let a = if entry.flags & PHAR_ENT_COMPRESSED_GZ != 0 {
                util::zlib_inflate(
                    &raw[..(entry.compressed_filesize as usize).min(raw.len())],
                    -15,
                )
                .map(|mut d| {
                    d.truncate(n);
                    d
                })
                .unwrap_or_default()
            } else if entry.flags & PHAR_ENT_COMPRESSED_BZ2 != 0 {
                util::bz2_decompress(
                    vm,
                    &raw[..(entry.compressed_filesize as usize).min(raw.len())],
                )
                .map(|mut d| {
                    d.truncate(n);
                    d
                })
                .unwrap_or_default()
            } else {
                raw[..n.min(raw.len())].to_vec()
            };
            entry.uncompressed_filesize = a.len() as u32;
            if a.is_empty() {
                return zfail("unable to read in alias, truncated");
            }
            actual_alias = Some(a);
        }
        let fnm = entry.filename.clone();
        set_inode(&mut entry, fname);
        let mut d = mydata.borrow_mut();
        if !d.manifest.contains_key(&fnm) {
            d.manifest.insert(fnm, entry.into_ref());
        }
    }
    let is_data = !mydata
        .borrow()
        .manifest
        .contains_key(b".phar/stub.php".as_slice());
    mydata.borrow_mut().is_data = is_data;
    if !is_data && require_hash() && mydata.borrow().signature.is_none() {
        return zfail("signature is missing");
    }
    mydata.borrow_mut().fp = Some(Rc::new(RefCell::new(fp)));
    g(|g| {
        if !g.fname_map.contains_key(fname) {
            g.fname_map.insert(fname.to_vec(), mydata.clone());
        }
    });
    if let Some(a) = actual_alias {
        if !validate_alias(&a) {
            fname_map_del(fname);
            return fail(format!(
                "phar error: invalid alias \"{}\" in zip-based phar \"{}\"",
                l(&a),
                f
            ));
        }
        mydata.borrow_mut().is_temporary_alias = false;
        if let Some(fd) = find_alias(&a) {
            if util::free_alias(&fd).is_err() {
                fname_map_del(fname);
                return fail(format!(
                    "phar error: Unable to add zip-based phar \"{}\" with implicit alias, alias is already in use",
                    f
                ));
            }
        }
        mydata.borrow_mut().alias = Some(a.clone());
        g(|g| {
            if !g.alias_map.contains_key(&a) {
                g.alias_map.insert(a, mydata.clone());
            }
        });
    } else {
        match alias.filter(|a| !a.is_empty()) {
            Some(a) => {
                if let Some(fd) = find_alias(a) {
                    if util::free_alias(&fd).is_err() {
                        fname_map_del(fname);
                        return fail(format!(
                            "phar error: Unable to add zip-based phar \"{}\" with explicit alias, alias is already in use",
                            f
                        ));
                    }
                }
                g(|g| {
                    if !g.alias_map.contains_key(a) {
                        g.alias_map.insert(a.to_vec(), mydata.clone());
                    }
                });
                mydata.borrow_mut().alias = Some(a.to_vec());
            }
            None => mydata.borrow_mut().alias = Some(fname.to_vec()),
        }
        mydata.borrow_mut().is_temporary_alias = true;
    }
    Ok(mydata)
}

/// phar_open_or_create_zip()
pub fn open_or_create_zip(
    vm: &mut Vm,
    fname: &[u8],
    alias: Option<&[u8]>,
    is_data: bool,
    options: i64,
) -> Res<ArcRef> {
    let phar = create_or_parse_filename(vm, fname, alias, is_data, options)?;
    let mut p = phar.borrow_mut();
    p.is_data = is_data;
    if p.is_zip {
        drop(p);
        return Ok(phar);
    }
    if p.is_brandnew {
        p.is_zip = true;
        p.is_tar = false;
        drop(p);
        return Ok(phar);
    }
    fail(format!(
        "phar zip error: phar \"{}\" already exists as a regular phar and must be deleted from disk prior to creating as a zip-based phar",
        l(fname)
    ))
}

struct Pass {
    filefp: Vec<u8>,
    centralfp: Vec<u8>,
    old: Option<Buf>,
    error: Option<String>,
}

/// phar_zip_changed_apply_int(): false to stop.
fn changed_apply_int(vm: &mut Vm, phar: &ArcRef, entry: &EntryRef, p: &mut Pass) -> bool {
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
    let ename = l(&name);
    util::add_virtual_dirs(&mut phar.borrow_mut(), &name);
    let mut local = [0u8; LOCAL_LEN];
    let mut central = [0u8; CENTRAL_LEN];
    let mut perms = [0u8; UNIX3_LEN];
    local[0..4].copy_from_slice(b"PK\x03\x04");
    central[0..4].copy_from_slice(b"PK\x01\x02");
    s16(&mut central, 30, UNIX3_LEN as u16);
    s16(&mut local, 28, UNIX3_LEN as u16);
    perms[0] = b'n';
    perms[1] = b'u';
    s16(&mut perms, 2, (UNIX3_LEN - 4) as u16);
    let (flags, is_dir, ts) = {
        let e = entry.borrow();
        (e.flags, e.is_dir, e.timestamp)
    };
    s16(&mut perms, 8, (flags & PHAR_ENT_PERM_MASK) as u16);
    let pcrc = goro_zlib::crc32(0, &perms[8..10]);
    s32(&mut perms, 4, pcrc);
    if flags & PHAR_ENT_COMPRESSED_GZ != 0 {
        s16(&mut central, 10, PHAR_ZIP_COMP_DEFLATE);
        s16(&mut local, 8, PHAR_ZIP_COMP_DEFLATE);
    }
    if flags & PHAR_ENT_COMPRESSED_BZ2 != 0 {
        s16(&mut central, 10, PHAR_ZIP_COMP_BZIP2);
        s16(&mut local, 8, PHAR_ZIP_COMP_BZIP2);
    }
    let (dt, dd) = u2d_time(ts);
    s16(&mut local, 10, dt);
    s16(&mut local, 12, dd);
    s16(&mut central, 12, dt);
    s16(&mut central, 14, dd);
    let fl = (name.len() + usize::from(is_dir)) as u16;
    s16(&mut central, 28, fl);
    s16(&mut local, 26, fl);
    // language encoding flag (UTF-8 file names)
    s16(&mut central, 8, 1 << 11);
    s16(&mut local, 6, 1 << 11);
    s32(&mut central, 42, p.filefp.len() as u32);
    let mut not_really_modified = false;
    let mut cfp: Option<Vec<u8>> = None;
    let is_modified = entry.borrow().is_modified;
    let mut skip_to_dir = false;
    if is_modified {
        if is_dir {
            let mut e = entry.borrow_mut();
            e.is_modified = false;
            if e.fp_type == FpType::Mod {
                e.fp = None;
                e.fp_type = FpType::Fp;
            }
            skip_to_dir = true;
        }
        if !skip_to_dir {
            if util::open_entry_fp(vm, entry, false).is_err() {
                p.error = Some(format!(
                    "unable to open file contents of file \"{}\" in zip-based phar \"{}\"",
                    ename, pf
                ));
                return false;
            }
            let (eflags, old_flags) = {
                let e = entry.borrow();
                (e.flags, e.old_flags)
            };
            if eflags & PHAR_ENT_COMPRESSION_MASK != 0 && (old_flags == eflags || old_flags == 0) {
                not_really_modified = true;
            } else {
                let Some(data) = util::read_entry_contents(vm, entry, false) else {
                    p.error = Some(format!(
                        "unable to seek to start of file \"{}\" to zip-based phar \"{}\"",
                        ename, pf
                    ));
                    return false;
                };
                let crc = goro_zlib::crc32(0, &data);
                let usz = entry.borrow().uncompressed_filesize;
                entry.borrow_mut().crc32 = crc;
                s32(&mut central, 24, usz);
                s32(&mut local, 22, usz);
                if eflags & PHAR_ENT_COMPRESSION_MASK == 0 {
                    entry.borrow_mut().compressed_filesize = usz;
                    s32(&mut central, 20, usz);
                    s32(&mut local, 18, usz);
                } else {
                    let Some(c) = util::compress(vm, eflags, &data) else {
                        p.error = Some(if eflags & PHAR_ENT_COMPRESSED_GZ != 0 {
                            format!(
                                "unable to gzip compress file \"{}\" to zip-based phar \"{}\"",
                                ename, pf
                            )
                        } else {
                            format!(
                                "unable to bzip2 compress file \"{}\" to zip-based phar \"{}\"",
                                ename, pf
                            )
                        });
                        return false;
                    };
                    let csz = c.len() as u32;
                    {
                        let mut e = entry.borrow_mut();
                        e.compressed_filesize = csz;
                        e.old_flags = e.flags;
                        e.is_modified = true;
                    }
                    s32(&mut central, 20, csz);
                    s32(&mut local, 18, csz);
                    cfp = Some(c);
                }
            }
        }
    }
    if !skip_to_dir {
        if !is_modified || not_really_modified {
            let e = entry.borrow();
            s32(&mut central, 24, e.uncompressed_filesize);
            s32(&mut local, 22, e.uncompressed_filesize);
            s32(&mut central, 20, e.compressed_filesize);
            s32(&mut local, 18, e.compressed_filesize);
        }
        let crc = entry.borrow().crc32;
        s32(&mut central, 16, crc);
        s32(&mut local, 14, crc);
    }
    // continue_dir: file metadata
    if entry.borrow().metadata.has_data() {
        let need = {
            let e = entry.borrow();
            e.metadata.str.is_none() && e.metadata.val.is_some()
        };
        if need {
            let v = entry.borrow().metadata.val.clone().unwrap();
            let s = goro_ext_standard::serialize::php_serialize(vm, &v);
            entry.borrow_mut().metadata.str = s;
        }
        let n = entry
            .borrow()
            .metadata
            .str
            .as_ref()
            .map(|s| s.len())
            .unwrap_or(0) as u16;
        s16(&mut central, 32, n);
    }
    let header_offset = p.filefp.len();
    entry.borrow_mut().header_offset = header_offset as i64;
    let offset = header_offset + LOCAL_LEN + name.len() + usize::from(is_dir) + UNIX3_LEN;
    p.filefp.extend_from_slice(&local);
    p.centralfp.extend_from_slice(&central);
    p.filefp.extend_from_slice(&name);
    p.centralfp.extend_from_slice(&name);
    if is_dir {
        p.filefp.push(b'/');
        p.centralfp.push(b'/');
    }
    p.filefp.extend_from_slice(&perms);
    p.centralfp.extend_from_slice(&perms);
    let is_modified_now = entry.borrow().is_modified;
    if !not_really_modified && is_modified_now {
        if let Some(c) = cfp {
            p.filefp.extend_from_slice(&c);
        } else {
            if let Err(e) = util::open_entry_fp(vm, entry, false) {
                p.error = Some(e.unwrap_or_default());
                return false;
            }
            let Some(data) = util::read_entry_contents(vm, entry, false) else {
                p.error = Some(format!(
                    "unable to write contents of file \"{}\" in zip-based phar \"{}\"",
                    ename, pf
                ));
                return false;
            };
            p.filefp.extend_from_slice(&data);
        }
        entry.borrow_mut().is_modified = false;
    } else {
        entry.borrow_mut().is_modified = false;
        let (csz, is_dir, off) = {
            let e = entry.borrow();
            (
                e.compressed_filesize as usize,
                e.is_dir,
                e.offset_abs.max(0) as usize,
            )
        };
        if !is_dir && csz > 0 {
            let src = p.old.clone().or_else(|| util::get_efp(vm, entry, false));
            let ok = match &src {
                Some(old) => {
                    let o = old.borrow();
                    if off + csz <= o.len() {
                        p.filefp.extend_from_slice(&o[off..off + csz]);
                        true
                    } else {
                        false
                    }
                }
                None => false,
            };
            if !ok {
                p.error = Some(format!(
                    "unable to copy contents of file \"{}\" while creating zip-based phar \"{}\"",
                    ename, pf
                ));
                return false;
            }
        }
    }
    {
        let mut e = entry.borrow_mut();
        e.fp = None;
        e.offset = offset as i64;
        e.offset_abs = offset as i64;
        e.fp_type = FpType::Fp;
    }
    if let Some(s) = entry.borrow().metadata.str.clone() {
        p.centralfp.extend_from_slice(&s);
    }
    true
}

/// phar_zip_applysignature()
fn applysignature(vm: &mut Vm, phar: &ArcRef, pass: &mut Pass) -> Result<(), ()> {
    let (is_data, sig_flags) = {
        let p = phar.borrow();
        (p.is_data, p.sig_flags)
    };
    if is_data && sig_flags == 0 {
        return Ok(());
    }
    let mut newfile = pass.filefp.clone();
    newfile.extend_from_slice(&pass.centralfp);
    if let Some(m) = phar.borrow().metadata.str.clone() {
        newfile.extend_from_slice(&m);
    }
    let signature = match util::create_signature(vm, phar, &newfile) {
        Ok(s) => s,
        Err(e) => {
            pass.error = Some(format!(
                "phar error: unable to write signature to zip-based phar: {}",
                e.unwrap_or_default()
            ));
            return Err(());
        }
    };
    let sig_flags = phar.borrow().sig_flags;
    let mut data = Vec::new();
    data.extend_from_slice(&sig_flags.to_le_bytes());
    data.extend_from_slice(&(signature.len() as u32).to_le_bytes());
    data.extend_from_slice(&signature);
    let n = data.len() as u32;
    let entry = Entry {
        fp: Some(Rc::new(RefCell::new(data))),
        fp_type: FpType::Mod,
        is_modified: true,
        filename: b".phar/signature.bin".to_vec(),
        uncompressed_filesize: n,
        compressed_filesize: n,
        phar: Rc::downgrade(phar),
        ..Default::default()
    }
    .into_ref();
    changed_apply_int(vm, phar, &entry, pass);
    if pass.error.is_some() {
        return Err(());
    }
    Ok(())
}

/// phar_zip_flush()
pub fn zip_flush(
    vm: &mut Vm,
    phar: &ArcRef,
    user_stub: Option<&[u8]>,
    is_default_stub: bool,
) -> PErr {
    const NEWSTUB: &[u8] = b"<?php // zip-based phar archive stub file\n__HALT_COMPILER();";
    let fname = phar.borrow().fname.clone();
    let f = l(&fname);
    let mk = |data: Vec<u8>, name: &[u8]| -> EntryRef {
        let n = data.len() as u32;
        Entry {
            flags: PHAR_ENT_PERM_DEF_FILE,
            timestamp: now(),
            is_modified: true,
            is_zip: true,
            phar: Rc::downgrade(phar),
            fp_type: FpType::Mod,
            uncompressed_filesize: n,
            compressed_filesize: n,
            fp: Some(Rc::new(RefCell::new(data))),
            filename: name.to_vec(),
            ..Default::default()
        }
        .into_ref()
    };
    if !phar.borrow().is_data {
        let (temp, alias) = {
            let p = phar.borrow();
            (p.is_temporary_alias, p.alias.clone().unwrap_or_default())
        };
        if !temp && !alias.is_empty() {
            let e = mk(alias.clone(), b".phar/alias.txt");
            phar.borrow_mut()
                .manifest
                .insert(b".phar/alias.txt".to_vec(), e);
        } else {
            phar.borrow_mut()
                .manifest
                .shift_remove(b".phar/alias.txt".as_slice());
        }
        // register alias
        if !alias.is_empty() {
            if let Err(e) = util::get_archive(&fname, Some(&alias)) {
                return Some(e.unwrap_or_default());
            }
        }
        if let Some(stub) = user_stub.filter(|_| !is_default_stub) {
            let Some(pos) = stristr(stub, b"__HALT_COMPILER();") else {
                return Some(format!("illegal stub for zip-based phar \"{}\"", f));
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
    let oldfile: Option<Buf> = {
        let p = phar.borrow();
        if p.fp.is_some() && !p.is_brandnew {
            p.fp.clone()
        } else {
            None
        }
    };
    let oldfile = oldfile.or_else(|| read_file(&fname).map(|v| Rc::new(RefCell::new(v))));
    let mut pass = Pass {
        filefp: Vec::new(),
        centralfp: Vec::new(),
        old: oldfile,
        error: None,
    };
    let mut eocd = [0u8; EOCD_LEN];
    eocd[0..4].copy_from_slice(b"PK\x05\x06");
    {
        let mut p = phar.borrow_mut();
        if !p.is_data && p.sig_flags == 0 {
            p.sig_flags = PHAR_SIG_SHA256;
        }
    }
    let (n, sig) = {
        let p = phar.borrow();
        (p.manifest.len() as u16, p.sig_flags)
    };
    let n = if sig != 0 { n + 1 } else { n };
    s16(&mut eocd, 8, n);
    s16(&mut eocd, 10, n);
    let entries: Vec<EntryRef> = phar.borrow().manifest.values().cloned().collect();
    for e in &entries {
        if !changed_apply_int(vm, phar, e, &mut pass) {
            break;
        }
    }
    // phar_metadata_tracker_try_ensure_has_serialized_data()
    let need = {
        let p = phar.borrow();
        p.metadata.str.is_none() && p.metadata.val.is_some()
    };
    if need {
        let v = phar.borrow().metadata.val.clone().unwrap();
        let s = goro_ext_standard::serialize::php_serialize(vm, &v);
        phar.borrow_mut().metadata.str = s;
    }
    if let Some(e) = pass.error.take() {
        return Some(format!("phar zip flush of \"{}\" failed: {}", f, e));
    }
    if applysignature(vm, phar, &mut pass).is_err() {
        return Some(format!(
            "phar zip flush of \"{}\" failed: {}",
            f,
            pass.error.unwrap_or_default()
        ));
    }
    let cdir_size = pass.centralfp.len() as u32;
    let cdir_offset = pass.filefp.len() as u32;
    s32(&mut eocd, 12, cdir_size);
    s32(&mut eocd, 16, cdir_offset);
    let central = std::mem::take(&mut pass.centralfp);
    pass.filefp.extend_from_slice(&central);
    let meta = phar.borrow().metadata.str.clone();
    if let Some(m) = meta {
        s16(&mut eocd, 20, m.len() as u16);
        pass.filefp.extend_from_slice(&eocd);
        pass.filefp.extend_from_slice(&m);
    } else {
        pass.filefp.extend_from_slice(&eocd);
    }
    {
        let mut p = phar.borrow_mut();
        p.ufp = None;
        p.is_brandnew = false;
    }
    let newbuf = Rc::new(RefCell::new(pass.filefp));
    if !phar.borrow().donotflush && std::fs::write(l(&fname), &*newbuf.borrow()).is_err() {
        phar.borrow_mut().fp_cache = None;
        phar.borrow_mut().fp = Some(newbuf);
        crate::open_failed_warning(vm, &fname);
        return Some(format!("unable to open new phar \"{}\" for writing", f));
    }
    phar.borrow_mut().fp_cache = None;
    phar.borrow_mut().fp = Some(newbuf);
    None
}
