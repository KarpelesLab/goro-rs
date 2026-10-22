//! cdf.c and readcdf.c: Composite Document Files (OLE2).

use crate::fmt::Arg;
use crate::funcs::{cdf_ctime, cdf_timestamp_to_sec};
use crate::magic::*;

const LOOP_LIMIT: usize = 10000;
const ELEMENT_LIMIT: usize = 100000;
const SECID_FREE: i32 = -1;
const SECID_END_OF_CHAIN: i32 = -2;
const CDF_MAGIC: u64 = 0xE11AB1A1E011CFD0;
const DIRECTORY_SIZE: usize = 128;

const ESRCH: i32 = 3;
const EFTYPE: i32 = 22;

const DIR_TYPE_USER_STORAGE: u8 = 1;
const DIR_TYPE_USER_STREAM: u8 = 2;
const DIR_TYPE_ROOT_STORAGE: u8 = 5;

const CDF_EMPTY: u32 = 0x00;
const CDF_NULL: u32 = 0x01;
const CDF_SIGNED16: u32 = 0x02;
const CDF_SIGNED32: u32 = 0x03;
const CDF_FLOAT: u32 = 0x04;
const CDF_DOUBLE: u32 = 0x05;
const CDF_BOOL: u32 = 0x0b;
const CDF_UNSIGNED32: u32 = 0x13;
const CDF_SIGNED64: u32 = 0x14;
const CDF_UNSIGNED64: u32 = 0x15;
const CDF_LENGTH32_STRING: u32 = 0x1e;
const CDF_LENGTH32_WSTRING: u32 = 0x1f;
const CDF_FILETIME: u32 = 0x40;
const CDF_CLIPBOARD: u32 = 0x47;
const CDF_VECTOR: u32 = 0x1000;
const CDF_ARRAY: u32 = 0x2000;
const CDF_BYREF: u32 = 0x4000;
const CDF_RESERVED: u32 = 0x8000;
const CDF_TYPEMASK: u32 = 0xfff;
const CDF_PROPERTY_NAME_OF_APPLICATION: u32 = 0x12;

fn u16le(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b[o], b[o + 1]])
}
fn u32le(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(b[o..o + 4].try_into().unwrap())
}
fn u64le(b: &[u8], o: usize) -> u64 {
    u64::from_le_bytes(b[o..o + 8].try_into().unwrap())
}

struct Info<'a> {
    buf: &'a [u8],
    fd: Option<Fd>,
}

struct Header {
    magic: u64,
    sec_size_p2: u16,
    short_sec_size_p2: u16,
    secid_first_directory: i32,
    min_size_standard_stream: u32,
    secid_first_sector_in_short_sat: i32,
    secid_first_sector_in_master_sat: i32,
    num_sectors_in_master_sat: u32,
    master_sat: [i32; 109],
}

impl Header {
    fn ss(&self) -> usize {
        1usize << self.sec_size_p2
    }
    fn short_ss(&self) -> usize {
        1usize << self.short_sec_size_p2
    }
}

#[derive(Clone)]
struct Dirent {
    name: [u16; 32],
    typ: u8,
    storage_uuid: [u64; 2],
    stream_first_sector: i32,
    size: u32,
}

/// A sector allocation table
struct Sat {
    tab: Vec<u8>,
    len: usize,
}

impl Sat {
    fn get(&self, i: i32) -> i32 {
        let o = i as usize * 4;
        if i < 0 || o + 4 > self.tab.len() {
            return SECID_END_OF_CHAIN;
        }
        u32le(&self.tab, o) as i32
    }
}

#[derive(Default)]
struct Stream {
    tab: Option<Vec<u8>>,
    len: usize,
    ss: usize,
}

struct Cdf<'a> {
    info: Info<'a>,
    errno: i32,
}

#[derive(Clone)]
struct Prop {
    id: u32,
    typ: u32,
    val: u64,
    s: Option<(usize, u32)>,
}

impl Cdf<'_> {
    fn read(&mut self, off: usize, len: usize) -> Option<Vec<u8>> {
        let siz = off.checked_add(len);
        let Some(siz) = siz else {
            self.errno = 22;
            return None;
        };
        if self.info.buf.len() >= siz {
            return Some(self.info.buf[off..siz].to_vec());
        }
        match &self.info.fd {
            Some(fd) => fd_read(fd, off as u64, len),
            None => {
                self.errno = 22;
                None
            }
        }
    }

    fn read_header(&mut self) -> Option<Header> {
        let b = self.read(0, 512)?;
        let mut h = Header {
            magic: u64le(&b, 0),
            sec_size_p2: u16le(&b, 30),
            short_sec_size_p2: u16le(&b, 32),
            secid_first_directory: u32le(&b, 48) as i32,
            min_size_standard_stream: u32le(&b, 56),
            secid_first_sector_in_short_sat: u32le(&b, 60) as i32,
            secid_first_sector_in_master_sat: u32le(&b, 68) as i32,
            num_sectors_in_master_sat: u32le(&b, 72),
            master_sat: [0; 109],
        };
        for i in 0..109 {
            h.master_sat[i] = u32le(&b, 76 + i * 4) as i32;
        }
        if h.magic != CDF_MAGIC || h.sec_size_p2 > 20 || h.short_sec_size_p2 > 20 {
            self.errno = EFTYPE;
            return None;
        }
        Some(h)
    }

    fn read_sector(&mut self, h: &Header, id: i32) -> Option<Vec<u8>> {
        let ss = h.ss();
        if id < 0 {
            return None;
        }
        let pos = ss + id as usize * ss;
        self.read(pos, ss)
    }

    fn read_sat(&mut self, h: &Header) -> Option<Sat> {
        let ss = h.ss();
        let nsatpersec = ss / 4 - 1;
        let mut i = 0usize;
        while i < 109 && h.master_sat[i] != SECID_FREE {
            i += 1;
        }
        let limit = u32::MAX as usize / (64 * ss);
        if (nsatpersec > 0 && h.num_sectors_in_master_sat as usize > limit / nsatpersec)
            || i > limit
        {
            self.errno = EFTYPE;
            return None;
        }
        let sat_len = h.num_sectors_in_master_sat as usize * nsatpersec + i;
        let mut tab = vec![0u8; sat_len * ss];
        i = 0;
        while i < 109 {
            if h.master_sat[i] < 0 {
                break;
            }
            let s = self.read_sector(h, h.master_sat[i])?;
            if ss * i + ss > tab.len() {
                return None;
            }
            tab[ss * i..ss * i + ss].copy_from_slice(&s);
            i += 1;
        }
        let mut mid = h.secid_first_sector_in_master_sat;
        'out: for j in 0..h.num_sectors_in_master_sat as usize {
            if mid < 0 {
                break 'out;
            }
            if j >= LOOP_LIMIT {
                self.errno = EFTYPE;
                return None;
            }
            let msa = self.read_sector(h, mid)?;
            for k in 0..nsatpersec {
                let sec = u32le(&msa, k * 4) as i32;
                if sec < 0 {
                    break 'out;
                }
                if i >= sat_len {
                    self.errno = EFTYPE;
                    return None;
                }
                let s = self.read_sector(h, sec)?;
                tab[ss * i..ss * i + ss].copy_from_slice(&s);
                i += 1;
            }
            mid = u32le(&msa, nsatpersec * 4) as i32;
        }
        Some(Sat { tab, len: i })
    }

    fn count_chain(&mut self, sat: &Sat, sid: i32, size: usize) -> Option<usize> {
        let maxsector = ((sat.len * size) / 4) as i64;
        if sid == SECID_END_OF_CHAIN {
            return Some(0);
        }
        let mut sid = sid;
        let mut i = 0usize;
        while sid >= 0 {
            if i >= LOOP_LIMIT || sid as i64 >= maxsector {
                self.errno = EFTYPE;
                return None;
            }
            sid = sat.get(sid);
            i += 1;
        }
        if i == 0 {
            self.errno = EFTYPE;
            return None;
        }
        Some(i)
    }

    fn read_long_sector_chain(
        &mut self,
        h: &Header,
        sat: &Sat,
        sid: i32,
        len: usize,
    ) -> Result<Stream, ()> {
        let ss = h.ss();
        let n = self.count_chain(sat, sid, ss);
        if sid == SECID_END_OF_CHAIN || len == 0 {
            return Err(());
        }
        let Some(n) = n else {
            self.errno = EFTYPE;
            return Err(());
        };
        let mut st = Stream {
            tab: Some(vec![0u8; n * ss]),
            len: n,
            ss,
        };
        let mut sid = sid;
        let mut i = 0usize;
        while sid >= 0 {
            if i >= LOOP_LIMIT || i >= st.len {
                self.errno = EFTYPE;
                return Err(());
            }
            let Some(s) = self.read_sector(h, sid) else {
                self.errno = EFTYPE;
                return Err(());
            };
            st.tab.as_mut().unwrap()[i * ss..i * ss + ss].copy_from_slice(&s);
            sid = sat.get(sid);
            i += 1;
        }
        Ok(st)
    }

    fn read_short_sector_chain(
        &mut self,
        h: &Header,
        ssat: &Sat,
        sst: &Stream,
        sid: i32,
        _len: usize,
    ) -> Result<Stream, ()> {
        let ss = h.short_ss();
        let Some(n) = self.count_chain(ssat, sid, h.ss()) else {
            self.errno = EFTYPE;
            return Err(());
        };
        let mut st = Stream {
            tab: Some(vec![0u8; n * ss]),
            len: n,
            ss,
        };
        let mut sid = sid;
        let mut i = 0usize;
        let src = sst.tab.as_deref().unwrap_or(&[]);
        while sid >= 0 {
            if i >= LOOP_LIMIT || i >= st.len {
                self.errno = EFTYPE;
                return Err(());
            }
            let pos = sid as usize * ss;
            if pos + ss > h.ss() * sst.len || pos + ss > src.len() {
                self.errno = EFTYPE;
                return Err(());
            }
            st.tab.as_mut().unwrap()[i * ss..i * ss + ss].copy_from_slice(&src[pos..pos + ss]);
            sid = ssat.get(sid);
            i += 1;
        }
        Ok(st)
    }

    #[allow(clippy::too_many_arguments)]
    fn read_sector_chain(
        &mut self,
        h: &Header,
        sat: &Sat,
        ssat: &Sat,
        sst: &Stream,
        sid: i32,
        len: usize,
    ) -> Result<Stream, ()> {
        if len < h.min_size_standard_stream as usize && sst.tab.is_some() {
            self.read_short_sector_chain(h, ssat, sst, sid, len)
        } else {
            self.read_long_sector_chain(h, sat, sid, len)
        }
    }

    fn read_dir(&mut self, h: &Header, sat: &Sat) -> Option<Vec<Dirent>> {
        let ss = h.ss();
        let mut sid = h.secid_first_directory;
        let ns = self.count_chain(sat, sid, ss)?;
        let nd = ss / DIRECTORY_SIZE;
        let mut dir = Vec::with_capacity(ns * nd);
        for i in 0..ns {
            if i >= LOOP_LIMIT {
                self.errno = EFTYPE;
                return None;
            }
            let Some(buf) = self.read_sector(h, sid) else {
                self.errno = EFTYPE;
                return None;
            };
            for j in 0..nd {
                let b = &buf[j * DIRECTORY_SIZE..(j + 1) * DIRECTORY_SIZE];
                let mut name = [0u16; 32];
                for (k, n) in name.iter_mut().enumerate() {
                    *n = u16le(b, k * 2);
                }
                dir.push(Dirent {
                    name,
                    typ: b[66],
                    storage_uuid: [u64le(b, 80), u64le(b, 88)],
                    stream_first_sector: u32le(b, 116) as i32,
                    size: u32le(b, 120),
                });
            }
            sid = sat.get(sid);
        }
        Some(dir)
    }

    fn read_ssat(&mut self, h: &Header, sat: &Sat) -> Option<Sat> {
        let ss = h.ss();
        let mut sid = h.secid_first_sector_in_short_sat;
        let n = self.count_chain(sat, sid, ss)?;
        let mut tab = vec![0u8; n * ss];
        let mut i = 0usize;
        while sid >= 0 {
            if i >= LOOP_LIMIT || i >= n {
                self.errno = EFTYPE;
                return None;
            }
            let s = self.read_sector(h, sid)?;
            tab[i * ss..i * ss + ss].copy_from_slice(&s);
            sid = sat.get(sid);
            i += 1;
        }
        Some(Sat { tab, len: n })
    }

    /// cdf_read_short_stream(): the stream and the root storage index
    fn read_short_stream(
        &mut self,
        h: &Header,
        sat: &Sat,
        dir: &[Dirent],
    ) -> Result<(Stream, Option<usize>), ()> {
        let Some(i) = dir.iter().position(|d| d.typ == DIR_TYPE_ROOT_STORAGE) else {
            return Ok((Stream::default(), None));
        };
        let d = &dir[i];
        if d.stream_first_sector < 0 {
            return Ok((Stream::default(), Some(i)));
        }
        let st = self.read_long_sector_chain(h, sat, d.stream_first_sector, d.size as usize)?;
        Ok((st, Some(i)))
    }

    fn find_stream(&mut self, dir: &[Dirent], name: &[u8], typ: u8) -> usize {
        for i in (1..=dir.len()).rev() {
            let d = &dir[i - 1];
            if d.typ == typ && namecmp(name, &d.name) {
                return i;
            }
        }
        self.errno = ESRCH;
        0
    }

    #[allow(clippy::too_many_arguments)]
    fn read_user_stream(
        &mut self,
        h: &Header,
        sat: &Sat,
        ssat: &Sat,
        sst: &Stream,
        dir: &[Dirent],
        name: &[u8],
    ) -> Result<Stream, ()> {
        let i = self.find_stream(dir, name, DIR_TYPE_USER_STREAM);
        if i == 0 {
            return Err(());
        }
        let d = &dir[i - 1];
        self.read_sector_chain(h, sat, ssat, sst, d.stream_first_sector, d.size as usize)
    }

    /// cdf_read_property_info()
    fn read_property_info(&mut self, sst: &Stream, offs: u32) -> Result<Vec<Prop>, ()> {
        let tab = sst.tab.as_deref().unwrap_or(&[]);
        let total = sst.ss * sst.len;
        let fail = |c: &mut Self| {
            c.errno = EFTYPE;
            Err(())
        };
        let get32 = |o: usize| -> u32 { if o + 4 <= tab.len() { u32le(tab, o) } else { 0 } };
        if offs > u32::MAX / 4 {
            return fail(self);
        }
        let offs = offs as usize;
        if offs + 8 > total {
            return fail(self);
        }
        let sh_len = get32(offs) as usize;
        if sh_len > (u32::MAX / 64) as usize {
            return fail(self);
        }
        if offs + sh_len > total {
            return fail(self);
        }
        let sh_properties = get32(offs + 4) as usize;
        if sh_properties > (u32::MAX as usize) / (64 * 40) {
            return fail(self);
        }
        let mut info: Vec<Prop> = vec![
            Prop {
                id: 0,
                typ: 0,
                val: 0,
                s: None,
            };
            sh_properties
        ];
        let p = offs + 8;
        let e = offs + sh_len;
        if p >= e || e > total {
            return fail(self);
        }
        let mut i = 0usize;
        while i < sh_properties {
            // cdf_get_property_info_pos()
            let tail = (i << 1) + 1;
            if p >= e || p + (tail + 1) * 4 > total {
                return fail(self);
            }
            let mut ofs = get32(p + tail * 4) as usize;
            if ofs < 8 {
                return fail(self);
            }
            ofs -= 8;
            if ofs > e - p {
                return fail(self);
            }
            let q = p + ofs;
            info[i].id = get32(p + (i << 1) * 4);
            let left = e - q;
            if left < 4 {
                return fail(self);
            }
            info[i].typ = get32(q);
            let typ = info[i].typ;
            let (nelements, mut slen) = if typ & CDF_VECTOR != 0 {
                if left < 8 {
                    return fail(self);
                }
                let n = get32(q + 4) as usize;
                if n > ELEMENT_LIMIT || n == 0 {
                    return fail(self);
                }
                (n, 2usize)
            } else {
                (1usize, 1usize)
            };
            let mut o4 = slen * 4;
            let copy = |inp: &mut Prop, len: usize| -> bool {
                if inp.typ & CDF_VECTOR != 0 {
                    return false;
                }
                if e - (q + o4) < len || q + o4 > e {
                    return false;
                }
                let at = q + o4;
                inp.val = match len {
                    2 => u16le(tab, at) as u64,
                    4 => u32le(tab, at) as u64,
                    _ => u64le(tab, at),
                };
                true
            };
            let unknown = typ & (CDF_ARRAY | CDF_BYREF | CDF_RESERVED) != 0;
            let mut is_unknown = unknown;
            if !unknown {
                match typ & CDF_TYPEMASK {
                    CDF_NULL | CDF_EMPTY => {}
                    CDF_SIGNED16 => is_unknown = !copy(&mut info[i], 2),
                    CDF_SIGNED32 | CDF_BOOL | CDF_UNSIGNED32 | CDF_FLOAT => {
                        is_unknown = !copy(&mut info[i], 4)
                    }
                    CDF_SIGNED64 | CDF_UNSIGNED64 | CDF_DOUBLE | CDF_FILETIME => {
                        is_unknown = !copy(&mut info[i], 8)
                    }
                    CDF_LENGTH32_STRING | CDF_LENGTH32_WSTRING => {
                        let mut j = 0usize;
                        while j < nelements && i < sh_properties {
                            if o4 + 4 > left {
                                return fail(self);
                            }
                            let mut l = get32(q + slen * 4) as usize;
                            o4 += 4;
                            if o4 + l > left {
                                return fail(self);
                            }
                            info[i].s = Some((q + o4, l as u32));
                            if j > 0 {
                                info[i].typ = typ;
                            }
                            if l & 1 != 0 {
                                l += 1;
                            }
                            slen += l >> 1;
                            o4 = slen * 4;
                            j += 1;
                            i += 1;
                        }
                        i -= 1;
                    }
                    CDF_CLIPBOARD => {
                        if typ & CDF_VECTOR != 0 {
                            is_unknown = true;
                        }
                    }
                    _ => is_unknown = true,
                }
            }
            if is_unknown {
                info[i].val = 0;
                info[i].s = None;
            }
            i += 1;
        }
        Ok(info)
    }
}

fn namecmp(d: &[u8], s: &[u16; 32]) -> bool {
    // strlen(name) + 1 characters, the NUL included
    if d.len() >= 32 {
        return false;
    }
    d.iter()
        .copied()
        .chain(std::iter::once(0))
        .zip(s.iter())
        .all(|(c, &w)| c as u16 == w)
}

static APP2MIME: &[(&str, &str)] = &[
    ("Word", "msword"),
    ("Excel", "vnd.ms-excel"),
    ("Powerpoint", "vnd.ms-powerpoint"),
    ("Crystal Reports", "x-rpt"),
    ("Advanced Installer", "vnd.ms-msi"),
    ("InstallShield", "vnd.ms-msi"),
    ("Microsoft Patch Compiler", "vnd.ms-msi"),
    ("NAnt", "vnd.ms-msi"),
    ("Windows Installer", "vnd.ms-msi"),
];
static NAME2MIME: &[(&str, &str)] = &[
    ("Book", "vnd.ms-excel"),
    ("Workbook", "vnd.ms-excel"),
    ("WordDocument", "msword"),
    ("PowerPoint", "vnd.ms-powerpoint"),
    ("DigitalSignature", "vnd.ms-msi"),
];
static NAME2DESC: &[(&str, &str)] = &[
    ("Book", "Microsoft Excel"),
    ("Workbook", "Microsoft Excel"),
    ("WordDocument", "Microsoft Word"),
    ("PowerPoint", "Microsoft PowerPoint"),
    ("DigitalSignature", "Microsoft Installer"),
];

const MSI_CLSID: [u64; 2] = [0x00000000000c1084, 0x46000000000000c0];

fn app_to_mime(vbuf: &[u8], nv: &[(&'static str, &'static str)]) -> Option<&'static str> {
    let lower = vbuf.to_ascii_lowercase();
    for &(pat, mime) in nv {
        let p = pat.to_ascii_lowercase();
        if lower.windows(p.len()).any(|w| w == p.as_bytes()) || p.is_empty() {
            return Some(mime);
        }
    }
    None
}

fn notmime(ms: &MagicSet) -> bool {
    ms.flags & MAGIC_MIME == 0
}

fn property_name(p: u32) -> String {
    let n = match p {
        0x01 => "Code page",
        0x02 => "Title",
        0x03 => "Subject",
        0x04 => "Author",
        0x05 => "Keywords",
        0x06 => "Comments",
        0x07 => "Template",
        0x08 => "Last Saved By",
        0x09 => "Revision Number",
        0x0a => "Total Editing Time",
        0x0b => "Last Printed",
        0x0c => "Create Time/Date",
        0x0d => "Last Saved Time/Date",
        0x0e => "Number of Pages",
        0x0f => "Number of Words",
        0x10 => "Number of Characters",
        0x11 => "Thumbnail",
        0x12 => "Name of Creating Application",
        0x13 => "Security",
        0x80000000 => "Locale ID",
        _ => return format!("{:#x}", p),
    };
    n.to_string()
}

fn elapsed_time(ts: i64) -> String {
    let mut ts = ts / 10_000_000;
    let secs = ts % 60;
    ts /= 60;
    let mins = ts % 60;
    ts /= 60;
    let hours = ts % 24;
    ts /= 24;
    let days = ts;
    let mut s = String::new();
    if days != 0 {
        s.push_str(&format!("{}d+", days));
    }
    if days != 0 || hours != 0 {
        s.push_str(&format!("{:02}:", hours));
    }
    s.push_str(&format!("{:02}:", mins));
    s.push_str(&format!("{:02}", secs));
    s
}

/// cdf_file_property_info()
fn file_property_info(ms: &mut MagicSet, tab: &[u8], info: &[Prop], root: Option<&Dirent>) -> i32 {
    let mut str_: Option<&'static str> = None;
    if !notmime(ms)
        && let Some(r) = root
        && r.storage_uuid == MSI_CLSID
    {
        str_ = Some("x-msi");
    }
    macro_rules! pf {
        ($f:expr, $($a:expr),*) => {
            if notmime(ms) && ms.printf($f, &[$($a),*]).is_err() {
                return -1;
            }
        };
    }
    for p in info {
        let buf = property_name(p.id);
        let b = buf.as_bytes();
        match p.typ {
            CDF_NULL => {}
            CDF_SIGNED16 => pf!(b", %s: %hd", Arg::S(b), Arg::I(p.val as u16 as i16 as i64)),
            CDF_SIGNED32 => pf!(b", %s: %d", Arg::S(b), Arg::I(p.val as u32 as i32 as i64)),
            CDF_UNSIGNED32 => pf!(b", %s: %u", Arg::S(b), Arg::I(p.val as u32 as i64)),
            CDF_FLOAT => pf!(
                b", %s: %g",
                Arg::S(b),
                Arg::F(f32::from_bits(p.val as u32) as f64)
            ),
            CDF_DOUBLE => pf!(b", %s: %g", Arg::S(b), Arg::F(f64::from_bits(p.val))),
            CDF_LENGTH32_STRING | CDF_LENGTH32_WSTRING => {
                let Some((start, l)) = p.s else {
                    continue;
                };
                let mut len = l as i64;
                if len > 1 {
                    let k = if p.typ == CDF_LENGTH32_WSTRING { 2 } else { 1 };
                    let mut vbuf: Vec<u8> = Vec::new();
                    let mut s = start;
                    let e = start + l as usize;
                    while s < e && vbuf.len() < 1024 && len > 0 {
                        len -= 1;
                        let c = *tab.get(s).unwrap_or(&0);
                        if c == 0 {
                            break;
                        }
                        if c_isprint(c) {
                            vbuf.push(c);
                        }
                        s += k;
                    }
                    if vbuf.len() == 1024 {
                        vbuf.pop();
                    }
                    if notmime(ms) {
                        if !vbuf.is_empty() {
                            pf!(b", %s: %s", Arg::S(b), Arg::S(&vbuf));
                        }
                    } else if str_.is_none() && p.id == CDF_PROPERTY_NAME_OF_APPLICATION {
                        str_ = app_to_mime(&vbuf, APP2MIME);
                    }
                }
            }
            CDF_FILETIME => {
                let tp = p.val as i64;
                if tp != 0 {
                    if tp < 1000000000000000 {
                        let t = elapsed_time(tp);
                        pf!(b", %s: %s", Arg::S(b), Arg::S(t.as_bytes()));
                    } else {
                        let sec = cdf_timestamp_to_sec(tp).unwrap_or(-1);
                        let c = cdf_ctime(sec);
                        pf!(b", %s: %s", Arg::S(b), Arg::S(&c));
                    }
                }
            }
            CDF_CLIPBOARD => {}
            _ => return -1,
        }
    }
    if ms.flags & MAGIC_MIME_TYPE != 0 {
        let Some(s) = str_ else {
            return 0;
        };
        if ms
            .printf(b"application/%s", &[Arg::S(s.as_bytes())])
            .is_err()
        {
            return -1;
        }
    }
    1
}

/// cdf_file_summary_info()
fn file_summary_info(cdf: &mut Cdf, ms: &mut MagicSet, sst: &Stream, root: Option<&Dirent>) -> i32 {
    let tab = sst.tab.as_deref().unwrap_or(&[]);
    let total = sst.ss * sst.len;
    if 28 > total || 0x1c + 20 > total {
        return -1;
    }
    let byte_order = u16le(tab, 0);
    let os_version = u16le(tab, 4);
    let os = u16le(tab, 6);
    let sd_offset = u32le(tab, 0x1c + 16);
    let Ok(info) = cdf.read_property_info(sst, sd_offset) else {
        return -1;
    };
    if notmime(ms) {
        if ms
            .printf(b"Composite Document File V2 Document", &[])
            .is_err()
        {
            return -1;
        }
        let bo: &[u8] = if byte_order == 0xfffe {
            b"Little"
        } else {
            b"Big"
        };
        if ms.printf(b", %s Endian", &[Arg::S(bo)]).is_err() {
            return -2;
        }
        let r = match os {
            2 => ms.printf(
                b", Os: Windows, Version %d.%d",
                &[
                    Arg::I((os_version & 0xff) as i64),
                    Arg::I((os_version >> 8) as i64),
                ],
            ),
            1 => ms.printf(
                b", Os: MacOS, Version %d.%d",
                &[
                    Arg::I((os_version >> 8) as i64),
                    Arg::I((os_version & 0xff) as i64),
                ],
            ),
            _ => ms.printf(
                b", Os %d, Version: %d.%d",
                &[
                    Arg::I(os as i64),
                    Arg::I((os_version & 0xff) as i64),
                    Arg::I((os_version >> 8) as i64),
                ],
            ),
        };
        if r.is_err() {
            return -2;
        }
        if let Some(r) = root
            && r.storage_uuid == MSI_CLSID
            && ms.printf(b", %s", &[Arg::S(b"MSI Installer")]).is_err()
        {
            return -2;
        }
    }
    let m = file_property_info(ms, tab, &info, root);
    if m == -1 { -2 } else { m }
}

/// cdf_file_catalog()
fn file_catalog(ms: &mut MagicSet, sst: &Stream) -> i32 {
    if notmime(ms) {
        if ms.printf(b"Microsoft Thumbs.db [", &[]).is_err() {
            return -1;
        }
        let tab = sst.tab.as_deref().unwrap_or(&[]);
        let eb = sst.ss * sst.len;
        let rd16 = |o: usize| -> Option<u16> {
            if o + 2 <= tab.len() && o + 2 <= eb {
                Some(u16le(tab, o))
            } else {
                None
            }
        };
        // count the records
        let mut nr = 0usize;
        let mut b = 0usize;
        loop {
            let reclen = rd16(b).unwrap_or(0) as usize;
            if reclen == 0 {
                break;
            }
            b += reclen;
            nr += 1;
            if b > eb {
                break;
            }
        }
        if nr == 0 {
            return -1;
        }
        nr -= 1;
        let mut names: Vec<Vec<u8>> = vec![Vec::new(); nr];
        let mut b = 0usize;
        let mut i = 0usize;
        let mut j = 0usize;
        while i < nr {
            let Some(namlen) = rd16(b) else {
                break;
            };
            let reclen = namlen as usize;
            if b + 8 + 8 > eb {
                break;
            }
            if reclen < 14 {
                b += reclen;
                continue;
            }
            let n = (255usize).min(reclen - 14);
            let np = b + 16;
            if np + n * 2 > eb {
                break;
            }
            let mut name = Vec::new();
            for k in 0..n {
                let c = rd16(np + k * 2).unwrap_or(0);
                if c == 0 {
                    break;
                }
                name.push(c as u8);
            }
            names[j] = name;
            j = i;
            i += 1;
            b += reclen;
        }
        let cat_num = j;
        for (i, n) in names.iter().enumerate().take(cat_num).skip(1) {
            let sep: &[u8] = if i == cat_num - 1 { b"]" } else { b", " };
            if ms.printf(b"%s%s", &[Arg::S(n), Arg::S(sep)]).is_err() {
                return -1;
            }
        }
    } else if ms.flags & MAGIC_MIME_TYPE != 0 && ms.printf(b"application/CDFV2", &[]).is_err() {
        return -1;
    }
    1
}

#[allow(clippy::too_many_arguments)]
fn check_summary_info(
    cdf: &mut Cdf,
    ms: &mut MagicSet,
    h: &Header,
    sat: &Sat,
    ssat: &Sat,
    sst: &Stream,
    dir: &[Dirent],
    scn: &Stream,
    root: Option<&Dirent>,
    expn: &mut &'static str,
) -> i32 {
    let mut i = file_summary_info(cdf, ms, scn, root);
    if i < 0 {
        *expn = "Can't expand summary_info";
        return i;
    }
    if i == 1 {
        return i;
    }
    let mut s: Option<&'static str> = None;
    for d in dir {
        if s.is_some() {
            break;
        }
        let name: Vec<u8> = d.name.iter().map(|&c| c as u8).collect();
        let name = cstr(&name);
        s = app_to_mime(name, if notmime(ms) { NAME2DESC } else { NAME2MIME });
    }
    if notmime(ms) {
        if let Some(st) = s {
            if ms.printf(b"%s", &[Arg::S(st.as_bytes())]).is_err() {
                return -1;
            }
            i = 1;
        }
    } else if ms.flags & MAGIC_MIME_TYPE != 0 {
        let st = s.unwrap_or("vnd.ms-office");
        if ms
            .printf(b"application/%s", &[Arg::S(st.as_bytes())])
            .is_err()
        {
            return -1;
        }
        i = 1;
    }
    if i <= 0 {
        match cdf.read_user_stream(h, sat, ssat, sst, dir, b"Catalog") {
            Err(()) => return -1,
            Ok(c) => {
                i = file_catalog(ms, &c);
            }
        }
    }
    i
}

static SECTIONINFO: &[(&str, &str, &[(&[u8], u8)])] = &[
    (
        "Encrypted",
        "encrypted",
        &[
            (b"EncryptedPackage", DIR_TYPE_USER_STREAM),
            (b"EncryptedSummary", DIR_TYPE_USER_STREAM),
        ],
    ),
    (
        "QuickBooks",
        "quickbooks",
        &[(b"mfbu_header", DIR_TYPE_USER_STREAM)],
    ),
    (
        "Microsoft Excel",
        "vnd.ms-excel",
        &[
            (b"Book", DIR_TYPE_USER_STREAM),
            (b"Workbook", DIR_TYPE_USER_STREAM),
        ],
    ),
    (
        "Microsoft Word",
        "msword",
        &[(b"WordDocument", DIR_TYPE_USER_STREAM)],
    ),
    (
        "Microsoft PowerPoint",
        "vnd.ms-powerpoint",
        &[(b"PowerPoint", DIR_TYPE_USER_STREAM)],
    ),
    (
        "Microsoft Outlook Message",
        "vnd.ms-outlook",
        &[
            (b"__properties_version1.0", DIR_TYPE_USER_STREAM),
            (b"__recip_version1.0_#00000000", DIR_TYPE_USER_STORAGE),
        ],
    ),
];

fn file_dir_info(cdf: &mut Cdf, ms: &mut MagicSet, dir: &[Dirent]) -> i32 {
    for &(name, mime, sections) in SECTIONINFO {
        let found = sections
            .iter()
            .any(|&(s, t)| cdf.find_stream(dir, s, t) > 0);
        if !found {
            continue;
        }
        if notmime(ms) {
            if ms.printf(b"CDFV2 %s", &[Arg::S(name.as_bytes())]).is_err() {
                return -1;
            }
        } else if ms.flags & MAGIC_MIME_TYPE != 0
            && ms
                .printf(b"application/%s", &[Arg::S(mime.as_bytes())])
                .is_err()
        {
            return -1;
        }
        return 1;
    }
    -1
}

/// file_trycdf()
pub fn file_trycdf(ms: &mut MagicSet, b: &Buffer) -> i32 {
    if ms.flags & (MAGIC_APPLE | MAGIC_EXTENSION) != 0 {
        return 0;
    }
    let mut cdf = Cdf {
        info: Info {
            buf: b.fbuf.bytes(),
            fd: b.fd.clone(),
        },
        errno: 0,
    };
    let Some(h) = cdf.read_header() else {
        return 0;
    };
    let mut expn: &'static str = "";
    let i = trycdf_inner(&mut cdf, ms, &h, &mut expn);
    if i != -1 {
        return i;
    }
    if notmime(ms) {
        if ms
            .printf(b"Composite Document File V2 Document", &[])
            .is_err()
        {
            return -1;
        }
        if !expn.is_empty() && ms.printf(b", %s", &[Arg::S(expn.as_bytes())]).is_err() {
            return -1;
        }
    } else if ms.flags & MAGIC_MIME_TYPE != 0
        && ms.printf(b"application/x-ole-storage", &[]).is_err()
    {
        return -1;
    }
    1
}

fn trycdf_inner(cdf: &mut Cdf, ms: &mut MagicSet, h: &Header, expn: &mut &'static str) -> i32 {
    let Some(sat) = cdf.read_sat(h) else {
        *expn = "Can't read SAT";
        return -1;
    };
    let Some(ssat) = cdf.read_ssat(h, &sat) else {
        *expn = "Can't read SSAT";
        return -1;
    };
    let Some(dir) = cdf.read_dir(h, &sat) else {
        *expn = "Can't read directory";
        return -1;
    };
    let Ok((sst, root_i)) = cdf.read_short_stream(h, &sat, &dir) else {
        *expn = "Cannot read short stream";
        return -1;
    };
    let root = root_i.map(|i| dir[i].clone());
    if let Ok(scn) = cdf.read_user_stream(h, &sat, &ssat, &sst, &dir, b"FileHeader") {
        const SIG: &[u8] = b"HWP Document File";
        let tab = scn.tab.as_deref().unwrap_or(&[]);
        if scn.len * scn.ss >= SIG.len() && tab.starts_with(SIG) {
            if notmime(ms) {
                if ms
                    .printf(b"Hancom HWP (Hangul Word Processor) file, version 5.0", &[])
                    .is_err()
                {
                    return -1;
                }
            } else if ms.flags & MAGIC_MIME_TYPE != 0
                && ms.printf(b"application/x-hwp", &[]).is_err()
            {
                return -1;
            }
            return 1;
        }
    }
    let mut i;
    match cdf.read_user_stream(h, &sat, &ssat, &sst, &dir, b"\x05SummaryInformation") {
        Err(()) => {
            i = -1;
            if cdf.errno != ESRCH {
                *expn = "Cannot read summary info";
            }
        }
        Ok(scn) => {
            i = check_summary_info(
                cdf,
                ms,
                h,
                &sat,
                &ssat,
                &sst,
                &dir,
                &scn,
                root.as_ref(),
                expn,
            );
        }
    }
    if i <= 0 {
        match cdf.read_user_stream(
            h,
            &sat,
            &ssat,
            &sst,
            &dir,
            b"\x05DocumentSummaryInformation",
        ) {
            Err(()) => {
                i = -1;
                if cdf.errno != ESRCH {
                    *expn = "Cannot read summary info";
                }
            }
            Ok(scn) => {
                i = check_summary_info(
                    cdf,
                    ms,
                    h,
                    &sat,
                    &ssat,
                    &sst,
                    &dir,
                    &scn,
                    root.as_ref(),
                    expn,
                );
            }
        }
    }
    if i <= 0 {
        i = file_dir_info(cdf, ms, &dir);
        if i < 0 {
            *expn = "Cannot read section info";
        }
    }
    i
}
