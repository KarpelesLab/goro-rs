//! The tokenizer: a state-by-state port of lexbor's html/tokenizer.c and
//! html/tokenizer/state*.c. Each state function takes (data, end) indices
//! into `mem` and returns the new data position, exactly like lexbor.

use super::*;
use std::sync::OnceLock;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum St {
    DataBefore,
    Data,
    PlaintextBefore,
    Plaintext,
    TagOpen,
    EndTagOpen,
    TagName,
    BeforeAttrName,
    AttrName,
    AfterAttrName,
    BeforeAttrValue,
    AttrValueDq,
    AttrValueSq,
    AttrValueUnq,
    AfterAttrValueQuoted,
    Cr,
    SelfClosingStartTag,
    BogusCommentBefore,
    BogusComment,
    MarkupDeclOpen,
    MarkupDeclComment,
    MarkupDeclDoctype,
    MarkupDeclCdata,
    CdataSectionBefore,
    CdataSection,
    CdataSectionBracket,
    CdataSectionEnd,
    CharRef,
    CharRefAttr,
    CharRefNamed,
    CharRefAmbiguous,
    CharRefNumeric,
    CharRefHexStart,
    CharRefDecStart,
    CharRefHex,
    CharRefDec,
    // comments
    CommentBeforeStart,
    CommentStartDash,
    Comment,
    CommentLt,
    CommentLtBang,
    CommentLtBangDash,
    CommentLtBangDashDash,
    CommentEndDash,
    CommentEnd,
    CommentEndBang,
    // doctype
    DoctypeBefore,
    DoctypeBeforeName,
    DoctypeName,
    DoctypeAfterName,
    DoctypeAfterNamePublic,
    DoctypeAfterNameSystem,
    DoctypeAfterPublicKw,
    DoctypeBeforePublicId,
    DoctypePublicIdDq,
    DoctypePublicIdSq,
    DoctypeAfterPublicId,
    DoctypeBetween,
    DoctypeAfterSystemKw,
    DoctypeBeforeSystemId,
    DoctypeSystemIdDq,
    DoctypeSystemIdSq,
    DoctypeAfterSystemId,
    DoctypeBogus,
    // RCDATA / RAWTEXT
    RcdataBefore,
    Rcdata,
    RcdataLt,
    RcdataEndTagOpen,
    RcdataEndTagName,
    RawtextBefore,
    Rawtext,
    RawtextLt,
    RawtextEndTagOpen,
    RawtextEndTagName,
    // script
    ScriptDataBefore,
    ScriptData,
    ScriptDataLt,
    ScriptDataEndTagOpen,
    ScriptDataEndTagName,
    ScriptDataEscapeStart,
    ScriptDataEscapeStartDash,
    ScriptDataEscaped,
    ScriptDataEscapedDash,
    ScriptDataEscapedDashDash,
    ScriptDataEscapedLt,
    ScriptDataEscapedEndTagOpen,
    ScriptDataEscapedEndTagName,
    ScriptDataDoubleEscapeStart,
    ScriptDataDoubleEscaped,
    ScriptDataDoubleEscapedDash,
    ScriptDataDoubleEscapedDashDash,
    ScriptDataDoubleEscapedLt,
    ScriptDataDoubleEscapedEndTagOpen,
    ScriptDataDoubleEscapeEnd,
}

// Tokenizer error messages (dom_lexbor_tokenizer_error_code_to_string)
const ABCLOFEMCO: &str = "abrupt-closing-of-empty-comment";
const ABDOPUID: &str = "abrupt-doctype-public-identifier";
const ABDOSYID: &str = "abrupt-doctype-system-identifier";
const ABOFDIINNUCHRE: &str = "absence-of-digits-in-numeric-character-reference";
const CHREOUUNRA: &str = "character-reference-outside-unicode-range";
const COCHRE: &str = "control-character-reference";
const EOBETANA: &str = "eof-before-tag-name";
const EOINCD: &str = "eof-in-cdata";
const EOINCO: &str = "eof-in-comment";
const EOINDO: &str = "eof-in-doctype";
const EOINSCHTCOLITE: &str = "eof-in-script-html-comment-like-text";
const EOINTA: &str = "eof-in-tag";
const INCLCO: &str = "incorrectly-closed-comment";
const INOPCO: &str = "incorrectly-opened-comment";
const INCHSEAFDONA: &str = "invalid-character-sequence-after-doctype-name";
const INFICHOFTANA: &str = "invalid-first-character-of-tag-name";
const MIATVA: &str = "missing-attribute-value";
const MIDONA: &str = "missing-doctype-name";
const MIDOPUID: &str = "missing-doctype-public-identifier";
const MIDOSYID: &str = "missing-doctype-system-identifier";
const MIENTANA: &str = "missing-end-tag-name";
const MIQUBEDOPUID: &str = "missing-quote-before-doctype-public-identifier";
const MIQUBEDOSYID: &str = "missing-quote-before-doctype-system-identifier";
const MISEAFCHRE: &str = "missing-semicolon-after-character-reference";
const MIWHAFDOPUKE: &str = "missing-whitespace-after-doctype-public-keyword";
const MIWHAFDOSYKE: &str = "missing-whitespace-after-doctype-system-keyword";
const MIWHBEDONA: &str = "missing-whitespace-before-doctype-name";
const MIWHBEAT: &str = "missing-whitespace-between-attributes";
const MIWHBEDOPUANSYID: &str = "missing-whitespace-between-doctype-public-and-system-identifiers";
const NECO: &str = "nested-comment";
const NOCHRE: &str = "noncharacter-character-reference";
const NUCHRE: &str = "null-character-reference";
const SUCHRE: &str = "surrogate-character-reference";
const UNCHAFDOSYID: &str = "unexpected-character-after-doctype-system-identifier";
const UNCHINATNA: &str = "unexpected-character-in-attribute-name";
const UNCHINUNATVA: &str = "unexpected-character-in-unquoted-attribute-value";
const UNEQSIBEATNA: &str = "unexpected-equals-sign-before-attribute-name";
const UNNUCH: &str = "unexpected-null-character";
const UNQUMAINOFTANA: &str = "unexpected-question-mark-instead-of-tag-name";
const UNSOINTA: &str = "unexpected-solidus-in-tag";
const UNNACHRE: &str = "unknown-named-character-reference";

const REPLACEMENT: &[u8] = "\u{FFFD}".as_bytes();

fn is_alpha(c: u8) -> bool {
    c.is_ascii_alphabetic()
}
fn is_alnum(c: u8) -> bool {
    c.is_ascii_alphanumeric()
}
fn hex_val(c: u8) -> Option<u32> {
    (c as char).to_digit(16)
}
fn dec_val(c: u8) -> Option<u32> {
    if c.is_ascii_digit() {
        Some((c - b'0') as u32)
    } else {
        None
    }
}

/// lexbor_str_res_replacement_character for code points <= 0x9F
fn replacement_char(n: u32) -> u32 {
    const TBL: [u32; 32] = [
        0x20AC, 0x81, 0x201A, 0x0192, 0x201E, 0x2026, 0x2020, 0x2021, 0x02C6, 0x2030, 0x0160,
        0x2039, 0x0152, 0x8D, 0x017D, 0x8F, 0x90, 0x2018, 0x2019, 0x201C, 0x201D, 0x2022, 0x2013,
        0x2014, 0x02DC, 0x2122, 0x0161, 0x203A, 0x0153, 0x9D, 0x017E, 0x0178,
    ];
    if (0x80..=0x9F).contains(&n) {
        TBL[(n - 0x80) as usize]
    } else {
        n
    }
}

fn utf8_of(cp: u32, out: &mut Vec<u8>) {
    if cp <= 0x7F {
        out.push(cp as u8);
    } else if cp <= 0x7FF {
        out.push((0xC0 | (cp >> 6)) as u8);
        out.push((0x80 | (cp & 0x3F)) as u8);
    } else if cp <= 0xFFFF {
        out.push((0xE0 | (cp >> 12)) as u8);
        out.push((0x80 | ((cp >> 6) & 0x3F)) as u8);
        out.push((0x80 | (cp & 0x3F)) as u8);
    } else if cp <= 0x1FFFFF {
        out.push((0xF0 | (cp >> 18)) as u8);
        out.push((0x80 | ((cp >> 12) & 0x3F)) as u8);
        out.push((0x80 | ((cp >> 6) & 0x3F)) as u8);
        out.push((0x80 | (cp & 0x3F)) as u8);
    }
}

/// The named character reference trie (lexbor's static SBST).
struct TrieNode {
    key: u8,
    children: Vec<(u8, usize)>,
    value: Option<&'static [u8]>,
}

fn trie() -> &'static Vec<TrieNode> {
    static T: OnceLock<Vec<TrieNode>> = OnceLock::new();
    T.get_or_init(|| {
        let mut nodes = vec![TrieNode {
            key: 0,
            children: Vec::new(),
            value: None,
        }];
        for (name, value) in ENTITIES.iter() {
            let mut cur = 0usize;
            for &c in name.iter() {
                let found = nodes[cur]
                    .children
                    .iter()
                    .find(|(k, _)| *k == c)
                    .map(|(_, i)| *i);
                cur = match found {
                    Some(i) => i,
                    None => {
                        nodes.push(TrieNode {
                            key: c,
                            children: Vec::new(),
                            value: None,
                        });
                        let i = nodes.len() - 1;
                        nodes[cur].children.push((c, i));
                        i
                    }
                };
            }
            nodes[cur].value = Some(value);
        }
        nodes
    })
}

/// lexbor_str_data_ncasecmp_first: compare the rest of a static markup
/// with `sec`; Some(matched length) when the markup ended or `sec` ran out.
fn ncasecmp_first(first: &[u8], sec: &[u8]) -> Option<usize> {
    for (i, &s) in sec.iter().enumerate() {
        if i >= first.len() {
            return Some(i);
        }
        if !first[i].eq_ignore_ascii_case(&s) {
            return None;
        }
    }
    Some(sec.len())
}

impl Parser {
    // ---------------------------------------------------------------------
    // Helpers (lexbor's tokenizer macros)

    #[inline]
    fn c(&self, p: usize) -> u8 {
        self.mem.get(p).copied().unwrap_or(0)
    }

    fn err(&mut self, pos: usize, msg: &'static str) {
        self.tok_errors.push(TokError { msg, pos });
    }

    fn append_data(&mut self, data: usize) {
        if data > self.begin {
            let (b, e) = (self.begin, data);
            self.tmp.extend_from_slice(&self.mem[b..e]);
        }
    }

    fn append(&mut self, s: &[u8]) {
        self.tmp.extend_from_slice(s);
    }

    fn append_mem(&mut self, p: usize, len: usize) {
        let e = (p + len).min(self.mem.len());
        if p < e {
            let v = self.mem[p..e].to_vec();
            self.tmp.extend_from_slice(&v);
        }
    }

    fn append_replace(&mut self) {
        self.tmp.extend_from_slice(REPLACEMENT);
    }

    fn set_tag(&mut self, from: usize) {
        let name = self.tmp[from.min(self.tmp.len())..].to_vec();
        self.token.tag_id = self.tag_id_for(&name);
    }

    fn set_name(&mut self) {
        let name: Vec<u8> = self.tmp.iter().map(|c| c.to_ascii_lowercase()).collect();
        if let Some(a) = self.token.attrs.last_mut() {
            a.name = Some(name);
        }
    }

    fn set_value(&mut self) {
        let v = self.tmp.clone();
        if let Some(a) = self.token.attrs.last_mut() {
            a.value = Some(v);
        }
    }

    fn token_set_begin(&mut self, v: usize) {
        self.tmp.clear();
        self.token.begin = v;
        self.token.line = self.cur_line;
        self.token.column = self.cur_col;
    }

    fn attr_add(&mut self) {
        self.token.attrs.push(TokenAttr::default());
    }

    fn attr_last(&mut self) -> &mut TokenAttr {
        if self.token.attrs.is_empty() {
            self.token.attrs.push(TokenAttr::default());
        }
        self.token.attrs.last_mut().unwrap()
    }

    fn attr_set_name_begin(&mut self, v: usize) {
        self.tmp.clear();
        self.attr_last().name_begin = v;
    }

    fn attr_set_value_begin(&mut self, v: usize) {
        self.tmp.clear();
        self.attr_last().value_begin = v;
    }

    fn attr_value_begin(&self) -> usize {
        self.token.attrs.last().map(|a| a.value_begin).unwrap_or(0)
    }

    fn set_text(&mut self) {
        self.token.text_start = 0;
        self.token.text_end = self.tmp.len();
    }

    fn emit(&mut self) {
        let mut tok = std::mem::take(&mut self.token);
        self.tree_token(&mut tok);
        self.token = tok;
    }

    /// lxb_html_tokenizer_state_token_done_m
    fn token_done(&mut self) {
        if self.token.begin != self.token.end {
            self.emit();
        }
        self.token.clean();
        self.tmp.clear();
    }

    /// lxb_html_tokenizer_state_token_done_wo_check_m
    fn token_done_wo_check(&mut self) {
        self.emit();
        self.token.clean();
    }

    fn emit_text_not_empty(&mut self) {
        if self.token.begin != self.token.end {
            self.token.tag_id = _TEXT;
            self.set_text();
            self.emit();
            self.token.clean();
        }
    }

    /// The CR handling shared by most character-consuming states: returns
    /// Err(position to return) when the CR ends the chunk.
    fn cr(&mut self, data: &mut usize, end: usize, back: St) -> Result<(), usize> {
        *data += 1;
        if *data >= end {
            self.append_data(*data - 1);
            self.state = St::Cr;
            self.state_return = back;
            return Err(*data);
        }
        self.append_data(*data);
        if let Some(l) = self.tmp.last_mut() {
            *l = 0x0A;
        }
        self.begin = *data + 1;
        if self.c(*data) != 0x0A {
            self.begin = *data;
            *data -= 1;
        }
        Ok(())
    }

    // ---------------------------------------------------------------------
    // Driver

    /// lxb_html_tokenizer_chunk (with PHP's line/column patch)
    pub(crate) fn tkz_chunk(&mut self, start: usize, end: usize) {
        self.is_eof = false;
        self.last = end;
        let mut data = start;
        while data < end {
            let mut col = self.cur_col;
            let new_data = self.step(data, end).min(self.mem.len());
            while data < new_data {
                let ch = self.mem[data];
                if ch == b'\n' {
                    self.cur_line += 1;
                    col = 0;
                } else if ch & 0xC0 != 0x80 {
                    col += 1;
                }
                data += 1;
            }
            self.cur_col = col;
            if new_data < data {
                data = new_data;
            }
        }
    }

    /// lxb_html_tokenizer_end
    pub(crate) fn tkz_end(&mut self) {
        let data = self.eof_pos;
        let end = data + 1;
        self.is_eof = true;
        let mut guard = 0;
        while self.step(data, end) < end {
            guard += 1;
            if guard > 10000 {
                break;
            }
        }
        self.is_eof = false;
        self.token.clean();
        self.token.tag_id = _END_OF_FILE;
        self.emit();
    }

    /// lxb_html_tokenizer_set_state_by_tag
    pub(crate) fn tkz_set_state_by_tag(&mut self, scripting: bool, tag_id: u32, ns: u8) {
        if ns != NS_HTML {
            self.state = St::DataBefore;
            return;
        }
        match tag_id {
            TITLE | TEXTAREA => {
                self.tmp_tag_id = tag_id;
                self.state = St::RcdataBefore;
            }
            STYLE | XMP | IFRAME | NOEMBED | NOFRAMES => {
                self.tmp_tag_id = tag_id;
                self.state = St::RawtextBefore;
            }
            SCRIPT => {
                self.tmp_tag_id = tag_id;
                self.state = St::ScriptDataBefore;
            }
            NOSCRIPT => {
                if scripting {
                    self.tmp_tag_id = tag_id;
                    self.state = St::RawtextBefore;
                    return;
                }
                self.state = St::DataBefore;
            }
            PLAINTEXT => self.state = St::PlaintextBefore,
            _ => {}
        }
    }

    pub(crate) fn set_tkz_state(&mut self, s: St) {
        self.state = s;
    }

    pub(crate) fn set_tmp_tag_id(&mut self, t: u32) {
        self.tmp_tag_id = t;
    }

    /// lxb_html_tokenizer_current_namespace
    fn current_namespace(&self) -> u8 {
        match self.adjusted_current_node() {
            Some(n) => self.dom.nodes[n].ns,
            None => NS_UNDEF,
        }
    }

    fn step(&mut self, data: usize, end: usize) -> usize {
        match self.state {
            St::DataBefore => self.st_data_before(data, end),
            St::Data => self.st_data(data, end),
            St::PlaintextBefore => self.st_plaintext_before(data, end),
            St::Plaintext => self.st_plaintext(data, end),
            St::TagOpen => self.st_tag_open(data, end),
            St::EndTagOpen => self.st_end_tag_open(data, end),
            St::TagName => self.st_tag_name(data, end),
            St::BeforeAttrName => self.st_before_attr_name(data, end),
            St::AttrName => self.st_attr_name(data, end),
            St::AfterAttrName => self.st_after_attr_name(data, end),
            St::BeforeAttrValue => self.st_before_attr_value(data, end),
            St::AttrValueDq => self.st_attr_value_quoted(data, end, b'"', St::AttrValueDq),
            St::AttrValueSq => self.st_attr_value_quoted(data, end, b'\'', St::AttrValueSq),
            St::AttrValueUnq => self.st_attr_value_unquoted(data, end),
            St::AfterAttrValueQuoted => self.st_after_attr_value_quoted(data, end),
            St::Cr => self.st_cr(data, end),
            St::SelfClosingStartTag => self.st_self_closing_start_tag(data, end),
            St::BogusCommentBefore => {
                self.token.tag_id = _EM_COMMENT;
                self.state = St::BogusComment;
                data
            }
            St::BogusComment => self.st_bogus_comment(data, end),
            St::MarkupDeclOpen => self.st_markup_declaration_open(data, end),
            St::MarkupDeclComment => self.st_markup_declaration_comment(data, end),
            St::MarkupDeclDoctype => self.st_markup_declaration_doctype(data, end),
            St::MarkupDeclCdata => self.st_markup_declaration_cdata(data, end),
            St::CdataSectionBefore => self.st_cdata_section_before(data, end),
            St::CdataSection => self.st_cdata_section(data, end),
            St::CdataSectionBracket => self.st_cdata_section_bracket(data, end),
            St::CdataSectionEnd => self.st_cdata_section_end(data, end),
            St::CharRef => {
                self.is_attribute = false;
                self.st_char_ref(data, end)
            }
            St::CharRefAttr => {
                self.is_attribute = true;
                self.st_char_ref(data, end)
            }
            St::CharRefNamed => self.st_char_ref_named(data, end),
            St::CharRefAmbiguous => self.st_char_ref_ambiguous(data, end),
            St::CharRefNumeric => self.st_char_ref_numeric(data, end),
            St::CharRefHexStart => self.st_char_ref_hex_start(data, end),
            St::CharRefDecStart => self.st_char_ref_dec_start(data, end),
            St::CharRefHex => self.st_char_ref_hex(data, end),
            St::CharRefDec => self.st_char_ref_dec(data, end),
            St::CommentBeforeStart => self.st_comment_before_start(data, end),
            St::CommentStartDash => self.st_comment_start_dash(data, end),
            St::Comment => self.st_comment(data, end),
            St::CommentLt => self.st_comment_lt(data, end),
            St::CommentLtBang => self.st_comment_lt_bang(data, end),
            St::CommentLtBangDash => self.st_comment_lt_bang_dash(data, end),
            St::CommentLtBangDashDash => self.st_comment_lt_bang_dash_dash(data, end),
            St::CommentEndDash => self.st_comment_end_dash(data, end),
            St::CommentEnd => self.st_comment_end(data, end),
            St::CommentEndBang => self.st_comment_end_bang(data, end),
            St::DoctypeBefore => self.st_doctype_before(data, end),
            St::DoctypeBeforeName => self.st_doctype_before_name(data, end),
            St::DoctypeName => self.st_doctype_name(data, end),
            St::DoctypeAfterName => self.st_doctype_after_name(data, end),
            St::DoctypeAfterNamePublic => self.st_doctype_after_name_kw(data, end, true),
            St::DoctypeAfterNameSystem => self.st_doctype_after_name_kw(data, end, false),
            St::DoctypeAfterPublicKw => self.st_doctype_after_public_kw(data, end),
            St::DoctypeBeforePublicId => self.st_doctype_before_public_id(data, end),
            St::DoctypePublicIdDq => self.st_doctype_id_quoted(data, end, b'"', true),
            St::DoctypePublicIdSq => self.st_doctype_id_quoted(data, end, b'\'', true),
            St::DoctypeAfterPublicId => self.st_doctype_after_public_id(data, end),
            St::DoctypeBetween => self.st_doctype_between(data, end),
            St::DoctypeAfterSystemKw => self.st_doctype_after_system_kw(data, end),
            St::DoctypeBeforeSystemId => self.st_doctype_before_system_id(data, end),
            St::DoctypeSystemIdDq => self.st_doctype_id_quoted(data, end, b'"', false),
            St::DoctypeSystemIdSq => self.st_doctype_id_quoted(data, end, b'\'', false),
            St::DoctypeAfterSystemId => self.st_doctype_after_system_id(data, end),
            St::DoctypeBogus => self.st_doctype_bogus(data, end),
            St::RcdataBefore => self.st_text_before(data, St::Rcdata),
            St::Rcdata => self.st_rawtext(data, end, true),
            St::RcdataLt => self.st_rawtext_lt(data, St::RcdataEndTagOpen, St::Rcdata),
            St::RcdataEndTagOpen => {
                self.st_end_tag_open_generic(data, St::RcdataEndTagName, St::Rcdata)
            }
            St::RcdataEndTagName => self.st_end_tag_name_generic(data, end, St::Rcdata, false),
            St::RawtextBefore => self.st_text_before(data, St::Rawtext),
            St::Rawtext => self.st_rawtext(data, end, false),
            St::RawtextLt => self.st_rawtext_lt(data, St::RawtextEndTagOpen, St::Rawtext),
            St::RawtextEndTagOpen => {
                self.st_end_tag_open_generic(data, St::RawtextEndTagName, St::Rawtext)
            }
            St::RawtextEndTagName => self.st_end_tag_name_generic(data, end, St::Rawtext, true),
            St::ScriptDataBefore => self.st_text_before(data, St::ScriptData),
            St::ScriptData => self.st_script_data(data, end),
            St::ScriptDataLt => self.st_script_data_lt(data, end),
            St::ScriptDataEndTagOpen => {
                self.st_end_tag_open_generic(data, St::ScriptDataEndTagName, St::ScriptData)
            }
            St::ScriptDataEndTagName => {
                self.st_end_tag_name_generic(data, end, St::ScriptData, true)
            }
            St::ScriptDataEscapeStart => self.st_script_escape_start(data, end),
            St::ScriptDataEscapeStartDash => self.st_script_escape_start_dash(data, end),
            St::ScriptDataEscaped => self.st_script_escaped(data, end),
            St::ScriptDataEscapedDash => self.st_script_escaped_dash(data, end),
            St::ScriptDataEscapedDashDash => self.st_script_escaped_dash_dash(data, end),
            St::ScriptDataEscapedLt => self.st_script_escaped_lt(data, end),
            St::ScriptDataEscapedEndTagOpen => self.st_end_tag_open_generic(
                data,
                St::ScriptDataEscapedEndTagName,
                St::ScriptDataEscaped,
            ),
            St::ScriptDataEscapedEndTagName => {
                self.st_end_tag_name_generic(data, end, St::ScriptDataEscaped, true)
            }
            St::ScriptDataDoubleEscapeStart => self.st_script_double_escape_start(data, end),
            St::ScriptDataDoubleEscaped => self.st_script_double_escaped(data, end),
            St::ScriptDataDoubleEscapedDash => self.st_script_double_escaped_dash(data, end),
            St::ScriptDataDoubleEscapedDashDash => {
                self.st_script_double_escaped_dash_dash(data, end)
            }
            St::ScriptDataDoubleEscapedLt => {
                if self.c(data) == 0x2F {
                    self.state = St::ScriptDataDoubleEscapedEndTagOpen;
                    return data + 1;
                }
                self.state = St::ScriptDataDoubleEscaped;
                data
            }
            St::ScriptDataDoubleEscapedEndTagOpen => {
                if is_alpha(self.c(data)) {
                    self.entity_start = self.tmp.len() + 1;
                    self.state = St::ScriptDataDoubleEscapeEnd;
                } else {
                    self.state = St::ScriptDataDoubleEscaped;
                }
                self.append(b"/");
                data
            }
            St::ScriptDataDoubleEscapeEnd => self.st_script_double_escape_end(data, end),
        }
    }

    // ---------------------------------------------------------------------
    // state.c

    fn st_data_before(&mut self, data: usize, _end: usize) -> usize {
        if !self.is_eof {
            self.token_set_begin(data);
        }
        self.state = St::Data;
        data
    }

    fn st_data(&mut self, mut data: usize, end: usize) -> usize {
        self.begin = data;
        while data != end {
            match self.c(data) {
                0x3C => {
                    self.append_data(data);
                    self.token.end = data;
                    self.state = St::TagOpen;
                    return data + 1;
                }
                0x26 => {
                    self.append_data(data + 1);
                    self.state = St::CharRef;
                    self.state_return = St::Data;
                    return data + 1;
                }
                0x0D => {
                    if let Err(r) = self.cr(&mut data, end, St::Data) {
                        return r;
                    }
                }
                0x00 => {
                    if self.is_eof {
                        if self.token.begin != 0 {
                            self.token.end = self.last;
                        }
                        if self.token.begin != self.token.end {
                            self.token.tag_id = _TEXT;
                            self.append_data(data);
                            self.set_text();
                            self.token_done_wo_check();
                        }
                        return end;
                    }
                    self.token.null_count += 1;
                    self.err(data, UNNUCH);
                }
                _ => {}
            }
            data += 1;
        }
        self.append_data(data);
        data
    }

    fn st_plaintext_before(&mut self, data: usize, _end: usize) -> usize {
        if !self.is_eof {
            self.token_set_begin(data);
        }
        self.token.tag_id = _TEXT;
        self.state = St::Plaintext;
        data
    }

    fn st_plaintext(&mut self, mut data: usize, end: usize) -> usize {
        self.begin = data;
        while data != end {
            match self.c(data) {
                0x0D => {
                    if let Err(r) = self.cr(&mut data, end, St::Plaintext) {
                        return r;
                    }
                }
                0x00 => {
                    self.append_data(data);
                    if self.is_eof {
                        if self.token.begin != 0 {
                            self.token.end = self.last;
                        }
                        self.set_text();
                        self.token_done();
                        return end;
                    }
                    self.begin = data + 1;
                    self.append_replace();
                    self.err(data, UNNUCH);
                }
                _ => {}
            }
            data += 1;
        }
        self.append_data(data);
        data
    }

    fn st_tag_open(&mut self, data: usize, end: usize) -> usize {
        let ch = self.c(data);
        if is_alpha(ch) {
            self.state = St::TagName;
            self.emit_text_not_empty();
            self.token_set_begin(data);
            return data;
        } else if ch == 0x2F {
            self.state = St::EndTagOpen;
            return data + 1;
        } else if ch == 0x21 {
            self.state = St::MarkupDeclOpen;
            self.emit_text_not_empty();
            return data + 1;
        } else if ch == 0x3F {
            self.state = St::BogusCommentBefore;
            self.emit_text_not_empty();
            self.token_set_begin(data);
            self.err(data, UNQUMAINOFTANA);
            return data;
        } else if ch == 0x00 && self.is_eof {
            self.append(b"<");
            self.token.end = self.last;
            self.emit_text_not_empty();
            let e = self.token.end;
            self.err(e, EOBETANA);
            return end;
        }
        self.append(b"<");
        self.err(data, INFICHOFTANA);
        self.state = St::Data;
        data
    }

    fn st_end_tag_open(&mut self, data: usize, end: usize) -> usize {
        let ch = self.c(data);
        if is_alpha(ch) {
            self.state = St::TagName;
            self.emit_text_not_empty();
            self.token_set_begin(data);
            self.token.typ |= TOKEN_CLOSE;
            return data;
        } else if ch == 0x3E {
            self.state = St::Data;
            self.err(data, MIENTANA);
            return data + 1;
        } else if ch == 0x00 && self.is_eof {
            self.append(b"</");
            self.token.end = self.last;
            self.emit_text_not_empty();
            let e = self.token.end;
            self.err(e, EOBETANA);
            return end;
        }
        self.state = St::BogusCommentBefore;
        self.err(data, INFICHOFTANA);
        self.emit_text_not_empty();
        self.token_set_begin(data);
        data
    }

    fn st_tag_name(&mut self, mut data: usize, end: usize) -> usize {
        self.begin = data;
        while data != end {
            match self.c(data) {
                0x09 | 0x0A | 0x0C | 0x0D | 0x20 => {
                    self.append_data(data);
                    self.set_tag(0);
                    self.token.end = data;
                    self.state = St::BeforeAttrName;
                    return data + 1;
                }
                0x2F => {
                    self.append_data(data);
                    self.set_tag(0);
                    self.token.end = data;
                    self.state = St::SelfClosingStartTag;
                    return data + 1;
                }
                0x3E => {
                    self.state = St::DataBefore;
                    self.append_data(data);
                    self.set_tag(0);
                    self.token.end = data;
                    self.token_done();
                    return data + 1;
                }
                0x00 => {
                    if self.is_eof {
                        self.token.end = self.last;
                        let e = self.token.end;
                        self.err(e, EOINTA);
                        return end;
                    }
                    self.append_data(data);
                    self.begin = data + 1;
                    self.append_replace();
                    self.err(data, UNNUCH);
                }
                _ => {}
            }
            data += 1;
        }
        self.append_data(data);
        data
    }

    fn st_before_attr_name(&mut self, mut data: usize, end: usize) -> usize {
        while data != end {
            match self.c(data) {
                0x09 | 0x0A | 0x0C | 0x0D | 0x20 => {}
                0x3D => {
                    self.attr_add();
                    self.attr_set_name_begin(data);
                    self.append_mem(data, 1);
                    self.err(data, UNEQSIBEATNA);
                    self.state = St::AttrName;
                    return data + 1;
                }
                0x2F | 0x3E => {
                    self.state = St::AfterAttrName;
                    return data;
                }
                ch => {
                    if ch == 0x00 && self.is_eof {
                        self.state = St::AfterAttrName;
                        return data;
                    }
                    self.attr_add();
                    self.attr_set_name_begin(data);
                    self.state = St::AttrName;
                    return data;
                }
            }
            data += 1;
        }
        data
    }

    fn st_attr_name(&mut self, mut data: usize, end: usize) -> usize {
        self.begin = data;
        while data != end {
            match self.c(data) {
                0x09 | 0x0A | 0x0C | 0x0D | 0x20 | 0x2F | 0x3E => {
                    self.append_data(data);
                    self.set_name();
                    self.attr_last().name_end = data;
                    self.state = St::AfterAttrName;
                    return data;
                }
                0x00 => {
                    if self.is_eof {
                        let l = self.last;
                        self.attr_last().name_end = l;
                        self.state = St::AfterAttrName;
                        return data;
                    }
                    self.append_data(data);
                    self.begin = data + 1;
                    self.append_replace();
                    self.err(data, UNNUCH);
                }
                0x3D => {
                    self.append_data(data);
                    self.set_name();
                    self.attr_last().name_end = data;
                    self.state = St::BeforeAttrValue;
                    return data + 1;
                }
                0x22 | 0x27 | 0x3C => {
                    self.err(data, UNCHINATNA);
                }
                _ => {}
            }
            data += 1;
        }
        self.append_data(data);
        data
    }

    fn st_after_attr_name(&mut self, mut data: usize, end: usize) -> usize {
        while data != end {
            match self.c(data) {
                0x09 | 0x0A | 0x0C | 0x0D | 0x20 => {}
                0x2F => {
                    self.state = St::SelfClosingStartTag;
                    return data + 1;
                }
                0x3D => {
                    self.state = St::BeforeAttrValue;
                    return data + 1;
                }
                0x3E => {
                    self.state = St::DataBefore;
                    self.token_done();
                    return data + 1;
                }
                ch => {
                    if ch == 0x00 && self.is_eof {
                        let l = self.last;
                        self.err(l, EOINTA);
                        return end;
                    }
                    self.attr_add();
                    self.attr_set_name_begin(data);
                    self.state = St::AttrName;
                    return data;
                }
            }
            data += 1;
        }
        data
    }

    fn st_before_attr_value(&mut self, mut data: usize, end: usize) -> usize {
        while data != end {
            match self.c(data) {
                0x09 | 0x0A | 0x0C | 0x0D | 0x20 => {}
                0x22 => {
                    self.state = St::AttrValueDq;
                    return data + 1;
                }
                0x27 => {
                    self.state = St::AttrValueSq;
                    return data + 1;
                }
                0x3E => {
                    self.state = St::DataBefore;
                    self.err(data, MIATVA);
                    self.token_done();
                    return data + 1;
                }
                _ => {
                    self.state = St::AttrValueUnq;
                    return data;
                }
            }
            data += 1;
        }
        data
    }

    fn st_attr_value_quoted(&mut self, mut data: usize, end: usize, q: u8, me: St) -> usize {
        if self.attr_value_begin() == 0 && !self.is_eof {
            self.attr_set_value_begin(data);
        }
        self.begin = data;
        while data != end {
            let ch = self.c(data);
            if ch == q {
                self.append_data(data);
                self.attr_last().value_end = data;
                self.set_value();
                self.state = St::AfterAttrValueQuoted;
                return data + 1;
            }
            match ch {
                0x26 => {
                    self.append_data(data + 1);
                    self.state = St::CharRefAttr;
                    self.state_return = me;
                    return data + 1;
                }
                0x0D => {
                    if let Err(r) = self.cr(&mut data, end, me) {
                        return r;
                    }
                }
                0x00 => {
                    if self.is_eof {
                        if self.attr_value_begin() != 0 {
                            let l = self.last;
                            self.attr_last().value_end = l;
                        }
                        let l = self.last;
                        self.err(l, EOINTA);
                        return end;
                    }
                    self.append_data(data);
                    self.begin = data + 1;
                    self.append_replace();
                    self.err(data, UNNUCH);
                }
                _ => {}
            }
            data += 1;
        }
        self.append_data(data);
        data
    }

    fn st_attr_value_unquoted(&mut self, mut data: usize, end: usize) -> usize {
        if self.attr_value_begin() == 0 && !self.is_eof {
            self.attr_set_value_begin(data);
        }
        self.begin = data;
        while data != end {
            match self.c(data) {
                0x09 | 0x0A | 0x0C | 0x0D | 0x20 => {
                    self.append_data(data);
                    self.attr_last().value_end = data;
                    self.set_value();
                    self.state = St::BeforeAttrName;
                    return data + 1;
                }
                0x26 => {
                    self.append_data(data + 1);
                    self.state = St::CharRefAttr;
                    self.state_return = St::AttrValueUnq;
                    return data + 1;
                }
                0x3E => {
                    self.state = St::DataBefore;
                    self.append_data(data);
                    self.attr_last().value_end = data;
                    self.set_value();
                    self.token_done();
                    return data + 1;
                }
                0x00 => {
                    if self.is_eof {
                        if self.attr_value_begin() != 0 {
                            let l = self.last;
                            self.attr_last().value_end = l;
                        }
                        let l = self.last;
                        self.err(l, EOINTA);
                        return end;
                    }
                    self.append_data(data);
                    self.begin = data + 1;
                    self.append_replace();
                    self.err(data, UNNUCH);
                }
                0x22 | 0x27 | 0x3C | 0x3D | 0x60 => {
                    let e = self.token.end;
                    self.err(e, UNCHINUNATVA);
                }
                _ => {}
            }
            data += 1;
        }
        self.append_data(data);
        data
    }

    fn st_after_attr_value_quoted(&mut self, data: usize, end: usize) -> usize {
        match self.c(data) {
            0x09 | 0x0A | 0x0C | 0x0D | 0x20 => {
                self.state = St::BeforeAttrName;
                data + 1
            }
            0x2F => {
                self.state = St::SelfClosingStartTag;
                data + 1
            }
            0x3E => {
                self.state = St::DataBefore;
                self.token_done();
                data + 1
            }
            ch => {
                if ch == 0x00 && self.is_eof {
                    let l = self.last;
                    self.err(l, EOINTA);
                    return end;
                }
                self.err(data, MIWHBEAT);
                self.state = St::BeforeAttrName;
                data
            }
        }
    }

    fn st_cr(&mut self, mut data: usize, _end: usize) -> usize {
        self.append(b"\n");
        if self.c(data) == 0x0A {
            data += 1;
        }
        self.state = self.state_return;
        data
    }

    fn st_self_closing_start_tag(&mut self, data: usize, end: usize) -> usize {
        match self.c(data) {
            0x3E => {
                self.state = St::DataBefore;
                self.token.typ |= TOKEN_CLOSE_SELF;
                self.token_done();
                data + 1
            }
            ch => {
                if ch == 0x00 && self.is_eof {
                    let e = self.token.end;
                    self.err(e, EOINTA);
                    return end;
                }
                self.err(data, UNSOINTA);
                self.state = St::BeforeAttrName;
                data
            }
        }
    }

    fn st_bogus_comment(&mut self, mut data: usize, end: usize) -> usize {
        self.begin = data;
        while data != end {
            match self.c(data) {
                0x3E => {
                    self.state = St::DataBefore;
                    self.append_data(data);
                    self.token.end = data;
                    self.set_text();
                    self.token_done_wo_check();
                    return data + 1;
                }
                0x0D => {
                    if let Err(r) = self.cr(&mut data, end, St::BogusComment) {
                        return r;
                    }
                }
                0x00 => {
                    self.append_data(data);
                    if self.is_eof {
                        if self.token.begin != 0 {
                            self.token.end = self.last;
                        }
                        self.set_text();
                        self.token_done_wo_check();
                        return end;
                    }
                    self.append_replace();
                    self.begin = data + 1;
                    self.err(data, UNNUCH);
                }
                _ => {}
            }
            data += 1;
        }
        self.append_data(data);
        data
    }

    fn st_markup_declaration_open(&mut self, data: usize, end: usize) -> usize {
        if !self.is_eof {
            self.token_set_begin(data);
        }
        let ch = self.c(data);
        if ch == 0x2D {
            if end - data < 2 {
                self.state = St::MarkupDeclComment;
                return data + 1;
            }
            if self.c(data + 1) == 0x2D {
                self.state = St::CommentBeforeStart;
                return data + 2;
            }
        } else if ch == 0x44 || ch == 0x64 {
            if end - data < 7 {
                self.markup_static = b"doctype";
                self.state = St::MarkupDeclDoctype;
                return data;
            }
            if self.mem[data..data + 7].eq_ignore_ascii_case(b"doctype") {
                self.state = St::DoctypeBefore;
                return data + 7;
            }
        } else if ch == 0x5B {
            if end - data < 7 {
                self.markup_static = b"[CDATA[";
                self.state = St::MarkupDeclCdata;
                return data;
            }
            if &self.mem[data..data + 7] == b"[CDATA[" {
                let ns = self.current_namespace();
                if ns != NS_HTML && ns != NS_UNDEF {
                    let d = data + 7;
                    self.token_set_begin(d);
                    self.state = St::CdataSectionBefore;
                    return d;
                }
                self.state = St::BogusCommentBefore;
                return data;
            }
        }
        if self.is_eof {
            self.token.end = self.last;
            self.token.begin = self.token.end;
        }
        self.err(data, INOPCO);
        self.state = St::BogusCommentBefore;
        data
    }

    fn st_markup_declaration_comment(&mut self, data: usize, _end: usize) -> usize {
        if self.c(data) == 0x2D {
            self.state = St::CommentBeforeStart;
            return data + 1;
        }
        self.err(data, INOPCO);
        self.state = St::BogusCommentBefore;
        data
    }

    fn st_markup_declaration_doctype(&mut self, data: usize, end: usize) -> usize {
        let m = self.markup_static;
        let sec = self.mem[data..end].to_vec();
        match ncasecmp_first(m, &sec) {
            None => {
                self.err(data, INOPCO);
                self.state = St::BogusCommentBefore;
                data
            }
            Some(n) if n >= m.len() => {
                self.state = St::DoctypeBefore;
                data + n
            }
            Some(n) => {
                self.markup_static = &m[n..];
                end
            }
        }
    }

    fn st_markup_declaration_cdata(&mut self, data: usize, end: usize) -> usize {
        let m = self.markup_static;
        let sec = self.mem[data..end].to_vec();
        match ncasecmp_first(m, &sec) {
            None => {
                self.err(data, INOPCO);
                self.state = St::BogusCommentBefore;
                data
            }
            Some(n) if n >= m.len() => {
                let ns = self.current_namespace();
                if ns != NS_HTML && ns != NS_UNDEF {
                    self.state = St::CdataSectionBefore;
                    return data + n;
                }
                self.append(b"[CDATA");
                self.state = St::BogusCommentBefore;
                data
            }
            Some(n) => {
                self.markup_static = &m[n..];
                end
            }
        }
    }

    fn st_cdata_section_before(&mut self, data: usize, _end: usize) -> usize {
        if !self.is_eof {
            self.token_set_begin(data);
        } else {
            let l = self.last;
            self.token_set_begin(l);
        }
        self.token.tag_id = _TEXT;
        self.state = St::CdataSection;
        data
    }

    fn st_cdata_section(&mut self, mut data: usize, end: usize) -> usize {
        self.begin = data;
        while data != end {
            match self.c(data) {
                0x5D => {
                    self.append_data(data);
                    self.token.end = data;
                    self.state = St::CdataSectionBracket;
                    return data + 1;
                }
                0x0D => {
                    if let Err(r) = self.cr(&mut data, end, St::CdataSection) {
                        return r;
                    }
                }
                0x00 => {
                    if self.is_eof {
                        let l = self.last;
                        self.err(l, EOINCD);
                        if self.token.begin != 0 {
                            self.append_data(data);
                            self.token.end = self.last;
                        }
                        self.set_text();
                        self.token_done();
                        return end;
                    }
                    self.token.null_count += 1;
                }
                _ => {}
            }
            data += 1;
        }
        self.append_data(data);
        data
    }

    fn st_cdata_section_bracket(&mut self, data: usize, _end: usize) -> usize {
        if self.c(data) == 0x5D {
            self.state = St::CdataSectionEnd;
            return data + 1;
        }
        self.append(b"]");
        self.state = St::CdataSection;
        data
    }

    fn st_cdata_section_end(&mut self, data: usize, _end: usize) -> usize {
        let ch = self.c(data);
        if ch == 0x5D {
            self.append_mem(data, 1);
            return data + 1;
        } else if ch == 0x3E {
            self.state = St::DataBefore;
            self.set_text();
            self.token_done();
            return data + 1;
        }
        self.append(b"]]");
        self.state = St::CdataSection;
        data
    }

    fn st_char_ref(&mut self, data: usize, _end: usize) -> usize {
        let ch = self.c(data);
        if is_alnum(ch) {
            self.entity = 0;
            self.entity_match = None;
            self.entity_start = self.tmp.len().saturating_sub(1);
            self.state = St::CharRefNamed;
            return data;
        } else if ch == 0x23 {
            self.markup_ptr = data;
            self.entity_start = self.tmp.len().saturating_sub(1);
            self.append_mem(data, 1);
            self.state = St::CharRefNumeric;
            return data + 1;
        }
        self.state = self.state_return;
        data
    }

    fn st_char_ref_named(&mut self, mut data: usize, end: usize) -> usize {
        let t = trie();
        let begin = data;
        let mut entry = self.entity;
        while data < end {
            let ch = self.c(data);
            let found = t[entry]
                .children
                .iter()
                .find(|(k, _)| *k == ch)
                .map(|(_, i)| *i);
            match found {
                None => {
                    self.append_mem(begin, data - begin);
                    return self.char_ref_named_done(data);
                }
                Some(i) => {
                    if let Some(v) = t[i].value {
                        self.entity_end = self.tmp.len() + (data - begin);
                        self.entity_match = Some((t[i].key, v));
                    }
                    entry = i;
                }
            }
            data += 1;
        }
        self.entity = entry;
        self.append_mem(begin, end - begin);
        data
    }

    fn char_ref_named_done(&mut self, data: usize) -> usize {
        let (key, value) = match self.entity_match {
            None => {
                self.state = St::CharRefAmbiguous;
                return data;
            }
            Some(m) => m,
        };
        self.state = self.state_return;
        if self.is_attribute && key != 0x3B {
            let ch = self.c(data);
            if ch == 0x3D || is_alnum(ch) {
                return data;
            }
        }
        if key != 0x3B {
            self.err(data, MISEAFCHRE);
        }
        let start = self.entity_start;
        let tail_start = (self.entity_end + 1).min(self.tmp.len());
        let tail = self.tmp[tail_start..].to_vec();
        self.tmp.truncate(start.min(self.tmp.len()));
        self.tmp.extend_from_slice(value);
        self.tmp.extend_from_slice(&tail);
        data
    }

    fn st_char_ref_ambiguous(&mut self, data: usize, _end: usize) -> usize {
        if self.c(data) == 0x3B {
            self.err(data, UNNACHRE);
        }
        self.state = self.state_return;
        data
    }

    fn st_char_ref_numeric(&mut self, data: usize, _end: usize) -> usize {
        self.entity_number = 0;
        let ch = self.c(data);
        if ch == 0x78 || ch == 0x58 {
            self.append_mem(data, 1);
            self.state = St::CharRefHexStart;
            return data + 1;
        }
        self.state = St::CharRefDecStart;
        data
    }

    fn st_char_ref_hex_start(&mut self, data: usize, _end: usize) -> usize {
        if hex_val(self.c(data)).is_some() {
            self.state = St::CharRefHex;
        } else {
            self.err(data, ABOFDIINNUCHRE);
            self.state = self.state_return;
        }
        data
    }

    fn st_char_ref_dec_start(&mut self, data: usize, _end: usize) -> usize {
        if dec_val(self.c(data)).is_some() {
            self.state = St::CharRefDec;
        } else {
            self.err(data, ABOFDIINNUCHRE);
            self.state = self.state_return;
        }
        data
    }

    fn st_char_ref_hex(&mut self, mut data: usize, end: usize) -> usize {
        while data != end {
            let ch = self.c(data);
            match hex_val(ch) {
                None => {
                    self.state = self.state_return;
                    if ch == b';' {
                        data += 1;
                    }
                    return self.char_ref_numeric_end(data);
                }
                Some(v) => {
                    if self.entity_number <= 0x10FFFF {
                        self.entity_number = (self.entity_number << 4) | v;
                    }
                }
            }
            data += 1;
        }
        data
    }

    fn st_char_ref_dec(&mut self, mut data: usize, end: usize) -> usize {
        while data != end {
            let ch = self.c(data);
            match dec_val(ch) {
                None => {
                    self.state = self.state_return;
                    if ch == b';' {
                        data += 1;
                    }
                    return self.char_ref_numeric_end(data);
                }
                Some(v) => {
                    if self.entity_number <= 0x10FFFF {
                        self.entity_number = v + self.entity_number * 10;
                    }
                }
            }
            data += 1;
        }
        data
    }

    fn char_ref_numeric_end(&mut self, data: usize) -> usize {
        let start = self.entity_start.min(self.tmp.len());
        let n = self.entity_number;
        let m = self.markup_ptr;
        let mut fffd = false;
        if n == 0 {
            self.err(m, NUCHRE);
            fffd = true;
        } else if n > 0x10FFFF {
            self.err(m, CHREOUUNRA);
            fffd = true;
        } else if (0xD800..=0xDFFF).contains(&n) {
            self.err(m, SUCHRE);
            fffd = true;
        } else if (0xFDD0..=0xFDEF).contains(&n) {
            self.err(m, NOCHRE);
        }
        self.tmp.truncate(start);
        if fffd {
            self.tmp.extend_from_slice(REPLACEMENT);
            return data;
        }
        if (n & 0xFFFE) == 0xFFFE && n <= 0x10FFFF {
            self.err(m, NOCHRE);
        }
        if (n <= 0x1F && n != 0x09 && n != 0x0A && n != 0x0C && n != 0x20)
            || (0x7F..=0x9F).contains(&n)
        {
            self.err(m, COCHRE);
        }
        let cp = if n <= 0x9F { replacement_char(n) } else { n };
        let mut buf = Vec::new();
        utf8_of(cp, &mut buf);
        self.tmp.extend_from_slice(&buf);
        data
    }

    // ---------------------------------------------------------------------
    // state_comment.c

    fn st_comment_before_start(&mut self, data: usize, end: usize) -> usize {
        if !self.is_eof {
            self.token_set_begin(data);
            self.token.end = data;
        }
        self.token.tag_id = _EM_COMMENT;
        // comment start state
        let ch = self.c(data);
        if ch == 0x2D {
            self.state = St::CommentStartDash;
            data + 1
        } else if ch == 0x3E {
            self.state = St::DataBefore;
            self.err(data, ABCLOFEMCO);
            self.set_text();
            self.token_done_wo_check();
            data + 1
        } else {
            let _ = end;
            self.state = St::Comment;
            data
        }
    }

    fn st_comment_start_dash(&mut self, data: usize, end: usize) -> usize {
        let ch = self.c(data);
        if ch == 0x2D {
            self.state = St::CommentEnd;
            return data + 1;
        } else if ch == 0x3E {
            self.state = St::DataBefore;
            self.err(data, ABCLOFEMCO);
            self.set_text();
            self.token_done_wo_check();
            return data + 1;
        } else if ch == 0x00 && self.is_eof {
            self.append(b"-");
            let l = self.last;
            self.err(l, EOINCO);
            self.set_text();
            self.token_done_wo_check();
            return end;
        }
        self.append(b"-");
        self.state = St::Comment;
        data
    }

    fn st_comment(&mut self, mut data: usize, end: usize) -> usize {
        self.begin = data;
        while data != end {
            match self.c(data) {
                0x3C => {
                    data += 1;
                    self.append_data(data);
                    self.state = St::CommentLt;
                    return data;
                }
                0x2D => {
                    self.token.end = data.wrapping_sub(1);
                    self.append_data(data);
                    self.state = St::CommentEndDash;
                    return data + 1;
                }
                0x0D => {
                    if let Err(r) = self.cr(&mut data, end, St::Comment) {
                        return r;
                    }
                }
                0x00 => {
                    self.append_data(data);
                    if self.is_eof {
                        if self.token.begin != 0 {
                            self.token.end = self.last;
                        }
                        let e = self.token.end;
                        self.err(e, EOINCO);
                        self.set_text();
                        self.token_done();
                        return end;
                    }
                    self.begin = data + 1;
                    self.append_replace();
                    self.err(data, UNNUCH);
                }
                _ => {}
            }
            data += 1;
        }
        self.append_data(data);
        data
    }

    fn st_comment_lt(&mut self, data: usize, _end: usize) -> usize {
        let ch = self.c(data);
        if ch == 0x21 {
            self.append_mem(data, 1);
            self.state = St::CommentLtBang;
            return data + 1;
        } else if ch == 0x3C {
            self.append_mem(data, 1);
            return data + 1;
        }
        self.state = St::Comment;
        data
    }

    fn st_comment_lt_bang(&mut self, data: usize, _end: usize) -> usize {
        if self.c(data) == 0x2D {
            self.state = St::CommentLtBangDash;
            return data + 1;
        }
        self.state = St::Comment;
        data
    }

    fn st_comment_lt_bang_dash(&mut self, data: usize, _end: usize) -> usize {
        if self.c(data) == 0x2D {
            self.state = St::CommentLtBangDashDash;
            return data + 1;
        }
        self.state = St::CommentEndDash;
        data
    }

    fn st_comment_lt_bang_dash_dash(&mut self, data: usize, _end: usize) -> usize {
        let ch = self.c(data);
        if ch == 0x3E || (ch == 0x00 && self.is_eof) {
            self.state = St::CommentEnd;
            return data;
        }
        self.err(data, NECO);
        self.state = St::CommentEnd;
        data
    }

    fn st_comment_end_dash(&mut self, data: usize, end: usize) -> usize {
        let ch = self.c(data);
        if ch == 0x2D {
            self.state = St::CommentEnd;
            return data + 1;
        } else if ch == 0x00 && self.is_eof {
            let l = self.last;
            self.err(l, EOINCO);
            self.set_text();
            self.token_done_wo_check();
            return end;
        }
        self.append(b"-");
        self.state = St::Comment;
        data
    }

    fn st_comment_end(&mut self, data: usize, end: usize) -> usize {
        let ch = self.c(data);
        if ch == 0x3E {
            self.state = St::DataBefore;
            self.set_text();
            self.token_done_wo_check();
            return data + 1;
        } else if ch == 0x21 {
            self.state = St::CommentEndBang;
            return data + 1;
        } else if ch == 0x2D {
            self.append_mem(data, 1);
            return data + 1;
        } else if ch == 0x00 && self.is_eof {
            let l = self.last;
            self.err(l, EOINCO);
            self.set_text();
            self.token_done_wo_check();
            return end;
        }
        self.append(b"--");
        self.state = St::Comment;
        data
    }

    fn st_comment_end_bang(&mut self, data: usize, end: usize) -> usize {
        let ch = self.c(data);
        if ch == 0x2D {
            self.state = St::CommentEndDash;
            return data + 1;
        } else if ch == 0x3E {
            self.state = St::DataBefore;
            self.err(data, INCLCO);
            self.set_text();
            self.token_done_wo_check();
            return data + 1;
        } else if ch == 0x00 && self.is_eof {
            let l = self.last;
            self.err(l, EOINCO);
            self.set_text();
            self.token_done_wo_check();
            return end;
        }
        self.state = St::Comment;
        data
    }

    // ---------------------------------------------------------------------
    // state_doctype.c

    fn doctype_eof(&mut self, pos: usize, end: usize) -> usize {
        self.err(pos, EOINDO);
        self.token.typ |= TOKEN_FORCE_QUIRKS;
        self.token_done();
        end
    }

    fn st_doctype_before(&mut self, mut data: usize, end: usize) -> usize {
        if !self.is_eof {
            self.token.end = data;
        } else {
            self.token.end = self.last;
        }
        self.token.tag_id = _EM_DOCTYPE;
        // DOCTYPE state
        match self.c(data) {
            0x09 | 0x0A | 0x0C | 0x0D | 0x20 => data += 1,
            0x3E => {}
            ch => {
                if ch == 0x00 && self.is_eof {
                    let l = self.last;
                    let r = self.doctype_eof(l, end);
                    let _ = r;
                    return end;
                }
                self.err(data, MIWHBEDONA);
            }
        }
        self.state = St::DoctypeBeforeName;
        data
    }

    fn st_doctype_before_name(&mut self, mut data: usize, end: usize) -> usize {
        while data != end {
            match self.c(data) {
                0x09 | 0x0A | 0x0C | 0x0D | 0x20 => {}
                0x00 => {
                    if self.is_eof {
                        let l = self.last;
                        return self.doctype_eof(l, end);
                    }
                    self.attr_add();
                    self.attr_set_name_begin(data);
                    self.append_replace();
                    self.err(data, UNNUCH);
                    self.state = St::DoctypeName;
                    return data + 1;
                }
                0x3E => {
                    self.token.typ |= TOKEN_FORCE_QUIRKS;
                    self.state = St::DataBefore;
                    self.err(data, MIDONA);
                    self.token_done();
                    return data + 1;
                }
                _ => {
                    self.attr_add();
                    self.attr_set_name_begin(data);
                    self.state = St::DoctypeName;
                    return data;
                }
            }
            data += 1;
        }
        data
    }

    fn st_doctype_name(&mut self, mut data: usize, end: usize) -> usize {
        self.begin = data;
        while data != end {
            match self.c(data) {
                0x09 | 0x0A | 0x0C | 0x0D | 0x20 => {
                    self.append_data(data);
                    self.set_name();
                    self.attr_last().name_end = data;
                    self.state = St::DoctypeAfterName;
                    return data + 1;
                }
                0x3E => {
                    self.state = St::DataBefore;
                    self.append_data(data);
                    self.set_name();
                    self.attr_last().name_end = data;
                    self.token_done();
                    return data + 1;
                }
                0x00 => {
                    self.append_data(data);
                    if self.is_eof {
                        let l = self.last;
                        self.attr_last().name_end = l;
                        self.err(l, EOINDO);
                        self.token.typ |= TOKEN_FORCE_QUIRKS;
                        self.set_name();
                        self.token_done();
                        return end;
                    }
                    self.begin = data + 1;
                    self.append_replace();
                    self.err(data, UNNUCH);
                }
                _ => {}
            }
            data += 1;
        }
        self.append_data(data);
        data
    }

    fn st_doctype_after_name(&mut self, mut data: usize, end: usize) -> usize {
        while data != end {
            match self.c(data) {
                0x09 | 0x0A | 0x0C | 0x0D | 0x20 => {}
                0x3E => {
                    self.state = St::DataBefore;
                    self.token_done();
                    return data + 1;
                }
                ch => {
                    if ch == 0x00 && self.is_eof {
                        let l = self.last;
                        return self.doctype_eof(l, end);
                    }
                    self.attr_add();
                    self.attr_set_name_begin(data);
                    if data + 6 > end {
                        if ch == 0x50 || ch == 0x70 {
                            self.markup_static = b"PUBLIC";
                            self.state = St::DoctypeAfterNamePublic;
                            return data;
                        }
                        if ch == 0x53 || ch == 0x73 {
                            self.markup_static = b"SYSTEM";
                            self.state = St::DoctypeAfterNameSystem;
                            return data;
                        }
                    } else if self.mem[data..data + 6].eq_ignore_ascii_case(b"PUBLIC") {
                        let a = self.attr_last();
                        a.name_end = data + 6;
                        a.name = Some(b"public".to_vec());
                        self.state = St::DoctypeAfterPublicKw;
                        return data + 6;
                    } else if self.mem[data..data + 6].eq_ignore_ascii_case(b"SYSTEM") {
                        let a = self.attr_last();
                        a.name_end = data + 6;
                        a.name = Some(b"system".to_vec());
                        self.state = St::DoctypeAfterSystemKw;
                        return data + 6;
                    }
                    self.token.attrs.pop();
                    self.err(data, INCHSEAFDONA);
                    self.token.typ |= TOKEN_FORCE_QUIRKS;
                    self.state = St::DoctypeBogus;
                    return data;
                }
            }
            data += 1;
        }
        data
    }

    fn st_doctype_after_name_kw(&mut self, data: usize, end: usize, public: bool) -> usize {
        let m = self.markup_static;
        let sec = self.mem[data..end].to_vec();
        match ncasecmp_first(m, &sec) {
            None => {
                self.token.attrs.pop();
                self.err(data, INCHSEAFDONA);
                self.state = St::DoctypeBogus;
                data
            }
            Some(n) if n >= m.len() => {
                let pos = data + n;
                let a = self.attr_last();
                a.name_end = pos;
                if public {
                    a.name = Some(b"public".to_vec());
                    self.state = St::DoctypeAfterPublicKw;
                } else {
                    a.name = Some(b"system".to_vec());
                    self.state = St::DoctypeAfterSystemKw;
                }
                pos + 1
            }
            Some(n) => {
                self.markup_static = &m[n..];
                end
            }
        }
    }

    fn st_doctype_after_public_kw(&mut self, data: usize, end: usize) -> usize {
        match self.c(data) {
            0x09 | 0x0A | 0x0C | 0x0D | 0x20 => {
                self.state = St::DoctypeBeforePublicId;
                data + 1
            }
            0x22 => {
                self.err(data, MIWHAFDOPUKE);
                self.state = St::DoctypePublicIdDq;
                data + 1
            }
            0x27 => {
                self.err(data, MIWHAFDOPUKE);
                self.state = St::DoctypePublicIdSq;
                data + 1
            }
            0x3E => {
                self.token.typ |= TOKEN_FORCE_QUIRKS;
                self.state = St::DataBefore;
                self.err(data, MIDOPUID);
                self.token_done();
                data + 1
            }
            ch => {
                if ch == 0x00 && self.is_eof {
                    self.token.typ |= TOKEN_FORCE_QUIRKS;
                    let l = self.last;
                    self.err(l, EOINDO);
                    self.token_done();
                    return end;
                }
                self.token.typ |= TOKEN_FORCE_QUIRKS;
                self.state = St::DoctypeBogus;
                self.err(data, MIQUBEDOPUID);
                data
            }
        }
    }

    fn st_doctype_before_public_id(&mut self, data: usize, end: usize) -> usize {
        match self.c(data) {
            0x09 | 0x0A | 0x0C | 0x0D | 0x20 => {}
            0x22 => self.state = St::DoctypePublicIdDq,
            0x27 => self.state = St::DoctypePublicIdSq,
            0x3E => {
                self.token.typ |= TOKEN_FORCE_QUIRKS;
                self.state = St::DataBefore;
                self.err(data, MIDOPUID);
                self.token_done();
            }
            ch => {
                if ch == 0x00 && self.is_eof {
                    let l = self.last;
                    return self.doctype_eof(l, end);
                }
                self.err(data, MIQUBEDOPUID);
                self.token.typ |= TOKEN_FORCE_QUIRKS;
                self.state = St::DoctypeBogus;
                return data;
            }
        }
        data + 1
    }

    fn st_doctype_id_quoted(&mut self, mut data: usize, end: usize, q: u8, public: bool) -> usize {
        let me = match (public, q) {
            (true, b'"') => St::DoctypePublicIdDq,
            (true, _) => St::DoctypePublicIdSq,
            (false, b'"') => St::DoctypeSystemIdDq,
            (false, _) => St::DoctypeSystemIdSq,
        };
        if self.attr_value_begin() == 0 && !self.is_eof {
            self.attr_set_value_begin(data);
        }
        self.begin = data;
        while data != end {
            let ch = self.c(data);
            if ch == q {
                self.append_data(data);
                self.set_value();
                self.attr_last().value_end = data;
                self.state = if public {
                    St::DoctypeAfterPublicId
                } else {
                    St::DoctypeAfterSystemId
                };
                return data + 1;
            }
            match ch {
                0x3E => {
                    self.state = St::DataBefore;
                    self.err(data, if public { ABDOPUID } else { ABDOSYID });
                    self.append_data(data);
                    self.set_value();
                    self.attr_last().value_end = data;
                    self.token_done();
                    return data + 1;
                }
                0x0D => {
                    if let Err(r) = self.cr(&mut data, end, me) {
                        return r;
                    }
                }
                0x00 => {
                    self.append_data(data);
                    if self.is_eof {
                        let l = self.last;
                        self.attr_last().value_end = l;
                        if self.attr_value_begin() == 0 {
                            self.attr_last().value_begin = l;
                        }
                        self.err(l, EOINDO);
                        self.token.typ |= TOKEN_FORCE_QUIRKS;
                        self.set_value();
                        self.token_done();
                        return end;
                    }
                    self.begin = data + 1;
                    self.append_replace();
                    self.err(data, UNNUCH);
                }
                _ => {}
            }
            data += 1;
        }
        self.append_data(data);
        data
    }

    fn st_doctype_after_public_id(&mut self, data: usize, end: usize) -> usize {
        match self.c(data) {
            0x09 | 0x0A | 0x0C | 0x0D | 0x20 => {
                self.state = St::DoctypeBetween;
                data + 1
            }
            0x3E => {
                self.state = St::DataBefore;
                self.token_done();
                data + 1
            }
            0x22 => {
                self.err(data, MIWHBEDOPUANSYID);
                self.attr_add();
                self.state = St::DoctypeSystemIdDq;
                data + 1
            }
            0x27 => {
                self.err(data, MIWHBEDOPUANSYID);
                self.attr_add();
                self.state = St::DoctypeSystemIdSq;
                data + 1
            }
            ch => {
                if ch == 0x00 && self.is_eof {
                    let l = self.last;
                    return self.doctype_eof(l, end);
                }
                self.err(data, MIQUBEDOSYID);
                self.token.typ |= TOKEN_FORCE_QUIRKS;
                self.state = St::DoctypeBogus;
                data
            }
        }
    }

    fn st_doctype_between(&mut self, data: usize, end: usize) -> usize {
        match self.c(data) {
            0x09 | 0x0A | 0x0C | 0x0D | 0x20 => data + 1,
            0x3E => {
                self.state = St::DataBefore;
                self.token_done();
                data + 1
            }
            0x22 => {
                self.attr_add();
                self.state = St::DoctypeSystemIdDq;
                data + 1
            }
            0x27 => {
                self.attr_add();
                self.state = St::DoctypeSystemIdSq;
                data + 1
            }
            ch => {
                if ch == 0x00 && self.is_eof {
                    let l = self.last;
                    return self.doctype_eof(l, end);
                }
                self.err(data, MIQUBEDOSYID);
                self.token.typ |= TOKEN_FORCE_QUIRKS;
                self.state = St::DoctypeBogus;
                data
            }
        }
    }

    fn st_doctype_after_system_kw(&mut self, data: usize, end: usize) -> usize {
        match self.c(data) {
            0x09 | 0x0A | 0x0C | 0x0D | 0x20 => {
                self.state = St::DoctypeBeforeSystemId;
                data + 1
            }
            0x22 => {
                self.err(data, MIWHAFDOSYKE);
                self.state = St::DoctypeSystemIdDq;
                data + 1
            }
            0x27 => {
                self.err(data, MIWHAFDOSYKE);
                self.state = St::DoctypeSystemIdSq;
                data + 1
            }
            0x3E => {
                self.token.typ |= TOKEN_FORCE_QUIRKS;
                self.state = St::DataBefore;
                self.err(data, MIDOSYID);
                self.token_done();
                data + 1
            }
            ch => {
                if ch == 0x00 && self.is_eof {
                    let l = self.last;
                    return self.doctype_eof(l, end);
                }
                self.token.typ |= TOKEN_FORCE_QUIRKS;
                self.state = St::DoctypeBogus;
                self.err(data, MIQUBEDOSYID);
                data
            }
        }
    }

    fn st_doctype_before_system_id(&mut self, data: usize, end: usize) -> usize {
        match self.c(data) {
            0x09 | 0x0A | 0x0C | 0x0D | 0x20 => data + 1,
            0x22 => {
                self.state = St::DoctypeSystemIdDq;
                data + 1
            }
            0x27 => {
                self.state = St::DoctypeSystemIdSq;
                data + 1
            }
            0x3E => {
                self.token.typ |= TOKEN_FORCE_QUIRKS;
                self.state = St::DataBefore;
                self.err(data, MIDOSYID);
                self.token_done();
                data + 1
            }
            ch => {
                if ch == 0x00 && self.is_eof {
                    let l = self.last;
                    return self.doctype_eof(l, end);
                }
                self.token.typ |= TOKEN_FORCE_QUIRKS;
                self.state = St::DoctypeBogus;
                self.err(data, MIQUBEDOSYID);
                data
            }
        }
    }

    fn st_doctype_after_system_id(&mut self, data: usize, end: usize) -> usize {
        match self.c(data) {
            0x09 | 0x0A | 0x0C | 0x0D | 0x20 => data + 1,
            0x3E => {
                self.state = St::DataBefore;
                self.token_done();
                data + 1
            }
            ch => {
                if ch == 0x00 && self.is_eof {
                    let l = self.last;
                    return self.doctype_eof(l, end);
                }
                self.err(data, UNCHAFDOSYID);
                self.state = St::DoctypeBogus;
                data
            }
        }
    }

    fn st_doctype_bogus(&mut self, mut data: usize, end: usize) -> usize {
        while data != end {
            match self.c(data) {
                0x3E => {
                    self.state = St::DataBefore;
                    self.token_done();
                    return data + 1;
                }
                0x00 => {
                    if self.is_eof {
                        self.token_done();
                        return end;
                    }
                    self.err(data, UNNUCH);
                }
                _ => {}
            }
            data += 1;
        }
        data
    }

    // ---------------------------------------------------------------------
    // state_rcdata.c / state_rawtext.c / state_script.c

    fn st_text_before(&mut self, data: usize, next: St) -> usize {
        if !self.is_eof {
            self.token_set_begin(data);
        }
        self.state = next;
        data
    }

    fn st_rawtext(&mut self, mut data: usize, end: usize, rcdata: bool) -> usize {
        let me = if rcdata { St::Rcdata } else { St::Rawtext };
        self.begin = data;
        while data != end {
            match self.c(data) {
                0x3C => {
                    self.append_data(data + 1);
                    self.token.end = data;
                    self.state = if rcdata { St::RcdataLt } else { St::RawtextLt };
                    return data + 1;
                }
                0x26 if rcdata => {
                    self.append_data(data + 1);
                    self.state = St::CharRef;
                    self.state_return = St::Rcdata;
                    return data + 1;
                }
                0x0D => {
                    if let Err(r) = self.cr(&mut data, end, me) {
                        return r;
                    }
                }
                0x00 => {
                    self.append_data(data);
                    if self.is_eof {
                        if self.token.begin != 0 {
                            self.token.end = self.last;
                        }
                        self.token.tag_id = _TEXT;
                        self.set_text();
                        self.token_done();
                        return end;
                    }
                    self.begin = data + 1;
                    self.append_replace();
                    self.err(data, UNNUCH);
                }
                _ => {}
            }
            data += 1;
        }
        self.append_data(data);
        data
    }

    fn st_rawtext_lt(&mut self, data: usize, open: St, back: St) -> usize {
        if self.c(data) == 0x2F {
            self.state = open;
            return data + 1;
        }
        self.state = back;
        data
    }

    fn st_end_tag_open_generic(&mut self, data: usize, name: St, back: St) -> usize {
        if is_alpha(self.c(data)) {
            self.temp = data;
            self.entity_start = self.tmp.len().saturating_sub(1);
            self.state = name;
        } else {
            self.state = back;
        }
        self.append(b"/");
        data
    }

    fn st_end_tag_name_generic(
        &mut self,
        mut data: usize,
        end: usize,
        back: St,
        set_begin_temp: bool,
    ) -> usize {
        self.begin = data;
        while data != end {
            let ch = self.c(data);
            match ch {
                0x09 | 0x0A | 0x0C | 0x0D | 0x20 | 0x2F | 0x3E => {
                    self.append_data(data);
                    self.set_tag(self.entity_start + 2);
                    if self.tmp_tag_id != self.token.tag_id {
                        self.state = back;
                        return data;
                    }
                    // emit the text token
                    self.token.tag_id = _TEXT;
                    self.tmp.truncate(self.entity_start.min(self.tmp.len()));
                    self.set_text();
                    self.token_done();
                    // the close token
                    self.token.tag_id = self.tmp_tag_id;
                    if ch == 0x3E || set_begin_temp {
                        self.token.begin = self.temp;
                    }
                    self.token.end = data;
                    self.token.typ |= TOKEN_CLOSE;
                    if ch == 0x3E {
                        self.state = St::DataBefore;
                        self.token_done();
                    } else if ch == 0x2F {
                        self.state = St::SelfClosingStartTag;
                    } else {
                        self.state = St::BeforeAttrName;
                    }
                    return data + 1;
                }
                _ => {
                    if !is_alpha(ch) {
                        self.append_data(data);
                        self.state = back;
                        return data;
                    }
                }
            }
            data += 1;
        }
        self.append_data(data);
        data
    }

    fn st_script_data(&mut self, mut data: usize, end: usize) -> usize {
        self.begin = data;
        while data != end {
            match self.c(data) {
                0x3C => {
                    self.append_data(data + 1);
                    self.token.end = data;
                    self.state = St::ScriptDataLt;
                    return data + 1;
                }
                0x0D => {
                    if let Err(r) = self.cr(&mut data, end, St::ScriptData) {
                        return r;
                    }
                }
                0x00 => {
                    self.append_data(data);
                    if self.is_eof {
                        if self.token.begin != 0 {
                            self.token.end = self.last;
                        }
                        self.token.tag_id = _TEXT;
                        self.set_text();
                        self.token_done();
                        return end;
                    }
                    self.begin = data + 1;
                    self.append_replace();
                    self.err(data, UNNUCH);
                }
                _ => {}
            }
            data += 1;
        }
        self.append_data(data);
        data
    }

    fn st_script_data_lt(&mut self, data: usize, _end: usize) -> usize {
        match self.c(data) {
            0x2F => {
                self.state = St::ScriptDataEndTagOpen;
                data + 1
            }
            0x21 => {
                self.state = St::ScriptDataEscapeStart;
                data + 1
            }
            _ => {
                self.state = St::ScriptData;
                data
            }
        }
    }

    fn st_script_escape_start(&mut self, data: usize, _end: usize) -> usize {
        if self.c(data) == 0x2D {
            self.state = St::ScriptDataEscapeStartDash;
            return data + 1;
        }
        self.append(b"!");
        self.state = St::ScriptData;
        data
    }

    fn st_script_escape_start_dash(&mut self, data: usize, _end: usize) -> usize {
        if self.c(data) == 0x2D {
            self.append(b"!--");
            self.state = St::ScriptDataEscapedDashDash;
            return data + 1;
        }
        self.append(b"!-");
        self.state = St::ScriptData;
        data
    }

    fn script_eof_text(&mut self, end: usize) -> usize {
        let l = self.last;
        self.err(l, EOINSCHTCOLITE);
        self.token.tag_id = _TEXT;
        self.set_text();
        self.token.end = self.last;
        self.token_done();
        end
    }

    fn st_script_escaped(&mut self, mut data: usize, end: usize) -> usize {
        self.begin = data;
        while data != end {
            match self.c(data) {
                0x2D => {
                    self.append_data(data + 1);
                    self.state = St::ScriptDataEscapedDash;
                    return data + 1;
                }
                0x3C => {
                    self.append_data(data + 1);
                    self.token.end = data;
                    self.state = St::ScriptDataEscapedLt;
                    return data + 1;
                }
                0x0D => {
                    if let Err(r) = self.cr(&mut data, end, St::ScriptDataEscaped) {
                        return r;
                    }
                }
                0x00 => {
                    self.append_data(data);
                    if self.is_eof {
                        return self.script_eof_text(end);
                    }
                    self.begin = data + 1;
                    self.append_replace();
                    self.err(data, UNNUCH);
                }
                _ => {}
            }
            data += 1;
        }
        self.append_data(data);
        data
    }

    fn st_script_escaped_dash(&mut self, data: usize, end: usize) -> usize {
        match self.c(data) {
            0x2D => {
                self.append_mem(data, 1);
                self.state = St::ScriptDataEscapedDashDash;
                data + 1
            }
            0x3C => {
                self.append_mem(data, 1);
                self.token.end = data;
                self.state = St::ScriptDataEscapedLt;
                data + 1
            }
            0x00 => {
                if self.is_eof {
                    return self.script_eof_text(end);
                }
                self.append_replace();
                self.err(data, UNNUCH);
                self.state = St::ScriptDataEscaped;
                data + 1
            }
            _ => {
                self.state = St::ScriptDataEscaped;
                data
            }
        }
    }

    fn st_script_escaped_dash_dash(&mut self, data: usize, _end: usize) -> usize {
        match self.c(data) {
            0x2D => {
                self.append(b"-");
                data + 1
            }
            0x3C => {
                self.append(b"<");
                self.token.end = data;
                self.state = St::ScriptDataEscapedLt;
                data + 1
            }
            0x3E => {
                self.state = St::ScriptData;
                data
            }
            _ => {
                self.state = St::ScriptDataEscaped;
                data
            }
        }
    }

    fn st_script_escaped_lt(&mut self, data: usize, _end: usize) -> usize {
        let ch = self.c(data);
        if ch == 0x2F {
            self.state = St::ScriptDataEscapedEndTagOpen;
            return data + 1;
        }
        if is_alpha(ch) {
            self.entity_start = self.tmp.len();
            self.state = St::ScriptDataDoubleEscapeStart;
            return data;
        }
        self.state = St::ScriptDataEscaped;
        data
    }

    fn st_script_double_escape_start(&mut self, mut data: usize, end: usize) -> usize {
        self.begin = data;
        while data != end {
            let ch = self.c(data);
            match ch {
                0x09 | 0x0A | 0x0C | 0x0D | 0x20 | 0x2F | 0x3E => {
                    self.append_data(data);
                    let s = self.entity_start.min(self.tmp.len());
                    if self.tmp.len() - s == 6 && self.tmp[s..].eq_ignore_ascii_case(b"script") {
                        self.state = St::ScriptDataDoubleEscaped;
                        return data;
                    }
                    self.state = St::ScriptDataEscaped;
                    return data;
                }
                _ => {
                    if !is_alpha(ch) {
                        self.append_data(data);
                        self.state = St::ScriptDataEscaped;
                        return data;
                    }
                }
            }
            data += 1;
        }
        self.append_data(data);
        data
    }

    fn st_script_double_escaped(&mut self, mut data: usize, end: usize) -> usize {
        self.begin = data;
        while data != end {
            match self.c(data) {
                0x2D => {
                    self.append_data(data + 1);
                    self.state = St::ScriptDataDoubleEscapedDash;
                    return data + 1;
                }
                0x3C => {
                    self.append_data(data + 1);
                    self.state = St::ScriptDataDoubleEscapedLt;
                    return data + 1;
                }
                0x0D => {
                    if let Err(r) = self.cr(&mut data, end, St::ScriptDataDoubleEscaped) {
                        return r;
                    }
                }
                0x00 => {
                    self.append_data(data);
                    if self.is_eof {
                        return self.script_eof_text(end);
                    }
                    self.begin = data + 1;
                    self.append_replace();
                    self.err(data, UNNUCH);
                }
                _ => {}
            }
            data += 1;
        }
        self.append_data(data);
        data
    }

    fn st_script_double_escaped_dash(&mut self, data: usize, end: usize) -> usize {
        match self.c(data) {
            0x2D => {
                self.append_mem(data, 1);
                self.state = St::ScriptDataDoubleEscapedDashDash;
                data + 1
            }
            0x3C => {
                self.append_mem(data, 1);
                self.state = St::ScriptDataDoubleEscapedLt;
                data + 1
            }
            0x00 => {
                if self.is_eof {
                    return self.script_eof_text(end);
                }
                self.append_replace();
                self.err(data, UNNUCH);
                self.state = St::ScriptDataDoubleEscaped;
                data + 1
            }
            _ => {
                self.state = St::ScriptDataDoubleEscaped;
                data
            }
        }
    }

    fn st_script_double_escaped_dash_dash(&mut self, data: usize, end: usize) -> usize {
        match self.c(data) {
            0x2D => {
                self.append_mem(data, 1);
                data + 1
            }
            0x3C => {
                self.append_mem(data, 1);
                self.state = St::ScriptDataDoubleEscapedLt;
                data + 1
            }
            0x3E => {
                self.append_mem(data, 1);
                self.state = St::ScriptData;
                data + 1
            }
            0x00 => {
                if self.is_eof {
                    return self.script_eof_text(end);
                }
                self.append_replace();
                self.err(data, UNNUCH);
                self.state = St::ScriptDataDoubleEscaped;
                data + 1
            }
            _ => {
                self.state = St::ScriptDataDoubleEscaped;
                data
            }
        }
    }

    fn st_script_double_escape_end(&mut self, mut data: usize, end: usize) -> usize {
        self.begin = data;
        while data != end {
            let ch = self.c(data);
            match ch {
                0x09 | 0x0A | 0x0C | 0x0D | 0x20 | 0x2F | 0x3E => {
                    self.append_data(data);
                    let s = self.entity_start.min(self.tmp.len());
                    if self.tmp.len() - s == 6 && self.tmp[s..].eq_ignore_ascii_case(b"script") {
                        self.state = St::ScriptDataEscaped;
                        return data;
                    }
                    self.state = St::ScriptDataDoubleEscaped;
                    return data;
                }
                _ => {
                    if !is_alpha(ch) {
                        self.append_data(data);
                        self.state = St::ScriptDataDoubleEscaped;
                        return data;
                    }
                }
            }
            data += 1;
        }
        self.append_data(data);
        data
    }
}
