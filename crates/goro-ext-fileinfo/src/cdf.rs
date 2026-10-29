//! Compound File Binary format ([MS-CFB]: "OLE 2" / CDF V2) directory
//! walking, to tell Office 97-2003 documents apart by their stream names
//! (PowerPoint Document, Workbook/Book, WordDocument).
//!
//! The descriptions, mime types, Apple codes and extensions are the ones
//! PHP 8.5's finfo reports (tests ext/fileinfo/tests/finfo_file_002.phpt,
//! finfo_apple_flag.phpt, bug79756.phpt and observation).

use crate::eval::Found;

const SIG: &[u8] = b"\xd0\xcf\x11\xe0\xa1\xb1\x1a\xe1";
const FREESECT: u32 = 0xffff_ffff;
const ENDOFCHAIN: u32 = 0xffff_fffe;

fn u16le(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?))
}
fn u32le(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

/// The names of the directory entries, or None if `b` is not a readable
/// compound file.
pub fn stream_names(b: &[u8]) -> Option<Vec<String>> {
    if !b.starts_with(SIG) || b.len() < 512 {
        return None;
    }
    if u16le(b, 28)? != 0xfffe {
        return None;
    }
    let shift = u16le(b, 30)? as u32;
    if !(7..=16).contains(&shift) {
        return None;
    }
    let ssz = 1usize << shift;
    let nfat = u32le(b, 44)? as usize;
    let first_dir = u32le(b, 48)?;
    let first_difat = u32le(b, 68)?;
    let sector = |n: u32| -> Option<&[u8]> {
        let st = (n as usize).checked_add(1)?.checked_mul(ssz)?;
        b.get(st..st + ssz)
    };
    // The FAT sectors, from the header's DIFAT and the DIFAT chain.
    let mut fat_secs = Vec::new();
    for k in 0..109 {
        let s = u32le(b, 76 + 4 * k)?;
        if s != FREESECT && s != ENDOFCHAIN {
            fat_secs.push(s);
        }
    }
    let mut d = first_difat;
    let mut guard = 0;
    while d != ENDOFCHAIN && d != FREESECT && guard < 1000 && fat_secs.len() < nfat {
        guard += 1;
        let s = sector(d)?;
        for k in 0..ssz / 4 - 1 {
            let v = u32le(s, 4 * k)?;
            if v != FREESECT && v != ENDOFCHAIN {
                fat_secs.push(v);
            }
        }
        d = u32le(s, ssz - 4)?;
    }
    let mut fat: Vec<u32> = Vec::new();
    for &fs in &fat_secs {
        let Some(s) = sector(fs) else { break };
        for k in 0..ssz / 4 {
            fat.push(u32le(s, 4 * k)?);
        }
    }
    let mut names = Vec::new();
    let mut cur = first_dir;
    let mut seen = 0;
    while cur != ENDOFCHAIN && cur != FREESECT {
        seen += 1;
        if seen > 10_000 {
            break;
        }
        let s = sector(cur)?;
        for e in s.chunks_exact(128) {
            let nl = u16le(e, 64)? as usize;
            if nl < 2 || nl > 64 || e[66] == 0 {
                continue;
            }
            let units: Vec<u16> = e[..nl - 2]
                .chunks_exact(2)
                .map(|c| u16::from_le_bytes([c[0], c[1]]))
                .collect();
            names.push(String::from_utf16_lossy(&units));
        }
        cur = *fat.get(cur as usize)?;
    }
    (!names.is_empty()).then_some(names)
}

pub fn detect(b: &[u8]) -> Option<Found> {
    let names = stream_names(b)?;
    let has = |n: &str| names.iter().any(|x| x == n);
    let mut f = Found {
        desc: b"Composite Document File V2 Document, Little Endian".to_vec(),
        ..Found::default()
    };
    let (mime, apple, ext): (&str, Option<&str>, Option<&str>) = if has("PowerPoint Document") {
        (
            "application/vnd.ms-powerpoint",
            Some("????PPT3"),
            Some("ppt/pps/pot"),
        )
    } else if has("Workbook") || has("Book") {
        (
            "application/vnd.ms-excel",
            Some("????XLS5"),
            Some("xls/xla/xlt/"),
        )
    } else if has("WordDocument") {
        ("application/msword", Some("MSWDWDBN"), Some("doc/dot"))
    } else if has("VisioDocument") {
        ("application/vnd.visio", None, Some("vsd"))
    } else {
        ("application/CDFV2", None, None)
    };
    f.mime = Some(mime.as_bytes().to_vec());
    f.apple = apple.map(|s| s.as_bytes().to_vec());
    f.ext = ext.map(|s| s.as_bytes().to_vec());
    Some(f)
}
