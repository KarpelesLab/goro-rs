//! libmagic's file.h / magic.h: the magic entry, the magic set and the
//! buffers being typed.

use std::cell::RefCell;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::rc::Rc;

// ---------------------------------------------------------------------------
// magic.h flags

pub const MAGIC_NONE: i32 = 0;
pub const MAGIC_SYMLINK: i32 = 0x2;
pub const MAGIC_DEVICES: i32 = 0x8;
pub const MAGIC_MIME_TYPE: i32 = 0x10;
pub const MAGIC_CONTINUE: i32 = 0x20;
pub const MAGIC_CHECK: i32 = 0x40;
pub const MAGIC_PRESERVE_ATIME: i32 = 0x80;
pub const MAGIC_RAW: i32 = 0x100;
pub const MAGIC_MIME_ENCODING: i32 = 0x400;
pub const MAGIC_MIME: i32 = MAGIC_MIME_TYPE | MAGIC_MIME_ENCODING;
pub const MAGIC_APPLE: i32 = 0x800;
pub const MAGIC_EXTENSION: i32 = 0x1000000;
pub const MAGIC_NODESC: i32 = MAGIC_EXTENSION | MAGIC_MIME | MAGIC_APPLE;
pub const MAGIC_NO_CHECK_TAR: i32 = 0x2000;
pub const MAGIC_NO_CHECK_SOFT: i32 = 0x4000;
pub const MAGIC_NO_CHECK_TEXT: i32 = 0x20000;
pub const MAGIC_NO_CHECK_CDF: i32 = 0x40000;
pub const MAGIC_NO_CHECK_CSV: i32 = 0x80000;
pub const MAGIC_NO_CHECK_ENCODING: i32 = 0x200000;
pub const MAGIC_NO_CHECK_JSON: i32 = 0x400000;
pub const MAGIC_NO_CHECK_SIMH: i32 = 0x800000;

pub const MAGIC_VERSION: i32 = 546;

// ---------------------------------------------------------------------------
// file.h

pub const FILE_BADSIZE: usize = usize::MAX;
pub const MAXSTRING: usize = 128;
pub const MAGICNO: u32 = 0xF11E041C;
pub const VERSIONNO: u32 = 19;
pub const FILE_MAGICSIZE: usize = 432;
pub const MAX_CTIME: i64 = 0x3afff487cf;

pub const INDIR: u8 = 0x01;
pub const OFFADD: u8 = 0x02;
pub const INDIROFFADD: u8 = 0x04;
pub const UNSIGNED: u8 = 0x08;
pub const NOSPACE: u8 = 0x10;
pub const BINTEST: u8 = 0x20;
pub const TEXTTEST: u8 = 0x40;
pub const OFFNEGATIVE: u8 = 0x80;

pub const FILE_INVALID: u8 = 0;
pub const FILE_BYTE: u8 = 1;
pub const FILE_SHORT: u8 = 2;
pub const FILE_DEFAULT: u8 = 3;
pub const FILE_LONG: u8 = 4;
pub const FILE_STRING: u8 = 5;
pub const FILE_DATE: u8 = 6;
pub const FILE_BESHORT: u8 = 7;
pub const FILE_BELONG: u8 = 8;
pub const FILE_BEDATE: u8 = 9;
pub const FILE_LESHORT: u8 = 10;
pub const FILE_LELONG: u8 = 11;
pub const FILE_LEDATE: u8 = 12;
pub const FILE_PSTRING: u8 = 13;
pub const FILE_LDATE: u8 = 14;
pub const FILE_BELDATE: u8 = 15;
pub const FILE_LELDATE: u8 = 16;
pub const FILE_REGEX: u8 = 17;
pub const FILE_BESTRING16: u8 = 18;
pub const FILE_LESTRING16: u8 = 19;
pub const FILE_SEARCH: u8 = 20;
pub const FILE_MEDATE: u8 = 21;
pub const FILE_MELDATE: u8 = 22;
pub const FILE_MELONG: u8 = 23;
pub const FILE_QUAD: u8 = 24;
pub const FILE_LEQUAD: u8 = 25;
pub const FILE_BEQUAD: u8 = 26;
pub const FILE_QDATE: u8 = 27;
pub const FILE_LEQDATE: u8 = 28;
pub const FILE_BEQDATE: u8 = 29;
pub const FILE_QLDATE: u8 = 30;
pub const FILE_LEQLDATE: u8 = 31;
pub const FILE_BEQLDATE: u8 = 32;
pub const FILE_FLOAT: u8 = 33;
pub const FILE_BEFLOAT: u8 = 34;
pub const FILE_LEFLOAT: u8 = 35;
pub const FILE_DOUBLE: u8 = 36;
pub const FILE_BEDOUBLE: u8 = 37;
pub const FILE_LEDOUBLE: u8 = 38;
pub const FILE_BEID3: u8 = 39;
pub const FILE_LEID3: u8 = 40;
pub const FILE_INDIRECT: u8 = 41;
pub const FILE_QWDATE: u8 = 42;
pub const FILE_LEQWDATE: u8 = 43;
pub const FILE_BEQWDATE: u8 = 44;
pub const FILE_NAME: u8 = 45;
pub const FILE_USE: u8 = 46;
pub const FILE_CLEAR: u8 = 47;
pub const FILE_DER: u8 = 48;
pub const FILE_GUID: u8 = 49;
pub const FILE_OFFSET: u8 = 50;
pub const FILE_BEVARINT: u8 = 51;
pub const FILE_LEVARINT: u8 = 52;
pub const FILE_MSDOSDATE: u8 = 53;
pub const FILE_LEMSDOSDATE: u8 = 54;
pub const FILE_BEMSDOSDATE: u8 = 55;
pub const FILE_MSDOSTIME: u8 = 56;
pub const FILE_LEMSDOSTIME: u8 = 57;
pub const FILE_BEMSDOSTIME: u8 = 58;
pub const FILE_OCTAL: u8 = 59;

pub fn is_libmagic_string(t: u8) -> bool {
    matches!(
        t,
        FILE_STRING
            | FILE_PSTRING
            | FILE_BESTRING16
            | FILE_LESTRING16
            | FILE_REGEX
            | FILE_SEARCH
            | FILE_INDIRECT
            | FILE_NAME
            | FILE_USE
            | FILE_OCTAL
    )
}

pub const FILE_FMT_NONE: u8 = 0;
pub const FILE_FMT_NUM: u8 = 1;
pub const FILE_FMT_STR: u8 = 2;
pub const FILE_FMT_QUAD: u8 = 3;
pub const FILE_FMT_FLOAT: u8 = 4;
pub const FILE_FMT_DOUBLE: u8 = 5;

pub const FILE_FACTOR_OP_PLUS: u8 = b'+';
pub const FILE_FACTOR_OP_MINUS: u8 = b'-';
pub const FILE_FACTOR_OP_TIMES: u8 = b'*';
pub const FILE_FACTOR_OP_DIV: u8 = b'/';
pub const FILE_FACTOR_OP_NONE: u8 = 0;

pub const FILE_OPAND: u8 = 0;
pub const FILE_OPOR: u8 = 1;
pub const FILE_OPXOR: u8 = 2;
pub const FILE_OPADD: u8 = 3;
pub const FILE_OPMINUS: u8 = 4;
pub const FILE_OPMULTIPLY: u8 = 5;
pub const FILE_OPDIVIDE: u8 = 6;
pub const FILE_OPMODULO: u8 = 7;
pub const FILE_OPS_MASK: u8 = 0x07;
pub const FILE_OPSIGNED: u8 = 0x20;
pub const FILE_OPINVERSE: u8 = 0x40;
pub const FILE_OPINDIRECT: u8 = 0x80;

pub const COND_NONE: u8 = 0;
pub const COND_IF: u8 = 1;
pub const COND_ELIF: u8 = 2;
pub const COND_ELSE: u8 = 3;

pub const STRING_COMPACT_WHITESPACE: u32 = 1 << 0;
pub const STRING_COMPACT_OPTIONAL_WHITESPACE: u32 = 1 << 1;
pub const STRING_IGNORE_LOWERCASE: u32 = 1 << 2;
pub const STRING_IGNORE_UPPERCASE: u32 = 1 << 3;
pub const REGEX_OFFSET_START: u32 = 1 << 4;
pub const STRING_TEXTTEST: u32 = 1 << 5;
pub const STRING_BINTEST: u32 = 1 << 6;
pub const PSTRING_1_LE: u32 = 1 << 7;
pub const PSTRING_2_BE: u32 = 1 << 8;
pub const PSTRING_2_LE: u32 = 1 << 9;
pub const PSTRING_4_BE: u32 = 1 << 10;
pub const PSTRING_4_LE: u32 = 1 << 11;
pub const REGEX_LINE_COUNT: u32 = 1 << 11;
pub const PSTRING_LEN: u32 =
    PSTRING_1_LE | PSTRING_2_LE | PSTRING_2_BE | PSTRING_4_LE | PSTRING_4_BE;
pub const PSTRING_LENGTH_INCLUDES_ITSELF: u32 = 1 << 12;
pub const STRING_TRIM: u32 = 1 << 13;
pub const STRING_FULL_WORD: u32 = 1 << 14;
pub const STRING_IGNORE_CASE: u32 = STRING_IGNORE_LOWERCASE | STRING_IGNORE_UPPERCASE;
pub const STRING_DEFAULT_RANGE: u32 = 100;
pub const INDIRECT_RELATIVE: u32 = 1 << 0;

pub const EVENT_HAD_ERR: i32 = 0x01;

pub const FILE_INDIR_MAX: u16 = 50;
pub const FILE_NAME_MAX: u16 = 100;
pub const FILE_REGEX_MAX: u16 = 8192;
pub const FILE_BYTES_MAX: usize = 7 * 1024 * 1024;
pub const FILE_ENCODING_MAX: usize = 64 * 1024;
pub const FILE_MAGWARN_MAX: u16 = 64;

pub const S_IFMT: u32 = 0o170000;
pub const S_IFREG: u32 = 0o100000;
pub const S_IFDIR: u32 = 0o040000;

// ---------------------------------------------------------------------------
// struct magic

/// struct magic (FILE_MAGICSIZE bytes in a compiled database)
#[derive(Clone)]
pub struct Magic {
    pub cont_level: u16,
    pub flag: u8,
    pub factor: u8,
    pub reln: u8,
    pub vallen: u8,
    pub typ: u8,
    pub in_type: u8,
    pub in_op: u8,
    pub mask_op: u8,
    pub cond: u8,
    pub factor_op: u8,
    pub offset: i32,
    pub in_offset: i32,
    pub lineno: u32,
    /// num_mask, or str_range (low half) and str_flags (high half)
    pub u: u64,
    pub value: [u8; MAXSTRING],
    pub desc: [u8; 64],
    pub mimetype: [u8; 80],
    pub apple: [u8; 8],
    pub ext: [u8; 120],
}

impl Default for Magic {
    fn default() -> Self {
        Magic {
            cont_level: 0,
            flag: 0,
            factor: 0,
            reln: 0,
            vallen: 0,
            typ: 0,
            in_type: 0,
            in_op: 0,
            mask_op: 0,
            cond: 0,
            factor_op: 0,
            offset: 0,
            in_offset: 0,
            lineno: 0,
            u: 0,
            value: [0; MAXSTRING],
            desc: [0; 64],
            mimetype: [0; 80],
            apple: [0; 8],
            ext: [0; 120],
        }
    }
}

fn rd16(b: &[u8], swap: bool) -> u16 {
    let v = u16::from_le_bytes([b[0], b[1]]);
    if swap { v.swap_bytes() } else { v }
}
fn rd32(b: &[u8], swap: bool) -> u32 {
    let v = u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    if swap { v.swap_bytes() } else { v }
}
fn rd64(b: &[u8]) -> u64 {
    u64::from_le_bytes(b[..8].try_into().unwrap())
}

impl Magic {
    pub fn num_mask(&self) -> u64 {
        self.u
    }
    pub fn set_num_mask(&mut self, v: u64) {
        self.u = v;
    }
    pub fn str_range(&self) -> u32 {
        self.u as u32
    }
    pub fn set_str_range(&mut self, v: u32) {
        self.u = (self.u & 0xffff_ffff_0000_0000) | v as u64;
    }
    pub fn str_flags(&self) -> u32 {
        (self.u >> 32) as u32
    }
    pub fn set_str_flags(&mut self, v: u32) {
        self.u = (self.u & 0xffff_ffff) | ((v as u64) << 32);
    }
    pub fn value_q(&self) -> u64 {
        rd64(&self.value)
    }
    pub fn set_value_q(&mut self, v: u64) {
        self.value[..8].copy_from_slice(&v.to_le_bytes());
    }
    pub fn value_f(&self) -> f32 {
        f32::from_bits(rd32(&self.value, false))
    }
    pub fn value_d(&self) -> f64 {
        f64::from_bits(rd64(&self.value))
    }

    /// One entry of a compiled database (bs1() when `swap`).
    pub fn from_bytes(b: &[u8], swap: bool) -> Magic {
        let mut m = Magic {
            cont_level: rd16(&b[0..], swap),
            flag: b[2],
            factor: b[3],
            reln: b[4],
            vallen: b[5],
            typ: b[6],
            in_type: b[7],
            in_op: b[8],
            mask_op: b[9],
            cond: b[10],
            factor_op: b[11],
            offset: rd32(&b[12..], swap) as i32,
            in_offset: rd32(&b[16..], swap) as i32,
            lineno: rd32(&b[20..], swap),
            u: rd64(&b[24..]),
            ..Default::default()
        };
        m.value.copy_from_slice(&b[32..160]);
        m.desc.copy_from_slice(&b[160..224]);
        m.mimetype.copy_from_slice(&b[224..304]);
        m.apple.copy_from_slice(&b[304..312]);
        m.ext.copy_from_slice(&b[312..432]);
        if swap {
            if is_libmagic_string(m.typ) {
                let r = m.str_range().swap_bytes();
                let f = m.str_flags().swap_bytes();
                m.u = (r as u64) | ((f as u64) << 32);
            } else {
                let q = m.value_q().swap_bytes();
                m.set_value_q(q);
                m.u = m.u.swap_bytes();
            }
        }
        m
    }

    /// The in-memory layout, for apprentice_sort()'s memcmp().
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut b = Vec::with_capacity(FILE_MAGICSIZE);
        b.extend_from_slice(&self.cont_level.to_le_bytes());
        b.extend_from_slice(&[
            self.flag,
            self.factor,
            self.reln,
            self.vallen,
            self.typ,
            self.in_type,
            self.in_op,
            self.mask_op,
            self.cond,
            self.factor_op,
        ]);
        b.extend_from_slice(&self.offset.to_le_bytes());
        b.extend_from_slice(&self.in_offset.to_le_bytes());
        b.extend_from_slice(&self.lineno.to_le_bytes());
        b.extend_from_slice(&self.u.to_le_bytes());
        b.extend_from_slice(&self.value);
        b.extend_from_slice(&self.desc);
        b.extend_from_slice(&self.mimetype);
        b.extend_from_slice(&self.apple);
        b.extend_from_slice(&self.ext);
        b
    }
}

/// strlen() of a NUL-terminated field
pub fn cstr(b: &[u8]) -> &[u8] {
    match b.iter().position(|&c| c == 0) {
        Some(n) => &b[..n],
        None => b,
    }
}

// ---------------------------------------------------------------------------
// union VALUETYPE

#[derive(Clone)]
pub struct ValueType(pub [u8; MAXSTRING]);

impl ValueType {
    pub fn b(&self) -> u8 {
        self.0[0]
    }
    pub fn h(&self) -> u16 {
        u16::from_le_bytes([self.0[0], self.0[1]])
    }
    pub fn l(&self) -> u32 {
        u32::from_le_bytes(self.0[..4].try_into().unwrap())
    }
    pub fn q(&self) -> u64 {
        u64::from_le_bytes(self.0[..8].try_into().unwrap())
    }
    pub fn f(&self) -> f32 {
        f32::from_bits(self.l())
    }
    pub fn d(&self) -> f64 {
        f64::from_bits(self.q())
    }
    pub fn set_b(&mut self, v: u8) {
        self.0[0] = v;
    }
    pub fn set_h(&mut self, v: u16) {
        self.0[..2].copy_from_slice(&v.to_le_bytes());
    }
    pub fn set_l(&mut self, v: u32) {
        self.0[..4].copy_from_slice(&v.to_le_bytes());
    }
    pub fn set_q(&mut self, v: u64) {
        self.0[..8].copy_from_slice(&v.to_le_bytes());
    }
    pub fn set_f(&mut self, v: f32) {
        self.set_l(v.to_bits());
    }
    pub fn set_d(&mut self, v: f64) {
        self.set_q(v.to_bits());
    }
}

// ---------------------------------------------------------------------------
// buffers

/// A view on (part of) a byte buffer; bytes past its end read as NUL
/// (the SLOP of file_or_stream()).
#[derive(Clone)]
pub struct BufRef {
    pub data: Rc<[u8]>,
    pub off: usize,
    pub len: usize,
}

impl BufRef {
    pub fn new(v: Vec<u8>) -> BufRef {
        let len = v.len();
        BufRef {
            data: Rc::from(v.into_boxed_slice()),
            off: 0,
            len,
        }
    }
    pub fn empty() -> BufRef {
        BufRef::new(Vec::new())
    }
    pub fn at(&self, i: usize) -> u8 {
        if i < self.len {
            self.data[self.off + i]
        } else {
            0
        }
    }
    pub fn bytes(&self) -> &[u8] {
        &self.data[self.off..self.off + self.len]
    }
    /// `s + off` limited to `len` bytes
    pub fn sub(&self, off: usize, len: usize) -> BufRef {
        let off = off.min(self.len);
        let len = len.min(self.len - off);
        BufRef {
            data: self.data.clone(),
            off: self.off + off,
            len,
        }
    }
}

/// The descriptor of the file being typed (when it is a plain file).
pub type Fd = Rc<RefCell<File>>;

pub fn fd_read(fd: &Fd, off: u64, len: usize) -> Option<Vec<u8>> {
    let mut f = fd.borrow_mut();
    f.seek(SeekFrom::Start(off)).ok()?;
    let mut v = vec![0u8; len];
    f.read_exact(&mut v).ok()?;
    Some(v)
}

pub enum EBuf {
    Unfilled,
    Bad,
    Filled(Option<BufRef>),
}

/// struct buffer
pub struct Buffer {
    pub fd: Option<Fd>,
    pub st_mode: u32,
    pub st_size: i64,
    pub fbuf: BufRef,
    pub ebuf: RefCell<EBuf>,
}

impl Buffer {
    pub fn new(fd: Option<Fd>, st_mode: u32, st_size: i64, data: BufRef) -> Buffer {
        Buffer {
            fd,
            st_mode,
            st_size,
            fbuf: data,
            ebuf: RefCell::new(EBuf::Unfilled),
        }
    }

    pub fn flen(&self) -> usize {
        self.fbuf.len
    }

    /// buffer_fill(): the last flen bytes of the file
    pub fn fill(&self) -> Result<Option<BufRef>, ()> {
        match &*self.ebuf.borrow() {
            EBuf::Bad => return Err(()),
            EBuf::Filled(Some(b)) => return Ok(Some(b.clone())),
            _ => {}
        }
        let r = (|| {
            if self.st_mode & S_IFMT != S_IFREG {
                return Err(());
            }
            let elen = (self.st_size.max(0) as usize).min(self.flen());
            if elen == 0 {
                return Ok(None);
            }
            let fd = self.fd.as_ref().ok_or(())?;
            let eoff = self.st_size as u64 - elen as u64;
            let v = fd_read(fd, eoff, elen).ok_or(())?;
            Ok(Some(BufRef::new(v)))
        })();
        *self.ebuf.borrow_mut() = match &r {
            Ok(b) => EBuf::Filled(b.clone()),
            Err(()) => EBuf::Bad,
        };
        r
    }
}

// ---------------------------------------------------------------------------
// struct magic_set

#[derive(Clone, Copy, Default)]
pub struct LevelInfo {
    pub off: i32,
    pub got_match: bool,
    pub last_match: bool,
    pub last_cond: u8,
}

#[derive(Clone, Default)]
pub struct Search {
    /// start of search in the source (buffer and absolute index)
    pub s: Option<(Rc<[u8]>, usize, usize)>,
    pub s_len: usize,
    pub offset: usize,
    pub rm_len: usize,
}

impl Search {
    /// byte `i` of the search region
    pub fn at(&self, i: usize) -> u8 {
        match &self.s {
            Some((d, start, end)) => {
                let p = start + i;
                if p < *end { d[p] } else { 0 }
            }
            None => 0,
        }
    }
    pub fn bytes(&self, len: usize) -> Vec<u8> {
        (0..len).map(|i| self.at(i)).collect()
    }
}

pub type MList = Rc<Vec<Magic>>;

pub struct MagicSet {
    pub mlist: [Option<Vec<MList>>; 2],
    pub c: Vec<LevelInfo>,
    pub o_buf: Option<Vec<u8>>,
    pub offset: u32,
    pub eoffset: u32,
    pub error: i32,
    pub flags: i32,
    pub event_flags: i32,
    pub file: Vec<u8>,
    pub line: usize,
    pub mode: u32,
    pub magwarn: u16,
    pub search: Search,
    pub ms_value: ValueType,
    pub indir_max: u16,
    pub name_max: u16,
    pub regex_max: u16,
    pub magwarn_max: u16,
    pub bytes_max: usize,
    pub encoding_max: usize,
}

impl MagicSet {
    /// file_ms_alloc()
    pub fn new(flags: i32) -> MagicSet {
        MagicSet {
            mlist: [None, None],
            c: vec![LevelInfo::default(); 10],
            o_buf: None,
            offset: 0,
            eoffset: 0,
            error: -1,
            flags,
            event_flags: 0,
            file: b"unknown".to_vec(),
            line: 0,
            mode: 0,
            magwarn: 0,
            search: Search::default(),
            ms_value: ValueType([0; MAXSTRING]),
            indir_max: FILE_INDIR_MAX,
            name_max: FILE_NAME_MAX,
            regex_max: FILE_REGEX_MAX,
            magwarn_max: FILE_MAGWARN_MAX,
            bytes_max: FILE_BYTES_MAX,
            encoding_max: FILE_ENCODING_MAX,
        }
    }

    /// file_check_mem()
    pub fn check_mem(&mut self, level: usize) {
        if level >= self.c.len() {
            self.c.resize(20 + level, LevelInfo::default());
        }
        let li = &mut self.c[level];
        li.got_match = false;
        li.last_match = false;
        li.last_cond = COND_NONE;
    }
}

/// What libmagic needs from PHP: diagnostics, streams and PCRE.
pub trait Host {
    /// file_magwarn(): php_error_docref(NULL, E_WARNING, ...)
    fn warn(&mut self, msg: &[u8]);
    /// php_stream_open_wrapper(fn, "rb", REPORT_ERRORS) + read it all
    fn read_file(&mut self, path: &[u8]) -> Option<Vec<u8>>;
    /// php_sys_stat(): (mode, size)
    fn stat(&mut self, path: &[u8]) -> Option<(u32, i64)>;
    /// the names in a directory (php_stream_opendir()), None on failure
    fn readdir(&mut self, path: &[u8]) -> Option<Vec<Vec<u8>>>;
    /// pcre_get_compiled_regex_cache() of "~pat~flags": false on failure
    fn regex_compile(&mut self, pattern: &[u8]) -> bool;
    /// php_pcre_match_impl(): Ok(Some((start, len))) for a match
    fn regex_match(&mut self, pattern: &[u8], subject: &[u8])
    -> Result<Option<(usize, usize)>, ()>;
}

// ---------------------------------------------------------------------------
// C library helpers (C locale)

pub fn c_isspace(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}
pub fn c_isprint(c: u8) -> bool {
    (0x20..0x7f).contains(&c)
}

/// strtoull()/strtol() style parse: (value, bytes consumed, overflow)
pub fn c_strtoull(s: &[u8], base: u32) -> (u64, usize, bool) {
    let mut i = 0;
    while i < s.len() && c_isspace(s[i]) {
        i += 1;
    }
    let mut neg = false;
    if i < s.len() && (s[i] == b'+' || s[i] == b'-') {
        neg = s[i] == b'-';
        i += 1;
    }
    let mut base = base;
    if (base == 0 || base == 16)
        && i + 1 < s.len()
        && s[i] == b'0'
        && (s[i + 1] | 0x20) == b'x'
        && i + 2 < s.len()
        && (s[i + 2] as char).is_ascii_hexdigit()
    {
        i += 2;
        base = 16;
    } else if base == 0 {
        base = if i < s.len() && s[i] == b'0' { 8 } else { 10 };
    }
    let start = i;
    let mut v: u64 = 0;
    let mut ovf = false;
    while i < s.len() {
        let d = match (s[i] as char).to_digit(36) {
            Some(d) if d < base => d,
            _ => break,
        };
        match v
            .checked_mul(base as u64)
            .and_then(|x| x.checked_add(d as u64))
        {
            Some(x) => v = x,
            None => ovf = true,
        }
        i += 1;
    }
    if i == start {
        return (0, 0, false);
    }
    if ovf {
        v = u64::MAX;
    } else if neg {
        v = v.wrapping_neg();
    }
    (v, i, ovf)
}

/// strtol(): a long (64-bit)
pub fn c_strtol(s: &[u8], base: u32) -> (i64, usize) {
    let (v, n, ovf) = c_strtoull(s, base);
    if n == 0 {
        return (0, 0);
    }
    // sign and range of a long
    let mut i = 0;
    while i < s.len() && c_isspace(s[i]) {
        i += 1;
    }
    let neg = s.get(i) == Some(&b'-');
    if ovf {
        return (if neg { i64::MIN } else { i64::MAX }, n);
    }
    if neg {
        let mag = v.wrapping_neg();
        if mag > (i64::MAX as u64) + 1 {
            return (i64::MIN, n);
        }
        (v as i64, n)
    } else {
        if v > i64::MAX as u64 {
            return (i64::MAX, n);
        }
        (v as i64, n)
    }
}
