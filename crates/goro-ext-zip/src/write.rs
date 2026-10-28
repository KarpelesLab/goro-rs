//! Writing archives (PKWARE APPNOTE.TXT section 4.3): local headers with
//! sizes (no data descriptors), the central directory and the end record,
//! with ZIP64 records when sizes, offsets or the entry count need them.
//!
//! Entries are either copied as stored ([`Payload::Raw`], for unchanged
//! entries of an existing archive) or compressed and encrypted by the
//! writer ([`Payload::Plain`]).
//!
//! TorrentZip output ([`torrentzip`]) follows the published TorrentZip
//! format: entries sorted by lower-cased name, deflated at level 9, DOS
//! time 1996-12-24 23:32, no extra fields or comments, and the archive
//! comment `TORRENTZIPPED-` + the CRC-32 of the central directory.

use std::borrow::Cow;

use crate::codec::{self, cm};
use crate::crypto::{self, AesStrength};
use crate::error::{Error, Result, er};
use crate::read::{AesInfo, SIG_CENTRAL, SIG_EOCD, SIG_EOCD64, SIG_EOCD64_LOCATOR, SIG_LOCAL, extra_id, gp};

/// Encryption of a [`Payload::Plain`] entry.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Encryption {
    #[default]
    None,
    /// Traditional PKWARE encryption with this password.
    Pkware(Vec<u8>),
    /// WinZip AES with this password.
    Aes(AesStrength, Vec<u8>),
}

/// The data of an entry to write.
#[derive(Debug, Clone)]
pub enum Payload<'a> {
    /// Bytes copied as stored (compressed, possibly encrypted).
    Raw {
        data: Cow<'a, [u8]>,
        /// The stored method (99 for WinZip AES).
        method: u16,
        /// General purpose bits kept from the source: encryption (0, 6),
        /// method options (1, 2).
        flags: u16,
        crc: u32,
        /// Size of the decoded data.
        size: u64,
        /// WinZip AES parameters (written as extra field 0x9901).
        aes: Option<AesInfo>,
    },
    /// Data the writer compresses with `method` at `level` (-1: default)
    /// and encrypts. With `store_if_larger`, data that does not shrink is
    /// stored instead (libzip's behaviour for `ZipArchive::CM_DEFAULT`).
    Plain {
        data: Cow<'a, [u8]>,
        method: u16,
        level: i32,
        encryption: Encryption,
        store_if_larger: bool,
    },
}

/// One entry to write. The ZIP64 (0x0001) and WinZip AES (0x9901) extra
/// records are generated; others in `local_extra`/`central_extra` are
/// written as given.
#[derive(Debug, Clone)]
pub struct WriteEntry<'a> {
    pub name: Vec<u8>,
    pub comment: Vec<u8>,
    pub local_extra: Vec<u8>,
    pub central_extra: Vec<u8>,
    pub dos_time: u16,
    pub dos_date: u16,
    /// Host system in the high byte, specification version in the low one.
    pub version_made_by: u16,
    pub internal_attr: u16,
    pub external_attr: u32,
    /// Sets general purpose bit 11 (name and comment are UTF-8).
    pub utf8: bool,
    pub payload: Payload<'a>,
}

impl<'a> WriteEntry<'a> {
    /// An entry with defaults: made by Unix / 6.3, mtime `mtime`, the
    /// regular-file or directory mode in the external attributes.
    pub fn new(name: Vec<u8>, mtime: i64, payload: Payload<'a>) -> WriteEntry<'a> {
        let (dos_time, dos_date) = crate::dostime::unix_to_dos(mtime);
        let dir = name.last() == Some(&b'/');
        WriteEntry {
            utf8: crate::text::needs_utf8_flag(&name),
            name,
            comment: Vec::new(),
            local_extra: Vec::new(),
            central_extra: Vec::new(),
            dos_time,
            dos_date,
            version_made_by: (3 << 8) | 63,
            internal_attr: 0,
            external_attr: if dir { 0o040777 << 16 } else { 0o100666 << 16 },
            payload,
        }
    }
}

struct CentralRec {
    made_by: u16,
    needed: u16,
    flags: u16,
    method: u16,
    time: u16,
    date: u16,
    crc: u32,
    comp: u64,
    size: u64,
    offset: u64,
    name: Vec<u8>,
    extra: Vec<u8>,
    comment: Vec<u8>,
    internal: u16,
    external: u32,
}

/// Builds an archive entry by entry.
#[derive(Default)]
pub struct ArchiveWriter {
    out: Vec<u8>,
    central: Vec<CentralRec>,
}

fn push16(v: &mut Vec<u8>, x: u16) {
    v.extend_from_slice(&x.to_le_bytes());
}
fn push32(v: &mut Vec<u8>, x: u32) {
    v.extend_from_slice(&x.to_le_bytes());
}
fn push64(v: &mut Vec<u8>, x: u64) {
    v.extend_from_slice(&x.to_le_bytes());
}

fn sat32(x: u64) -> u32 {
    if x >= 0xFFFF_FFFF { 0xFFFF_FFFF } else { x as u32 }
}

fn deflate_level_bits(level: i32) -> u16 {
    match level {
        8 | 9 => 2,
        2 => 4,
        1 => 6,
        _ => 0,
    }
}

impl ArchiveWriter {
    pub fn new() -> ArchiveWriter {
        ArchiveWriter::default()
    }

    /// Bytes written so far.
    pub fn len(&self) -> usize {
        self.out.len()
    }

    pub fn is_empty(&self) -> bool {
        self.out.is_empty()
    }

    /// Number of entries written.
    pub fn count(&self) -> usize {
        self.central.len()
    }

    /// Compresses/encrypts as needed and appends the entry.
    pub fn add(&mut self, e: WriteEntry<'_>) -> Result<()> {
        if e.name.len() > 0xFFFF || e.comment.len() > 0xFFFF {
            return Err(Error::new(er::INVAL));
        }
        let mut lextra = crate::read::strip_extra(&e.local_extra, &[extra_id::ZIP64, extra_id::AES]);
        let mut cextra = crate::read::strip_extra(&e.central_extra, &[extra_id::ZIP64, extra_id::AES]);
        let mut flags: u16 = if e.utf8 { gp::UTF8 } else { 0 };
        let (stored, method, crc, size, aes, extra_needed) = match e.payload {
            Payload::Raw {
                data,
                method,
                flags: f,
                crc,
                size,
                aes,
            } => {
                flags |= f & (gp::ENCRYPTED | gp::STRONG_ENCRYPTION | 6);
                let need = if aes.is_some() {
                    51
                } else if f & gp::ENCRYPTED != 0 {
                    20
                } else {
                    0
                };
                (data, method, crc, size, aes, need)
            }
            Payload::Plain {
                data,
                method,
                level,
                encryption,
                store_if_larger,
            } => {
                if !codec::can_compress(method) {
                    return Err(Error::new(er::COMPNOTSUPP));
                }
                let crc = crate::crc::crc32(&data);
                let size = data.len() as u64;
                let mut m = method;
                let mut comp: Cow<'_, [u8]> = if m == cm::STORE {
                    data
                } else {
                    let c = codec::compress(m, level, &data)?;
                    if store_if_larger && c.len() >= data.len() {
                        m = cm::STORE;
                        data
                    } else {
                        Cow::Owned(c)
                    }
                };
                if m == cm::DEFLATE {
                    flags |= deflate_level_bits(level);
                }
                match encryption {
                    Encryption::None => (comp, m, crc, size, None, 0),
                    Encryption::Pkware(pw) => {
                        flags |= gp::ENCRYPTED;
                        comp = Cow::Owned(crypto::pkware_encrypt(&comp, &pw, (crc >> 24) as u8));
                        (comp, m, crc, size, None, 20)
                    }
                    Encryption::Aes(s, pw) => {
                        flags |= gp::ENCRYPTED;
                        // AE-2 (no CRC) for small entries, as the WinZip
                        // specification recommends.
                        let version = if size < 20 { 2 } else { 1 };
                        comp = Cow::Owned(crypto::aes_encrypt(&comp, &pw, s));
                        let info = AesInfo {
                            version,
                            strength: s,
                            method: m,
                        };
                        let crc = if version == 2 { 0 } else { crc };
                        (comp, cm::AES, crc, size, Some(info), 51)
                    }
                }
            }
        };
        if let Some(a) = aes {
            for x in [&mut lextra, &mut cextra] {
                push16(x, extra_id::AES);
                push16(x, 7);
                x.extend_from_slice(&a.to_bytes());
            }
        }
        let comp = stored.len() as u64;
        let offset = self.out.len() as u64;
        let local64 = size >= 0xFFFF_FFFF || comp >= 0xFFFF_FFFF;
        let real = aes.map(|a| a.method).unwrap_or(method);
        let mut needed = codec::version_needed(real).max(extra_needed);
        if e.name.last() == Some(&b'/') {
            needed = needed.max(20);
        }
        if local64 || offset >= 0xFFFF_FFFF {
            needed = needed.max(45);
        }
        let mut lx = Vec::new();
        if local64 {
            push16(&mut lx, extra_id::ZIP64);
            push16(&mut lx, 16);
            push64(&mut lx, size);
            push64(&mut lx, comp);
        }
        lx.extend_from_slice(&lextra);
        if lx.len() > 0xFFFF {
            return Err(Error::new(er::INVAL));
        }
        let o = &mut self.out;
        push32(o, SIG_LOCAL);
        push16(o, needed);
        push16(o, flags);
        push16(o, method);
        push16(o, e.dos_time);
        push16(o, e.dos_date);
        push32(o, crc);
        push32(o, if local64 { 0xFFFF_FFFF } else { comp as u32 });
        push32(o, if local64 { 0xFFFF_FFFF } else { size as u32 });
        push16(o, e.name.len() as u16);
        push16(o, lx.len() as u16);
        o.extend_from_slice(&e.name);
        o.extend_from_slice(&lx);
        o.extend_from_slice(&stored);
        // central ZIP64 record: only the saturated fields, in order
        let mut cx = Vec::new();
        let mut z = Vec::new();
        if size >= 0xFFFF_FFFF {
            push64(&mut z, size);
        }
        if comp >= 0xFFFF_FFFF {
            push64(&mut z, comp);
        }
        if offset >= 0xFFFF_FFFF {
            push64(&mut z, offset);
        }
        if !z.is_empty() {
            push16(&mut cx, extra_id::ZIP64);
            push16(&mut cx, z.len() as u16);
            cx.extend_from_slice(&z);
        }
        cx.extend_from_slice(&cextra);
        if cx.len() > 0xFFFF {
            return Err(Error::new(er::INVAL));
        }
        self.central.push(CentralRec {
            made_by: e.version_made_by,
            needed,
            flags,
            method,
            time: e.dos_time,
            date: e.dos_date,
            crc,
            comp,
            size,
            offset,
            name: e.name,
            extra: cx,
            comment: e.comment,
            internal: e.internal_attr,
            external: e.external_attr,
        });
        Ok(())
    }

    fn central_bytes(&self) -> Vec<u8> {
        let mut c = Vec::new();
        for r in &self.central {
            push32(&mut c, SIG_CENTRAL);
            push16(&mut c, r.made_by);
            push16(&mut c, r.needed);
            push16(&mut c, r.flags);
            push16(&mut c, r.method);
            push16(&mut c, r.time);
            push16(&mut c, r.date);
            push32(&mut c, r.crc);
            push32(&mut c, sat32(r.comp));
            push32(&mut c, sat32(r.size));
            push16(&mut c, r.name.len() as u16);
            push16(&mut c, r.extra.len() as u16);
            push16(&mut c, r.comment.len() as u16);
            push16(&mut c, 0);
            push16(&mut c, r.internal);
            push32(&mut c, r.external);
            push32(&mut c, sat32(r.offset));
            c.extend_from_slice(&r.name);
            c.extend_from_slice(&r.extra);
            c.extend_from_slice(&r.comment);
        }
        c
    }

    /// Writes the central directory and end records with the archive
    /// `comment` (at most 65535 bytes) and returns the archive.
    pub fn finish(self, comment: &[u8]) -> Vec<u8> {
        let cd = self.central_bytes();
        self.finish_with(cd, comment)
    }

    fn finish_with(mut self, cd: Vec<u8>, comment: &[u8]) -> Vec<u8> {
        let cd_offset = self.out.len() as u64;
        let cd_size = cd.len() as u64;
        let n = self.central.len() as u64;
        self.out.extend_from_slice(&cd);
        let o = &mut self.out;
        if n >= 0xFFFF || cd_offset >= 0xFFFF_FFFF || cd_size >= 0xFFFF_FFFF {
            let eocd64 = o.len() as u64;
            push32(o, SIG_EOCD64);
            push64(o, 44);
            push16(o, (3 << 8) | 63);
            push16(o, 45);
            push32(o, 0);
            push32(o, 0);
            push64(o, n);
            push64(o, n);
            push64(o, cd_size);
            push64(o, cd_offset);
            push32(o, SIG_EOCD64_LOCATOR);
            push32(o, 0);
            push64(o, eocd64);
            push32(o, 1);
        }
        let comment = &comment[..comment.len().min(0xFFFF)];
        push32(o, SIG_EOCD);
        push16(o, 0);
        push16(o, 0);
        push16(o, n.min(0xFFFF) as u16);
        push16(o, n.min(0xFFFF) as u16);
        push32(o, sat32(cd_size));
        push32(o, sat32(cd_offset));
        push16(o, comment.len() as u16);
        o.extend_from_slice(comment);
        self.out
    }
}

/// Writes `entries` and `comment` as an archive.
pub fn write_archive(entries: Vec<WriteEntry<'_>>, comment: &[u8]) -> Result<Vec<u8>> {
    let mut w = ArchiveWriter::new();
    for e in entries {
        w.add(e)?;
    }
    Ok(w.finish(comment))
}

/// The empty archive: an end record alone.
pub fn empty_archive(comment: &[u8]) -> Vec<u8> {
    ArchiveWriter::new().finish(comment)
}

/// TorrentZip DOS time and date: 1996-12-24 23:32:00.
pub const TORRENTZIP_TIME: u16 = 0xBC00;
pub const TORRENTZIP_DATE: u16 = 0x2198;

/// Writes `(name, data)` entries as a TorrentZip archive.
pub fn torrentzip(mut entries: Vec<(Vec<u8>, Vec<u8>)>) -> Result<Vec<u8>> {
    entries.sort_by(|a, b| a.0.to_ascii_lowercase().cmp(&b.0.to_ascii_lowercase()));
    let mut w = ArchiveWriter::new();
    for (name, data) in entries {
        let crc = crate::crc::crc32(&data);
        let mut d = goro_ext_zlib::Deflater::new(goro_ext_zlib::DeflateParams {
            level: 9,
            window_bits: 15,
            mem_level: 8,
            strategy: goro_ext_zlib::Strategy::Default,
            wrap: goro_ext_zlib::Wrap::Raw,
        })
        .map_err(|_| Error::new(er::INTERNAL))?;
        let mut comp = Vec::new();
        d.deflate(&data, goro_ext_zlib::Flush::Finish, &mut comp);
        w.add(WriteEntry {
            utf8: false,
            name,
            comment: Vec::new(),
            local_extra: Vec::new(),
            central_extra: Vec::new(),
            dos_time: TORRENTZIP_TIME,
            dos_date: TORRENTZIP_DATE,
            version_made_by: 0,
            internal_attr: 0,
            external_attr: 0,
            payload: Payload::Raw {
                data: Cow::Owned(comp),
                method: cm::DEFLATE,
                flags: 2,
                crc,
                size: data.len() as u64,
                aes: None,
            },
        })?;
    }
    let cd = w.central_bytes();
    let comment = format!("TORRENTZIPPED-{:08X}", crate::crc::crc32(&cd));
    Ok(w.finish_with(cd, comment.as_bytes()))
}

/// Whether `comment` is the TorrentZip comment of the central directory
/// at `cd_offset..cd_offset + cd_size` of `data`.
pub fn torrentzip_comment_matches(comment: &[u8], data: &[u8], cd_offset: u64, cd_size: u64) -> bool {
    let Some(hex) = comment.strip_prefix(b"TORRENTZIPPED-") else {
        return false;
    };
    if hex.len() != 8 {
        return false;
    }
    let Some(end) = cd_offset.checked_add(cd_size) else {
        return false;
    };
    if end > data.len() as u64 {
        return false;
    }
    let crc = crate::crc::crc32(&data[cd_offset as usize..end as usize]);
    format!("{crc:08X}").as_bytes() == hex
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::read::{ReadOptions, parse};

    fn plain(data: &[u8], method: u16, enc: Encryption) -> Payload<'_> {
        Payload::Plain {
            data: Cow::Borrowed(data),
            method,
            level: -1,
            encryption: enc,
            store_if_larger: false,
        }
    }

    #[test]
    fn roundtrip() {
        let text = b"hello hello hello hello hello hello".to_vec();
        let mut e1 = WriteEntry::new(b"a.txt".to_vec(), 1_577_232_000, plain(&text, cm::DEFLATE, Encryption::None));
        e1.comment = b"first".to_vec();
        let e2 = WriteEntry::new(b"dir/".to_vec(), 1_577_232_000, plain(b"", cm::STORE, Encryption::None));
        let e3 = WriteEntry::new(
            b"secret".to_vec(),
            0,
            plain(&text, cm::BZIP2, Encryption::Aes(AesStrength::Aes256, b"pw".to_vec())),
        );
        let e4 = WriteEntry::new(
            b"trad".to_vec(),
            0,
            plain(&text, cm::STORE, Encryption::Pkware(b"pw".to_vec())),
        );
        let z = write_archive(vec![e1, e2, e3, e4], b"archive comment").unwrap();
        let a = parse(&z, ReadOptions { check_consistency: true }).unwrap();
        assert_eq!(a.entries.len(), 4);
        assert_eq!(a.comment, b"archive comment");
        assert_eq!(a.entries[0].comment, b"first");
        assert_eq!(a.entries[0].mtime(), 1_577_232_000);
        assert_eq!(a.read(&z, 0, None).unwrap(), text);
        assert!(a.entries[1].is_dir());
        assert_eq!(a.entries[2].encryption_method(), crate::read::em::AES_256);
        assert_eq!(a.entries[2].real_method(), cm::BZIP2);
        assert_eq!(a.read(&z, 2, None).unwrap_err().code, er::NOPASSWD);
        assert_eq!(a.read(&z, 2, Some(b"pw")).unwrap(), text);
        assert_eq!(a.read(&z, 3, Some(b"pw")).unwrap(), text);
        // raw copy
        let raw = a.raw(&z, 0).unwrap();
        let e = &a.entries[0];
        let copy = WriteEntry {
            name: b"b.txt".to_vec(),
            comment: Vec::new(),
            local_extra: Vec::new(),
            central_extra: Vec::new(),
            dos_time: e.dos_time,
            dos_date: e.dos_date,
            version_made_by: e.version_made_by,
            internal_attr: 0,
            external_attr: e.external_attr,
            utf8: false,
            payload: Payload::Raw {
                data: Cow::Borrowed(raw),
                method: e.method,
                flags: e.flags,
                crc: e.crc,
                size: e.size,
                aes: None,
            },
        };
        let z2 = write_archive(vec![copy], b"").unwrap();
        let a2 = parse(&z2, ReadOptions::default()).unwrap();
        assert_eq!(a2.read(&z2, 0, None).unwrap(), text);
    }

    #[test]
    fn empty_and_errors() {
        let z = empty_archive(b"");
        assert_eq!(z.len(), 22);
        assert!(parse(&z, ReadOptions::default()).unwrap().entries.is_empty());
        assert_eq!(parse(b"not a zip file at all, really", ReadOptions::default()).unwrap_err().code, er::NOZIP);
        // CRC damage
        let mut z = write_archive(
            vec![WriteEntry::new(b"x".to_vec(), 0, plain(b"abc", cm::STORE, Encryption::None))],
            b"",
        )
        .unwrap();
        z[31] ^= 1;
        let a = parse(&z, ReadOptions::default()).unwrap();
        assert_eq!(a.read(&z, 0, None).unwrap_err().code, er::CRC);
    }

    #[test]
    fn torrent() {
        let z = torrentzip(vec![(b"b".to_vec(), b"2".to_vec()), (b"A".to_vec(), b"1".to_vec())]).unwrap();
        let a = parse(&z, ReadOptions { check_consistency: true }).unwrap();
        assert!(a.is_torrentzip(&z));
        assert_eq!(a.entries[0].name, b"A");
        assert_eq!(a.read(&z, 1, None).unwrap(), b"2");
    }
}
