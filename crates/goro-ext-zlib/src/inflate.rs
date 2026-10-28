//! Streaming inflater for raw deflate (RFC 1951), zlib (RFC 1950) and gzip
//! (RFC 1952) data.
//!
//! The decoder is a resumable state machine that takes input and output a
//! piece at a time, like zlib's `inflate()` as the zlib manual describes it:
//! it stops when it runs out of input or of output space, reports
//! `Z_NEED_DICT` for a zlib stream with a preset dictionary, honours
//! `Z_BLOCK` (stop at the next block boundary) and gives back the exact
//! number of input bytes it used. PHP exposes these details through
//! `inflate_add()`, `inflate_get_status()` and `inflate_get_read_len()`;
//! the behaviours were checked against PHP 8.5 in tests/fixtures.

use crate::checksum::{Adler32, Crc32};
use std::sync::OnceLock;

/// Return codes of an inflate call (zlib manual: inflate; the `ZLIB_*`
/// constants).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InflateStatus {
    Ok,
    StreamEnd,
    NeedDict,
    BufError,
    /// Corrupt input; the message is zlib's description of the problem.
    DataError(&'static str),
}

impl InflateStatus {
    /// The zlib return code (`ZLIB_OK`, `ZLIB_STREAM_END`, ...).
    pub fn code(self) -> i64 {
        match self {
            InflateStatus::Ok => 0,
            InflateStatus::StreamEnd => 1,
            InflateStatus::NeedDict => 2,
            InflateStatus::BufError => -5,
            InflateStatus::DataError(_) => -3,
        }
    }
}

/// Container expected by the inflater.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InflateWrap {
    Raw,
    Zlib,
    Gzip,
    /// zlib or gzip, told apart by the gzip magic number.
    Auto,
}

impl InflateWrap {
    /// From zlib's `windowBits` convention: negative for raw, +16 for
    /// gzip, +32 for automatic detection.
    pub fn from_window_bits(wb: i64) -> Option<(InflateWrap, u32)> {
        match wb {
            -15..=-8 => Some((InflateWrap::Raw, (-wb) as u32)),
            8..=15 => Some((InflateWrap::Zlib, wb as u32)),
            24..=31 => Some((InflateWrap::Gzip, (wb - 16) as u32)),
            40..=47 => Some((InflateWrap::Auto, (wb - 32) as u32)),
            _ => None,
        }
    }
}

/// Flush argument of an inflate call. Only `Block` and `Finish` change
/// what inflate does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InflateFlush {
    None,
    Block,
    Finish,
}

// Table entries: value << 16 | kind << 8 | extra << 4 | len.
const K_LIT: u32 = 0;
const K_BASE: u32 = 1;
const K_EOB: u32 = 2;
const K_SUB: u32 = 3;
const K_BAD: u32 = 4;

#[inline(always)]
fn e_len(e: u32) -> u32 {
    e & 15
}
#[inline(always)]
fn e_extra(e: u32) -> u32 {
    (e >> 4) & 15
}
#[inline(always)]
fn e_kind(e: u32) -> u32 {
    (e >> 8) & 7
}
#[inline(always)]
fn e_val(e: u32) -> u32 {
    e >> 16
}
const fn entry(kind: u32, val: u32, extra: u32, len: u32) -> u32 {
    (val << 16) | (kind << 8) | (extra << 4) | len
}

const LEN_BASE: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];
const LEN_EXTRA: [u8; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
const DIST_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const DIST_EXTRA: [u8; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];
const CL_ORDER: [usize; 19] = [
    16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum TableKind {
    Codes,
    Lens,
    Dists,
}

/// A two-level decoding table: a root indexed by the next `root` bits,
/// with sub-tables for longer codes.
#[derive(Clone)]
struct Table {
    root: u32,
    e: Vec<u32>,
}

impl Table {
    #[inline(always)]
    fn lookup(&self, hold: u64) -> u32 {
        let e = self.e[(hold as usize) & ((1 << self.root) - 1)];
        if e_kind(e) == K_SUB {
            let idx = e_val(e) as usize + ((hold >> self.root) as usize & ((1 << e_extra(e)) - 1));
            self.e[idx]
        } else {
            e
        }
    }

    /// Builds the table for canonical code lengths `lens` (RFC 1951
    /// 3.2.2). Over-subscribed sets are rejected, and incomplete ones too
    /// except a single one-bit code for literals or distances.
    fn build(lens: &[u8], kind: TableKind, root: u32) -> Option<Table> {
        let mut count = [0u32; 16];
        for &l in lens {
            count[l as usize] += 1;
        }
        let max = (1..16).rev().find(|&l| count[l] != 0).unwrap_or(0);
        let size = 1usize << root;
        if max == 0 {
            // No codes at all: any use of the table is an error.
            return Some(Table {
                root,
                e: vec![entry(K_BAD, 0, 0, 1); size],
            });
        }
        let mut left: i32 = 1;
        for &c in &count[1..16] {
            left <<= 1;
            left -= c as i32;
            if left < 0 {
                return None;
            }
        }
        if left > 0 && (kind == TableKind::Codes || max != 1) {
            return None;
        }
        let mut next = [0u32; 16];
        let mut code = 0u32;
        count[0] = 0;
        for l in 1..16 {
            code = (code + count[l - 1]) << 1;
            next[l] = code;
        }
        let mut e = vec![entry(K_BAD, 0, 0, 1); size];
        // Sub-table size needed for each root prefix.
        let mut sub_bits = vec![0u8; if max as u32 > root { size } else { 0 }];
        let mut codes = Vec::with_capacity(lens.len());
        for (sym, &l) in lens.iter().enumerate() {
            if l == 0 {
                continue;
            }
            let c = next[l as usize];
            next[l as usize] += 1;
            let rev = c.reverse_bits() >> (32 - l as u32);
            codes.push((sym, l as u32, rev));
            if l as u32 > root {
                let p = (rev as usize) & (size - 1);
                sub_bits[p] = sub_bits[p].max((l as u32 - root) as u8);
            }
        }
        let mut sub_off = vec![0u32; sub_bits.len()];
        for p in 0..sub_bits.len() {
            if sub_bits[p] != 0 {
                sub_off[p] = e.len() as u32;
                e[p] = entry(K_SUB, e.len() as u32, sub_bits[p] as u32, root);
                let n = 1usize << sub_bits[p];
                e.extend(std::iter::repeat_n(entry(K_BAD, 0, 0, root + 1), n));
            }
        }
        for (sym, l, rev) in codes {
            let ent = match kind {
                TableKind::Codes => entry(K_LIT, sym as u32, 0, l),
                TableKind::Lens => match sym {
                    0..=255 => entry(K_LIT, sym as u32, 0, l),
                    256 => entry(K_EOB, 0, 0, l),
                    257..=285 => entry(
                        K_BASE,
                        LEN_BASE[sym - 257] as u32,
                        LEN_EXTRA[sym - 257] as u32,
                        l,
                    ),
                    _ => entry(K_BAD, 0, 0, l),
                },
                TableKind::Dists => match sym {
                    0..=29 => entry(K_BASE, DIST_BASE[sym] as u32, DIST_EXTRA[sym] as u32, l),
                    _ => entry(K_BAD, 0, 0, l),
                },
            };
            if l <= root {
                let mut i = rev as usize;
                while i < size {
                    e[i] = ent;
                    i += 1 << l;
                }
            } else {
                let p = rev as usize & (size - 1);
                let base = sub_off[p] as usize;
                let n = 1usize << sub_bits[p];
                let mut i = (rev >> root) as usize;
                while i < n {
                    e[base + i] = ent;
                    i += 1 << (l - root);
                }
            }
        }
        Some(Table { root, e })
    }
}

const LIT_ROOT: u32 = 10;
const DIST_ROOT: u32 = 8;

fn fixed_tables() -> &'static (Table, Table) {
    static T: OnceLock<(Table, Table)> = OnceLock::new();
    T.get_or_init(|| {
        let mut l = [0u8; 288];
        for (i, x) in l.iter_mut().enumerate() {
            *x = match i {
                0..=143 => 8,
                144..=255 => 9,
                256..=279 => 7,
                _ => 8,
            };
        }
        let lit = Table::build(&l, TableKind::Lens, LIT_ROOT).unwrap();
        let dist = Table::build(&[5u8; 32], TableKind::Dists, DIST_ROOT).unwrap();
        (lit, dist)
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Head,
    GzFlags,
    GzTime,
    GzOs,
    GzExLen,
    GzExtra,
    GzName,
    GzComment,
    GzHcrc,
    DictId,
    Dict,
    Type,
    TypeDo,
    Stored,
    Copy,
    Table,
    LenLens,
    CodeLens,
    Len,
    LenExt,
    Dist,
    DistExt,
    Match,
    Lit,
    Check,
    Length,
    Done,
    Bad,
}

/// Why [`Inflater::set_dictionary`] refused a dictionary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DictionaryError {
    /// The stream is not waiting for one (zlib's `Z_STREAM_ERROR`).
    NotExpected,
    /// Its Adler-32 differs from the stream's (zlib's `Z_DATA_ERROR`).
    Mismatch,
}

/// Streaming inflater.
pub struct Inflater {
    wrap: InflateWrap,
    wbits: u32,
    mode: Mode,
    hold: u64,
    bits: u32,
    last: bool,
    gzip: bool,
    gz_flags: u32,
    gz_count: usize,
    head_crc: Option<Crc32>,
    adler: Adler32,
    crc: Crc32,
    dict_id: u32,
    have_dict: bool,
    dynamic: Option<Box<(Table, Table)>>,
    nlen: usize,
    ndist: usize,
    ncode: usize,
    have: usize,
    lens: [u8; 320],
    codes: Option<Table>,
    length: usize,
    extra: u32,
    dist: usize,
    /// The last `1 << wbits` bytes of output from earlier calls.
    window: Vec<u8>,
    total_in: u64,
    total_out: u64,
    error: &'static str,
}

/// Per-call input and output state.
struct Io<'a, 'b> {
    input: &'a [u8],
    pos: usize,
    out: &'b mut Vec<u8>,
    base: usize,
    limit: usize,
}

impl Io<'_, '_> {
    #[inline(always)]
    fn produced(&self) -> usize {
        self.out.len() - self.base
    }
    #[inline(always)]
    fn room(&self) -> usize {
        self.limit - self.produced()
    }
}

macro_rules! need {
    ($s:expr, $io:expr, $n:expr) => {
        while $s.bits < $n {
            if $io.pos >= $io.input.len() {
                break;
            }
            $s.hold |= ($io.input[$io.pos] as u64) << $s.bits;
            $io.pos += 1;
            $s.bits += 8;
        }
    };
}

impl Inflater {
    pub fn new(wrap: InflateWrap, wbits: u32) -> Inflater {
        Inflater {
            wrap,
            wbits: wbits.clamp(8, 15),
            mode: Mode::Head,
            hold: 0,
            bits: 0,
            last: false,
            gzip: false,
            gz_flags: 0,
            gz_count: 0,
            head_crc: None,
            adler: Adler32::new(),
            crc: Crc32::new(),
            dict_id: 0,
            have_dict: false,
            dynamic: None,
            nlen: 0,
            ndist: 0,
            ncode: 0,
            have: 0,
            lens: [0; 320],
            codes: None,
            length: 0,
            extra: 0,
            dist: 0,
            window: Vec::new(),
            total_in: 0,
            total_out: 0,
            error: "",
        }
    }

    /// Back to the start of a stream (zlib's `inflateReset`).
    pub fn reset(&mut self) {
        let (w, b) = (self.wrap, self.wbits);
        let window = std::mem::take(&mut self.window);
        *self = Inflater::new(w, b);
        self.window = window;
        self.window.clear();
    }

    /// Bytes of input used since the start of the stream.
    pub fn total_in(&self) -> u64 {
        self.total_in
    }

    pub fn total_out(&self) -> u64 {
        self.total_out
    }

    /// zlib's message for the last data error.
    pub fn error_message(&self) -> &'static str {
        self.error
    }

    /// True while waiting for a preset dictionary.
    pub fn needs_dictionary(&self) -> bool {
        self.mode == Mode::Dict && !self.have_dict
    }

    /// Supplies the preset dictionary (zlib manual: inflateSetDictionary):
    /// for a zlib stream once `NeedDict` was returned, checked against the
    /// stream's Adler-32 (`Err` if it differs); for raw deflate at any time.
    pub fn set_dictionary(&mut self, dict: &[u8]) -> Result<(), DictionaryError> {
        match self.wrap {
            InflateWrap::Raw => {}
            _ => {
                if self.mode != Mode::Dict {
                    return Err(DictionaryError::NotExpected);
                }
                if crate::checksum::adler32(dict) != self.dict_id {
                    return Err(DictionaryError::Mismatch);
                }
            }
        }
        let wsize = 1usize << self.wbits;
        let d = if dict.len() > wsize {
            &dict[dict.len() - wsize..]
        } else {
            dict
        };
        self.push_window(d);
        self.have_dict = true;
        Ok(())
    }

    fn push_window(&mut self, data: &[u8]) {
        let wsize = 1usize << self.wbits;
        if data.len() >= wsize {
            self.window.clear();
            self.window.extend_from_slice(&data[data.len() - wsize..]);
        } else {
            let keep = (wsize - data.len()).min(self.window.len());
            let drop = self.window.len() - keep;
            self.window.drain(..drop);
            self.window.extend_from_slice(data);
        }
    }

    fn bad(&mut self, msg: &'static str) {
        self.mode = Mode::Bad;
        self.error = msg;
    }

    /// One inflate call: decodes from `input`, appending at most `limit`
    /// bytes to `out`. Returns the number of input bytes used and the
    /// status, as zlib's `inflate()` would.
    pub fn inflate(
        &mut self,
        input: &[u8],
        out: &mut Vec<u8>,
        limit: usize,
        flush: InflateFlush,
    ) -> (usize, InflateStatus) {
        if self.mode == Mode::Type {
            self.mode = Mode::TypeDo;
        }
        let base = out.len();
        let mut io = Io {
            input,
            pos: 0,
            out,
            base,
            limit,
        };
        let mut ck_pos = base;
        let mut ret = InflateStatus::Ok;
        self.run(&mut io, flush, &mut ck_pos, &mut ret);
        let used = io.pos;
        let produced = io.produced();
        // Checksum and window for the output of this call.
        if io.out.len() > ck_pos {
            self.update_check(&io.out[ck_pos..]);
        }
        if produced > 0 && self.mode != Mode::Done && self.mode != Mode::Bad {
            self.push_window(&io.out[base..]);
        }
        self.total_in += used as u64;
        self.total_out += produced as u64;
        if let InflateStatus::Ok = ret
            && ((used == 0 && produced == 0) || flush == InflateFlush::Finish)
        {
            ret = InflateStatus::BufError;
        }
        (used, ret)
    }

    fn update_check(&mut self, data: &[u8]) {
        if self.wrap == InflateWrap::Raw {
            return;
        }
        if self.gzip {
            self.crc.update(data);
        } else {
            self.adler.update(data);
        }
    }

    fn flush_check(&mut self, io: &Io, ck_pos: &mut usize) {
        self.update_check(&io.out[*ck_pos..]);
        *ck_pos = io.out.len();
    }

    #[inline(always)]
    fn drop_bits(&mut self, n: u32) {
        self.hold >>= n;
        self.bits -= n;
    }

    /// The available history before this call plus this call's output.
    fn copy_match(&mut self, io: &mut Io, len: usize, dist: usize) {
        let produced = io.produced();
        if dist > produced {
            // Part of the match comes from the window.
            let back = dist - produced;
            let wl = self.window.len();
            let start = wl - back;
            let from_win = back.min(len);
            io.out
                .extend_from_slice(&self.window[start..start + from_win]);
            let rest = len - from_win;
            if rest > 0 {
                copy_within_out(io.out, dist, rest);
            }
        } else {
            copy_within_out(io.out, dist, len);
        }
    }

    fn run(
        &mut self,
        io: &mut Io,
        flush: InflateFlush,
        ck_pos: &mut usize,
        ret: &mut InflateStatus,
    ) {
        loop {
            match self.mode {
                Mode::Head => {
                    if self.wrap == InflateWrap::Raw {
                        self.mode = Mode::TypeDo;
                        continue;
                    }
                    need!(self, io, 16);
                    if self.bits < 16 {
                        return;
                    }
                    let allow_gzip = matches!(self.wrap, InflateWrap::Gzip | InflateWrap::Auto);
                    let allow_zlib = matches!(self.wrap, InflateWrap::Zlib | InflateWrap::Auto);
                    if allow_gzip && (self.hold & 0xffff) == 0x8b1f {
                        self.gzip = true;
                        let mut c = Crc32::new();
                        c.update(&[0x1f, 0x8b]);
                        self.head_crc = Some(c);
                        self.drop_bits(16);
                        self.mode = Mode::GzFlags;
                        continue;
                    }
                    let cmf = (self.hold & 0xff) as u32;
                    let flg = ((self.hold >> 8) & 0xff) as u32;
                    if !allow_zlib || !((cmf << 8) + flg).is_multiple_of(31) {
                        self.bad("incorrect header check");
                        continue;
                    }
                    if cmf & 15 != 8 {
                        self.bad("unknown compression method");
                        continue;
                    }
                    let len = (cmf >> 4) + 8;
                    if len > 15 || len > self.wbits {
                        self.bad("invalid window size");
                        continue;
                    }
                    self.drop_bits(16);
                    self.mode = if flg & 0x20 != 0 {
                        Mode::DictId
                    } else {
                        Mode::Type
                    };
                    if self.mode == Mode::Type && flush == InflateFlush::Block {
                        return;
                    }
                }
                Mode::GzFlags => {
                    need!(self, io, 16);
                    if self.bits < 16 {
                        return;
                    }
                    let b = [(self.hold & 0xff) as u8, ((self.hold >> 8) & 0xff) as u8];
                    if b[0] != 8 {
                        self.bad("unknown compression method");
                        continue;
                    }
                    if b[1] & 0xe0 != 0 {
                        self.bad("unknown header flags set");
                        continue;
                    }
                    self.gz_flags = b[1] as u32;
                    self.head_crc_update(&b);
                    self.drop_bits(16);
                    self.mode = Mode::GzTime;
                }
                Mode::GzTime => {
                    need!(self, io, 32);
                    if self.bits < 32 {
                        return;
                    }
                    let b = (self.hold as u32).to_le_bytes();
                    self.head_crc_update(&b);
                    self.drop_bits(32);
                    self.mode = Mode::GzOs;
                }
                Mode::GzOs => {
                    need!(self, io, 16);
                    if self.bits < 16 {
                        return;
                    }
                    let b = (self.hold as u16).to_le_bytes();
                    self.head_crc_update(&b);
                    self.drop_bits(16);
                    self.mode = Mode::GzExLen;
                }
                Mode::GzExLen => {
                    if self.gz_flags & 4 != 0 {
                        need!(self, io, 16);
                        if self.bits < 16 {
                            return;
                        }
                        let b = (self.hold as u16).to_le_bytes();
                        self.head_crc_update(&b);
                        self.gz_count = u16::from_le_bytes(b) as usize;
                        self.drop_bits(16);
                    }
                    self.mode = Mode::GzExtra;
                }
                Mode::GzExtra => {
                    if self.gz_flags & 4 != 0 {
                        let n = self.gz_count.min(io.input.len() - io.pos);
                        let chunk = &io.input[io.pos..io.pos + n];
                        if let Some(c) = &mut self.head_crc {
                            c.update(chunk);
                        }
                        io.pos += n;
                        self.gz_count -= n;
                        if self.gz_count > 0 {
                            return;
                        }
                    }
                    self.mode = Mode::GzName;
                }
                Mode::GzName | Mode::GzComment => {
                    let flag = if self.mode == Mode::GzName { 8 } else { 16 };
                    if self.gz_flags & flag != 0 {
                        let rest = &io.input[io.pos..];
                        match rest.iter().position(|&b| b == 0) {
                            Some(i) => {
                                let chunk = &rest[..=i];
                                if let Some(c) = &mut self.head_crc {
                                    c.update(chunk);
                                }
                                io.pos += i + 1;
                            }
                            None => {
                                if let Some(c) = &mut self.head_crc {
                                    c.update(rest);
                                }
                                io.pos = io.input.len();
                                return;
                            }
                        }
                    }
                    self.mode = if self.mode == Mode::GzName {
                        Mode::GzComment
                    } else {
                        Mode::GzHcrc
                    };
                }
                Mode::GzHcrc => {
                    if self.gz_flags & 2 != 0 {
                        need!(self, io, 16);
                        if self.bits < 16 {
                            return;
                        }
                        let want = self
                            .head_crc
                            .as_ref()
                            .map(|c| c.value() & 0xffff)
                            .unwrap_or(0);
                        if (self.hold & 0xffff) as u32 != want {
                            self.bad("header crc mismatch");
                            continue;
                        }
                        self.drop_bits(16);
                    }
                    self.head_crc = None;
                    self.mode = Mode::Type;
                    if flush == InflateFlush::Block {
                        return;
                    }
                }
                Mode::DictId => {
                    need!(self, io, 32);
                    if self.bits < 32 {
                        return;
                    }
                    self.dict_id = (self.hold as u32).swap_bytes();
                    self.drop_bits(32);
                    self.mode = Mode::Dict;
                }
                Mode::Dict => {
                    if !self.have_dict {
                        *ret = InflateStatus::NeedDict;
                        return;
                    }
                    self.mode = Mode::Type;
                    if flush == InflateFlush::Block {
                        return;
                    }
                }
                Mode::Type | Mode::TypeDo => {
                    if self.last {
                        // Skip to a byte boundary for the trailer.
                        let r = self.bits & 7;
                        self.drop_bits(r);
                        self.mode = Mode::Check;
                        continue;
                    }
                    need!(self, io, 3);
                    if self.bits < 3 {
                        return;
                    }
                    self.last = self.hold & 1 != 0;
                    let t = (self.hold >> 1) & 3;
                    self.drop_bits(3);
                    match t {
                        0 => self.mode = Mode::Stored,
                        1 => {
                            self.dynamic = None;
                            self.mode = Mode::Len;
                        }
                        2 => self.mode = Mode::Table,
                        _ => self.bad("invalid block type"),
                    }
                }
                Mode::Stored => {
                    let r = self.bits & 7;
                    self.drop_bits(r);
                    need!(self, io, 32);
                    if self.bits < 32 {
                        return;
                    }
                    let v = self.hold as u32;
                    if (v & 0xffff) != ((v >> 16) ^ 0xffff) {
                        self.bad("invalid stored block lengths");
                        continue;
                    }
                    self.length = (v & 0xffff) as usize;
                    self.drop_bits(32);
                    self.mode = Mode::Copy;
                }
                Mode::Copy => {
                    if self.length > 0 {
                        let n = self.length.min(io.input.len() - io.pos).min(io.room());
                        if n == 0 {
                            return;
                        }
                        io.out.extend_from_slice(&io.input[io.pos..io.pos + n]);
                        io.pos += n;
                        self.length -= n;
                        continue;
                    }
                    self.mode = Mode::Type;
                    if flush == InflateFlush::Block {
                        return;
                    }
                }
                Mode::Table => {
                    need!(self, io, 14);
                    if self.bits < 14 {
                        return;
                    }
                    self.nlen = (self.hold & 31) as usize + 257;
                    self.ndist = ((self.hold >> 5) & 31) as usize + 1;
                    self.ncode = ((self.hold >> 10) & 15) as usize + 4;
                    self.drop_bits(14);
                    if self.nlen > 286 || self.ndist > 30 {
                        self.bad("too many length or distance symbols");
                        continue;
                    }
                    self.have = 0;
                    self.lens = [0; 320];
                    self.mode = Mode::LenLens;
                }
                Mode::LenLens => {
                    while self.have < self.ncode {
                        need!(self, io, 3);
                        if self.bits < 3 {
                            return;
                        }
                        self.lens[CL_ORDER[self.have]] = (self.hold & 7) as u8;
                        self.drop_bits(3);
                        self.have += 1;
                    }
                    while self.have < 19 {
                        self.lens[CL_ORDER[self.have]] = 0;
                        self.have += 1;
                    }
                    match Table::build(&self.lens[..19], TableKind::Codes, 7) {
                        Some(t) => self.codes = Some(t),
                        None => {
                            self.bad("invalid code lengths set");
                            continue;
                        }
                    }
                    self.have = 0;
                    self.lens = [0; 320];
                    self.mode = Mode::CodeLens;
                }
                Mode::CodeLens => {
                    let total = self.nlen + self.ndist;
                    let mut failed = false;
                    while self.have < total {
                        let codes = self.codes.as_ref().unwrap();
                        let e = loop {
                            let e = codes.lookup(self.hold);
                            if e_len(e) <= self.bits {
                                break Some(e);
                            }
                            if io.pos >= io.input.len() {
                                break None;
                            }
                            self.hold |= (io.input[io.pos] as u64) << self.bits;
                            io.pos += 1;
                            self.bits += 8;
                        };
                        let Some(e) = e else { return };
                        if e_kind(e) == K_BAD {
                            // Every symbol of an all-zero code reads as 0.
                            self.drop_bits(1.min(self.bits));
                            self.lens[self.have] = 0;
                            self.have += 1;
                            continue;
                        }
                        let sym = e_val(e);
                        let l = e_len(e);
                        if sym < 16 {
                            self.drop_bits(l);
                            self.lens[self.have] = sym as u8;
                            self.have += 1;
                            continue;
                        }
                        let (xb, base) = match sym {
                            16 => (2, 3),
                            17 => (3, 3),
                            _ => (7, 11),
                        };
                        need!(self, io, l + xb);
                        if self.bits < l + xb {
                            return;
                        }
                        self.drop_bits(l);
                        let value = if sym == 16 {
                            if self.have == 0 {
                                failed = true;
                                break;
                            }
                            self.lens[self.have - 1]
                        } else {
                            0
                        };
                        let copy = base + (self.hold & ((1 << xb) - 1)) as usize;
                        self.drop_bits(xb);
                        if self.have + copy > total {
                            failed = true;
                            break;
                        }
                        for _ in 0..copy {
                            self.lens[self.have] = value;
                            self.have += 1;
                        }
                    }
                    if failed {
                        self.bad("invalid bit length repeat");
                        continue;
                    }
                    if self.lens[256] == 0 {
                        self.bad("invalid code -- missing end-of-block");
                        continue;
                    }
                    let Some(lt) = Table::build(&self.lens[..self.nlen], TableKind::Lens, LIT_ROOT)
                    else {
                        self.bad("invalid literal/lengths set");
                        continue;
                    };
                    let Some(dt) = Table::build(
                        &self.lens[self.nlen..self.nlen + self.ndist],
                        TableKind::Dists,
                        DIST_ROOT,
                    ) else {
                        self.bad("invalid distances set");
                        continue;
                    };
                    self.dynamic = Some(Box::new((lt, dt)));
                    self.codes = None;
                    self.mode = Mode::Len;
                }
                Mode::Len => {
                    if io.input.len() - io.pos >= 16 && io.room() >= 258 + 8 {
                        self.fast(io);
                        if self.mode != Mode::Len {
                            if self.mode == Mode::Type && flush == InflateFlush::Block {
                                return;
                            }
                            continue;
                        }
                    }
                    let tables = match &self.dynamic {
                        Some(t) => &**t,
                        None => fixed_tables(),
                    };
                    let e = loop {
                        let e = tables.0.lookup(self.hold);
                        if e_len(e) <= self.bits {
                            break Some(e);
                        }
                        if io.pos >= io.input.len() {
                            break None;
                        }
                        self.hold |= (io.input[io.pos] as u64) << self.bits;
                        io.pos += 1;
                        self.bits += 8;
                    };
                    let Some(e) = e else { return };
                    match e_kind(e) {
                        K_LIT => {
                            self.drop_bits(e_len(e));
                            self.length = e_val(e) as usize;
                            self.mode = Mode::Lit;
                        }
                        K_BASE => {
                            self.drop_bits(e_len(e));
                            self.length = e_val(e) as usize;
                            self.extra = e_extra(e);
                            self.mode = Mode::LenExt;
                        }
                        K_EOB => {
                            self.drop_bits(e_len(e));
                            self.mode = Mode::Type;
                            if flush == InflateFlush::Block {
                                return;
                            }
                        }
                        _ => self.bad("invalid literal/length code"),
                    }
                }
                Mode::Lit => {
                    if io.room() == 0 {
                        return;
                    }
                    io.out.push(self.length as u8);
                    self.mode = Mode::Len;
                }
                Mode::LenExt => {
                    if self.extra > 0 {
                        need!(self, io, self.extra);
                        if self.bits < self.extra {
                            return;
                        }
                        self.length += (self.hold & ((1 << self.extra) - 1)) as usize;
                        let x = self.extra;
                        self.drop_bits(x);
                    }
                    self.mode = Mode::Dist;
                }
                Mode::Dist => {
                    let tables = match &self.dynamic {
                        Some(t) => &**t,
                        None => fixed_tables(),
                    };
                    let e = loop {
                        let e = tables.1.lookup(self.hold);
                        if e_len(e) <= self.bits {
                            break Some(e);
                        }
                        if io.pos >= io.input.len() {
                            break None;
                        }
                        self.hold |= (io.input[io.pos] as u64) << self.bits;
                        io.pos += 1;
                        self.bits += 8;
                    };
                    let Some(e) = e else { return };
                    if e_kind(e) != K_BASE {
                        self.bad("invalid distance code");
                        continue;
                    }
                    self.drop_bits(e_len(e));
                    self.dist = e_val(e) as usize;
                    self.extra = e_extra(e);
                    self.mode = Mode::DistExt;
                }
                Mode::DistExt => {
                    if self.extra > 0 {
                        need!(self, io, self.extra);
                        if self.bits < self.extra {
                            return;
                        }
                        self.dist += (self.hold & ((1 << self.extra) - 1)) as usize;
                        let x = self.extra;
                        self.drop_bits(x);
                    }
                    if self.dist > self.window.len() + io.produced() {
                        self.bad("invalid distance too far back");
                        continue;
                    }
                    self.mode = Mode::Match;
                }
                Mode::Match => {
                    let n = self.length.min(io.room());
                    if n == 0 {
                        return;
                    }
                    // A match resumed in a later call only has the window.
                    if self.dist > self.window.len() + io.produced() {
                        self.bad("invalid distance too far back");
                        continue;
                    }
                    let d = self.dist;
                    self.copy_match(io, n, d);
                    self.length -= n;
                    if self.length == 0 {
                        self.mode = Mode::Len;
                    }
                }
                Mode::Check => {
                    if self.wrap == InflateWrap::Raw {
                        self.mode = Mode::Done;
                        continue;
                    }
                    need!(self, io, 32);
                    if self.bits < 32 {
                        return;
                    }
                    self.flush_check(io, ck_pos);
                    let v = self.hold as u32;
                    let ok = if self.gzip {
                        v == self.crc.value()
                    } else {
                        v.swap_bytes() == self.adler.value()
                    };
                    if !ok {
                        self.bad("incorrect data check");
                        continue;
                    }
                    self.drop_bits(32);
                    self.mode = if self.gzip { Mode::Length } else { Mode::Done };
                }
                Mode::Length => {
                    need!(self, io, 32);
                    if self.bits < 32 {
                        return;
                    }
                    let total = self.total_out + io.produced() as u64;
                    if self.hold as u32 != total as u32 {
                        self.bad("incorrect length check");
                        continue;
                    }
                    self.drop_bits(32);
                    self.mode = Mode::Done;
                }
                Mode::Done => {
                    *ret = InflateStatus::StreamEnd;
                    return;
                }
                Mode::Bad => {
                    *ret = InflateStatus::DataError(self.error);
                    return;
                }
            }
        }
    }

    fn head_crc_update(&mut self, b: &[u8]) {
        if let Some(c) = &mut self.head_crc {
            c.update(b);
        }
    }

    /// Decodes symbols while at least 16 input bytes and 266 bytes of
    /// output space remain, refilling the bit buffer eight bytes at a time.
    /// Leaves the mode at `Len` (needs the slow path), `Type` (end of
    /// block) or `Bad`.
    fn fast(&mut self, io: &mut Io) {
        let tables = match &self.dynamic {
            Some(t) => &**t,
            None => fixed_tables(),
        };
        let (lt, dt) = (&tables.0, &tables.1);
        let input = io.input;
        let mut pos = io.pos;
        let mut hold = self.hold;
        let mut bits = self.bits;
        let out = &mut *io.out;
        let base = io.base;
        let limit_end = if io.limit == usize::MAX {
            usize::MAX
        } else {
            base + io.limit
        };
        let win_len = self.window.len();
        let mut o = out.len();
        // Work in a zero-extended buffer so stores need no capacity checks.
        let chunk = 1 << 16;
        out.resize(o.saturating_add(chunk).min(limit_end), 0);
        let mut err: Option<&'static str> = None;
        let mut eob = false;
        while pos + 8 <= input.len() {
            if o + 266 > out.len() {
                if out.len() >= limit_end {
                    break;
                }
                let new_end = out
                    .len()
                    .saturating_add(out.len() - base + chunk)
                    .min(limit_end);
                if new_end < o + 266 {
                    break;
                }
                out.resize(new_end, 0);
            }
            // Refill to at least 56 bits.
            let w = u64::from_le_bytes(input[pos..pos + 8].try_into().unwrap());
            hold |= w << bits;
            pos += ((63 - bits) >> 3) as usize;
            bits |= 56;

            let mut e = lt.lookup(hold);
            if e_kind(e) == K_LIT {
                hold >>= e_len(e);
                bits -= e_len(e);
                out[o] = e_val(e) as u8;
                o += 1;
                // A second literal fits without refilling.
                e = lt.lookup(hold);
                if e_kind(e) == K_LIT {
                    hold >>= e_len(e);
                    bits -= e_len(e);
                    out[o] = e_val(e) as u8;
                    o += 1;
                    continue;
                }
                if bits < 48 {
                    // Not enough bits for a full match: refill first.
                    continue;
                }
            }
            match e_kind(e) {
                K_BASE => {
                    hold >>= e_len(e);
                    bits -= e_len(e);
                    let x = e_extra(e);
                    let len = e_val(e) as usize + (hold & ((1u64 << x) - 1)) as usize;
                    hold >>= x;
                    bits -= x;
                    let d = dt.lookup(hold);
                    if e_kind(d) != K_BASE {
                        err = Some("invalid distance code");
                        break;
                    }
                    hold >>= e_len(d);
                    bits -= e_len(d);
                    let x = e_extra(d);
                    let dist = e_val(d) as usize + (hold & ((1u64 << x) - 1)) as usize;
                    hold >>= x;
                    bits -= x;
                    let produced = o - base;
                    if dist > produced {
                        if dist > produced + win_len {
                            err = Some("invalid distance too far back");
                            break;
                        }
                        // From the window: rare, do it carefully.
                        let back = dist - produced;
                        let start = win_len - back;
                        let from_win = back.min(len);
                        out[o..o + from_win].copy_from_slice(&self.window[start..start + from_win]);
                        o += from_win;
                        for _ in from_win..len {
                            out[o] = out[o - dist];
                            o += 1;
                        }
                        continue;
                    }
                    copy_match_fast(out, o, dist, len);
                    o += len;
                }
                K_EOB => {
                    hold >>= e_len(e);
                    bits -= e_len(e);
                    eob = true;
                    break;
                }
                K_LIT => unreachable!(),
                _ => {
                    err = Some("invalid literal/length code");
                    break;
                }
            }
        }
        out.truncate(o);
        // Give back whole unused bytes.
        let unused = (bits >> 3) as usize;
        pos -= unused;
        bits &= 7;
        hold &= (1u64 << bits) - 1;
        io.pos = pos;
        self.hold = hold;
        self.bits = bits;
        if let Some(m) = err {
            self.bad(m);
        } else if eob {
            self.mode = Mode::Type;
        }
    }
}

/// Copies `len` bytes from `dist` back, within `out` (which has room).
#[inline(always)]
fn copy_match_fast(out: &mut [u8], o: usize, dist: usize, len: usize) {
    if dist >= len {
        out.copy_within(o - dist..o - dist + len, o);
    } else if dist == 1 {
        let b = out[o - 1];
        out[o..o + len].fill(b);
    } else if dist >= 8 {
        let mut i = 0;
        while i < len {
            let n = (len - i).min(dist);
            out.copy_within(o - dist + i..o - dist + i + n, o + i);
            i += n;
        }
    } else {
        for i in 0..len {
            out[o + i] = out[o + i - dist];
        }
    }
}

/// Appends `len` bytes copied from `dist` back in `out`.
fn copy_within_out(out: &mut Vec<u8>, dist: usize, len: usize) {
    let o = out.len();
    out.resize(o + len, 0);
    copy_match_fast(out, o, dist, len);
}
