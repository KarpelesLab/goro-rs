//! BER/DER encoding and decoding (liblber's encode.c / decode.c).
//!
//! Output is DER-style: definite, minimal lengths, minimal two's
//! complement integers and 0xFF for TRUE, as ber_printf() produces with
//! LBER_USE_DER. Input accepts any definite-length BER.

/// ber_tag_t
pub type Tag = u32;

pub const TAG_BOOLEAN: Tag = 0x01;
pub const TAG_INTEGER: Tag = 0x02;
pub const TAG_OCTETSTRING: Tag = 0x04;
pub const TAG_NULL: Tag = 0x05;
pub const TAG_ENUMERATED: Tag = 0x0a;
pub const TAG_SEQUENCE: Tag = 0x30;
pub const TAG_SET: Tag = 0x31;

/// A BER encoder (BerElement opened for writing).
#[derive(Default, Clone, Debug)]
pub struct Writer {
    buf: Vec<u8>,
    /// Start offsets (just after the tag) of the open constructed elements.
    open: Vec<usize>,
}

fn put_tag(out: &mut Vec<u8>, tag: Tag) {
    let bytes = tag.to_be_bytes();
    let skip = bytes.iter().take_while(|&&b| b == 0).count().min(3);
    out.extend_from_slice(&bytes[skip..]);
}

/// The DER length octets of `len`.
pub fn encode_len(len: usize) -> Vec<u8> {
    if len < 0x80 {
        return vec![len as u8];
    }
    let bytes = (len as u64).to_be_bytes();
    let skip = bytes.iter().take_while(|&&b| b == 0).count();
    let mut v = vec![0x80 | (8 - skip) as u8];
    v.extend_from_slice(&bytes[skip..]);
    v
}

/// The minimal two's complement octets of `n`.
pub fn int_octets(n: i64) -> Vec<u8> {
    let bytes = n.to_be_bytes();
    let mut i = 0;
    while i < 7 {
        let b = bytes[i];
        let next = bytes[i + 1];
        if (b == 0 && next & 0x80 == 0) || (b == 0xff && next & 0x80 != 0) {
            i += 1;
        } else {
            break;
        }
    }
    bytes[i..].to_vec()
}

impl Writer {
    pub fn new() -> Writer {
        Writer::default()
    }

    /// A primitive element.
    pub fn raw(&mut self, tag: Tag, content: &[u8]) -> &mut Self {
        put_tag(&mut self.buf, tag);
        self.buf.extend_from_slice(&encode_len(content.len()));
        self.buf.extend_from_slice(content);
        self
    }

    pub fn int_tagged(&mut self, tag: Tag, n: i64) -> &mut Self {
        let o = int_octets(n);
        self.raw(tag, &o)
    }
    pub fn int(&mut self, n: i64) -> &mut Self {
        self.int_tagged(TAG_INTEGER, n)
    }
    pub fn enumerated(&mut self, n: i64) -> &mut Self {
        self.int_tagged(TAG_ENUMERATED, n)
    }
    pub fn bool_tagged(&mut self, tag: Tag, b: bool) -> &mut Self {
        self.raw(tag, &[if b { 0xff } else { 0 }])
    }
    pub fn boolean(&mut self, b: bool) -> &mut Self {
        self.bool_tagged(TAG_BOOLEAN, b)
    }
    pub fn octets(&mut self, s: &[u8]) -> &mut Self {
        self.raw(TAG_OCTETSTRING, s)
    }
    pub fn null(&mut self) -> &mut Self {
        self.raw(TAG_NULL, &[])
    }

    /// Open a constructed element ('{' / '[' with a tag).
    pub fn start(&mut self, tag: Tag) -> &mut Self {
        put_tag(&mut self.buf, tag);
        self.open.push(self.buf.len());
        self
    }
    pub fn start_seq(&mut self) -> &mut Self {
        self.start(TAG_SEQUENCE)
    }
    pub fn start_set(&mut self) -> &mut Self {
        self.start(TAG_SET)
    }

    /// Close the innermost constructed element.
    pub fn end(&mut self) -> &mut Self {
        let start = self.open.pop().expect("unbalanced BER end()");
        let len = self.buf.len() - start;
        let l = encode_len(len);
        self.buf.splice(start..start, l);
        self
    }

    /// Append already encoded elements.
    pub fn append_encoded(&mut self, der: &[u8]) -> &mut Self {
        self.buf.extend_from_slice(der);
        self
    }

    pub fn is_balanced(&self) -> bool {
        self.open.is_empty()
    }

    /// ber_flatten()
    pub fn finish(mut self) -> Vec<u8> {
        while !self.open.is_empty() {
            self.end();
        }
        self.buf
    }

    pub fn bytes(&self) -> &[u8] {
        &self.buf
    }
}

/// Decoding error (LBER_ERROR).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BerError;

pub type BerResult<T> = Result<T, BerError>;

/// A BER decoder over a byte slice (a BerElement opened for reading).
#[derive(Clone, Copy, Debug)]
pub struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

/// Parse one tag+length header: (tag, header length, content length).
pub fn parse_header(data: &[u8]) -> BerResult<(Tag, usize, usize)> {
    let mut i = 0;
    let first = *data.first().ok_or(BerError)?;
    let mut tag: Tag = first as Tag;
    i += 1;
    if first & 0x1f == 0x1f {
        loop {
            let b = *data.get(i).ok_or(BerError)?;
            i += 1;
            if tag > 0x00ff_ffff {
                return Err(BerError);
            }
            tag = (tag << 8) | b as Tag;
            if b & 0x80 == 0 {
                break;
            }
        }
    }
    let lb = *data.get(i).ok_or(BerError)?;
    i += 1;
    let len = if lb & 0x80 == 0 {
        lb as usize
    } else {
        let n = (lb & 0x7f) as usize;
        if n == 0 || n > 8 {
            // indefinite or overlong length: not supported (as liblber)
            return Err(BerError);
        }
        let mut l: u64 = 0;
        for _ in 0..n {
            let b = *data.get(i).ok_or(BerError)?;
            i += 1;
            l = (l << 8) | b as u64;
        }
        if l > usize::MAX as u64 / 2 {
            return Err(BerError);
        }
        l as usize
    };
    Ok((tag, i, len))
}

/// How many bytes the first complete element of `data` takes, if it is
/// all there (Some(Err) when the header is malformed).
pub fn element_len(data: &[u8]) -> Option<BerResult<usize>> {
    // header needs at least 2 bytes; long forms more
    if data.len() < 2 {
        return None;
    }
    match parse_header(data) {
        Ok((_, h, l)) => {
            let total = h.checked_add(l)?;
            if data.len() >= total {
                Some(Ok(total))
            } else {
                None
            }
        }
        Err(_) => {
            // incomplete header vs. invalid: retry when more data comes,
            // unless the length byte says indefinite / too long
            let lb_pos = if data[0] & 0x1f == 0x1f {
                let mut i = 1;
                while i < data.len() && data[i] & 0x80 != 0 {
                    i += 1;
                }
                i + 1
            } else {
                1
            };
            match data.get(lb_pos) {
                None => None,
                Some(&lb) if lb & 0x80 != 0 => {
                    let n = (lb & 0x7f) as usize;
                    if n == 0 || n > 8 {
                        Some(Err(BerError))
                    } else if data.len() < lb_pos + 1 + n {
                        None
                    } else {
                        Some(Err(BerError))
                    }
                }
                Some(_) => Some(Err(BerError)),
            }
        }
    }
}

pub fn decode_int(c: &[u8]) -> BerResult<i64> {
    if c.is_empty() || c.len() > 8 {
        return Err(BerError);
    }
    let mut v: i64 = if c[0] & 0x80 != 0 { -1 } else { 0 };
    for &b in c {
        v = (v << 8) | b as i64;
    }
    Ok(v)
}

impl<'a> Reader<'a> {
    pub fn new(data: &'a [u8]) -> Reader<'a> {
        Reader { data, pos: 0 }
    }

    pub fn remaining(&self) -> &'a [u8] {
        &self.data[self.pos..]
    }
    pub fn is_empty(&self) -> bool {
        self.pos >= self.data.len()
    }
    pub fn position(&self) -> usize {
        self.pos
    }

    /// ber_peek_tag(): the next tag, without consuming it.
    pub fn peek_tag(&self) -> Option<Tag> {
        parse_header(self.remaining()).ok().map(|(t, _, _)| t)
    }

    /// The next element: (tag, content), consumed.
    pub fn read_tlv(&mut self) -> BerResult<(Tag, &'a [u8])> {
        let rest = self.remaining();
        let (tag, h, l) = parse_header(rest)?;
        if rest.len() < h + l {
            return Err(BerError);
        }
        self.pos += h + l;
        Ok((tag, &rest[h..h + l]))
    }

    /// The next element as encoded (header included), consumed.
    pub fn read_raw(&mut self) -> BerResult<&'a [u8]> {
        let rest = self.remaining();
        let (_, h, l) = parse_header(rest)?;
        if rest.len() < h + l {
            return Err(BerError);
        }
        self.pos += h + l;
        Ok(&rest[..h + l])
    }

    /// The next element, which must have tag `tag`.
    pub fn expect(&mut self, tag: Tag) -> BerResult<&'a [u8]> {
        let save = self.pos;
        let (t, c) = self.read_tlv()?;
        if t != tag {
            self.pos = save;
            return Err(BerError);
        }
        Ok(c)
    }

    /// Enter a constructed element with tag `tag`: a reader over its content.
    pub fn enter(&mut self, tag: Tag) -> BerResult<Reader<'a>> {
        Ok(Reader::new(self.expect(tag)?))
    }
    pub fn seq(&mut self) -> BerResult<Reader<'a>> {
        self.enter(TAG_SEQUENCE)
    }

    pub fn int_tagged(&mut self, tag: Tag) -> BerResult<i64> {
        decode_int(self.expect(tag)?)
    }
    pub fn int(&mut self) -> BerResult<i64> {
        self.int_tagged(TAG_INTEGER)
    }
    pub fn enumerated(&mut self) -> BerResult<i64> {
        self.int_tagged(TAG_ENUMERATED)
    }
    pub fn bool_tagged(&mut self, tag: Tag) -> BerResult<bool> {
        let c = self.expect(tag)?;
        if c.len() != 1 {
            return Err(BerError);
        }
        Ok(c[0] != 0)
    }
    pub fn boolean(&mut self) -> BerResult<bool> {
        self.bool_tagged(TAG_BOOLEAN)
    }
    pub fn octets(&mut self) -> BerResult<&'a [u8]> {
        self.expect(TAG_OCTETSTRING)
    }
    /// An element of any tag, its content (ber_scanf "m" on any tag).
    pub fn any(&mut self) -> BerResult<&'a [u8]> {
        Ok(self.read_tlv()?.1)
    }
    /// The next element if it has tag `tag`.
    pub fn optional(&mut self, tag: Tag) -> BerResult<Option<&'a [u8]>> {
        if self.peek_tag() == Some(tag) {
            Ok(Some(self.expect(tag)?))
        } else {
            Ok(None)
        }
    }
    pub fn skip(&mut self) -> BerResult<()> {
        self.read_tlv().map(|_| ())
    }

    /// "{v}": a SEQUENCE OF OCTET STRING-like values (any tag inside).
    pub fn string_list(&mut self, tag: Tag) -> BerResult<Vec<Vec<u8>>> {
        let mut r = self.enter(tag)?;
        let mut out = Vec::new();
        while !r.is_empty() {
            out.push(r.any()?.to_vec());
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ints() {
        assert_eq!(int_octets(0), vec![0]);
        assert_eq!(int_octets(127), vec![127]);
        assert_eq!(int_octets(128), vec![0, 128]);
        assert_eq!(int_octets(-1), vec![0xff]);
        assert_eq!(int_octets(-129), vec![0xff, 0x7f]);
        for n in [0i64, 1, -1, 255, 256, -256, i64::MAX, i64::MIN, 65535] {
            assert_eq!(decode_int(&int_octets(n)).unwrap(), n);
        }
    }

    #[test]
    fn nested() {
        let mut w = Writer::new();
        w.start_seq().int(1).octets(b"opaque").end();
        assert_eq!(
            w.finish(),
            vec![
                0x30, 0x0b, 2, 1, 1, 4, 6, b'o', b'p', b'a', b'q', b'u', b'e'
            ]
        );
        let big = vec![b'x'; 300];
        let mut w = Writer::new();
        w.start_seq().octets(&big).end();
        let v = w.finish();
        let mut r = Reader::new(&v);
        let mut s = r.seq().unwrap();
        assert_eq!(s.octets().unwrap(), &big[..]);
        assert!(s.is_empty());
    }
}
