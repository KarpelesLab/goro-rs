//! Tar archives (POSIX ustar headers, 512-byte blocks) as phar uses them
//! (PHP manual: Phar, "tar-based phars"): the files, plus `.phar/stub.php`,
//! `.phar/alias.txt`, `.phar/.metadata.bin`,
//! `.phar/.metadata/<file>/.metadata.bin` and `.phar/signature.bin` (a
//! signature of the bytes before it). Messages from ext/phar/tests/tar and
//! the layout of PHP 8.5's output.

use crate::sig::{self, SigType};
use crate::{Archive, Comp, Entry, Format, ParseOptions, lossy, rd32};

/// The tar-based phar stub PHP writes when there is no other.
pub const DEFAULT_STUB: &[u8] = b"<?php // tar-based phar archive stub file\n__HALT_COMPILER();";

fn octal(f: &[u8]) -> Option<u64> {
    let s: Vec<u8> = f
        .iter()
        .copied()
        .skip_while(|&c| c == b' ')
        .take_while(|&c| c != 0 && c != b' ')
        .collect();
    if s.is_empty() {
        return Some(0);
    }
    let mut n = 0u64;
    for c in s {
        if !(b'0'..=b'7').contains(&c) {
            return None;
        }
        n = n.checked_mul(8)?.checked_add((c - b'0') as u64)?;
    }
    Some(n)
}

fn checksum(h: &[u8]) -> u64 {
    h.iter()
        .enumerate()
        .map(|(i, &b)| {
            if (148..156).contains(&i) {
                32
            } else {
                b as u64
            }
        })
        .sum()
}

fn field(f: &[u8]) -> &[u8] {
    match f.iter().position(|&c| c == 0) {
        Some(p) => &f[..p],
        None => f,
    }
}

/// Whether the data starts with a tar header (ustar magic, or a valid
/// checksum).
pub fn looks_like_tar(data: &[u8]) -> bool {
    if data.len() < 512 {
        return false;
    }
    let h = &data[..512];
    if &h[257..262] == b"ustar" {
        return true;
    }
    match octal(&h[148..156]) {
        Some(c) => c != 0 && c == checksum(h),
        None => false,
    }
}

fn corrupt(fname: &[u8], what: &str) -> String {
    format!(
        "phar error: \"{}\" is a corrupted tar file ({what})",
        lossy(fname)
    )
}

/// Parses a tar file. Errors carry PHP's "phar error: ..." text.
pub fn parse(fname: &[u8], data: &[u8], opts: &ParseOptions) -> Result<Archive, String> {
    let mut a = Archive::new(fname, Format::Tar, opts.is_data);
    a.sig_type = None;
    let mut p = 0usize;
    let mut long_name: Option<Vec<u8>> = None;
    let mut sig_bin: Option<(usize, Vec<u8>)> = None;
    let mut metas: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
    let mut links: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
    let mut ended = false;
    while p + 512 <= data.len() {
        let h = &data[p..p + 512];
        if h.iter().all(|&c| c == 0) {
            ended = true;
            break;
        }
        let stored = octal(&h[148..156]);
        let mut name = field(&h[..100]).to_vec();
        if &h[257..262] == b"ustar" {
            let prefix = field(&h[345..500]);
            if !prefix.is_empty() {
                let mut n = prefix.to_vec();
                n.push(b'/');
                n.extend_from_slice(&name);
                name = n;
            }
        }
        if stored != Some(checksum(h)) {
            return Err(corrupt(
                fname,
                &format!("checksum mismatch of file \"{}\"", lossy(&name)),
            ));
        }
        let Some(size) = octal(&h[124..136]) else {
            return Err(corrupt(fname, "invalid entry size"));
        };
        let size = size as usize;
        let ty = h[156];
        let start = p + 512;
        let padded = size.div_ceil(512) * 512;
        if start + padded > data.len() {
            return Err(corrupt(fname, "truncated"));
        }
        let body = data[start..start + size].to_vec();
        let hstart = p;
        p = start + padded;
        if ty == b'L' {
            let mut n = body;
            while n.last() == Some(&0) {
                n.pop();
            }
            long_name = Some(n);
            continue;
        }
        if let Some(n) = long_name.take() {
            name = n;
        }
        if name.starts_with(b"./") {
            name.drain(..2);
        }
        while name.starts_with(b"/") {
            name.remove(0);
        }
        let is_dir = ty == b'5' || name.ends_with(b"/");
        while name.ends_with(b"/") {
            name.pop();
        }
        let mode = octal(&h[100..108]).unwrap_or(0o644) as u32;
        let mtime = octal(&h[136..148]).unwrap_or(0) as u32;
        // phar's own entries
        match &name[..] {
            b".phar/stub.php" => {
                a.stub = body;
                continue;
            }
            b".phar/alias.txt" => {
                if body.len() > 511 {
                    return Err(format!(
                        "phar error: tar-based phar \"{}\" has alias that is larger than 511 bytes, cannot process",
                        lossy(fname)
                    ));
                }
                a.alias = Some(body);
                continue;
            }
            b".phar/.metadata.bin" => {
                a.metadata = Some(body);
                continue;
            }
            b".phar/signature.bin" => {
                sig_bin = Some((hstart, body));
                continue;
            }
            _ => {}
        }
        if let Some(rest) = name.strip_prefix(b".phar/.metadata/".as_slice())
            && let Some(file) = rest.strip_suffix(b"/.metadata.bin".as_slice())
        {
            metas.push((file.to_vec(), body));
            continue;
        }
        let link = field(&h[157..257]).to_vec();
        let mut e = if is_dir {
            Entry::dir(&name, mtime)
        } else {
            Entry::file(&name, body, mtime, mode)
        };
        e.flags = mode & 0o777;
        if ty == b'1' || ty == b'2' {
            e.link = Some(link.clone());
            if ty == b'1' {
                links.push((name.clone(), link));
            }
        }
        a.put(e);
    }
    // a header cut short (test tar/tar_001.phpt)
    if !ended && p < data.len() && data[p..].iter().any(|&c| c != 0) {
        return Err(corrupt(fname, "truncated"));
    }
    // symbolic links read as their target (test tar/links4.phpt)
    let syms: Vec<(Vec<u8>, Vec<u8>)> = a
        .entries()
        .iter()
        .filter(|e| e.link.is_some() && !links.iter().any(|(n, _)| n == &e.name))
        .map(|e| (e.name.clone(), e.link.clone().unwrap()))
        .collect();
    for (name, target) in syms {
        let t = if target.starts_with(b"/") {
            target.clone()
        } else {
            let mut d = match name.iter().rposition(|&c| c == b'/') {
                Some(p) => name[..=p].to_vec(),
                None => Vec::new(),
            };
            d.extend_from_slice(&target);
            d
        };
        let t = norm(&t);
        if let Some(src) = a.get(&t).map(|e| e.raw.clone())
            && let Some(e) = a.get_mut(&name)
        {
            e.set_contents(src);
        }
    }
    // hard links take their target's contents
    for (name, target) in links {
        let mut t = target.clone();
        while t.starts_with(b"/") {
            t.remove(0);
        }
        let Some(src) = a.get(&t).map(|e| e.raw.clone()) else {
            return Err(format!(
                "phar error: \"{}\" is a corrupted tar file - hard link to non-existent file \"{}\"",
                lossy(fname),
                lossy(&target)
            ));
        };
        if let Some(e) = a.get_mut(&name) {
            e.set_contents(src);
        }
    }
    for (file, meta) in metas {
        let f = file.strip_suffix(b"/").unwrap_or(&file).to_vec();
        if let Some(e) = a.get_mut(&f) {
            e.metadata = Some(meta);
        }
    }
    match sig_bin {
        Some((at, b)) => {
            if b.len() < 8 {
                return Err(format!(
                    "phar error: tar-based phar \"{}\" signature cannot be read",
                    lossy(fname)
                ));
            }
            let flags = rd32(&b, 0);
            let len = rd32(&b, 4) as usize;
            let Some(t) = SigType::from_flag(flags) else {
                return Err(format!(
                    "phar error: tar-based phar \"{}\" has an unsupported signature",
                    lossy(fname)
                ));
            };
            if b.len() < 8 + len {
                return Err(format!(
                    "phar error: tar-based phar \"{}\" signature cannot be read",
                    lossy(fname)
                ));
            }
            let s = b[8..8 + len].to_vec();
            if !sig::check(t, &data[..at], &s, opts.verify.as_deref())? {
                return Err(format!(
                    "phar error: tar-based phar \"{}\" has a broken signature",
                    lossy(fname)
                ));
            }
            a.sig = Some((t, s));
            a.sig_type = Some(t);
        }
        None => {
            if opts.require_hash && !opts.is_data {
                return Err(format!(
                    "tar-based phar \"{}\" does not have a signature",
                    lossy(fname)
                ));
            }
            a.sig_type = if opts.is_data {
                None
            } else {
                Some(SigType::Sha256)
            };
        }
    }
    // a tar opened as a phar keeps a stub
    if a.stub.is_empty() && !opts.is_data {
        a.stub = DEFAULT_STUB.to_vec();
    }
    Ok(a)
}

/// A path with `.` and `..` resolved, without a leading slash.
fn norm(p: &[u8]) -> Vec<u8> {
    let mut parts: Vec<&[u8]> = Vec::new();
    for seg in p.split(|&c| c == b'/') {
        match seg {
            b"" | b"." => {}
            b".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    parts.join(&b'/')
}

/// A ustar header.
fn header(
    name: &[u8],
    mode: u32,
    size: usize,
    mtime: u32,
    ty: u8,
    link: &[u8],
) -> Option<[u8; 512]> {
    let mut h = [0u8; 512];
    if name.len() <= 100 {
        h[..name.len()].copy_from_slice(name);
    } else {
        // split at a slash into prefix (155) and name (100)
        let mut split = None;
        for (i, &c) in name.iter().enumerate() {
            if c == b'/' && i <= 155 && name.len() - i - 1 <= 100 && name.len() - i - 1 > 0 {
                split = Some(i);
                break;
            }
        }
        let i = split?;
        h[345..345 + i].copy_from_slice(&name[..i]);
        let rest = &name[i + 1..];
        h[..rest.len()].copy_from_slice(rest);
    }
    let put = |h: &mut [u8; 512], at: usize, s: String| {
        h[at..at + s.len()].copy_from_slice(s.as_bytes());
    };
    put(&mut h, 100, format!("{:07o}", mode & 0o7777));
    put(&mut h, 124, format!("{size:011o}"));
    put(&mut h, 136, format!("{mtime:011o}"));
    h[156] = ty;
    let l = link.len().min(100);
    h[157..157 + l].copy_from_slice(&link[..l]);
    h[257..263].copy_from_slice(b"ustar\0");
    h[263..265].copy_from_slice(b"00");
    let sum = checksum(&h);
    put(&mut h, 148, format!("{sum:07o}"));
    h[155] = b' ';
    Some(h)
}

#[allow(clippy::too_many_arguments)]
fn push(
    out: &mut Vec<u8>,
    fname: &[u8],
    name: &[u8],
    mode: u32,
    mtime: u32,
    ty: u8,
    data: &[u8],
    link: &[u8],
) -> Result<(), String> {
    let Some(h) = header(name, mode, data.len(), mtime, ty, link) else {
        return Err(format!(
            "tar-based phar \"{}\" cannot be created, filename \"{}\" is too long for tar file format",
            lossy(fname),
            lossy(name)
        ));
    };
    out.extend_from_slice(&h);
    out.extend_from_slice(data);
    let pad = data.len().div_ceil(512) * 512 - data.len();
    out.extend(std::iter::repeat_n(0u8, pad));
    Ok(())
}

/// Serializes a tar-format archive (PHP 8.5's layout).
pub fn write(a: &mut Archive, now: u32, sign: &sig::Signer) -> Result<Vec<u8>, String> {
    let fname = a.fname.clone();
    let mut out = Vec::new();
    let mut metas: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
    for e in a.entries_mut() {
        if e.is_dir {
            push(
                &mut out,
                &fname,
                &e.name,
                e.perms(),
                e.mtime,
                b'5',
                b"",
                b"",
            )?;
        } else {
            let data = e.contents(&fname)?.to_vec();
            let ty = match &e.link {
                Some(_) => b'2',
                None => b'0',
            };
            let link = e.link.clone().unwrap_or_default();
            if ty == b'2' {
                push(
                    &mut out,
                    &fname,
                    &e.name,
                    e.perms(),
                    e.mtime,
                    ty,
                    b"",
                    &link,
                )?;
            } else {
                push(
                    &mut out,
                    &fname,
                    &e.name,
                    e.perms(),
                    e.mtime,
                    ty,
                    &data,
                    b"",
                )?;
            }
        }
        if let Some(m) = &e.metadata {
            metas.push((e.name.clone(), m.clone()));
        }
        // tar entries are stored uncompressed
        e.comp = Comp::None;
    }
    if !a.is_data {
        let stub = if a.stub.is_empty() {
            DEFAULT_STUB.to_vec()
        } else {
            a.stub.clone()
        };
        push(
            &mut out,
            &fname,
            b".phar/stub.php",
            0o666,
            now,
            b'0',
            &stub,
            b"",
        )?;
        if let Some(al) = a.alias.clone() {
            push(
                &mut out,
                &fname,
                b".phar/alias.txt",
                0o666,
                now,
                b'0',
                &al,
                b"",
            )?;
        }
    }
    if let Some(m) = a.metadata.clone() {
        push(
            &mut out,
            &fname,
            b".phar/.metadata.bin",
            0,
            0,
            b'0',
            &m,
            b"",
        )?;
    }
    for (n, m) in metas {
        let mut path = b".phar/.metadata/".to_vec();
        path.extend_from_slice(&n);
        path.extend_from_slice(b"/.metadata.bin");
        push(&mut out, &fname, &path, 0, 0, b'0', &m, b"")?;
    }
    match a.sig_type {
        Some(t) => {
            let s = sig::make(t, &out, sign)?;
            let b = sig::bin(t, &s);
            push(
                &mut out,
                &fname,
                b".phar/signature.bin",
                0o666,
                now,
                b'0',
                &b,
                b"",
            )?;
            a.sig = Some((t, s));
        }
        _ => a.sig = None,
    }
    out.extend(std::iter::repeat_n(0u8, 1024));
    Ok(out)
}
