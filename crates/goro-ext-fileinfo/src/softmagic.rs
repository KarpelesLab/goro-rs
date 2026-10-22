//! softmagic.c (and der.c): matching the magic entries.

use crate::apprentice::{convert_libmagic_pattern, printable};
use crate::fmt::Arg;
use crate::funcs::{FILE_T_LOCAL, FILE_T_WINDOWS, fmtdate, fmtdatetime, fmtnum, fmttime, strtrim};
use crate::magic::*;

/// Mutable match state shared by the recursion (the int pointers of C).
pub struct St<'a> {
    pub indir_count: &'a mut u16,
    pub name_count: &'a mut u16,
    pub printed_something: &'a mut bool,
    pub need_separator: &'a mut bool,
    pub firstline: &'a mut bool,
}

/// file_softmagic()
pub fn file_softmagic(
    ms: &mut MagicSet,
    host: &mut dyn Host,
    b: &Buffer,
    mode: u8,
    text: bool,
) -> i32 {
    let mut nc = 0u16;
    let mut ic = 0u16;
    let mut printed_something = false;
    let mut need_separator = false;
    let mut firstline = true;
    let mut rv = 0;
    let lists = ms.mlist[0].clone().unwrap_or_default();
    for ml in lists {
        let mut st = St {
            indir_count: &mut ic,
            name_count: &mut nc,
            printed_something: &mut printed_something,
            need_separator: &mut need_separator,
            firstline: &mut firstline,
        };
        let mut retv = 0;
        let mut fm = 0;
        let ret = do_match(
            ms, host, &ml, b, 0, mode, text, false, &mut st, &mut retv, &mut fm,
        );
        match ret {
            -1 => return -1,
            0 => continue,
            _ => {
                if ms.flags & MAGIC_CONTINUE == 0 {
                    return ret;
                }
                rv = ret;
            }
        }
    }
    rv
}

const FLT: u32 = STRING_BINTEST | STRING_TEXTTEST;

/// match()
#[allow(clippy::too_many_arguments)]
pub fn do_match(
    ms: &mut MagicSet,
    host: &mut dyn Host,
    magic: &[Magic],
    b: &Buffer,
    offset: usize,
    mode: u8,
    text: bool,
    flip: bool,
    st: &mut St,
    returnval: &mut i32,
    found_match: &mut i32,
) -> i32 {
    let nmagic = magic.len();
    let mut cont_level: usize = 0;
    let print = ms.flags & MAGIC_NODESC == 0;
    let mut bb: BufRef = b.fbuf.clone();
    ms.check_mem(cont_level);
    let mut magindex = 0usize;
    macro_rules! flush {
        ($l:lifetime) => {{
            while magindex + 1 < nmagic && magic[magindex + 1].cont_level != 0 {
                magindex += 1;
            }
            cont_level = 0;
            magindex += 1;
            continue $l;
        }};
    }
    'outer: while magindex < nmagic {
        let m = &magic[magindex];
        if m.typ != FILE_NAME
            && ((is_libmagic_string(m.typ)
                && ((text && m.str_flags() & FLT == STRING_BINTEST)
                    || (!text && m.str_flags() & FLT == STRING_TEXTTEST)))
                || (m.flag & mode) != mode)
        {
            flush!('outer);
        }
        if msetoffset(ms, m, &mut bb, b, offset, cont_level).is_err() {
            flush!('outer);
        }
        ms.line = m.lineno as usize;
        let flush;
        match mget(
            ms,
            host,
            m,
            b,
            &bb,
            offset,
            cont_level,
            mode,
            text,
            flip,
            st,
            returnval,
            found_match,
        ) {
            -1 => return -1,
            0 => flush = m.reln != b'!',
            _ => {
                if m.typ == FILE_INDIRECT {
                    *found_match = 1;
                    *returnval = 1;
                }
                match magiccheck(ms, host, m) {
                    -1 => return -1,
                    0 => flush = true,
                    _ => flush = false,
                }
            }
        }
        if flush {
            flush!('outer);
        }
        let e = handle_annotation(ms, m, *st.firstline);
        if e != 0 {
            *found_match = 1;
            *st.need_separator = true;
            *st.printed_something = true;
            *returnval = 1;
            *st.firstline = false;
            return e;
        }
        if m.desc[0] != 0 {
            *found_match = 1;
            if print {
                *returnval = 1;
                *st.need_separator = true;
                *st.printed_something = true;
                if print_sep(ms, *st.firstline).is_err() {
                    return -1;
                }
                if mprint(ms, host, m).is_err() {
                    return -1;
                }
            }
        }
        match moffset(ms, m, &bb, offset) {
            Some(o) => ms.c[cont_level].off = o,
            None => {
                if let Some(li) = ms.c.get_mut(cont_level) {
                    if m.typ == FILE_DER {
                        li.off = 0;
                    }
                }
                flush!('outer);
            }
        }
        cont_level += 1;
        ms.check_mem(cont_level);
        while magindex + 1 < nmagic && magic[magindex + 1].cont_level != 0 {
            magindex += 1;
            let m = &magic[magindex];
            ms.line = m.lineno as usize;
            if cont_level < m.cont_level as usize {
                continue;
            }
            if cont_level > m.cont_level as usize {
                cont_level = m.cont_level as usize;
            }
            if msetoffset(ms, m, &mut bb, b, offset, cont_level).is_err() {
                flush!('outer);
            }
            if m.flag & OFFADD != 0 {
                if cont_level == 0 {
                    return 0;
                }
                ms.offset = ms.offset.wrapping_add(ms.c[cont_level - 1].off as u32);
            }
            if (m.cond == COND_ELSE || m.cond == COND_ELIF) && ms.c[cont_level].last_match {
                continue;
            }
            let mut flush = false;
            match mget(
                ms,
                host,
                m,
                b,
                &bb,
                offset,
                cont_level,
                mode,
                text,
                flip,
                st,
                returnval,
                found_match,
            ) {
                -1 => return -1,
                0 => {
                    if m.reln != b'!' {
                        continue;
                    }
                    flush = true;
                }
                _ => {
                    if m.typ == FILE_INDIRECT {
                        *found_match = 1;
                        *returnval = 1;
                    }
                }
            }
            let r = if flush { 1 } else { magiccheck(ms, host, m) };
            match r {
                -1 => return -1,
                0 => ms.c[cont_level].last_match = false,
                _ => {
                    ms.c[cont_level].last_match = true;
                    let mut skip = false;
                    if m.typ == FILE_CLEAR {
                        ms.c[cont_level].got_match = false;
                    } else if ms.c[cont_level].got_match {
                        if m.typ == FILE_DEFAULT {
                            skip = true;
                        }
                    } else {
                        ms.c[cont_level].got_match = true;
                    }
                    if skip {
                        continue;
                    }
                    let e = handle_annotation(ms, m, *st.firstline);
                    if e != 0 {
                        *found_match = 1;
                        *st.need_separator = true;
                        *st.printed_something = true;
                        *returnval = 1;
                        return e;
                    }
                    if m.desc[0] != 0 {
                        *found_match = 1;
                    }
                    if print && m.desc[0] != 0 {
                        *returnval = 1;
                        if !*st.printed_something {
                            *st.printed_something = true;
                            if print_sep(ms, *st.firstline).is_err() {
                                return -1;
                            }
                        }
                        if *st.need_separator
                            && m.flag & NOSPACE == 0
                            && ms.printf(b" ", &[]).is_err()
                        {
                            return -1;
                        }
                        if mprint(ms, host, m).is_err() {
                            return -1;
                        }
                        *st.need_separator = true;
                    }
                    match moffset(ms, m, &bb, offset) {
                        Some(o) => ms.c[cont_level].off = o,
                        None => {
                            if m.typ == FILE_DER {
                                ms.c[cont_level].off = 0;
                            }
                            cont_level -= 1;
                        }
                    }
                    cont_level += 1;
                    ms.check_mem(cont_level);
                }
            }
        }
        if *st.printed_something {
            *st.firstline = false;
        }
        if *found_match != 0 {
            if ms.flags & MAGIC_CONTINUE == 0 {
                return *returnval;
            }
            *st.printed_something = false;
            *st.firstline = false;
        }
        cont_level = 0;
        magindex += 1;
    }
    *returnval
}

/// check_fmt(): whether the description has a %s conversion
fn check_fmt(desc: &[u8]) -> bool {
    if !desc.contains(&b'%') {
        return false;
    }
    let mut i = 0;
    while i < desc.len() {
        if desc[i] == b'%' {
            let mut j = i + 1;
            while j < desc.len() && (desc[j] == b'-' || desc[j].is_ascii_digit() || desc[j] == b'.')
            {
                j += 1;
            }
            if j < desc.len() && desc[j] == b's' {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// varexpand(): ${x?yes:no}
fn varexpand(mode: u32, s: &[u8], len: usize) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut sptr = 0usize;
    let at = |i: usize| *s.get(i).unwrap_or(&0);
    while let Some(rel) = s[sptr..].windows(2).position(|w| w == b"${") {
        let ptr = sptr + rel;
        if out.len() + (ptr - sptr) >= len {
            return None;
        }
        out.extend_from_slice(&s[sptr..ptr]);
        let ptr = ptr + 2;
        if at(ptr) == 0 || at(ptr + 1) != b'?' {
            return None;
        }
        let t = ptr + 2;
        let mut et = t;
        while at(et) != 0 && at(et) != b':' {
            et += 1;
        }
        if at(et) != b':' {
            return None;
        }
        let e = et + 1;
        let mut ee = e;
        while at(ee) != 0 && at(ee) != b'}' {
            ee += 1;
        }
        if at(ee) != b'}' {
            return None;
        }
        let part = match at(ptr) {
            b'x' => {
                if mode & 0o111 != 0 {
                    &s[t..et]
                } else {
                    &s[e..ee]
                }
            }
            _ => return None,
        };
        if out.len() + part.len() >= len {
            return None;
        }
        out.extend_from_slice(part);
        sptr = ee + 1;
    }
    if out.len() + (s.len() - sptr) >= len {
        return None;
    }
    out.extend_from_slice(&s[sptr..]);
    Some(out)
}

/// The C library's "%g" (snprintf()).
fn c_g(v: f64) -> Vec<u8> {
    if v.is_nan() {
        return b"nan".to_vec();
    }
    if v.is_infinite() {
        return if v < 0.0 {
            b"-inf".to_vec()
        } else {
            b"inf".to_vec()
        };
    }
    if v == 0.0 {
        return if v.is_sign_negative() {
            b"-0".to_vec()
        } else {
            b"0".to_vec()
        };
    }
    let p = 6i32;
    let s = format!("{:.*e}", (p - 1) as usize, v);
    let (_, e) = s.split_once('e').unwrap();
    let x: i32 = e.parse().unwrap();
    let strip = |s: String| -> String {
        if s.contains('.') {
            let t = s.trim_end_matches('0');
            t.trim_end_matches('.').to_string()
        } else {
            s
        }
    };
    if x < -4 || x >= p {
        let (m, _) = s.split_once('e').unwrap();
        let m = strip(m.to_string());
        format!("{}e{}{:02}", m, if x < 0 { '-' } else { '+' }, x.abs()).into_bytes()
    } else {
        strip(format!("{:.*}", (p - 1 - x) as usize, v)).into_bytes()
    }
}

/// mprint()
fn mprint(ms: &mut MagicSet, host: &mut dyn Host, m: &Magic) -> Result<(), ()> {
    let raw_desc = cstr(&m.desc).to_vec();
    let desc = varexpand(ms.mode, &raw_desc, 512).unwrap_or_else(|| raw_desc.clone());
    macro_rules! printer {
        ($val:expr, $s:ty, $u:ty) => {{
            let v = ms.signextend(m, $val as u64, host);
            if check_fmt(&desc) {
                let buf = if m.flag & UNSIGNED != 0 {
                    (v as $u).to_string()
                } else {
                    (v as $s).to_string()
                };
                ms.printf(&desc, &[Arg::S(buf.as_bytes())])
            } else if m.flag & UNSIGNED != 0 {
                ms.printf(&desc, &[Arg::I(v as $u as i64)])
            } else {
                ms.printf(&desc, &[Arg::I(v as $s as i64)])
            }
        }};
    }
    let p = ms.ms_value.clone();
    match m.typ {
        FILE_BYTE => printer!(p.b(), i8, u8),
        FILE_SHORT | FILE_BESHORT | FILE_LESHORT => printer!(p.h(), i16, u16),
        FILE_LONG | FILE_BELONG | FILE_LELONG | FILE_MELONG => printer!(p.l(), i32, u32),
        FILE_QUAD | FILE_BEQUAD | FILE_LEQUAD | FILE_OFFSET => printer!(p.q(), i64, u64),
        FILE_STRING | FILE_PSTRING | FILE_BESTRING16 | FILE_LESTRING16 => {
            if m.reln == b'=' || m.reln == b'!' {
                let s = printable(ms.flags, &m.value, 512);
                ms.printf(&desc, &[Arg::S(&s)])?;
            } else {
                if m.value[0] == 0 {
                    let s = &mut ms.ms_value.0;
                    if let Some(n) = cstr(s).iter().position(|&c| c == b'\r' || c == b'\n') {
                        s[n] = 0;
                    }
                }
                let s = ms.ms_value.0;
                let mut st: &[u8] = cstr(&s);
                if m.str_flags() & STRING_TRIM != 0 {
                    st = strtrim(st);
                }
                let s = printable(ms.flags, st, 512);
                ms.printf(&desc, &[Arg::S(&s)])?;
                if m.typ == FILE_PSTRING && ms.pstring_length_size(m) == FILE_BADSIZE {
                    return Err(());
                }
            }
            Ok(())
        }
        FILE_DATE | FILE_BEDATE | FILE_LEDATE | FILE_MEDATE => {
            let s = fmtdatetime(p.l() as u64, 0);
            ms.printf(&desc, &[Arg::S(&s)])
        }
        FILE_LDATE | FILE_BELDATE | FILE_LELDATE | FILE_MELDATE => {
            let s = fmtdatetime(p.l() as u64, FILE_T_LOCAL);
            ms.printf(&desc, &[Arg::S(&s)])
        }
        FILE_QDATE | FILE_BEQDATE | FILE_LEQDATE => {
            let s = fmtdatetime(p.q(), 0);
            ms.printf(&desc, &[Arg::S(&s)])
        }
        FILE_QLDATE | FILE_BEQLDATE | FILE_LEQLDATE => {
            let s = fmtdatetime(p.q(), FILE_T_LOCAL);
            ms.printf(&desc, &[Arg::S(&s)])
        }
        FILE_QWDATE | FILE_BEQWDATE | FILE_LEQWDATE => {
            let s = fmtdatetime(p.q(), FILE_T_WINDOWS);
            ms.printf(&desc, &[Arg::S(&s)])
        }
        FILE_FLOAT | FILE_BEFLOAT | FILE_LEFLOAT => {
            let vf = p.f() as f64;
            if check_fmt(&desc) {
                let buf = c_g(vf);
                ms.printf(&desc, &[Arg::S(&buf)])
            } else {
                ms.printf(&desc, &[Arg::F(vf)])
            }
        }
        FILE_DOUBLE | FILE_BEDOUBLE | FILE_LEDOUBLE => {
            let vd = p.d();
            if check_fmt(&desc) {
                let buf = c_g(vd);
                ms.printf(&desc, &[Arg::S(&buf)])
            } else {
                ms.printf(&desc, &[Arg::F(vd)])
            }
        }
        FILE_SEARCH | FILE_REGEX => {
            let cp = ms.search.bytes(ms.search.rm_len);
            let cps: &[u8] = cstr(&cp);
            let scp = if m.str_flags() & STRING_TRIM != 0 {
                strtrim(cps)
            } else {
                cps
            };
            let s = printable(ms.flags, scp, 512);
            ms.printf(&desc, &[Arg::S(&s)])
        }
        FILE_DEFAULT | FILE_CLEAR => ms.printf(b"%s", &[Arg::S(&raw_desc)]),
        FILE_INDIRECT | FILE_USE | FILE_NAME => Ok(()),
        FILE_DER => {
            let s = printable(ms.flags, &p.0, 512);
            ms.printf(&desc, &[Arg::S(&s)])
        }
        FILE_GUID => {
            let s = print_guid(&p.0);
            ms.printf(&desc, &[Arg::S(&s)])
        }
        FILE_MSDOSDATE | FILE_BEMSDOSDATE | FILE_LEMSDOSDATE => {
            let s = fmtdate(p.h());
            ms.printf(&desc, &[Arg::S(&s)])
        }
        FILE_MSDOSTIME | FILE_BEMSDOSTIME | FILE_LEMSDOSTIME => {
            let s = fmttime(p.h());
            ms.printf(&desc, &[Arg::S(&s)])
        }
        FILE_OCTAL => {
            let s = fmtnum(cstr(&m.value), 8);
            ms.printf(&desc, &[Arg::S(&s)])
        }
        t => {
            ms.magerror(format!("invalid m->type ({}) in mprint()", t).into_bytes());
            Err(())
        }
    }
}

/// file_print_guid()
pub fn print_guid(g: &[u8]) -> Vec<u8> {
    let d1 = u32::from_le_bytes(g[0..4].try_into().unwrap());
    let d2 = u16::from_le_bytes([g[4], g[5]]);
    let d3 = u16::from_le_bytes([g[6], g[7]]);
    format!(
        "{:08X}-{:04X}-{:04X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}",
        d1, d2, d3, g[8], g[9], g[10], g[11], g[12], g[13], g[14], g[15]
    )
    .into_bytes()
}

/// moffset(): None for the -1/0 returns
fn moffset(ms: &mut MagicSet, m: &Magic, b: &BufRef, offset: usize) -> Option<i32> {
    let nbytes = b.len;
    let off = ms.offset;
    let o: i32 = match m.typ {
        FILE_BYTE => off.wrapping_add(1) as i32,
        FILE_SHORT | FILE_BESHORT | FILE_LESHORT | FILE_MSDOSDATE | FILE_LEMSDOSDATE
        | FILE_BEMSDOSDATE | FILE_MSDOSTIME | FILE_LEMSDOSTIME | FILE_BEMSDOSTIME => {
            off.wrapping_add(2) as i32
        }
        FILE_LONG | FILE_BELONG | FILE_LELONG | FILE_MELONG => off.wrapping_add(4) as i32,
        FILE_QUAD | FILE_BEQUAD | FILE_LEQUAD => off.wrapping_add(8) as i32,
        FILE_STRING | FILE_PSTRING | FILE_BESTRING16 | FILE_LESTRING16 | FILE_OCTAL => {
            if m.reln == b'=' || m.reln == b'!' {
                off.wrapping_add(m.vallen as u32) as i32
            } else {
                if m.value[0] == 0 {
                    let s = &mut ms.ms_value.0;
                    if let Some(n) = cstr(s).iter().position(|&c| c == b'\r' || c == b'\n') {
                        s[n] = 0;
                    }
                }
                let mut o = off.wrapping_add(cstr(&ms.ms_value.0).len() as u32);
                if m.typ == FILE_PSTRING {
                    let l = ms.pstring_length_size(m);
                    if l == FILE_BADSIZE {
                        return None;
                    }
                    o = o.wrapping_add(l as u32);
                }
                o as i32
            }
        }
        FILE_DATE | FILE_BEDATE | FILE_LEDATE | FILE_MEDATE | FILE_LDATE | FILE_BELDATE
        | FILE_LELDATE | FILE_MELDATE => off.wrapping_add(4) as i32,
        FILE_QDATE | FILE_BEQDATE | FILE_LEQDATE | FILE_QLDATE | FILE_BEQLDATE | FILE_LEQLDATE => {
            off.wrapping_add(8) as i32
        }
        FILE_FLOAT | FILE_BEFLOAT | FILE_LEFLOAT => off.wrapping_add(4) as i32,
        FILE_DOUBLE | FILE_BEDOUBLE | FILE_LEDOUBLE => off.wrapping_add(8) as i32,
        FILE_REGEX => {
            if m.str_flags() & REGEX_OFFSET_START != 0 {
                ms.search.offset.wrapping_sub(offset) as i32
            } else {
                (ms.search.offset + ms.search.rm_len).wrapping_sub(offset) as i32
            }
        }
        FILE_SEARCH => {
            if m.str_flags() & REGEX_OFFSET_START != 0 {
                ms.search.offset.wrapping_sub(offset) as i32
            } else {
                (ms.search.offset + m.vallen as usize).wrapping_sub(offset) as i32
            }
        }
        FILE_CLEAR | FILE_DEFAULT | FILE_INDIRECT | FILE_OFFSET | FILE_USE => off as i32,
        FILE_DER => {
            let o = der_offs(ms, m, nbytes);
            if o == -1 || o as usize > nbytes {
                return None;
            }
            o
        }
        FILE_GUID => off.wrapping_add(16) as i32,
        _ => 0,
    };
    if o as isize as usize > nbytes {
        return None;
    }
    Some(o)
}

fn cvt_id3(v: u32) -> u32 {
    (v & 0x7f) | (((v >> 8) & 0x7f) << 7) | (((v >> 16) & 0x7f) << 14) | (((v >> 24) & 0x7f) << 21)
}

fn cvt_flip(t: u8, flip: bool) -> u8 {
    if !flip {
        return t;
    }
    match t {
        FILE_BESHORT => FILE_LESHORT,
        FILE_BELONG => FILE_LELONG,
        FILE_BEDATE => FILE_LEDATE,
        FILE_BELDATE => FILE_LELDATE,
        FILE_BEQUAD => FILE_LEQUAD,
        FILE_BEQDATE => FILE_LEQDATE,
        FILE_BEQLDATE => FILE_LEQLDATE,
        FILE_BEQWDATE => FILE_LEQWDATE,
        FILE_LESHORT => FILE_BESHORT,
        FILE_LELONG => FILE_BELONG,
        FILE_LEDATE => FILE_BEDATE,
        FILE_LELDATE => FILE_BELDATE,
        FILE_LEQUAD => FILE_BEQUAD,
        FILE_LEQDATE => FILE_BEQDATE,
        FILE_LEQLDATE => FILE_BEQLDATE,
        FILE_LEQWDATE => FILE_BEQWDATE,
        FILE_BEFLOAT => FILE_LEFLOAT,
        FILE_LEFLOAT => FILE_BEFLOAT,
        FILE_BEDOUBLE => FILE_LEDOUBLE,
        FILE_LEDOUBLE => FILE_BEDOUBLE,
        t => t,
    }
}

/// DO_CVT on an unsigned integer of `bits` bits: Err on a zero divide
fn do_cvt(v: u64, m: &Magic, bits: u32) -> Result<u64, ()> {
    let mask = if bits == 64 {
        u64::MAX
    } else {
        (1u64 << bits) - 1
    };
    let nm = m.num_mask() & mask;
    let mut v = v & mask;
    if m.num_mask() != 0 {
        v = match m.mask_op & FILE_OPS_MASK {
            FILE_OPAND => v & nm,
            FILE_OPOR => v | nm,
            FILE_OPXOR => v ^ nm,
            FILE_OPADD => v.wrapping_add(nm),
            FILE_OPMINUS => v.wrapping_sub(nm),
            FILE_OPMULTIPLY => v.wrapping_mul(nm),
            FILE_OPDIVIDE => {
                if nm == 0 {
                    return Err(());
                }
                v / nm
            }
            FILE_OPMODULO => {
                if nm == 0 {
                    return Err(());
                }
                v % nm
            }
            _ => v,
        } & mask;
    }
    if m.mask_op & FILE_OPINVERSE != 0 {
        v = !v & mask;
    }
    Ok(v)
}

fn do_cvt_e(v: u64, m: &Magic, bits: u32) -> Result<u64, u8> {
    do_cvt(v, m, bits).map_err(|_| 1)
}

fn be16(p: &[u8]) -> u16 {
    u16::from_be_bytes([p[0], p[1]])
}
fn le16(p: &[u8]) -> u16 {
    u16::from_le_bytes([p[0], p[1]])
}
fn be32(p: &[u8]) -> u32 {
    u32::from_be_bytes(p[..4].try_into().unwrap())
}
fn le32(p: &[u8]) -> u32 {
    u32::from_le_bytes(p[..4].try_into().unwrap())
}
fn me32(p: &[u8]) -> u32 {
    ((p[1] as u32) << 24) | ((p[0] as u32) << 16) | ((p[3] as u32) << 8) | (p[2] as u32)
}
fn be64(p: &[u8]) -> u64 {
    u64::from_be_bytes(p[..8].try_into().unwrap())
}
fn le64(p: &[u8]) -> u64 {
    u64::from_le_bytes(p[..8].try_into().unwrap())
}

/// mconvert(): false on failure
fn mconvert(ms: &mut MagicSet, m: &Magic, flip: bool) -> bool {
    // Err(1): zero divide, Err(2): invalid type, Err(3): pascal string
    let r: Result<(), u8> = (|| {
        let p = &mut ms.ms_value;
        match cvt_flip(m.typ, flip) {
            FILE_BYTE => {
                let v = do_cvt_e(p.b() as u64, m, 8)?;
                p.set_b(v as u8);
            }
            FILE_SHORT | FILE_MSDOSDATE | FILE_LEMSDOSDATE | FILE_BEMSDOSDATE | FILE_MSDOSTIME
            | FILE_LEMSDOSTIME | FILE_BEMSDOSTIME => {
                let v = do_cvt_e(p.h() as u64, m, 16)?;
                p.set_h(v as u16);
            }
            FILE_LONG | FILE_DATE | FILE_LDATE => {
                let v = do_cvt_e(p.l() as u64, m, 32)?;
                p.set_l(v as u32);
            }
            FILE_QUAD | FILE_QDATE | FILE_QLDATE | FILE_QWDATE | FILE_OFFSET => {
                let v = do_cvt_e(p.q(), m, 64)?;
                p.set_q(v);
            }
            FILE_STRING | FILE_BESTRING16 | FILE_LESTRING16 | FILE_OCTAL => {
                p.0[MAXSTRING - 1] = 0;
            }
            FILE_PSTRING => return Err(3),
            FILE_BESHORT => {
                let h = be16(&p.0);
                let v = do_cvt_e(h as u64, m, 16)?;
                p.set_h(v as u16);
            }
            FILE_BELONG | FILE_BEDATE | FILE_BELDATE => {
                let l = be32(&p.0);
                let v = do_cvt_e(l as u64, m, 32)?;
                p.set_l(v as u32);
            }
            FILE_BEQUAD | FILE_BEQDATE | FILE_BEQLDATE | FILE_BEQWDATE => {
                let q = be64(&p.0);
                let v = do_cvt_e(q, m, 64)?;
                p.set_q(v);
            }
            FILE_LESHORT => {
                let h = le16(&p.0);
                let v = do_cvt_e(h as u64, m, 16)?;
                p.set_h(v as u16);
            }
            FILE_LELONG | FILE_LEDATE | FILE_LELDATE => {
                let l = le32(&p.0);
                let v = do_cvt_e(l as u64, m, 32)?;
                p.set_l(v as u32);
            }
            FILE_LEQUAD | FILE_LEQDATE | FILE_LEQLDATE | FILE_LEQWDATE => {
                let q = le64(&p.0);
                let v = do_cvt_e(q, m, 64)?;
                p.set_q(v);
            }
            FILE_MELONG | FILE_MEDATE | FILE_MELDATE => {
                let l = me32(&p.0);
                let v = do_cvt_e(l as u64, m, 32)?;
                p.set_l(v as u32);
            }
            FILE_FLOAT => cvt_float(p, m).map_err(|_| 1u8)?,
            FILE_BEFLOAT => {
                let l = be32(&p.0);
                p.set_l(l);
                cvt_float(p, m).map_err(|_| 1u8)?;
            }
            FILE_LEFLOAT => {
                let l = le32(&p.0);
                p.set_l(l);
                cvt_float(p, m).map_err(|_| 1u8)?;
            }
            FILE_DOUBLE => cvt_double(p, m).map_err(|_| 1u8)?,
            FILE_BEDOUBLE => {
                let q = be64(&p.0);
                p.set_q(q);
                cvt_double(p, m).map_err(|_| 1u8)?;
            }
            FILE_LEDOUBLE => {
                let q = le64(&p.0);
                p.set_q(q);
                cvt_double(p, m).map_err(|_| 1u8)?;
            }
            FILE_REGEX | FILE_SEARCH | FILE_DEFAULT | FILE_CLEAR | FILE_NAME | FILE_USE
            | FILE_DER | FILE_GUID => {}
            _ => return Err(2),
        }
        Ok(())
    })();
    match r {
        Ok(()) => true,
        Err(3) => pstring_convert(ms, m),
        Err(1) => {
            ms.magerror(b"zerodivide in mconvert()".to_vec());
            false
        }
        Err(_) => {
            ms.magerror(format!("invalid type {} in mconvert()", m.typ).into_bytes());
            false
        }
    }
}

fn pstring_convert(ms: &mut MagicSet, m: &Magic) -> bool {
    let sz = ms.pstring_length_size(m);
    if sz == FILE_BADSIZE {
        return false;
    }
    let s = ms.ms_value.0;
    let mut len = ms.pstring_get_length(m, &s);
    if len == FILE_BADSIZE {
        return false;
    }
    let max = MAXSTRING - sz;
    if len >= max {
        len = max;
    }
    let p = &mut ms.ms_value.0;
    p[..len].copy_from_slice(&s[sz..sz + len]);
    p[len] = 0;
    true
}

fn cvt_float(p: &mut ValueType, m: &Magic) -> Result<(), ()> {
    if m.num_mask() != 0 {
        let nm = m.num_mask() as f32;
        let f = p.f();
        let f = match m.mask_op & FILE_OPS_MASK {
            FILE_OPADD => f + nm,
            FILE_OPMINUS => f - nm,
            FILE_OPMULTIPLY => f * nm,
            FILE_OPDIVIDE => {
                if nm == 0.0 {
                    return Err(());
                }
                f / nm
            }
            _ => f,
        };
        p.set_f(f);
    }
    Ok(())
}

fn cvt_double(p: &mut ValueType, m: &Magic) -> Result<(), ()> {
    if m.num_mask() != 0 {
        let nm = m.num_mask() as f64;
        let f = p.d();
        let f = match m.mask_op & FILE_OPS_MASK {
            FILE_OPADD => f + nm,
            FILE_OPMINUS => f - nm,
            FILE_OPMULTIPLY => f * nm,
            FILE_OPDIVIDE => {
                if nm == 0.0 {
                    return Err(());
                }
                f / nm
            }
            _ => f,
        };
        p.set_d(f);
    }
    Ok(())
}

/// mcopy()
fn mcopy(
    ms: &mut MagicSet,
    typ: u8,
    indir: bool,
    s: &BufRef,
    offset: u32,
    nbytes: usize,
    m: &Magic,
) {
    let mut size = MAXSTRING;
    let mut offset = offset as usize;
    if !indir {
        match typ {
            FILE_DER | FILE_SEARCH => {
                if offset > nbytes {
                    offset = nbytes;
                }
                ms.search.s = Some((s.data.clone(), s.off + offset, s.off + s.len));
                ms.search.s_len = nbytes - offset;
                ms.search.offset = offset;
                return;
            }
            FILE_REGEX => {
                if nbytes < offset {
                    ms.search.s_len = 0;
                    ms.search.s = None;
                    return;
                }
                let (linecnt, mut bytecnt) = if m.str_flags() & REGEX_LINE_COUNT != 0 {
                    (m.str_range() as usize, m.str_range() as usize * 80)
                } else {
                    (0, m.str_range() as usize)
                };
                if bytecnt == 0 || bytecnt > nbytes - offset {
                    bytecnt = nbytes - offset;
                }
                if bytecnt > ms.regex_max as usize {
                    bytecnt = ms.regex_max as usize;
                }
                let buf = offset;
                let end = offset + bytecnt;
                let mut last = end;
                let mut lines = linecnt;
                let mut b = buf;
                let bytes = s.bytes();
                let find = |from: usize, ch: u8| -> Option<usize> {
                    if from >= end {
                        return None;
                    }
                    bytes[from..end]
                        .iter()
                        .position(|&c| c == ch)
                        .map(|p| from + p)
                };
                while lines > 0 && b < end {
                    let c = b;
                    let nb = match find(c, b'\n').or_else(|| find(c, b'\r')) {
                        Some(x) => x,
                        None => break,
                    };
                    b = nb;
                    if b + 1 < end && bytes[b] == b'\r' && bytes[b + 1] == b'\n' {
                        b += 1;
                    }
                    if b + 1 < end && bytes[b] == b'\n' {
                        b += 1;
                    }
                    last = b;
                    lines -= 1;
                    b += 1;
                }
                if lines > 0 {
                    last = end;
                }
                ms.search.s = Some((s.data.clone(), s.off + buf, s.off + s.len));
                ms.search.s_len = last - buf;
                ms.search.offset = offset;
                ms.search.rm_len = 0;
                return;
            }
            FILE_BESTRING16 | FILE_LESTRING16 => {
                if offset < nbytes {
                    let mut src = offset + if typ == FILE_BESTRING16 { 1 } else { 0 };
                    let esrc = nbytes;
                    let p = &mut ms.ms_value.0;
                    let edst = MAXSTRING - 1;
                    let mut dst = 0usize;
                    while src < esrc {
                        if dst < edst {
                            p[dst] = s.at(src);
                        } else {
                            break;
                        }
                        if p[dst] == 0 {
                            let nz = if typ == FILE_BESTRING16 {
                                s.at(src - 1) != 0
                            } else {
                                src + 1 < esrc && s.at(src + 1) != 0
                            };
                            if nz {
                                p[dst] = b' ';
                            }
                        }
                        src += 2;
                        dst += 1;
                    }
                    p[edst] = 0;
                    p[dst] = 0;
                    return;
                }
            }
            FILE_STRING | FILE_PSTRING => {
                let r = m.str_range() as usize;
                if r != 0 && r < MAXSTRING {
                    size = r;
                }
            }
            _ => {}
        }
    }
    let p = &mut ms.ms_value.0;
    if typ == FILE_OFFSET {
        *p = [0; MAXSTRING];
        p[..8].copy_from_slice(&(offset as u64).to_le_bytes());
        return;
    }
    if offset >= nbytes {
        *p = [0; MAXSTRING];
        return;
    }
    let n = (nbytes - offset).min(size);
    for (i, d) in p.iter_mut().enumerate() {
        *d = if i < n { s.at(offset + i) } else { 0 };
    }
}

/// do_ops(): None on overflow
fn do_ops(m: &Magic, lhs: i64, off: i64) -> Option<u32> {
    const UINT_MAX: i64 = u32::MAX as i64;
    const INT_MIN: i64 = i32::MIN as i64;
    if lhs >= UINT_MAX || lhs <= INT_MIN || off >= UINT_MAX || off <= INT_MIN {
        return None;
    }
    let mut offset = if off != 0 {
        match m.in_op & FILE_OPS_MASK {
            FILE_OPAND => lhs & off,
            FILE_OPOR => lhs | off,
            FILE_OPXOR => lhs ^ off,
            FILE_OPADD => lhs + off,
            FILE_OPMINUS => lhs - off,
            FILE_OPMULTIPLY => lhs.wrapping_mul(off),
            FILE_OPDIVIDE => lhs / off,
            _ => lhs % off,
        }
    } else {
        lhs
    };
    if m.in_op & FILE_OPINVERSE != 0 {
        offset = !offset;
    }
    if offset >= UINT_MAX {
        return None;
    }
    Some(offset as u32)
}

/// msetoffset()
fn msetoffset(
    ms: &mut MagicSet,
    m: &Magic,
    bb: &mut BufRef,
    b: &Buffer,
    o: usize,
    cont_level: usize,
) -> Result<(), ()> {
    let normal = |ms: &mut MagicSet, bb: &mut BufRef, offset: i32| {
        *bb = b.fbuf.clone();
        ms.offset = offset as u32;
        ms.eoffset = 0;
    };
    if m.flag & OFFNEGATIVE != 0 {
        let offset = m.offset.wrapping_neg();
        if cont_level > 0 && m.flag & (OFFADD | INDIROFFADD) != 0 {
            normal(ms, bb, offset);
            return Ok(());
        }
        let eb = b.fill()?;
        if o != 0 {
            ms.magerror(format!("non zero offset {} at level {}", o, cont_level).into_bytes());
            return Err(());
        }
        let elen = eb.as_ref().map_or(0, |e| e.len);
        if m.offset as u32 as usize > elen {
            return Err(());
        }
        *bb = eb.unwrap_or_else(BufRef::empty);
        let v = (elen - m.offset as usize) as u32;
        ms.offset = v;
        ms.eoffset = v;
    } else {
        let offset = m.offset;
        if cont_level == 0 {
            normal(ms, bb, offset);
        } else {
            ms.offset = ms.eoffset.wrapping_add(offset as u32);
        }
    }
    Ok(())
}

fn oob(n: usize, o: i64, i: usize) -> bool {
    (n as u64) < (o as u32 as u64) || (i as u64) > (n as u64).wrapping_sub(o as u64)
}

fn sext(sgn: bool, bits: u32, v: u64) -> i64 {
    match (sgn, bits) {
        (true, 8) => v as i8 as i64,
        (true, 16) => v as i16 as i64,
        (true, 32) => v as i32 as i64,
        (true, _) => v as i64,
        (false, 8) => v as u8 as i64,
        (false, 16) => v as u16 as i64,
        (false, 32) => v as u32 as i64,
        (false, _) => v as i64,
    }
}

fn strtoull8(s: &[u8]) -> u64 {
    c_strtoull(cstr(s), 8).0
}

/// mget()
#[allow(clippy::too_many_arguments)]
fn mget(
    ms: &mut MagicSet,
    host: &mut dyn Host,
    m: &Magic,
    b: &Buffer,
    s: &BufRef,
    o: usize,
    cont_level: usize,
    mode: u8,
    text: bool,
    flip: bool,
    st: &mut St,
    returnval: &mut i32,
    found_match: &mut i32,
) -> i32 {
    let mut offset: u32 = ms.offset;
    let nbytes = s.len;
    if *st.indir_count >= ms.indir_max {
        ms.error(
            0,
            format!("indirect count ({}) exceeded", *st.indir_count).into_bytes(),
        );
        return -1;
    }
    if *st.name_count >= ms.name_max {
        ms.error(
            0,
            format!("name use count ({}) exceeded", *st.name_count).into_bytes(),
        );
        return -1;
    }
    mcopy(
        ms,
        m.typ,
        m.flag & INDIR != 0,
        s,
        (offset as usize).wrapping_add(o) as u32,
        nbytes as u32 as usize,
        m,
    );
    if m.flag & INDIR != 0 {
        let mut off: i64 = m.in_offset as i64;
        let sgn = m.in_op & FILE_OPSIGNED != 0;
        if m.in_op & FILE_OPINDIRECT != 0 {
            let base = offset as i64 + off;
            let hb = |i: usize| s.at((base as usize).wrapping_add(i));
            let hbv: Vec<u8> = (0..8).map(hb).collect();
            match cvt_flip(m.in_type, flip) {
                FILE_BYTE => {
                    if oob(nbytes, base, 1) {
                        return 0;
                    }
                    off = sext(sgn, 8, hbv[0] as u64);
                }
                FILE_SHORT | FILE_LESHORT => {
                    if oob(nbytes, base, 2) {
                        return 0;
                    }
                    off = sext(sgn, 16, le16(&hbv) as u64);
                }
                FILE_BESHORT => {
                    if oob(nbytes, base, 2) {
                        return 0;
                    }
                    off = sext(sgn, 16, be16(&hbv) as u64);
                }
                FILE_LONG | FILE_LEID3 | FILE_LELONG => {
                    if oob(nbytes, base, 4) {
                        return 0;
                    }
                    off = sext(sgn, 32, le32(&hbv) as u64);
                }
                FILE_BELONG | FILE_BEID3 => {
                    if oob(nbytes, base, 4) {
                        return 0;
                    }
                    off = sext(sgn, 32, be32(&hbv) as u64);
                }
                FILE_MELONG => {
                    if oob(nbytes, base, 4) {
                        return 0;
                    }
                    off = sext(sgn, 32, me32(&hbv) as u64);
                }
                FILE_BEQUAD => {
                    if oob(nbytes, base, 8) {
                        return 0;
                    }
                    off = sext(sgn, 64, be64(&hbv));
                }
                FILE_LEQUAD => {
                    if oob(nbytes, base, 8) {
                        return 0;
                    }
                    off = sext(sgn, 64, le64(&hbv));
                }
                FILE_OCTAL => {
                    if oob(nbytes, offset as i64, m.vallen as usize) {
                        return 0;
                    }
                    off = sext(sgn, 64, strtoull8(&ms.ms_value.0));
                }
                _ => return 0,
            }
        }
        let p = ms.ms_value.clone();
        let in_type = cvt_flip(m.in_type, flip);
        let (need, lhs): (usize, i64) = match in_type {
            FILE_BYTE => (1, sext(sgn, 8, p.b() as u64)),
            FILE_BESHORT => (2, sext(sgn, 16, be16(&p.0) as u64)),
            FILE_LESHORT => (2, sext(sgn, 16, le16(&p.0) as u64)),
            FILE_SHORT => (2, sext(sgn, 16, p.h() as u64)),
            FILE_BELONG | FILE_BEID3 => {
                let mut l = be32(&p.0);
                if in_type == FILE_BEID3 {
                    l = cvt_id3(l);
                }
                (4, sext(sgn, 32, l as u64))
            }
            FILE_LELONG | FILE_LEID3 => {
                let mut l = le32(&p.0);
                if in_type == FILE_LEID3 {
                    l = cvt_id3(l);
                }
                (4, sext(sgn, 32, l as u64))
            }
            FILE_MELONG => (4, sext(sgn, 32, me32(&p.0) as u64)),
            FILE_LONG => (4, sext(sgn, 32, p.l() as u64)),
            FILE_LEQUAD => (8, sext(sgn, 64, le64(&p.0))),
            FILE_BEQUAD => (8, sext(sgn, 64, be64(&p.0))),
            FILE_OCTAL => (m.vallen as usize, sext(sgn, 64, strtoull8(&p.0))),
            _ => return 0,
        };
        if oob(nbytes, offset as i64, need) {
            return 0;
        }
        match do_ops(m, lhs, off) {
            Some(v) => offset = v,
            None => return 0,
        }
        if m.flag & INDIROFFADD != 0 {
            if cont_level == 0 {
                return 0;
            }
            offset = offset.wrapping_add(ms.c[cont_level - 1].off as u32);
            if offset == 0 {
                return 0;
            }
        }
        mcopy(ms, m.typ, false, s, offset, nbytes, m);
        ms.offset = offset;
    }
    match m.typ {
        FILE_BYTE => {
            if oob(nbytes, offset as i64, 1) {
                return 0;
            }
        }
        FILE_SHORT | FILE_BESHORT | FILE_LESHORT => {
            if oob(nbytes, offset as i64, 2) {
                return 0;
            }
        }
        FILE_LONG | FILE_BELONG | FILE_LELONG | FILE_MELONG | FILE_DATE | FILE_BEDATE
        | FILE_LEDATE | FILE_MEDATE | FILE_LDATE | FILE_BELDATE | FILE_LELDATE | FILE_MELDATE
        | FILE_FLOAT | FILE_BEFLOAT | FILE_LEFLOAT => {
            if oob(nbytes, offset as i64, 4) {
                return 0;
            }
        }
        FILE_DOUBLE | FILE_BEDOUBLE | FILE_LEDOUBLE => {
            if oob(nbytes, offset as i64, 8) {
                return 0;
            }
        }
        FILE_GUID => {
            if oob(nbytes, offset as i64, 16) {
                return 0;
            }
        }
        FILE_STRING | FILE_PSTRING | FILE_SEARCH | FILE_OCTAL => {
            if oob(nbytes, offset as i64, m.vallen as usize) {
                return 0;
            }
        }
        FILE_REGEX => {
            if nbytes < offset as usize {
                return 0;
            }
        }
        FILE_INDIRECT => {
            if m.str_flags() & INDIRECT_RELATIVE != 0 {
                offset = offset.wrapping_add(o as u32);
            }
            if offset == 0 {
                return 0;
            }
            if nbytes < offset as usize {
                return 0;
            }
            let Some(pb) = ms.push_buffer() else {
                return -1;
            };
            *st.indir_count += 1;
            let bb = Buffer::new(
                b.fd.clone(),
                b.st_mode,
                b.st_size,
                s.sub(offset as usize, nbytes - offset as usize),
            );
            let mut rv = -1;
            let lists = ms.mlist[0].clone().unwrap_or_default();
            for ml in lists {
                let mut r0 = 0;
                let mut f0 = 0;
                rv = do_match(
                    ms, host, &ml, &bb, 0, BINTEST, text, false, st, &mut r0, &mut f0,
                );
                if rv != 0 {
                    break;
                }
            }
            let rbuf = ms.pop_buffer(pb);
            if rbuf.is_none() && ms.event_flags & EVENT_HAD_ERR != 0 {
                return -1;
            }
            if rv == 1 {
                if ms.flags & MAGIC_NODESC == 0
                    && ms.printf(cstr(&m.desc), &[Arg::I(offset as i64)]).is_err()
                {
                    return -1;
                }
                let r = match &rbuf {
                    Some(r) => ms.printf(b"%s", &[Arg::S(r)]),
                    None => ms.printf(b"(null)", &[]),
                };
                if r.is_err() {
                    return -1;
                }
            }
            return rv;
        }
        FILE_USE => {
            if nbytes < offset as usize {
                return 0;
            }
            let mut name = cstr(&m.value);
            let mut flip = flip;
            if name.first() == Some(&b'^') {
                name = &name[1..];
                flip = !flip;
            }
            let Some((ml, start, n)) = ms.magicfind(name) else {
                let mut e = b"cannot find entry `".to_vec();
                e.extend_from_slice(name);
                e.push(b'\'');
                ms.error(0, e);
                return -1;
            };
            let saved = ms.c.clone();
            let oneed_separator = *st.need_separator;
            if m.flag & NOSPACE != 0 {
                *st.need_separator = false;
            }
            let mut nfound_match = 0;
            *st.name_count += 1;
            let eoffset = ms.eoffset;
            let rv = do_match(
                ms,
                host,
                &ml[start..start + n],
                b,
                offset as usize + o,
                mode,
                text,
                flip,
                st,
                returnval,
                &mut nfound_match,
            );
            ms.ms_value.set_q(nfound_match as u64);
            *st.name_count -= 1;
            *found_match |= nfound_match;
            ms.c = saved;
            if rv != 1 {
                *st.need_separator = oneed_separator;
            }
            ms.offset = offset;
            ms.eoffset = eoffset;
            return (rv != 0 || *found_match != 0) as i32;
        }
        FILE_NAME => {
            if ms.flags & MAGIC_NODESC != 0 {
                return 1;
            }
            if ms.printf(b"%s", &[Arg::S(cstr(&m.desc))]).is_err() {
                return -1;
            }
            return 1;
        }
        _ => {}
    }
    if !mconvert(ms, m, flip) {
        return 0;
    }
    1
}

fn c_islower(c: u8) -> bool {
    c.is_ascii_lowercase()
}
fn c_isupper(c: u8) -> bool {
    c.is_ascii_uppercase()
}

/// file_strncmp(): `b` is the data
fn file_strncmp(a: &[u8], b: &dyn Fn(usize) -> u8, len: usize, maxlen: usize, flags: u32) -> u64 {
    let ai = |i: usize| *a.get(i).unwrap_or(&0);
    let ws = flags & (STRING_COMPACT_WHITESPACE | STRING_COMPACT_OPTIONAL_WHITESPACE);
    let eb = if ws != 0 { maxlen } else { len };
    let mut v: i64 = 0;
    let mut pa = 0usize;
    let mut pb = 0usize;
    let mut len = len + 1;
    if flags == 0 {
        loop {
            len -= 1;
            if len == 0 {
                break;
            }
            v = b(pb) as i64 - ai(pa) as i64;
            pb += 1;
            pa += 1;
            if v != 0 {
                break;
            }
        }
    } else {
        loop {
            len -= 1;
            if len == 0 {
                break;
            }
            if pb >= eb {
                v = 1;
                break;
            }
            let ca = ai(pa);
            if flags & STRING_IGNORE_LOWERCASE != 0 && c_islower(ca) {
                v = b(pb).to_ascii_lowercase() as i64 - ca as i64;
                pb += 1;
                pa += 1;
                if v != 0 {
                    break;
                }
            } else if flags & STRING_IGNORE_UPPERCASE != 0 && c_isupper(ca) {
                v = b(pb).to_ascii_uppercase() as i64 - ca as i64;
                pb += 1;
                pa += 1;
                if v != 0 {
                    break;
                }
            } else if flags & STRING_COMPACT_WHITESPACE != 0 && c_isspace(ca) {
                let mut i = 0u32;
                pa += 1;
                let cb = b(pb);
                pb += 1;
                if c_isspace(cb) {
                    if !c_isspace(ai(pa)) {
                        while i < 2048 && pb < eb && c_isspace(b(pb)) {
                            i += 1;
                            pb += 1;
                        }
                    }
                } else {
                    v = 1;
                    break;
                }
            } else if flags & STRING_COMPACT_OPTIONAL_WHITESPACE != 0 && c_isspace(ca) {
                pa += 1;
                while pb < eb && c_isspace(b(pb)) {
                    pb += 1;
                }
            } else {
                v = b(pb) as i64 - ca as i64;
                pb += 1;
                pa += 1;
                if v != 0 {
                    break;
                }
            }
        }
        if len == 0 && v == 0 && flags & STRING_FULL_WORD != 0 {
            let c = b(pb);
            if c != 0 && !c_isspace(c) {
                v = 1;
            }
        }
    }
    v as u64
}

/// magiccheck()
fn magiccheck(ms: &mut MagicSet, host: &mut dyn Host, m: &Magic) -> i32 {
    let mut l: u64 = m.value_q();
    let v: u64;
    let p = &ms.ms_value;
    match m.typ {
        FILE_BYTE => v = p.b() as u64,
        FILE_SHORT | FILE_BESHORT | FILE_LESHORT | FILE_MSDOSDATE | FILE_LEMSDOSDATE
        | FILE_BEMSDOSDATE | FILE_MSDOSTIME | FILE_LEMSDOSTIME | FILE_BEMSDOSTIME => {
            v = p.h() as u64
        }
        FILE_LONG | FILE_BELONG | FILE_LELONG | FILE_MELONG | FILE_DATE | FILE_BEDATE
        | FILE_LEDATE | FILE_MEDATE | FILE_LDATE | FILE_BELDATE | FILE_LELDATE | FILE_MELDATE => {
            v = p.l() as u64
        }
        FILE_QUAD | FILE_LEQUAD | FILE_BEQUAD | FILE_QDATE | FILE_BEQDATE | FILE_LEQDATE
        | FILE_QLDATE | FILE_BEQLDATE | FILE_LEQLDATE | FILE_QWDATE | FILE_BEQWDATE
        | FILE_LEQWDATE | FILE_OFFSET => v = p.q(),
        FILE_FLOAT | FILE_BEFLOAT | FILE_LEFLOAT => {
            let fl = m.value_f();
            let fv = p.f();
            return match m.reln {
                b'x' => 1,
                b'!' => (fl.is_nan() || fv.is_nan() || fv != fl) as i32,
                b'=' => (!(fl.is_nan() || fv.is_nan()) && fv == fl) as i32,
                b'>' => (fv > fl) as i32,
                b'<' => (fv < fl) as i32,
                r => {
                    ms.magerror(
                        format!("cannot happen with float: invalid relation `{}'", r as char)
                            .into_bytes(),
                    );
                    -1
                }
            };
        }
        FILE_DOUBLE | FILE_BEDOUBLE | FILE_LEDOUBLE => {
            let dl = m.value_d();
            let dv = p.d();
            return match m.reln {
                b'x' => 1,
                b'!' => (dl.is_nan() || dv.is_nan() || dv != dl) as i32,
                b'=' => (!(dl.is_nan() || dv.is_nan()) && dv == dl) as i32,
                b'>' => (dv > dl) as i32,
                b'<' => (dv < dl) as i32,
                r => {
                    ms.magerror(
                        format!(
                            "cannot happen with double: invalid relation `{}'",
                            r as char
                        )
                        .into_bytes(),
                    );
                    -1
                }
            };
        }
        FILE_DEFAULT | FILE_CLEAR => {
            l = 0;
            v = 0;
        }
        FILE_STRING | FILE_PSTRING | FILE_OCTAL | FILE_BESTRING16 | FILE_LESTRING16 => {
            l = 0;
            let data = p.0;
            let flags = if m.typ == FILE_BESTRING16 || m.typ == FILE_LESTRING16 {
                0
            } else {
                m.str_flags()
            };
            v = file_strncmp(
                &m.value,
                &|i| *data.get(i).unwrap_or(&0),
                m.vallen as usize,
                MAXSTRING,
                flags,
            );
        }
        FILE_SEARCH => {
            if ms.search.s.is_none() {
                return 0;
            }
            let slen = (m.vallen as usize).min(MAXSTRING);
            l = 0;
            let mut vv: u64 = 0;
            let srch = ms.search.clone();
            if slen > 0 && m.str_flags() == 0 {
                let mut idx = m.str_range() as usize + slen;
                if m.str_range() == 0 || srch.s_len < idx {
                    idx = srch.s_len;
                }
                let hay = srch.bytes(idx);
                let needle = &m.value[..slen];
                match hay.windows(slen).position(|w| w == needle) {
                    None => vv = 1,
                    Some(i) => {
                        ms.search.offset += i;
                        ms.search.rm_len = srch.s_len - i;
                    }
                }
            } else {
                let mut idx = 0usize;
                while m.str_range() == 0 || idx < m.str_range() as usize {
                    if slen + idx > srch.s_len {
                        vv = 1;
                        break;
                    }
                    let base = idx;
                    vv = file_strncmp(
                        &m.value,
                        &|i| srch.at(base + i),
                        slen,
                        srch.s_len - idx,
                        m.str_flags(),
                    );
                    if vv == 0 {
                        ms.search.offset += idx;
                        ms.search.rm_len = srch.s_len - idx;
                        break;
                    }
                    idx += 1;
                }
            }
            v = vv;
        }
        FILE_REGEX => {
            let pat = convert_libmagic_pattern(
                &m.value[..m.vallen as usize],
                m.str_flags() & STRING_IGNORE_CASE != 0,
                true,
            );
            l = 0;
            if !host.regex_compile(&pat) {
                return -1;
            }
            let hay = ms.search.bytes(ms.search.s_len);
            match host.regex_match(&pat, &hay) {
                Err(()) => v = 1,
                Ok(None) => v = 1,
                Ok(Some((start, len))) => {
                    if let Some((_, s, _)) = ms.search.s.as_mut() {
                        *s += start;
                    }
                    ms.search.offset += start;
                    ms.search.rm_len = len;
                    v = 0;
                }
            }
        }
        FILE_USE => return (ms.ms_value.q() != 0) as i32,
        FILE_NAME | FILE_INDIRECT => return 1,
        FILE_DER => {
            let r = der_cmp(ms, m);
            return if r == -1 { 0 } else { r };
        }
        FILE_GUID => {
            l = 0;
            let mut d: i64 = 0;
            for i in 0..16 {
                if m.value[i] != p.0[i] {
                    d = m.value[i] as i64 - p.0[i] as i64;
                    break;
                }
            }
            v = d as u64;
        }
        t => {
            ms.magerror(format!("invalid type {} in magiccheck()", t).into_bytes());
            return -1;
        }
    }
    let v = ms.signextend(m, v, host);
    let matched = match m.reln {
        b'x' => true,
        b'!' => v != l,
        b'=' => v == l,
        b'>' => {
            if m.flag & UNSIGNED != 0 {
                v > l
            } else {
                (v as i64) > (l as i64)
            }
        }
        b'<' => {
            if m.flag & UNSIGNED != 0 {
                v < l
            } else {
                (v as i64) < (l as i64)
            }
        }
        b'&' => (v & l) == l,
        b'^' => (v & l) != l,
        r => {
            ms.magerror(format!("cannot happen: invalid relation `{}'", r as char).into_bytes());
            return -1;
        }
    };
    matched as i32
}

/// handle_annotation()
fn handle_annotation(ms: &mut MagicSet, m: &Magic, firstline: bool) -> i32 {
    if ms.flags & MAGIC_APPLE != 0 && m.apple[0] != 0 {
        if print_sep(ms, firstline).is_err() {
            return -1;
        }
        let a = cstr(&m.apple).to_vec();
        if ms.printf(b"%s", &[Arg::S(&a)]).is_err() {
            return -1;
        }
        return 1;
    }
    if ms.flags & MAGIC_EXTENSION != 0 && m.ext[0] != 0 {
        if print_sep(ms, firstline).is_err() {
            return -1;
        }
        if ms.printf(b"%s", &[Arg::S(cstr(&m.ext))]).is_err() {
            return -1;
        }
        return 1;
    }
    if ms.flags & MAGIC_MIME_TYPE != 0 && m.mimetype[0] != 0 {
        if print_sep(ms, firstline).is_err() {
            return -1;
        }
        let raw = cstr(&m.mimetype);
        let p = varexpand(ms.mode, raw, 1024).unwrap_or_else(|| raw.to_vec());
        if ms.printf(b"%s", &[Arg::S(&p)]).is_err() {
            return -1;
        }
        return 1;
    }
    0
}

fn print_sep(ms: &mut MagicSet, firstline: bool) -> Result<(), ()> {
    if firstline {
        return Ok(());
    }
    ms.separator()
}

// ---------------------------------------------------------------------------
// der.c

const DER_BAD: u32 = u32::MAX;

static DER_TAGS: [&str; 37] = [
    "eoc",
    "bool",
    "int",
    "bit_str",
    "octet_str",
    "null",
    "obj_id",
    "obj_desc",
    "ext",
    "real",
    "enum",
    "embed",
    "utf8_str",
    "rel_oid",
    "time",
    "res2",
    "seq",
    "set",
    "num_str",
    "prt_str",
    "t61_str",
    "vid_str",
    "ia5_str",
    "utc_time",
    "gen_time",
    "gr_str",
    "vis_str",
    "gen_str",
    "univ_str",
    "char_str",
    "bmp_str",
    "date",
    "tod",
    "datetime",
    "duration",
    "oid-iri",
    "rel-oid-iri",
];

fn gettag(c: &dyn Fn(usize) -> u8, p: &mut usize, l: usize) -> u32 {
    if *p >= l {
        return DER_BAD;
    }
    let mut tag = (c(*p) & 0x1f) as u32;
    *p += 1;
    if tag != 0x1f {
        return tag;
    }
    if *p >= l {
        return DER_BAD;
    }
    while c(*p) >= 0x80 {
        tag = tag.wrapping_mul(128).wrapping_add(c(*p) as u32 - 0x80);
        *p += 1;
        if *p >= l {
            return DER_BAD;
        }
    }
    tag
}

fn getlength(c: &dyn Fn(usize) -> u8, p: &mut usize, l: usize) -> u32 {
    if *p >= l {
        return DER_BAD;
    }
    let onebyte = c(*p) & 0x80 == 0;
    let digits = (c(*p) & 0x7f) as usize;
    *p += 1;
    if *p + digits >= l {
        return DER_BAD;
    }
    if onebyte {
        return digits as u32;
    }
    let mut len: u64 = 0;
    for _ in 0..digits {
        len = (len << 8) | c(*p) as u64;
        *p += 1;
    }
    if len > u32::MAX as u64 - *p as u64 || *p as u64 + len > l as u64 {
        return DER_BAD;
    }
    len as u32
}

fn der_tag(tag: u32) -> String {
    if (tag as usize) < 0x25 {
        DER_TAGS[tag as usize].to_string()
    } else {
        format!("{:#x}", tag)
    }
}

fn der_data(tag: u32, d: &dyn Fn(usize) -> u8, len: u32) -> Vec<u8> {
    const BLEN: usize = 128;
    match tag {
        0x13 | 0x0c | 0x16 => {
            let mut out = Vec::new();
            for i in 0..len as usize {
                let c = d(i);
                if c == 0 || out.len() >= BLEN - 1 {
                    break;
                }
                out.push(c);
            }
            return out;
        }
        0x17 if len >= 12 => {
            let c: Vec<char> = (0..12).map(|i| d(i) as char).collect();
            let s = format!(
                "20{}{}-{}{}-{}{} {}{}:{}{}:{}{} GMT",
                c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7], c[8], c[9], c[10], c[11]
            );
            let mut b = s.into_bytes();
            b.truncate(BLEN - 1);
            return b;
        }
        _ => {}
    }
    let mut out = Vec::new();
    for i in 0..len as usize {
        let z = i << 1;
        if z < BLEN - 2 {
            out.extend_from_slice(format!("{:02x}", d(i)).as_bytes());
            out.truncate(BLEN - 1);
        }
    }
    out
}

/// der_offs()
fn der_offs(ms: &mut MagicSet, m: &Magic, nbytes: usize) -> i32 {
    let srch = ms.search.clone();
    let b = |i: usize| srch.at(i);
    let mut offs = 0usize;
    let len = if srch.s_len != 0 { srch.s_len } else { nbytes };
    if gettag(&b, &mut offs, len) == DER_BAD {
        return -1;
    }
    let tlen = getlength(&b, &mut offs, len);
    if tlen == DER_BAD {
        return -1;
    }
    let offs = offs
        .wrapping_add(ms.offset as usize)
        .wrapping_add(m.offset as isize as usize);
    if m.cont_level != 0 {
        if offs + tlen as usize > nbytes {
            return -1;
        }
        ms.c[m.cont_level as usize - 1].off = (offs + tlen as usize) as i32;
    }
    offs as i32
}

/// der_cmp()
fn der_cmp(ms: &mut MagicSet, m: &Magic) -> i32 {
    let srch = ms.search.clone();
    let b = |i: usize| srch.at(i);
    let s = cstr(&m.value);
    let mut offs = 0usize;
    let len = srch.s_len;
    let tag = gettag(&b, &mut offs, len);
    if tag == DER_BAD {
        return -1;
    }
    let tlen = getlength(&b, &mut offs, len);
    if tlen == DER_BAD {
        return -1;
    }
    let buf = der_tag(tag);
    let slen = buf.len();
    if !s.starts_with(buf.as_bytes()) {
        // strncmp(buf, s, slen) on a shorter s
        return 0;
    }
    let mut si = slen;
    let at = |i: usize| *s.get(i).unwrap_or(&0);
    loop {
        match at(si) {
            0 => return 1,
            b'=' => {
                si += 1;
                break;
            }
            c if c.is_ascii_digit() => {
                let mut n = 0usize;
                while at(si).is_ascii_digit() {
                    n = n * 10 + (at(si) - b'0') as usize;
                    si += 1;
                }
                if tlen as usize != n {
                    return 0;
                }
            }
            _ => return 0,
        }
    }
    let o = offs;
    let data = der_data(tag, &|i| b(o + i), tlen);
    let rest = &s[si.min(s.len())..];
    if data != rest && rest != b"x" {
        return 0;
    }
    let v = &mut ms.ms_value.0;
    *v = [0; MAXSTRING];
    let n = data.len().min(MAXSTRING - 1);
    v[..n].copy_from_slice(&data[..n]);
    1
}
