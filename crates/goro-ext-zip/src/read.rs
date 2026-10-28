//! Reading archives (PKWARE APPNOTE.TXT section 4.3): the end of central
//! directory record (and its ZIP64 form), the central directory, local
//! headers, extra fields, and entry data through decryption and
//! decompression with the CRC check.
//!
//! Errors follow libzip's codes as PHP reports them: no end record is
//! `ER_NOZIP`, multi-disk archives `ER_MULTIDISK`, a central directory
//! that does not fit `ER_INCONS`; with `check_consistency` (PHP's
//! `ZipArchive::CHECKCONS`) local headers must also agree with the
//! central directory and entries must not overlap.

use crate::codec::{self, cm};
use crate::crypto::{self, AesStrength};
use crate::error::{Error, Result, er};

pub const SIG_LOCAL: u32 = 0x0403_4b50;
pub const SIG_CENTRAL: u32 = 0x0201_4b50;
pub const SIG_EOCD: u32 = 0x0605_4b50;
pub const SIG_EOCD64: u32 = 0x0606_4b50;
pub const SIG_EOCD64_LOCATOR: u32 = 0x0706_4b50;
pub const SIG_DATA_DESCRIPTOR: u32 = 0x0807_4b50;

/// Extra field ids used here.
pub mod extra_id {
    pub const ZIP64: u16 = 0x0001;
    pub const AES: u16 = 0x9901;
    pub const EXT_TIMESTAMP: u16 = 0x5455;
}

/// General purpose bit flags (APPNOTE 4.4.4).
pub mod gp {
    pub const ENCRYPTED: u16 = 1 << 0;
    pub const DATA_DESCRIPTOR: u16 = 1 << 3;
    pub const STRONG_ENCRYPTION: u16 = 1 << 6;
    pub const UTF8: u16 = 1 << 11;
}

/// Encryption methods as PHP numbers them (`ZipArchive::EM_*`).
pub mod em {
    pub const NONE: u16 = 0;
    pub const TRAD_PKWARE: u16 = 1;
    pub const AES_128: u16 = 0x0101;
    pub const AES_192: u16 = 0x0102;
    pub const AES_256: u16 = 0x0103;
    pub const UNKNOWN: u16 = 0xffff;
}

#[inline]
pub(crate) fn u16le(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([b[at], b[at + 1]])
}
#[inline]
pub(crate) fn u32le(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}
#[inline]
pub(crate) fn u64le(b: &[u8], at: usize) -> u64 {
    let mut a = [0u8; 8];
    a.copy_from_slice(&b[at..at + 8]);
    u64::from_le_bytes(a)
}

/// The `(id, data)` records of an extra field block; a truncated last
/// record is dropped.
pub fn extra_fields(extra: &[u8]) -> Vec<(u16, &[u8])> {
    let mut out = Vec::new();
    let mut p = 0;
    while p + 4 <= extra.len() {
        let id = u16le(extra, p);
        let n = u16le(extra, p + 2) as usize;
        if p + 4 + n > extra.len() {
            break;
        }
        out.push((id, &extra[p + 4..p + 4 + n]));
        p += 4 + n;
    }
    out
}

/// The data of the first extra record `id`.
pub fn find_extra(extra: &[u8], id: u16) -> Option<&[u8]> {
    extra_fields(extra).into_iter().find(|r| r.0 == id).map(|r| r.1)
}

/// `extra` without the records whose id is in `ids` (and without a
/// truncated tail).
pub fn strip_extra(extra: &[u8], ids: &[u16]) -> Vec<u8> {
    let mut out = Vec::with_capacity(extra.len());
    for (id, d) in extra_fields(extra) {
        if !ids.contains(&id) {
            out.extend_from_slice(&id.to_le_bytes());
            out.extend_from_slice(&(d.len() as u16).to_le_bytes());
            out.extend_from_slice(d);
        }
    }
    out
}

/// WinZip AES parameters (extra field 0x9901).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AesInfo {
    /// 1 (AE-1: CRC kept) or 2 (AE-2: CRC is 0).
    pub version: u16,
    pub strength: AesStrength,
    /// The compression method of the data.
    pub method: u16,
}

impl AesInfo {
    /// From the record's 7 data bytes.
    pub fn parse(d: &[u8]) -> Option<AesInfo> {
        if d.len() < 7 || &d[2..4] != b"AE" {
            return None;
        }
        Some(AesInfo {
            version: u16le(d, 0),
            strength: AesStrength::from_byte(d[4])?,
            method: u16le(d, 5),
        })
    }

    /// The record's data bytes.
    pub fn to_bytes(&self) -> [u8; 7] {
        let v = self.version.to_le_bytes();
        let m = self.method.to_le_bytes();
        [v[0], v[1], b'A', b'E', self.strength as u8, m[0], m[1]]
    }
}

/// A central directory entry with its local header's extra field and data
/// position.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Entry {
    pub version_made_by: u16,
    pub version_needed: u16,
    pub flags: u16,
    /// The stored method (99 for WinZip AES; see [`Entry::real_method`]).
    pub method: u16,
    pub dos_time: u16,
    pub dos_date: u16,
    pub crc: u32,
    pub comp_size: u64,
    pub size: u64,
    pub disk_start: u32,
    pub internal_attr: u16,
    pub external_attr: u32,
    /// Offset of the local header.
    pub local_offset: u64,
    pub name: Vec<u8>,
    /// Central directory extra field (ZIP64 record included).
    pub extra: Vec<u8>,
    pub comment: Vec<u8>,
    /// Local header extra field.
    pub local_extra: Vec<u8>,
    /// Offset of the (possibly encrypted) data; none when the local
    /// header is missing or does not fit.
    pub data_offset: Option<u64>,
    pub aes: Option<AesInfo>,
}

impl Entry {
    /// The compression method of the data (inside WinZip AES too).
    pub fn real_method(&self) -> u16 {
        match &self.aes {
            Some(a) if self.method == cm::AES => a.method,
            _ => self.method,
        }
    }

    pub fn is_encrypted(&self) -> bool {
        self.flags & gp::ENCRYPTED != 0
    }

    /// The encryption method (`em::*`).
    pub fn encryption_method(&self) -> u16 {
        if !self.is_encrypted() {
            return em::NONE;
        }
        if let Some(a) = &self.aes
            && self.method == cm::AES
        {
            return 0x0100 + a.strength as u16;
        }
        if self.flags & gp::STRONG_ENCRYPTION != 0 {
            return em::UNKNOWN;
        }
        em::TRAD_PKWARE
    }

    pub fn is_dir(&self) -> bool {
        self.name.last() == Some(&b'/')
    }

    pub fn utf8(&self) -> bool {
        self.flags & gp::UTF8 != 0
    }

    /// The modification time (DOS fields read as UTC).
    pub fn mtime(&self) -> i64 {
        crate::dostime::dos_to_unix(self.dos_time, self.dos_date)
    }

    /// The host system of "version made by" (`ZipArchive::OPSYS_*`).
    pub fn opsys(&self) -> u8 {
        (self.version_made_by >> 8) as u8
    }
}

/// Options of [`parse`].
#[derive(Debug, Clone, Copy, Default)]
pub struct ReadOptions {
    /// Cross-check local headers and entry placement (`ZipArchive::CHECKCONS`).
    pub check_consistency: bool,
}

/// A parsed archive: the entries in central directory order and the
/// archive comment.
#[derive(Debug, Clone, Default)]
pub struct Archive {
    pub entries: Vec<Entry>,
    pub comment: Vec<u8>,
    /// The archive uses the ZIP64 end records.
    pub zip64: bool,
    pub cd_offset: u64,
    pub cd_size: u64,
    pub eocd_offset: u64,
}

struct Eocd {
    pos: usize,
    entries: u64,
    cd_size: u64,
    cd_offset: u64,
    comment: Vec<u8>,
    zip64: bool,
}

fn read_eocd(data: &[u8], pos: usize, check: bool) -> Result<Eocd> {
    let disk = u16le(data, pos + 4);
    let cd_disk = u16le(data, pos + 6);
    let n_disk = u16le(data, pos + 8);
    let mut entries = u16le(data, pos + 10) as u64;
    let mut cd_size = u32le(data, pos + 12) as u64;
    let mut cd_offset = u32le(data, pos + 16) as u64;
    let clen = u16le(data, pos + 20) as usize;
    let avail = data.len() - pos - 22;
    if check && clen != avail {
        return Err(Error::new(er::INCONS));
    }
    let comment = data[pos + 22..pos + 22 + clen.min(avail)].to_vec();
    let mut zip64 = false;
    if pos >= 20 && u32le(data, pos - 20) == SIG_EOCD64_LOCATOR {
        let off = u64le(data, pos - 20 + 8);
        if let Ok(o) = usize::try_from(off)
            && o + 56 <= data.len()
            && u32le(data, o) == SIG_EOCD64
        {
            zip64 = true;
            let disk64 = u32le(data, o + 16);
            let cd_disk64 = u32le(data, o + 20);
            if disk64 != 0 || cd_disk64 != 0 {
                return Err(Error::new(er::MULTIDISK));
            }
            entries = u64le(data, o + 32);
            cd_size = u64le(data, o + 40);
            cd_offset = u64le(data, o + 48);
        }
    }
    if !zip64 && (disk != 0 || cd_disk != 0 || n_disk as u64 != entries) {
        return Err(Error::new(er::MULTIDISK));
    }
    Ok(Eocd {
        pos,
        entries,
        cd_size,
        cd_offset,
        comment,
        zip64,
    })
}

/// Applies a ZIP64 extra record to the saturated fields.
fn apply_zip64(e: &mut Entry, d: &[u8]) {
    let mut p = 0;
    let next = |p: &mut usize, w: usize| -> Option<u64> {
        if *p + w > d.len() {
            return None;
        }
        let v = if w == 8 {
            u64le(d, *p)
        } else {
            u32le(d, *p) as u64
        };
        *p += w;
        Some(v)
    };
    if e.size == 0xFFFF_FFFF
        && let Some(v) = next(&mut p, 8)
    {
        e.size = v;
    }
    if e.comp_size == 0xFFFF_FFFF
        && let Some(v) = next(&mut p, 8)
    {
        e.comp_size = v;
    }
    if e.local_offset == 0xFFFF_FFFF
        && let Some(v) = next(&mut p, 8)
    {
        e.local_offset = v;
    }
    if e.disk_start == 0xFFFF
        && let Some(v) = next(&mut p, 4)
    {
        e.disk_start = v as u32;
    }
}

/// Parses an archive held in memory.
pub fn parse(data: &[u8], opts: ReadOptions) -> Result<Archive> {
    if data.len() < 22 {
        return Err(Error::new(er::NOZIP));
    }
    // The end record is within the last 64K + 22 bytes; the last one that
    // describes a central directory wins.
    let lo = data.len().saturating_sub(22 + 0xFFFF);
    let mut found = false;
    let mut last_err = Error::new(er::NOZIP);
    let mut pos = data.len() - 22;
    let eocd = loop {
        if u32le(data, pos) == SIG_EOCD {
            found = true;
            match read_eocd(data, pos, opts.check_consistency) {
                Ok(e) => {
                    let fits = e.cd_offset.checked_add(e.cd_size).is_some_and(|end| end <= e.pos as u64);
                    let starts = e.cd_size == 0
                        || (e.cd_offset as usize + 4 <= data.len() && u32le(data, e.cd_offset as usize) == SIG_CENTRAL);
                    if fits && starts {
                        break e;
                    }
                    last_err = Error::new(er::INCONS);
                }
                Err(e) => last_err = e,
            }
        }
        if pos == lo {
            return Err(if found { last_err } else { Error::new(er::NOZIP) });
        }
        pos -= 1;
    };
    if opts.check_consistency && eocd.cd_offset + eocd.cd_size != eocd.pos as u64 && !eocd.zip64 {
        return Err(Error::new(er::INCONS));
    }
    let mut entries = Vec::with_capacity(eocd.entries.min(1 << 20) as usize);
    let mut p = eocd.cd_offset as usize;
    let end = (eocd.cd_offset + eocd.cd_size) as usize;
    for _ in 0..eocd.entries {
        if p + 46 > end || u32le(data, p) != SIG_CENTRAL {
            return Err(Error::new(er::INCONS));
        }
        let nlen = u16le(data, p + 28) as usize;
        let xlen = u16le(data, p + 30) as usize;
        let clen = u16le(data, p + 32) as usize;
        if p + 46 + nlen + xlen + clen > end {
            return Err(Error::new(er::INCONS));
        }
        let mut e = Entry {
            version_made_by: u16le(data, p + 4),
            version_needed: u16le(data, p + 6),
            flags: u16le(data, p + 8),
            method: u16le(data, p + 10),
            dos_time: u16le(data, p + 12),
            dos_date: u16le(data, p + 14),
            crc: u32le(data, p + 16),
            comp_size: u32le(data, p + 20) as u64,
            size: u32le(data, p + 24) as u64,
            disk_start: u16le(data, p + 34) as u32,
            internal_attr: u16le(data, p + 36),
            external_attr: u32le(data, p + 38),
            local_offset: u32le(data, p + 42) as u64,
            name: data[p + 46..p + 46 + nlen].to_vec(),
            extra: data[p + 46 + nlen..p + 46 + nlen + xlen].to_vec(),
            comment: data[p + 46 + nlen + xlen..p + 46 + nlen + xlen + clen].to_vec(),
            ..Default::default()
        };
        if let Some(z) = find_extra(&e.extra, extra_id::ZIP64) {
            let z = z.to_vec();
            apply_zip64(&mut e, &z);
        }
        if e.disk_start != 0 {
            return Err(Error::new(er::MULTIDISK));
        }
        e.aes = find_extra(&e.extra, extra_id::AES).and_then(AesInfo::parse);
        entries.push(e);
        p += 46 + nlen + xlen + clen;
    }
    if opts.check_consistency && p != end {
        return Err(Error::new(er::INCONS));
    }
    // Local headers.
    for e in entries.iter_mut() {
        let Ok(o) = usize::try_from(e.local_offset) else {
            continue;
        };
        if o + 30 > data.len() || u32le(data, o) != SIG_LOCAL {
            if opts.check_consistency {
                return Err(Error::new(er::INCONS));
            }
            continue;
        }
        let nlen = u16le(data, o + 26) as usize;
        let xlen = u16le(data, o + 28) as usize;
        if o + 30 + nlen + xlen > data.len() {
            if opts.check_consistency {
                return Err(Error::new(er::INCONS));
            }
            continue;
        }
        e.local_extra = data[o + 30 + nlen..o + 30 + nlen + xlen].to_vec();
        let dofs = (o + 30 + nlen + xlen) as u64;
        if opts.check_consistency {
            let lflags = u16le(data, o + 6);
            let lmethod = u16le(data, o + 8);
            if &data[o + 30..o + 30 + nlen] != e.name.as_slice()
                || lmethod != e.method
                || (lflags & !gp::DATA_DESCRIPTOR) != (e.flags & !gp::DATA_DESCRIPTOR)
            {
                return Err(Error::new(er::INCONS));
            }
            if lflags & gp::DATA_DESCRIPTOR == 0 {
                let mut lcomp = u32le(data, o + 18) as u64;
                let mut lsize = u32le(data, o + 22) as u64;
                if let Some(z) = find_extra(&e.local_extra, extra_id::ZIP64)
                    && z.len() >= 16
                {
                    if lsize == 0xFFFF_FFFF {
                        lsize = u64le(z, 0);
                    }
                    if lcomp == 0xFFFF_FFFF {
                        lcomp = u64le(z, 8);
                    }
                }
                if u32le(data, o + 14) != e.crc || lcomp != e.comp_size || lsize != e.size {
                    return Err(Error::new(er::INCONS));
                }
            }
            if dofs + e.comp_size > eocd.cd_offset {
                return Err(Error::new(er::INCONS));
            }
        }
        e.data_offset = Some(dofs);
    }
    if opts.check_consistency {
        // entries must not overlap
        let mut spans: Vec<(u64, u64)> = entries
            .iter()
            .map(|e| (e.local_offset, e.data_offset.unwrap_or(e.local_offset) + e.comp_size))
            .collect();
        spans.sort();
        for w in spans.windows(2) {
            if w[0].1 > w[1].0 {
                return Err(Error::new(er::INCONS));
            }
        }
    }
    Ok(Archive {
        entries,
        comment: eocd.comment,
        zip64: eocd.zip64,
        cd_offset: eocd.cd_offset,
        cd_size: eocd.cd_size,
        eocd_offset: eocd.pos as u64,
    })
}

/// Decoded entry data.
#[derive(Debug, Clone)]
pub struct Decoded {
    pub data: Vec<u8>,
    /// The CRC matched (always true for AE-2 entries, which store none).
    pub crc_ok: bool,
}

/// Decrypts (with `password` when the entry is encrypted) and decompresses
/// an entry's raw data. Errors: `ER_NOPASSWD`, `ER_WRONGPASSWD`,
/// `ER_ENCRNOTSUPP`, `ER_COMPNOTSUPP`, `ER_COMPRESSED_DATA`, and
/// `ER_EOF`. A decoded length other than the declared size clears
/// [`Decoded::crc_ok`].
pub fn decode_entry(e: &Entry, raw: &[u8], password: Option<&[u8]>) -> Result<Decoded> {
    let plain;
    let body: &[u8] = if e.is_encrypted() {
        let Some(pw) = password else {
            return Err(Error::new(er::NOPASSWD));
        };
        match e.encryption_method() {
            em::TRAD_PKWARE => {
                let check =
                    crypto::pkware_check_byte(e.crc, e.dos_time, e.flags & gp::DATA_DESCRIPTOR != 0);
                plain = crypto::pkware_decrypt(raw, pw, check)?;
                &plain
            }
            em::AES_128 | em::AES_192 | em::AES_256 => {
                let a = e.aes.expect("aes entry");
                plain = crypto::aes_decrypt(raw, pw, a.strength)?;
                &plain
            }
            _ => return Err(Error::new(er::ENCRNOTSUPP)),
        }
    } else {
        raw
    };
    let m = e.real_method();
    if !codec::can_decompress(m) {
        return Err(Error::new(er::COMPNOTSUPP));
    }
    let data = codec::decompress(m, e.flags, body, e.size)?;
    // a length that differs from the declared size fails like a CRC
    // mismatch (test bug49072.phpt: a damaged entry gives "CRC error")
    let ae2 = e.aes.is_some_and(|a| a.version == 2) && e.method == cm::AES;
    let crc_ok = data.len() as u64 == e.size && (ae2 || crate::crc::crc32(&data) == e.crc);
    Ok(Decoded { data, crc_ok })
}

impl Archive {
    /// The raw (compressed, possibly encrypted) data of entry `i`.
    pub fn raw<'d>(&self, data: &'d [u8], i: usize) -> Result<&'d [u8]> {
        let e = self.entries.get(i).ok_or(Error::new(er::INVAL))?;
        let start = e.data_offset.ok_or(Error::new(er::INCONS))?;
        let end = start.checked_add(e.comp_size).ok_or(Error::new(er::INCONS))?;
        if end > data.len() as u64 {
            return Err(Error::new(er::EOF));
        }
        Ok(&data[start as usize..end as usize])
    }

    /// Entry `i` decoded, without failing on a CRC mismatch.
    pub fn decode(&self, data: &[u8], i: usize, password: Option<&[u8]>) -> Result<Decoded> {
        let raw = self.raw(data, i)?;
        decode_entry(&self.entries[i], raw, password)
    }

    /// Entry `i` decoded; a CRC mismatch is `ER_CRC`.
    pub fn read(&self, data: &[u8], i: usize, password: Option<&[u8]>) -> Result<Vec<u8>> {
        let d = self.decode(data, i, password)?;
        if !d.crc_ok {
            return Err(Error::new(er::CRC));
        }
        Ok(d.data)
    }

    /// The index of the entry named `name` (exact bytes).
    pub fn locate(&self, name: &[u8]) -> Option<usize> {
        self.entries.iter().position(|e| e.name == name)
    }

    /// Whether the archive is in TorrentZip form: the comment is
    /// `TORRENTZIPPED-` and the CRC-32 of the central directory in
    /// upper-case hex.
    pub fn is_torrentzip(&self, data: &[u8]) -> bool {
        crate::write::torrentzip_comment_matches(&self.comment, data, self.cd_offset, self.cd_size)
    }
}
