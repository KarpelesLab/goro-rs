//! Zip archives as phar reads and writes them (PKWARE APPNOTE.TXT: local
//! headers, central directory, end of central directory record; the ASi
//! Unix extra field 0x756e for permissions). Phar's entries are
//! `.phar/stub.php`, `.phar/alias.txt` and `.phar/signature.bin`; the
//! archive metadata is the zip comment, an entry's its file comment.
//! Messages from ext/phar/tests/zip, the layout from PHP 8.5's output.

use crate::sig::{self, SigType};
use crate::{Archive, Comp, Entry, Format, ParseOptions, lossy, rd16, rd32};

/// The zip-based phar stub PHP writes when there is no other.
pub const DEFAULT_STUB: &[u8] = b"<?php // zip-based phar archive stub file\n__HALT_COMPILER();";

/// Whether the data is a zip (a local header first, or an end record).
pub fn looks_like_zip(data: &[u8]) -> bool {
    data.starts_with(b"PK\x03\x04") || (data.starts_with(b"PK\x05\x06") && data.len() >= 22)
}

fn err(fname: &[u8], what: &str) -> String {
    format!("phar error: {what} in zip-based phar \"{}\"", lossy(fname))
}

/// The name of an unsupported compression method (as PHP's message).
fn method_name(m: u16) -> &'static str {
    match m {
        1 => "Shrunk",
        2..=5 => "Reduce",
        6 => "Implode",
        7 => "Tokenize",
        9 => "Deflate64",
        10 => "PKWare Implode/old IBM TERSE",
        14 => "LZMA",
        18 => "IBM TERSE",
        19 => "IBM LZ77",
        97 => "WavPack",
        98 => "PPMd",
        _ => "unknown",
    }
}

/// DOS date and time to a Unix time (UTC).
pub fn dos_to_unix(time: u16, date: u16) -> u32 {
    let year = 1980 + (date >> 9) as i64;
    let month = ((date >> 5) & 15).clamp(1, 12) as i64;
    let day = (date & 31).max(1) as i64;
    let h = (time >> 11) as i64;
    let mi = ((time >> 5) & 63) as i64;
    let s = ((time & 31) * 2) as i64;
    // days from civil (proleptic Gregorian)
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    (days * 86400 + h * 3600 + mi * 60 + s).max(0) as u32
}

/// A Unix time (UTC) to DOS (time, date); 1980-01-01 at the earliest.
pub fn unix_to_dos(t: u32) -> (u16, u16) {
    let t = t as i64;
    let days = t.div_euclid(86400);
    let secs = t.rem_euclid(86400);
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    if y < 1980 {
        return (0, 0x21);
    }
    let time = (((secs / 3600) << 11) | (((secs / 60) % 60) << 5) | ((secs % 60) / 2)) as u16;
    let date = (((y - 1980) << 9) | (m << 5) | d) as u16;
    (time, date)
}

/// Parses a zip file.
pub fn parse(fname: &[u8], data: &[u8], opts: &ParseOptions) -> Result<Archive, String> {
    let mut a = Archive::new(fname, Format::Zip, opts.is_data);
    a.sig_type = None;
    // the end of central directory record, searched backwards over the
    // longest comment
    let lo = data.len().saturating_sub(22 + 65535);
    let mut eocd = None;
    if data.len() >= 22 {
        let mut p = data.len() - 22;
        loop {
            if &data[p..p + 4] == b"PK\x05\x06" {
                let clen = rd16(data, p + 20) as usize;
                if p + 22 + clen <= data.len() {
                    eocd = Some(p);
                }
                break;
            }
            if p == lo {
                break;
            }
            p -= 1;
        }
    }
    let Some(e) = eocd else {
        return Err(err(fname, "end of central directory not found"));
    };
    if rd16(data, e + 4) != 0 || rd16(data, e + 6) != 0 {
        return Err(err(
            fname,
            "split archives spanning multiple zips cannot be processed",
        ));
    }
    let count = rd16(data, e + 8) as usize;
    if count != rd16(data, e + 10) as usize {
        return Err(err(
            fname,
            "corrupt zip archive, conflicting file count in end of central directory record",
        ));
    }
    let clen = rd16(data, e + 20) as usize;
    if clen > 0 {
        a.metadata = Some(data[e + 22..e + 22 + clen].to_vec());
        a.comment = data[e + 22..e + 22 + clen].to_vec();
    }
    let mut p = rd32(data, e + 16) as usize;
    let mut sig_bin: Option<(usize, Vec<u8>)> = None;
    // the central entries other than the signature's: signed with the
    // local part and the comment (observed on PHP 8.5)
    let mut signed_cd: Vec<u8> = Vec::new();
    for _ in 0..count {
        let cd_start = p;
        if p + 46 > data.len() || &data[p..p + 4] != b"PK\x01\x02" {
            return Err(err(
                fname,
                "corrupted central directory entry, no magic signature",
            ));
        }
        let flags = rd16(data, p + 8);
        let method = rd16(data, p + 10);
        let time = rd16(data, p + 12);
        let date = rd16(data, p + 14);
        let crc = rd32(data, p + 16);
        let csize = rd32(data, p + 20) as usize;
        let usize_ = rd32(data, p + 24);
        let nlen = rd16(data, p + 28) as usize;
        let xlen = rd16(data, p + 30) as usize;
        let mlen = rd16(data, p + 32) as usize;
        let ext_attr = rd32(data, p + 38);
        let off = rd32(data, p + 42) as usize;
        if nlen == 0 {
            return Err(err(
                fname,
                "Cannot process zips created from stdin (zero-length filename)",
            ));
        }
        if p + 46 + nlen > data.len() {
            return Err(err(
                fname,
                "corrupted central directory entry, no magic signature",
            ));
        }
        let mut name = data[p + 46..p + 46 + nlen].to_vec();
        let x0 = p + 46 + nlen;
        if x0 + xlen > data.len() {
            return Err(err(
                fname,
                "Unable to process extra field header for file in central directory",
            ));
        }
        // the extra fields: the ASi Unix one gives the permissions
        let mut perms: Option<u32> = None;
        let mut q = x0;
        while q + 4 <= x0 + xlen {
            let id = rd16(data, q);
            let len = rd16(data, q + 2) as usize;
            if q + 4 + len > x0 + xlen {
                return Err(err(
                    fname,
                    "Unable to process extra field header for file in central directory",
                ));
            }
            if id == 0x756e && len >= 6 {
                perms = Some(rd16(data, q + 8) as u32 & 0o777);
            }
            q += 4 + len;
        }
        let c0 = x0 + xlen;
        let comment = data[c0.min(data.len())..(c0 + mlen).min(data.len())].to_vec();
        p = c0 + mlen;
        if name != b".phar/signature.bin" {
            signed_cd.extend_from_slice(&data[cd_start..p.min(data.len())]);
        }
        if flags & 1 != 0 {
            return Err(err(fname, "Cannot process encrypted zip files"));
        }
        let comp = match method {
            0 => Comp::None,
            8 => Comp::Gz,
            12 => Comp::Bz2,
            m => {
                return Err(err(
                    fname,
                    &format!(
                        "unsupported compression method ({}) used in this zip",
                        method_name(m)
                    ),
                ));
            }
        };
        // the data, after the local header
        if off + 30 > data.len() || &data[off..off + 4] != b"PK\x03\x04" {
            return Err(err(
                fname,
                "corrupted central directory entry, no magic signature",
            ));
        }
        let lnlen = rd16(data, off + 26) as usize;
        let lxlen = rd16(data, off + 28) as usize;
        let d0 = off + 30 + lnlen + lxlen;
        let raw = data[d0.min(data.len())..(d0 + csize).min(data.len())].to_vec();
        if name.starts_with(b"/") {
            name.remove(0);
        }
        let is_dir = name.ends_with(b"/");
        if is_dir {
            name.pop();
        }
        match &name[..] {
            b".phar/stub.php" | b".phar/alias.txt" | b".phar/signature.bin" => {
                let mut e = Entry::file(&name, Vec::new(), 0, 0o644);
                e.raw = raw;
                e.comp = comp;
                e.size = usize_;
                e.crc32 = crc;
                e.data = None;
                let body = e
                    .contents(fname)
                    .map_err(|m| format!("phar error: {m}"))?
                    .to_vec();
                match &name[..] {
                    b".phar/stub.php" => a.stub = body,
                    b".phar/alias.txt" => a.alias = Some(body),
                    _ => sig_bin = Some((off, body)),
                }
                continue;
            }
            _ => {}
        }
        if name == b".phar" || name.starts_with(b".phar/") {
            continue;
        }
        let mtime = dos_to_unix(time, date);
        let mut e = if is_dir {
            Entry::dir(&name, mtime)
        } else {
            let mut e = Entry::file(&name, Vec::new(), mtime, 0o644);
            e.raw = raw;
            e.comp = comp;
            e.size = usize_;
            e.crc32 = crc;
            e.data = None;
            e.checked = false;
            e
        };
        let perm = perms.unwrap_or_else(|| {
            let m = (ext_attr >> 16) & 0o777;
            if m != 0 {
                m
            } else if is_dir {
                0o777
            } else {
                0o666
            }
        });
        e.flags = perm | e.comp.flag();
        if !comment.is_empty() {
            e.metadata = Some(comment);
        }
        a.put(e);
    }
    match sig_bin {
        Some((at, b)) => {
            if b.len() < 8 {
                return Err(err(fname, "signature cannot be read"));
            }
            let flags = rd32(&b, 0);
            let len = rd32(&b, 4) as usize;
            let Some(t) = SigType::from_flag(flags) else {
                return Err(err(fname, "signature cannot be read"));
            };
            if b.len() < 8 + len {
                return Err(err(fname, "signature cannot be read"));
            }
            let s = b[8..8 + len].to_vec();
            let mut signed = data[..at].to_vec();
            signed.extend_from_slice(&signed_cd);
            signed.extend_from_slice(&a.comment);
            if !sig::check(t, &signed, &s, opts.verify.as_deref())? {
                return Err(err(fname, "signature cannot be verified"));
            }
            a.sig = Some((t, s));
            a.sig_type = Some(t);
        }
        None => {
            if opts.require_hash && !opts.is_data {
                return Err(err(fname, "signature is missing"));
            }
            a.sig_type = if opts.is_data {
                None
            } else {
                Some(SigType::Sha256)
            };
        }
    }
    if a.stub.is_empty() && !opts.is_data {
        a.stub = DEFAULT_STUB.to_vec();
    }
    Ok(a)
}

/// The ASi Unix extra field (0x756e) with the permissions; PHP's CRC
/// covers the mode only (observed on PHP 8.5: 845bb5fc for 0644).
fn asi_extra(perms: u32) -> Vec<u8> {
    let mode = (perms as u16).to_le_bytes();
    let mut out = vec![0x6e, 0x75, 14, 0];
    out.extend_from_slice(&crate::crc32(&mode).to_le_bytes());
    out.extend_from_slice(&mode);
    out.extend_from_slice(&[0u8; 8]);
    out
}

struct Item {
    name: Vec<u8>,
    raw: Vec<u8>,
    method: u16,
    crc: u32,
    size: u32,
    time: u16,
    date: u16,
    perms: u32,
    comment: Vec<u8>,
    offset: u32,
}

fn local(out: &mut Vec<u8>, it: &mut Item) {
    it.offset = out.len() as u32;
    let extra = asi_extra(it.perms);
    out.extend_from_slice(b"PK\x03\x04");
    out.extend_from_slice(&version(it.method).to_le_bytes());
    out.extend_from_slice(&0x0800u16.to_le_bytes());
    out.extend_from_slice(&it.method.to_le_bytes());
    out.extend_from_slice(&it.time.to_le_bytes());
    out.extend_from_slice(&it.date.to_le_bytes());
    out.extend_from_slice(&it.crc.to_le_bytes());
    out.extend_from_slice(&(it.raw.len() as u32).to_le_bytes());
    out.extend_from_slice(&it.size.to_le_bytes());
    out.extend_from_slice(&(it.name.len() as u16).to_le_bytes());
    out.extend_from_slice(&(extra.len() as u16).to_le_bytes());
    out.extend_from_slice(&it.name);
    out.extend_from_slice(&extra);
    out.extend_from_slice(&it.raw);
}

fn version(method: u16) -> u16 {
    match method {
        8 => 20,
        12 => 46,
        _ => 0,
    }
}

fn item(
    name: &[u8],
    raw: Vec<u8>,
    method: u16,
    crc: u32,
    size: u32,
    mtime: u32,
    perms: u32,
) -> Item {
    let (time, date) = unix_to_dos(mtime);
    Item {
        name: name.to_vec(),
        raw,
        method,
        crc,
        size,
        time,
        date,
        perms,
        comment: Vec::new(),
        offset: 0,
    }
}

/// Serializes a zip-format archive (PHP 8.5's layout).
pub fn write(a: &mut Archive, now: u32, sign: &sig::Signer) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let mut items: Vec<Item> = Vec::new();
    for e in a.entries_mut() {
        let mut it = if e.is_dir {
            let mut n = e.name.clone();
            n.push(b'/');
            item(&n, Vec::new(), 0, 0, 0, e.mtime, e.perms())
        } else {
            let method = match e.comp {
                Comp::None => 0,
                Comp::Gz => 8,
                Comp::Bz2 => 12,
            };
            item(
                &e.name,
                e.raw.clone(),
                method,
                e.crc32,
                e.size,
                e.mtime,
                e.perms(),
            )
        };
        it.comment = e.metadata.clone().unwrap_or_default();
        local(&mut out, &mut it);
        items.push(it);
    }
    if !a.is_data {
        let stub = if a.stub.is_empty() {
            DEFAULT_STUB.to_vec()
        } else {
            a.stub.clone()
        };
        // modes and times as PHP 8.5 writes them
        let mut it = item(
            b".phar/stub.php",
            stub.clone(),
            0,
            crate::crc32(&stub),
            stub.len() as u32,
            now,
            0o666,
        );
        local(&mut out, &mut it);
        items.push(it);
        if let Some(al) = a.alias.clone() {
            let mut it = item(
                b".phar/alias.txt",
                al.clone(),
                0,
                crate::crc32(&al),
                al.len() as u32,
                now,
                0o666,
            );
            local(&mut out, &mut it);
            items.push(it);
        }
    }
    let comment = a.metadata.clone().unwrap_or_default();
    match a.sig_type {
        Some(t) => {
            let mut signed = out.clone();
            for it in &items {
                central(&mut signed, it);
            }
            signed.extend_from_slice(&comment);
            let s = sig::make(t, &signed, sign)?;
            let b = sig::bin(t, &s);
            let mut it = item(
                b".phar/signature.bin",
                b.clone(),
                0,
                crate::crc32(&b),
                b.len() as u32,
                0,
                0,
            );
            local(&mut out, &mut it);
            items.push(it);
            a.sig = Some((t, s));
        }
        _ => a.sig = None,
    }
    let cd0 = out.len();
    for it in &items {
        central(&mut out, it);
    }
    let cd_len = out.len() - cd0;
    out.extend_from_slice(b"PK\x05\x06");
    out.extend_from_slice(&[0, 0, 0, 0]);
    out.extend_from_slice(&(items.len() as u16).to_le_bytes());
    out.extend_from_slice(&(items.len() as u16).to_le_bytes());
    out.extend_from_slice(&(cd_len as u32).to_le_bytes());
    out.extend_from_slice(&(cd0 as u32).to_le_bytes());
    out.extend_from_slice(&(comment.len() as u16).to_le_bytes());
    out.extend_from_slice(&comment);
    Ok(out)
}

/// A central directory entry.
fn central(out: &mut Vec<u8>, it: &Item) {
    {
        let extra = asi_extra(it.perms);
        out.extend_from_slice(b"PK\x01\x02");
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&version(it.method).to_le_bytes());
        out.extend_from_slice(&0x0800u16.to_le_bytes());
        out.extend_from_slice(&it.method.to_le_bytes());
        out.extend_from_slice(&it.time.to_le_bytes());
        out.extend_from_slice(&it.date.to_le_bytes());
        out.extend_from_slice(&it.crc.to_le_bytes());
        out.extend_from_slice(&(it.raw.len() as u32).to_le_bytes());
        out.extend_from_slice(&it.size.to_le_bytes());
        out.extend_from_slice(&(it.name.len() as u16).to_le_bytes());
        out.extend_from_slice(&(extra.len() as u16).to_le_bytes());
        out.extend_from_slice(&(it.comment.len() as u16).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&it.offset.to_le_bytes());
        out.extend_from_slice(&it.name);
        out.extend_from_slice(&extra);
        out.extend_from_slice(&it.comment);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn asi_crc_matches_php() {
        // PHP 8.5 writes 845bb5fc for mode 0644
        assert_eq!(&super::asi_extra(0o644)[4..8], &[0x84, 0x5b, 0xb5, 0xfc]);
    }

    #[test]
    fn dos_time() {
        assert_eq!(super::unix_to_dos(0), (0, 0x21));
        let (t, d) = super::unix_to_dos(1_000_000_000);
        assert_eq!(super::dos_to_unix(t, d), 1_000_000_000);
    }
}
