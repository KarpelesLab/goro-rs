//! PEM (RFC 7468, and RFC 1421's headers for the legacy encrypted
//! private keys) and base64 (RFC 4648).

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn base64_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let n = match c.len() {
            3 => (c[0] as u32) << 16 | (c[1] as u32) << 8 | c[2] as u32,
            2 => (c[0] as u32) << 16 | (c[1] as u32) << 8,
            _ => (c[0] as u32) << 16,
        };
        out.push(B64[(n >> 18) as usize & 63] as char);
        out.push(B64[(n >> 12) as usize & 63] as char);
        out.push(if c.len() > 1 {
            B64[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if c.len() > 2 {
            B64[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

/// Base64 in lines of `width` characters, each ending in `\n`.
pub fn base64_lines(data: &[u8], width: usize) -> String {
    let s = base64_encode(data);
    let mut out = String::with_capacity(s.len() + s.len() / width + 1);
    for ch in s.as_bytes().chunks(width) {
        out.push_str(std::str::from_utf8(ch).unwrap_or(""));
        out.push('\n');
    }
    out
}

fn val(c: u8) -> Option<u32> {
    Some(match c {
        b'A'..=b'Z' => c - b'A',
        b'a'..=b'z' => c - b'a' + 26,
        b'0'..=b'9' => c - b'0' + 52,
        b'+' => 62,
        b'/' => 63,
        _ => return None,
    } as u32)
}

/// Decodes base64, skipping whitespace; none on other junk.
pub fn base64_decode(s: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let mut acc = 0u32;
    let mut n = 0;
    let mut pad = 0;
    for &c in s {
        if c.is_ascii_whitespace() {
            continue;
        }
        if c == b'=' {
            pad += 1;
            continue;
        }
        if pad > 0 {
            return None;
        }
        acc = (acc << 6) | val(c)?;
        n += 1;
        if n == 4 {
            out.extend_from_slice(&[(acc >> 16) as u8, (acc >> 8) as u8, acc as u8]);
            acc = 0;
            n = 0;
        }
    }
    match n {
        0 => {}
        2 => out.push((acc >> 4) as u8),
        3 => out.extend_from_slice(&[(acc >> 10) as u8, (acc >> 2) as u8]),
        _ => return None,
    }
    Some(out)
}

/// A PEM block.
#[derive(Debug, Clone)]
pub struct Block {
    pub label: String,
    /// RFC 1421 headers (`Proc-Type`, `DEK-Info`).
    pub headers: Vec<(String, String)>,
    pub data: Vec<u8>,
    /// Byte offset just past the END line.
    pub end: usize,
}

impl Block {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

/// Every PEM block in `text`, in order (blocks with bad base64 are
/// skipped).
pub fn parse_all(text: &[u8]) -> Vec<Block> {
    let mut out = Vec::new();
    let mut pos = 0;
    while let Some(b) = parse_from(text, pos) {
        pos = b.end;
        out.push(b);
    }
    out
}

/// The first PEM block at or after `start` (malformed ones skipped).
pub fn parse_from(text: &[u8], start: usize) -> Option<Block> {
    let mut pos = start;
    loop {
        let rel = find(&text[pos..], b"-----BEGIN ")?;
        let lstart = pos + rel + 11;
        let lend_rel = find(&text[lstart..], b"-----")?;
        let label = &text[lstart..lstart + lend_rel];
        if label.contains(&b'\n') {
            pos = lstart;
            continue;
        }
        let label = String::from_utf8_lossy(label).into_owned();
        let body_start = lstart + lend_rel + 5;
        let end_marker = format!("-----END {}-----", label);
        let Some(erel) = find(&text[body_start..], end_marker.as_bytes()) else {
            pos = body_start;
            continue;
        };
        let body = &text[body_start..body_start + erel];
        let end = body_start + erel + end_marker.len();
        let (headers, b64) = split_headers(body);
        match base64_decode(b64) {
            Some(data) => {
                return Some(Block {
                    label,
                    headers,
                    data,
                    end,
                });
            }
            None => {
                pos = end;
                continue;
            }
        }
    }
}

fn split_headers(body: &[u8]) -> (Vec<(String, String)>, &[u8]) {
    let mut headers = Vec::new();
    let text = body;
    // Headers are "Name: value" lines before a blank line.
    let first_nonblank = text
        .iter()
        .position(|c| !c.is_ascii_whitespace())
        .unwrap_or(text.len());
    let rest = &text[first_nonblank..];
    let line_end = rest.iter().position(|&c| c == b'\n').unwrap_or(rest.len());
    let first_line = &rest[..line_end];
    if !first_line.contains(&b':') {
        return (headers, body);
    }
    let mut i = first_nonblank;
    loop {
        let rest = &text[i..];
        let le = rest.iter().position(|&c| c == b'\n').unwrap_or(rest.len());
        let line = &rest[..le];
        let line_trim = trim(line);
        i += le + 1;
        if line_trim.is_empty() {
            break;
        }
        if let Some(c) = line_trim.iter().position(|&x| x == b':') {
            headers.push((
                String::from_utf8_lossy(trim(&line_trim[..c])).into_owned(),
                String::from_utf8_lossy(trim(&line_trim[c + 1..])).into_owned(),
            ));
        } else {
            // Not a header after all.
            return (Vec::new(), body);
        }
        if i >= text.len() {
            break;
        }
    }
    (headers, &text[i.min(text.len())..])
}

fn trim(b: &[u8]) -> &[u8] {
    let s = b
        .iter()
        .position(|c| !c.is_ascii_whitespace())
        .unwrap_or(b.len());
    let e = b
        .iter()
        .rposition(|c| !c.is_ascii_whitespace())
        .map_or(s, |x| x + 1);
    &b[s..e.max(s)]
}

pub fn find(h: &[u8], n: &[u8]) -> Option<usize> {
    if n.is_empty() || h.len() < n.len() {
        return if n.is_empty() { Some(0) } else { None };
    }
    h.windows(n.len()).position(|w| w == n)
}

/// A PEM block with 64-column base64.
pub fn encode(label: &str, data: &[u8]) -> String {
    format!(
        "-----BEGIN {}-----\n{}-----END {}-----\n",
        label,
        base64_lines(data, 64),
        label
    )
}

/// A PEM block with RFC 1421 headers.
pub fn encode_with_headers(label: &str, headers: &[(&str, String)], data: &[u8]) -> String {
    let mut s = format!("-----BEGIN {}-----\n", label);
    for (k, v) in headers {
        s.push_str(&format!("{}: {}\n", k, v));
    }
    s.push('\n');
    s.push_str(&base64_lines(data, 64));
    s.push_str(&format!("-----END {}-----\n", label));
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn b64() {
        assert_eq!(base64_encode(b"foob"), "Zm9vYg==");
        assert_eq!(base64_decode(b"Zm9v\nYmE=").unwrap(), b"fooba");
    }
    #[test]
    fn pem() {
        let p = encode("TEST", b"hello world");
        let b = parse_all(p.as_bytes());
        assert_eq!(b.len(), 1);
        assert_eq!(b[0].data, b"hello world");
        let h = encode_with_headers(
            "RSA PRIVATE KEY",
            &[("Proc-Type", "4,ENCRYPTED".into())],
            b"x",
        );
        let b = parse_all(h.as_bytes());
        assert_eq!(b[0].header("proc-type"), Some("4,ENCRYPTED"));
        assert_eq!(b[0].data, b"x");
    }
}
