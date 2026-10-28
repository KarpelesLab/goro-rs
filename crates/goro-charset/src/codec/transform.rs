//! The byte-level "encodings" mbstring still accepts, deprecated since PHP
//! 8.2: BASE64, Quoted-Printable and UUENCODE.
//!
//! Converting to BASE64 or Quoted-Printable encodes the input bytes as they
//! are, whatever the source encoding; converting from any of the three
//! decodes to bytes, whatever the target. Converting to UUENCODE takes the
//! low byte of each decoded character (observed on PHP 8.5).

use super::ERR;

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// BASE64 with CRLF after every 76 characters (test
/// ext/mbstring/tests/base64_encoding.phpt).
pub(crate) fn base64_encode(s: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len() * 4 / 3 + s.len() / 38 + 4);
    let mut col = 0;
    for chunk in s.chunks(3) {
        if col == 76 {
            out.extend_from_slice(b"\r\n");
            col = 0;
        }
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let v = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(B64[(v >> 18) as usize & 63]);
        out.push(B64[(v >> 12) as usize & 63]);
        out.push(if chunk.len() > 1 {
            B64[(v >> 6) as usize & 63]
        } else {
            b'='
        });
        out.push(if chunk.len() > 2 {
            B64[v as usize & 63]
        } else {
            b'='
        });
        col += 4;
    }
    out
}

/// Decodes BASE64: whitespace and `=` are skipped, other non-Base64 bytes
/// are errors, leftover bits at the end are dropped (observed on PHP 8.5).
/// Returns byte values, with [`ERR`] for errors.
pub(crate) fn base64_decode(s: &[u8]) -> Vec<u32> {
    let mut out = Vec::with_capacity(s.len() * 3 / 4 + 3);
    let mut bits = 0u32;
    let mut n = 0;
    for &b in s {
        let v = match b {
            b'A'..=b'Z' => b - b'A',
            b'a'..=b'z' => b - b'a' + 26,
            b'0'..=b'9' => b - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' | b' ' | b'\t' | b'\r' | b'\n' => continue,
            _ => {
                out.push(ERR);
                continue;
            }
        };
        bits = (bits << 6) | v as u32;
        n += 6;
        if n >= 8 {
            n -= 8;
            out.push((bits >> n) & 0xFF);
            bits &= (1 << n) - 1;
        }
    }
    out
}

/// Quoted-Printable: `=` and bytes from 0x80 are escaped, LF becomes CRLF,
/// a lone CR is dropped, and lines are soft-broken with `=` CRLF once they
/// reach 72 characters (test ext/mbstring/tests/qprint_encoding.phpt,
/// observed on PHP 8.5).
pub(crate) fn qprint_encode(s: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len() + s.len() / 8);
    let mut col = 0;
    let mut i = 0;
    while i < s.len() {
        let b = s[i];
        i += 1;
        if b == b'\r' {
            if s.get(i) == Some(&b'\n') {
                i += 1;
                out.extend_from_slice(b"\r\n");
                col = 0;
            }
            continue;
        }
        if b == b'\n' {
            out.extend_from_slice(b"\r\n");
            col = 0;
            continue;
        }
        if b == 0 {
            // NUL is written as it is and restarts the line count
            // (observed on PHP 8.5).
            out.push(0);
            col = 0;
            continue;
        }
        if col >= 72 {
            out.extend_from_slice(b"=\r\n");
            col = 0;
        }
        if b == b'=' || b >= 0x80 {
            out.push(b'=');
            out.push(b"0123456789ABCDEF"[(b >> 4) as usize]);
            out.push(b"0123456789ABCDEF"[(b & 15) as usize]);
            col += 3;
        } else {
            out.push(b);
            col += 1;
        }
    }
    out
}

fn hexval(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'A'..=b'F' => Some(b - b'A' + 10),
        b'a'..=b'f' => Some(b - b'a' + 10),
        _ => None,
    }
}

/// Decodes Quoted-Printable: `=XX` (either case), soft breaks `=` CRLF and
/// `=` LF; any other `=` stays as it is (observed on PHP 8.5).
pub(crate) fn qprint_decode(s: &[u8]) -> Vec<u32> {
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        let b = s[i];
        if b == b'=' {
            if s.get(i + 1) == Some(&b'\r') && s.get(i + 2) == Some(&b'\n') {
                i += 3;
                continue;
            }
            if s.get(i + 1) == Some(&b'\n') {
                i += 2;
                continue;
            }
            if let (Some(h), Some(l)) = (
                s.get(i + 1).and_then(|&c| hexval(c)),
                s.get(i + 2).and_then(|&c| hexval(c)),
            ) {
                out.push(((h << 4) | l) as u32);
                i += 3;
                continue;
            }
        }
        out.push(b as u32);
        i += 1;
    }
    out
}

#[inline]
fn uu_char(v: u8) -> u8 {
    if v == 0 { b'`' } else { v + 0x20 }
}

/// UUENCODE: a "begin 0644 filename" line, then lines of up to 45 bytes
/// (observed on PHP 8.5; no trailing "end" line).
pub(crate) fn uuencode(s: &[u8]) -> Vec<u8> {
    let mut out = b"begin 0644 filename\n".to_vec();
    for line in s.chunks(45) {
        out.push(uu_char(line.len() as u8));
        for g in line.chunks(3) {
            let b = [g[0], *g.get(1).unwrap_or(&0), *g.get(2).unwrap_or(&0)];
            out.push(uu_char(b[0] >> 2));
            out.push(uu_char(((b[0] & 3) << 4) | (b[1] >> 4)));
            out.push(uu_char(((b[1] & 15) << 2) | (b[2] >> 6)));
            out.push(uu_char(b[2] & 63));
        }
        out.push(b'\n');
    }
    out
}

/// Decodes UUENCODE: nothing until a "begin " line; then each line's length
/// character gives the byte count, until a line of length 0 (observed on
/// PHP 8.5).
pub(crate) fn uudecode(s: &[u8]) -> Vec<u32> {
    let mut out = Vec::new();
    let mut lines = s.split(|&b| b == b'\n');
    // Skip to the "begin" line.
    loop {
        match lines.next() {
            None => return out,
            Some(l) if l.starts_with(b"begin ") => break,
            Some(_) => {}
        }
    }
    for line in lines {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        let Some(&first) = line.first() else { continue };
        let len = (first.wrapping_sub(0x20) & 0x3F) as usize;
        if len == 0 {
            break;
        }
        let data = &line[1..];
        let mut produced = 0;
        for g in data.chunks(4) {
            let v: Vec<u8> = (0..4)
                .map(|k| g.get(k).map_or(0, |&c| c.wrapping_sub(0x20) & 0x3F))
                .collect();
            let bytes = [
                (v[0] << 2) | (v[1] >> 4),
                (v[1] << 4) | (v[2] >> 2),
                (v[2] << 6) | v[3],
            ];
            for &b in &bytes {
                if produced < len {
                    out.push(b as u32);
                    produced += 1;
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bytes(v: &[u32]) -> Vec<u8> {
        v.iter().map(|&c| c as u8).collect()
    }

    #[test]
    fn base64() {
        // test ext/mbstring/tests/base64_encoding.phpt
        for (raw, enc) in [
            (&b""[..], &b""[..]),
            (b"a", b"YQ=="),
            (b"ab", b"YWI="),
            (b"\x01\x02\x03", b"AQID"),
            (b"\xFF\xFE\x11\x22", b"//4RIg=="),
            (b"\x00", b"AA=="),
            (b"\x00\x00", b"AAA="),
            (b"\x00\x00\x00", b"AAAA"),
        ] {
            assert_eq!(base64_encode(raw), enc);
            assert_eq!(bytes(&base64_decode(enc)), raw);
        }
        let long = b"ABCDEFGHIJ".repeat(20);
        let enc = base64_encode(&long);
        assert_eq!(&enc[76..78], b"\r\n");
        assert_eq!(bytes(&base64_decode(&enc)), long);
        // observed on PHP 8.5
        let d = base64_decode(b"Q!c=Op/w==AB");
        assert_eq!(d[0], ERR);
        assert_eq!(bytes(&d[1..]), b"\x41\xc3\xa9\xff\x00\x01");
    }

    #[test]
    fn qprint() {
        // test ext/mbstring/tests/qprint_encoding.phpt
        assert_eq!(qprint_encode("あ".as_bytes()), b"=E3=81=82");
        assert_eq!(bytes(&qprint_decode(b"=E3=81=82")), "あ".as_bytes());
        let raw = b"***,*-S,\xac,,\xb7,l,,,,\xb6UTF7,\xb5\xb5\xb5\xb5\xb5\xb5K\xb5\xb5!\xa4\xa4\xa4\xa4\xa4\xbd\xb5,\xb5\xb5\xb5\xb5\xb5\xb5\xb5\xb5\xb5&";
        let enc = b"***,*-S,=AC,,=B7,l,,,,=B6UTF7,=B5=B5=B5=B5=B5=B5K=B5=B5!=A4=A4=A4=A4=A4=BD=\r\n=B5,=B5=B5=B5=B5=B5=B5=B5=B5=B5&";
        assert_eq!(qprint_encode(raw), enc);
        assert_eq!(bytes(&qprint_decode(enc)), raw);
        // observed on PHP 8.5
        assert_eq!(
            bytes(&qprint_decode(b"a=\r\nb=\nc=3d=3D=4=G1= x=\r")),
            b"abc===4=G1= x=\r"
        );
    }

    #[test]
    fn uu() {
        // observed on PHP 8.5
        assert_eq!(
            bytes(&uudecode(b"begin 644 filename\n#86)C\n`\nend\n")),
            b"abc"
        );
        assert!(uudecode(b"#86)C\n").is_empty());
        assert_eq!(
            uuencode(b"A\xe9\xff\x00 =\n\xff@&<"),
            b"begin 0644 filename\n+0>G_`\"`]\"O]`)CP`\n"
        );
    }
}
