//! The phar file format (PHP manual: "Phar File Format"): the stub ending
//! with `__HALT_COMPILER();`, the manifest (length, entry count, API
//! version (big-endian), global flags, alias, metadata, entries), the
//! contents, and an optional signature. Messages from ext/phar/tests.

use crate::sig::{self, SigType};
use crate::stub::{find_halt, stub_end};
use crate::{Archive, Comp, Entry, FLAG_SIGNATURE, Format, ParseOptions, lossy, rd32};

fn corrupt(fname: &[u8], what: &str) -> String {
    format!("internal corruption of phar \"{}\" ({what})", lossy(fname))
}

/// Where the manifest of a phar starts, or the error of a missing
/// `__HALT_COMPILER();`.
pub fn manifest_start(data: &[u8]) -> Result<usize, String> {
    match find_halt(data) {
        Some(h) => Ok(stub_end(data, h)),
        None => Err("__HALT_COMPILER(); must be declared in a phar".into()),
    }
}

/// Parses a phar file.
pub fn parse(fname: &[u8], data: &[u8], opts: &ParseOptions) -> Result<Archive, String> {
    let start = manifest_start(data)?;
    parse_at(fname, data, start, opts)
}

/// Parses a phar whose manifest starts at `start`.
pub fn parse_at(
    fname: &[u8],
    data: &[u8],
    start: usize,
    opts: &ParseOptions,
) -> Result<Archive, String> {
    let mut a = Archive::new(fname, Format::Phar, opts.is_data);
    a.stub = data[..start].to_vec();
    if data.len() < start + 4 {
        return Err(corrupt(fname, "truncated manifest at manifest length"));
    }
    let mlen = rd32(data, start) as usize;
    if mlen > 100 * 1024 * 1024 {
        return Err(format!(
            "manifest cannot be larger than 100 MB in phar \"{}\"",
            lossy(fname)
        ));
    }
    let m0 = start + 4;
    if data.len() < m0 + mlen || mlen < 14 {
        return Err(corrupt(fname, "truncated manifest header"));
    }
    let m = &data[m0..m0 + mlen];
    let count = rd32(m, 0) as usize;
    // each entry takes at least 24 bytes (test 009.phpt, 010.phpt)
    if count > 0 && count.saturating_mul(24) > mlen.saturating_sub(10) {
        return Err(corrupt(
            fname,
            "too many manifest entries for size of manifest",
        ));
    }
    a.api = u16::from_be_bytes([m[4], m[5]]);
    if a.api >> 12 != 1 {
        return Err(format!(
            "phar \"{}\" is API version {}.{}.{}, and cannot be processed",
            lossy(fname),
            a.api >> 12,
            (a.api >> 8) & 0xf,
            (a.api >> 4) & 0xf
        ));
    }
    a.flags = rd32(m, 6);
    let alen = rd32(m, 10) as usize;
    let mut p = 14;
    if p + alen + 4 > mlen {
        return Err(corrupt(fname, "buffer overrun"));
    }
    if alen > 0 {
        a.alias = Some(m[p..p + alen].to_vec());
    }
    p += alen;
    let metalen = rd32(m, p) as usize;
    p += 4;
    if p + metalen > mlen {
        return Err(corrupt(fname, "buffer overrun"));
    }
    if metalen > 0 {
        a.metadata = Some(m[p..p + metalen].to_vec());
    }
    p += metalen;
    if count > 0 && count.saturating_mul(24) > mlen - p {
        return Err(corrupt(
            fname,
            "too many manifest entries for size of manifest",
        ));
    }
    let mut entries = Vec::with_capacity(count);
    let mut clens = Vec::with_capacity(count);
    for _ in 0..count {
        if p + 4 > mlen {
            return Err(corrupt(fname, "truncated manifest entry"));
        }
        let nlen = rd32(m, p) as usize;
        p += 4;
        if p + nlen + 24 > mlen {
            return Err(corrupt(fname, "truncated manifest entry"));
        }
        let mut name = m[p..p + nlen].to_vec();
        p += nlen;
        let size = rd32(m, p);
        let mtime = rd32(m, p + 4);
        let clen = rd32(m, p + 8);
        let crc = rd32(m, p + 12);
        let flags = rd32(m, p + 16);
        let elen = rd32(m, p + 20) as usize;
        p += 24;
        if p + elen > mlen {
            return Err(corrupt(fname, "truncated manifest entry"));
        }
        let meta = (elen > 0).then(|| m[p..p + elen].to_vec());
        p += elen;
        let comp = match flags & 0xf000 {
            0x1000 => Comp::Gz,
            0x2000 => Comp::Bz2,
            0 => Comp::None,
            _ => {
                return Err(corrupt(fname, "unknown compression method"));
            }
        };
        if comp == Comp::None && size != clen {
            return Err(corrupt(
                fname,
                "compressed and uncompressed size does not match for uncompressed entry",
            ));
        }
        let is_dir = name.ends_with(b"/");
        if is_dir {
            name.pop();
        }
        if name.starts_with(b"/") {
            name.remove(0);
        }
        entries.push(Entry {
            name,
            is_dir,
            raw: Vec::new(),
            comp: if is_dir { Comp::None } else { comp },
            size,
            crc32: crc,
            mtime,
            flags,
            metadata: meta,
            data: None,
            link: None,
            checked: false,
        });
        clens.push(clen as usize);
    }
    // the contents follow the manifest
    let mut off = m0 + mlen;
    let total: usize = clens.iter().sum();
    let content_end = off + total;
    // the signature
    if a.flags & FLAG_SIGNATURE != 0 {
        if data.len() < content_end + 8 || &data[data.len() - 4..] != b"GBMB" {
            return Err(format!("phar \"{}\" has a broken signature", lossy(fname)));
        }
        let sflags = rd32(data, data.len() - 8);
        let Some(t) = SigType::from_flag(sflags) else {
            return Err(format!(
                "phar \"{}\" has a broken or unsupported signature",
                lossy(fname)
            ));
        };
        let (sig_start, sig) = if t.is_openssl() {
            if data.len() < content_end + 12 {
                return Err(format!(
                    "phar \"{}\" openssl signature length could not be read",
                    lossy(fname)
                ));
            }
            let slen = rd32(data, data.len() - 12) as usize;
            if data.len() < content_end + 12 + slen {
                return Err(format!(
                    "phar \"{}\" openssl signature could not be read",
                    lossy(fname)
                ));
            }
            let s0 = data.len() - 12 - slen;
            (s0, data[s0..s0 + slen].to_vec())
        } else {
            let n = t.digest_len().unwrap();
            if data.len() < content_end + 8 + n {
                return Err(format!("phar \"{}\" has a broken signature", lossy(fname)));
            }
            let s0 = data.len() - 8 - n;
            (s0, data[s0..s0 + n].to_vec())
        };
        let ok = sig::check(t, &data[..sig_start], &sig, opts.verify.as_deref())?;
        if !ok {
            return Err(format!("phar \"{}\" has a broken signature", lossy(fname)));
        }
        a.sig = Some((t, sig));
        a.sig_type = Some(t);
    } else if opts.require_hash {
        return Err(format!(
            "phar \"{}\" does not have a signature",
            lossy(fname)
        ));
    } else {
        a.sig_type = if opts.is_data {
            None
        } else {
            Some(SigType::Sha256)
        };
    }
    for (mut e, clen) in entries.into_iter().zip(clens) {
        let end = (off + clen).min(data.len());
        e.raw = if off < data.len() {
            data[off..end].to_vec()
        } else {
            Vec::new()
        };
        off += clen;
        a.put(e);
    }
    Ok(a)
}

/// Serializes a phar-format archive.
pub fn write(a: &mut Archive, sign: &sig::Signer) -> Result<Vec<u8>, String> {
    let mut out = a.stub.clone();
    let mut entries = Vec::new();
    let mut gflags = 0u32;
    let mut contents: Vec<Vec<u8>> = Vec::new();
    let fname = a.fname.clone();
    for e in a.entries_mut() {
        let mut name = e.name.clone();
        if e.is_dir {
            name.push(b'/');
        }
        // an entry not read yet keeps its stored bytes
        let raw = e.raw.clone();
        let _ = &fname;
        entries.extend_from_slice(&(name.len() as u32).to_le_bytes());
        entries.extend_from_slice(&name);
        entries.extend_from_slice(&e.size.to_le_bytes());
        entries.extend_from_slice(&e.mtime.to_le_bytes());
        entries.extend_from_slice(&(raw.len() as u32).to_le_bytes());
        entries.extend_from_slice(&e.crc32.to_le_bytes());
        let flags = if e.is_dir {
            e.flags & 0o777
        } else {
            (e.flags & !crate::FLAG_COMP_MASK) | e.comp.flag()
        };
        gflags |= flags & crate::FLAG_COMP_MASK;
        entries.extend_from_slice(&flags.to_le_bytes());
        let meta = e.metadata.as_deref().unwrap_or(b"");
        entries.extend_from_slice(&(meta.len() as u32).to_le_bytes());
        entries.extend_from_slice(meta);
        contents.push(raw);
    }
    if a.sig_type.is_some() {
        gflags |= FLAG_SIGNATURE;
    }
    let alias = a.alias.clone().unwrap_or_default();
    let meta = a.metadata.clone().unwrap_or_default();
    let mut m = Vec::new();
    m.extend_from_slice(&(a.entries().len() as u32).to_le_bytes());
    m.extend_from_slice(&0x1110u16.to_be_bytes());
    m.extend_from_slice(&gflags.to_le_bytes());
    m.extend_from_slice(&(alias.len() as u32).to_le_bytes());
    m.extend_from_slice(&alias);
    m.extend_from_slice(&(meta.len() as u32).to_le_bytes());
    m.extend_from_slice(&meta);
    m.extend_from_slice(&entries);
    out.extend_from_slice(&(m.len() as u32).to_le_bytes());
    out.extend_from_slice(&m);
    for c in contents {
        out.extend_from_slice(&c);
    }
    a.flags = gflags;
    a.api = 0x1110;
    if let Some(t) = a.sig_type {
        let s = sig::make(t, &out, sign)?;
        out.extend_from_slice(&sig::trailer(t, &s));
        a.sig = Some((t, s));
    } else {
        a.sig = None;
    }
    Ok(out)
}
