//! funcs.c, ascmagic.c, print.c, is_tar.c, is_json.c, is_csv.c,
//! is_simh.c and magic.c's buffer entry point.

use crate::encoding::file_encoding;
use crate::fmt::{self, Arg};
use crate::magic::*;
use crate::softmagic::file_softmagic;

pub const FILE_T_LOCAL: i32 = 1;
pub const FILE_T_WINDOWS: i32 = 2;

const FILE_SEPARATOR: &[u8] = b"\n- ";

/// file_checkfmt()
fn checkfmt(fmt: &[u8]) -> Result<(), String> {
    let at = |i: usize| *fmt.get(i).unwrap_or(&0);
    let mut p = 0usize;
    while at(p) != 0 {
        if at(p) != b'%' {
            p += 1;
            continue;
        }
        p += 1;
        if at(p) == b'%' {
            p += 1;
            continue;
        }
        while at(p) != 0 && b"#0.'+- ".contains(&at(p)) {
            p += 1;
        }
        if at(p) == b'*' {
            return Err("* not allowed in format".into());
        }
        let field = |p: &mut usize, what: &str| -> Result<(), String> {
            let mut fw = 0u64;
            while at(*p).is_ascii_digit() {
                fw = fw * 10 + (at(*p) - b'0') as u64;
                *p += 1;
            }
            if fw < 1024 {
                Ok(())
            } else {
                Err(format!("field {} too large: {}", what, fw))
            }
        };
        field(&mut p, "width")?;
        if at(p) == b'.' {
            p += 1;
            field(&mut p, "precision")?;
        }
        if !at(p).is_ascii_alphabetic() {
            return Err(format!("bad format char: {}", at(p) as char));
        }
        p += 1;
    }
    Ok(())
}

pub struct PushBuf {
    buf: Option<Vec<u8>>,
    offset: u32,
}

impl MagicSet {
    fn clearbuf(&mut self) {
        self.o_buf = None;
    }

    /// file_printf()
    pub fn printf(&mut self, f: &[u8], args: &[Arg]) -> Result<(), ()> {
        if self.event_flags & EVENT_HAD_ERR != 0 {
            return Ok(());
        }
        let f = cstr(f);
        if let Err(e) = checkfmt(f) {
            self.clearbuf();
            let mut m = b"Bad magic format `".to_vec();
            m.extend_from_slice(f);
            m.extend_from_slice(format!("' ({})", e).as_bytes());
            self.error(0, m);
            return Err(());
        }
        let s = fmt::format(f, args);
        let blen = self.o_buf.as_ref().map_or(0, |b| b.len());
        if s.len() > 1024 || s.len() + blen > 1024 * 1024 {
            self.clearbuf();
            self.error(
                0,
                format!("Output buffer space exceeded {}+{}", s.len(), blen).into_bytes(),
            );
            return Err(());
        }
        self.o_buf
            .get_or_insert_with(Vec::new)
            .extend_from_slice(&s);
        Ok(())
    }

    fn append_raw(&mut self, s: &[u8]) {
        self.o_buf.get_or_insert_with(Vec::new).extend_from_slice(s);
    }

    fn error_core(&mut self, error: i32, msg: Vec<u8>, lineno: usize) {
        if self.event_flags & EVENT_HAD_ERR != 0 {
            return;
        }
        if lineno != 0 {
            self.clearbuf();
            self.append_raw(format!("line {}:", lineno).as_bytes());
        }
        if self.o_buf.as_ref().is_some_and(|b| !b.is_empty()) {
            self.append_raw(b" ");
        }
        self.append_raw(&msg);
        if error > 0 {
            let e = std::io::Error::from_raw_os_error(error).to_string();
            let e = e.split(" (os error").next().unwrap_or("").to_string();
            self.append_raw(format!(" ({})", e).as_bytes());
        }
        self.event_flags |= EVENT_HAD_ERR;
        self.error = error;
    }

    /// file_error()
    pub fn error(&mut self, error: i32, msg: Vec<u8>) {
        self.error_core(error, msg, 0);
    }

    /// file_magerror()
    pub fn magerror(&mut self, msg: Vec<u8>) {
        let line = self.line;
        self.error_core(0, msg, line);
    }

    pub fn separator(&mut self) -> Result<(), ()> {
        self.printf(FILE_SEPARATOR, &[])
    }

    /// file_reset(): Err when nothing is loaded
    pub fn reset(&mut self, checkloaded: bool) -> Result<(), ()> {
        if checkloaded && self.mlist[0].is_none() {
            self.error(0, b"no magic files loaded".to_vec());
            return Err(());
        }
        self.clearbuf();
        self.event_flags &= !EVENT_HAD_ERR;
        self.error = -1;
        Ok(())
    }

    pub fn push_buffer(&mut self) -> Option<PushBuf> {
        if self.event_flags & EVENT_HAD_ERR != 0 {
            return None;
        }
        let pb = PushBuf {
            buf: self.o_buf.take(),
            offset: self.offset,
        };
        self.offset = 0;
        Some(pb)
    }

    pub fn pop_buffer(&mut self, pb: PushBuf) -> Option<Vec<u8>> {
        if self.event_flags & EVENT_HAD_ERR != 0 {
            return None;
        }
        let r = self.o_buf.take();
        self.o_buf = pb.buf;
        self.offset = pb.offset;
        r
    }

    fn printedlen(&self) -> usize {
        self.o_buf.as_ref().map_or(0, |b| b.len())
    }

    /// file_getbuffer()
    pub fn getbuffer(&self) -> Option<Vec<u8>> {
        if self.event_flags & EVENT_HAD_ERR != 0 {
            return None;
        }
        let buf = self.o_buf.as_ref()?;
        if self.flags & MAGIC_RAW != 0 {
            return Some(buf.clone());
        }
        let buf = cstr(buf);
        let octalify = |out: &mut Vec<u8>, c: u8| {
            out.push(b'\\');
            out.push(b'0' + ((c >> 6) & 3));
            out.push(b'0' + ((c >> 3) & 7));
            out.push(b'0' + (c & 7));
        };
        let mut out = Vec::with_capacity(buf.len());
        // mbrtowc() in the C.UTF-8 LC_CTYPE PHP sets up
        if let Ok(s) = std::str::from_utf8(buf) {
            for ch in s.chars() {
                let mut tmp = [0u8; 4];
                let enc = ch.encode_utf8(&mut tmp).as_bytes();
                let c = ch as u32;
                let printable = c >= 0x20 && c != 0x7f && !(0x80..0xa0).contains(&c);
                if printable {
                    out.extend_from_slice(enc);
                } else {
                    for &b in enc {
                        octalify(&mut out, b);
                    }
                }
            }
            return Some(out);
        }
        for &c in buf {
            if c_isprint(c) {
                out.push(c);
            } else {
                octalify(&mut out, c);
            }
        }
        Some(out)
    }

    /// The text of magic_error()
    pub fn error_text(&self) -> Option<Vec<u8>> {
        if self.event_flags & EVENT_HAD_ERR != 0 {
            Some(self.o_buf.clone().unwrap_or_default())
        } else {
            None
        }
    }

    pub fn errno(&self) -> i32 {
        if self.event_flags & EVENT_HAD_ERR != 0 {
            self.error
        } else {
            0
        }
    }

    /// file_replace() for the patterns ascmagic uses: "<lit>$" (multiline)
    fn replace_eol(&mut self, lit: &[u8], rep: &[u8]) -> usize {
        let Some(buf) = self.o_buf.as_ref() else {
            return 0;
        };
        let buf = cstr(buf).to_vec();
        let mut out = Vec::with_capacity(buf.len());
        let mut n = 0;
        let mut i = 0;
        while i < buf.len() {
            if buf[i..].starts_with(lit) {
                let e = i + lit.len();
                if e == buf.len() || buf[e] == b'\n' {
                    out.extend_from_slice(rep);
                    i = e;
                    n += 1;
                    continue;
                }
            }
            out.push(buf[i]);
            i += 1;
        }
        self.o_buf = Some(out);
        n
    }
}

fn trim_nuls(buf: &[u8]) -> usize {
    let mut n = buf.len();
    while n > 1 && buf[n - 1] == 0 {
        n -= 1;
    }
    n
}

/// file_strtrim()
pub fn strtrim(s: &[u8]) -> &[u8] {
    let mut a = 0;
    while a < s.len() && c_isspace(s[a]) {
        a += 1;
    }
    let mut e = s.len();
    while e > a && c_isspace(s[e - 1]) {
        e -= 1;
    }
    &s[a..e]
}

// ---------------------------------------------------------------------------
// print.c

static DAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
static MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

fn asctime(tm: &libc::tm) -> Vec<u8> {
    let wd = DAYS.get(tm.tm_wday as usize).copied().unwrap_or("???");
    let mo = MONTHS.get(tm.tm_mon as usize).copied().unwrap_or("???");
    format!(
        "{} {}{:3} {:02}:{:02}:{:02} {}",
        wd,
        mo,
        tm.tm_mday,
        tm.tm_hour,
        tm.tm_min,
        tm.tm_sec,
        tm.tm_year as i64 + 1900
    )
    .into_bytes()
}

fn breakdown(t: i64, local: bool) -> Option<libc::tm> {
    let tt = t as libc::time_t;
    // SAFETY: an all-zero libc::tm is a valid value for the out parameter.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    // SAFETY: the time and the tm out parameter are valid for the call.
    let r = unsafe {
        if local {
            libc::localtime_r(&tt, &mut tm)
        } else {
            libc::gmtime_r(&tt, &mut tm)
        }
    };
    if r.is_null() { None } else { Some(tm) }
}

/// cdf_timestamp_to_timespec(): the seconds (mktime() of the date)
pub fn cdf_timestamp_to_sec(t: i64) -> Option<i64> {
    fn isleap(y: i64) -> bool {
        y % 4 == 0 && (y % 100 != 0 || y % 400 == 0)
    }
    const MDAYS: [i64; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut t = t / 10_000_000;
    // SAFETY: an all-zero libc::tm is valid.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    tm.tm_sec = (t % 60) as i32;
    t /= 60;
    tm.tm_min = (t % 60) as i32;
    t /= 60;
    tm.tm_hour = (t % 24) as i32;
    t /= 24;
    let year = 1601 + t / 365;
    let mut rdays = 0i64;
    for y in 1601..year {
        rdays += isleap(y) as i64 + 365;
    }
    t -= rdays - 1;
    let mut days = t;
    let mut mday = days;
    for (m, &md) in MDAYS.iter().enumerate() {
        let sub = md + (m == 1 && isleap(year)) as i64;
        if mday < sub {
            break;
        }
        mday -= sub;
    }
    let mut mon = 12;
    for (m, &md) in MDAYS.iter().enumerate() {
        days -= md;
        if m == 1 && isleap(year) {
            days -= 1;
        }
        if days <= 0 {
            mon = m;
            break;
        }
    }
    tm.tm_mday = mday as i32;
    tm.tm_mon = mon as i32;
    tm.tm_year = (year - 1900) as i32;
    tm.tm_isdst = 0;
    // SAFETY: tm is a valid, initialized struct tm.
    let s = unsafe { libc::mktime(&mut tm) };
    if s == -1 { None } else { Some(s as i64) }
}

/// cdf_ctime() without the newline
pub fn cdf_ctime(sec: i64) -> Vec<u8> {
    if sec <= MAX_CTIME
        && let Some(tm) = breakdown(sec, true)
    {
        return asctime(&tm);
    }
    format!("*Bad* {:#016x}", sec).into_bytes()
}

/// file_fmtdatetime()
pub fn fmtdatetime(v: u64, flags: i32) -> Vec<u8> {
    let t: i64 = if flags & FILE_T_WINDOWS != 0 {
        match cdf_timestamp_to_sec(v as i64) {
            Some(s) => s,
            None => return b"*Invalid datetime*".to_vec(),
        }
    } else {
        v as i64
    };
    if t > MAX_CTIME {
        return b"*Invalid datetime*".to_vec();
    }
    match breakdown(t, flags & FILE_T_LOCAL != 0) {
        Some(tm) => asctime(&tm),
        None => b"*Invalid datetime*".to_vec(),
    }
}

/// file_fmtdate(): strftime("%a, %b %d %Y") of an MS-DOS date
pub fn fmtdate(v: u16) -> Vec<u8> {
    let mday = v & 0x1f;
    let mon = ((v >> 5) & 0xf) as i32 - 1;
    let year = (v >> 9) as i32 + 80 + 1900;
    let mo = if (0..12).contains(&mon) {
        MONTHS[mon as usize]
    } else {
        "?"
    };
    format!("Sun, {} {:02} {}", mo, mday, year).into_bytes()
}

/// file_fmttime(): strftime("%T") of an MS-DOS time
pub fn fmttime(v: u16) -> Vec<u8> {
    format!(
        "{:02}:{:02}:{:02}",
        v >> 11,
        (v >> 5) & 0x3f,
        (v & 0x1f) * 2
    )
    .into_bytes()
}

/// file_fmtnum()
pub fn fmtnum(us: &[u8], base: u32) -> Vec<u8> {
    let (v, n, ovf) = c_strtoull(us, base);
    if n < us.len() || ovf {
        return b"*Invalid number*".to_vec();
    }
    v.to_string().into_bytes()
}

// ---------------------------------------------------------------------------
// is_tar.c

fn from_oct(w: &[u8]) -> i64 {
    let mut digs = w.len();
    let mut p = 0usize;
    let at = |i: usize| *w.get(i).unwrap_or(&0);
    if digs == 0 {
        return -1;
    }
    while c_isspace(at(p)) {
        p += 1;
        if digs == 0 {
            return -1;
        }
        digs -= 1;
    }
    let mut value: i64 = 0;
    while digs > 0 && (b'0'..=b'7').contains(&at(p)) {
        value = (value << 3) | (at(p) - b'0') as i64;
        p += 1;
        digs -= 1;
    }
    if digs > 0 && at(p) != 0 && !c_isspace(at(p)) {
        return -1;
    }
    value as i32 as i64
}

fn c_strncmp(a: &[u8], b: &[u8], n: usize) -> bool {
    for i in 0..n {
        let x = *a.get(i).unwrap_or(&0);
        let y = *b.get(i).unwrap_or(&0);
        if x != y {
            return false;
        }
        if x == 0 {
            return true;
        }
    }
    true
}

fn is_tar(buf: &[u8]) -> i32 {
    if buf.len() < 512 {
        return 0;
    }
    let name = &buf[..100];
    if let Some(nul) = name.iter().position(|&c| c == 0)
        && nul >= 8
        && &name[nul - 7..nul] == b"/gpkg-1"
    {
        return 0;
    }
    let recsum = from_oct(&buf[148..156]);
    let mut sum: i64 = buf[..512].iter().map(|&c| c as i64).sum();
    for &c in &buf[148..156] {
        sum -= c as i64;
    }
    sum += b' ' as i64 * 8;
    if sum != recsum {
        return 0;
    }
    if c_strncmp(&buf[257..265], b"ustar  \0", 8) {
        return 3;
    }
    if c_strncmp(&buf[257..265], b"ustar\0", 8) {
        return 2;
    }
    1
}

fn file_is_tar(ms: &mut MagicSet, buf: &[u8]) -> i32 {
    let mime = ms.flags & MAGIC_MIME;
    if ms.flags & (MAGIC_APPLE | MAGIC_EXTENSION) != 0 {
        return 0;
    }
    let tar = is_tar(buf);
    if !(1..=3).contains(&tar) {
        return 0;
    }
    if mime == MAGIC_MIME_ENCODING {
        return 1;
    }
    let names = [
        "tar archive",
        "POSIX tar archive",
        "POSIX tar archive (GNU)",
    ];
    let s: &str = if mime != 0 {
        "application/x-tar"
    } else {
        names[tar as usize - 1]
    };
    if ms.printf(b"%s", &[Arg::S(s.as_bytes())]).is_err() {
        return -1;
    }
    1
}

// ---------------------------------------------------------------------------
// is_json.c

const JSON_ARRAY: usize = 0;
const JSON_CONSTANT: usize = 1;
const JSON_NUMBER: usize = 2;
const JSON_OBJECT: usize = 3;
const JSON_STRING: usize = 4;
const JSON_ARRAYN: usize = 5;

fn json_isspace(c: u8) -> bool {
    matches!(c, b' ' | b'\n' | b'\r' | b'\t')
}

struct Json<'a> {
    b: &'a [u8],
}

impl Json<'_> {
    fn ue(&self) -> usize {
        self.b.len()
    }
    fn skip_space(&self, mut uc: usize) -> usize {
        while uc < self.ue() && json_isspace(self.b[uc]) {
            uc += 1;
        }
        uc
    }
    fn parse_string(&self, ucp: &mut usize) -> bool {
        let ue = self.ue();
        let mut uc = *ucp;
        while uc < ue {
            let c = self.b[uc];
            uc += 1;
            match c {
                0 => break,
                b'\\' => {
                    if uc == ue {
                        break;
                    }
                    let c = self.b[uc];
                    uc += 1;
                    match c {
                        0 => break,
                        b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't' => continue,
                        b'u' => {
                            if ue - uc < 4 {
                                uc = ue;
                                break;
                            }
                            let mut ok = true;
                            for _ in 0..4 {
                                let h = self.b[uc];
                                uc += 1;
                                if !h.is_ascii_hexdigit() {
                                    ok = false;
                                    break;
                                }
                            }
                            if !ok {
                                break;
                            }
                            continue;
                        }
                        _ => break,
                    }
                }
                b'"' => {
                    *ucp = uc;
                    return true;
                }
                _ => continue,
            }
        }
        *ucp = uc;
        false
    }
    fn parse_array(&self, ucp: &mut usize, st: &mut [usize; 6], lvl: usize) -> bool {
        let ue = self.ue();
        let mut uc = *ucp;
        while uc < ue {
            uc = self.skip_space(uc);
            if uc == ue {
                break;
            }
            if self.b[uc] == b']' {
                st[JSON_ARRAYN] += 1;
                *ucp = uc + 1;
                return true;
            }
            if !self.parse(&mut uc, st, lvl + 1) {
                break;
            }
            if uc == ue {
                break;
            }
            match self.b[uc] {
                b',' => {
                    uc += 1;
                    continue;
                }
                b']' => {
                    st[JSON_ARRAYN] += 1;
                    *ucp = uc + 1;
                    return true;
                }
                _ => break,
            }
        }
        *ucp = uc;
        false
    }
    fn parse_object(&self, ucp: &mut usize, st: &mut [usize; 6], lvl: usize) -> bool {
        let ue = self.ue();
        let mut uc = *ucp;
        while uc < ue {
            uc = self.skip_space(uc);
            if uc == ue {
                break;
            }
            if self.b[uc] == b'}' {
                *ucp = uc + 1;
                return true;
            }
            let c = self.b[uc];
            uc += 1;
            if c != b'"' {
                break;
            }
            if !self.parse_string(&mut uc) {
                break;
            }
            uc = self.skip_space(uc);
            if uc == ue {
                break;
            }
            let c = self.b[uc];
            uc += 1;
            if c != b':' {
                break;
            }
            if !self.parse(&mut uc, st, lvl + 1) {
                break;
            }
            if uc == ue {
                break;
            }
            let c = self.b[uc];
            uc += 1;
            match c {
                b',' => continue,
                b'}' => {
                    *ucp = uc;
                    return true;
                }
                _ => {
                    *ucp = uc - 1;
                    return false;
                }
            }
        }
        *ucp = uc;
        false
    }
    fn parse_number(&self, ucp: &mut usize) -> bool {
        let ue = self.ue();
        let mut uc = *ucp;
        let mut got = false;
        if uc == ue {
            return false;
        }
        if self.b[uc] == b'-' {
            uc += 1;
        }
        while uc < ue && self.b[uc].is_ascii_digit() {
            got = true;
            uc += 1;
        }
        if uc != ue {
            if self.b[uc] == b'.' {
                uc += 1;
            }
            while uc < ue && self.b[uc].is_ascii_digit() {
                got = true;
                uc += 1;
            }
            if uc != ue && got && (self.b[uc] == b'e' || self.b[uc] == b'E') {
                uc += 1;
                got = false;
                if uc != ue {
                    if self.b[uc] == b'+' || self.b[uc] == b'-' {
                        uc += 1;
                    }
                    while uc < ue && self.b[uc].is_ascii_digit() {
                        got = true;
                        uc += 1;
                    }
                }
            }
        }
        *ucp = uc;
        got
    }
    fn parse_const(&self, ucp: &mut usize, s: &[u8]) -> bool {
        // s includes the trailing NUL in C's sizeof
        let ue = self.ue();
        let mut uc = *ucp;
        let mut len = s.len() + 1;
        len -= 1;
        *ucp = (*ucp + len - 1).min(ue);
        let mut si = 0usize;
        while uc < ue {
            len -= 1;
            if len == 0 {
                break;
            }
            si += 1;
            let c = self.b[uc];
            uc += 1;
            if c != *s.get(si).unwrap_or(&0) {
                return false;
            }
        }
        true
    }
    fn parse(&self, ucp: &mut usize, st: &mut [usize; 6], lvl: usize) -> bool {
        self.parse_r(ucp, st, lvl) != 0
    }
    fn parse_r(&self, ucp: &mut usize, st: &mut [usize; 6], lvl: usize) -> i32 {
        let ue = self.ue();
        let mut uc = self.skip_space(*ucp);
        let ouc = uc;
        let mut rv = false;
        if uc != ue {
            if lvl > 500 {
                return 0;
            }
            let c = self.b[uc];
            uc += 1;
            let t;
            match c {
                b'"' => {
                    rv = self.parse_string(&mut uc);
                    t = JSON_STRING;
                }
                b'[' => {
                    rv = self.parse_array(&mut uc, st, lvl + 1);
                    t = JSON_ARRAY;
                }
                b'{' => {
                    rv = self.parse_object(&mut uc, st, lvl + 1);
                    t = JSON_OBJECT;
                }
                b't' => {
                    rv = self.parse_const(&mut uc, b"true");
                    t = JSON_CONSTANT;
                }
                b'f' => {
                    rv = self.parse_const(&mut uc, b"false");
                    t = JSON_CONSTANT;
                }
                b'n' => {
                    rv = self.parse_const(&mut uc, b"null");
                    t = JSON_CONSTANT;
                }
                _ => {
                    uc -= 1;
                    rv = self.parse_number(&mut uc);
                    t = JSON_NUMBER;
                }
            }
            if rv {
                st[t] += 1;
            }
            uc = self.skip_space(uc);
        }
        *ucp = uc;
        if lvl == 0 {
            if !rv {
                return 0;
            }
            if uc == ue {
                return if st[JSON_ARRAYN] != 0 || st[JSON_OBJECT] != 0 {
                    1
                } else {
                    0
                };
            }
            let mut uc2 = uc;
            if self.b[ouc] == self.b[uc] && self.parse(&mut uc2, st, 1) {
                return if st[JSON_ARRAYN] != 0 || st[JSON_OBJECT] != 0 {
                    2
                } else {
                    0
                };
            }
            return 0;
        }
        rv as i32
    }
}

fn file_is_json(ms: &mut MagicSet, buf: &[u8]) -> i32 {
    let mime = ms.flags & MAGIC_MIME;
    if ms.flags & (MAGIC_APPLE | MAGIC_EXTENSION) != 0 {
        return 0;
    }
    let mut st = [0usize; 6];
    let j = Json { b: buf };
    let mut uc = 0usize;
    let jt = j.parse_r(&mut uc, &mut st, 0);
    if jt == 0 {
        return 0;
    }
    if mime == MAGIC_MIME_ENCODING {
        return 1;
    }
    let r = if mime != 0 {
        ms.printf(
            b"application/%s",
            &[Arg::S(if jt == 1 { b"json" } else { b"x-ndjson" })],
        )
    } else {
        ms.printf(
            b"%sJSON text data",
            &[Arg::S(if jt == 1 { b"" } else { b"New Line Delimited " })],
        )
    };
    if r.is_err() {
        return -1;
    }
    1
}

// ---------------------------------------------------------------------------
// is_csv.c

fn csv_parse(b: &[u8]) -> bool {
    let (mut nf, mut tf, mut nl) = (0usize, 0usize, 0usize);
    let mut uc = 0usize;
    let ue = b.len();
    while uc < ue {
        let c = b[uc];
        uc += 1;
        match c {
            b'"' => {
                // eatquote()
                let mut quote = false;
                let mut done = false;
                while uc < ue {
                    let c = b[uc];
                    uc += 1;
                    if c != b'"' {
                        if quote {
                            uc -= 1;
                            done = true;
                            break;
                        }
                        continue;
                    }
                    quote = !quote;
                }
                if !done {
                    uc = ue;
                }
            }
            b',' => nf += 1,
            b'\n' => {
                nl += 1;
                if nl == 10 {
                    return tf > 1 && tf == nf;
                }
                if tf == 0 {
                    if nf == 0 {
                        return false;
                    }
                    tf = nf;
                } else if tf != nf {
                    return false;
                }
                nf = 0;
            }
            _ => {}
        }
    }
    tf > 1 && nl >= 2
}

fn file_is_csv(ms: &mut MagicSet, buf: &[u8], looks_text: bool, code: Option<&str>) -> i32 {
    let mime = ms.flags & MAGIC_MIME;
    if !looks_text {
        return 0;
    }
    if ms.flags & (MAGIC_APPLE | MAGIC_EXTENSION) != 0 {
        return 0;
    }
    if !csv_parse(buf) {
        return 0;
    }
    if mime == MAGIC_MIME_ENCODING {
        return 1;
    }
    let r = if mime != 0 {
        ms.printf(b"text/csv", &[])
    } else {
        ms.printf(
            b"CSV %s%stext",
            &[
                Arg::S(code.unwrap_or("").as_bytes()),
                Arg::S(if code.is_some() { b" " } else { b"" }),
            ],
        )
    };
    if r.is_err() {
        return -1;
    }
    1
}

// ---------------------------------------------------------------------------
// is_simh.c

fn simh_parse(b: &[u8]) -> bool {
    let getlen = |uc: &mut usize| -> u32 {
        let n = u32::from_le_bytes(b[*uc..*uc + 4].try_into().unwrap());
        *uc += 4;
        if n == 0xffff_ffff {
            return n;
        }
        let mut n = n & 0x00ff_ffff;
        if n & 1 != 0 {
            n += 1;
        }
        n
    };
    let mut uc = 0usize;
    let ue = b.len();
    let (mut nt, mut nr) = (0usize, 0usize);
    while ue - uc >= 4 {
        let nbytes = getlen(&mut uc);
        if (nt > 0 || nr > 0) && nbytes == 0xFFFF_FFFF {
            break;
        }
        if nbytes == 0 {
            nt += 1;
            if nt == 10 {
                break;
            }
            continue;
        }
        uc = uc.saturating_add(nbytes as usize);
        if uc > ue || ue - uc < 4 {
            break;
        }
        let cbytes = getlen(&mut uc);
        if nbytes != cbytes {
            return false;
        }
        nr += 1;
    }
    if nt * 4 == uc {
        return false;
    }
    nr != 0
}

fn file_is_simh(ms: &mut MagicSet, buf: &[u8]) -> i32 {
    let mime = ms.flags & MAGIC_MIME;
    if ms.flags & (MAGIC_APPLE | MAGIC_EXTENSION) != 0 {
        return 0;
    }
    if !simh_parse(buf) {
        return 0;
    }
    if mime == MAGIC_MIME_ENCODING {
        return 1;
    }
    let r = if mime != 0 {
        ms.printf(b"application/SIMH-tape-data", &[])
    } else {
        ms.printf(b"SIMH tape data", &[])
    };
    if r.is_err() {
        return -1;
    }
    1
}

// ---------------------------------------------------------------------------
// ascmagic.c

fn encode_utf8(ubuf: &[u32]) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(ubuf.len());
    for &c in ubuf {
        if c <= 0x7f {
            out.push(c as u8);
        } else if c <= 0x7ff {
            out.push(((c >> 6) + 0xc0) as u8);
            out.push(((c & 0x3f) + 0x80) as u8);
        } else if c <= 0xffff {
            out.push(((c >> 12) + 0xe0) as u8);
            out.push((((c >> 6) & 0x3f) + 0x80) as u8);
            out.push(((c & 0x3f) + 0x80) as u8);
        } else if c <= 0x1fffff {
            out.push(((c >> 18) + 0xf0) as u8);
            out.push((((c >> 12) & 0x3f) + 0x80) as u8);
            out.push((((c >> 6) & 0x3f) + 0x80) as u8);
            out.push(((c & 0x3f) + 0x80) as u8);
        } else if c <= 0x3ffffff {
            out.push(((c >> 24) + 0xf8) as u8);
            for sh in [18, 12, 6, 0] {
                out.push((((c >> sh) & 0x3f) + 0x80) as u8);
            }
        } else if c <= 0x7fffffff {
            out.push(((c >> 30) + 0xfc) as u8);
            for sh in [24, 18, 12, 6, 0] {
                out.push((((c >> sh) & 0x3f) + 0x80) as u8);
            }
        } else {
            return None;
        }
    }
    Some(out)
}

const MAXLINELEN: usize = 300;

/// file_ascmagic()
fn file_ascmagic(ms: &mut MagicSet, host: &mut dyn Host, b: &Buffer, text: bool) -> i32 {
    let all = b.fbuf.bytes();
    let mut flen = trim_nuls(all);
    if flen & 1 != 0 && all.len() & 1 == 0 {
        flen += 1;
    }
    let buf = &all[..flen];
    let enc = file_encoding(buf, ms.encoding_max);
    if !enc.text {
        return 0;
    }
    ascmagic_with_encoding(ms, host, b, buf, &enc.ubuf, enc.code, enc.typ, text)
}

#[allow(clippy::too_many_arguments)]
fn ascmagic_with_encoding(
    ms: &mut MagicSet,
    host: &mut dyn Host,
    b: &Buffer,
    buf: &[u8],
    ubuf: &[u32],
    code: &str,
    typ: &str,
    text: bool,
) -> i32 {
    let mime = ms.flags & MAGIC_MIME;
    let mut need_separator = false;
    let nbytes = trim_nuls(buf);
    if nbytes <= 1 {
        return 0;
    }
    let ulen = ubuf.len();
    if ulen > 0 && ms.flags & MAGIC_NO_CHECK_SOFT == 0 {
        let Some(utf8) = encode_utf8(ubuf) else {
            return 0;
        };
        let bb = Buffer::new(b.fd.clone(), b.st_mode, b.st_size, BufRef::new(utf8));
        let mut rv = file_softmagic(ms, host, &bb, TEXTTEST, text);
        if rv == 0 {
            rv = -1;
        } else {
            need_separator = true;
        }
        if ms.flags & (MAGIC_APPLE | MAGIC_EXTENSION) != 0 {
            return if rv == -1 { 0 } else { 1 };
        }
    }
    if ms.flags & (MAGIC_APPLE | MAGIC_EXTENSION) != 0 {
        return 0;
    }
    let (mut n_crlf, mut n_lf, mut n_cr, mut n_nel) = (0usize, 0usize, 0usize, 0usize);
    let mut seen_cr = false;
    let mut last_line_end: usize = usize::MAX;
    let mut has_long_lines = 0usize;
    let mut has_escapes = false;
    let mut has_backspace = false;
    for (i, &c) in ubuf.iter().enumerate() {
        if c == b'\n' as u32 {
            if seen_cr {
                n_crlf += 1;
            } else {
                n_lf += 1;
            }
            last_line_end = i;
        } else if seen_cr {
            n_cr += 1;
        }
        seen_cr = c == b'\r' as u32;
        if seen_cr {
            last_line_end = i;
        }
        if c == 0x85 {
            n_nel += 1;
            last_line_end = i;
        }
        if i > last_line_end.wrapping_add(MAXLINELEN) {
            let ll = i.wrapping_sub(last_line_end);
            if ll > has_long_lines {
                has_long_lines = ll;
            }
        }
        if c == 0x1b {
            has_escapes = true;
        }
        if c == 0x08 {
            has_backspace = true;
        }
    }
    if typ == "binary" {
        return 0;
    }
    let len = ms.printedlen();
    let mut executable = false;
    macro_rules! p {
        ($($e:expr),*) => {
            if ms.printf($($e),*).is_err() {
                return -1;
            }
        };
    }
    if mime != 0 {
        if mime & MAGIC_MIME_TYPE != 0 {
            if len != 0 {
                if ms.flags & MAGIC_CONTINUE == 0 {
                    return 1;
                }
                if need_separator && ms.separator().is_err() {
                    return -1;
                }
            }
            p!(b"text/plain", &[]);
        }
    } else {
        if len != 0 {
            if ms.replace_eol(b" text", b", ") == 0 {
                if ms.replace_eol(b" text executable", b", ") == 0 {
                    p!(b", ", &[]);
                } else {
                    executable = true;
                }
            }
        }
        p!(b"%s", &[Arg::S(code.as_bytes())]);
        p!(b" %s", &[Arg::S(typ.as_bytes())]);
        if executable {
            p!(b" executable", &[]);
        }
        if has_long_lines != 0 {
            p!(
                b", with very long lines (%zu)",
                &[Arg::I(has_long_lines as i64)]
            );
        }
        if (n_crlf == 0 && n_cr == 0 && n_nel == 0 && n_lf == 0)
            || (n_crlf != 0 || n_cr != 0 || n_nel != 0)
        {
            p!(b", with", &[]);
            if n_crlf == 0 && n_cr == 0 && n_nel == 0 && n_lf == 0 {
                p!(b" no", &[]);
            } else {
                if n_crlf != 0 {
                    p!(b" CRLF", &[]);
                    if n_cr != 0 || n_lf != 0 || n_nel != 0 {
                        p!(b",", &[]);
                    }
                }
                if n_cr != 0 {
                    p!(b" CR", &[]);
                    if n_lf != 0 || n_nel != 0 {
                        p!(b",", &[]);
                    }
                }
                if n_lf != 0 {
                    p!(b" LF", &[]);
                    if n_nel != 0 {
                        p!(b",", &[]);
                    }
                }
                if n_nel != 0 {
                    p!(b" NEL", &[]);
                }
            }
            p!(b" line terminators", &[]);
        }
        if has_escapes {
            p!(b", with escape sequences", &[]);
        }
        if has_backspace {
            p!(b", with overstriking", &[]);
        }
    }
    1
}

// ---------------------------------------------------------------------------
// funcs.c: file_buffer()

fn file_default(ms: &mut MagicSet, nb: usize) -> i32 {
    if ms.flags & MAGIC_MIME != 0 {
        if ms.flags & MAGIC_MIME_TYPE != 0
            && ms
                .printf(
                    b"application/%s",
                    &[Arg::S(if nb != 0 { b"octet-stream" } else { b"x-empty" })],
                )
                .is_err()
        {
            return -1;
        }
        return 1;
    }
    if ms.flags & MAGIC_APPLE != 0 {
        if ms.printf(b"UNKNUNKN", &[]).is_err() {
            return -1;
        }
        return 1;
    }
    if ms.flags & MAGIC_EXTENSION != 0 {
        if ms.printf(b"???", &[]).is_err() {
            return -1;
        }
        return 1;
    }
    0
}

fn trim_separator(ms: &mut MagicSet) {
    if let Some(b) = ms.o_buf.as_mut() {
        let l = cstr(b).len();
        if l < FILE_SEPARATOR.len() + 1 {
            return;
        }
        if b[..l].ends_with(FILE_SEPARATOR) {
            b.truncate(l - FILE_SEPARATOR.len());
        }
    }
}

/// checkdone(): true when done
fn checkdone(ms: &mut MagicSet, rv: &mut i32) -> bool {
    if ms.flags & MAGIC_CONTINUE == 0 {
        return true;
    }
    if ms.separator().is_err() {
        *rv = -1;
    }
    false
}

/// file_buffer()
pub fn file_buffer(ms: &mut MagicSet, host: &mut dyn Host, b: &Buffer) -> i32 {
    let mut m = 0;
    let mut rv = 0;
    let mut looks_text = false;
    let mut code: &str = "";
    let mut have_code = false;
    let mut code_mime: &str = "binary";
    let mut def: &[u8] = b"data";
    let nb = b.flen();
    ms.mode = b.st_mode;
    let buf = b.fbuf.bytes().to_vec();
    'simple: {
        if nb == 0 {
            def = b"empty";
            break 'simple;
        } else if nb == 1 {
            def = b"very short file (no magic)";
            break 'simple;
        }
        if ms.flags & MAGIC_NO_CHECK_ENCODING == 0 {
            let e = file_encoding(&buf, ms.encoding_max);
            looks_text = e.text;
            code = e.code;
            have_code = true;
            code_mime = e.code_mime;
        }
        'done: {
            if ms.flags & MAGIC_NO_CHECK_TAR == 0 {
                m = file_is_tar(ms, &buf);
                if m != 0 && checkdone(ms, &mut rv) {
                    break 'done;
                }
            }
            if ms.flags & MAGIC_NO_CHECK_JSON == 0 {
                m = file_is_json(ms, &buf);
                if m != 0 && checkdone(ms, &mut rv) {
                    break 'done;
                }
            }
            if ms.flags & MAGIC_NO_CHECK_CSV == 0 {
                let c = if have_code { Some(code) } else { None };
                m = file_is_csv(ms, &buf, looks_text, c);
                if m != 0 && checkdone(ms, &mut rv) {
                    break 'done;
                }
            }
            if ms.flags & MAGIC_NO_CHECK_SIMH == 0 {
                m = file_is_simh(ms, &buf);
                if m != 0 && checkdone(ms, &mut rv) {
                    break 'done;
                }
            }
            if ms.flags & MAGIC_NO_CHECK_CDF == 0 {
                m = crate::cdf::file_trycdf(ms, b);
                if m != 0 && checkdone(ms, &mut rv) {
                    break 'done;
                }
            }
            if ms.flags & MAGIC_NO_CHECK_SOFT == 0 {
                m = file_softmagic(ms, host, b, BINTEST, looks_text);
                if m != 0 && checkdone(ms, &mut rv) {
                    break 'done;
                }
            }
            if ms.flags & MAGIC_NO_CHECK_TEXT == 0 {
                m = file_ascmagic(ms, host, b, looks_text);
                if m != 0 {
                    break 'done;
                }
            }
            if m == 0 {
                m = 1;
                rv = file_default(ms, nb);
                if rv == 0 && ms.printf(b"%s", &[Arg::S(def)]).is_err() {
                    rv = -1;
                }
            }
        }
        return finish(ms, m, rv, code_mime);
    }
    if m == 0 {
        m = 1;
        rv = file_default(ms, nb);
        if rv == 0 && ms.printf(b"%s", &[Arg::S(def)]).is_err() {
            rv = -1;
        }
    }
    finish(ms, m, rv, code_mime)
}

fn finish(ms: &mut MagicSet, m: i32, mut rv: i32, code_mime: &str) -> i32 {
    trim_separator(ms);
    if ms.flags & MAGIC_MIME_ENCODING != 0 {
        if ms.flags & MAGIC_MIME_TYPE != 0 && ms.printf(b"; charset=", &[]).is_err() {
            rv = -1;
        }
        if ms.printf(b"%s", &[Arg::S(code_mime.as_bytes())]).is_err() {
            rv = -1;
        }
    }
    if rv != 0 {
        return rv;
    }
    m
}

/// magic_buffer() / the end of file_or_stream(): the description
pub fn magic_buffer(ms: &mut MagicSet, host: &mut dyn Host, b: &Buffer) -> Option<Vec<u8>> {
    if ms.reset(true).is_err() {
        return None;
    }
    if file_buffer(ms, host, b) == -1 {
        return None;
    }
    ms.getbuffer()
}
