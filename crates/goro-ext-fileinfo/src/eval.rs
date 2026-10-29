//! Evaluates parsed magic(5) entries against a buffer.
//!
//! Semantics follow the magic(5) manual page (continuation levels,
//! relative and indirect offsets, `default`/`clear`, `name`/`use`,
//! `indirect`) and observations of PHP 8.5's finfo: the first mime type
//! set by a matching line wins, entries that print nothing do not count as
//! a match, negative offsets only work on files, `indirect` recursion
//! stops after 50 levels with "indirect count (50) exceeded".

use crate::magic::{
    DateStyle, Endian, Entry, Kind, Line, Magic, SF_LC, SF_LINES, SF_REL, SF_START, SF_SW, SF_TRIM,
    SF_UC, SF_W,
};

/// What one matching entry produced.
#[derive(Default, Debug, Clone)]
pub struct Found {
    pub desc: Vec<u8>,
    pub mime: Option<Vec<u8>>,
    pub apple: Option<Vec<u8>>,
    pub ext: Option<Vec<u8>>,
}

impl Found {
    fn print(&mut self, s: &[u8]) {
        if s.is_empty() {
            return;
        }
        if let Some(r) = s.strip_prefix(b"\\b") {
            self.desc.extend_from_slice(r);
        } else if let Some(r) = s.strip_prefix(b"\x08") {
            self.desc.extend_from_slice(r);
        } else {
            if !self.desc.is_empty() {
                self.desc.push(b' ');
            }
            self.desc.extend_from_slice(s);
        }
    }
    fn meta(&mut self, l: &Line) {
        if self.mime.is_none() {
            self.mime = l.mime.clone();
        }
        if self.apple.is_none() {
            self.apple = l.apple.clone();
        }
        if self.ext.is_none() {
            self.ext = l.ext.clone();
        }
    }
}

pub struct Ctx<'a> {
    pub db: &'a Magic,
    pub buf: &'a [u8],
    /// Negative offsets count from the end (finfo_file only).
    pub file_mode: bool,
    pub indirect: u32,
    pub names: u32,
    /// Inside `use ^name`: little and big endian are swapped.
    pub flip: bool,
}

/// A value read for a test, kept for printing.
enum Val {
    Int(u64, u8, bool),
    Float(f64),
    Bytes(Vec<u8>),
    Date(u64, u8, DateStyle),
    None,
}

fn read_uint(b: &[u8], at: usize, size: u8, e: Endian) -> Option<u64> {
    let n = size as usize;
    let s = b.get(at..at.checked_add(n)?)?;
    Some(match (n, e) {
        (1, _) => s[0] as u64,
        (2, Endian::Big) => u16::from_be_bytes([s[0], s[1]]) as u64,
        (2, _) => u16::from_le_bytes([s[0], s[1]]) as u64,
        (4, Endian::Big) => u32::from_be_bytes([s[0], s[1], s[2], s[3]]) as u64,
        (4, Endian::Little) => u32::from_le_bytes([s[0], s[1], s[2], s[3]]) as u64,
        (4, Endian::Middle) => u32::from_be_bytes([s[1], s[0], s[3], s[2]]) as u64,
        (8, Endian::Big) => u64::from_be_bytes(s.try_into().ok()?),
        (8, _) => u64::from_le_bytes(s.try_into().ok()?),
        _ => return None,
    })
}

fn sext(v: u64, size: u8) -> i64 {
    match size {
        1 => v as u8 as i8 as i64,
        2 => v as u16 as i16 as i64,
        4 => v as u32 as i32 as i64,
        _ => v as i64,
    }
}

fn mask(size: u8) -> u64 {
    if size >= 8 {
        u64::MAX
    } else {
        (1u64 << (size as u32 * 8)) - 1
    }
}

fn apply_op(v: u64, op: u8, n: u64) -> u64 {
    match op {
        b'&' => v & n,
        b'|' => v | n,
        b'^' => v ^ n,
        b'+' => v.wrapping_add(n),
        b'-' => v.wrapping_sub(n),
        b'*' => v.wrapping_mul(n),
        b'/' => v.checked_div(n).unwrap_or(v),
        b'%' => v.checked_rem(n).unwrap_or(v),
        _ => v,
    }
}

fn id3(v: u64) -> u64 {
    ((v >> 24) & 0x7f) << 21 | ((v >> 16) & 0x7f) << 14 | ((v >> 8) & 0x7f) << 7 | (v & 0x7f)
}

/// Nesting limit of `use` (TIFF directories chain one call per entry).
const MAX_USE: u32 = 256;

pub enum Stop {
    /// "Failed identify data": the message after the errno.
    Error(String),
}

type Res<T> = Result<T, Stop>;

impl<'a> Ctx<'a> {
    pub fn new(db: &'a Magic, buf: &'a [u8], file_mode: bool) -> Ctx<'a> {
        Ctx {
            db,
            buf,
            file_mode,
            indirect: 0,
            names: 0,
            flip: false,
        }
    }

    /// Runs the binary (`text == false`) or text entries in order; returns
    /// the first match, or all of them with `all`.
    pub fn run(&mut self, text: bool, all: bool, out: &mut Vec<Found>) -> Res<()> {
        let db = self.db;
        for e in &db.entries {
            if e.text != text {
                continue;
            }
            let mut f = Found::default();
            if self.run_entry(e, 0, &mut f)? && !f.desc.is_empty() {
                out.push(f);
                if !all {
                    break;
                }
            }
        }
        Ok(())
    }

    /// The byte order of a read, swapped inside `use ^name` (magic(5):
    /// "leshort is treated as beshort and vice versa").
    fn en(&self, e: Endian) -> Endian {
        match (self.flip, e) {
            (true, Endian::Little) => Endian::Big,
            (true, Endian::Big) => Endian::Little,
            _ => e,
        }
    }

    fn abs_off(&self, base: i64, rel: Option<usize>, o: i64) -> Option<usize> {
        self.abs_off_end(base, rel, o, false)
    }

    fn abs_off_end(&self, base: i64, rel: Option<usize>, o: i64, from_end: bool) -> Option<usize> {
        if let Some(r) = rel {
            let v = (r as i64).wrapping_add(o);
            return (v >= 0).then_some(v as usize);
        }
        if o < 0 || from_end {
            if !self.file_mode {
                return None;
            }
            let v = (self.buf.len() as i64).wrapping_add(o);
            return (v >= 0).then_some(v as usize);
        }
        let v = base.wrapping_add(o);
        (v >= 0).then_some(v as usize)
    }

    fn resolve(&self, l: &Line, base: usize, parent_end: usize) -> Option<usize> {
        let off = &l.off;
        let Some(ind) = &off.ind else {
            return self.abs_off_end(
                base as i64,
                off.rel.then_some(parent_end),
                off.base,
                off.from_end,
            );
        };
        // The value is read relative to a named rule's base; it is an
        // absolute offset.
        let at = self.abs_off(base as i64, ind.rel.then_some(parent_end), ind.base)?;
        let mut v = if ind.special == b'o' {
            let s = self.buf.get(at..)?;
            let n = s.iter().take_while(|c| (b'0'..=b'7').contains(c)).count();
            let mut x = 0u64;
            for &c in &s[..n] {
                x = x * 8 + (c - b'0') as u64;
            }
            x
        } else {
            let raw = read_uint(self.buf, at, ind.size, self.en(ind.endian))?;
            let raw = if ind.special == b'i' { id3(raw) } else { raw };
            if ind.signed {
                sext(raw, ind.size) as u64
            } else {
                raw
            }
        };
        if ind.op != 0 {
            let n = if ind.operand_ind {
                let p = self.abs_off(base as i64, None, ind.operand)?;
                let r = read_uint(self.buf, p, ind.size, self.en(ind.endian))?;
                if ind.signed {
                    sext(r, ind.size) as u64
                } else {
                    r
                }
            } else {
                ind.operand as u64
            };
            v = if ind.op == b'~' {
                !v
            } else {
                apply_op(v, ind.op, n)
            };
        }
        let mut o = (v as i64).wrapping_add(off.base);
        if off.rel {
            o = o.wrapping_add(parent_end as i64);
        }
        (o >= 0).then_some(o as usize)
    }

    /// Runs one entry from `base`; true if its first line matched.
    fn run_entry(&mut self, e: &Entry, base: usize, f: &mut Found) -> Res<bool> {
        self.run_lines(&e.lines, base, f, false)
    }

    /// `named`: the first line is a `name` line matched at `base`.
    fn run_lines(&mut self, lines: &[Line], base: usize, f: &mut Found, named: bool) -> Res<bool> {
        const MAXL: usize = 64;
        let mut ends = [0usize; MAXL + 1];
        let mut has = [false; MAXL + 2];
        let mut cont = 0usize;
        let mut any = false;
        for (i, l) in lines.iter().enumerate() {
            let lv = l.level.min(MAXL);
            if lv > cont {
                continue;
            }
            cont = lv;
            if i == 0 && named {
                ends[0] = base;
                has[1] = false;
                cont = 1;
                continue;
            }
            let parent_end = if lv == 0 { base } else { ends[lv - 1] };
            let m = match l.kind {
                Kind::Default => {
                    if has[lv] {
                        None
                    } else {
                        self.resolve(l, base, parent_end).map(|o| (o, Val::None))
                    }
                }
                Kind::Clear => {
                    has[lv] = false;
                    self.resolve(l, base, parent_end).map(|o| (o, Val::None))
                }
                Kind::Use => {
                    let Some(mut o) = self.resolve(l, base, parent_end) else {
                        if lv == 0 {
                            return Ok(false);
                        }
                        continue;
                    };
                    // An indirect `use` offset is from the current rule's
                    // base, so rules can walk chained records (the JPEG
                    // segment walk of the magic database tests ship).
                    if l.off.ind.is_some() && !l.off.rel {
                        o = o.saturating_add(base);
                    }
                    let Some(ne) = self.db.names.get(&l.sval) else {
                        return Err(Stop::Error(format!(
                            "cannot find entry `{}'",
                            String::from_utf8_lossy(&l.sval)
                        )));
                    };
                    self.names += 1;
                    if self.names > MAX_USE {
                        return Err(Stop::Error(format!(
                            "name use count ({}) exceeded",
                            MAX_USE
                        )));
                    }
                    f.print(&l.desc);
                    f.meta(l);
                    let saved = self.flip;
                    if l.flip {
                        self.flip = !self.flip;
                    }
                    let before = f.desc.len();
                    let r = self.run_lines(&ne.lines, o, f, true);
                    self.flip = saved;
                    let matched = r? && f.desc.len() > before;
                    self.names -= 1;
                    // Children of a `use` run only when the named rule
                    // printed something (observed on PHP 8.5: a checker
                    // rule without messages leaves its `default` sibling
                    // children unreached).
                    if !matched {
                        if lv == 0 {
                            return Ok(false);
                        }
                        continue;
                    }
                    ends[lv] = o;
                    has[lv] = true;
                    has[lv + 1] = false;
                    cont = lv + 1;
                    any = true;
                    continue;
                }
                Kind::Indirect => {
                    // Offsets of `indirect` are from the start of the
                    // buffer, or of the named rule's base with `/r`.
                    let ib = if l.sflags & SF_REL != 0 { base } else { 0 };
                    let Some(o) = self.resolve(l, ib, parent_end) else {
                        if lv == 0 {
                            return Ok(false);
                        }
                        continue;
                    };
                    self.indirect += 1;
                    if self.indirect > 50 {
                        return Err(Stop::Error("indirect count (50) exceeded".into()));
                    }
                    f.print(&l.desc);
                    f.meta(l);
                    if o < self.buf.len() {
                        let sub = &self.buf[o..];
                        let mut c = Ctx {
                            db: self.db,
                            buf: sub,
                            file_mode: false,
                            indirect: self.indirect,
                            names: self.names,
                            flip: false,
                        };
                        let mut v = Vec::new();
                        c.run(false, false, &mut v)?;
                        if let Some(x) = v.into_iter().next() {
                            // Appended as is ("Exif Standard: [TIFF image
                            // data, ...]", observed on PHP 8.5).
                            f.desc.extend_from_slice(&x.desc);
                            if f.mime.is_none() {
                                f.mime = x.mime;
                            }
                        }
                    }
                    self.indirect -= 1;
                    ends[lv] = o;
                    has[lv] = true;
                    has[lv + 1] = false;
                    cont = lv + 1;
                    any = true;
                    continue;
                }
                _ => match self.resolve(l, base, parent_end) {
                    Some(o) => self.test(l, o),
                    None => None,
                },
            };
            match m {
                Some((end, val)) => {
                    if l.kind != Kind::Clear {
                        has[lv] = true;
                    }
                    has[lv + 1] = false;
                    ends[lv] = end;
                    cont = lv + 1;
                    any = true;
                    let d = format_desc(&l.desc, &val);
                    f.print(&d);
                    f.meta(l);
                }
                None => {
                    if lv == 0 {
                        return Ok(false);
                    }
                }
            }
        }
        Ok(any)
    }

    /// Tests one line at absolute offset `o`; on a match returns the end
    /// offset (for `&` children) and the value to print.
    fn test(&self, l: &Line, o: usize) -> Option<(usize, Val)> {
        let b = self.buf;
        match l.kind {
            Kind::Int(_) | Kind::Date(..) | Kind::Id3 | Kind::Offset => {
                let size = match l.kind {
                    Kind::Int(n) | Kind::Date(n, _) => n,
                    Kind::Id3 => 4,
                    _ => 8,
                };
                let raw = if l.kind == Kind::Offset {
                    o as u64
                } else {
                    let r = read_uint(b, o, size, self.en(l.endian))?;
                    if l.kind == Kind::Id3 { id3(r) } else { r }
                };
                let v = if l.op != 0 {
                    apply_op(raw, l.op, l.operand) & mask(size)
                } else {
                    raw
                };
                let signed = !l.unsigned;
                let ok = cmp_int(v, l.num, l.reln, size, signed);
                if !ok {
                    return None;
                }
                let end = if l.kind == Kind::Offset {
                    o
                } else {
                    o + size as usize
                };
                let val = match l.kind {
                    Kind::Date(sz, st) => Val::Date(v, sz, st),
                    _ => Val::Int(v, size, signed),
                };
                Some((end, val))
            }
            Kind::Float(size) => {
                let r = read_uint(b, o, size, self.en(l.endian))?;
                let v = if size == 4 {
                    f32::from_bits(r as u32) as f64
                } else {
                    f64::from_bits(r)
                };
                let ok = match l.reln {
                    b'x' => true,
                    b'=' => v == l.fnum,
                    b'!' => v != l.fnum,
                    b'<' => v < l.fnum,
                    b'>' => v > l.fnum,
                    _ => false,
                };
                ok.then_some((o + size as usize, Val::Float(v)))
            }
            Kind::Str => {
                if o > b.len() {
                    return None;
                }
                let s = &b[o..];
                if l.reln == b'x' {
                    if o >= b.len() {
                        return None;
                    }
                    let v = cstr(s, l);
                    let end = o + v.len();
                    return Some((end, Val::Bytes(v)));
                }
                match l.reln {
                    b'=' | b'!' => {
                        let m = str_match(s, &l.sval, l.sflags);
                        match (l.reln, m) {
                            (b'=', Some(n)) => Some((o + n, Val::Bytes(s[..n].to_vec()))),
                            (b'!', None) => {
                                if o >= b.len() {
                                    return None;
                                }
                                let v = cstr(s, l);
                                Some((o + l.sval.len().min(s.len()), Val::Bytes(v)))
                            }
                            _ => None,
                        }
                    }
                    b'<' | b'>' => {
                        if s.is_empty() {
                            return None;
                        }
                        let n = l.sval.len().min(s.len());
                        let mut ord = std::cmp::Ordering::Equal;
                        for k in 0..l.sval.len() {
                            let fc = s.get(k).copied().unwrap_or(0);
                            let mc = l.sval[k];
                            if fc != mc {
                                ord = fc.cmp(&mc);
                                break;
                            }
                        }
                        let ok = if l.reln == b'>' {
                            ord == std::cmp::Ordering::Greater
                        } else {
                            ord == std::cmp::Ordering::Less
                        };
                        let v = cstr(s, l);
                        ok.then_some((o + n.max(v.len()).min(s.len()), Val::Bytes(v)))
                    }
                    _ => None,
                }
            }
            Kind::PStr(lsize, le, incl) => {
                let n = read_uint(b, o, lsize, le)? as usize;
                let n = if incl {
                    n.checked_sub(lsize as usize)?
                } else {
                    n
                };
                let st = o + lsize as usize;
                let s = b.get(st..st.checked_add(n)?.min(b.len()))?;
                let ok = match l.reln {
                    b'x' => true,
                    b'=' => str_match(s, &l.sval, l.sflags).is_some(),
                    b'!' => str_match(s, &l.sval, l.sflags).is_none(),
                    b'>' => s > &l.sval[..],
                    b'<' => s < &l.sval[..],
                    _ => false,
                };
                let mut v = s.to_vec();
                if let Some(p) = v.iter().position(|&c| c == 0 || c == b'\n' || c == b'\r') {
                    v.truncate(p);
                }
                ok.then_some((st + s.len(), Val::Bytes(v)))
            }
            Kind::Str16 => {
                let s = b.get(o..)?;
                let unit = |k: usize| -> Option<u16> {
                    let p = s.get(2 * k..2 * k + 2)?;
                    Some(if l.endian == Endian::Big {
                        u16::from_be_bytes([p[0], p[1]])
                    } else {
                        u16::from_le_bytes([p[0], p[1]])
                    })
                };
                if l.reln == b'x' {
                    let mut v = Vec::new();
                    let mut k = 0;
                    while let Some(u) = unit(k) {
                        if u == 0 || v.len() >= 127 {
                            break;
                        }
                        v.push(if u < 0x100 { u as u8 } else { b'?' });
                        k += 1;
                    }
                    return Some((o + 2 * k, Val::Bytes(v)));
                }
                let mut ok = true;
                for (k, &c) in l.sval.iter().enumerate() {
                    if unit(k) != Some(c as u16) {
                        ok = false;
                        break;
                    }
                }
                if l.reln == b'!' {
                    ok = !ok;
                }
                ok.then(|| (o + 2 * l.sval.len(), Val::Bytes(l.sval.clone())))
            }
            Kind::Search => {
                if o > b.len() {
                    return None;
                }
                let s = &b[o..];
                let range = if l.range == 0 {
                    s.len()
                } else {
                    l.range as usize
                };
                let mut found = None;
                if l.reln == b'x' {
                    found = Some((0, 0));
                } else {
                    for p in 0..range.min(s.len()) {
                        if let Some(n) = str_match(&s[p..], &l.sval, l.sflags) {
                            found = Some((p, n));
                            break;
                        }
                    }
                }
                match (l.reln, found) {
                    (b'!', None) => Some((o, Val::Bytes(Vec::new()))),
                    (b'!', Some(_)) => None,
                    (_, Some((p, n))) => {
                        let st = o + p;
                        let end = if l.sflags & SF_START != 0 { st } else { st + n };
                        let mut v = b[st..].to_vec();
                        if let Some(z) = v.iter().position(|&c| c == 0) {
                            v.truncate(z);
                        }
                        v.truncate(8192);
                        Some((end, Val::Bytes(v)))
                    }
                    _ => None,
                }
            }
            Kind::Regex => {
                if o > b.len() {
                    return None;
                }
                let s = &b[o..];
                let lim = if l.sflags & SF_LINES != 0 {
                    let want = if l.range == 0 {
                        usize::MAX
                    } else {
                        l.range as usize
                    };
                    let cap = if l.range == 0 {
                        8192
                    } else {
                        (l.range as usize).saturating_mul(80)
                    };
                    let mut n = 0;
                    let mut end = s.len().min(cap);
                    for (k, &c) in s.iter().enumerate().take(cap) {
                        if c == b'\n' {
                            n += 1;
                            if n >= want {
                                end = k + 1;
                                break;
                            }
                        }
                    }
                    end
                } else if l.range == 0 {
                    s.len().min(8192)
                } else {
                    s.len().min(l.range as usize)
                };
                let region = &s[..lim];
                if l.reln == b'x' {
                    return Some((o, Val::Bytes(Vec::new())));
                }
                let re = l.regex.as_ref()?;
                let mut m = re.matcher();
                let hit = m.exec(region, 0).unwrap_or(false);
                let g = if hit { m.group(0) } else { None };
                match (l.reln, g) {
                    (b'!', None) => Some((o, Val::Bytes(Vec::new()))),
                    (b'!', Some(_)) => None,
                    (_, Some((a, z))) => {
                        let end = if l.sflags & SF_START != 0 {
                            o + a
                        } else {
                            o + z
                        };
                        Some((end, Val::Bytes(region[a..z].to_vec())))
                    }
                    _ => None,
                }
            }
            Kind::Octal => {
                let s = b.get(o..)?;
                let n = s.iter().take_while(|c| (b'0'..=b'7').contains(c)).count();
                if n == 0 {
                    return None;
                }
                let mut x = 0u64;
                for &c in &s[..n] {
                    x = x.wrapping_mul(8).wrapping_add((c - b'0') as u64);
                }
                let want = crate::magic::parse_num(&l.sval)
                    .map(|v| v.0 as u64)
                    .unwrap_or(0);
                let ok = cmp_int(x, want, l.reln, 8, false);
                ok.then_some((o + n, Val::Int(x, 8, false)))
            }
            Kind::Guid => {
                let s = b.get(o..o + 16)?;
                let g = format!(
                    "{:08X}-{:04X}-{:04X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}",
                    u32::from_le_bytes([s[0], s[1], s[2], s[3]]),
                    u16::from_le_bytes([s[4], s[5]]),
                    u16::from_le_bytes([s[6], s[7]]),
                    s[8],
                    s[9],
                    s[10],
                    s[11],
                    s[12],
                    s[13],
                    s[14],
                    s[15]
                );
                let ok = l.reln == b'x' || g.as_bytes().eq_ignore_ascii_case(&l.sval);
                ok.then(|| (o + 16, Val::Bytes(g.into_bytes())))
            }
            // ASN.1 DER walking is not implemented: such tests never match.
            Kind::Der => None,
            _ => None,
        }
    }
}

fn cmp_int(v: u64, num: u64, reln: u8, size: u8, signed: bool) -> bool {
    let m = mask(size);
    let v = v & m;
    let n = num & m;
    match reln {
        b'x' => true,
        b'=' => v == n,
        b'!' => v != n,
        b'&' => v & n == n,
        b'^' => v & n != n,
        b'<' | b'>' => {
            let ord = if signed {
                sext(v, size).cmp(&sext(n, size))
            } else {
                v.cmp(&n)
            };
            if reln == b'<' {
                ord == std::cmp::Ordering::Less
            } else {
                ord == std::cmp::Ordering::Greater
            }
        }
        _ => false,
    }
}

/// The string printed for `string x` and friends: up to a NUL or line end,
/// at most 127 bytes (observed on PHP 8.5), trimmed with `T`.
fn cstr(s: &[u8], l: &Line) -> Vec<u8> {
    let mut n = s
        .iter()
        .position(|&c| c == 0 || c == b'\n' || c == b'\r')
        .unwrap_or(s.len());
    n = n.min(127);
    if l.range > 0 {
        n = n.min(l.range as usize);
    }
    let mut v = s[..n].to_vec();
    if l.sflags & SF_TRIM != 0 {
        while v.last().is_some_and(|c| c.is_ascii_whitespace()) {
            v.pop();
        }
        let k = v.iter().take_while(|c| c.is_ascii_whitespace()).count();
        v.drain(..k);
    }
    v
}

/// Matches magic string `m` at the start of `s` with the string flags;
/// returns the number of file bytes consumed.
pub fn str_match(s: &[u8], m: &[u8], flags: u32) -> Option<usize> {
    let mut i = 0; // in s
    let mut j = 0; // in m
    while j < m.len() {
        let mc = m[j];
        if mc == b' ' && flags & (SF_W | SF_SW) != 0 {
            if flags & SF_W != 0 && (i >= s.len() || !s[i].is_ascii_whitespace()) {
                return None;
            }
            while i < s.len() && s[i].is_ascii_whitespace() {
                i += 1;
            }
            j += 1;
            continue;
        }
        let fc = *s.get(i)?;
        let ok = if fc == mc {
            true
        } else if flags & SF_LC != 0 && mc.is_ascii_lowercase() {
            fc.to_ascii_lowercase() == mc
        } else if flags & SF_UC != 0 && mc.is_ascii_uppercase() {
            fc.to_ascii_uppercase() == mc
        } else {
            false
        };
        if !ok {
            return None;
        }
        i += 1;
        j += 1;
    }
    if flags & crate::magic::SF_FULLWORD != 0
        && let Some(&c) = s.get(i)
        && (c.is_ascii_alphanumeric() || c == b'_')
    {
        return None;
    }
    Some(i)
}

/// ctime(3)-style date: "Fri May 21 05:20:00 1993".
pub fn fmt_time(secs: i64, local: bool) -> String {
    let (y, mo, d, h, mi, s, wd) = if local {
        local_parts(secs)
    } else {
        utc_parts(secs)
    };
    const DAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    const MONS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    format!(
        "{} {} {:2} {:02}:{:02}:{:02} {}",
        DAYS[wd as usize % 7],
        MONS[(mo as usize).saturating_sub(1) % 12],
        d,
        h,
        mi,
        s,
        y
    )
}

fn utc_parts(secs: i64) -> (i64, i64, i64, i64, i64, i64, i64) {
    let days = secs.div_euclid(86400);
    let rem = secs.rem_euclid(86400);
    // Civil-from-days (proleptic Gregorian calendar).
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let mut y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    if m <= 2 {
        y += 1;
    }
    let wd = (days + 4).rem_euclid(7);
    (y, m, d, rem / 3600, rem % 3600 / 60, rem % 60, wd)
}

fn local_parts(secs: i64) -> (i64, i64, i64, i64, i64, i64, i64) {
    let t = secs as libc::time_t;
    // SAFETY: localtime_r fills the provided struct.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    let r = unsafe { libc::localtime_r(&t, &mut tm) };
    if r.is_null() {
        return utc_parts(secs);
    }
    (
        tm.tm_year as i64 + 1900,
        tm.tm_mon as i64 + 1,
        tm.tm_mday as i64,
        tm.tm_hour as i64,
        tm.tm_min as i64,
        tm.tm_sec as i64,
        tm.tm_wday as i64,
    )
}

fn date_string(v: u64, size: u8, st: DateStyle) -> String {
    match st {
        DateStyle::Unix | DateStyle::UnixLocal => {
            let s = if size == 4 { v as u32 as i64 } else { v as i64 };
            if s == 0 && size == 4 {
                // Shown like any other time stamp.
            }
            fmt_time(s, st == DateStyle::UnixLocal)
        }
        DateStyle::Windows => {
            let secs = (v / 10_000_000) as i64 - 11_644_473_600;
            fmt_time(secs, false)
        }
        DateStyle::MsdosDate => {
            let d = v & 0xffff;
            format!(
                "{} {:02}, {}",
                [
                    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov",
                    "Dec"
                ][((d >> 5) & 0xf).clamp(1, 12) as usize - 1],
                d & 0x1f,
                1980 + (d >> 9)
            )
        }
        DateStyle::MsdosTime => {
            let t = v & 0xffff;
            format!(
                "{:02}:{:02}:{:02}",
                t >> 11,
                (t >> 5) & 0x3f,
                (t & 0x1f) * 2
            )
        }
    }
}

/// Expands the printf conversion of a description with the value.
fn format_desc(desc: &[u8], val: &Val) -> Vec<u8> {
    let Some(p) = desc.iter().position(|&c| c == b'%') else {
        return desc.to_vec();
    };
    let mut out = desc[..p].to_vec();
    let mut i = p + 1;
    if desc.get(i) == Some(&b'%') {
        out.push(b'%');
        out.extend_from_slice(&format_desc(&desc[i + 1..], val));
        return out;
    }
    let mut left = false;
    let mut zero = false;
    let mut alt = false;
    let mut plus = false;
    let mut space = false;
    while let Some(&c) = desc.get(i) {
        match c {
            b'-' => left = true,
            b'0' => zero = true,
            b'#' => alt = true,
            b'+' => plus = true,
            b' ' => space = true,
            _ => break,
        }
        i += 1;
    }
    let mut width = 0usize;
    while let Some(&c) = desc.get(i).filter(|c| c.is_ascii_digit()) {
        width = width * 10 + (c - b'0') as usize;
        i += 1;
    }
    let mut prec: Option<usize> = None;
    if desc.get(i) == Some(&b'.') {
        i += 1;
        let mut pv = 0;
        while let Some(&c) = desc.get(i).filter(|c| c.is_ascii_digit()) {
            pv = pv * 10 + (c - b'0') as usize;
            i += 1;
        }
        prec = Some(pv);
    }
    while let Some(&c) = desc.get(i) {
        if matches!(c, b'h' | b'l' | b'q' | b'j' | b'z' | b't' | b'L') {
            i += 1;
        } else {
            break;
        }
    }
    let conv = desc.get(i).copied().unwrap_or(b's');
    i += 1;
    let (body, numeric_neg): (Vec<u8>, bool) = match (conv, val) {
        (b'd' | b'i', Val::Int(v, sz, signed)) => {
            // C printf: a 4- or 8-byte value prints signed with %d.
            if *signed || *sz >= 4 {
                let n = sext(*v, *sz);
                (n.unsigned_abs().to_string().into_bytes(), n < 0)
            } else {
                ((v & mask(*sz)).to_string().into_bytes(), false)
            }
        }
        (b'u', Val::Int(v, sz, _)) => ((v & mask(*sz)).to_string().into_bytes(), false),
        (b'x', Val::Int(v, sz, _)) => (format!("{:x}", v & mask(*sz)).into_bytes(), false),
        (b'X', Val::Int(v, sz, _)) => (format!("{:X}", v & mask(*sz)).into_bytes(), false),
        (b'o', Val::Int(v, sz, _)) => (format!("{:o}", v & mask(*sz)).into_bytes(), false),
        (b'c', Val::Int(v, _, _)) => (vec![*v as u8], false),
        (b'd' | b'i' | b'u', Val::Date(v, _, _)) => (v.to_string().into_bytes(), false),
        (b'x', Val::Date(v, _, _)) => (format!("{:x}", v).into_bytes(), false),
        (b's', Val::Date(v, sz, st)) => (date_string(*v, *sz, *st).into_bytes(), false),
        (b's', Val::Bytes(b)) => {
            let mut b = b.clone();
            if let Some(p) = prec {
                b.truncate(p);
            }
            (b, false)
        }
        (b's', Val::Int(v, _, _)) => (v.to_string().into_bytes(), false),
        (b'f' | b'g' | b'e' | b'G' | b'E', Val::Float(f)) => {
            let s = match conv {
                b'f' => format!("{:.*}", prec.unwrap_or(6), f),
                _ => fmt_g(*f, prec.unwrap_or(6)),
            };
            (s.into_bytes(), false)
        }
        (b'd' | b'u' | b'x', Val::Float(f)) => ((*f as i64).to_string().into_bytes(), false),
        (_, Val::Bytes(b)) => (b.clone(), false),
        (_, Val::Int(v, _, _)) => (v.to_string().into_bytes(), false),
        _ => (Vec::new(), false),
    };
    let mut body = body;
    if matches!(conv, b'd' | b'i' | b'u' | b'x' | b'X' | b'o')
        && let Some(p) = prec
    {
        while body.len() < p {
            body.insert(0, b'0');
        }
    }
    let mut prefix: Vec<u8> = Vec::new();
    if numeric_neg {
        prefix.push(b'-');
    } else if plus && matches!(conv, b'd' | b'i') {
        prefix.push(b'+');
    } else if space && matches!(conv, b'd' | b'i') {
        prefix.push(b' ');
    }
    if alt {
        match conv {
            b'x' if body != b"0" => prefix.extend_from_slice(b"0x"),
            b'X' if body != b"0" => prefix.extend_from_slice(b"0X"),
            b'o' if !body.starts_with(b"0") => prefix.push(b'0'),
            _ => {}
        }
    }
    let total = prefix.len() + body.len();
    if total < width {
        let pad = width - total;
        if left {
            out.extend_from_slice(&prefix);
            out.extend_from_slice(&body);
            out.extend(std::iter::repeat_n(b' ', pad));
        } else if zero && conv != b's' && conv != b'c' {
            out.extend_from_slice(&prefix);
            out.extend(std::iter::repeat_n(b'0', pad));
            out.extend_from_slice(&body);
        } else {
            out.extend(std::iter::repeat_n(b' ', pad));
            out.extend_from_slice(&prefix);
            out.extend_from_slice(&body);
        }
    } else {
        out.extend_from_slice(&prefix);
        out.extend_from_slice(&body);
    }
    // Text after the conversion ("%%" there prints a percent sign).
    let rest = &desc[i.min(desc.len())..];
    let mut k = 0;
    while k < rest.len() {
        if rest[k] == b'%' && rest.get(k + 1) == Some(&b'%') {
            out.push(b'%');
            k += 2;
        } else {
            out.push(rest[k]);
            k += 1;
        }
    }
    out
}

fn fmt_g(f: f64, prec: usize) -> String {
    let p = prec.max(1);
    if f == 0.0 {
        return "0".into();
    }
    let e = f.abs().log10().floor() as i32;
    if e < -4 || e >= p as i32 {
        let s = format!("{:.*e}", p - 1, f);
        // Rust prints 1.5e3; C prints 1.5e+03.
        if let Some((m, ex)) = s.split_once('e') {
            let m = if m.contains('.') {
                m.trim_end_matches('0').trim_end_matches('.').to_string()
            } else {
                m.to_string()
            };
            let (sign, digits) = ex.strip_prefix('-').map(|d| ('-', d)).unwrap_or(('+', ex));
            return format!("{}e{}{:0>2}", m, sign, digits);
        }
        s
    } else {
        let dec = (p as i32 - 1 - e).max(0) as usize;
        let s = format!("{:.*}", dec, f);
        if s.contains('.') {
            s.trim_end_matches('0').trim_end_matches('.').to_string()
        } else {
            s
        }
    }
}
