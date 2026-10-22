//! ext/libxml/image_svg.c: getimagesize() & co. for SVG images, registered
//! as an image handler (IMAGETYPE_SVG).
//!
//! The C code reads the document with an xmlTextReader up to the root
//! element; this reads the prolog (XML declaration, PIs, comments, doctype)
//! and the root start tag the same way.

use goro_core::image_handlers::{ImageHandler, ImageInfo};

/// The root element: (local name, attributes).
type Root = (Vec<u8>, Vec<(Vec<u8>, Vec<u8>)>);

fn is_ws(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\r' | b'\n')
}

fn find(hay: &[u8], from: usize, needle: &[u8]) -> Option<usize> {
    hay.get(from..)?
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|p| from + p)
}

/// Attribute value normalization: the predefined and character entities.
fn decode_attr(v: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(v.len());
    let mut i = 0;
    while i < v.len() {
        match v[i] {
            b'<' => return None,
            b'&' => {
                let end = i + v[i..].iter().position(|&c| c == b';')?;
                let ent = &v[i + 1..end];
                let ch: u32 = match ent {
                    b"lt" => '<' as u32,
                    b"gt" => '>' as u32,
                    b"amp" => '&' as u32,
                    b"quot" => '"' as u32,
                    b"apos" => '\'' as u32,
                    _ if ent.first() == Some(&b'#') => {
                        let (digits, radix) = match ent.get(1) {
                            Some(b'x') => (&ent[2..], 16),
                            _ => (&ent[1..], 10),
                        };
                        u32::from_str_radix(std::str::from_utf8(digits).ok()?, radix).ok()?
                    }
                    _ => return None,
                };
                let c = char::from_u32(ch)?;
                let mut b = [0u8; 4];
                out.extend_from_slice(c.encode_utf8(&mut b).as_bytes());
                i = end + 1;
            }
            b'\t' | b'\n' | b'\r' => {
                out.push(b' ');
                i += 1;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    Some(out)
}

/// The root element of `data`, or None when the reader would fail first.
fn root_element(data: &[u8]) -> Option<Root> {
    let mut i = 0;
    if data.starts_with(b"\xEF\xBB\xBF") {
        i = 3;
    }
    loop {
        while i < data.len() && is_ws(data[i]) {
            i += 1;
        }
        let rest = &data[i..];
        if rest.starts_with(b"<?") {
            i = find(data, i + 2, b"?>")? + 2;
        } else if rest.starts_with(b"<!--") {
            i = find(data, i + 4, b"-->")? + 3;
        } else if rest.starts_with(b"<!DOCTYPE") {
            // up to the '>' outside quotes and the internal subset
            let mut j = i + 9;
            let mut depth = 0;
            let mut quote = 0u8;
            loop {
                let c = *data.get(j)?;
                if quote != 0 {
                    if c == quote {
                        quote = 0;
                    }
                } else {
                    match c {
                        b'"' | b'\'' => quote = c,
                        b'[' => depth += 1,
                        b']' => depth -= 1,
                        b'>' if depth == 0 => break,
                        _ => {}
                    }
                }
                j += 1;
            }
            i = j + 1;
        } else if rest.first() == Some(&b'<') {
            return start_tag(data, i + 1);
        } else {
            return None;
        }
    }
}

fn is_name_char(c: u8) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-' | b'.' | b':') || c >= 0x80
}

fn start_tag(data: &[u8], mut i: usize) -> Option<Root> {
    let start = i;
    let first = *data.get(i)?;
    if !(first.is_ascii_alphabetic() || first == b'_' || first == b':' || first >= 0x80) {
        return None;
    }
    while i < data.len() && is_name_char(data[i]) {
        i += 1;
    }
    let qname = data[start..i].to_vec();
    let mut attrs: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
    loop {
        let ws_start = i;
        while i < data.len() && is_ws(data[i]) {
            i += 1;
        }
        match data.get(i)? {
            b'>' => break,
            b'/' if data.get(i + 1) == Some(&b'>') => break,
            _ if i == ws_start => return None,
            _ => {}
        }
        let ns = i;
        while i < data.len() && is_name_char(data[i]) {
            i += 1;
        }
        if i == ns {
            return None;
        }
        let name = data[ns..i].to_vec();
        while i < data.len() && is_ws(data[i]) {
            i += 1;
        }
        if data.get(i) != Some(&b'=') {
            return None;
        }
        i += 1;
        while i < data.len() && is_ws(data[i]) {
            i += 1;
        }
        let q = *data.get(i)?;
        if q != b'"' && q != b'\'' {
            return None;
        }
        let vs = i + 1;
        let ve = vs + data[vs..].iter().position(|&c| c == q)?;
        let value = decode_attr(&data[vs..ve])?;
        if attrs.iter().any(|(n, _)| *n == name) {
            // Attribute redefined: a well-formedness error
            return None;
        }
        attrs.push((name, value));
        i = ve + 1;
    }
    if std::str::from_utf8(&data[..i]).is_err() {
        return None;
    }
    // The local name when the prefix is declared on the element.
    let local = match qname.iter().position(|&c| c == b':') {
        Some(p) => {
            let mut decl = b"xmlns:".to_vec();
            decl.extend_from_slice(&qname[..p]);
            if attrs.iter().any(|(n, _)| *n == decl) {
                qname[p + 1..].to_vec()
            } else {
                qname
            }
        }
        None => qname,
    };
    Some((local, attrs))
}

/// php_libxml_parse_dimension(): digits, then an optional alphabetic unit.
/// Returns the unit.
fn parse_dimension(input: &[u8]) -> Option<&[u8]> {
    if !input.first()?.is_ascii_digit() {
        return None;
    }
    let mut i = 1;
    while i < input.len() && input[i].is_ascii_digit() {
        i += 1;
    }
    let unit = &input[i..];
    unit.iter().all(|c| c.is_ascii_alphabetic()).then_some(unit)
}

fn strtol(s: &[u8]) -> u32 {
    let mut n: i64 = 0;
    for &c in s.iter().take_while(|c| c.is_ascii_digit()) {
        n = n.saturating_mul(10).saturating_add((c - b'0') as i64);
    }
    n as u32
}

/// php_libxml_svg_image_handle()
fn handle(data: &[u8]) -> Option<ImageInfo> {
    // Early check before doing more expensive work
    if data.first() != Some(&b'<') {
        return None;
    }
    let (name, attrs) = root_element(data)?;
    if !name.eq_ignore_ascii_case(b"svg") {
        return None;
    }
    let attr = |n: &[u8]| attrs.iter().find(|(k, _)| k == n).map(|(_, v)| v.clone());
    let width = attr(b"width")?;
    let height = attr(b"height")?;
    let wu = parse_dimension(&width)?;
    let hu = parse_dimension(&height)?;
    let unit = |u: &[u8]| (!u.is_empty()).then(|| String::from_utf8_lossy(u).into_owned());
    Some(ImageInfo {
        width: strtol(&width),
        height: strtol(&height),
        width_unit: unit(wu),
        height_unit: unit(hu),
    })
}

fn identify(data: &[u8]) -> bool {
    handle(data).is_some()
}

static SVG_IMAGE_HANDLER: ImageHandler = ImageHandler {
    mime_type: "image/svg+xml",
    extension: ".svg",
    const_name: "IMAGETYPE_SVG",
    identify,
    get_info: handle,
};

/// php_libxml_register_image_svg_handler()
pub fn register(vm: &mut goro_core::vm::Vm) {
    goro_core::image_handlers::register(vm, &SVG_IMAGE_HANDLER);
}
