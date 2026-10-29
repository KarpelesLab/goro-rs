//! multipart/form-data bodies (RFC 7578) as libcurl's MIME API builds them
//! for CURLOPT_POSTFIELDS arrays: the boundary shape, part headers and
//! the escaping of names (observed on PHP 8.5 with libcurl 8.18; libcurl
//! documentation of CURLOPT_MIME_OPTIONS).

/// One form part.
#[derive(Clone, Debug)]
pub struct Part {
    pub name: Vec<u8>,
    /// The file name of a file part.
    pub filename: Option<Vec<u8>>,
    pub content_type: Option<Vec<u8>>,
    pub data: Vec<u8>,
}

/// A boundary: 24 dashes and 22 random alphanumerics.
pub fn boundary(rand: &mut dyn FnMut() -> u64) -> Vec<u8> {
    const SET: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
    let mut b = vec![b'-'; 24];
    for _ in 0..22 {
        b.push(SET[(rand() % SET.len() as u64) as usize]);
    }
    b
}

/// Escapes a name or file name for a Content-Disposition parameter: by
/// default `"`, CR and LF become %22, %0D and %0A; with
/// CURLMIMEOPT_FORMESCAPE, `"` and `\` are backslash-escaped instead.
fn escape(s: &[u8], formescape: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    for &c in s {
        match c {
            b'"' if formescape => out.extend_from_slice(b"\\\""),
            b'\\' if formescape => out.extend_from_slice(b"\\\\"),
            b'"' => out.extend_from_slice(b"%22"),
            b'\r' => out.extend_from_slice(b"%0D"),
            b'\n' => out.extend_from_slice(b"%0A"),
            _ => out.push(c),
        }
    }
    out
}

/// The body for `parts` with `boundary`.
pub fn encode(parts: &[Part], boundary: &[u8], formescape: bool) -> Vec<u8> {
    let mut out = Vec::new();
    for p in parts {
        out.extend_from_slice(b"--");
        out.extend_from_slice(boundary);
        out.extend_from_slice(b"\r\nContent-Disposition: form-data; name=\"");
        out.extend_from_slice(&escape(&p.name, formescape));
        out.push(b'"');
        if let Some(f) = &p.filename {
            out.extend_from_slice(b"; filename=\"");
            out.extend_from_slice(&escape(f, formescape));
            out.push(b'"');
        }
        out.extend_from_slice(b"\r\n");
        if let Some(t) = &p.content_type {
            out.extend_from_slice(b"Content-Type: ");
            out.extend_from_slice(t);
            out.extend_from_slice(b"\r\n");
        }
        out.extend_from_slice(b"\r\n");
        out.extend_from_slice(&p.data);
        out.extend_from_slice(b"\r\n");
    }
    out.extend_from_slice(b"--");
    out.extend_from_slice(boundary);
    out.extend_from_slice(b"--\r\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn body() {
        let parts = vec![
            Part {
                name: b"a".to_vec(),
                filename: None,
                content_type: None,
                data: b"1".to_vec(),
            },
            Part {
                name: b"f".to_vec(),
                filename: Some(b"n.txt".to_vec()),
                content_type: Some(b"text/x".to_vec()),
                data: b"xyz".to_vec(),
            },
        ];
        let b = encode(&parts, b"BB", false);
        assert_eq!(
            b,
            b"--BB\r\nContent-Disposition: form-data; name=\"a\"\r\n\r\n1\r\n--BB\r\nContent-Disposition: form-data; name=\"f\"; filename=\"n.txt\"\r\nContent-Type: text/x\r\n\r\nxyz\r\n--BB--\r\n"
        );
        let mut n = 0u64;
        let bd = boundary(&mut || {
            n += 7;
            n
        });
        assert_eq!(bd.len(), 46);
        assert!(bd.starts_with(b"------------------------"));
    }
}
