//! Multipart form-data generation (lib/mime.c, MIMESTRATEGY_FORM)

use std::io::Read;

use super::consts::code;
use super::{CurlCode, Mime, MimeData};

const MIME_BOUNDARY_DASHES: usize = 24;
const MIME_RAND_BOUNDARY_CHARS: usize = 22;

/// Curl_rand_alnum()
pub(crate) fn rand_alnum(n: usize) -> Vec<u8> {
    const ALNUM: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    let mut out = Vec::with_capacity(n);
    let mut buf = vec![0u8; n * 4 + 16];
    loop {
        if let Ok(mut f) = std::fs::File::open("/dev/urandom")
            && f.read_exact(&mut buf).is_ok()
        {
            for ch in buf.chunks(4) {
                let r = u32::from_ne_bytes([ch[0], ch[1], ch[2], ch[3]]);
                let space = ALNUM.len() as u32;
                if r >= u32::MAX - u32::MAX % space {
                    continue;
                }
                out.push(ALNUM[(r % space) as usize]);
                if out.len() == n {
                    return out;
                }
            }
            continue;
        }
        // no /dev/urandom: time based fallback
        let mut x = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(1)
            | 1;
        while out.len() < n {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            out.push(ALNUM[(x % ALNUM.len() as u64) as usize]);
        }
        return out;
    }
}

/// A new boundary: 24 dashes + 22 random alphanumerics
pub(crate) fn new_boundary() -> Vec<u8> {
    let mut b = vec![b'-'; MIME_BOUNDARY_DASHES];
    b.extend_from_slice(&rand_alnum(MIME_RAND_BOUNDARY_CHARS));
    b
}

/// Curl_mime_contenttype()
fn contenttype_for(filename: &[u8]) -> Option<&'static str> {
    const CTTS: &[(&str, &str)] = &[
        (".gif", "image/gif"),
        (".jpg", "image/jpeg"),
        (".jpeg", "image/jpeg"),
        (".png", "image/png"),
        (".svg", "image/svg+xml"),
        (".txt", "text/plain"),
        (".htm", "text/html"),
        (".html", "text/html"),
        (".pdf", "application/pdf"),
        (".xml", "application/xml"),
    ];
    for (ext, ty) in CTTS {
        let e = ext.as_bytes();
        if filename.len() >= e.len() && filename[filename.len() - e.len()..].eq_ignore_ascii_case(e)
        {
            return Some(ty);
        }
    }
    None
}

/// escape_string() with the form table (or the mime one with
/// CURLMIMEOPT_FORMESCAPE)
fn escape_string(src: &[u8], formescape: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(src.len());
    for &c in src {
        if c == 0 {
            break;
        }
        if formescape {
            match c {
                b'\\' => out.extend_from_slice(b"\\\\"),
                b'"' => out.extend_from_slice(b"\\\""),
                _ => out.push(c),
            }
        } else {
            match c {
                b'"' => out.extend_from_slice(b"%22"),
                b'\r' => out.extend_from_slice(b"%0D"),
                b'\n' => out.extend_from_slice(b"%0A"),
                _ => out.push(c),
            }
        }
    }
    out
}

fn basename(p: &std::path::Path) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;
    let s = p.as_os_str().as_bytes();
    match s.iter().rposition(|&c| c == b'/') {
        Some(i) => s[i + 1..].to_vec(),
        None => s.to_vec(),
    }
}

/// A piece of the generated body
enum Seg {
    Static(Vec<u8>),
    /// data of part #i
    Part(usize),
}

/// The client reader of a mime post
pub(crate) struct MimeReader {
    segs: Vec<Seg>,
    idx: usize,
    pos: usize,
    file: Option<std::fs::File>,
    /// part sizes (-1 unknown)
    sizes: Vec<i64>,
    pub total: i64,
    pub boundary: Vec<u8>,
    part_read: i64,
}

impl MimeReader {
    /// Curl_mime_prepare_headers() + the body layout. `cthdr` is the
    /// Content-Type of the request (custom one from CURLOPT_HTTPHEADER or
    /// "multipart/form-data"). Returns the reader and the
    /// "Content-Type: ...; boundary=..." header line to add (None when a
    /// custom Content-Type header is used as is... libcurl still adds the
    /// boundary then).
    pub fn new(mime: &Mime, cthdr: &[u8], formescape: bool) -> (MimeReader, Vec<u8>) {
        let boundary = new_boundary();
        let mut segs = Vec::new();
        let mut sizes = Vec::new();
        let mut total: i64 = 0;
        let mut unknown = false;
        let n = mime.parts.len();
        for (i, part) in mime.parts.iter().enumerate() {
            let mut hdr = Vec::new();
            if i == 0 {
                hdr.extend_from_slice(b"--");
            } else {
                hdr.extend_from_slice(b"\r\n--");
            }
            hdr.extend_from_slice(&boundary);
            hdr.extend_from_slice(b"\r\n");
            // part headers
            let filename: Option<Vec<u8>> = match (&part.filename, &part.data) {
                (Some(f), _) => Some(f.clone()),
                (None, MimeData::File(p)) => Some(basename(p)),
                _ => None,
            };
            let mut contenttype: Option<Vec<u8>> = part.content_type.clone();
            let custom = contenttype.is_some();
            if contenttype.is_none() {
                contenttype = match &part.data {
                    MimeData::File(p) => {
                        use std::os::unix::ffi::OsStrExt;
                        filename
                            .as_deref()
                            .and_then(contenttype_for)
                            .or_else(|| contenttype_for(p.as_os_str().as_bytes()))
                            .or(if filename.is_some() {
                                Some("application/octet-stream")
                            } else {
                                None
                            })
                            .map(|s| s.as_bytes().to_vec())
                    }
                    _ => filename
                        .as_deref()
                        .and_then(contenttype_for)
                        .map(|s| s.as_bytes().to_vec()),
                };
            }
            if let Some(ct) = &contenttype
                && !custom
                && ct.len() >= 10
                && ct[..10].eq_ignore_ascii_case(b"text/plain")
                && filename.is_none()
            {
                contenttype = None;
            }
            hdr.extend_from_slice(b"Content-Disposition: form-data");
            hdr.extend_from_slice(b"; name=\"");
            hdr.extend_from_slice(&escape_string(&part.name, formescape));
            hdr.push(b'"');
            if let Some(f) = &filename {
                hdr.extend_from_slice(b"; filename=\"");
                hdr.extend_from_slice(&escape_string(f, formescape));
                hdr.push(b'"');
            }
            hdr.extend_from_slice(b"\r\n");
            if let Some(ct) = &contenttype {
                hdr.extend_from_slice(b"Content-Type: ");
                hdr.extend_from_slice(ct);
                hdr.extend_from_slice(b"\r\n");
            }
            hdr.extend_from_slice(b"\r\n");
            total += hdr.len() as i64;
            segs.push(Seg::Static(hdr));
            let size = match &part.data {
                MimeData::Bytes(b) => b.len() as i64,
                MimeData::File(p) => match std::fs::metadata(p) {
                    Ok(m) if m.is_file() => m.len() as i64,
                    _ => -1,
                },
                MimeData::Callback { size, .. } => *size,
            };
            if size < 0 {
                unknown = true;
            } else {
                total += size;
            }
            sizes.push(size);
            segs.push(Seg::Part(i));
        }
        let mut end = Vec::new();
        if n > 0 {
            end.extend_from_slice(b"\r\n--");
        } else {
            end.extend_from_slice(b"--");
        }
        end.extend_from_slice(&boundary);
        end.extend_from_slice(b"--\r\n");
        total += end.len() as i64;
        segs.push(Seg::Static(end));
        let mut ct = b"Content-Type: ".to_vec();
        ct.extend_from_slice(cthdr);
        ct.extend_from_slice(b"; boundary=");
        ct.extend_from_slice(&boundary);
        (
            MimeReader {
                segs,
                idx: 0,
                pos: 0,
                file: None,
                sizes,
                total: if unknown { -1 } else { total },
                boundary,
                part_read: 0,
            },
            ct,
        )
    }

    /// Read up to `max` bytes. Ok((data, eos))
    pub fn read(
        &mut self,
        mime: &mut Mime,
        max: usize,
    ) -> Result<(Vec<u8>, bool), (CurlCode, String)> {
        let mut out = Vec::new();
        while out.len() < max && self.idx < self.segs.len() {
            match &self.segs[self.idx] {
                Seg::Static(b) => {
                    let n = (b.len() - self.pos).min(max - out.len());
                    out.extend_from_slice(&b[self.pos..self.pos + n]);
                    self.pos += n;
                    if self.pos == b.len() {
                        self.idx += 1;
                        self.pos = 0;
                    }
                }
                Seg::Part(i) => {
                    let i = *i;
                    let want = max - out.len();
                    let got: Vec<u8> = match &mut mime.parts[i].data {
                        MimeData::Bytes(b) => {
                            let n = (b.len() - self.pos).min(want);
                            let v = b[self.pos..self.pos + n].to_vec();
                            self.pos += n;
                            v
                        }
                        MimeData::File(p) => {
                            if self.file.is_none() {
                                match std::fs::File::open(&*p) {
                                    Ok(f) => self.file = Some(f),
                                    Err(_) => {
                                        return Err((
                                            code::READ_ERROR,
                                            "read error getting mime data".into(),
                                        ));
                                    }
                                }
                            }
                            let f = self.file.as_mut().unwrap();
                            let mut buf = vec![0u8; want];
                            match f.read(&mut buf) {
                                Ok(n) => {
                                    buf.truncate(n);
                                    buf
                                }
                                Err(_) => {
                                    return Err((
                                        code::READ_ERROR,
                                        "read error getting mime data".into(),
                                    ));
                                }
                            }
                        }
                        MimeData::Callback { reader, .. } => {
                            let mut v = reader(want);
                            v.truncate(want);
                            v
                        }
                    };
                    let size = self.sizes[i];
                    self.part_read += got.len() as i64;
                    let finished = got.is_empty() || (size >= 0 && self.part_read >= size);
                    if got.is_empty() && size >= 0 && self.part_read < size {
                        return Err((
                            code::READ_ERROR,
                            format!(
                                "client mime read EOF fail, only {}/{} of needed bytes read",
                                self.part_read, size
                            ),
                        ));
                    }
                    out.extend_from_slice(&got);
                    if finished {
                        self.idx += 1;
                        self.pos = 0;
                        self.file = None;
                        self.part_read = 0;
                    }
                }
            }
        }
        Ok((out, self.idx >= self.segs.len()))
    }
}
