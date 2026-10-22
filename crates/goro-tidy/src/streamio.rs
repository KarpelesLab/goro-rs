//! streamio.c (+ buffio.c's TidyBuffer source/sink): character stream
//! I/O with the encoding transformations to/from UCS-4.
//!
//! Input streams live in the document (`doc.doc_in`, or
//! `doc.config.cfg_in` while reading a configuration file); the reading
//! functions take the document and an `InSel` naming the stream, so that
//! error reports made while reading can still see the stream.

use crate::doc::*;
use crate::enums::OptId;
use crate::enums::code::*;
use crate::message;
use crate::utf8::*;

pub const CHARBUF_SIZE: usize = 5;
pub const LASTPOS_SIZE: usize = 64;

pub const UNICODE_BOM_BE: u32 = 0xFEFF;
pub const UNICODE_BOM_LE: u32 = 0xFFFE;
pub const UNICODE_BOM_UTF8: u32 = 0xEFBBBF;
pub const UNICODE_BOM: u32 = 0xFEFF;

/// ISO2022State
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Iso2022State {
    #[default]
    FsmAscii,
    FsmEsc,
    FsmEscd,
    FsmEscdp,
    FsmEscp,
    FsmNonascii,
}

/// A TidyBuffer used as input source.
#[derive(Clone, Debug, Default)]
pub struct InputSource {
    pub data: Vec<u8>,
    pub next: usize,
}

impl InputSource {
    pub fn new(data: Vec<u8>) -> Self {
        InputSource { data, next: 0 }
    }

    /// tidyBufGetByte: EOF is END_OF_STREAM
    pub fn get_byte(&mut self) -> u32 {
        if self.eof() {
            END_OF_STREAM
        } else {
            let b = self.data[self.next];
            self.next += 1;
            b as u32
        }
    }

    /// tidyBufEndOfInput
    pub fn eof(&self) -> bool {
        self.next >= self.data.len()
    }

    /// tidyBufUngetByte
    pub fn unget_byte(&mut self, _bv: u8) {
        if self.next > 0 {
            self.next -= 1;
        }
    }
}

/// _StreamIn
#[derive(Clone, Debug)]
pub struct StreamIn {
    pub state: Iso2022State,
    pub pushed: bool,
    pub charbuf: Vec<u32>,
    pub tabs: i32,
    pub lastcols: [i32; LASTPOS_SIZE],
    pub curlastpos: usize,
    pub firstlastpos: usize,
    pub curcol: i32,
    pub curline: i32,
    pub encoding: u32,
    pub source: InputSource,
}

/// Which input stream to read.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InSel {
    Doc,
    Cfg,
}

/// Where a StreamOut writes.
#[derive(Clone, Debug)]
pub enum Sink {
    Buffer(Vec<u8>),
    Stderr,
}

/// _StreamOut
#[derive(Clone, Debug)]
pub struct StreamOut {
    pub encoding: u32,
    pub state: Iso2022State,
    pub nl: u32,
    pub sink: Sink,
}

impl StreamOut {
    /// BufferOutput()
    pub fn buffer(encoding: u32, nl: u32) -> Self {
        StreamOut {
            encoding,
            state: Iso2022State::FsmAscii,
            nl,
            sink: Sink::Buffer(Vec::new()),
        }
    }

    /// StdErrOutput()
    pub fn stderr() -> Self {
        StreamOut {
            encoding: ASCII,
            state: Iso2022State::FsmAscii,
            nl: TIDY_LF,
            sink: Sink::Stderr,
        }
    }

    /// The bytes written so far (empty for stderr).
    pub fn bytes(&self) -> &[u8] {
        match &self.sink {
            Sink::Buffer(b) => b,
            Sink::Stderr => &[],
        }
    }

    fn put_byte(&mut self, b: u32) {
        match &mut self.sink {
            Sink::Buffer(v) => v.push(b as u8),
            Sink::Stderr => {
                use std::io::Write;
                let _ = std::io::stderr().write_all(&[b as u8]);
            }
        }
    }
}

/// initStreamIn() + BufferInput()
pub fn buffer_input(data: Vec<u8>, encoding: u32) -> StreamIn {
    StreamIn {
        state: Iso2022State::FsmAscii,
        pushed: false,
        charbuf: Vec::with_capacity(CHARBUF_SIZE),
        tabs: 0,
        lastcols: [0; LASTPOS_SIZE],
        curlastpos: 0,
        firstlastpos: 0,
        curcol: 1,
        curline: 1,
        encoding,
        source: InputSource::new(data),
    }
}

fn sin(doc: &mut Doc, sel: InSel) -> &mut StreamIn {
    match sel {
        InSel::Doc => doc.doc_in.as_mut().expect("tidy: no input stream"),
        InSel::Cfg => doc.config.cfg_in.as_mut().expect("tidy: no config stream"),
    }
}

/// ReadBOMEncoding(): returns the encoding of a BOM, or -1.
pub fn read_bom_encoding(doc: &mut Doc, sel: InSel) -> i32 {
    let (c, c1);
    {
        let s = sin(doc, sel);
        c = s.source.get_byte();
        if c == END_OF_STREAM {
            return -1;
        }
        c1 = s.source.get_byte();
        if c1 == END_OF_STREAM {
            s.source.unget_byte(c as u8);
            return -1;
        }
    }
    let bom = (c << 8) + c1;
    let enc = sin(doc, sel).encoding;
    if bom == UNICODE_BOM_BE {
        if enc != UTF16 && enc != UTF16BE {
            message::report_encoding_warning(doc, ENCODING_MISMATCH, UTF16BE);
        }
        return UTF16BE as i32;
    } else if bom == UNICODE_BOM_LE {
        if enc != UTF16 && enc != UTF16LE {
            message::report_encoding_warning(doc, ENCODING_MISMATCH, UTF16LE);
        }
        return UTF16LE as i32;
    } else {
        let c2 = sin(doc, sel).source.get_byte();
        if c2 == END_OF_STREAM {
            let s = sin(doc, sel);
            s.source.unget_byte(c1 as u8);
            s.source.unget_byte(c as u8);
            return -1;
        }
        if ((c << 16) + (c1 << 8) + c2) == UNICODE_BOM_UTF8 {
            if enc != UTF8 {
                message::report_encoding_warning(doc, ENCODING_MISMATCH, UTF8);
            }
            return UTF8 as i32;
        } else {
            sin(doc, sel).source.unget_byte(c2 as u8);
        }
    }
    let s = sin(doc, sel);
    s.source.unget_byte(c1 as u8);
    s.source.unget_byte(c as u8);
    -1
}

fn pop_last_pos(s: &mut StreamIn) {
    s.curlastpos = (s.curlastpos + 1) % LASTPOS_SIZE;
    if s.curlastpos == s.firstlastpos {
        s.firstlastpos = (s.firstlastpos + 1) % LASTPOS_SIZE;
    }
}

fn save_last_pos(s: &mut StreamIn) {
    pop_last_pos(s);
    s.lastcols[s.curlastpos] = s.curcol;
}

fn restore_last_pos(s: &mut StreamIn) {
    if s.firstlastpos == s.curlastpos {
        s.curcol = 0;
    } else {
        s.curcol = s.lastcols[s.curlastpos];
        if s.curlastpos == 0 {
            s.curlastpos = LASTPOS_SIZE;
        }
        s.curlastpos -= 1;
    }
}

/// Set the lexer's position to the stream's (error position just before
/// the offending character).
fn mark_position(doc: &mut Doc, sel: InSel) {
    let (l, c) = {
        let s = sin(doc, sel);
        (s.curline, s.curcol)
    };
    if let Some(lx) = doc.lexer.as_mut() {
        lx.lines = l as u32;
        lx.columns = c as u32;
    }
}

/// ReadChar()
pub fn read_char(doc: &mut Doc, sel: InSel) -> u32 {
    let mut c;
    if sin(doc, sel).pushed {
        return pop_char(sin(doc, sel));
    }
    save_last_pos(sin(doc, sel));
    {
        let s = sin(doc, sel);
        if s.tabs > 0 {
            s.curcol += 1;
            s.tabs -= 1;
            return ' ' as u32;
        }
    }
    loop {
        c = read_char_from_stream(doc, sel);
        if c == END_OF_STREAM {
            return END_OF_STREAM;
        }
        if c == '\n' as u32 {
            let s = sin(doc, sel);
            s.curcol = 1;
            s.curline += 1;
            break;
        }
        if c == '\t' as u32 {
            let keeptabs = doc.cfg_bool(OptId::KeepTabs);
            let tabsize = doc.cfg(OptId::TabSize) as i32;
            let s = sin(doc, sel);
            if !keeptabs {
                s.tabs = if tabsize > 0 {
                    tabsize - ((s.curcol - 1) % tabsize) - 1
                } else {
                    0
                };
                c = ' ' as u32;
            }
            s.curcol += 1;
            break;
        }
        if c == '\r' as u32 {
            c = read_char_from_stream(doc, sel);
            let s = sin(doc, sel);
            if c != '\n' as u32 {
                unget_char_in(c, s);
                c = '\n' as u32;
            }
            s.curcol = 1;
            s.curline += 1;
            break;
        }
        // strip control characters, except for Esc
        if c == 0o33 {
            break;
        }
        // "Form Feed is allowed in HTML" (sic: '\015' is CR)
        if c == 0o15 && !doc.cfg_bool(OptId::XmlTags) {
            break;
        }
        if c < 32 {
            continue;
        }
        let enc = sin(doc, sel).encoding;
        if enc == RAW || enc == ISO2022 || enc == UTF8 || enc == SHIFTJIS || enc == BIG5 {
            sin(doc, sel).curcol += 1;
            break;
        }
        if enc == UTF16LE || enc == UTF16 || enc == UTF16BE {
            if !is_valid_utf16_from_ucs4(c) {
                message::report_encoding_error(doc, INVALID_UTF16, c, true);
                c = 0;
            } else if is_low_surrogate(c) {
                let n = c;
                let m = read_char_from_stream(doc, sel);
                if m == END_OF_STREAM {
                    return END_OF_STREAM;
                }
                c = 0;
                if is_high_surrogate(m) {
                    let n2 = combine_surrogate_pair(m, n);
                    if is_valid_combined_char(n2) {
                        c = n2;
                    }
                }
                if c == 0 {
                    message::report_encoding_error(doc, INVALID_UTF16, c, true);
                }
            }
        }
        match enc {
            MACROMAN => c = decode_mac_roman(c),
            IBM858 => c = decode_ibm850(c),
            LATIN0 => c = decode_latin0(c),
            _ => {}
        }
        if 127 < c && c < 160 {
            let mut repl_mode = DISCARDED_CHAR;
            let is_vendor_char = enc == WIN1252 || enc == MACROMAN;
            let is_mac_char = enc == MACROMAN;
            mark_position(doc, sel);
            let c1 = if is_mac_char {
                decode_mac_roman(c)
            } else {
                decode_win1252(c)
            };
            if c1 != 0 {
                repl_mode = REPLACED_CHAR;
            }
            if c1 == 0 && is_vendor_char {
                message::report_encoding_error(
                    doc,
                    VENDOR_SPECIFIC_CHARS,
                    c,
                    repl_mode == DISCARDED_CHAR,
                );
            } else if !is_vendor_char {
                message::report_encoding_error(
                    doc,
                    INVALID_SGML_CHARS,
                    c,
                    repl_mode == DISCARDED_CHAR,
                );
            }
            c = c1;
        }
        if c == 0 {
            continue;
        }
        sin(doc, sel).curcol += 1;
        break;
    }
    c
}

fn pop_char(s: &mut StreamIn) -> u32 {
    let mut c = END_OF_STREAM;
    if s.pushed {
        c = s.charbuf.pop().unwrap_or(END_OF_STREAM);
        if s.charbuf.is_empty() {
            s.pushed = false;
        }
        if c == '\n' as u32 {
            s.curcol = 1;
            s.curline += 1;
            pop_last_pos(s);
            return c;
        }
        s.curcol += 1;
        pop_last_pos(s);
    }
    c
}

fn unget_char_in(c: u32, s: &mut StreamIn) {
    if c == END_OF_STREAM {
        return;
    }
    s.pushed = true;
    s.charbuf.push(c);
    if c == '\n' as u32 {
        s.curline -= 1;
    }
    restore_last_pos(s);
}

/// UngetChar()
pub fn unget_char(doc: &mut Doc, sel: InSel, c: u32) {
    unget_char_in(c, sin(doc, sel));
}

/// IsEOF()
pub fn is_eof(doc: &mut Doc, sel: InSel) -> bool {
    sin(doc, sel).source.eof()
}

/// ReadCharFromStream()
fn read_char_from_stream(doc: &mut Doc, sel: InSel) -> u32 {
    let s = sin(doc, sel);
    if s.source.eof() {
        return END_OF_STREAM;
    }
    let mut c = s.source.get_byte();
    if c == END_OF_STREAM {
        return c;
    }
    if s.encoding == ISO2022 {
        if c == 0x1b {
            s.state = Iso2022State::FsmEsc;
            return c;
        }
        match s.state {
            Iso2022State::FsmEsc => {
                if c == '$' as u32 {
                    s.state = Iso2022State::FsmEscd;
                } else if c == '(' as u32 {
                    s.state = Iso2022State::FsmEscp;
                } else {
                    s.state = Iso2022State::FsmAscii;
                }
            }
            Iso2022State::FsmEscd => {
                if c == '(' as u32 {
                    s.state = Iso2022State::FsmEscdp;
                } else {
                    s.state = Iso2022State::FsmNonascii;
                }
            }
            Iso2022State::FsmEscdp => s.state = Iso2022State::FsmNonascii,
            Iso2022State::FsmEscp => s.state = Iso2022State::FsmAscii,
            Iso2022State::FsmNonascii => c |= 0x80,
            Iso2022State::FsmAscii => {}
        }
        return c;
    }
    if s.encoding == UTF16LE {
        let c1 = s.source.get_byte();
        if c1 == END_OF_STREAM {
            return END_OF_STREAM;
        }
        return (c1 << 8) + c;
    }
    if s.encoding == UTF16 || s.encoding == UTF16BE {
        let c1 = s.source.get_byte();
        if c1 == END_OF_STREAM {
            return END_OF_STREAM;
        }
        return (c << 8) + c1;
    }
    if s.encoding == UTF8 {
        let (err, mut n, count) = decode_utf8_bytes_to_char(c, Successors::Source(&mut s.source));
        if !err && n == END_OF_STREAM && count == 1 {
            return END_OF_STREAM;
        } else if err {
            mark_position(doc, sel);
            message::report_encoding_error(doc, INVALID_UTF8, n, false);
            n = 0xFFFD;
        }
        return n;
    }
    if s.encoding == BIG5 || s.encoding == SHIFTJIS {
        if c < 128 {
            return c;
        } else if s.encoding == SHIFTJIS && (0xa1..=0xdf).contains(&c) {
            return c;
        } else {
            let c1 = s.source.get_byte();
            if c1 == END_OF_STREAM {
                return END_OF_STREAM;
            }
            return (c << 8) + c1;
        }
    }
    c
}

/// WriteChar()
pub fn write_char(c: u32, out: &mut StreamOut) {
    let mut c = c;
    if c == LF {
        if out.nl == TIDY_CRLF {
            write_char(CR, out);
        } else if out.nl == TIDY_CR {
            c = CR;
        }
    }
    match out.encoding {
        MACROMAN => encode_mac_roman(c, out),
        WIN1252 => encode_win1252(c, out),
        IBM858 => encode_ibm858(c, out),
        LATIN0 => encode_latin0(c, out),
        UTF8 => {
            let (err, buf, count) = encode_char_to_utf8_bytes(c);
            if !err {
                for &b in &buf[..count as usize] {
                    out.put_byte(b as u32);
                }
            }
            if count <= 0 {
                out.put_byte(0xEF);
                out.put_byte(0xBF);
                out.put_byte(0xBF);
            }
        }
        ISO2022 => {
            if c == 0x1b {
                out.state = Iso2022State::FsmEsc;
            } else {
                match out.state {
                    Iso2022State::FsmEsc => {
                        if c == '$' as u32 {
                            out.state = Iso2022State::FsmEscd;
                        } else if c == '(' as u32 {
                            out.state = Iso2022State::FsmEscp;
                        } else {
                            out.state = Iso2022State::FsmAscii;
                        }
                    }
                    Iso2022State::FsmEscd => {
                        if c == '(' as u32 {
                            out.state = Iso2022State::FsmEscdp;
                        } else {
                            out.state = Iso2022State::FsmNonascii;
                        }
                    }
                    Iso2022State::FsmEscdp => out.state = Iso2022State::FsmNonascii,
                    Iso2022State::FsmEscp => out.state = Iso2022State::FsmAscii,
                    Iso2022State::FsmNonascii => c &= 0x7F,
                    Iso2022State::FsmAscii => {}
                }
            }
            out.put_byte(c);
        }
        UTF16LE | UTF16BE | UTF16 => {
            let mut the_chars = [0u32; 2];
            let mut num_chars = 1;
            if !is_valid_utf16_from_ucs4(c) {
                num_chars = 0;
            } else if is_combined_char(c) {
                num_chars = 2;
                match split_surrogate_pair(c) {
                    Some((lo, hi)) => {
                        the_chars[0] = lo;
                        the_chars[1] = hi;
                    }
                    None => num_chars = 0,
                }
            } else {
                the_chars[0] = c;
            }
            for &ch in &the_chars[..num_chars] {
                if out.encoding == UTF16LE {
                    out.put_byte(ch & 0xFF);
                    out.put_byte((ch >> 8) & 0xFF);
                } else {
                    out.put_byte((ch >> 8) & 0xFF);
                    out.put_byte(ch & 0xFF);
                }
            }
        }
        BIG5 | SHIFTJIS => {
            if c < 128 {
                out.put_byte(c);
            } else {
                out.put_byte((c >> 8) & 0xFF);
                out.put_byte(c & 0xFF);
            }
        }
        _ => out.put_byte(c),
    }
}

/// outBOM()
pub fn out_bom(out: &mut StreamOut) {
    if out.encoding == UTF8
        || out.encoding == UTF16LE
        || out.encoding == UTF16BE
        || out.encoding == UTF16
    {
        write_char(UNICODE_BOM, out);
    }
}

const WIN2UNICODE: [u32; 32] = [
    0x20AC, 0x0000, 0x201A, 0x0192, 0x201E, 0x2026, 0x2020, 0x2021, 0x02C6, 0x2030, 0x0160, 0x2039,
    0x0152, 0x0000, 0x017D, 0x0000, 0x0000, 0x2018, 0x2019, 0x201C, 0x201D, 0x2022, 0x2013, 0x2014,
    0x02DC, 0x2122, 0x0161, 0x203A, 0x0153, 0x0000, 0x017E, 0x0178,
];

/// DecodeWin1252()
pub fn decode_win1252(c: u32) -> u32 {
    if 127 < c && c < 160 {
        WIN2UNICODE[(c - 128) as usize]
    } else {
        c
    }
}

fn encode_win1252(c: u32, out: &mut StreamOut) {
    if c < 128 || (c > 159 && c < 256) {
        out.put_byte(c);
    } else if let Some(i) = WIN2UNICODE.iter().position(|&u| u == c) {
        out.put_byte(i as u32 + 128);
    }
}

const MAC2UNICODE: [u32; 128] = [
    0x00C4, 0x00C5, 0x00C7, 0x00C9, 0x00D1, 0x00D6, 0x00DC, 0x00E1, 0x00E0, 0x00E2, 0x00E4, 0x00E3,
    0x00E5, 0x00E7, 0x00E9, 0x00E8, 0x00EA, 0x00EB, 0x00ED, 0x00EC, 0x00EE, 0x00EF, 0x00F1, 0x00F3,
    0x00F2, 0x00F4, 0x00F6, 0x00F5, 0x00FA, 0x00F9, 0x00FB, 0x00FC, 0x2020, 0x00B0, 0x00A2, 0x00A3,
    0x00A7, 0x2022, 0x00B6, 0x00DF, 0x00AE, 0x00A9, 0x2122, 0x00B4, 0x00A8, 0x2260, 0x00C6, 0x00D8,
    0x221E, 0x00B1, 0x2264, 0x2265, 0x00A5, 0x00B5, 0x2202, 0x2211, 0x220F, 0x03C0, 0x222B, 0x00AA,
    0x00BA, 0x03A9, 0x00E6, 0x00F8, 0x00BF, 0x00A1, 0x00AC, 0x221A, 0x0192, 0x2248, 0x2206, 0x00AB,
    0x00BB, 0x2026, 0x00A0, 0x00C0, 0x00C3, 0x00D5, 0x0152, 0x0153, 0x2013, 0x2014, 0x201C, 0x201D,
    0x2018, 0x2019, 0x00F7, 0x25CA, 0x00FF, 0x0178, 0x2044, 0x20AC, 0x2039, 0x203A, 0xFB01, 0xFB02,
    0x2021, 0x00B7, 0x201A, 0x201E, 0x2030, 0x00C2, 0x00CA, 0x00C1, 0x00CB, 0x00C8, 0x00CD, 0x00CE,
    0x00CF, 0x00CC, 0x00D3, 0x00D4, 0xF8FF, 0x00D2, 0x00DA, 0x00DB, 0x00D9, 0x0131, 0x02C6, 0x02DC,
    0x00AF, 0x02D8, 0x02D9, 0x02DA, 0x00B8, 0x02DD, 0x02DB, 0x02C7,
];

/// DecodeMacRoman()
pub fn decode_mac_roman(c: u32) -> u32 {
    if 127 < c && c < 256 {
        MAC2UNICODE[(c - 128) as usize]
    } else {
        c
    }
}

fn encode_mac_roman(c: u32, out: &mut StreamOut) {
    if c < 128 {
        out.put_byte(c);
    } else if let Some(i) = MAC2UNICODE.iter().position(|&u| u == c) {
        out.put_byte(i as u32 + 128);
    }
}

const IBM2UNICODE: [u32; 128] = [
    0x00C7, 0x00FC, 0x00E9, 0x00E2, 0x00E4, 0x00E0, 0x00E5, 0x00E7, 0x00EA, 0x00EB, 0x00E8, 0x00EF,
    0x00EE, 0x00EC, 0x00C4, 0x00C5, 0x00C9, 0x00E6, 0x00C6, 0x00F4, 0x00F6, 0x00F2, 0x00FB, 0x00F9,
    0x00FF, 0x00D6, 0x00DC, 0x00F8, 0x00A3, 0x00D8, 0x00D7, 0x0192, 0x00E1, 0x00ED, 0x00F3, 0x00FA,
    0x00F1, 0x00D1, 0x00AA, 0x00BA, 0x00BF, 0x00AE, 0x00AC, 0x00BD, 0x00BC, 0x00A1, 0x00AB, 0x00BB,
    0x2591, 0x2592, 0x2593, 0x2502, 0x2524, 0x00C1, 0x00C2, 0x00C0, 0x00A9, 0x2563, 0x2551, 0x2557,
    0x255D, 0x00A2, 0x00A5, 0x2510, 0x2514, 0x2534, 0x252C, 0x251C, 0x2500, 0x253C, 0x00E3, 0x00C3,
    0x255A, 0x2554, 0x2569, 0x2566, 0x2560, 0x2550, 0x256c, 0x00a4, 0x00f0, 0x00d0, 0x00ca, 0x00cb,
    0x00c8, 0x20AC, 0x00cd, 0x00ce, 0x00cf, 0x2518, 0x250c, 0x2588, 0x2584, 0x00a6, 0x00cc, 0x2580,
    0x00d3, 0x00df, 0x00d4, 0x00d2, 0x00f5, 0x00d5, 0x00b5, 0x00fe, 0x00de, 0x00da, 0x00db, 0x00d9,
    0x00fd, 0x00dd, 0x00af, 0x00b4, 0x00ad, 0x00b1, 0x2017, 0x00be, 0x00b6, 0x00a7, 0x00f7, 0x00b8,
    0x00b0, 0x00a8, 0x00b7, 0x00b9, 0x00b3, 0x00b2, 0x25a0, 0x00a0,
];

fn decode_ibm850(c: u32) -> u32 {
    if 127 < c && c < 256 {
        IBM2UNICODE[(c - 128) as usize]
    } else {
        c
    }
}

fn encode_ibm858(c: u32, out: &mut StreamOut) {
    if c < 128 {
        out.put_byte(c);
    } else if let Some(i) = IBM2UNICODE.iter().position(|&u| u == c) {
        out.put_byte(i as u32 + 128);
    }
}

fn decode_latin0(c: u32) -> u32 {
    if 163 < c && c < 191 {
        match c {
            0xA4 => 0x20AC,
            0xA6 => 0x0160,
            0xA8 => 0x0161,
            0xB4 => 0x017D,
            0xB8 => 0x017E,
            0xBC => 0x0152,
            0xBD => 0x0153,
            0xBE => 0x0178,
            _ => c,
        }
    } else {
        c
    }
}

fn encode_latin0(c: u32, out: &mut StreamOut) {
    let c = match c {
        0x20AC => 0xA4,
        0x0160 => 0xA6,
        0x0161 => 0xA8,
        0x017D => 0xB4,
        0x017E => 0xB8,
        0x0152 => 0xBC,
        0x0153 => 0xBD,
        0x0178 => 0xBE,
        _ => c,
    };
    out.put_byte(c);
}

/// enc2iana: (id, iana name, tidy option name)
pub const ENC2IANA: &[(u32, Option<&str>, &str)] = &[
    (ASCII, Some("us-ascii"), "ascii"),
    (LATIN0, Some("iso-8859-15"), "latin0"),
    (LATIN1, Some("iso-8859-1"), "latin1"),
    (UTF8, Some("utf-8"), "utf8"),
    (MACROMAN, Some("macintosh"), "mac"),
    (WIN1252, Some("windows-1252"), "win1252"),
    (IBM858, Some("ibm00858"), "ibm858"),
    (UTF16LE, Some("utf-16"), "utf16le"),
    (UTF16BE, Some("utf-16"), "utf16be"),
    (UTF16, Some("utf-16"), "utf16"),
    (BIG5, Some("big5"), "big5"),
    (SHIFTJIS, Some("shift_jis"), "shiftjis"),
    (ISO2022, None, "iso2022"),
    (RAW, None, "raw"),
];

/// GetEncodingNameFromTidyId()
pub fn get_encoding_name_from_tidy_id(id: u32) -> Option<&'static str> {
    for &(eid, name, _) in ENC2IANA {
        name?;
        if eid == id {
            return name;
        }
    }
    None
}

/// GetEncodingOptNameFromTidyId()
pub fn get_encoding_opt_name_from_tidy_id(id: u32) -> Option<&'static str> {
    ENC2IANA.iter().find(|e| e.0 == id).map(|e| e.2)
}

/// GetCharEncodingFromOptName()
pub fn get_char_encoding_from_opt_name(charenc: &[u8]) -> i32 {
    for &(id, _, opt) in ENC2IANA {
        if crate::tmbstr::tmbstrcasecmp(charenc, opt.as_bytes()) == 0 {
            return id as i32;
        }
    }
    -1
}
