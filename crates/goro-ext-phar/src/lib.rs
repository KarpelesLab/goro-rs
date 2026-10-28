//! The archive formats of PHP's phar extension, on bytes: the phar format
//! (PHP manual: "Phar File Format": stub, manifest, file contents,
//! signature), tar (POSIX ustar) and zip (PKWARE APPNOTE) archives as phar
//! and PharData use them, entry compression (zlib, bzip2), signatures
//! (MD5, SHA-1, SHA-256, SHA-512, OpenSSL through a callback) and the
//! default stub. Error messages are PHP's, from ext/phar/tests and
//! observations on PHP 8.5.
//!
//! An [`Archive`] holds every entry in memory, as stored (possibly
//! compressed) and decoded on demand by [`Entry::contents`], which checks
//! sizes and CRCs as PHP does when a file is opened.

pub mod phar;
pub mod sig;
pub mod stub;
pub mod tar;
pub mod zip;

use std::collections::HashMap;

pub use sig::SigType;

/// The container format of an archive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Phar,
    Tar,
    Zip,
}

/// A compression: of one entry, or of a whole phar/tar file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Comp {
    None,
    /// Entries: raw deflate; whole files: gzip.
    Gz,
    Bz2,
}

impl Comp {
    /// Phar manifest flag bits (PHP manual: Phar::GZ = 0x1000, BZ2 = 0x2000).
    pub fn flag(self) -> u32 {
        match self {
            Comp::None => 0,
            Comp::Gz => 0x1000,
            Comp::Bz2 => 0x2000,
        }
    }
}

/// Manifest flag of a phar with a signature.
pub const FLAG_SIGNATURE: u32 = 0x10000;
/// Entry flag bits holding the compression.
pub const FLAG_COMP_MASK: u32 = 0xf000;

/// CRC-32 (ISO-HDLC, as zip, gzip and PHP's crc32()).
pub fn crc32(data: &[u8]) -> u32 {
    static TABLE: std::sync::OnceLock<[u32; 256]> = std::sync::OnceLock::new();
    let t = TABLE.get_or_init(|| {
        let mut t = [0u32; 256];
        for (i, e) in t.iter_mut().enumerate() {
            let mut c = i as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 {
                    0xedb8_8320 ^ (c >> 1)
                } else {
                    c >> 1
                };
            }
            *e = c;
        }
        t
    });
    let mut c = !0u32;
    for &b in data {
        c = t[((c ^ b as u32) & 0xff) as usize] ^ (c >> 8);
    }
    !c
}

/// Raw deflate at zlib's default level.
pub fn deflate(data: &[u8]) -> Vec<u8> {
    goro_ext_zlib::compress(data, -1, goro_ext_zlib::Wrap::Raw)
}

/// Raw inflate; none when the data is not a complete deflate stream.
pub fn inflate(data: &[u8]) -> Option<Vec<u8>> {
    goro_ext_zlib::gzinflate(data, 0).ok()
}

/// gzip (a whole .phar.gz or .tar.gz).
pub fn gzip(data: &[u8]) -> Vec<u8> {
    goro_ext_zlib::compress(data, -1, goro_ext_zlib::Wrap::Gzip)
}

/// gunzip; none when the data is not gzip.
pub fn gunzip(data: &[u8]) -> Option<Vec<u8>> {
    goro_ext_zlib::gzdecode(data, 0).ok()
}

/// bzip2 at block size 9.
pub fn bzip2(data: &[u8]) -> Vec<u8> {
    use compcol::{Encoder as _, Status};
    let mut enc = compcol::bzip2::Encoder::with_config(compcol::bzip2::EncoderConfig { level: 9 });
    let mut out = Vec::new();
    let mut buf = vec![0u8; 64 * 1024];
    let mut pos = 0;
    while pos < data.len() {
        let Ok((p, _)) = enc.encode(&data[pos..], &mut buf) else {
            break;
        };
        out.extend_from_slice(&buf[..p.written]);
        pos += p.consumed;
        if p.consumed == 0 && p.written == 0 {
            break;
        }
    }
    while let Ok((p, st)) = enc.finish(&mut buf) {
        out.extend_from_slice(&buf[..p.written]);
        if st != Status::OutputFull {
            break;
        }
    }
    out
}

/// bunzip2 of the first stream; none on an error.
pub fn bunzip2(data: &[u8]) -> Option<Vec<u8>> {
    use compcol::{Decoder as _, Status};
    let mut dec = compcol::bzip2::Decoder::with_config(
        compcol::bzip2::DecoderConfig::default().with_multi_stream(false),
    );
    let mut out = Vec::new();
    let mut buf = vec![0u8; 64 * 1024];
    let mut pos = 0;
    loop {
        let r = if pos < data.len() {
            dec.decode(&data[pos..], &mut buf)
        } else {
            dec.finish(&mut buf)
        };
        let (p, st) = r.ok()?;
        out.extend_from_slice(&buf[..p.written]);
        pos += p.consumed;
        match st {
            Status::StreamEnd => return Some(out),
            Status::OutputFull => {}
            Status::InputEmpty => {
                if pos >= data.len() && p.written == 0 && p.consumed == 0 {
                    return None;
                }
            }
        }
    }
}

/// Compresses an entry's contents.
pub fn compress(c: Comp, data: &[u8]) -> Vec<u8> {
    match c {
        Comp::None => data.to_vec(),
        Comp::Gz => deflate(data),
        Comp::Bz2 => bzip2(data),
    }
}

/// One file or directory of an archive.
#[derive(Clone, Debug)]
pub struct Entry {
    /// The path inside the archive, without a leading or trailing slash.
    pub name: Vec<u8>,
    pub is_dir: bool,
    /// The bytes as stored, compressed with `comp`.
    pub raw: Vec<u8>,
    pub comp: Comp,
    /// The uncompressed size and CRC the archive claims.
    pub size: u32,
    pub crc32: u32,
    pub mtime: u32,
    /// Phar manifest flags: permissions in the low 9 bits, compression.
    pub flags: u32,
    /// Serialized metadata.
    pub metadata: Option<Vec<u8>>,
    /// Decoded and checked contents.
    pub data: Option<Vec<u8>>,
    /// A tar link's target (symbolic or hard).
    pub link: Option<Vec<u8>>,
    /// Whether the CRC was found right (zip and phar entries are checked
    /// when opened).
    pub checked: bool,
}

impl Entry {
    /// A new file with `data` (uncompressed).
    pub fn file(name: &[u8], data: Vec<u8>, mtime: u32, perms: u32) -> Entry {
        Entry {
            name: name.to_vec(),
            is_dir: false,
            size: data.len() as u32,
            crc32: crc32(&data),
            raw: data.clone(),
            comp: Comp::None,
            mtime,
            flags: perms & 0o777,
            metadata: None,
            data: Some(data),
            link: None,
            checked: true,
        }
    }

    /// A new directory.
    pub fn dir(name: &[u8], mtime: u32) -> Entry {
        Entry {
            name: name.to_vec(),
            is_dir: true,
            raw: Vec::new(),
            comp: Comp::None,
            size: 0,
            crc32: 0,
            mtime,
            flags: 0o777,
            metadata: None,
            data: Some(Vec::new()),
            link: None,
            checked: true,
        }
    }

    pub fn perms(&self) -> u32 {
        self.flags & 0o777
    }

    /// The uncompressed contents, checked against the claimed size and
    /// CRC (PHP's messages for a phar named `fname`).
    pub fn contents(&mut self, fname: &[u8]) -> Result<&[u8], String> {
        if self.data.is_none() {
            let corrupt = |what: &str, name: &[u8]| {
                format!(
                    "internal corruption of phar \"{}\" ({} file \"{}\")",
                    String::from_utf8_lossy(fname),
                    what,
                    String::from_utf8_lossy(name)
                )
            };
            let data = match self.comp {
                Comp::None => self.raw.clone(),
                Comp::Gz => match inflate(&self.raw) {
                    Some(d) => d,
                    None => {
                        return Err(format!(
                            "internal corruption of phar \"{}\" (actual filesize mismatch on file \"{}\")",
                            String::from_utf8_lossy(fname),
                            String::from_utf8_lossy(&self.name)
                        ));
                    }
                },
                Comp::Bz2 => match bunzip2(&self.raw) {
                    Some(d) => d,
                    None => {
                        return Err(corrupt("actual filesize mismatch on", &self.name));
                    }
                },
            };
            if data.len() != self.size as usize {
                return Err(corrupt("actual filesize mismatch on", &self.name));
            }
            if crc32(&data) != self.crc32 {
                return Err(corrupt("crc32 mismatch on", &self.name));
            }
            self.data = Some(data);
            self.checked = true;
        }
        Ok(self.data.as_deref().unwrap())
    }

    /// Replaces the contents (kept uncompressed until the archive is
    /// written).
    pub fn set_contents(&mut self, data: Vec<u8>) {
        self.size = data.len() as u32;
        self.crc32 = crc32(&data);
        self.raw = compress(self.comp, &data);
        self.data = Some(data);
        self.checked = true;
    }

    /// Changes the compression (the contents must be readable).
    pub fn set_comp(&mut self, c: Comp, fname: &[u8]) -> Result<(), String> {
        if self.is_dir || c == self.comp {
            self.comp = if self.is_dir { Comp::None } else { c };
            return Ok(());
        }
        let data = self.contents(fname)?.to_vec();
        self.comp = c;
        self.raw = compress(c, &data);
        self.flags = (self.flags & !FLAG_COMP_MASK) | c.flag();
        Ok(())
    }
}

/// A phar, tar or zip archive.
#[derive(Clone, Debug)]
pub struct Archive {
    /// The file it is read from and written to.
    pub fname: Vec<u8>,
    pub format: Format,
    /// The compression of the whole file (phar and tar).
    pub comp: Comp,
    pub alias: Option<Vec<u8>>,
    /// The stub (phar: the bytes before the manifest; tar/zip:
    /// `.phar/stub.php`).
    pub stub: Vec<u8>,
    /// Serialized archive metadata.
    pub metadata: Option<Vec<u8>>,
    entries: Vec<Entry>,
    index: HashMap<Vec<u8>, usize>,
    /// The signature read or last written: its type and bytes.
    pub sig: Option<(SigType, Vec<u8>)>,
    /// The signature to write (none for PharData).
    pub sig_type: Option<SigType>,
    /// The manifest API version (phar format).
    pub api: u16,
    /// The global manifest flags read (phar format).
    pub flags: u32,
    /// A PharData archive (no stub, alias or signature by default).
    pub is_data: bool,
    /// The archive comment (zip).
    pub comment: Vec<u8>,
}

impl Archive {
    pub fn new(fname: &[u8], format: Format, is_data: bool) -> Archive {
        Archive {
            fname: fname.to_vec(),
            format,
            comp: Comp::None,
            alias: None,
            stub: Vec::new(),
            metadata: None,
            entries: Vec::new(),
            index: HashMap::new(),
            sig: None,
            sig_type: if is_data { None } else { Some(SigType::Sha256) },
            api: 0x1110,
            flags: 0,
            is_data,
            comment: Vec::new(),
        }
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    pub fn entries_mut(&mut self) -> &mut [Entry] {
        &mut self.entries
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn get(&self, name: &[u8]) -> Option<&Entry> {
        self.index.get(name).map(|&i| &self.entries[i])
    }

    pub fn get_mut(&mut self, name: &[u8]) -> Option<&mut Entry> {
        let i = *self.index.get(name)?;
        Some(&mut self.entries[i])
    }

    /// Adds or replaces an entry (a replaced one keeps its place).
    pub fn put(&mut self, e: Entry) {
        match self.index.get(&e.name) {
            Some(&i) => self.entries[i] = e,
            None => {
                self.index.insert(e.name.clone(), self.entries.len());
                self.entries.push(e);
            }
        }
    }

    /// Removes an entry.
    pub fn remove(&mut self, name: &[u8]) -> Option<Entry> {
        let i = self.index.remove(name)?;
        let e = self.entries.remove(i);
        for (k, v) in self.entries.iter().enumerate().skip(i) {
            self.index.insert(v.name.clone(), k);
        }
        Some(e)
    }

    /// Renames an entry in place.
    pub fn rename(&mut self, from: &[u8], to: &[u8]) -> bool {
        let Some(i) = self.index.remove(from) else {
            return false;
        };
        if let Some(j) = self.index.remove(to) {
            // the target is replaced
            self.entries[i].name = to.to_vec();
            self.entries.remove(j);
            self.reindex();
            return true;
        }
        self.entries[i].name = to.to_vec();
        self.index.insert(to.to_vec(), i);
        true
    }

    fn reindex(&mut self) {
        self.index.clear();
        for (k, v) in self.entries.iter().enumerate() {
            self.index.insert(v.name.clone(), k);
        }
    }

    /// Whether `dir` (no trailing slash; empty for the root) is a
    /// directory: an explicit entry or a prefix of one.
    pub fn is_dir(&self, dir: &[u8]) -> bool {
        if dir.is_empty() {
            return true;
        }
        if let Some(e) = self.get(dir) {
            return e.is_dir;
        }
        let mut p = dir.to_vec();
        p.push(b'/');
        self.entries.iter().any(|e| e.name.starts_with(&p))
    }

    /// The names directly inside `dir` (no trailing slash; empty for the
    /// root), files and directories, in manifest order.
    pub fn list_dir(&self, dir: &[u8]) -> Vec<Vec<u8>> {
        let mut prefix = dir.to_vec();
        if !prefix.is_empty() {
            prefix.push(b'/');
        }
        let mut out: Vec<Vec<u8>> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for e in &self.entries {
            if !e.name.starts_with(&prefix) || e.name.len() == prefix.len() {
                continue;
            }
            if is_meta_name(&e.name) && !self.is_data {
                continue;
            }
            let rest = &e.name[prefix.len()..];
            let first = match rest.iter().position(|&c| c == b'/') {
                Some(p) => &rest[..p],
                None => rest,
            };
            if first.is_empty() {
                continue;
            }
            if seen.insert(first.to_vec()) {
                out.push(first.to_vec());
            }
        }
        out
    }

    /// Whether any entry is compressed with `c`.
    pub fn any_comp(&self, c: Comp) -> bool {
        self.entries.iter().any(|e| !e.is_dir && e.comp == c)
    }

    /// Serializes the archive in its format, compressing the whole file
    /// when `comp` says so. `now` is the time stamp of the entries the
    /// format adds (tar/zip `.phar/` files); `sign` makes OpenSSL
    /// signatures.
    pub fn serialize(&mut self, now: u32, sign: &sig::Signer) -> Result<Vec<u8>, String> {
        let body = match self.format {
            Format::Phar => phar::write(self, sign)?,
            Format::Tar => tar::write(self, now, sign)?,
            Format::Zip => zip::write(self, now, sign)?,
        };
        Ok(match (self.format, self.comp) {
            (Format::Zip, _) | (_, Comp::None) => body,
            (_, Comp::Gz) => gzip(&body),
            (_, Comp::Bz2) => bzip2(&body),
        })
    }

    /// Reads an archive file's bytes, recognizing its format: gzip or
    /// bzip2 around a phar or tar, a zip, a tar, or a phar (whose stub
    /// ends with `__HALT_COMPILER();`).
    pub fn parse(fname: &[u8], data: &[u8], opts: &ParseOptions) -> Result<Archive, String> {
        let (data, comp) = if data.starts_with(b"\x1f\x8b") {
            match gunzip(data) {
                Some(d) => (d, Comp::Gz),
                None => {
                    return Err(format!(
                        "unable to decompress gzipped phar archive \"{}\"",
                        String::from_utf8_lossy(fname)
                    ));
                }
            }
        } else if data.starts_with(b"BZh") {
            match bunzip2(data) {
                Some(d) => (d, Comp::Bz2),
                None => {
                    return Err(format!(
                        "unable to decompress bzipped phar archive \"{}\"",
                        String::from_utf8_lossy(fname)
                    ));
                }
            }
        } else {
            (data.to_vec(), Comp::None)
        };
        let mut a = if zip::looks_like_zip(&data) {
            zip::parse(fname, &data, opts)?
        } else if tar::looks_like_tar(&data) {
            tar::parse(fname, &data, opts)?
        } else if opts.hint == Some(Format::Zip) {
            zip::parse(fname, &data, opts)?
        } else if opts.hint == Some(Format::Tar) {
            tar::parse(fname, &data, opts)?
        } else {
            phar::parse(fname, &data, opts)?
        };
        a.comp = comp;
        Ok(a)
    }
}

/// Options of [`Archive::parse`].
#[derive(Default)]
pub struct ParseOptions {
    /// phar.require_hash: a phar without a signature is refused.
    pub require_hash: bool,
    /// Verifies OpenSSL signatures: (type, signed data, signature) →
    /// whether it verifies; an error message when no key is found.
    pub verify: Option<Box<sig::Verifier>>,
    /// Opened as PharData.
    pub is_data: bool,
    /// The format the file name suggests (".zip", ".tar"), for contents
    /// that are neither clearly.
    pub hint: Option<Format>,
}

/// Whether a tar/zip entry name is one of phar's own (`.phar/...`).
pub fn is_meta_name(name: &[u8]) -> bool {
    name == b".phar" || name.starts_with(b".phar/")
}

/// PHP's `"%s"` of a byte string.
pub(crate) fn lossy(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

pub(crate) fn rd32(b: &[u8], p: usize) -> u32 {
    u32::from_le_bytes([b[p], b[p + 1], b[p + 2], b[p + 3]])
}

pub(crate) fn rd16(b: &[u8], p: usize) -> u16 {
    u16::from_le_bytes([b[p], b[p + 1]])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc() {
        assert_eq!(crc32(b"hello"), 0x3610a686);
    }

    #[test]
    fn roundtrip_formats() {
        for f in [Format::Phar, Format::Tar, Format::Zip] {
            let mut a = Archive::new(b"/x/a.phar", f, false);
            a.stub = stub::default_stub(None, None).unwrap();
            a.put(Entry::file(b"a.txt", b"hello".to_vec(), 0, 0o644));
            let mut e = Entry::file(b"d/b.txt", b"world".to_vec(), 0, 0o644);
            e.set_comp(Comp::Gz, b"x").unwrap();
            a.put(e);
            a.put(Entry::dir(b"empty", 0));
            a.metadata = Some(b"i:1;".to_vec());
            let bytes = a.serialize(0, &|_, _| Err("no".into())).unwrap();
            let mut b = Archive::parse(b"/x/a.phar", &bytes, &ParseOptions::default()).unwrap();
            assert_eq!(b.format, f);
            assert_eq!(
                b.get_mut(b"a.txt").unwrap().contents(b"x").unwrap(),
                b"hello"
            );
            assert_eq!(
                b.get_mut(b"d/b.txt").unwrap().contents(b"x").unwrap(),
                b"world"
            );
            assert!(b.get(b"empty").unwrap().is_dir);
            assert_eq!(b.metadata.as_deref(), Some(&b"i:1;"[..]));
            assert!(b.sig.is_some());
        }
    }

    /// Archives made by PHP 8.5, read when PHAR_FIXTURE_DIR names a
    /// directory holding them (a.phar, t.tar, pt.phar.tar, z.zip,
    /// pz.phar.zip): parsed, signatures checked, and written again the
    /// same.
    #[test]
    fn parse_php_archives() {
        let Ok(dir) = std::env::var("PHAR_FIXTURE_DIR") else {
            return;
        };
        for (f, n) in [
            ("a.phar", 3),
            ("t.tar", 3),
            ("pt.phar.tar", 1),
            ("z.zip", 2),
            ("pz.phar.zip", 1),
        ] {
            let data = std::fs::read(format!("{dir}/{f}")).unwrap();
            let opts = ParseOptions {
                is_data: !f.contains(".phar"),
                ..Default::default()
            };
            let a =
                Archive::parse(f.as_bytes(), &data, &opts).unwrap_or_else(|e| panic!("{f}: {e}"));
            assert_eq!(a.len(), n, "{f}");
            let mut b = a.clone();
            let out = b.serialize(0, &|_, _| Err("no".into())).unwrap();
            if f == "a.phar" || f == "t.tar" || f == "z.zip" {
                assert_eq!(out, data, "{f}");
            }
        }
    }
}
