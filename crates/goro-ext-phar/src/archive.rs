//! The archive model (phar_internal.h) and the core of phar.c: opening,
//! parsing and creating phar archives, file name analysis and flushing of
//! phar-format archives.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use goro_core::value::Value;
use goro_core::vm::Vm;
use indexmap::{IndexMap, IndexSet};

use crate::util;

pub const PHP_PHAR_API_VERSION: &str = "1.1.1";
pub const PHAR_API_VERSION: u16 = 0x1110;
pub const PHAR_API_VERSION_NODIR: u16 = 0x1100;
pub const PHAR_API_MIN_DIR: u16 = 0x1110;
pub const PHAR_API_MIN_READ: u16 = 0x1000;
pub const PHAR_API_VER_MASK: u16 = 0xFFF0;

pub const PHAR_HDR_COMPRESSION_MASK: u32 = 0x0000F000;
pub const PHAR_HDR_SIGNATURE: u32 = 0x00010000;

pub const PHAR_FILE_COMPRESSION_MASK: u32 = 0x00F00000;
pub const PHAR_FILE_COMPRESSED_NONE: u32 = 0x00000000;
pub const PHAR_FILE_COMPRESSED_GZ: u32 = 0x00100000;
pub const PHAR_FILE_COMPRESSED_BZ2: u32 = 0x00200000;

pub const PHAR_SIG_MD5: u32 = 0x0001;
pub const PHAR_SIG_SHA1: u32 = 0x0002;
pub const PHAR_SIG_SHA256: u32 = 0x0003;
pub const PHAR_SIG_SHA512: u32 = 0x0004;
pub const PHAR_SIG_OPENSSL: u32 = 0x0010;
pub const PHAR_SIG_OPENSSL_SHA256: u32 = 0x0011;
pub const PHAR_SIG_OPENSSL_SHA512: u32 = 0x0012;

pub const PHAR_ENT_COMPRESSION_MASK: u32 = 0x0000F000;
pub const PHAR_ENT_COMPRESSED_NONE: u32 = 0x00000000;
pub const PHAR_ENT_COMPRESSED_GZ: u32 = 0x00001000;
pub const PHAR_ENT_COMPRESSED_BZ2: u32 = 0x00002000;

pub const PHAR_ENT_PERM_MASK: u32 = 0x000001FF;
pub const PHAR_ENT_PERM_DEF_FILE: u32 = 0x000001B6;
pub const PHAR_ENT_PERM_DEF_DIR: u32 = 0x000001FF;

pub const PHAR_FORMAT_SAME: i64 = 0;
pub const PHAR_FORMAT_PHAR: i64 = 1;
pub const PHAR_FORMAT_TAR: i64 = 2;
pub const PHAR_FORMAT_ZIP: i64 = 3;

pub const TAR_FILE: u8 = b'0';
pub const TAR_LINK: u8 = b'1';
pub const TAR_SYMLINK: u8 = b'2';
pub const TAR_DIR: u8 = b'5';
pub const TAR_GLOBAL_HDR: u8 = b'g';
pub const TAR_FILE_HDR: u8 = b'x';

pub const PHAR_MIME_PHP: i64 = 0;
pub const PHAR_MIME_PHPS: i64 = 1;
pub const PHAR_MIME_OTHER: i64 = 2;

pub const REPORT_ERRORS: i64 = 8;
pub const STREAM_OPEN_FOR_INCLUDE: i64 = 0x80;
pub const PHP_STREAM_URL_STAT_QUIET: i64 = 2;

/// A temporary stream (php_stream_fopen_tmpfile()) or the in-memory
/// contents of a file stream.
pub type Buf = Rc<RefCell<Vec<u8>>>;
pub type ArcRef = Rc<RefCell<Archive>>;
pub type EntryRef = Rc<RefCell<Entry>>;

pub fn new_buf() -> Buf {
    Rc::new(RefCell::new(Vec::new()))
}

/// Error of an operation reporting through `char **error` (None: failed
/// without a message).
pub type PErr = Option<String>;
pub type Res<T> = Result<T, PErr>;

pub fn fail<T>(msg: String) -> Res<T> {
    Err(Some(msg))
}

pub fn l(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

/// phar_fp_type
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FpType {
    /// phar_archive_data->fp
    Fp,
    /// phar_archive_data->ufp (decompressed contents)
    Ufp,
    /// phar_entry_info->fp (modified contents)
    Mod,
    /// a file outside of the phar mounted inside it (entry->tmp)
    Tmp,
}

/// phar_metadata_tracker: the metadata value and/or its serialization.
#[derive(Clone, Default)]
pub struct Meta {
    pub val: Option<Value>,
    pub str: Option<Vec<u8>>,
}

impl Meta {
    /// phar_metadata_tracker_has_data()
    pub fn has_data(&self) -> bool {
        self.val.is_some() || self.str.is_some()
    }
    /// phar_metadata_tracker_free()
    pub fn free(&mut self) {
        self.str = None;
        self.val = None;
    }
    /// phar_parse_metadata_lazy()
    pub fn parse_lazy(&mut self, buf: &[u8]) {
        self.free();
        if !buf.is_empty() {
            self.str = Some(buf.to_vec());
        }
    }
}

/// phar_entry_info
#[derive(Clone)]
pub struct Entry {
    pub uncompressed_filesize: u32,
    pub timestamp: u32,
    pub compressed_filesize: u32,
    pub crc32: u32,
    pub flags: u32,
    pub old_flags: u32,
    pub metadata: Meta,
    pub filename: Vec<u8>,
    pub fp_type: FpType,
    pub offset_abs: i64,
    pub offset: i64,
    pub header_offset: i64,
    pub fp: Option<Buf>,
    pub cfp: Option<Buf>,
    pub fp_refcount: i32,
    pub fileinfo_lock_count: u32,
    pub tmp: Option<Vec<u8>>,
    pub phar: Weak<RefCell<Archive>>,
    pub link: Option<Vec<u8>>,
    pub tar_type: u8,
    pub inode: u16,
    pub is_crc_checked: bool,
    pub is_modified: bool,
    pub is_deleted: bool,
    pub is_dir: bool,
    pub is_mounted: bool,
    pub is_temp_dir: bool,
    pub is_tar: bool,
    pub is_zip: bool,
}

impl Default for Entry {
    fn default() -> Entry {
        Entry {
            uncompressed_filesize: 0,
            timestamp: 0,
            compressed_filesize: 0,
            crc32: 0,
            flags: 0,
            old_flags: 0,
            metadata: Meta::default(),
            filename: Vec::new(),
            fp_type: FpType::Fp,
            offset_abs: 0,
            offset: 0,
            header_offset: 0,
            fp: None,
            cfp: None,
            fp_refcount: 0,
            fileinfo_lock_count: 0,
            tmp: None,
            phar: Weak::new(),
            link: None,
            tar_type: 0,
            inode: 0,
            is_crc_checked: false,
            is_modified: false,
            is_deleted: false,
            is_dir: false,
            is_mounted: false,
            is_temp_dir: false,
            is_tar: false,
            is_zip: false,
        }
    }
}

impl Entry {
    pub fn phar(&self) -> ArcRef {
        self.phar
            .upgrade()
            .expect("phar entry outlived its archive")
    }
    pub fn into_ref(self) -> EntryRef {
        Rc::new(RefCell::new(self))
    }
}

/// phar_archive_data
pub struct Archive {
    pub fname: Vec<u8>,
    /// Offset of the file extension in fname.
    pub ext: Option<usize>,
    pub ext_len: usize,
    pub alias: Option<Vec<u8>>,
    pub version: String,
    pub halt_offset: usize,
    pub manifest: IndexMap<Vec<u8>, EntryRef>,
    pub virtual_dirs: IndexSet<Vec<u8>>,
    pub mounted_dirs: IndexSet<Vec<u8>>,
    pub flags: u32,
    pub min_timestamp: u32,
    pub max_timestamp: u32,
    pub fp: Option<Buf>,
    /// The file `fp` was read from, (device, inode): php's phar->fp is an
    /// open stream, which sees the file rewritten in place.
    pub fp_ino: Option<(u64, u64)>,
    pub ufp: Option<Buf>,
    pub refcount: i32,
    pub sig_flags: u32,
    pub signature: Option<Vec<u8>>,
    pub metadata: Meta,
    pub is_temporary_alias: bool,
    pub is_modified: bool,
    pub is_writeable: bool,
    pub is_brandnew: bool,
    pub donotflush: bool,
    pub is_zip: bool,
    pub is_tar: bool,
    pub is_data: bool,
}

impl Default for Archive {
    fn default() -> Archive {
        Archive {
            fname: Vec::new(),
            ext: None,
            ext_len: 0,
            alias: None,
            version: String::new(),
            halt_offset: 0,
            manifest: IndexMap::new(),
            virtual_dirs: IndexSet::new(),
            mounted_dirs: IndexSet::new(),
            flags: 0,
            min_timestamp: 0,
            max_timestamp: 0,
            fp: None,
            fp_ino: None,
            ufp: None,
            refcount: 0,
            sig_flags: 0,
            signature: None,
            metadata: Meta::default(),
            is_temporary_alias: false,
            is_modified: false,
            is_writeable: false,
            is_brandnew: false,
            donotflush: false,
            is_zip: false,
            is_tar: false,
            is_data: false,
        }
    }
}

impl Archive {
    pub fn alias_len(&self) -> usize {
        self.alias.as_ref().map(|a| a.len()).unwrap_or(0)
    }
    pub fn find(&self, name: &[u8]) -> Option<EntryRef> {
        self.manifest.get(name).cloned()
    }
    /// Set ext/ext_len from fname: the first '.' of the basename (but not
    /// a leading one).
    pub fn set_ext_from_fname(&mut self) {
        let f = &self.fname;
        if let Some(slash) = f.iter().rposition(|&c| c == b'/') {
            let mut ext = f[slash..]
                .iter()
                .position(|&c| c == b'.')
                .map(|i| i + slash);
            if ext == Some(slash) {
                ext = f[slash + 1..]
                    .iter()
                    .position(|&c| c == b'.')
                    .map(|i| i + slash + 1);
            }
            if let Some(e) = ext {
                self.ext = Some(e);
                self.ext_len = f.len() - e;
            }
        }
    }
}

/// phar_entry_data: an open handle on an entry.
pub struct EntryData {
    pub phar: ArcRef,
    pub fp: Option<Buf>,
    pub position: i64,
    pub zero: i64,
    pub internal_file: EntryRef,
}

/// PHAR_G()
pub struct Globals {
    pub fname_map: IndexMap<Vec<u8>, ArcRef>,
    pub alias_map: IndexMap<Vec<u8>, ArcRef>,
    pub readonly: bool,
    pub require_hash: bool,
    pub readonly_orig: bool,
    pub require_hash_orig: bool,
    pub intercepted: bool,
    pub request_init: bool,
    pub cwd: Option<Vec<u8>>,
    pub cwd_init: bool,
    pub openssl_privatekey: Option<Vec<u8>>,
    pub last_phar: Option<ArcRef>,
    pub last_phar_name: Vec<u8>,
    pub last_alias: Option<Vec<u8>>,
    pub mung_list: u32,
}

thread_local! {
    pub static G: RefCell<Globals> = RefCell::new(Globals {
        fname_map: IndexMap::new(),
        alias_map: IndexMap::new(),
        readonly: true,
        require_hash: true,
        readonly_orig: true,
        require_hash_orig: true,
        intercepted: false,
        request_init: false,
        cwd: None,
        cwd_init: false,
        openssl_privatekey: None,
        last_phar: None,
        last_phar_name: Vec::new(),
        last_alias: None,
        mung_list: 0,
    });
}

pub fn g<R>(f: impl FnOnce(&mut Globals) -> R) -> R {
    G.with(|g| f(&mut g.borrow_mut()))
}

pub fn readonly() -> bool {
    g(|g| g.readonly)
}

pub fn require_hash() -> bool {
    g(|g| g.require_hash)
}

pub fn find_fname(fname: &[u8]) -> Option<ArcRef> {
    g(|g| g.fname_map.get(fname).cloned())
}

pub fn find_alias(alias: &[u8]) -> Option<ArcRef> {
    g(|g| g.alias_map.get(alias).cloned())
}

/// Invalidate the phar_get_archive() cache.
pub fn invalidate_last() {
    g(|g| {
        g.last_phar = None;
        g.last_phar_name.clear();
        g.last_alias = None;
    });
}

/// zend_hash_str_del(&PHAR_G(phar_fname_map), fname): the destructor
/// (destroy_phar_data) drops the aliases pointing to the archive.
pub fn fname_map_del(fname: &[u8]) -> bool {
    let removed = g(|g| g.fname_map.shift_remove(fname));
    match removed {
        Some(p) => {
            g(|g| g.alias_map.retain(|_, v| !Rc::ptr_eq(v, &p)));
            let last = g(|g| g.last_phar.as_ref().is_some_and(|l| Rc::ptr_eq(l, &p)));
            if last {
                invalidate_last();
            }
            true
        }
        None => false,
    }
}

pub fn now() -> u32 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as u32)
        .unwrap_or(0)
}

/// phar_validate_alias()
pub fn validate_alias(alias: &[u8]) -> bool {
    !alias
        .iter()
        .any(|c| matches!(c, b'/' | b'\\' | b':' | b';' | b'\n' | b'\r'))
}

/// zend_inline_hash_func() (DJBX33A)
fn zend_hash_func(s: &[u8]) -> u64 {
    let mut h: u64 = 5381;
    for &c in s {
        h = h.wrapping_mul(33).wrapping_add(c as u64);
    }
    h | 0x8000_0000_0000_0000
}

const MAXPATHLEN: usize = 4096;

/// phar_set_inode()
pub fn set_inode(entry: &mut Entry, phar_fname: &[u8]) {
    let tmp_len = MAXPATHLEN.min(entry.filename.len() + phar_fname.len());
    let len1 = phar_fname.len().min(tmp_len);
    let mut tmp = phar_fname[..len1].to_vec();
    let len2 = (tmp_len - len1).min(entry.filename.len());
    tmp.extend_from_slice(&entry.filename[..len2]);
    entry.inode = zend_hash_func(&tmp) as u16;
}

/// The (device, inode) of a plain file.
pub fn file_ino(fname: &[u8]) -> Option<(u64, u64)> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let m = std::fs::metadata(l(fname)).ok()?;
        m.is_file().then(|| (m.dev(), m.ino()))
    }
    #[cfg(not(unix))]
    {
        let _ = fname;
        None
    }
}

/// What reading the archive's open stream sees now: the file it was
/// opened from, when that file was rewritten in place since (a file
/// replaced by another one leaves the stream on the old contents).
fn fp_current(p: &Archive, fp: Buf) -> Buf {
    if p.fp_ino.is_some() && file_ino(&p.fname) == p.fp_ino {
        if let Some(now) = read_file(&p.fname) {
            if now != *fp.borrow() {
                *fp.borrow_mut() = now;
            }
        }
    }
    fp
}

/// php_stream_open_wrapper(fname, "rb", IGNORE_URL|STREAM_MUST_SEEK)
pub fn read_file(fname: &[u8]) -> Option<Vec<u8>> {
    if fname.is_empty() {
        return None;
    }
    let p = l(fname);
    if std::path::Path::new(&p).is_dir() {
        return None;
    }
    std::fs::read(&p).ok()
}

/// expand_filepath(): an absolute, lexically normalized path.
pub fn expand_filepath(path: &[u8]) -> Option<Vec<u8>> {
    if path.is_empty() || path.len() >= MAXPATHLEN - 1 {
        return None;
    }
    let mut full = if path[0] == b'/' {
        path.to_vec()
    } else {
        let cwd = std::env::current_dir().ok()?;
        let mut c = cwd.to_string_lossy().as_bytes().to_vec();
        c.push(b'/');
        c.extend_from_slice(path);
        c
    };
    // virtual_file_ex(CWD_FILEPATH): collapse "//", "." and ".."
    let mut parts: Vec<&[u8]> = Vec::new();
    for seg in full.split(|&c| c == b'/') {
        match seg {
            b"" | b"." => {}
            b".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    let mut out = Vec::with_capacity(full.len());
    for p in &parts {
        out.push(b'/');
        out.extend_from_slice(p);
    }
    if out.is_empty() {
        out.push(b'/');
    }
    full.clear();
    if out.len() >= MAXPATHLEN {
        return None;
    }
    // (CWD_FILEPATH: the symbolic links of the existing part are resolved)
    use std::os::unix::ffi::OsStrExt;
    let canon = |p: &[u8]| -> Option<Vec<u8>> {
        std::fs::canonicalize(std::ffi::OsStr::from_bytes(p))
            .ok()
            .map(|c| c.as_os_str().as_bytes().to_vec())
    };
    if let Some(c) = canon(&out) {
        return Some(c);
    }
    let mut cut = out.len();
    while let Some(s) = out[..cut].iter().rposition(|&c| c == b'/') {
        if s == 0 {
            break;
        }
        if let Some(mut c) = canon(&out[..s]) {
            c.extend_from_slice(&out[s..]);
            return Some(c);
        }
        cut = s;
    }
    Some(out)
}

/// php_stream_stat_path(): (is_dir) or None.
pub fn stat_is_dir(vm: &mut Vm, path: &[u8]) -> Option<bool> {
    if has_scheme(path) {
        return goro_ext_standard::native_streams::stat_path(vm, path, PHP_STREAM_URL_STAT_QUIET)
            .ok()
            .flatten()
            .map(|sb| sb.mode & 0o170000 == 0o040000);
    }
    std::fs::metadata(l(path)).ok().map(|m| m.is_dir())
}

/// "scheme://" prefix
pub fn has_scheme(p: &[u8]) -> bool {
    let n = p
        .iter()
        .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, b'+' | b'-' | b'.'))
        .count();
    n > 1 && p[n..].starts_with(b"://")
}

pub fn starts_with_ci(s: &[u8], p: &[u8]) -> bool {
    s.len() >= p.len() && s[..p.len()].eq_ignore_ascii_case(p)
}

/// php_stristr()-like search: offset of `needle` in `hay`, ignoring case.
pub fn stristr(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || hay.len() < needle.len() {
        return None;
    }
    (0..=hay.len() - needle.len()).find(|&i| hay[i..i + needle.len()].eq_ignore_ascii_case(needle))
}

pub fn memmem(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || hay.len() < needle.len() {
        return None;
    }
    hay.windows(needle.len()).position(|w| w == needle)
}

// ---------------------------------------------------------------------------
// reference counting

/// phar_archive_delref(): true when the archive was destroyed.
pub fn archive_delref(phar: &ArcRef) -> bool {
    let (rc, fname) = {
        let mut p = phar.borrow_mut();
        p.refcount -= 1;
        (p.refcount, p.fname.clone())
    };
    if rc < 0 {
        fname_map_del(&fname);
        return true;
    } else if rc == 0 {
        invalidate_last();
        let mut p = phar.borrow_mut();
        let mut remove_fname_cache = p.manifest.is_empty();
        if p.fp.is_some() && (p.flags & PHAR_FILE_COMPRESSION_MASK == 0 || p.alias.is_none()) {
            p.fp = None;
            remove_fname_cache |= p.flags & PHAR_FILE_COMPRESSION_MASK != 0;
        }
        drop(p);
        if remove_fname_cache {
            fname_map_del(&fname);
            return true;
        }
    }
    false
}

/// phar_entry_delref()
pub fn entry_delref(idata: EntryData) {
    {
        let mut e = idata.internal_file.borrow_mut();
        e.fp_refcount -= 1;
        if e.fp_refcount < 0 {
            e.fp_refcount = 0;
        }
    }
    archive_delref(&idata.phar);
}

/// phar_entry_remove()
pub fn entry_remove(vm: &mut Vm, idata: EntryData) -> PErr {
    let phar = idata.phar.clone();
    let (refc, locks, name) = {
        let e = idata.internal_file.borrow();
        (e.fp_refcount, e.fileinfo_lock_count, e.filename.clone())
    };
    if refc < 2 && locks == 0 {
        let mut p = phar.borrow_mut();
        p.manifest.shift_remove(&name);
        p.refcount -= 1;
    } else {
        idata.internal_file.borrow_mut().is_deleted = true;
        entry_delref(idata);
    }
    if !phar.borrow().donotflush {
        return flush(vm, &phar);
    }
    None
}

// ---------------------------------------------------------------------------
// opening

/// phar_open_parsed_phar(): Ok when already loaded.
pub fn open_parsed_phar(
    fname: &[u8],
    alias: Option<&[u8]>,
    is_data: bool,
    _options: i64,
) -> Res<ArcRef> {
    match util::get_archive(fname, alias) {
        Ok(phar) => {
            let ok = {
                let p = phar.borrow();
                alias.is_none_or(|a| a.is_empty()) || p.fname == fname
            };
            if ok {
                if !is_data {
                    let p = phar.borrow();
                    if p.halt_offset == 0
                        && !p.is_brandnew
                        && (p.is_tar || p.is_zip)
                        && readonly()
                        && !p.manifest.contains_key(b".phar/stub.php".as_slice())
                    {
                        return fail(format!(
                            "'{}' is not a phar archive. Use PharData::__construct() for a standard zip or tar archive",
                            l(fname)
                        ));
                    }
                }
                return Ok(phar);
            }
            Err(None)
        }
        Err(e) => Err(e),
    }
}

/// php_stream_read() of `n` bytes at `pos`.
fn rd(fp: &[u8], pos: usize, n: usize) -> Option<&[u8]> {
    if pos.checked_add(n)? > fp.len() {
        return None;
    }
    Some(&fp[pos..pos + n])
}

fn get32(b: &[u8], pos: &mut usize) -> u32 {
    let v = u32::from_le_bytes([b[*pos], b[*pos + 1], b[*pos + 2], b[*pos + 3]]);
    *pos += 4;
    v
}

/// phar_parse_pharfile()
#[allow(clippy::too_many_arguments)]
pub fn parse_pharfile(
    vm: &mut Vm,
    fp: Vec<u8>,
    fname: &[u8],
    alias: Option<&[u8]>,
    mut halt_offset: usize,
    compression: u32,
) -> Res<ArcRef> {
    let f = l(fname);
    let alloc_fail = |m: &str| -> Res<ArcRef> { fail(m.replace("%s", &f)) };
    if halt_offset > fp.len() {
        return alloc_fail("cannot seek to __HALT_COMPILER(); location in phar \"%s\"");
    }
    let Some(b3) = rd(&fp, halt_offset, 3) else {
        return alloc_fail("internal corruption of phar \"%s\" (truncated manifest at stub end)");
    };
    if (b3[0] == b' ' || b3[0] == b'\n') && b3[1] == b'?' && b3[2] == b'>' {
        halt_offset += 3;
        let Some(&next) = fp.get(halt_offset) else {
            return alloc_fail(
                "internal corruption of phar \"%s\" (truncated manifest at stub end)",
            );
        };
        let mut nextchar = next;
        if nextchar == b'\r' {
            match fp.get(halt_offset + 1) {
                Some(b'\n') => {
                    nextchar = b'\n';
                }
                _ => {
                    return alloc_fail(
                        "internal corruption of phar \"%s\" (truncated manifest at stub end)",
                    );
                }
            }
            halt_offset += 1;
        }
        if nextchar == b'\n' {
            halt_offset += 1;
        }
    }
    let Some(b4) = rd(&fp, halt_offset, 4) else {
        return alloc_fail(
            "internal corruption of phar \"%s\" (truncated manifest at manifest length)",
        );
    };
    let manifest_len = u32::from_le_bytes([b4[0], b4[1], b4[2], b4[3]]) as usize;
    if manifest_len > 1048576 * 100 {
        return alloc_fail("manifest cannot be larger than 100 MB in phar \"%s\"");
    }
    const MANIFEST_FIXED_LEN: usize = 18;
    let Some(buf) =
        rd(&fp, halt_offset + 4, manifest_len).filter(|_| manifest_len >= MANIFEST_FIXED_LEN)
    else {
        return alloc_fail("internal corruption of phar \"%s\" (truncated manifest header)");
    };
    let buf = buf.to_vec();
    let end = buf.len();
    let mut pos = 0usize;
    macro_rules! safe32 {
        () => {{
            if pos + 4 > end {
                return alloc_fail(
                    "internal corruption of phar \"%s\" (truncated manifest header)",
                );
            }
            get32(&buf, &mut pos)
        }};
    }
    let manifest_count = safe32!();
    if manifest_count == 0 {
        return alloc_fail(
            "in phar \"%s\", manifest claims to have zero entries.  Phars must have at least 1 entry",
        );
    }
    let manifest_ver = ((buf[pos] as u16) << 8) + buf[pos + 1] as u16;
    pos += 2;
    if manifest_ver & PHAR_API_VER_MASK < PHAR_API_MIN_READ {
        return fail(format!(
            "phar \"{}\" is API version {}.{}.{}, and cannot be processed",
            f,
            manifest_ver >> 12,
            (manifest_ver >> 8) & 0xF,
            (manifest_ver >> 4) & 0x0F
        ));
    }
    let mut manifest_flags = safe32!();
    manifest_flags &= !PHAR_HDR_COMPRESSION_MASK;
    manifest_flags &= !PHAR_FILE_COMPRESSION_MASK;
    manifest_flags |= compression;

    let mut signature: Option<Vec<u8>> = None;
    let sig_flags: u32;
    if manifest_flags & PHAR_HDR_SIGNATURE != 0 {
        let broken = || fail(format!("phar \"{}\" has a broken signature", f));
        let flen = fp.len();
        if flen < 8 || flen - 8 < 20 || &fp[flen - 4..] != b"GBMB" {
            return broken();
        }
        sig_flags = u32::from_le_bytes([fp[flen - 8], fp[flen - 7], fp[flen - 6], fp[flen - 5]]);
        let check = |vm: &mut Vm, digest_len: usize, name: &str| -> Res<Vec<u8>> {
            if flen < 8 + digest_len {
                return fail(format!("phar \"{}\" has a broken signature", f));
            }
            let read_len = flen - 8 - digest_len;
            let digest = &fp[read_len..read_len + digest_len];
            match util::verify_signature(vm, &fp, read_len, sig_flags, digest, fname) {
                Ok(s) => Ok(s),
                Err(e) => fail(format!(
                    "phar \"{}\" {} signature could not be verified: {}",
                    f,
                    name,
                    e.unwrap_or_default()
                )),
            }
        };
        match sig_flags {
            PHAR_SIG_OPENSSL_SHA512 | PHAR_SIG_OPENSSL_SHA256 | PHAR_SIG_OPENSSL => {
                // the signature followed by its length
                if flen < 12 {
                    return fail(format!(
                        "phar \"{}\" openssl signature length could not be read",
                        f
                    ));
                }
                let signature_len =
                    u32::from_le_bytes([fp[flen - 12], fp[flen - 11], fp[flen - 10], fp[flen - 9]])
                        as usize;
                let Some(end_of_phar) = (flen - 12).checked_sub(signature_len).filter(|&e| e > 0)
                else {
                    return fail(format!(
                        "phar \"{}\" openssl signature could not be read",
                        f
                    ));
                };
                let sig = fp[end_of_phar..end_of_phar + signature_len].to_vec();
                match util::verify_signature(vm, &fp, end_of_phar, sig_flags, &sig, fname) {
                    Ok(s) => signature = Some(s),
                    Err(e) => {
                        return fail(format!(
                            "phar \"{}\" openssl signature could not be verified: {}",
                            f,
                            e.unwrap_or_default()
                        ));
                    }
                }
            }
            PHAR_SIG_SHA512 => signature = Some(check(vm, 64, "SHA512")?),
            PHAR_SIG_SHA256 => signature = Some(check(vm, 32, "SHA256")?),
            PHAR_SIG_SHA1 => signature = Some(check(vm, 20, "SHA1")?),
            PHAR_SIG_MD5 => signature = Some(check(vm, 16, "MD5")?),
            _ => {
                return fail(format!(
                    "phar \"{}\" has a broken or unsupported signature",
                    f
                ));
            }
        }
    } else if require_hash() {
        return fail(format!("phar \"{}\" does not have a signature", f));
    } else {
        sig_flags = 0;
    }

    // alias
    let tmp_len = safe32!() as usize;
    if pos + tmp_len > end {
        return alloc_fail("internal corruption of phar \"%s\" (buffer overrun)");
    }
    if manifest_len < MANIFEST_FIXED_LEN + tmp_len {
        return alloc_fail("internal corruption of phar \"%s\" (truncated manifest header)");
    }
    let mut register_alias = false;
    let mut temp_alias = false;
    let mut alias: Option<Vec<u8>> = alias.map(|a| a.to_vec());
    if tmp_len > 0 {
        let stored = buf[pos..pos + tmp_len].to_vec();
        if let Some(a) = alias.as_ref().filter(|a| !a.is_empty()) {
            if *a != stored {
                return fail(format!(
                    "cannot load phar \"{}\" with implicit alias \"{}\" under different alias \"{}\"",
                    f,
                    l(&stored),
                    l(a)
                ));
            }
        }
        alias = Some(stored);
        pos += tmp_len;
        register_alias = true;
    } else if alias.as_ref().is_none_or(|a| a.is_empty()) {
        alias = None;
    } else {
        register_alias = true;
        temp_alias = true;
    }

    if manifest_count as usize > (manifest_len - MANIFEST_FIXED_LEN - tmp_len) / (5 * 4 + 1) {
        return alloc_fail(
            "internal corruption of phar \"%s\" (too many manifest entries for size of manifest)",
        );
    }

    let mydata = Rc::new(RefCell::new(Archive::default()));
    let len = safe32!() as usize;
    if len > end - pos {
        return alloc_fail("internal corruption of phar \"%s\" (trying to read past buffer end)");
    }
    mydata
        .borrow_mut()
        .metadata
        .parse_lazy(&buf[pos..pos + len]);
    pos += len;
    mydata.borrow_mut().fname = fname.to_vec();
    let mut offset = (halt_offset + manifest_len + 4) as i64;
    let first_offset = offset;
    for _ in 0..manifest_count {
        if pos + 28 > end {
            return alloc_fail("internal corruption of phar \"%s\" (truncated manifest entry)");
        }
        let mut filename_len = get32(&buf, &mut pos) as usize;
        if filename_len == 0 {
            return alloc_fail("zero-length filename encountered in phar \"%s\"");
        }
        if filename_len > end - pos - 24 {
            return alloc_fail("internal corruption of phar \"%s\" (truncated manifest entry)");
        }
        let mut entry = Entry {
            phar: Rc::downgrade(&mydata),
            fp_type: FpType::Fp,
            ..Default::default()
        };
        entry.is_dir = manifest_ver & PHAR_API_VER_MASK >= PHAR_API_MIN_DIR
            && buf[pos + filename_len - 1] == b'/';
        util::add_virtual_dirs(&mut mydata.borrow_mut(), &buf[pos..pos + filename_len]);
        let filename_raw = pos;
        pos += filename_len;
        entry.uncompressed_filesize = get32(&buf, &mut pos);
        entry.timestamp = get32(&buf, &mut pos);
        {
            let mut d = mydata.borrow_mut();
            if offset == first_offset {
                d.min_timestamp = entry.timestamp;
                d.max_timestamp = entry.timestamp;
            } else if d.min_timestamp > entry.timestamp {
                d.min_timestamp = entry.timestamp;
            } else if d.max_timestamp < entry.timestamp {
                d.max_timestamp = entry.timestamp;
            }
        }
        entry.compressed_filesize = get32(&buf, &mut pos);
        entry.crc32 = get32(&buf, &mut pos);
        entry.flags = get32(&buf, &mut pos);
        if entry.is_dir {
            filename_len -= 1;
            entry.flags |= PHAR_ENT_PERM_DEF_DIR;
        }
        entry.filename = buf[filename_raw..filename_raw + filename_len].to_vec();
        let mlen = get32(&buf, &mut pos) as usize;
        if mlen > end - pos {
            return alloc_fail("internal corruption of phar \"%s\" (truncated manifest entry)");
        }
        entry.metadata.parse_lazy(&buf[pos..pos + mlen]);
        pos += mlen;
        entry.offset = offset;
        entry.offset_abs = offset;
        offset += entry.compressed_filesize as i64;
        match entry.flags & PHAR_ENT_COMPRESSION_MASK {
            PHAR_ENT_COMPRESSED_GZ | PHAR_ENT_COMPRESSED_BZ2 => {}
            _ => {
                if entry.uncompressed_filesize != entry.compressed_filesize {
                    return alloc_fail(
                        "internal corruption of phar \"%s\" (compressed and uncompressed size does not match for uncompressed entry)",
                    );
                }
            }
        }
        manifest_flags |= entry.flags & PHAR_ENT_COMPRESSION_MASK;
        entry.is_crc_checked = manifest_flags & PHAR_HDR_SIGNATURE != 0;
        set_inode(&mut entry, fname);
        let key = entry.filename.clone();
        let mut d = mydata.borrow_mut();
        if !d.manifest.contains_key(&key) {
            d.manifest.insert(key, entry.into_ref());
        }
    }
    {
        let mut d = mydata.borrow_mut();
        d.version = format!(
            "{}.{}.{}",
            manifest_ver >> 12,
            (manifest_ver >> 8) & 0xF,
            (manifest_ver >> 4) & 0xF
        );
        d.halt_offset = halt_offset;
        d.flags = manifest_flags;
        d.set_ext_from_fname();
        d.alias = Some(alias.clone().unwrap_or_else(|| fname.to_vec()));
        d.sig_flags = sig_flags;
        d.fp = Some(Rc::new(RefCell::new(fp)));
        // (a compressed phar's fp is a decompressed copy)
        d.fp_ino = if compression == PHAR_FILE_COMPRESSED_NONE {
            file_ino(fname)
        } else {
            None
        };
        d.signature = signature;
    }
    request_initialize();
    if register_alias {
        let a = alias.unwrap_or_default();
        mydata.borrow_mut().is_temporary_alias = temp_alias;
        if !validate_alias(&a) {
            return alloc_fail("Cannot open archive \"%s\", invalid alias");
        }
        if let Some(fd) = find_alias(&a) {
            if util::free_alias(&fd).is_err() {
                return alloc_fail(
                    "Cannot open archive \"%s\", alias is already in use by existing archive",
                );
            }
        }
        g(|g| {
            if !g.alias_map.contains_key(&a) {
                g.alias_map.insert(a, mydata.clone());
            }
        });
    } else {
        mydata.borrow_mut().is_temporary_alias = true;
    }
    g(|g| {
        if !g.fname_map.contains_key(fname) {
            g.fname_map.insert(fname.to_vec(), mydata.clone());
        }
    });
    Ok(mydata)
}

/// phar_request_initialize()
pub fn request_initialize() {
    g(|g| {
        if !g.request_init {
            g.request_init = true;
            g.last_phar = None;
            g.last_phar_name.clear();
            g.last_alias = None;
            g.mung_list = 0;
            g.cwd = None;
            g.cwd_init = false;
        }
    });
}

/// phar_open_or_create_filename()
pub fn open_or_create_filename(
    vm: &mut Vm,
    fname: &[u8],
    alias: Option<&[u8]>,
    is_data: bool,
    options: i64,
) -> Res<ArcRef> {
    let f = l(fname);
    let (mut ext_str, mut ext_len) = (None, 0i64);
    let found = detect_phar_fname_ext(
        vm,
        fname,
        &mut ext_str,
        &mut ext_len,
        !is_data as i32,
        0,
        true,
    );
    if !found {
        if !detect_phar_fname_ext(
            vm,
            fname,
            &mut ext_str,
            &mut ext_len,
            !is_data as i32,
            1,
            true,
        ) {
            if ext_len == -2 {
                return fail(format!(
                    "Cannot create a phar archive from a URL like \"{}\". Phar objects can only be created from local files",
                    f
                ));
            }
            return fail(format!(
                "Cannot create phar '{}', file extension (or combination) not recognised or the directory does not exist",
                f
            ));
        }
    }
    match open_parsed_phar(fname, alias, is_data, options) {
        Ok(test) => {
            let t = test.borrow();
            if t.is_data && !t.is_tar && !t.is_zip {
                return fail(format!(
                    "Cannot open '{}' as a PharData object. Use Phar::__construct() for executable archives",
                    f
                ));
            }
            if readonly()
                && !t.is_data
                && (t.is_tar || t.is_zip)
                && !t.manifest.contains_key(b".phar/stub.php".as_slice())
            {
                return fail(format!(
                    "'{}' is not a phar archive. Use PharData::__construct() for a standard zip or tar archive",
                    f
                ));
            }
            let w = !readonly() || t.is_data;
            drop(t);
            if w {
                test.borrow_mut().is_writeable = true;
            }
            return Ok(test);
        }
        Err(Some(e)) => return Err(Some(e)),
        Err(None) => {}
    }
    let ext = ext_str
        .map(|i| &fname[i..i + ext_len.max(0) as usize])
        .unwrap_or(b"");
    if ext.len() > 3 {
        if let Some(z) = ext.iter().position(|&c| c == b'z') {
            if ext.len() - z >= 3 && &ext[z + 1..z + 3] == b"ip" {
                return crate::zip::open_or_create_zip(vm, fname, alias, is_data, options);
            }
        }
        if let Some(t) = ext.iter().position(|&c| c == b't') {
            if ext.len() - t >= 3 && &ext[t + 1..t + 3] == b"ar" {
                return crate::tar::open_or_create_tar(vm, fname, alias, is_data, options);
            }
        }
    }
    create_or_parse_filename(vm, fname, alias, is_data, options)
}

/// phar_create_or_parse_filename()
pub fn create_or_parse_filename(
    vm: &mut Vm,
    fname: &[u8],
    alias: Option<&[u8]>,
    is_data: bool,
    options: i64,
) -> Res<ArcRef> {
    if let Some(fp) = read_file(fname) {
        let actual = expand_filepath(fname).unwrap_or_else(|| fname.to_vec());
        let p = open_from_fp(vm, fp, &actual, alias, options)?;
        let w = p.borrow().is_data || !readonly();
        if w {
            p.borrow_mut().is_writeable = true;
        }
        return Ok(p);
    }
    if readonly() && !is_data {
        if options & REPORT_ERRORS != 0 {
            return fail(format!(
                "creating archive \"{}\" disabled by the php.ini setting phar.readonly",
                l(fname)
            ));
        }
        return Err(None);
    }
    let Some(full) = expand_filepath(fname) else {
        return Err(None);
    };
    let mydata = Rc::new(RefCell::new(Archive::default()));
    {
        let mut d = mydata.borrow_mut();
        d.fname = full.clone();
        d.set_ext_from_fname();
        d.version = PHP_PHAR_API_VERSION.to_string();
        d.is_temporary_alias = alias.is_none();
        d.is_writeable = true;
        d.is_brandnew = true;
    }
    request_initialize();
    g(|g| {
        if !g.fname_map.contains_key(&full) {
            g.fname_map.insert(full.clone(), mydata.clone());
        }
    });
    let mut alias = alias;
    if is_data {
        alias = None;
        let mut d = mydata.borrow_mut();
        d.is_data = true;
        d.is_tar = true;
    } else {
        if let Some(a) = alias {
            if let Some(fd) = find_alias(a) {
                if util::free_alias(&fd).is_err() {
                    fname_map_del(&full);
                    return fail(format!(
                        "phar error: phar \"{}\" cannot set alias \"{}\", already in use by another phar archive",
                        l(&full),
                        l(a)
                    ));
                }
            }
        }
        mydata.borrow_mut().alias = Some(alias.map(|a| a.to_vec()).unwrap_or_else(|| full.clone()));
    }
    if let Some(a) = alias.filter(|a| !a.is_empty()) {
        let added = g(|g| {
            if g.alias_map.contains_key(a) {
                false
            } else {
                g.alias_map.insert(a.to_vec(), mydata.clone());
                true
            }
        });
        if !added {
            fname_map_del(&full);
            if options & REPORT_ERRORS != 0 {
                return fail(format!(
                    "archive \"{}\" cannot be associated with alias \"{}\", already in use",
                    l(fname),
                    l(a)
                ));
            }
            return Err(None);
        }
    }
    Ok(mydata)
}

/// phar_open_from_filename()
pub fn open_from_filename(
    vm: &mut Vm,
    fname: &[u8],
    alias: Option<&[u8]>,
    options: i64,
) -> Res<ArcRef> {
    let is_data = memmem(fname, b".phar").is_none();
    match open_parsed_phar(fname, alias, is_data, options) {
        Ok(p) => return Ok(p),
        Err(Some(e)) => return Err(Some(e)),
        Err(None) => {}
    }
    let Some(fp) = read_file(fname) else {
        if options & REPORT_ERRORS != 0 {
            return fail(format!("unable to open phar for reading \"{}\"", l(fname)));
        }
        return Err(None);
    };
    let actual = expand_filepath(fname).unwrap_or_else(|| fname.to_vec());
    open_from_fp(vm, fp, &actual, alias, options)
}

fn php_memnistr(hay: &[u8], needle: &[u8]) -> Option<usize> {
    stristr(hay, needle)
}

/// phar_open_from_fp(): find the __HALT_COMPILER(); token (or detect a
/// compressed, zip or tar archive) and parse the manifest.
pub fn open_from_fp(
    vm: &mut Vm,
    mut fp: Vec<u8>,
    fname: &[u8],
    alias: Option<&[u8]>,
    _options: i64,
) -> Res<ArcRef> {
    const TOKEN: &[u8] = b"__HALT_COMPILER();";
    let f = l(fname);
    let tokenlen = TOKEN.len();
    let window_size = 1024usize;
    let readsize = window_size;
    let mut recursion_count = 3;
    let mut compression = PHAR_FILE_COMPRESSED_NONE;
    'restart: loop {
        let mut buffer = vec![b' '; tokenlen + readsize + 1];
        let mut test = false;
        let mut halt_offset: usize = 0;
        let mut rpos = 0usize;
        // php_stream_eof(): set by the read that came up short
        let mut eof = false;
        while !eof {
            let got = readsize.min(fp.len() - rpos);
            if got < readsize {
                eof = true;
            }
            if got < tokenlen {
                return fail(format!(
                    "internal corruption of phar \"{}\" (truncated entry)",
                    f
                ));
            }
            buffer[tokenlen..tokenlen + got].copy_from_slice(&fp[rpos..rpos + got]);
            rpos += got;
            if !test && recursion_count > 0 {
                test = true;
                let pos = &buffer[tokenlen..tokenlen + got];
                if pos.starts_with(b"\x1f\x8b\x08") {
                    match util::zlib_inflate(&fp, 15 + 32) {
                        Some(d) => fp = d,
                        None => {
                            return fail(format!(
                                "unable to decompress gzipped phar archive \"{}\" to temporary file",
                                f
                            ));
                        }
                    }
                    compression = PHAR_FILE_COMPRESSED_GZ;
                    recursion_count -= 1;
                    if recursion_count == 0 {
                        return fail(format!(
                            "unable to decompress gzipped phar archive \"{}\"",
                            f
                        ));
                    }
                    continue 'restart;
                } else if pos.starts_with(b"BZh") {
                    match util::bz2_decompress(vm, &fp) {
                        Some(d) => fp = d,
                        None => {
                            return fail(format!(
                                "unable to decompress bzipped phar archive \"{}\" to temporary file",
                                f
                            ));
                        }
                    }
                    compression = PHAR_FILE_COMPRESSED_BZ2;
                    recursion_count -= 1;
                    if recursion_count == 0 {
                        return fail(format!(
                            "unable to decompress bzipped phar archive \"{}\"",
                            f
                        ));
                    }
                    continue 'restart;
                }
                if pos.starts_with(b"PK\x03\x04") {
                    return crate::zip::parse_zipfile(vm, fp, fname, alias);
                }
                if got >= 512 && crate::tar::is_tar(pos, fname) {
                    return crate::tar::parse_tarfile(vm, fp, fname, alias, compression);
                }
            }
            let search_end = (tokenlen + got).min(buffer.len());
            if let Some(p) = php_memnistr(&buffer[..search_end], TOKEN) {
                halt_offset += p;
                return parse_pharfile(vm, fp, fname, alias, halt_offset, compression);
            }
            halt_offset += got;
            buffer.copy_within(window_size..window_size + tokenlen, 0);
        }
        return fail(format!(
            "internal corruption of phar \"{}\" (__HALT_COMPILER(); not found)",
            f
        ));
    }
}

/// phar_analyze_path()
fn analyze_path(vm: &mut Vm, fname: &[u8], ext: usize, ext_len: usize, for_create: i32) -> bool {
    let filename = &fname[..ext + ext_len];
    if let Some(rp) = expand_filepath(filename) {
        if find_fname(&rp).is_some() {
            return true;
        }
    }
    match stat_is_dir(vm, filename) {
        Some(is_dir) => {
            if is_dir {
                return false;
            }
            for_create != 1
        }
        None => {
            if for_create == 0 {
                return false;
            }
            let slash = filename.iter().rposition(|&c| c == b'/');
            let dir: &[u8] = match slash {
                Some(s) => &filename[..s],
                None => filename,
            };
            match stat_is_dir(vm, dir) {
                Some(is_dir) => is_dir,
                None => {
                    if slash.is_none() {
                        let Some(realpath) = expand_filepath(dir) else {
                            return false;
                        };
                        let mut rp = realpath.clone();
                        if let Some(at) = memmem(&realpath, dir) {
                            rp.truncate(at + ext + ext_len);
                        }
                        let Some(s) = rp.iter().rposition(|&c| c == b'/') else {
                            return false;
                        };
                        rp.truncate(s);
                        return stat_is_dir(vm, &rp) == Some(true);
                    }
                    false
                }
            }
        }
    }
}

/// phar_check_str()
fn check_str(
    vm: &mut Vm,
    fname: &[u8],
    ext: usize,
    ext_len: usize,
    executable: i32,
    for_create: i32,
) -> bool {
    if ext_len >= 50 {
        return false;
    }
    let ext_str = &fname[ext..];
    // the C string of the extension ends at the end of fname
    let at = |i: usize| -> u8 { ext_str.get(i).copied().unwrap_or(0) };
    if executable == 1 {
        let Some(pos) = memmem(ext_str, b".phar") else {
            return false;
        };
        if (pos != 0 && ext_str[pos - 1] == b'/') || ext_len.wrapping_sub(pos) < 5 {
            return false;
        }
        let c = at(pos + 5);
        if !(c == 0 || c == b'/' || c == b'.') {
            return false;
        }
        return analyze_path(vm, fname, ext, ext_len, for_create);
    }
    if executable == 0 {
        let pos = memmem(ext_str, b".phar");
        let is_phar = match pos {
            Some(p) => {
                // *(pos - 1) reads before ext_str for a leading ".phar"
                let prev = if p == 0 {
                    if ext > 0 { fname[ext - 1] } else { 0 }
                } else {
                    ext_str[p - 1]
                };
                let c = at(p + 5);
                prev != b'/' && (c == 0 || c == b'/' || c == b'.')
            }
            None => false,
        };
        if !is_phar && at(1) != b'.' && at(1) != b'/' && at(1) != 0 {
            return analyze_path(vm, fname, ext, ext_len, for_create);
        }
    } else if at(1) != b'.' && at(1) != b'/' && at(1) != 0 {
        return analyze_path(vm, fname, ext, ext_len, for_create);
    }
    false
}

/// phar_detect_phar_fname_ext(): `ext_str` is an offset into `filename`;
/// `ext_len` -1 means "alias", -2 "URL".
pub fn detect_phar_fname_ext(
    vm: &mut Vm,
    filename: &[u8],
    ext_str: &mut Option<usize>,
    ext_len: &mut i64,
    executable: i32,
    for_create: i32,
    is_complete: bool,
) -> bool {
    *ext_str = None;
    *ext_len = 0;
    let flen = filename.len();
    if flen <= 1 {
        return false;
    }
    request_initialize();
    if let Some(pos) = filename.iter().position(|&c| c == b'/') {
        if pos != 0 {
            if filename[pos - 1] == b':' && pos < flen - 1 && filename[pos + 1] == b'/' {
                *ext_len = -2;
                *ext_str = None;
                return false;
            }
            if find_alias(&filename[..pos]).is_some() {
                *ext_str = Some(pos);
                *ext_len = -1;
                return false;
            }
        }
    }
    let have = g(|g| !g.fname_map.is_empty());
    if have {
        let woohoo = |pphar: &ArcRef, ext_str: &mut Option<usize>, ext_len: &mut i64, at: usize| {
            let p = pphar.borrow();
            *ext_str = Some(at - p.ext_len);
            *ext_len = p.ext_len as i64;
            if executable == 2 {
                return true;
            }
            if executable == 1 && !p.is_data {
                return true;
            }
            if executable == 0 && p.is_data {
                return true;
            }
            false
        };
        if is_complete {
            if let Some(pphar) = find_fname(filename) {
                return woohoo(&pphar, ext_str, ext_len, flen);
            }
        } else {
            let list: Vec<(Vec<u8>, ArcRef)> = g(|g| {
                g.fname_map
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect()
            });
            for (key, pphar) in list {
                if key.len() > flen {
                    continue;
                }
                if filename.starts_with(&key)
                    && (flen == key.len()
                        || filename[key.len()] == b'/'
                        || filename[key.len()] == 0)
                {
                    return woohoo(&pphar, ext_str, ext_len, key.len());
                }
            }
        }
    }
    // memchr(filename + 1, '.', filename_len)
    let find_dot = |from: usize| -> Option<usize> {
        filename
            .get(from..)
            .and_then(|s| s.iter().position(|&c| c == b'.'))
            .map(|i| i + from)
    };
    let mut pos = find_dot(1);
    loop {
        let Some(mut p) = pos else {
            return false;
        };
        while p != 0 && (filename[p - 1] == b'/' || filename[p - 1] == 0) {
            match find_dot(p + 1) {
                Some(n) => p = n,
                None => return false,
            }
        }
        let slash = filename[p..].iter().position(|&c| c == b'/').map(|i| i + p);
        match slash {
            None => {
                *ext_str = Some(p);
                // strlen(pos)
                let rest = &filename[p..];
                *ext_len = rest.iter().position(|&c| c == 0).unwrap_or(rest.len()) as i64;
                return check_str(vm, filename, p, *ext_len as usize, executable, for_create);
            }
            Some(s) => {
                *ext_str = Some(p);
                *ext_len = (s - p) as i64;
                if check_str(vm, filename, p, *ext_len as usize, executable, for_create) {
                    return true;
                }
                // strchr(pos + 1, '.')
                pos = find_dot(p + 1);
                if pos.is_some() {
                    *ext_str = None;
                    *ext_len = 0;
                    continue;
                }
                return false;
            }
        }
    }
}

/// IS_DIRECTORY_UP: two or more dots only
fn is_directory_up(e: &[u8]) -> bool {
    e.len() >= 2 && e.iter().all(|&c| c == b'.')
}

/// phar_fix_filepath(): remove "." and ".." references.
pub fn fix_filepath(path: Vec<u8>, use_cwd: bool) -> Vec<u8> {
    let path_length = path.len();
    let cwd = g(|g| g.cwd.clone());
    let cwd_len = cwd.as_ref().map(|c| c.len()).unwrap_or(0);
    let mut newpath: Vec<u8>;
    if cwd_len > 0 && use_cwd && path_length > 2 && path[0] == b'.' && path[1] == b'/' {
        newpath = cwd.unwrap();
    } else {
        newpath = vec![b'/'];
    }
    let mut ptr = 0usize;
    if path.first() == Some(&b'/') {
        ptr += 1;
    }
    let mut tok = ptr;
    let find_slash = |from: usize| -> Option<usize> {
        path.get(from..path_length)
            .and_then(|s| s.iter().position(|&c| c == b'/'))
            .map(|i| i + from)
    };
    // skip empty segments
    let next = |ptr: usize, tok: &mut usize| -> Option<usize> {
        let mut p = ptr;
        loop {
            let found = find_slash(p)?;
            if found == *tok {
                p = found + 1;
                *tok += 1;
                continue;
            }
            return Some(found);
        }
    };
    let mut cur = next(ptr, &mut tok);
    if cur.is_none() && path_length > tok {
        match path_length - tok {
            1 if path[tok] == b'.' => return b"/".to_vec(),
            2 if path[tok] == b'.' && path[tok + 1] == b'.' => return b"/".to_vec(),
            _ => {}
        }
        return path;
    }
    let mut newpath_len = newpath.len();
    while let Some(mut p) = cur {
        let mut ptr_length = p - tok;
        loop {
            let seg = &path[tok..tok + ptr_length];
            if is_directory_up(seg) {
                while newpath_len > 1 && newpath[newpath_len - 1] != b'/' {
                    newpath_len -= 1;
                }
                if newpath[0] != b'/' {
                    newpath.truncate(newpath_len);
                } else if newpath_len > 1 {
                    newpath_len -= 1;
                }
                newpath.truncate(newpath_len);
            } else if !(ptr_length == 1 && seg[0] == b'.') {
                newpath.truncate(newpath_len);
                if newpath_len > 1 {
                    newpath.push(b'/');
                }
                newpath.extend_from_slice(seg);
                newpath_len = newpath.len();
            }
            if p == path_length {
                return newpath[..newpath_len].to_vec();
            }
            tok = p + 1;
            match next(tok, &mut tok) {
                Some(n) => {
                    cur = Some(n);
                    break;
                }
                None => {
                    if path_length > tok {
                        ptr_length = path_length - tok;
                        p = path_length;
                        continue;
                    }
                    cur = None;
                    break;
                }
            }
        }
        if cur.is_some() {
            continue;
        }
    }
    newpath.truncate(newpath_len);
    newpath
}

/// phar_split_fname(): (arch, entry). On failure, `arch_on_fail` gets the
/// filename when no extension was detected.
pub fn split_fname(
    vm: &mut Vm,
    filename: &[u8],
    executable: i32,
    for_create: i32,
) -> Result<(Vec<u8>, Vec<u8>), Option<Vec<u8>>> {
    if filename.contains(&0) {
        return Err(None);
    }
    let filename = if starts_with_ci(filename, b"phar://") {
        &filename[7..]
    } else {
        filename
    };
    let mut ext_str = None;
    let mut ext_len = 0i64;
    if !detect_phar_fname_ext(
        vm,
        filename,
        &mut ext_str,
        &mut ext_len,
        executable,
        for_create,
        false,
    ) {
        if ext_len != -1 {
            if ext_str.is_none() {
                return Err(Some(filename.to_vec()));
            }
            return Err(None);
        }
        ext_len = 0;
    }
    let es = ext_str.unwrap_or(0);
    let arch_len = es + ext_len as usize;
    let arch = filename[..arch_len].to_vec();
    let entry = if arch_len < filename.len() {
        fix_filepath(filename[arch_len..].to_vec(), false)
    } else {
        b"/".to_vec()
    };
    Ok((arch, entry))
}

/// zend_get_executed_filename_ex()
pub fn executed_filename(vm: &Vm) -> Option<Vec<u8>> {
    if vm.current_file.is_empty() {
        return None;
    }
    Some(vm.current_file.as_bytes().to_vec())
}

/// phar_open_executed_filename()
pub fn open_executed_filename(vm: &mut Vm, alias: Option<&[u8]>) -> Res<()> {
    let Some(fname) = executed_filename(vm) else {
        return fail("cannot initialize a phar outside of PHP execution".to_string());
    };
    if open_parsed_phar(&fname, alias, false, REPORT_ERRORS).is_ok() {
        return Ok(());
    }
    let Some(fp) = read_file(&fname) else {
        return fail(format!("unable to open phar for reading \"{}\"", l(&fname)));
    };
    if stristr(&fp, b"__HALT_COMPILER").is_none() {
        return fail("__HALT_COMPILER(); must be declared in a phar".to_string());
    }
    let actual = expand_filepath(&fname).unwrap_or(fname);
    open_from_fp(vm, fp, &actual, alias, REPORT_ERRORS).map(|_| ())
}

/// phar_postprocess_file(): check the zip local header (process_zip > 0)
/// and the crc32 (process_zip < 1).
pub fn postprocess_file(
    vm: &mut Vm,
    phar: &ArcRef,
    entry: &EntryRef,
    fp: &Buf,
    zero: &mut i64,
    crc32: u32,
    process_zip: i32,
) -> Res<()> {
    let is_zip = entry.borrow().is_zip;
    if is_zip && process_zip > 0 {
        let pfname = l(&phar.borrow().fname);
        let ename = l(&entry.borrow().filename);
        if util::open_archive_fp(vm, phar).is_err() {
            return fail(format!(
                "phar error: unable to open zip-based phar archive \"{}\" to verify local file header for file \"{}\"",
                pfname, ename
            ));
        }
        let afp = phar.borrow().fp.clone().unwrap();
        let data = afp.borrow();
        let hoff = entry.borrow().header_offset as usize;
        let Some(local) = rd(&data, hoff, 30) else {
            return fail(format!(
                "phar error: internal corruption of zip-based phar \"{}\" (cannot read local file header for file \"{}\")",
                pfname, ename
            ));
        };
        let g16 = |b: &[u8], o: usize| u16::from_le_bytes([b[o], b[o + 1]]) as usize;
        let g32 = |b: &[u8], o: usize| u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]);
        let flags = g16(local, 6);
        let fnlen = g16(local, 26);
        let extra = g16(local, 28);
        let mut crc = g32(local, 14);
        let mut compsize = g32(local, 18);
        let mut uncompsize = g32(local, 22);
        let csize = entry.borrow().compressed_filesize as usize;
        if flags & 0x8 == 0x8 {
            let Some(desc) = rd(&data, hoff + 30 + fnlen + extra + csize, 16) else {
                return fail(format!(
                    "phar error: internal corruption of zip-based phar \"{}\" (cannot read local data descriptor for file \"{}\")",
                    pfname, ename
                ));
            };
            if desc[0] == b'P' && desc[1] == b'K' {
                crc = g32(desc, 4);
                compsize = g32(desc, 8);
                uncompsize = g32(desc, 12);
            } else {
                crc = g32(desc, 0);
                compsize = g32(desc, 4);
                uncompsize = g32(desc, 8);
            }
        }
        {
            let e = entry.borrow();
            if e.filename.len() != fnlen
                || e.crc32 != crc
                || e.uncompressed_filesize != uncompsize
                || e.compressed_filesize != compsize
            {
                return fail(format!(
                    "phar error: internal corruption of zip-based phar \"{}\" (local header of file \"{}\" does not match central directory)",
                    pfname, ename
                ));
            }
        }
        let off = (30 + hoff + fnlen + extra) as i64;
        {
            let mut e = entry.borrow_mut();
            e.offset = off;
            e.offset_abs = off;
        }
        if *zero != 0 && *zero != off {
            *zero = off;
        }
    }
    if process_zip == 1 {
        return Ok(());
    }
    let size = entry.borrow().uncompressed_filesize as usize;
    let data = fp.borrow();
    let z = (*zero).max(0) as usize;
    let ok = z + size <= data.len() && goro_zlib::crc32(0, &data[z..z + size]) == crc32;
    if ok {
        entry.borrow_mut().is_crc_checked = true;
        Ok(())
    } else {
        fail(format!(
            "phar error: internal corruption of phar \"{}\" (crc32 mismatch on file \"{}\")",
            l(&phar.borrow().fname),
            l(&entry.borrow().filename)
        ))
    }
}

/// phar_create_default_stub()
pub fn create_default_stub(index_php: Option<&[u8]>, web_index: Option<&[u8]>) -> Res<Vec<u8>> {
    use crate::stub::*;
    let index_php = index_php.unwrap_or(b"index.php");
    let web_index = web_index.unwrap_or(b"index.php");
    if index_php.len() > 400 {
        return fail(format!(
            "Illegal filename passed in for stub creation, was {} characters long, and only 400 or less is allowed",
            index_php.len()
        ));
    }
    if web_index.len() > 400 {
        return fail(format!(
            "Illegal web filename passed in for stub creation, was {} characters long, and only 400 or less is allowed",
            web_index.len()
        ));
    }
    let name_len = index_php.len() + 1;
    let web_len = web_index.len() + 1;
    let mut out = Vec::with_capacity(7000);
    out.extend_from_slice(NEWSTUB0.as_bytes());
    out.extend_from_slice(web_index);
    out.extend_from_slice(NEWSTUB1_0.as_bytes());
    out.extend_from_slice(NEWSTUB1_1.as_bytes());
    out.extend_from_slice(index_php);
    out.extend_from_slice(NEWSTUB2.as_bytes());
    out.extend_from_slice((name_len + web_len + NEWSTUB_LEN).to_string().as_bytes());
    out.extend_from_slice(NEWSTUB3_0.as_bytes());
    out.extend_from_slice(NEWSTUB3_1.as_bytes());
    Ok(out)
}

/// phar_flush()
pub fn flush(vm: &mut Vm, phar: &ArcRef) -> PErr {
    flush_ex(vm, phar, None, false)
}

fn put32(b: &mut Vec<u8>, v: u32) {
    b.extend_from_slice(&v.to_le_bytes());
}

/// phar_flush_ex(): save the archive to disk. Returns the error, if any.
pub fn flush_ex(
    vm: &mut Vm,
    phar: &ArcRef,
    user_stub: Option<&[u8]>,
    is_default_stub: bool,
) -> PErr {
    if phar.borrow().manifest.is_empty() && user_stub.is_none() {
        return None;
    }
    phar.borrow_mut().virtual_dirs.clear();
    let (is_zip, is_tar) = {
        let p = phar.borrow();
        (p.is_zip, p.is_tar)
    };
    if is_zip {
        return crate::zip::zip_flush(vm, phar, user_stub, is_default_stub);
    }
    if is_tar {
        return crate::tar::tar_flush(vm, phar, user_stub, is_default_stub);
    }
    if readonly() {
        return None;
    }
    let fname = phar.borrow().fname.clone();
    let f = l(&fname);
    let oldfile: Option<Buf> = {
        let p = phar.borrow();
        if p.fp.is_some() && !p.is_brandnew {
            p.fp.clone()
        } else {
            None
        }
    };
    let oldfile = oldfile.map(|b| fp_current(&phar.borrow(), b));
    let oldfile = oldfile.or_else(|| read_file(&fname).map(|v| Rc::new(RefCell::new(v))));
    let mut newfile: Vec<u8> = Vec::new();
    if let Some(stub) = user_stub {
        let Some(pos) = stristr(stub, b"__HALT_COMPILER();") else {
            return Some(format!(
                "illegal stub for phar \"{}\" (__HALT_COMPILER(); is missing)",
                f
            ));
        };
        let len = pos + "__HALT_COMPILER();".len();
        newfile.extend_from_slice(&stub[..len]);
        newfile.extend_from_slice(b" ?>\r\n");
        phar.borrow_mut().halt_offset = len + 5;
    } else {
        let (halt, brandnew) = {
            let p = phar.borrow();
            (p.halt_offset, p.is_brandnew)
        };
        if let (true, Some(oldfile)) = (halt != 0 && !brandnew, oldfile.as_ref()) {
            let old = oldfile.borrow();
            let n = halt.min(old.len());
            newfile.extend_from_slice(&old[..n]);
            if n != halt {
                return Some(format!(
                    "unable to copy stub of old phar to new phar \"{}\"",
                    f
                ));
            }
        } else {
            let stub = create_default_stub(None, None).unwrap_or_default();
            phar.borrow_mut().halt_offset = stub.len();
            newfile.extend_from_slice(&stub);
        }
    }
    let halt_offset = newfile.len();
    // clean up deleted entries no longer in use
    phar.borrow_mut().manifest.retain(|_, e| {
        let e = e.borrow();
        !(e.fp_refcount <= 0 && e.is_deleted)
    });
    // compress as necessary, calculate crcs, serialize metadata, manifest
    // size and file sizes
    let main_metadata: Option<Vec<u8>> = {
        let meta = phar.borrow().metadata.clone();
        if let Some(s) = meta.str {
            Some(s)
        } else if let Some(v) = meta.val {
            goro_ext_standard::serialize::php_serialize(vm, &v)
        } else {
            None
        }
    };
    let mut new_manifest_count = 0u32;
    let mut offset: usize = 0;
    let mut has_dirs = false;
    let mut global_flags = 0u32;
    let entries: Vec<EntryRef> = phar.borrow().manifest.values().cloned().collect();
    // compressed contents (cfp) of each entry
    let mut cfps: Vec<Option<Vec<u8>>> = vec![None; entries.len()];
    for (idx, er) in entries.iter().enumerate() {
        er.borrow_mut().cfp = None;
        let (deleted, mounted) = {
            let e = er.borrow();
            (e.is_deleted, e.is_mounted)
        };
        if deleted || mounted {
            continue;
        }
        new_manifest_count += 1;
        {
            let name = er.borrow().filename.clone();
            util::add_virtual_dirs(&mut phar.borrow_mut(), &name);
        }
        if er.borrow().is_dir {
            has_dirs = true;
        }
        let need_ser = {
            let e = er.borrow();
            e.metadata.val.is_some() && e.metadata.str.is_none()
        };
        if need_ser {
            let v = er.borrow().metadata.val.clone().unwrap();
            let s = goro_ext_standard::serialize::php_serialize(vm, &v);
            er.borrow_mut().metadata.str = s;
        }
        {
            let e = er.borrow();
            offset += 4
                + e.filename.len()
                + 24
                + e.metadata.str.as_ref().map(|s| s.len()).unwrap_or(0)
                + usize::from(e.is_dir);
        }
        let (modified, is_dir) = {
            let e = er.borrow();
            (e.is_modified, e.is_dir)
        };
        if (oldfile.is_some() && !modified) || is_dir {
            let mut e = er.borrow_mut();
            if e.fp_type == FpType::Ufp {
                e.fp_type = FpType::Fp;
            }
            continue;
        }
        if util::get_efp(vm, er, false).is_none() {
            if util::open_jit(vm, phar, er).is_err() {
                continue;
            }
        }
        let content = match util::read_entry_contents(vm, er, false) {
            Some(c) => c,
            None => {
                return Some(format!(
                    "unable to seek to start of file \"{}\" while creating new phar \"{}\"",
                    l(&er.borrow().filename),
                    f
                ));
            }
        };
        {
            let mut e = er.borrow_mut();
            e.crc32 = goro_zlib::crc32(0, &content);
            e.is_crc_checked = true;
        }
        let eflags = er.borrow().flags;
        if eflags & PHAR_ENT_COMPRESSION_MASK == 0 {
            let mut e = er.borrow_mut();
            e.compressed_filesize = e.uncompressed_filesize;
            continue;
        }
        let compressed = match util::compress(vm, eflags, &content) {
            Some(c) => c,
            None => {
                return Some(if eflags & PHAR_ENT_COMPRESSED_GZ != 0 {
                    format!(
                        "unable to gzip compress file \"{}\" to new phar \"{}\"",
                        l(&er.borrow().filename),
                        f
                    )
                } else {
                    format!(
                        "unable to bzip2 compress file \"{}\" to new phar \"{}\"",
                        l(&er.borrow().filename),
                        f
                    )
                });
            }
        };
        {
            let mut e = er.borrow_mut();
            e.compressed_filesize = compressed.len() as u32;
            e.old_flags = e.flags;
            e.is_modified = true;
        }
        cfps[idx] = Some(compressed);
        global_flags |= eflags & PHAR_ENT_COMPRESSION_MASK;
    }
    global_flags |= PHAR_HDR_SIGNATURE;

    // manifest pre-header
    let (alias, is_temp_alias) = {
        let p = phar.borrow();
        (p.alias.clone().unwrap_or_default(), p.is_temporary_alias)
    };
    let alias_len = if is_temp_alias { 0 } else { alias.len() };
    let mut manifest_len =
        (offset + alias_len + 18 + main_metadata.as_ref().map(|m| m.len()).unwrap_or(0)) as u32;
    let mut manifest_hack = false;
    let b0 = manifest_len.to_le_bytes()[0];
    if b0 == b'\r' || b0 == b'\n' {
        manifest_len += 1;
        manifest_hack = true;
    }
    let mut manifest = Vec::with_capacity(18);
    put32(&mut manifest, manifest_len);
    put32(&mut manifest, new_manifest_count);
    let ver = if has_dirs {
        PHAR_API_VERSION
    } else {
        PHAR_API_VERSION_NODIR
    };
    manifest.push(((ver >> 8) & 0xFF) as u8);
    manifest.push((ver & 0xF0) as u8);
    put32(&mut manifest, global_flags);
    put32(&mut manifest, alias_len as u32);
    newfile.extend_from_slice(&manifest);
    newfile.extend_from_slice(&alias[..alias_len]);
    put32(
        &mut newfile,
        main_metadata.as_ref().map(|m| m.len() as u32).unwrap_or(0),
    );
    if let Some(m) = &main_metadata {
        newfile.extend_from_slice(m);
    }
    // the manifest
    for er in &entries {
        let e = er.borrow();
        if e.is_deleted || e.is_mounted {
            continue;
        }
        put32(
            &mut newfile,
            (e.filename.len() + usize::from(e.is_dir)) as u32,
        );
        newfile.extend_from_slice(&e.filename);
        if e.is_dir {
            newfile.push(b'/');
        }
        put32(&mut newfile, e.uncompressed_filesize);
        put32(&mut newfile, now());
        put32(&mut newfile, e.compressed_filesize);
        put32(&mut newfile, e.crc32);
        put32(&mut newfile, e.flags);
        put32(
            &mut newfile,
            e.metadata.str.as_ref().map(|s| s.len() as u32).unwrap_or(0),
        );
        if let Some(s) = &e.metadata.str {
            newfile.extend_from_slice(s);
        }
    }
    if manifest_hack {
        newfile.push(manifest[0]);
    }
    // file contents
    for (idx, er) in entries.iter().enumerate() {
        let (skip, _) = {
            let e = er.borrow();
            (e.is_deleted || e.is_dir || e.is_mounted, ())
        };
        if skip {
            continue;
        }
        let data = match cfps[idx].take() {
            Some(c) => c,
            None => match util::read_entry_raw(vm, er, oldfile.as_ref()) {
                Some(d) => d,
                None => {
                    return Some(format!(
                        "unable to seek to start of file \"{}\" while creating new phar \"{}\"",
                        l(&er.borrow().filename),
                        f
                    ));
                }
            },
        };
        let off = newfile.len() as i64;
        newfile.extend_from_slice(&data);
        let mut e = er.borrow_mut();
        e.offset = off;
        e.offset_abs = off;
        e.is_modified = false;
        if e.fp_type == FpType::Mod {
            e.fp = None;
            e.fp_type = FpType::Fp;
        } else if e.fp_type == FpType::Ufp {
            e.fp_type = FpType::Fp;
        }
    }
    // signature
    if global_flags & PHAR_HDR_SIGNATURE != 0 {
        phar.borrow_mut().signature = None;
        let digest = match util::create_signature(vm, phar, &newfile) {
            Ok(d) => d,
            Err(e) => {
                return Some(format!(
                    "phar error: unable to write signature: {}",
                    e.unwrap_or_default()
                ));
            }
        };
        newfile.extend_from_slice(&digest);
        let sig_flags = phar.borrow().sig_flags;
        if matches!(
            sig_flags,
            PHAR_SIG_OPENSSL | PHAR_SIG_OPENSSL_SHA256 | PHAR_SIG_OPENSSL_SHA512
        ) {
            put32(&mut newfile, digest.len() as u32);
        }
        put32(&mut newfile, sig_flags);
        newfile.extend_from_slice(b"GBMB");
    }
    {
        let mut p = phar.borrow_mut();
        p.ufp = None;
        p.halt_offset = halt_offset;
        p.is_brandnew = false;
    }
    let donotflush = phar.borrow().donotflush;
    let newbuf = Rc::new(RefCell::new(newfile));
    if !donotflush {
        let flags = phar.borrow().flags;
        let out = if flags & PHAR_FILE_COMPRESSED_GZ != 0 {
            util::zlib_deflate(&newbuf.borrow(), 15 + 16)
        } else if flags & PHAR_FILE_COMPRESSED_BZ2 != 0 {
            util::bz2_compress(vm, &newbuf.borrow())
        } else {
            Some(newbuf.borrow().clone())
        };
        let out = out.unwrap_or_default();
        if std::fs::write(l(&fname), &out).is_err() {
            phar.borrow_mut().fp = Some(newbuf);
            phar.borrow_mut().fp_ino = None;
            let msg = format!("unable to open new phar \"{}\" for writing", f);
            vm.emit_warning(&format!(
                "{}({}): Failed to open stream: {}",
                vm.current_builtin_name().unwrap_or_default(),
                f,
                std::io::Error::last_os_error()
                    .to_string()
                    .split(" (os error")
                    .next()
                    .unwrap_or("")
            ));
            return Some(msg);
        }
    }
    let mut p = phar.borrow_mut();
    p.fp = Some(newbuf);
    // fp is the file's contents again, unless the archive is buffering
    // (the new contents are not written yet)
    p.fp_ino = if donotflush || flags_compressed(p.flags) {
        None
    } else {
        file_ino(&fname)
    };
    None
}

fn flags_compressed(flags: u32) -> bool {
    flags & (PHAR_FILE_COMPRESSED_GZ | PHAR_FILE_COMPRESSED_BZ2) != 0
}
