//! inflate.c from zlib 1.3.1 (GUNZIP enabled; the inflate_fast() shortcut is
//! not used, the generic state machine produces the same results).

use crate::checksum::{adler32, crc32};
use crate::inftrees::Code;
use crate::inftrees::{CodeType, ENOUGH, inflate_table};
use crate::*;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Mode {
    Head,
    Flags,
    Time,
    Os,
    Exlen,
    Extra,
    Name,
    Comment,
    Hcrc,
    DictId,
    Dict,
    Type,
    TypeDo,
    Stored,
    Copy_,
    Copy,
    Table,
    LenLens,
    CodeLens,
    Len_,
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum Table {
    Fixed,
    FixedDist,
    Dyn(usize),
}

struct Fixed {
    codes: [Code; 544],
    dist_off: usize,
}

fn fixed() -> &'static Fixed {
    static F: std::sync::OnceLock<Fixed> = std::sync::OnceLock::new();
    F.get_or_init(|| {
        let mut lens = [0u16; 320];
        let mut work = [0u16; 288];
        let mut codes = [Code::default(); 544];
        for (sym, l) in lens.iter_mut().enumerate().take(288) {
            *l = if sym < 144 {
                8
            } else if sym < 256 {
                9
            } else if sym < 280 {
                7
            } else {
                8
            };
        }
        let mut next = 0usize;
        let mut bits = 9u32;
        inflate_table(
            CodeType::Lens,
            &lens,
            288,
            &mut codes,
            &mut next,
            &mut bits,
            &mut work,
        );
        let dist_off = next;
        for l in lens.iter_mut().take(32) {
            *l = 5;
        }
        let mut bits = 5u32;
        inflate_table(
            CodeType::Dists,
            &lens,
            32,
            &mut codes,
            &mut next,
            &mut bits,
            &mut work,
        );
        Fixed { codes, dist_off }
    })
}

const ORDER: [usize; 19] = [
    16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
];

/// An inflate stream: z_stream + inflate_state.
pub struct Inflate {
    pub total_in: u64,
    pub total_out: u64,
    pub adler: u32,
    pub data_type: i32,
    pub msg: Option<&'static str>,

    mode: Mode,
    last: bool,
    wrap: i32,
    havedict: bool,
    flags: i32,
    dmax: u32,
    check: u32,
    total: u64,
    wbits: u32,
    wsize: u32,
    whave: u32,
    wnext: u32,
    window: Vec<u8>,
    hold: u64,
    bits: u32,
    length: u32,
    offset: u32,
    extra: u32,
    lencode: Table,
    distcode: Table,
    lenbits: u32,
    distbits: u32,
    ncode: u32,
    nlen: u32,
    ndist: u32,
    have: u32,
    next: usize,
    lens: [u16; 320],
    work: [u16; 288],
    codes: Vec<Code>,
    sane: bool,
    back: i32,
    was: u32,
}

impl Inflate {
    /// inflateInit2()
    pub fn new(window_bits: i32) -> Result<Box<Inflate>, i32> {
        let mut s = Box::new(Inflate {
            total_in: 0,
            total_out: 0,
            adler: 0,
            data_type: 0,
            msg: None,
            mode: Mode::Head,
            last: false,
            wrap: 0,
            havedict: false,
            flags: -1,
            dmax: 32768,
            check: 0,
            total: 0,
            wbits: 0,
            wsize: 0,
            whave: 0,
            wnext: 0,
            window: Vec::new(),
            hold: 0,
            bits: 0,
            length: 0,
            offset: 0,
            extra: 0,
            lencode: Table::Dyn(0),
            distcode: Table::Dyn(0),
            lenbits: 0,
            distbits: 0,
            ncode: 0,
            nlen: 0,
            ndist: 0,
            have: 0,
            next: 0,
            lens: [0; 320],
            work: [0; 288],
            codes: vec![Code::default(); ENOUGH],
            sane: true,
            back: -1,
            was: 0,
        });
        let r = s.reset2(window_bits);
        if r != Z_OK {
            return Err(r);
        }
        Ok(s)
    }

    /// inflateResetKeep()
    pub fn reset_keep(&mut self) {
        self.total_in = 0;
        self.total_out = 0;
        self.total = 0;
        self.msg = None;
        if self.wrap != 0 {
            self.adler = (self.wrap & 1) as u32;
        }
        self.mode = Mode::Head;
        self.last = false;
        self.havedict = false;
        self.flags = -1;
        self.dmax = 32768;
        self.hold = 0;
        self.bits = 0;
        self.lencode = Table::Dyn(0);
        self.distcode = Table::Dyn(0);
        self.next = 0;
        self.sane = true;
        self.back = -1;
    }

    /// inflateReset()
    pub fn reset(&mut self) {
        self.wsize = 0;
        self.whave = 0;
        self.wnext = 0;
        self.reset_keep();
    }

    /// inflateReset2()
    pub fn reset2(&mut self, window_bits: i32) -> i32 {
        let mut window_bits = window_bits;
        let wrap;
        if window_bits < 0 {
            if window_bits < -15 {
                return Z_STREAM_ERROR;
            }
            wrap = 0;
            window_bits = -window_bits;
        } else {
            wrap = (window_bits >> 4) + 5;
            if window_bits < 48 {
                window_bits &= 15;
            }
        }
        if window_bits != 0 && !(8..=15).contains(&window_bits) {
            return Z_STREAM_ERROR;
        }
        if !self.window.is_empty() && self.wbits != window_bits as u32 {
            self.window = Vec::new();
        }
        self.wrap = wrap;
        self.wbits = window_bits as u32;
        self.reset();
        Z_OK
    }

    /// Whether the stream has reached its end (mode DONE).
    pub fn is_done(&self) -> bool {
        self.mode == Mode::Done
    }

    fn updatewindow(&mut self, src: &[u8]) {
        if self.window.is_empty() {
            self.window = vec![0u8; 1usize << self.wbits];
        }
        if self.wsize == 0 {
            self.wsize = 1u32 << self.wbits;
            self.wnext = 0;
            self.whave = 0;
        }
        let copy = src.len() as u32;
        let end = src.len();
        if copy >= self.wsize {
            let ws = self.wsize as usize;
            self.window[..ws].copy_from_slice(&src[end - ws..end]);
            self.wnext = 0;
            self.whave = self.wsize;
        } else {
            let mut dist = self.wsize - self.wnext;
            if dist > copy {
                dist = copy;
            }
            let wn = self.wnext as usize;
            self.window[wn..wn + dist as usize]
                .copy_from_slice(&src[end - copy as usize..end - copy as usize + dist as usize]);
            let copy = copy - dist;
            if copy != 0 {
                self.window[..copy as usize].copy_from_slice(&src[end - copy as usize..end]);
                self.wnext = copy;
                self.whave = self.wsize;
            } else {
                self.wnext += dist;
                if self.wnext == self.wsize {
                    self.wnext = 0;
                }
                if self.whave < self.wsize {
                    self.whave += dist;
                }
            }
        }
    }

    /// inflateSetDictionary()
    pub fn set_dictionary(&mut self, dictionary: &[u8]) -> i32 {
        if self.wrap != 0 && self.mode != Mode::Dict {
            return Z_STREAM_ERROR;
        }
        if self.mode == Mode::Dict {
            let dictid = adler32(1, dictionary);
            if dictid != self.check {
                return Z_DATA_ERROR;
            }
        }
        self.updatewindow(dictionary);
        self.havedict = true;
        Z_OK
    }

    #[inline]
    fn code_at(&self, t: Table, idx: usize) -> Code {
        match t {
            Table::Fixed => fixed().codes[idx],
            Table::FixedDist => {
                let f = fixed();
                f.codes[f.dist_off + idx]
            }
            Table::Dyn(off) => self.codes[off + idx],
        }
    }

    #[inline]
    fn update_check(&self, check: u32, buf: &[u8]) -> u32 {
        if self.flags != 0 {
            crc32(check, buf)
        } else {
            adler32(check, buf)
        }
    }

    /// inflate()
    pub fn inflate(&mut self, io: &mut Io, flush: i32) -> i32 {
        if self.mode == Mode::Type {
            self.mode = Mode::TypeDo;
        }
        let mut next = io.in_pos;
        let mut have = io.avail_in() as u32;
        let mut put = io.out_pos;
        let mut left = io.avail_out() as u32;
        let mut hold = self.hold;
        let mut bits = self.bits;
        let in_ = have;
        let mut out = left;
        let mut ret = Z_OK;

        'leave: loop {
            macro_rules! pullbyte {
                () => {{
                    if have == 0 {
                        break 'leave;
                    }
                    have -= 1;
                    hold += (io.input[next] as u64) << bits;
                    next += 1;
                    bits += 8;
                }};
            }
            macro_rules! needbits {
                ($n:expr) => {{
                    while bits < ($n) as u32 {
                        pullbyte!();
                    }
                }};
            }
            macro_rules! getbits {
                ($n:expr) => {
                    (hold as u32) & ((1u32 << ($n)) - 1)
                };
            }
            macro_rules! dropbits {
                ($n:expr) => {{
                    hold >>= $n;
                    bits -= ($n) as u32;
                }};
            }
            macro_rules! initbits {
                () => {{
                    hold = 0;
                    bits = 0;
                }};
            }
            macro_rules! bytebits {
                () => {{
                    hold >>= bits & 7;
                    bits -= bits & 7;
                }};
            }
            macro_rules! crc2 {
                ($w:expr) => {{
                    let w = $w;
                    self.check = crc32(self.check, &[w as u8, (w >> 8) as u8]);
                }};
            }
            macro_rules! crc4 {
                ($w:expr) => {{
                    let w = $w;
                    self.check = crc32(
                        self.check,
                        &[w as u8, (w >> 8) as u8, (w >> 16) as u8, (w >> 24) as u8],
                    );
                }};
            }

            match self.mode {
                Mode::Head => {
                    if self.wrap == 0 {
                        self.mode = Mode::TypeDo;
                        continue;
                    }
                    needbits!(16);
                    if (self.wrap & 2) != 0 && hold == 0x8b1f {
                        if self.wbits == 0 {
                            self.wbits = 15;
                        }
                        self.check = 0;
                        crc2!(hold);
                        initbits!();
                        self.mode = Mode::Flags;
                        continue;
                    }
                    if (self.wrap & 1) == 0
                        || !((getbits!(8) << 8) + (hold >> 8) as u32).is_multiple_of(31)
                    {
                        self.msg = Some("incorrect header check");
                        self.mode = Mode::Bad;
                        continue;
                    }
                    if getbits!(4) != Z_DEFLATED as u32 {
                        self.msg = Some("unknown compression method");
                        self.mode = Mode::Bad;
                        continue;
                    }
                    dropbits!(4);
                    let len = getbits!(4) + 8;
                    if self.wbits == 0 {
                        self.wbits = len;
                    }
                    if len > 15 || len > self.wbits {
                        self.msg = Some("invalid window size");
                        self.mode = Mode::Bad;
                        continue;
                    }
                    self.dmax = 1u32 << len;
                    self.flags = 0;
                    self.check = 1;
                    self.adler = 1;
                    self.mode = if hold & 0x200 != 0 {
                        Mode::DictId
                    } else {
                        Mode::Type
                    };
                    initbits!();
                }
                Mode::Flags => {
                    needbits!(16);
                    self.flags = hold as i32;
                    if (self.flags & 0xff) != Z_DEFLATED {
                        self.msg = Some("unknown compression method");
                        self.mode = Mode::Bad;
                        continue;
                    }
                    if self.flags & 0xe000 != 0 {
                        self.msg = Some("unknown header flags set");
                        self.mode = Mode::Bad;
                        continue;
                    }
                    if (self.flags & 0x0200) != 0 && (self.wrap & 4) != 0 {
                        crc2!(hold);
                    }
                    initbits!();
                    self.mode = Mode::Time;
                }
                Mode::Time => {
                    needbits!(32);
                    if (self.flags & 0x0200) != 0 && (self.wrap & 4) != 0 {
                        crc4!(hold);
                    }
                    initbits!();
                    self.mode = Mode::Os;
                }
                Mode::Os => {
                    needbits!(16);
                    if (self.flags & 0x0200) != 0 && (self.wrap & 4) != 0 {
                        crc2!(hold);
                    }
                    initbits!();
                    self.mode = Mode::Exlen;
                }
                Mode::Exlen => {
                    if self.flags & 0x0400 != 0 {
                        needbits!(16);
                        self.length = hold as u32;
                        if (self.flags & 0x0200) != 0 && (self.wrap & 4) != 0 {
                            crc2!(hold);
                        }
                        initbits!();
                    }
                    self.mode = Mode::Extra;
                }
                Mode::Extra => {
                    if self.flags & 0x0400 != 0 {
                        let mut copy = self.length;
                        if copy > have {
                            copy = have;
                        }
                        if copy != 0 {
                            if (self.flags & 0x0200) != 0 && (self.wrap & 4) != 0 {
                                self.check =
                                    crc32(self.check, &io.input[next..next + copy as usize]);
                            }
                            have -= copy;
                            next += copy as usize;
                            self.length -= copy;
                        }
                        if self.length != 0 {
                            break 'leave;
                        }
                    }
                    self.length = 0;
                    self.mode = Mode::Name;
                }
                Mode::Name | Mode::Comment => {
                    let flag = if self.mode == Mode::Name {
                        0x0800
                    } else {
                        0x1000
                    };
                    if self.flags & flag != 0 {
                        if have == 0 {
                            break 'leave;
                        }
                        let mut copy = 0u32;
                        let mut len;
                        loop {
                            len = io.input[next + copy as usize];
                            copy += 1;
                            if !(len != 0 && copy < have) {
                                break;
                            }
                        }
                        if (self.flags & 0x0200) != 0 && (self.wrap & 4) != 0 {
                            self.check = crc32(self.check, &io.input[next..next + copy as usize]);
                        }
                        have -= copy;
                        next += copy as usize;
                        if len != 0 {
                            break 'leave;
                        }
                    }
                    self.length = 0;
                    self.mode = if self.mode == Mode::Name {
                        Mode::Comment
                    } else {
                        Mode::Hcrc
                    };
                }
                Mode::Hcrc => {
                    if self.flags & 0x0200 != 0 {
                        needbits!(16);
                        if (self.wrap & 4) != 0 && hold != (self.check & 0xffff) as u64 {
                            self.msg = Some("header crc mismatch");
                            self.mode = Mode::Bad;
                            continue;
                        }
                        initbits!();
                    }
                    self.check = 0;
                    self.adler = 0;
                    self.mode = Mode::Type;
                }
                Mode::DictId => {
                    needbits!(32);
                    let v = (hold as u32).swap_bytes();
                    self.check = v;
                    self.adler = v;
                    initbits!();
                    self.mode = Mode::Dict;
                }
                Mode::Dict => {
                    if !self.havedict {
                        io.out_pos = put;
                        io.in_pos = next;
                        self.hold = hold;
                        self.bits = bits;
                        return Z_NEED_DICT;
                    }
                    self.check = 1;
                    self.adler = 1;
                    self.mode = Mode::Type;
                }
                Mode::Type | Mode::TypeDo => {
                    if self.mode == Mode::Type && (flush == Z_BLOCK || flush == Z_TREES) {
                        break 'leave;
                    }
                    if self.last {
                        bytebits!();
                        self.mode = Mode::Check;
                        continue;
                    }
                    needbits!(3);
                    self.last = getbits!(1) != 0;
                    dropbits!(1);
                    match getbits!(2) {
                        0 => self.mode = Mode::Stored,
                        1 => {
                            self.lencode = Table::Fixed;
                            self.lenbits = 9;
                            self.distcode = Table::FixedDist;
                            self.distbits = 5;
                            self.mode = Mode::Len_;
                            if flush == Z_TREES {
                                dropbits!(2);
                                break 'leave;
                            }
                        }
                        2 => self.mode = Mode::Table,
                        _ => {
                            self.msg = Some("invalid block type");
                            self.mode = Mode::Bad;
                        }
                    }
                    dropbits!(2);
                }
                Mode::Stored => {
                    bytebits!();
                    needbits!(32);
                    if (hold & 0xffff) != ((hold >> 16) ^ 0xffff) {
                        self.msg = Some("invalid stored block lengths");
                        self.mode = Mode::Bad;
                        continue;
                    }
                    self.length = (hold as u32) & 0xffff;
                    initbits!();
                    self.mode = Mode::Copy_;
                    if flush == Z_TREES {
                        break 'leave;
                    }
                }
                Mode::Copy_ => {
                    self.mode = Mode::Copy;
                }
                Mode::Copy => {
                    let mut copy = self.length;
                    if copy != 0 {
                        if copy > have {
                            copy = have;
                        }
                        if copy > left {
                            copy = left;
                        }
                        if copy == 0 {
                            break 'leave;
                        }
                        let c = copy as usize;
                        io.output[put..put + c].copy_from_slice(&io.input[next..next + c]);
                        have -= copy;
                        next += c;
                        left -= copy;
                        put += c;
                        self.length -= copy;
                        continue;
                    }
                    self.mode = Mode::Type;
                }
                Mode::Table => {
                    needbits!(14);
                    self.nlen = getbits!(5) + 257;
                    dropbits!(5);
                    self.ndist = getbits!(5) + 1;
                    dropbits!(5);
                    self.ncode = getbits!(4) + 4;
                    dropbits!(4);
                    if self.nlen > 286 || self.ndist > 30 {
                        self.msg = Some("too many length or distance symbols");
                        self.mode = Mode::Bad;
                        continue;
                    }
                    self.have = 0;
                    self.mode = Mode::LenLens;
                }
                Mode::LenLens => {
                    while self.have < self.ncode {
                        needbits!(3);
                        self.lens[ORDER[self.have as usize]] = getbits!(3) as u16;
                        self.have += 1;
                        dropbits!(3);
                    }
                    while self.have < 19 {
                        self.lens[ORDER[self.have as usize]] = 0;
                        self.have += 1;
                    }
                    self.next = 0;
                    self.lencode = Table::Dyn(0);
                    self.lenbits = 7;
                    let r = inflate_table(
                        CodeType::Codes,
                        &self.lens,
                        19,
                        &mut self.codes,
                        &mut self.next,
                        &mut self.lenbits,
                        &mut self.work,
                    );
                    if r != 0 {
                        self.msg = Some("invalid code lengths set");
                        self.mode = Mode::Bad;
                        continue;
                    }
                    self.have = 0;
                    self.mode = Mode::CodeLens;
                }
                Mode::CodeLens => {
                    let mut bad = false;
                    while self.have < self.nlen + self.ndist {
                        let mut here;
                        loop {
                            here = self.code_at(self.lencode, getbits!(self.lenbits) as usize);
                            if here.bits as u32 <= bits {
                                break;
                            }
                            pullbyte!();
                        }
                        if here.val < 16 {
                            dropbits!(here.bits);
                            self.lens[self.have as usize] = here.val;
                            self.have += 1;
                        } else {
                            let len;
                            let mut copy;
                            if here.val == 16 {
                                needbits!(here.bits as u32 + 2);
                                dropbits!(here.bits);
                                if self.have == 0 {
                                    self.msg = Some("invalid bit length repeat");
                                    self.mode = Mode::Bad;
                                    bad = true;
                                    break;
                                }
                                len = self.lens[(self.have - 1) as usize];
                                copy = 3 + getbits!(2);
                                dropbits!(2);
                            } else if here.val == 17 {
                                needbits!(here.bits as u32 + 3);
                                dropbits!(here.bits);
                                len = 0;
                                copy = 3 + getbits!(3);
                                dropbits!(3);
                            } else {
                                needbits!(here.bits as u32 + 7);
                                dropbits!(here.bits);
                                len = 0;
                                copy = 11 + getbits!(7);
                                dropbits!(7);
                            }
                            if self.have + copy > self.nlen + self.ndist {
                                self.msg = Some("invalid bit length repeat");
                                self.mode = Mode::Bad;
                                bad = true;
                                break;
                            }
                            while copy > 0 {
                                copy -= 1;
                                self.lens[self.have as usize] = len;
                                self.have += 1;
                            }
                        }
                    }
                    if bad {
                        continue;
                    }
                    if self.lens[256] == 0 {
                        self.msg = Some("invalid code -- missing end-of-block");
                        self.mode = Mode::Bad;
                        continue;
                    }
                    self.next = 0;
                    self.lencode = Table::Dyn(0);
                    self.lenbits = 9;
                    let r = inflate_table(
                        CodeType::Lens,
                        &self.lens,
                        self.nlen as usize,
                        &mut self.codes,
                        &mut self.next,
                        &mut self.lenbits,
                        &mut self.work,
                    );
                    if r != 0 {
                        self.msg = Some("invalid literal/lengths set");
                        self.mode = Mode::Bad;
                        continue;
                    }
                    self.distcode = Table::Dyn(self.next);
                    self.distbits = 6;
                    let nlen = self.nlen as usize;
                    let lens_d: Vec<u16> = self.lens[nlen..nlen + self.ndist as usize].to_vec();
                    let r = inflate_table(
                        CodeType::Dists,
                        &lens_d,
                        self.ndist as usize,
                        &mut self.codes,
                        &mut self.next,
                        &mut self.distbits,
                        &mut self.work,
                    );
                    if r != 0 {
                        self.msg = Some("invalid distances set");
                        self.mode = Mode::Bad;
                        continue;
                    }
                    self.mode = Mode::Len_;
                    if flush == Z_TREES {
                        break 'leave;
                    }
                }
                Mode::Len_ => {
                    self.mode = Mode::Len;
                }
                Mode::Len => {
                    self.back = 0;
                    let mut here;
                    loop {
                        here = self.code_at(self.lencode, getbits!(self.lenbits) as usize);
                        if here.bits as u32 <= bits {
                            break;
                        }
                        pullbyte!();
                    }
                    if here.op != 0 && (here.op & 0xf0) == 0 {
                        let last = here;
                        loop {
                            here = self.code_at(
                                self.lencode,
                                last.val as usize
                                    + (getbits!(last.bits as u32 + last.op as u32) >> last.bits)
                                        as usize,
                            );
                            if (last.bits as u32 + here.bits as u32) <= bits {
                                break;
                            }
                            pullbyte!();
                        }
                        dropbits!(last.bits);
                        self.back += last.bits as i32;
                    }
                    dropbits!(here.bits);
                    self.back += here.bits as i32;
                    self.length = here.val as u32;
                    if here.op == 0 {
                        self.mode = Mode::Lit;
                        continue;
                    }
                    if here.op & 32 != 0 {
                        self.back = -1;
                        self.mode = Mode::Type;
                        continue;
                    }
                    if here.op & 64 != 0 {
                        self.msg = Some("invalid literal/length code");
                        self.mode = Mode::Bad;
                        continue;
                    }
                    self.extra = (here.op & 15) as u32;
                    self.mode = Mode::LenExt;
                }
                Mode::LenExt => {
                    if self.extra != 0 {
                        needbits!(self.extra);
                        self.length += getbits!(self.extra);
                        dropbits!(self.extra);
                        self.back += self.extra as i32;
                    }
                    self.was = self.length;
                    self.mode = Mode::Dist;
                }
                Mode::Dist => {
                    let mut here;
                    loop {
                        here = self.code_at(self.distcode, getbits!(self.distbits) as usize);
                        if here.bits as u32 <= bits {
                            break;
                        }
                        pullbyte!();
                    }
                    if (here.op & 0xf0) == 0 {
                        let last = here;
                        loop {
                            here = self.code_at(
                                self.distcode,
                                last.val as usize
                                    + (getbits!(last.bits as u32 + last.op as u32) >> last.bits)
                                        as usize,
                            );
                            if (last.bits as u32 + here.bits as u32) <= bits {
                                break;
                            }
                            pullbyte!();
                        }
                        dropbits!(last.bits);
                        self.back += last.bits as i32;
                    }
                    dropbits!(here.bits);
                    self.back += here.bits as i32;
                    if here.op & 64 != 0 {
                        self.msg = Some("invalid distance code");
                        self.mode = Mode::Bad;
                        continue;
                    }
                    self.offset = here.val as u32;
                    self.extra = (here.op & 15) as u32;
                    self.mode = Mode::DistExt;
                }
                Mode::DistExt => {
                    if self.extra != 0 {
                        needbits!(self.extra);
                        self.offset += getbits!(self.extra);
                        dropbits!(self.extra);
                        self.back += self.extra as i32;
                    }
                    self.mode = Mode::Match;
                }
                Mode::Match => {
                    if left == 0 {
                        break 'leave;
                    }
                    let mut copy = out - left;
                    if self.offset > copy {
                        copy = self.offset - copy;
                        if copy > self.whave && self.sane {
                            self.msg = Some("invalid distance too far back");
                            self.mode = Mode::Bad;
                            continue;
                        }
                        let from = if copy > self.wnext {
                            copy -= self.wnext;
                            (self.wsize - copy) as usize
                        } else {
                            (self.wnext - copy) as usize
                        };
                        if copy > self.length {
                            copy = self.length;
                        }
                        if copy > left {
                            copy = left;
                        }
                        left -= copy;
                        self.length -= copy;
                        let c = copy as usize;
                        io.output[put..put + c].copy_from_slice(&self.window[from..from + c]);
                        put += c;
                    } else {
                        let mut from = put - self.offset as usize;
                        copy = self.length;
                        if copy > left {
                            copy = left;
                        }
                        left -= copy;
                        self.length -= copy;
                        for _ in 0..copy {
                            io.output[put] = io.output[from];
                            put += 1;
                            from += 1;
                        }
                    }
                    if self.length == 0 {
                        self.mode = Mode::Len;
                    }
                }
                Mode::Lit => {
                    if left == 0 {
                        break 'leave;
                    }
                    io.output[put] = self.length as u8;
                    put += 1;
                    left -= 1;
                    self.mode = Mode::Len;
                }
                Mode::Check => {
                    if self.wrap != 0 {
                        needbits!(32);
                        out -= left;
                        self.total_out += out as u64;
                        self.total += out as u64;
                        if (self.wrap & 4) != 0 && out != 0 {
                            let c =
                                self.update_check(self.check, &io.output[put - out as usize..put]);
                            self.check = c;
                            self.adler = c;
                        }
                        out = left;
                        let v = if self.flags != 0 {
                            hold as u32
                        } else {
                            (hold as u32).swap_bytes()
                        };
                        if (self.wrap & 4) != 0 && v != self.check {
                            self.msg = Some("incorrect data check");
                            self.mode = Mode::Bad;
                            continue;
                        }
                        initbits!();
                    }
                    self.mode = Mode::Length;
                }
                Mode::Length => {
                    if self.wrap != 0 && self.flags != 0 {
                        needbits!(32);
                        if (self.wrap & 4) != 0 && hold != (self.total & 0xffffffff) {
                            self.msg = Some("incorrect length check");
                            self.mode = Mode::Bad;
                            continue;
                        }
                        initbits!();
                    }
                    self.mode = Mode::Done;
                }
                Mode::Done => {
                    ret = Z_STREAM_END;
                    break 'leave;
                }
                Mode::Bad => {
                    ret = Z_DATA_ERROR;
                    break 'leave;
                }
            }
        }

        // inf_leave:
        io.out_pos = put;
        io.in_pos = next;
        self.hold = hold;
        self.bits = bits;
        let avail_out = left;
        if self.wsize != 0
            || (out != avail_out
                && self.mode < Mode::Bad
                && (self.mode < Mode::Check || flush != Z_FINISH))
        {
            let n = (out - avail_out) as usize;
            let written: Vec<u8> = io.output[put - n..put].to_vec();
            self.updatewindow(&written);
        }
        let in_used = in_ - have;
        let out_used = out - avail_out;
        self.total_in += in_used as u64;
        self.total_out += out_used as u64;
        self.total += out_used as u64;
        if (self.wrap & 4) != 0 && out_used != 0 {
            let c = self.update_check(self.check, &io.output[put - out_used as usize..put]);
            self.check = c;
            self.adler = c;
        }
        self.data_type = self.bits as i32
            + if self.last { 64 } else { 0 }
            + if self.mode == Mode::Type { 128 } else { 0 }
            + if self.mode == Mode::Len_ || self.mode == Mode::Copy_ {
                256
            } else {
                0
            };
        if ((in_used == 0 && out_used == 0) || flush == Z_FINISH) && ret == Z_OK {
            ret = Z_BUF_ERROR;
        }
        ret
    }
}
