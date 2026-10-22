//! pprint.c: the pretty printer (indentation, wrapping, entities), writing
//! to `doc.doc_out`.

use std::cell::Cell;

use crate::attrs;
use crate::config;
use crate::doc::*;
use crate::entities;
use crate::enums::{AttrId, OptId, TagId};
use crate::lexer;
use crate::parser;
use crate::streamio;
use crate::tags;
use crate::tmbstr;
use crate::utf8;

pub const NORMAL: u32 = 0;
pub const PREFORMATTED: u32 = 1;
pub const COMMENT: u32 = 2;
pub const ATTRIBVALUE: u32 = 4;
pub const NOWRAP: u32 = 8;
pub const CDATA: u32 = 16;

/// `OtherNamespace` (a GetTokenMode value, used by pprint.c as a print mode)
const OTHER_NAMESPACE: u32 = 4;

/// TidyIndent
#[derive(Clone, Copy, Debug, Default)]
pub struct TidyIndent {
    pub spaces: i32,
    pub attr_val_start: i32,
    pub attr_string_start: i32,
}

/// TidyPrintImpl
#[derive(Clone, Debug, Default)]
pub struct PrintImpl {
    /// `linebuf` (`lbufsize` is its length, `linelen` the used length)
    pub linebuf: Vec<u32>,
    pub linelen: u32,
    pub wraphere: u32,
    pub line: u32,
    pub ix_ind: u32,
    pub indent: [TidyIndent; 2],
}

thread_local! {
    /// tidy's static `indent_char`
    static INDENT_CHAR: Cell<u32> = const { Cell::new(' ' as u32) };
}

/// PPrintTabs(): indent with tabs (process-wide, like tidy's static)
pub fn pprint_tabs() {
    INDENT_CHAR.with(|c| c.set('\t' as u32));
}

/// PPrintSpaces(): indent with spaces (process-wide, like tidy's static)
pub fn pprint_spaces() {
    INDENT_CHAR.with(|c| c.set(' ' as u32));
}

// ---- Unicode wrap points

const UCPC: u8 = 1;
const UCPD: u8 = 2;
const UCPE: u8 = 3;
const UCPS: u8 = 4;
const UCPI: u8 = 5;
const UCPF: u8 = 6;
const UCPO: u8 = 7;
const UCZS: u8 = 8;
const UCZL: u8 = 9;
const UCZP: u8 = 10;

static UNICODE4CAT: &[(u32, u8)] = &[
    (0x2000, UCZS),
    (0x2001, UCZS),
    (0x2002, UCZS),
    (0x2003, UCZS),
    (0x2004, UCZS),
    (0x2005, UCZS),
    (0x2006, UCZS),
    (0x2008, UCZS),
    (0x2009, UCZS),
    (0x200A, UCZS),
    (0x2010, UCPD),
    (0x2012, UCPD),
    (0x2013, UCPD),
    (0x2014, UCPD),
    (0x2015, UCPD),
    (0x2016, UCPO),
    (0x2017, UCPO),
    (0x2018, UCPI),
    (0x2019, UCPF),
    (0x201A, UCPS),
    (0x201B, UCPI),
    (0x201C, UCPI),
    (0x201D, UCPF),
    (0x201E, UCPS),
    (0x201F, UCPI),
    (0x2020, UCPO),
    (0x2021, UCPO),
    (0x2022, UCPO),
    (0x2023, UCPO),
    (0x2024, UCPO),
    (0x2025, UCPO),
    (0x2026, UCPO),
    (0x2027, UCPO),
    (0x2028, UCZL),
    (0x2029, UCZP),
    (0x2030, UCPO),
    (0x2031, UCPO),
    (0x2032, UCPO),
    (0x2033, UCPO),
    (0x2034, UCPO),
    (0x2035, UCPO),
    (0x2036, UCPO),
    (0x2037, UCPO),
    (0x2038, UCPO),
    (0x2039, UCPI),
    (0x203A, UCPF),
    (0x203B, UCPO),
    (0x203C, UCPO),
    (0x203D, UCPO),
    (0x203E, UCPO),
    (0x203F, UCPC),
    (0x2040, UCPC),
    (0x2041, UCPO),
    (0x2042, UCPO),
    (0x2043, UCPO),
    (0x2045, UCPS),
    (0x2046, UCPE),
    (0x2047, UCPO),
    (0x2048, UCPO),
    (0x2049, UCPO),
    (0x204A, UCPO),
    (0x204B, UCPO),
    (0x204C, UCPO),
    (0x204D, UCPO),
    (0x204E, UCPO),
    (0x204F, UCPO),
    (0x2050, UCPO),
    (0x2051, UCPO),
    (0x2053, UCPO),
    (0x2054, UCPC),
    (0x2057, UCPO),
    (0x205F, UCZS),
    (0x207D, UCPS),
    (0x207E, UCPE),
    (0x208D, UCPS),
    (0x208E, UCPE),
    (0x2329, UCPS),
    (0x232A, UCPE),
    (0x23B4, UCPS),
    (0x23B5, UCPE),
    (0x23B6, UCPO),
    (0x2768, UCPS),
    (0x2769, UCPE),
    (0x276A, UCPS),
    (0x276B, UCPE),
    (0x276C, UCPS),
    (0x276D, UCPE),
    (0x276E, UCPS),
    (0x276F, UCPE),
    (0x2770, UCPS),
    (0x2771, UCPE),
    (0x2772, UCPS),
    (0x2773, UCPE),
    (0x2774, UCPS),
    (0x2775, UCPE),
    (0x27E6, UCPS),
    (0x27E7, UCPE),
    (0x27E8, UCPS),
    (0x27E9, UCPE),
    (0x27EA, UCPS),
    (0x27EB, UCPE),
    (0x2983, UCPS),
    (0x2984, UCPE),
    (0x2985, UCPS),
    (0x2986, UCPE),
    (0x2987, UCPS),
    (0x2988, UCPE),
    (0x2989, UCPS),
    (0x298A, UCPE),
    (0x298B, UCPS),
    (0x298C, UCPE),
    (0x298D, UCPS),
    (0x298E, UCPE),
    (0x298F, UCPS),
    (0x2990, UCPE),
    (0x2991, UCPS),
    (0x2992, UCPE),
    (0x2993, UCPS),
    (0x2994, UCPE),
    (0x2995, UCPS),
    (0x2996, UCPE),
    (0x2997, UCPS),
    (0x2998, UCPE),
    (0x29D8, UCPS),
    (0x29D9, UCPE),
    (0x29DA, UCPS),
    (0x29DB, UCPE),
    (0x29FC, UCPS),
    (0x29FD, UCPE),
    (0x3001, UCPO),
    (0x3002, UCPO),
    (0x3003, UCPO),
    (0x3008, UCPS),
    (0x3009, UCPE),
    (0x300A, UCPS),
    (0x300B, UCPE),
    (0x300C, UCPS),
    (0x300D, UCPE),
    (0x300E, UCPS),
    (0x300F, UCPE),
    (0x3010, UCPS),
    (0x3011, UCPE),
    (0x3014, UCPS),
    (0x3015, UCPE),
    (0x3016, UCPS),
    (0x3017, UCPE),
    (0x3018, UCPS),
    (0x3019, UCPE),
    (0x301A, UCPS),
    (0x301B, UCPE),
    (0x301C, UCPD),
    (0x301D, UCPS),
    (0x301E, UCPE),
    (0x301F, UCPE),
    (0x3030, UCPD),
    (0x303D, UCPO),
    (0x30A0, UCPD),
    (0x30FB, UCPC),
    (0xFD3E, UCPS),
    (0xFD3F, UCPE),
    (0xFE30, UCPO),
    (0xFE31, UCPD),
    (0xFE32, UCPD),
    (0xFE33, UCPC),
    (0xFE34, UCPC),
    (0xFE35, UCPS),
    (0xFE36, UCPE),
    (0xFE37, UCPS),
    (0xFE38, UCPE),
    (0xFE39, UCPS),
    (0xFE3A, UCPE),
    (0xFE3B, UCPS),
    (0xFE3C, UCPE),
    (0xFE3D, UCPS),
    (0xFE3E, UCPE),
    (0xFE3F, UCPS),
    (0xFE40, UCPE),
    (0xFE41, UCPS),
    (0xFE42, UCPE),
    (0xFE43, UCPS),
    (0xFE44, UCPE),
    (0xFE45, UCPO),
    (0xFE46, UCPO),
    (0xFE47, UCPS),
    (0xFE48, UCPE),
    (0xFE49, UCPO),
    (0xFE4A, UCPO),
    (0xFE4B, UCPO),
    (0xFE4C, UCPO),
    (0xFE4D, UCPC),
    (0xFE4E, UCPC),
    (0xFE4F, UCPC),
    (0xFE50, UCPO),
    (0xFE51, UCPO),
    (0xFE52, UCPO),
    (0xFE54, UCPO),
    (0xFE55, UCPO),
    (0xFE56, UCPO),
    (0xFE57, UCPO),
    (0xFE58, UCPD),
    (0xFE59, UCPS),
    (0xFE5A, UCPE),
    (0xFE5B, UCPS),
    (0xFE5C, UCPE),
    (0xFE5D, UCPS),
    (0xFE5E, UCPE),
    (0xFE5F, UCPO),
    (0xFE60, UCPO),
    (0xFE61, UCPO),
    (0xFE63, UCPD),
    (0xFE68, UCPO),
    (0xFE6A, UCPO),
    (0xFE6B, UCPO),
    (0xFF01, UCPO),
    (0xFF02, UCPO),
    (0xFF03, UCPO),
    (0xFF05, UCPO),
    (0xFF06, UCPO),
    (0xFF07, UCPO),
    (0xFF08, UCPS),
    (0xFF09, UCPE),
    (0xFF0A, UCPO),
    (0xFF0C, UCPO),
    (0xFF0D, UCPD),
    (0xFF0E, UCPO),
    (0xFF0F, UCPO),
    (0xFF1A, UCPO),
    (0xFF1B, UCPO),
    (0xFF1F, UCPO),
    (0xFF20, UCPO),
    (0xFF3B, UCPS),
    (0xFF3C, UCPO),
    (0xFF3D, UCPE),
    (0xFF3F, UCPC),
    (0xFF5B, UCPS),
    (0xFF5D, UCPE),
    (0xFF5F, UCPS),
    (0xFF60, UCPE),
    (0xFF61, UCPO),
    (0xFF62, UCPS),
    (0xFF63, UCPE),
    (0xFF64, UCPO),
    (0xFF65, UCPC),
    (0x10100, UCPO),
    (0x10101, UCPO),
    (0x1039F, UCPO),
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum WrapPoint {
    None,
    Before,
    After,
}

/// CharacterWrapPoint()
fn character_wrap_point(c: u32) -> WrapPoint {
    for &(code, cat) in UNICODE4CAT {
        if code > c {
            break;
        }
        if code == c {
            if cat == UCPS || cat == UCPI {
                return WrapPoint::Before;
            } else {
                return WrapPoint::After;
            }
        }
    }
    WrapPoint::None
}

/// Big5WrapPoint()
fn big5_wrap_point(c: u32) -> WrapPoint {
    if (c & 0xFF00) == 0xA100 {
        if c > 0xA15C && c < 0xA1AD && (c & 1) == 1 {
            return WrapPoint::Before;
        }
        return WrapPoint::After;
    }
    WrapPoint::None
}

fn init_indent(ind: &mut TidyIndent) {
    ind.spaces = -1;
    ind.attr_val_start = -1;
    ind.attr_string_start = -1;
}

/// InitPrintBuf()
pub fn init_print_buf(doc: &mut Doc) {
    doc.pprint = PrintImpl::default();
    init_indent(&mut doc.pprint.indent[0]);
    init_indent(&mut doc.pprint.indent[1]);
    doc.pprint.line = 0;
}

/// FreePrintBuf()
pub fn free_print_buf(doc: &mut Doc) {
    init_print_buf(doc);
}

impl PrintImpl {
    /// expand()
    fn expand(&mut self, len: u32) {
        let mut buflen = self.linebuf.len() as u32;
        if buflen == 0 {
            buflen = 256;
        }
        while len >= buflen {
            buflen *= 2;
        }
        self.linebuf.resize(buflen as usize, 0);
    }

    fn get_spaces(&self) -> u32 {
        let spaces = self.indent[0].spaces;
        if spaces < 0 { 0 } else { spaces as u32 }
    }

    fn clear_in_string(&mut self) -> i32 {
        let ix = self.ix_ind as usize;
        self.indent[ix].attr_string_start = -1;
        -1
    }

    fn toggle_in_string(&mut self) -> i32 {
        let ix = self.ix_ind as usize;
        let in_string = self.indent[ix].attr_string_start >= 0;
        let v = if in_string { -1 } else { self.linelen as i32 };
        self.indent[ix].attr_string_start = v;
        v
    }

    fn is_in_string(&self) -> bool {
        let ind = &self.indent[0];
        ind.attr_string_start >= 0 && ind.attr_string_start < self.linelen as i32
    }

    fn is_wrap_in_string(&self) -> bool {
        let ind = &self.indent[0];
        let wrap = self.wraphere as i32;
        ind.attr_string_start == 0 || (ind.attr_string_start > 0 && ind.attr_string_start < wrap)
    }

    fn clear_in_attr_val(&mut self) {
        let ix = self.ix_ind as usize;
        self.indent[ix].attr_val_start = -1;
    }

    fn set_in_attr_val(&mut self) -> i32 {
        let ix = self.ix_ind as usize;
        self.indent[ix].attr_val_start = self.linelen as i32;
        self.linelen as i32
    }

    fn is_wrap_in_attr_val(&self) -> bool {
        let ind = &self.indent[0];
        let wrap = self.wraphere as i32;
        ind.attr_val_start == 0 || (ind.attr_val_start > 0 && ind.attr_val_start < wrap)
    }

    /// AddC()
    fn add_c(&mut self, c: u32, string_index: u32) {
        if string_index + 1 >= self.linebuf.len() as u32 {
            self.expand(string_index + 1);
        }
        self.linebuf[string_index as usize] = c;
    }

    /// AddChar()
    fn add_char(&mut self, c: u32) -> u32 {
        self.add_c(c, self.linelen);
        self.linelen += 1;
        self.linelen
    }

    /// AddAsciiString(): bytes are (signed) `char`s widened to uint
    fn add_ascii_string(&mut self, s: &[u8], string_index: u32) -> u32 {
        let s = tmbstr::cstr(s);
        let len = s.len() as u32;
        if string_index + len >= self.linebuf.len() as u32 {
            self.expand(string_index + len);
        }
        for (ix, &b) in s.iter().enumerate() {
            self.linebuf[string_index as usize + ix] = (b as i8) as i32 as u32;
        }
        string_index + len
    }

    /// AddString()
    fn add_string(&mut self, s: &[u8]) -> u32 {
        self.linelen = self.add_ascii_string(s, self.linelen);
        self.linelen
    }

    /// ResetLine(): reset indent state after flushing a new line
    fn reset_line(&mut self) {
        if self.ix_ind > 0 {
            self.indent[0] = self.indent[1];
            init_indent(&mut self.indent[1]);
        }
        if self.wraphere > 0 {
            let wrap = self.wraphere as i32;
            if self.indent[0].attr_string_start > wrap {
                self.indent[0].attr_string_start -= wrap;
            }
            if self.indent[0].attr_val_start > wrap {
                self.indent[0].attr_val_start -= wrap;
            }
        } else {
            if self.indent[0].attr_string_start > 0 {
                self.indent[0].attr_string_start = 0;
            }
            if self.indent[0].attr_val_start > 0 {
                self.indent[0].attr_val_start = 0;
            }
        }
        self.wraphere = 0;
        self.ix_ind = 0;
    }

    /// ResetLineAfterWrap(): shift text after wrap point to beginning of
    /// next line
    fn reset_line_after_wrap(&mut self) {
        if self.linelen > self.wraphere {
            let mut q = self.wraphere as usize;
            let end = self.linelen as usize;
            if !self.is_wrap_in_attr_val() {
                while q < end && self.linebuf[q] == ' ' as u32 {
                    q += 1;
                    self.wraphere += 1;
                }
            }
            let mut p = 0usize;
            while q < end {
                self.linebuf[p] = self.linebuf[q];
                p += 1;
                q += 1;
            }
            self.linelen -= self.wraphere;
        } else {
            self.linelen = 0;
        }
        self.reset_line();
    }
}

fn wc(doc: &mut Doc, c: u32) {
    streamio::write_char(c, doc.doc_out.as_mut().expect("tidy: no output stream"));
}

/// TidyClassicVS
fn tidy_classic_vs(doc: &Doc) -> bool {
    doc.cfg_auto_bool(OptId::VertSpace) == TIDY_YES_STATE
}

/// TidyAddVS
fn tidy_add_vs(doc: &Doc) -> bool {
    doc.cfg_auto_bool(OptId::VertSpace) != TIDY_AUTO_STATE
}

/// HasMixedContent()
fn has_mixed_content(doc: &Doc, element: Option<NodeId>) -> bool {
    let Some(e) = element else {
        return false;
    };
    let mut node = doc[e].content;
    while let Some(n) = node {
        if tags::node_is_text(doc, Some(n)) {
            return true;
        }
        node = doc[n].next;
    }
    false
}

/// WantIndent()
fn want_indent(doc: &Doc) -> bool {
    let pprint = &doc.pprint;
    let mut want_it = pprint.get_spaces() > 0;
    if want_it {
        let indent_attrs = doc.cfg_bool(OptId::IndentAttributes);
        want_it = (!pprint.is_wrap_in_attr_val() || indent_attrs) && !pprint.is_wrap_in_string();
    }
    want_it
}

/// WrapOff()
fn wrap_off(doc: &mut Doc) -> u32 {
    let save_wrap = doc.cfg(OptId::WrapLen);
    config::set_option_int(doc, OptId::WrapLen, 0xFFFF_FFFF);
    save_wrap
}

/// WrapOn()
fn wrap_on(doc: &mut Doc, save_wrap: u32) {
    config::set_option_int(doc, OptId::WrapLen, save_wrap as u64);
}

/// WrapOffCond()
fn wrap_off_cond(doc: &mut Doc, onoff: bool) -> u32 {
    if onoff {
        return wrap_off(doc);
    }
    doc.cfg(OptId::WrapLen)
}

/// SetWrap(): saves current output point as the wrap point, but only if
/// indentation would NOT overflow the current line.
fn set_wrap(doc: &mut Doc, indent: u32) -> bool {
    let wraplen = doc.cfg(OptId::WrapLen);
    let pprint = &mut doc.pprint;
    let wrap = indent.wrapping_add(pprint.linelen) < wraplen;
    if wrap {
        if pprint.indent[0].spaces < 0 {
            pprint.indent[0].spaces = indent as i32;
        }
        pprint.wraphere = pprint.linelen;
    } else if pprint.ix_ind == 0 {
        pprint.indent[1].spaces = indent as i32;
        pprint.ix_ind = 1;
    }
    wrap
}

/// CarryOver()
fn carry_over(val_to: &mut i32, val_from: &mut i32, wrap_point: u32) {
    if *val_from > wrap_point as i32 {
        *val_to = *val_from - wrap_point as i32;
        *val_from = -1;
    }
}

/// SetWrapAttr()
fn set_wrap_attr(doc: &mut Doc, indent: u32, attr_start: i32, str_start: i32) -> bool {
    let wraplen = doc.cfg(OptId::WrapLen);
    let pprint = &mut doc.pprint;
    let wrap = indent.wrapping_add(pprint.linelen) < wraplen;
    if wrap {
        if pprint.indent[0].spaces < 0 {
            pprint.indent[0].spaces = indent as i32;
        }
        pprint.wraphere = pprint.linelen;
    } else if pprint.ix_ind == 0 {
        pprint.indent[1].spaces = indent as i32;
        pprint.ix_ind = 1;
        let wh = pprint.wraphere;
        let [i0, i1] = &mut pprint.indent;
        carry_over(&mut i1.attr_string_start, &mut i0.attr_string_start, wh);
        carry_over(&mut i1.attr_val_start, &mut i0.attr_val_start, wh);
    }
    let ix = pprint.ix_ind as usize;
    pprint.indent[ix].attr_val_start = attr_start;
    pprint.indent[ix].attr_string_start = str_start;
    wrap
}

/// WriteIndentChar()
fn write_indent_char(doc: &mut Doc) {
    let mut spaces = doc.pprint.get_spaces();
    let tabsize = doc.cfg(OptId::TabSize);
    let indent_char = INDENT_CHAR.with(|c| c.get());
    if spaces != 0 && indent_char == '\t' as u32 && tabsize != 0 {
        spaces /= tabsize;
        if spaces == 0 {
            spaces = 1;
        }
    }
    for _ in 0..spaces {
        wc(doc, indent_char);
    }
}

/// WrapLine(): write the current line up to the saved wrap point
fn wrap_line(doc: &mut Doc) {
    if doc.pprint.wraphere == 0 {
        return;
    }
    if want_indent(doc) {
        write_indent_char(doc);
    }
    for i in 0..doc.pprint.wraphere as usize {
        let c = doc.pprint.linebuf[i];
        wc(doc, c);
    }
    if doc.pprint.is_wrap_in_string() {
        wc(doc, '\\' as u32);
    }
    wc(doc, '\n' as u32);
    doc.pprint.line += 1;
    doc.pprint.reset_line_after_wrap();
}

/// CheckWrapLine()
fn check_wrap_line(doc: &mut Doc) -> bool {
    if doc.pprint.get_spaces() + doc.pprint.linelen >= doc.cfg(OptId::WrapLen) {
        wrap_line(doc);
        return true;
    }
    false
}

/// CheckWrapIndent()
fn check_wrap_indent(doc: &mut Doc, indent: u32) -> bool {
    if doc.pprint.get_spaces() + doc.pprint.linelen >= doc.cfg(OptId::WrapLen) {
        wrap_line(doc);
        if doc.pprint.indent[0].spaces < 0 {
            doc.pprint.indent[0].spaces = indent as i32;
        }
        return true;
    }
    false
}

/// WrapAttrVal()
fn wrap_attr_val(doc: &mut Doc) {
    if want_indent(doc) {
        write_indent_char(doc);
    }
    for i in 0..doc.pprint.wraphere as usize {
        let c = doc.pprint.linebuf[i];
        wc(doc, c);
    }
    if doc.pprint.is_wrap_in_string() {
        wc(doc, '\\' as u32);
    } else {
        wc(doc, ' ' as u32);
    }
    wc(doc, '\n' as u32);
    doc.pprint.line += 1;
    doc.pprint.reset_line_after_wrap();
}

/// PFlushLineImpl()
fn pflush_line_impl(doc: &mut Doc) {
    check_wrap_line(doc);
    if want_indent(doc) {
        write_indent_char(doc);
    }
    for i in 0..doc.pprint.linelen as usize {
        let c = doc.pprint.linebuf[i];
        wc(doc, c);
    }
    if doc.pprint.is_in_string() {
        wc(doc, '\\' as u32);
    }
    doc.pprint.reset_line();
    doc.pprint.linelen = 0;
}

fn set_indent(doc: &mut Doc, indent: u32) {
    if doc.pprint.indent[0].spaces != indent as i32 {
        doc.pprint.indent[0].spaces = indent as i32;
    }
}

/// PFlushLine()
pub fn pflush_line(doc: &mut Doc, indent: u32) {
    if doc.pprint.linelen > 0 {
        pflush_line_impl(doc);
    }
    wc(doc, '\n' as u32);
    doc.pprint.line += 1;
    set_indent(doc, indent);
}

/// PCondFlushLine()
fn pcond_flush_line(doc: &mut Doc, indent: u32) {
    if doc.pprint.linelen > 0 {
        pflush_line_impl(doc);
        wc(doc, '\n' as u32);
        doc.pprint.line += 1;
    }
    set_indent(doc, indent);
}

/// PFlushLineSmart(): only writes a newline unless `vertical-space auto`
pub fn pflush_line_smart(doc: &mut Doc, indent: u32) {
    if doc.pprint.linelen > 0 {
        pflush_line_impl(doc);
    }
    if tidy_add_vs(doc) {
        wc(doc, '\n' as u32);
        doc.pprint.line += 1;
    }
    set_indent(doc, indent);
}

/// PCondFlushLineSmart()
fn pcond_flush_line_smart(doc: &mut Doc, indent: u32) {
    if doc.pprint.linelen > 0 {
        pflush_line_impl(doc);
        if tidy_add_vs(doc) {
            wc(doc, '\n' as u32);
            doc.pprint.line += 1;
        }
    }
    set_indent(doc, indent);
}

fn add_entity(doc: &mut Doc, name: &[u8]) {
    let mut e = Vec::with_capacity(name.len() + 2);
    e.push(b'&');
    e.extend_from_slice(name);
    e.push(b';');
    doc.pprint.add_string(&e);
}

fn add_ncr(doc: &mut Doc, c: u32) {
    let e = format!("&#{};", c);
    doc.pprint.add_string(e.as_bytes());
}

/// PPrintChar()
fn pprint_char(doc: &mut Doc, c: u32, mode: u32) {
    let outenc = doc.cfg(OptId::OutCharEncoding);
    let qmark = doc.cfg_bool(OptId::QuoteMarks);

    if c == ' ' as u32 && (mode & (PREFORMATTED | COMMENT | ATTRIBVALUE | CDATA)) == 0 {
        // coerce a space character to a non-breaking space
        if mode & NOWRAP != 0 {
            let ent: &[u8] = if doc.cfg_bool(OptId::NumEntities) || doc.cfg_bool(OptId::XmlTags) {
                b"&#160;"
            } else {
                b"&nbsp;"
            };
            doc.pprint.add_string(ent);
            return;
        } else {
            doc.pprint.wraphere = doc.pprint.linelen;
        }
    }

    // comment characters are passed raw
    if mode & (COMMENT | CDATA) != 0 {
        doc.pprint.add_char(c);
        return;
    }

    // except in CDATA map < to &lt; etc.
    if mode & CDATA == 0 {
        if c == '<' as u32 {
            doc.pprint.add_string(b"&lt;");
            return;
        }
        if c == '>' as u32 {
            doc.pprint.add_string(b"&gt;");
            return;
        }
        if c == '&' as u32
            && doc.cfg_bool(OptId::QuoteAmpersand)
            && !doc.cfg_bool(OptId::PreserveEntities)
            && mode != OTHER_NAMESPACE
        {
            doc.pprint.add_string(b"&amp;");
            return;
        }
        if c == '"' as u32 && qmark {
            doc.pprint.add_string(b"&quot;");
            return;
        }
        if c == '\'' as u32 && qmark {
            doc.pprint.add_string(b"&#39;");
            return;
        }
        if c == 160 && outenc != RAW {
            if doc.cfg_bool(OptId::QuoteNbsp) {
                if doc.cfg_bool(OptId::NumEntities) || doc.cfg_bool(OptId::XmlTags) {
                    doc.pprint.add_string(b"&#160;");
                } else {
                    doc.pprint.add_string(b"&nbsp;");
                }
            } else {
                doc.pprint.add_char(c);
            }
            return;
        }
    }

    // Handle encoding-specific issues
    match outenc {
        UTF8 | UTF16 | UTF16LE | UTF16BE => {
            if mode & PREFORMATTED == 0 && doc.cfg(OptId::PunctWrap) != 0 {
                let wp = character_wrap_point(c);
                if wp == WrapPoint::Before {
                    doc.pprint.wraphere = doc.pprint.linelen;
                } else if wp == WrapPoint::After {
                    doc.pprint.wraphere = doc.pprint.linelen + 1;
                }
            }
        }
        BIG5 => {
            doc.pprint.add_char(c);
            if mode & PREFORMATTED == 0 && doc.cfg(OptId::PunctWrap) != 0 {
                let wp = big5_wrap_point(c);
                if wp == WrapPoint::Before {
                    doc.pprint.wraphere = doc.pprint.linelen;
                } else if wp == WrapPoint::After {
                    doc.pprint.wraphere = doc.pprint.linelen + 1;
                }
            }
            return;
        }
        SHIFTJIS | ISO2022 | RAW => {
            doc.pprint.add_char(c);
            return;
        }
        _ => {}
    }

    // don't map latin-1 chars to entities
    if outenc == LATIN1 {
        if c > 255 {
            let vers = lexer::html_version(doc);
            match entities::entity_name(c, vers) {
                Some(p) if !doc.cfg_bool(OptId::NumEntities) => add_entity(doc, p),
                _ => add_ncr(doc, c),
            }
            return;
        }
        if c > 126 && c < 160 {
            add_ncr(doc, c);
            return;
        }
        doc.pprint.add_char(c);
        return;
    }

    // don't map UTF-8 / UTF-16 chars to entities
    if outenc == UTF8 || outenc == UTF16 || outenc == UTF16LE || outenc == UTF16BE {
        doc.pprint.add_char(c);
        return;
    }

    // use numeric entities only for XML
    if doc.cfg_bool(OptId::XmlTags) {
        if c > 127 && outenc == ASCII {
            add_ncr(doc, c);
            return;
        }
        doc.pprint.add_char(c);
        return;
    }

    // default treatment for ASCII
    if outenc == ASCII && (c > 126 || (c < ' ' as u32 && c != '\t' as u32)) {
        let vers = lexer::html_version(doc);
        match entities::entity_name(c, vers) {
            Some(p) if !doc.cfg_bool(OptId::NumEntities) => add_entity(doc, p),
            _ => add_ncr(doc, c),
        }
        return;
    }

    doc.pprint.add_char(c);
}

/// IncrWS()
fn incr_ws(start: u32, end: u32, indent: u32, ix_ws: i32) -> u32 {
    if ix_ws > 0 {
        let st = start + (ix_ws as u32).min(indent);
        return st.min(end);
    }
    start
}

/// `lexer->lexbuf[ix]` (0 past the end)
fn lb(doc: &Doc, ix: u32) -> u8 {
    doc.lxr().lexbuf.get(ix as usize).copied().unwrap_or(0)
}

/// GetUTF8(lexbuf + ix): (extra bytes, char)
fn lb_utf8(doc: &Doc, ix: u32) -> (u32, u32) {
    let buf = &doc.lxr().lexbuf;
    let s = buf.get(ix as usize..).unwrap_or(&[]);
    let (n, c) = utf8::get_utf8(s);
    (n as u32, c)
}

fn is_c_space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

/// PPrintText(): the line buffer is uint so it can hold Unicode values
/// unencoded; WriteChar does the encoding when flushing.
fn pprint_text(doc: &mut Doc, mode: u32, indent: u32, node: NodeId) {
    let mut start = doc[node].start;
    let mut end = doc[node].end;
    let ix_nl = text_ends_with_newline(doc, node, mode);
    let mut ix_ws = text_starts_with_whitespace(doc, node, start, mode);
    if ix_nl > 0 {
        end -= ix_nl as u32;
    }
    start = incr_ws(start, end, indent, ix_ws);

    let mut ix = start;
    while ix < end {
        check_wrap_indent(doc, indent);
        let mut c = lb(doc, ix) as u32;
        if c > 0x7F {
            let (n, ch) = lb_utf8(doc, ix);
            ix += n;
            c = ch;
        }
        if c == '\n' as u32 {
            pflush_line(doc, indent);
            ix_ws = text_starts_with_whitespace(doc, node, ix + 1, mode);
            ix = incr_ws(ix, end, indent, ix_ws);
        } else if c == '&' as u32
            && lexer::html_version(doc) == HT50
            && ((ix + 1) == end || ((ix + 1) < end && is_c_space(lb(doc, ix + 1))))
        {
            pprint_char(doc, c, mode | CDATA);
        } else {
            pprint_char(doc, c, mode);
        }
        ix += 1;
    }
}

/// PPrintAttrValue()
fn pprint_attr_value(
    doc: &mut Doc,
    indent: u32,
    value: Option<&[u8]>,
    delim: u32,
    wrappable: bool,
    script_attr: bool,
) {
    let scriptlets = doc.cfg_bool(OptId::WrapScriptlets);
    let mut mode = PREFORMATTED | ATTRIBVALUE;
    if wrappable {
        mode = NORMAL | ATTRIBVALUE;
    }
    let value = value.map(tmbstr::cstr);
    let at = |v: &[u8], i: usize| v.get(i).copied().unwrap_or(0);

    // look for ASP, Tango or PHP instructions for computed attribute value
    if let Some(v) = value {
        if at(v, 0) == b'<'
            && (at(v, 1) == b'%' || at(v, 1) == b'@' || tmbstr::tmbstrncmp(v, b"<?php", 5) == 0)
        {
            mode |= CDATA;
        }
    }

    let delim = if delim == 0 { '"' as u32 } else { delim };

    doc.pprint.add_char('=' as u32);

    // don't wrap after "=" for xml documents
    if !doc.cfg_bool(OptId::XmlOut) || doc.cfg_bool(OptId::XhtmlOut) {
        set_wrap(doc, indent);
        check_wrap_indent(doc, indent);
    }

    doc.pprint.add_char(delim);

    if let Some(v) = value {
        let wraplen = doc.cfg(OptId::WrapLen);
        let attr_start = doc.pprint.set_in_attr_val();
        let mut str_start = doc.pprint.clear_in_string();
        let mut i = 0usize;
        while i < v.len() {
            let mut c = v[i] as u32;

            if wrappable && c == ' ' as u32 {
                set_wrap_attr(doc, indent, attr_start, str_start);
            }
            if wrappable
                && doc.pprint.wraphere > 0
                && doc.pprint.get_spaces() + doc.pprint.linelen >= wraplen
            {
                wrap_attr_val(doc);
            }

            if c == delim {
                let entity: &[u8] = if c == '"' as u32 { b"&quot;" } else { b"&#39;" };
                doc.pprint.add_string(entity);
                i += 1;
                continue;
            } else if c == '"' as u32 {
                if doc.cfg_bool(OptId::QuoteMarks) {
                    doc.pprint.add_string(b"&quot;");
                } else {
                    doc.pprint.add_char(c);
                }
                if delim == '\'' as u32 && script_attr && scriptlets {
                    str_start = doc.pprint.toggle_in_string();
                }
                i += 1;
                continue;
            } else if c == '\'' as u32 {
                if doc.cfg_bool(OptId::QuoteMarks) {
                    doc.pprint.add_string(b"&#39;");
                } else {
                    doc.pprint.add_char(c);
                }
                if delim == '"' as u32 && script_attr && scriptlets {
                    str_start = doc.pprint.toggle_in_string();
                }
                i += 1;
                continue;
            }

            // look for UTF-8 multibyte character
            if c > 0x7F {
                let (n, ch) = utf8::get_utf8(&v[i..]);
                i += n;
                c = ch;
            }
            i += 1;

            if c == '\n' as u32 {
                // No indent inside Javascript literals
                let ind = if str_start < 0 && !doc.cfg_bool(OptId::LiteralAttribs) {
                    indent
                } else {
                    0
                };
                pflush_line(doc, ind);
                continue;
            }
            pprint_char(doc, c, mode);
        }
        doc.pprint.clear_in_attr_val();
        doc.pprint.clear_in_string();
    }
    doc.pprint.add_char(delim);
}

/// AttrIndent()
fn attr_indent(doc: &mut Doc, node: NodeId) -> u32 {
    let spaces = doc.cfg(OptId::IndentSpaces);
    let xtra = 2u32;
    if doc[node].element.is_none() {
        return spaces;
    }
    let parent = doc[node].parent.unwrap_or(node);
    if !tags::node_has_cm(doc, Some(node), CM_INLINE) || !should_indent(doc, parent) {
        return xtra + tmbstr::tmbstrlen(doc[node].element.as_deref().unwrap_or(b"")) as u32;
    }
    if let Some(c) = lexer::find_container(doc, Some(node)) {
        return xtra + tmbstr::tmbstrlen(doc[c].element.as_deref().unwrap_or(b"")) as u32;
    }
    spaces
}

/// PPrintAttribute()
fn pprint_attribute(doc: &mut Doc, indent: u32, node: NodeId, attr: AttId) {
    let mut indent = indent;
    let xml_out = doc.cfg_bool(OptId::XmlOut);
    let xhtml_out = doc.cfg_bool(OptId::XhtmlOut);
    let wrap_attrs = doc.cfg_bool(OptId::WrapAttVals);
    let uc_attrs = doc.cfg(OptId::UpperCaseAttrs);
    let mut ind_attrs = doc.cfg_bool(OptId::IndentAttributes);
    let mut xtra = attr_indent(doc, node);
    let first = doc[node].attributes == Some(attr);
    let name: Vec<u8> = doc[attr].attribute.clone().unwrap_or_default();
    let mut wrappable = false;

    if !ind_attrs {
        xtra = 0;
    }

    if ind_attrs {
        if tags::node_is_element(doc, Some(node)) && !first {
            indent += xtra;
            pcond_flush_line_smart(doc, indent);
        } else {
            ind_attrs = false;
        }
    }
    let _ = ind_attrs;

    check_wrap_indent(doc, indent);

    if !xml_out && !xhtml_out && doc[attr].dict.is_some() {
        if attrs::is_script(doc, &name) {
            wrappable = doc.cfg_bool(OptId::WrapScriptlets);
        } else if !(doc.attr_is(Some(attr), AttrId::CONTENT)
            || doc.attr_is(Some(attr), AttrId::VALUE)
            || doc.attr_is(Some(attr), AttrId::ALT)
            || doc.attr_is(Some(attr), AttrId::TITLE))
            && wrap_attrs
        {
            wrappable = true;
        }
    }

    if !first && !set_wrap(doc, indent) {
        pflush_line(doc, indent + xtra);
    } else if doc.pprint.linelen > 0 {
        doc.pprint.add_char(' ' as u32);
    }

    // Attribute name
    let nm = tmbstr::cstr(&name);
    let mut i = 0usize;
    while i < nm.len() {
        let mut c = nm[i] as u32;
        if c > 0x7F {
            let (n, ch) = utf8::get_utf8(&nm[i..]);
            i += n;
            c = ch;
        } else if uc_attrs == TIDY_UPPERCASE_YES {
            c = lexer::to_upper(c);
        }
        doc.pprint.add_char(c);
        i += 1;
    }

    check_wrap_indent(doc, indent);

    let delim = doc[attr].delim;
    match doc[attr].value.clone() {
        None => {
            let is_b = attrs::is_bool_attribute(doc, attr);
            let script_attr = attrs::attr_is_event(doc, attr);
            if xml_out {
                let v: &[u8] = if is_b { &name } else { b"" };
                pprint_attr_value(doc, indent, Some(v), delim, false, script_attr);
            } else if !is_b && !parser::is_new_node(doc, node) {
                pprint_attr_value(doc, indent, Some(b""), delim, true, script_attr);
            } else {
                set_wrap(doc, indent);
            }
        }
        Some(v) => pprint_attr_value(doc, indent, Some(&v), delim, wrappable, false),
    }
}

/// PPrintAttrs()
fn pprint_attrs(doc: &mut Doc, indent: u32, node: NodeId) {
    // add xml:space attribute to pre and other elements
    if doc.cfg_bool(OptId::XmlOut)
        && doc.cfg_bool(OptId::XmlSpace)
        && attrs::get_attr_by_name(doc, node, b"xml:space").is_none()
        && parser::xml_preserve_white_space(doc, node)
    {
        attrs::add_attribute(doc, node, b"xml:space", b"preserve");
    }

    let mut av = doc[node].attributes;
    while let Some(a) = av {
        if doc[a].attribute.is_some() {
            pprint_attribute(doc, indent, node, a);
        } else if let Some(asp) = doc[a].asp {
            doc.pprint.add_char(' ' as u32);
            pprint_asp(doc, indent, asp);
        } else if let Some(php) = doc[a].php {
            doc.pprint.add_char(' ' as u32);
            pprint_php(doc, indent, php);
        }
        av = doc[a].next;
    }
}

/// TextNodeEndWithSpace()
pub fn text_node_end_with_space(doc: &mut Doc, node: Option<NodeId>) -> bool {
    text_node_end_with_space_r(doc, node)
}

fn text_node_end_with_space_r(doc: &Doc, node: Option<NodeId>) -> bool {
    if let Some(n) = node {
        if tags::node_is_text(doc, Some(n)) && doc[n].end > doc[n].start {
            let mut c = 0u32;
            let mut i = doc[n].start;
            while i < doc[n].end {
                c = lb(doc, i) as u32;
                if c > 0x7F {
                    let (k, ch) = lb_utf8(doc, i);
                    i += k;
                    c = ch;
                }
                i += 1;
            }
            if c == ' ' as u32 || c == '\n' as u32 {
                return true;
            }
        }
    }
    false
}

/// AfterSpaceImp()
fn after_space_imp(doc: &Doc, node: Option<NodeId>, is_empty: bool) -> bool {
    if !tags::node_cm_is_inline(doc, node) {
        return true;
    }
    let n = node.expect("inline node");
    if let Some(prev) = doc[n].prev {
        if tags::node_is_text(doc, Some(prev)) {
            return text_node_end_with_space_r(doc, Some(prev));
        } else if doc.is(prev, TagId::BR) {
            return true;
        }
        return false;
    }
    if is_empty && !tags::node_cm_is_inline(doc, doc[n].parent) {
        return false;
    }
    after_space_imp(doc, doc[n].parent, is_empty)
}

/// AfterSpace()
fn after_space(doc: &Doc, node: NodeId) -> bool {
    after_space_imp(doc, Some(node), tags::node_cm_is_empty(doc, Some(node)))
}

/// isVoidElement(): void elements (area, base, br, col, command, embed,
/// hr, img, input, keygen, link, meta, param, source, track, wbr)
pub fn is_void_element(doc: &Doc, node: Option<NodeId>) -> bool {
    let Some(n) = node else {
        return false;
    };
    if doc[n].tag.is_none() {
        return false;
    }
    matches!(
        doc.tag_id(Some(n)),
        TagId::AREA
            | TagId::BASE
            | TagId::BR
            | TagId::COL
            | TagId::COMMAND
            | TagId::EMBED
            | TagId::HR
            | TagId::IMG
            | TagId::INPUT
            | TagId::KEYGEN
            | TagId::LINK
            | TagId::META
            | TagId::PARAM
            | TagId::SOURCE
            | TagId::TRACK
            | TagId::WBR
    )
}

/// Add an element name: UTF-8 decoded, optionally upper-cased.
fn add_element_name(doc: &mut Doc, s: &[u8], uc: bool) {
    let s = tmbstr::cstr(s);
    let mut i = 0usize;
    while i < s.len() {
        let mut c = s[i] as u32;
        if c > 0x7F {
            let (n, ch) = utf8::get_utf8(&s[i..]);
            i += n;
            c = ch;
        } else if uc {
            c = lexer::to_upper(c);
        }
        doc.pprint.add_char(c);
        i += 1;
    }
}

/// PPrintTag()
fn pprint_tag(doc: &mut Doc, mode: u32, indent: u32, node: NodeId) {
    let uc = doc.cfg_bool(OptId::UpperCaseTags);
    let xhtml_out = doc.cfg_bool(OptId::XhtmlOut);
    let xml_out = doc.cfg_bool(OptId::XmlOut);

    doc.pprint.add_char('<' as u32);
    if doc[node].type_ == NodeType::EndTag {
        doc.pprint.add_char('/' as u32);
    }
    if let Some(s) = doc[node].element.clone() {
        add_element_name(doc, &s, uc);
    }

    pprint_attrs(doc, indent, node);

    if (xml_out || xhtml_out)
        && (doc[node].type_ == NodeType::StartEndTag || tags::node_cm_is_empty(doc, Some(node)))
    {
        doc.pprint.add_char(' ' as u32);
        doc.pprint.add_char('/' as u32);
    }

    doc.pprint.add_char('>' as u32);

    if (doc[node].type_ == NodeType::StartEndTag && lexer::html_version(doc) == HT50)
        && !is_void_element(doc, Some(node))
    {
        pprint_end_tag(doc, mode, indent, node);
    }

    let t = doc[node].type_;
    if (t != NodeType::StartEndTag
        || xhtml_out
        || (t == NodeType::StartEndTag && lexer::html_version(doc) == HT50))
        && (mode & PREFORMATTED) == 0
    {
        let wraplen = doc.cfg(OptId::WrapLen);
        check_wrap_indent(doc, indent);

        if indent.wrapping_add(doc.pprint.linelen) < wraplen {
            if (mode & NOWRAP) == 0
                && (!tags::node_cm_is_inline(doc, Some(node)) || doc.is(node, TagId::BR))
                && after_space(doc, node)
            {
                doc.pprint.wraphere = doc.pprint.linelen;
            }
        } else if mode & NOWRAP != 0 || doc.is(node, TagId::BR) || after_space(doc, node) {
            pcond_flush_line_smart(doc, indent);
        }
    }
}

/// PPrintEndTag()
fn pprint_end_tag(doc: &mut Doc, _mode: u32, _indent: u32, node: NodeId) {
    let uc = doc.cfg_bool(OptId::UpperCaseTags);
    doc.pprint.add_string(b"</");
    if let Some(s) = doc[node].element.clone() {
        add_element_name(doc, &s, uc);
    }
    doc.pprint.add_char('>' as u32);
}

/// PPrintComment()
fn pprint_comment(doc: &mut Doc, indent: u32, node: NodeId) {
    set_wrap(doc, indent);
    doc.pprint.add_string(b"<!--");
    pprint_text(doc, COMMENT, 0, node);
    doc.pprint.add_string(b"--");
    doc.pprint.add_char('>' as u32);
    if doc[node].linebreak && doc[node].next.is_some() {
        pflush_line_smart(doc, indent);
    }
}

/// PPrintDocType()
fn pprint_doc_type(doc: &mut Doc, indent: u32, node: NodeId) {
    let wraplen = doc.cfg(OptId::WrapLen);
    let spaces = doc.cfg(OptId::IndentSpaces);
    let fpi = attrs::get_attr_by_name(doc, node, b"PUBLIC");
    let sys = attrs::get_attr_by_name(doc, node, b"SYSTEM");
    let fpi_val = fpi.and_then(|a| doc[a].value.clone());
    let sys_val = sys.and_then(|a| doc[a].value.clone());

    set_wrap(doc, indent);
    pcond_flush_line_smart(doc, indent);

    doc.pprint.add_string(b"<!DOCTYPE ");
    set_wrap(doc, indent);
    if let Some(e) = doc[node].element.clone() {
        doc.pprint.add_string(&e);
    }

    if let Some(fv) = &fpi_val {
        let d = doc[fpi.unwrap()].delim;
        doc.pprint.add_string(b" PUBLIC ");
        doc.pprint.add_char(d);
        doc.pprint.add_string(fv);
        doc.pprint.add_char(d);
    }

    if let (Some(_), Some(sv)) = (&fpi_val, &sys_val) {
        let sl = tmbstr::tmbstrlen(sv) as u32;
        let mut i = doc.pprint.linelen.wrapping_sub(sl + 2).wrapping_sub(1);
        let lim = (if spaces != 0 { spaces } else { 2 }).wrapping_mul(2);
        if !(i > 0 && (sl + 2).wrapping_add(i) < wraplen && i <= lim) {
            i = 0;
        }
        pcond_flush_line_smart(doc, i);
        if doc.pprint.linelen != 0 {
            doc.pprint.add_char(' ' as u32);
        }
    } else if sys_val.is_some() {
        doc.pprint.add_string(b" SYSTEM ");
    }

    if let Some(sv) = &sys_val {
        let d = doc[sys.unwrap()].delim;
        doc.pprint.add_char(d);
        doc.pprint.add_string(sv);
        doc.pprint.add_char(d);
    }

    if let Some(c) = doc[node].content {
        pcond_flush_line_smart(doc, indent);
        doc.pprint.add_char('[' as u32);
        pprint_text(doc, CDATA, 0, c);
        doc.pprint.add_char(']' as u32);
    }

    set_wrap(doc, 0);
    doc.pprint.add_char('>' as u32);
    pcond_flush_line_smart(doc, indent);
}

/// PPrintPI()
fn pprint_pi(doc: &mut Doc, indent: u32, node: NodeId) {
    set_wrap(doc, indent);
    doc.pprint.add_string(b"<?");
    if let Some(s) = doc[node].element.clone() {
        add_element_name(doc, &s, false);
    }
    // set CDATA to pass < and > unescaped
    pprint_text(doc, CDATA, indent, node);
    if doc.cfg_bool(OptId::XmlOut) || doc.cfg_bool(OptId::XhtmlOut) || doc[node].closed {
        doc.pprint.add_char('?' as u32);
    }
    doc.pprint.add_char('>' as u32);
    pcond_flush_line(doc, indent);
}

/// PPrintXmlDecl()
fn pprint_xml_decl(doc: &mut Doc, indent: u32, node: NodeId) {
    set_wrap(doc, indent);
    let save_wrap = wrap_off(doc);

    // no case translation for XML declaration pseudo attributes
    let uc_attrs = doc.cfg(OptId::UpperCaseAttrs);
    config::set_option_int(doc, OptId::UpperCaseAttrs, 0);

    doc.pprint.add_string(b"<?xml");

    if let Some(att) = attrs::attr_get_by_id(doc, node, AttrId::VERSION) {
        pprint_attribute(doc, indent, node, att);
    }
    if let Some(att) = attrs::attr_get_by_id(doc, node, AttrId::ENCODING) {
        pprint_attribute(doc, indent, node, att);
    }
    if let Some(att) = attrs::get_attr_by_name(doc, node, b"standalone") {
        pprint_attribute(doc, indent, node, att);
    }

    config::set_option_int(doc, OptId::UpperCaseAttrs, uc_attrs as u64);

    let end = doc[node].end;
    if end == 0 || lb(doc, end - 1) != b'?' {
        doc.pprint.add_char('?' as u32);
    }
    doc.pprint.add_char('>' as u32);
    wrap_on(doc, save_wrap);
    pflush_line_smart(doc, indent);
}

/// PPrintAsp(): note ASP and JSTE share <% ... %> syntax
fn pprint_asp(doc: &mut Doc, indent: u32, node: NodeId) {
    let wrap_asp = doc.cfg_bool(OptId::WrapAsp);
    let wrap_jste = doc.cfg_bool(OptId::WrapJste);
    let save_wrap = wrap_off_cond(doc, !wrap_asp || !wrap_jste);

    doc.pprint.add_string(b"<%");
    pprint_text(doc, if wrap_asp { CDATA } else { COMMENT }, indent, node);
    doc.pprint.add_string(b"%>");

    wrap_on(doc, save_wrap);
}

/// PPrintJste(): JSTE also supports <# ... #> syntax
fn pprint_jste(doc: &mut Doc, indent: u32, node: NodeId) {
    let wrap_asp = doc.cfg_bool(OptId::WrapAsp);
    let save_wrap = wrap_off_cond(doc, !wrap_asp);

    doc.pprint.add_string(b"<#");
    let m = if doc.cfg_bool(OptId::WrapJste) {
        CDATA
    } else {
        COMMENT
    };
    pprint_text(doc, m, indent, node);
    doc.pprint.add_string(b"#>");

    wrap_on(doc, save_wrap);
}

/// PPrintPhp(): PHP is based on XML processing instructions
fn pprint_php(doc: &mut Doc, indent: u32, node: NodeId) {
    let wrap_php = doc.cfg_bool(OptId::WrapPhp);
    doc.pprint.add_string(b"<?");
    pprint_text(doc, CDATA, indent, node);
    doc.pprint.add_string(b"?>");
    if wrap_php {
        pcond_flush_line(doc, indent);
    }
}

/// PPrintCDATA()
fn pprint_cdata(doc: &mut Doc, indent: u32, node: NodeId) {
    let indent = if doc.cfg_bool(OptId::IndentCdata) {
        indent
    } else {
        0
    };
    pcond_flush_line_smart(doc, indent);
    let save_wrap = wrap_off(doc);

    doc.pprint.add_string(b"<![CDATA[");
    pprint_text(doc, COMMENT, indent, node);
    doc.pprint.add_string(b"]]>");

    pcond_flush_line_smart(doc, indent);
    wrap_on(doc, save_wrap);
}

/// PPrintSection()
fn pprint_section(doc: &mut Doc, indent: u32, node: NodeId) {
    let wrap_sect = doc.cfg_bool(OptId::WrapSection);
    let save_wrap = wrap_off_cond(doc, !wrap_sect);

    doc.pprint.add_string(b"<![");
    pprint_text(doc, if wrap_sect { CDATA } else { COMMENT }, indent, node);
    doc.pprint.add_string(b"]>");

    wrap_on(doc, save_wrap);
}

const CDATA_START: &[u8] = b"<![CDATA[";
const CDATA_END: &[u8] = b"]]>";
const JS_COMMENT_START: &[u8] = b"//";
const JS_COMMENT_END: &[u8] = b"";
const VB_COMMENT_START: &[u8] = b"'";
const VB_COMMENT_END: &[u8] = b"";
const CSS_COMMENT_START: &[u8] = b"/*";
const CSS_COMMENT_END: &[u8] = b"*/";
const DEFAULT_COMMENT_START: &[u8] = b"";
const DEFAULT_COMMENT_END: &[u8] = b"";

/// InsideHead()
fn inside_head(doc: &Doc, node: NodeId) -> bool {
    if doc.is(node, TagId::HEAD) {
        return true;
    }
    match doc[node].parent {
        Some(p) => inside_head(doc, p),
        None => false,
    }
}

/// TextEndsWithNewline(): is text node and already ends w/ a newline?
fn text_ends_with_newline(doc: &Doc, node: NodeId, mode: u32) -> i32 {
    if (mode & (CDATA | COMMENT)) != 0
        && tags::node_is_text(doc, Some(node))
        && doc[node].end > doc[node].start
    {
        let mut ix = doc[node].end - 1;
        while ix > doc[node].start {
            let ch = lb(doc, ix);
            if ch != 0 && (ch == b' ' || ch == b'\t' || ch == b'\r') {
                ix -= 1;
            } else {
                break;
            }
        }
        if lb(doc, ix) == b'\n' {
            return (doc[node].end - ix - 1) as i32;
        }
    }
    -1
}

/// nodeIsTextLike()
pub fn node_is_text_like(doc: &Doc, node: NodeId) -> bool {
    if tags::node_is_text(doc, Some(node)) {
        return true;
    }
    let t = doc[node].type_;
    t == NodeType::AspTag || t == NodeType::PhpTag
}

/// TextStartsWithWhitespace()
fn text_starts_with_whitespace(doc: &Doc, node: NodeId, start: u32, mode: u32) -> i32 {
    if (mode & (CDATA | COMMENT)) != 0
        && node_is_text_like(doc, node)
        && doc[node].end > doc[node].start
        && start >= doc[node].start
    {
        let mut ix = start;
        while ix < doc[node].end {
            let ch = lb(doc, ix);
            if ch != 0 && (ch == b' ' || ch == b'\t' || ch == b'\r') {
                ix += 1;
            } else {
                break;
            }
        }
        if ix > start {
            return (ix - start) as i32;
        }
    }
    -1
}

/// HasCDATA()
fn has_cdata(doc: &Doc, node: NodeId) -> bool {
    if doc[node].type_ != NodeType::TextNode {
        return false;
    }
    let start = doc[node].start as usize;
    let len = (doc[node].end as i64 - doc[node].start as i64 + 1).max(0) as usize;
    let buf = doc.lxr().lexbuf.get(start..).unwrap_or(&[]);
    tmbstr::tmbsubstrn(buf, len, CDATA_START).is_some()
}

/// PPrintScriptStyle()
fn pprint_script_style(doc: &mut Doc, mode: u32, indent: u32, node: NodeId) {
    let mut comment_start = DEFAULT_COMMENT_START;
    let mut comment_end = DEFAULT_COMMENT_END;
    let mut has_cdata_v = false;
    let mut content_indent: i32 = -1;
    let xhtml_out = doc.cfg_bool(OptId::XhtmlOut);

    if inside_head(doc, node) {
        pflush_line_smart(doc, indent);
    }

    pcond_flush_line_smart(doc, indent);

    pprint_tag(doc, mode, indent, node);

    if doc[node].content.is_some() {
        pflush_line_smart(doc, indent);
    }

    if xhtml_out {
        if let Some(c) = doc[node].content {
            let type_ = attrs::attr_get_by_id(doc, node, AttrId::TYPE);
            if doc.attr_value_is(type_, b"text/javascript") {
                comment_start = JS_COMMENT_START;
                comment_end = JS_COMMENT_END;
            } else if doc.attr_value_is(type_, b"text/css") {
                comment_start = CSS_COMMENT_START;
                comment_end = CSS_COMMENT_END;
            } else if doc.attr_value_is(type_, b"text/vbscript") {
                comment_start = VB_COMMENT_START;
                comment_end = VB_COMMENT_END;
            }

            has_cdata_v = has_cdata(doc, c);

            if !has_cdata_v {
                let save_wrap = wrap_off(doc);
                doc.pprint.add_string(comment_start);
                doc.pprint.add_string(CDATA_START);
                doc.pprint.add_string(comment_end);
                pcond_flush_line_smart(doc, indent);
                wrap_on(doc, save_wrap);
            }
        }
    }

    let mut content = doc[node].content;
    while let Some(c) = content {
        pprint_tree(doc, mode | PREFORMATTED | NOWRAP | CDATA, indent, Some(c));
        if Some(c) == doc[node].last {
            content_indent = text_ends_with_newline(doc, c, CDATA);
        }
        content = doc[c].next;
    }

    if content_indent < 0 && doc[node].content.is_some() {
        pcond_flush_line_smart(doc, indent);
    }

    if xhtml_out && doc[node].content.is_some() && !has_cdata_v {
        let save_wrap = wrap_off(doc);
        doc.pprint.add_string(comment_start);
        doc.pprint.add_string(CDATA_END);
        doc.pprint.add_string(comment_end);
        wrap_on(doc, save_wrap);
        pcond_flush_line_smart(doc, indent);
    }

    if doc[node].content.is_some() {
        set_indent(doc, indent);
    }
    pprint_end_tag(doc, mode, indent, node);
    if doc.cfg_auto_bool(OptId::IndentContent) == TIDY_NO_STATE
        && doc[node].next.is_some()
        && !(tags::node_has_cm(doc, Some(node), CM_INLINE) || tags::node_is_text(doc, Some(node)))
    {
        pflush_line_smart(doc, indent);
    }
}

/// ShouldIndent()
fn should_indent(doc: &Doc, node: NodeId) -> bool {
    let indent_content = doc.cfg_auto_bool(OptId::IndentContent);
    if indent_content == TIDY_NO_STATE {
        return false;
    }
    if doc.is(node, TagId::TEXTAREA) {
        return false;
    }
    if indent_content == TIDY_AUTO_STATE {
        if doc[node].content.is_some() && tags::node_has_cm(doc, Some(node), CM_NO_INDENT) {
            let mut n = doc[node].content;
            while let Some(c) = n {
                if tags::node_has_cm(doc, Some(c), CM_BLOCK) {
                    return true;
                }
                n = doc[c].next;
            }
            return false;
        }
        if tags::node_has_cm(doc, Some(node), CM_HEADING) {
            return false;
        }
        if doc.is(node, TagId::HTML) {
            return false;
        }
        if doc.is(node, TagId::P) {
            return false;
        }
        if doc.is(node, TagId::TITLE) {
            return false;
        }
        if doc.is(node, TagId::DIV) && doc.is_o(doc[node].last, TagId::IMG) {
            return false;
        }
    }
    if tags::node_has_cm(doc, Some(node), CM_FIELD | CM_OBJECT) {
        return true;
    }
    if doc.is(node, TagId::MAP) {
        return true;
    }
    !tags::node_has_cm(doc, Some(node), CM_INLINE) && doc[node].content.is_some()
}

/// PrintBody(): print just the content of the body element
pub fn print_body(doc: &mut Doc) {
    if let Some(body) = lexer::find_body(doc) {
        let mut node = doc[body].content;
        while let Some(n) = node {
            pprint_tree(doc, NORMAL, 0, Some(n));
            node = doc[n].next;
        }
    }
}

/// PPrintMathML(): #130 MathML attr and entity fix
fn pprint_math_ml(doc: &mut Doc, indent: u32, node: NodeId) {
    let mode = OTHER_NAMESPACE;
    pprint_tag(doc, mode, indent, node);
    let mut content = doc[node].content;
    while let Some(c) = content {
        pprint_tree(doc, mode, indent, Some(c));
        content = doc[c].next;
    }
    pprint_end_tag(doc, mode, indent, node);
}

fn is_parse_pre(doc: &Doc, node: NodeId) -> bool {
    doc.parser_of(node)
        .is_some_and(|f| std::ptr::fn_addr_eq(f, parser::parse_pre as crate::tags::ParserFn))
}

/// PPrintTree()
pub fn pprint_tree(doc: &mut Doc, mode: u32, indent: u32, node: Option<NodeId>) {
    let Some(node) = node else {
        return;
    };
    let spaces = doc.cfg(OptId::IndentSpaces);
    let xhtml = doc.cfg_bool(OptId::XhtmlOut);
    let mut indent = indent;

    match doc[node].type_ {
        NodeType::TextNode => pprint_text(doc, mode, indent, node),
        NodeType::CommentTag => pprint_comment(doc, indent, node),
        NodeType::RootNode => {
            let mut content = doc[node].content;
            while let Some(c) = content {
                pprint_tree(doc, mode, indent, Some(c));
                content = doc[c].next;
            }
        }
        NodeType::DocTypeTag => pprint_doc_type(doc, indent, node),
        NodeType::ProcInsTag => pprint_pi(doc, indent, node),
        NodeType::XmlDecl => pprint_xml_decl(doc, indent, node),
        NodeType::CDATATag => pprint_cdata(doc, indent, node),
        NodeType::SectionTag => pprint_section(doc, indent, node),
        NodeType::AspTag => pprint_asp(doc, indent, node),
        NodeType::JsteTag => pprint_jste(doc, indent, node),
        NodeType::PhpTag => pprint_php(doc, indent, node),
        _ if doc.is(node, TagId::MATHML) => pprint_math_ml(doc, indent, node),
        _ if tags::node_cm_is_empty(doc, Some(node))
            || (doc[node].type_ == NodeType::StartEndTag && !xhtml) =>
        {
            if !tags::node_has_cm(doc, Some(node), CM_INLINE) {
                pcond_flush_line_smart(doc, indent);
            }

            if doc.is(node, TagId::BR) {
                if let Some(prev) = doc[node].prev {
                    if !(doc.is(prev, TagId::BR) || (mode & PREFORMATTED) != 0)
                        && doc.cfg_bool(OptId::BreakBeforeBR)
                    {
                        pflush_line_smart(doc, indent);
                    }
                }
            }

            if doc.is(node, TagId::HR) {
                let classic = tidy_classic_vs(doc);
                if classic {
                    if let Some(p) = doc[node].parent {
                        if doc[p].content != Some(node) {
                            pflush_line_smart(doc, indent);
                        }
                    }
                }
            }

            pprint_tag(doc, mode, indent, node);

            if doc[node].next.is_some() {
                if doc.is(node, TagId::PARAM) || doc.is(node, TagId::AREA) {
                    pcond_flush_line_smart(doc, indent);
                } else if (doc.is(node, TagId::BR) && (mode & PREFORMATTED) == 0)
                    || doc.is(node, TagId::HR)
                {
                    pflush_line_smart(doc, indent);
                }
            }
        }
        _ => {
            // some kind of container element
            if doc[node].type_ == NodeType::StartEndTag {
                doc[node].type_ = NodeType::StartTag;
            }

            if doc[node].tag.is_some() && (is_parse_pre(doc, node) || doc.is(node, TagId::TEXTAREA))
            {
                let classic = tidy_classic_vs(doc);
                let indprev = indent;

                pcond_flush_line_smart(doc, indent);

                if classic {
                    if let Some(p) = doc[node].parent {
                        if doc[p].content != Some(node) {
                            pflush_line_smart(doc, indent);
                        }
                    }
                }

                pprint_tag(doc, mode | NOWRAP, indent, node);

                indent = 0;
                let mut content = doc[node].content;
                while let Some(c) = content {
                    pprint_tree(doc, mode | PREFORMATTED | NOWRAP, indent, Some(c));
                    content = doc[c].next;
                }
                indent = indprev;
                pprint_end_tag(doc, mode, indent, node);

                if doc.cfg_auto_bool(OptId::IndentContent) == TIDY_NO_STATE
                    && doc[node].next.is_some()
                {
                    pflush_line_smart(doc, indent);
                }
            } else if doc.is(node, TagId::STYLE) || doc.is(node, TagId::SCRIPT) {
                pprint_script_style(doc, mode | PREFORMATTED | NOWRAP | CDATA, indent, node);
            } else if tags::node_cm_is_inline(doc, Some(node)) {
                if doc.cfg_bool(OptId::MakeClean) && doc.is(node, TagId::NOBR) {
                    // replace <nobr>...</nobr> by &nbsp; or &#160; etc.
                    let mut content = doc[node].content;
                    while let Some(c) = content {
                        pprint_tree(doc, mode | NOWRAP, indent, Some(c));
                        content = doc[c].next;
                    }
                    return;
                }

                // otherwise a normal inline element
                pprint_tag(doc, mode, indent, node);

                // indent content for SELECT, TEXTAREA, MAP, OBJECT and APPLET
                if should_indent(doc, node) {
                    indent += spaces;
                    pcond_flush_line_smart(doc, indent);

                    let mut content = doc[node].content;
                    while let Some(c) = content {
                        pprint_tree(doc, mode, indent, Some(c));
                        content = doc[c].next;
                    }

                    indent -= spaces;
                    pcond_flush_line_smart(doc, indent);
                } else {
                    let mut content = doc[node].content;
                    while let Some(c) = content {
                        pprint_tree(doc, mode, indent, Some(c));
                        content = doc[c].next;
                    }
                }
                pprint_end_tag(doc, mode, indent, node);
            } else {
                // other tags
                let indcont = doc.cfg_auto_bool(OptId::IndentContent) != TIDY_NO_STATE;
                let hideend = doc.cfg_bool(OptId::OmitOptionalTags);
                let classic = tidy_classic_vs(doc);
                let mut content_indent = indent;

                if classic && !doc.is(node, TagId::HTML) {
                    if let Some(p) = doc[node].parent {
                        if doc[p].content != Some(node) {
                            pflush_line_smart(doc, indent);
                        }
                    }
                }

                if should_indent(doc, node) {
                    content_indent += spaces;
                }

                pcond_flush_line_smart(doc, indent);

                // do not omit elements with attributes
                if !hideend
                    || !tags::node_has_cm(doc, Some(node), CM_OMITST)
                    || doc[node].attributes.is_some()
                {
                    pprint_tag(doc, mode, indent, node);

                    if should_indent(doc, node) {
                        // fix for bug 530791, don't wrap after <li> if
                        // first child is text node
                        if !(doc.is(node, TagId::LI) && tags::node_is_text(doc, doc[node].content))
                        {
                            pcond_flush_line_smart(doc, content_indent);
                        }
                    } else if tags::node_has_cm(doc, Some(node), CM_HTML)
                        || doc.is(node, TagId::NOFRAMES)
                        || (tags::node_has_cm(doc, Some(node), CM_HEAD)
                            && !doc.is(node, TagId::TITLE))
                    {
                        pflush_line_smart(doc, content_indent);
                    }
                } else if should_indent(doc, node) {
                    content_indent -= spaces;
                }

                let mut last: Option<NodeId> = None;
                let mut content = doc[node].content;
                while let Some(c) = content {
                    // kludge for naked text before block level tag
                    if let Some(l) = last {
                        if !indcont
                            && tags::node_is_text(doc, Some(l))
                            && doc[c].tag.is_some()
                            && !tags::node_has_cm(doc, Some(c), CM_INLINE)
                        {
                            pflush_line_smart(doc, content_indent);
                        }
                    }
                    pprint_tree(doc, mode, content_indent, Some(c));
                    last = Some(c);
                    content = doc[c].next;
                }

                // don't flush line for td and th
                if should_indent(doc, node)
                    || (!hideend
                        && (tags::node_has_cm(doc, Some(node), CM_HTML)
                            || doc.is(node, TagId::NOFRAMES)
                            || (tags::node_has_cm(doc, Some(node), CM_HEAD)
                                && !doc.is(node, TagId::TITLE))))
                {
                    pcond_flush_line_smart(doc, indent);
                    if !hideend || !tags::node_has_cm(doc, Some(node), CM_OPT) {
                        pprint_end_tag(doc, mode, indent, node);
                    }
                } else if !hideend || !tags::node_has_cm(doc, Some(node), CM_OPT) {
                    // newline before endtag for classic formatting
                    if classic && !has_mixed_content(doc, Some(node)) {
                        pflush_line_smart(doc, indent);
                    }
                    pprint_end_tag(doc, mode, indent, node);
                } else if hideend {
                    // Issue #390 - must still deal with adjusting indent
                    set_indent(doc, indent);
                }

                if (!indcont && !hideend && !doc.is(node, TagId::HTML) && !classic)
                    || (classic
                        && doc[node].next.is_some()
                        && tags::node_has_cm(
                            doc,
                            Some(node),
                            CM_LIST | CM_DEFLIST | CM_TABLE | CM_BLOCK,
                        ))
                {
                    pflush_line_smart(doc, indent);
                }
            }
        }
    }
}

/// PPrintXMLTree()
pub fn pprint_xml_tree(doc: &mut Doc, mode: u32, indent: u32, node: Option<NodeId>) {
    let Some(node) = node else {
        return;
    };
    let xhtml_out = doc.cfg_bool(OptId::XhtmlOut);
    let mut indent = indent;

    match doc[node].type_ {
        NodeType::TextNode => pprint_text(doc, mode, indent, node),
        NodeType::CommentTag => {
            pcond_flush_line_smart(doc, indent);
            pprint_comment(doc, indent, node);
        }
        NodeType::RootNode => {
            let mut content = doc[node].content;
            while let Some(c) = content {
                pprint_xml_tree(doc, mode, indent, Some(c));
                content = doc[c].next;
            }
        }
        NodeType::DocTypeTag => pprint_doc_type(doc, indent, node),
        NodeType::ProcInsTag => pprint_pi(doc, indent, node),
        NodeType::XmlDecl => pprint_xml_decl(doc, indent, node),
        NodeType::CDATATag => pprint_cdata(doc, indent, node),
        NodeType::SectionTag => pprint_section(doc, indent, node),
        NodeType::AspTag => pprint_asp(doc, indent, node),
        NodeType::JsteTag => pprint_jste(doc, indent, node),
        NodeType::PhpTag => pprint_php(doc, indent, node),
        _ if tags::node_has_cm(doc, Some(node), CM_EMPTY)
            || (doc[node].type_ == NodeType::StartEndTag && !xhtml_out) =>
        {
            pcond_flush_line_smart(doc, indent);
            pprint_tag(doc, mode, indent, node);
        }
        _ => {
            // some kind of container element
            let spaces = doc.cfg(OptId::IndentSpaces);
            let mut mixed = false;
            let cindent;

            let mut content = doc[node].content;
            while let Some(c) = content {
                if tags::node_is_text(doc, Some(c)) {
                    mixed = true;
                    break;
                }
                content = doc[c].next;
            }

            pcond_flush_line_smart(doc, indent);

            if parser::xml_preserve_white_space(doc, node) {
                indent = 0;
                mixed = false;
                cindent = 0;
            } else if mixed {
                cindent = indent;
            } else {
                cindent = indent + spaces;
            }

            pprint_tag(doc, mode, indent, node);
            if !mixed && doc[node].content.is_some() {
                pflush_line_smart(doc, cindent);
            }

            let mut content = doc[node].content;
            while let Some(c) = content {
                pprint_xml_tree(doc, mode, cindent, Some(c));
                content = doc[c].next;
            }

            if !mixed && doc[node].content.is_some() {
                pcond_flush_line_smart(doc, indent);
            }

            pprint_end_tag(doc, mode, indent, node);
        }
    }
}
