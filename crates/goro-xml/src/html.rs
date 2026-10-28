//! The legacy HTML parser and serializer behind `DOMDocument::loadHTML()`
//! and `saveHTML()` (PHP 8.5 with libxml2 2.15, observed; fixtures
//! `tests/fixtures/loadhtml.php`).
//!
//! Tokenization follows the WHATWG HTML tokenizer (character references,
//! attribute and comment rules, bogus comments for `<?...>`); tree
//! construction is the lenient HTML 4 one: implied `html`, `head` and
//! `body`, elements closed by start tags that cannot nest in them (`p` by
//! block elements, `li` by `li`, table cells by cells and rows), end tags
//! that close intervening elements with a mismatch error, and no table
//! fix-ups.

use crate::error::{Domain, LEVEL_ERROR, XmlError};
use crate::html4_entities::HTML4_ENTITIES;
use crate::tree::{Arena, AttrData, DocKind, DtdData, NodeData, NodeId, QName, Str};
use crate::xml::opt;

/// `LIBXML_HTML_NOIMPLIED`
pub const HTML_NOIMPLIED: u32 = 1 << 13;
/// `LIBXML_HTML_NODEFDTD`
pub const HTML_NODEFDTD: u32 = 1 << 2;

/// Element properties (HTML 4.01 DTD element declarations; HTML5 void and
/// raw text elements).
#[derive(Clone, Copy, Debug, Default)]
pub struct ElementInfo {
    /// Has no content and no end tag.
    pub void: bool,
    /// Phrase-level element (HTML 4.01 %inline).
    pub inline: bool,
    /// The end tag may be omitted.
    pub optional_end: bool,
    /// Content is raw text (`script`, `style`).
    pub raw: bool,
    /// Content is escapable raw text (`textarea`, `title`).
    pub rcdata: bool,
    /// Belongs in `head`.
    pub head: bool,
}

/// Properties of a known element (`None` for unknown elements).
pub fn element_info(name: &str) -> Option<ElementInfo> {
    let mut i = ElementInfo::default();
    match name {
        "area" | "base" | "basefont" | "br" | "col" | "embed" | "frame" | "hr" | "img"
        | "input" | "isindex" | "keygen" | "link" | "meta" | "param" | "source" | "track"
        | "wbr" => {
            i.void = true;
            i.inline = matches!(name, "br" | "img" | "input" | "basefont" | "wbr" | "embed");
            i.head = matches!(name, "base" | "link" | "meta");
        }
        "a" | "abbr" | "acronym" | "applet" | "b" | "bdo" | "big" | "button" | "cite" | "code"
        | "del" | "dfn" | "em" | "font" | "i" | "iframe" | "ins" | "kbd" | "label" | "map"
        | "object" | "q" | "s" | "samp" | "select" | "small" | "span" | "strike" | "strong"
        | "sub" | "sup" | "tt" | "u" | "var" | "bdi" | "data" | "mark" | "output" | "time"
        | "ruby" => i.inline = true,
        "script" => {
            i.raw = true;
            i.inline = true;
            i.head = true;
        }
        "style" => {
            i.raw = true;
            i.head = true;
        }
        "textarea" => {
            i.rcdata = true;
            i.inline = true;
        }
        "title" => {
            i.rcdata = true;
            i.head = true;
        }
        "html" | "head" | "body" | "p" | "li" | "dt" | "dd" | "option" | "optgroup" | "tr"
        | "td" | "th" | "thead" | "tbody" | "tfoot" | "colgroup" | "rt" | "rp" => {
            i.optional_end = true
        }
        "address" | "article" | "aside" | "blockquote" | "caption" | "center" | "details"
        | "dialog" | "dir" | "div" | "dl" | "fieldset" | "figcaption" | "figure" | "footer"
        | "form" | "frameset" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "header" | "hgroup"
        | "legend" | "main" | "menu" | "nav" | "noframes" | "noscript" | "ol" | "pre"
        | "section" | "summary" | "table" | "ul" | "xmp" | "listing" | "plaintext" | "search"
        | "template" => {}
        _ => return None,
    }
    Some(i)
}

/// Elements whose start tag closes an open `p` (WHATWG HTML tree
/// construction, "close a p element").
fn closes_p(name: &str) -> bool {
    matches!(
        name,
        "address"
            | "article"
            | "aside"
            | "blockquote"
            | "center"
            | "details"
            | "dialog"
            | "dir"
            | "div"
            | "dl"
            | "fieldset"
            | "figcaption"
            | "figure"
            | "footer"
            | "header"
            | "hgroup"
            | "main"
            | "menu"
            | "nav"
            | "ol"
            | "p"
            | "search"
            | "section"
            | "summary"
            | "ul"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "pre"
            | "listing"
            | "form"
            | "li"
            | "dd"
            | "dt"
            | "plaintext"
            | "table"
            | "hr"
            | "xmp"
    )
}

/// True if a start tag `new` closes the open element `open` (the HTML 4
/// rules for omitted end tags).
fn start_closes(new: &str, open: &str) -> bool {
    match open {
        "p" => closes_p(new),
        "li" => new == "li",
        "dt" | "dd" => matches!(new, "dt" | "dd"),
        "option" => matches!(new, "option" | "optgroup"),
        "optgroup" => new == "optgroup",
        "td" | "th" => matches!(new, "td" | "th" | "tr" | "tbody" | "thead" | "tfoot"),
        "tr" => matches!(new, "tr" | "tbody" | "thead" | "tfoot"),
        "thead" | "tbody" | "tfoot" => matches!(new, "tbody" | "thead" | "tfoot"),
        "colgroup" => matches!(new, "colgroup" | "thead" | "tbody" | "tfoot" | "tr"),
        "caption" => matches!(new, "tr" | "tbody" | "thead" | "tfoot" | "colgroup" | "col"),
        "a" => new == "a",
        "rt" | "rp" => matches!(new, "rt" | "rp"),
        _ => false,
    }
}

/// HTML 4.01 boolean attributes (printed minimized by `saveHTML`).
pub fn is_boolean_attr(name: &str) -> bool {
    matches!(
        name,
        "checked"
            | "compact"
            | "declare"
            | "defer"
            | "disabled"
            | "ismap"
            | "multiple"
            | "nohref"
            | "noresize"
            | "noshade"
            | "nowrap"
            | "readonly"
            | "selected"
    )
}

/// Attributes holding URIs (HTML 4.01 %URI), escaped on output.
fn is_uri_attr(el: &str, name: &str) -> bool {
    matches!(
        name,
        "href"
            | "src"
            | "action"
            | "background"
            | "codebase"
            | "data"
            | "longdesc"
            | "usemap"
            | "cite"
            | "classid"
            | "profile"
    ) || (name == "name" && el == "a")
}

// ----- tokenizer --------------------------------------------------------------

#[derive(Debug)]
enum Token {
    Start {
        name: String,
        attrs: Vec<(String, Option<String>)>,
        end: usize,
    },
    End {
        name: String,
        end: usize,
    },
    Text(String),
    Comment(String),
    Doctype {
        name: String,
        public: Option<String>,
        system: Option<String>,
    },
}

/// Windows-1252 remapping of C1 code points in numeric character
/// references (WHATWG HTML, "numeric character reference end state").
fn c1_remap(v: u32) -> u32 {
    const T: [u32; 32] = [
        0x20AC, 0x81, 0x201A, 0x0192, 0x201E, 0x2026, 0x2020, 0x2021, 0x02C6, 0x2030, 0x0160,
        0x2039, 0x0152, 0x8D, 0x017D, 0x8F, 0x90, 0x2018, 0x2019, 0x201C, 0x201D, 0x2022, 0x2013,
        0x2014, 0x02DC, 0x2122, 0x0161, 0x203A, 0x0153, 0x9D, 0x017E, 0x0178,
    ];
    if (0x80..=0x9F).contains(&v) {
        T[(v - 0x80) as usize]
    } else {
        v
    }
}

/// Looks up a named reference (the HTML 4.01 names plus `apos`).
fn named_entity(name: &str) -> Option<char> {
    if name == "apos" {
        return Some('\'');
    }
    HTML4_ENTITIES
        .iter()
        .find(|(_, n)| *n == name)
        .and_then(|(c, _)| char::from_u32(*c))
}

/// The HTML 4.01 name of a character, for output.
pub fn entity_name(c: char) -> Option<&'static str> {
    let v = c as u32;
    HTML4_ENTITIES
        .binary_search_by_key(&v, |(c, _)| *c)
        .ok()
        .map(|i| HTML4_ENTITIES[i].1)
}

struct Tokenizer<'a> {
    s: &'a str,
    b: &'a [u8],
    pos: usize,
}

impl<'a> Tokenizer<'a> {
    /// Decodes character references in `text` (HTML5 rules; `attr` changes
    /// the handling of references without `;`).
    fn decode_refs(text: &str, attr: bool) -> String {
        if !text.contains('&') {
            return text.to_string();
        }
        let mut out = String::with_capacity(text.len());
        let b = text.as_bytes();
        let mut i = 0;
        while i < b.len() {
            if b[i] != b'&' {
                let next = text[i..].find('&').map_or(text.len(), |k| i + k);
                out.push_str(&text[i..next]);
                i = next;
                continue;
            }
            // Numeric.
            if i + 1 < b.len() && b[i + 1] == b'#' {
                let mut j = i + 2;
                let hex = j < b.len() && (b[j] == b'x' || b[j] == b'X');
                if hex {
                    j += 1;
                }
                let ds = j;
                while j < b.len()
                    && (if hex {
                        b[j].is_ascii_hexdigit()
                    } else {
                        b[j].is_ascii_digit()
                    })
                {
                    j += 1;
                }
                if j == ds {
                    out.push('&');
                    i += 1;
                    continue;
                }
                let digits = &text[ds..j];
                let v = u32::from_str_radix(digits, if hex { 16 } else { 10 }).unwrap_or(0x110000);
                if j < b.len() && b[j] == b';' {
                    j += 1;
                }
                let v = c1_remap(v);
                let c = if v == 0 || v > 0x10FFFF || (0xD800..=0xDFFF).contains(&v) {
                    '\u{FFFD}'
                } else {
                    char::from_u32(v).unwrap_or('\u{FFFD}')
                };
                out.push(c);
                i = j;
                continue;
            }
            // Named.
            let mut j = i + 1;
            while j < b.len() && b[j].is_ascii_alphanumeric() {
                j += 1;
            }
            let name = &text[i + 1..j];
            if !name.is_empty() {
                if j < b.len() && b[j] == b';' {
                    if let Some(c) = named_entity(name) {
                        out.push(c);
                        i = j + 1;
                        continue;
                    }
                } else {
                    // Legacy references without ';' (amp, lt, gt, quot and
                    // the Latin-1 names), not in attributes before = or
                    // alphanumerics.
                    let mut k = name.len();
                    let mut found = None;
                    while k > 0 {
                        if let Some(c) = named_entity(&name[..k])
                            && (c as u32) < 0x100
                        {
                            found = Some((k, c));
                            break;
                        }
                        k -= 1;
                    }
                    if let Some((k, c)) = found {
                        let after = b.get(i + 1 + k).copied();
                        let blocked =
                            attr && after.is_some_and(|a| a.is_ascii_alphanumeric() || a == b'=');
                        if !blocked {
                            out.push(c);
                            i = i + 1 + k;
                            continue;
                        }
                    }
                }
            }
            out.push('&');
            i += 1;
        }
        out
    }

    fn skip_ws(&mut self) {
        while self.pos < self.b.len()
            && matches!(self.b[self.pos], b' ' | b'\t' | b'\n' | b'\r' | 0x0C)
        {
            self.pos += 1;
        }
    }

    /// Next token, or `None` at the end. `raw` is the raw-text element
    /// whose content is being read, if any.
    fn next(&mut self, raw: Option<(&str, bool)>) -> Option<Token> {
        if self.pos >= self.b.len() {
            return None;
        }
        if let Some((name, rcdata)) = raw {
            // Raw text up to "</name".
            let start = self.pos;
            let mut i = self.pos;
            loop {
                match memchr::memmem::find(&self.b[i..], b"</") {
                    Some(off) => {
                        let at = i + off;
                        let n = &self.b[at + 2..];
                        if n.len() >= name.len()
                            && n[..name.len()].eq_ignore_ascii_case(name.as_bytes())
                            && n.get(name.len()).is_none_or(|c| {
                                matches!(c, b'>' | b'/' | b' ' | b'\t' | b'\n' | b'\r' | 0x0C)
                            })
                        {
                            if at == start {
                                // No text before the end tag: tokenize it.
                                break;
                            }
                            self.pos = at;
                            let t = &self.s[start..at];
                            return Some(Token::Text(if rcdata {
                                Self::decode_refs(t, false)
                            } else {
                                t.to_string()
                            }));
                        }
                        i = at + 2;
                    }
                    None => {
                        self.pos = self.b.len();
                        let t = &self.s[start..];
                        return Some(Token::Text(if rcdata {
                            Self::decode_refs(t, false)
                        } else {
                            t.to_string()
                        }));
                    }
                }
            }
        }
        if self.b[self.pos] != b'<' {
            let start = self.pos;
            let end =
                memchr::memchr(b'<', &self.b[self.pos..]).map_or(self.b.len(), |o| self.pos + o);
            self.pos = end;
            let raw: String = self.s[start..end].chars().filter(|&c| c != '\0').collect();
            return Some(Token::Text(Self::decode_refs(&raw, false)));
        }
        let lt = self.pos;
        let n = self.b.get(lt + 1).copied();
        match n {
            Some(b'!') => {
                if self.s[lt..].starts_with("<!--") {
                    let body_start = lt + 4;
                    let (text, end) = match self.s[body_start..].find("-->") {
                        Some(o) => (&self.s[body_start..body_start + o], body_start + o + 3),
                        None => (&self.s[body_start..], self.b.len()),
                    };
                    // "<!-->" and "<!--->" end immediately.
                    let (text, end) = if self.s[body_start..].starts_with('>') {
                        ("", body_start + 1)
                    } else if self.s[body_start..].starts_with("->") {
                        ("", body_start + 2)
                    } else {
                        (text, end)
                    };
                    self.pos = end;
                    return Some(Token::Comment(text.to_string()));
                }
                if self.s.len() >= lt + 9 && self.s[lt + 2..lt + 9].eq_ignore_ascii_case("DOCTYPE")
                {
                    return Some(self.doctype(lt + 9));
                }
                // Bogus comment.
                let end = memchr::memchr(b'>', &self.b[lt..]).map_or(self.b.len(), |o| lt + o);
                let text = &self.s[lt + 2..end];
                self.pos = (end + 1).min(self.b.len());
                return Some(Token::Comment(text.to_string()));
            }
            Some(b'?') => {
                let end = memchr::memchr(b'>', &self.b[lt..]).map_or(self.b.len(), |o| lt + o);
                let text = &self.s[lt + 1..end];
                self.pos = (end + 1).min(self.b.len());
                Some(Token::Comment(text.to_string()))
            }
            Some(b'/') => {
                let ns = lt + 2;
                match self.b.get(ns) {
                    Some(c) if c.is_ascii_alphabetic() => {
                        let mut j = ns;
                        while j < self.b.len()
                            && !matches!(
                                self.b[j],
                                b'>' | b'/' | b' ' | b'\t' | b'\n' | b'\r' | 0x0C
                            )
                        {
                            j += 1;
                        }
                        let name = self.s[ns..j].to_ascii_lowercase();
                        let end =
                            memchr::memchr(b'>', &self.b[j..]).map_or(self.b.len(), |o| j + o);
                        self.pos = (end + 1).min(self.b.len());
                        Some(Token::End { name, end })
                    }
                    Some(b'>') => {
                        self.pos = ns + 1;
                        self.next(None)
                    }
                    None => {
                        self.pos = self.b.len();
                        Some(Token::Text("</".into()))
                    }
                    _ => {
                        let end =
                            memchr::memchr(b'>', &self.b[lt..]).map_or(self.b.len(), |o| lt + o);
                        let text = &self.s[lt + 2..end];
                        self.pos = (end + 1).min(self.b.len());
                        Some(Token::Comment(text.to_string()))
                    }
                }
            }
            Some(c) if c.is_ascii_alphabetic() => Some(self.start_tag(lt)),
            _ => {
                self.pos = lt + 1;
                Some(Token::Text("<".into()))
            }
        }
    }

    fn doctype(&mut self, mut i: usize) -> Token {
        let end = memchr::memchr(b'>', &self.b[i..]).map_or(self.b.len(), |o| i + o);
        let body = &self.s[i..end];
        self.pos = (end + 1).min(self.b.len());
        let _ = &mut i;
        let mut words = body.trim_start();
        let name_end = words
            .find(|c: char| c.is_ascii_whitespace())
            .unwrap_or(words.len());
        let name = words[..name_end].to_string();
        words = words[name_end..].trim_start();
        let mut public = None;
        let mut system = None;
        let lit = |w: &str| -> Option<(String, usize)> {
            let w = w.trim_start();
            let off_ws = 0;
            let q = w.chars().next()?;
            if q != '"' && q != '\'' {
                return None;
            }
            let close = w[1..].find(q)?;
            Some((w[1..1 + close].to_string(), off_ws + close + 2))
        };
        if words.len() >= 6 && words[..6].eq_ignore_ascii_case("PUBLIC") {
            let rest = words[6..].trim_start();
            if let Some((p, used)) = lit(rest) {
                public = Some(p);
                if let Some((s, _)) = lit(&rest[used..]) {
                    system = Some(s);
                }
            }
        } else if words.len() >= 6 && words[..6].eq_ignore_ascii_case("SYSTEM") {
            if let Some((s, _)) = lit(&words[6..]) {
                system = Some(s);
            }
        }
        Token::Doctype {
            name,
            public,
            system,
        }
    }

    fn start_tag(&mut self, lt: usize) -> Token {
        let mut j = lt + 1;
        while j < self.b.len()
            && !matches!(self.b[j], b'>' | b'/' | b' ' | b'\t' | b'\n' | b'\r' | 0x0C)
        {
            j += 1;
        }
        let name: String = self.s[lt + 1..j]
            .chars()
            .filter(|&c| c != '\0')
            .collect::<String>()
            .to_ascii_lowercase();
        self.pos = j;
        let mut attrs: Vec<(String, Option<String>)> = Vec::new();
        loop {
            self.skip_ws();
            if self.pos >= self.b.len() {
                break;
            }
            match self.b[self.pos] {
                b'>' => {
                    self.pos += 1;
                    break;
                }
                b'/' => {
                    self.pos += 1;
                    continue;
                }
                _ => {}
            }
            // Attribute name (a leading '=' is part of it).
            let ns = self.pos;
            let mut k = ns + 1;
            while k < self.b.len()
                && !matches!(
                    self.b[k],
                    b'>' | b'/' | b'=' | b' ' | b'\t' | b'\n' | b'\r' | 0x0C
                )
            {
                k += 1;
            }
            let aname = self.s[ns..k].to_ascii_lowercase();
            self.pos = k;
            self.skip_ws();
            let mut value = None;
            if self.pos < self.b.len() && self.b[self.pos] == b'=' {
                self.pos += 1;
                self.skip_ws();
                if self.pos < self.b.len() {
                    let q = self.b[self.pos];
                    if q == b'"' || q == b'\'' {
                        let vs = self.pos + 1;
                        let ve = memchr::memchr(q, &self.b[vs..]).map_or(self.b.len(), |o| vs + o);
                        value = Some(Self::decode_refs(&self.s[vs..ve], true));
                        self.pos = (ve + 1).min(self.b.len());
                    } else {
                        let vs = self.pos;
                        let mut ve = vs;
                        while ve < self.b.len()
                            && !matches!(self.b[ve], b'>' | b' ' | b'\t' | b'\n' | b'\r' | 0x0C)
                        {
                            ve += 1;
                        }
                        value = Some(Self::decode_refs(&self.s[vs..ve], true));
                        self.pos = ve;
                    }
                } else {
                    value = Some(String::new());
                }
            }
            if !attrs.iter().any(|(n, _)| *n == aname) {
                attrs.push((aname, value));
            }
        }
        Token::Start {
            name,
            attrs,
            end: self.pos.saturating_sub(1),
        }
    }
}

// ----- tree construction ------------------------------------------------------

struct Builder<'a> {
    arena: &'a mut Arena,
    doc: NodeId,
    flags: u32,
    stack: Vec<NodeId>,
    html: Option<NodeId>,
    head: Option<NodeId>,
    body: Option<NodeId>,
    body_seen: bool,
    frameset: bool,
    errors: Vec<XmlError>,
    url: Option<String>,
    src: &'a str,
    seen_content: bool,
}

impl Builder<'_> {
    fn name_of(&self, n: NodeId) -> &str {
        self.arena.element(n).map_or("", |e| &e.name.local)
    }

    fn current(&self) -> NodeId {
        self.stack.last().copied().unwrap_or(self.doc)
    }

    fn err(&mut self, code: u32, msg: String, at: usize) {
        if self.flags & opt::NOERROR != 0 {
            return;
        }
        let before = &self.src[..at.min(self.src.len())];
        let line = before.bytes().filter(|&b| b == b'\n').count() as u32 + 1;
        let ls = before.rfind('\n').map_or(0, |i| i + 1);
        let column = before[ls..].chars().count() as u32;
        self.errors.push(XmlError {
            domain: Domain::Html,
            code,
            level: LEVEL_ERROR,
            message: format!("{msg}\n"),
            file: self.url.clone(),
            line,
            column: column.max(1),
        });
    }

    fn new_element(&mut self, name: &str, attrs: Vec<(String, Option<String>)>) -> NodeId {
        let el = self.arena.new_element(self.doc, QName::local(name));
        for (an, v) in attrs {
            let a = self.arena.create(
                self.doc,
                NodeData::Attribute(Box::new(AttrData {
                    name: QName::local(&an),
                    is_id: false,
                    defaulted: false,
                })),
            );
            // A boolean attribute without value gets its name as value; other
            // valueless attributes have no value at all (observed on PHP 8.5).
            let v = match v {
                Some(v) => Some(v),
                None if is_boolean_attr(&an) => Some(an.clone()),
                None => None,
            };
            if let Some(v) = v {
                let t = self.arena.new_text(self.doc, v);
                self.arena.raw_append(a, t);
            }
            self.arena.raw_append_attr(el, a);
        }
        el
    }

    fn append(&mut self, n: NodeId) {
        let p = self.current();
        self.arena.raw_append(p, n);
    }

    fn push_element(&mut self, name: &str, attrs: Vec<(String, Option<String>)>) -> NodeId {
        let el = self.new_element(name, attrs);
        self.append(el);
        let void = element_info(name).is_some_and(|i| i.void);
        if !void {
            self.stack.push(el);
        }
        el
    }

    fn noimplied(&self) -> bool {
        self.flags & HTML_NOIMPLIED != 0
    }

    fn ensure_html(&mut self) {
        if self.noimplied() || !self.stack.is_empty() {
            return;
        }
        let h = self.new_element("html", Vec::new());
        self.arena.raw_append(self.doc, h);
        self.html = Some(h);
        self.stack.push(h);
    }

    /// Prepares for body content.
    fn ensure_body(&mut self) {
        self.ensure_html();
        if self.noimplied() {
            return;
        }
        if let Some(h) = self.head
            && self.stack.last() == Some(&h)
        {
            self.stack.pop();
        }
        if self.body.is_none()
            && !self.body_seen
            && !self.frameset
            && self.stack.last() == self.html.as_ref()
        {
            let b = self.push_element("body", Vec::new());
            self.body = Some(b);
            self.body_seen = true;
        }
    }

    fn ensure_head(&mut self) {
        self.ensure_html();
        if self.noimplied() || self.head.is_some() {
            return;
        }
        if self.stack.last() == self.html.as_ref() {
            let h = self.push_element("head", Vec::new());
            self.head = Some(h);
        }
    }

    fn text(&mut self, t: String) {
        let blank = t
            .bytes()
            .all(|b| matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 0x0C));
        if blank {
            let cur = self.current();
            if self.stack.is_empty() || Some(cur) == self.html {
                return;
            }
        } else if self.stack.is_empty() && self.noimplied() {
            // Text outside any element is dropped (observed on PHP 8.5).
            return;
        } else {
            let in_head = self.head.is_some() && self.stack.last() == self.head.as_ref();
            if self.stack.is_empty() || Some(self.current()) == self.html || in_head {
                self.ensure_body();
            }
        }
        if t.is_empty() {
            return;
        }
        let cur = self.current();
        if cur == self.doc {
            return;
        }
        if let Some(last) = self.arena.last_child(cur)
            && let NodeData::Text(s) = &mut self.arena.node_mut(last).data
        {
            s.push_str(&t);
            return;
        }
        let n = self.arena.new_text(self.doc, t);
        self.append(n);
    }

    fn start(
        &mut self,
        name: String,
        attrs: Vec<(String, Option<String>)>,
        end: usize,
    ) -> Option<(String, bool)> {
        self.seen_content = true;
        let info = element_info(&name);
        match name.as_str() {
            "html" => {
                if self.html.is_some() || !self.stack.is_empty() {
                    self.err(800, "htmlParseStartTag: misplaced <html> tag".into(), end);
                    return None;
                }
                let h = self.new_element("html", attrs);
                self.arena.raw_append(self.doc, h);
                self.html = Some(h);
                self.stack.push(h);
                return None;
            }
            "head" => {
                if self.head.is_some() || self.body_seen {
                    self.err(800, "htmlParseStartTag: misplaced <head> tag".into(), end);
                    return None;
                }
                self.ensure_html();
                let h = self.push_element("head", attrs);
                self.head = Some(h);
                return None;
            }
            "body" => {
                if self.body_seen {
                    self.err(800, "htmlParseStartTag: misplaced <body> tag".into(), end);
                    return None;
                }
                self.ensure_html();
                if let Some(h) = self.head
                    && self.stack.contains(&h)
                {
                    while let Some(x) = self.stack.pop() {
                        if x == h {
                            break;
                        }
                    }
                }
                let b = self.push_element("body", attrs);
                self.body = Some(b);
                self.body_seen = true;
                return None;
            }
            "frameset" => {
                self.ensure_html();
                if let Some(h) = self.head
                    && self.stack.last() == Some(&h)
                {
                    self.stack.pop();
                }
                self.frameset = true;
                self.push_element("frameset", attrs);
                return None;
            }
            _ => {}
        }
        let head_el = info.is_some_and(|i| i.head);
        if head_el && !self.body_seen && !self.frameset {
            let cur_is_html_or_head = self.stack.is_empty()
                || self.stack.last() == self.html.as_ref()
                || self.stack.last() == self.head.as_ref();
            if cur_is_html_or_head {
                self.ensure_head();
            }
        } else if !self.frameset || self.stack.is_empty() {
            let at_top = self.stack.is_empty()
                || self.stack.last() == self.html.as_ref()
                || (self.head.is_some() && self.stack.last() == self.head.as_ref());
            if at_top {
                self.ensure_body();
            }
        }
        // Omitted end tags.
        while let Some(&cur) = self.stack.last() {
            let cn = self.name_of(cur).to_string();
            if start_closes(&name, &cn) {
                self.stack.pop();
            } else {
                break;
            }
        }
        self.push_element(&name, attrs);
        let el = self.current();
        let _ = el;
        match info {
            Some(i) if i.raw => Some((name, false)),
            Some(i) if i.rcdata => Some((name, true)),
            _ => None,
        }
    }

    fn end(&mut self, name: String, end: usize) {
        if matches!(name.as_str(), "html" | "body" | "head") {
            // Closing the document structure: pop to it when open.
            if let Some(pos) = self.stack.iter().rposition(|&n| self.name_of(n) == name) {
                self.stack.truncate(pos);
                if name == "body" {
                    self.body = None;
                }
                if name == "head" {
                    // Keep `head` known so later head elements go to body.
                }
            }
            return;
        }
        let Some(pos) = self.stack.iter().rposition(|&n| self.name_of(n) == name) else {
            self.err(76, format!("Unexpected end tag : {name}"), end);
            return;
        };
        // Closing intervening elements whose end tags are required is an
        // error (observed on PHP 8.5).
        let above: Vec<NodeId> = self.stack[pos + 1..].to_vec();
        for n in above.iter().rev() {
            let inner = self.name_of(*n).to_string();
            // Unknown elements close silently (observed on PHP 8.5).
            if element_info(&inner).is_some_and(|i| !i.optional_end) {
                self.err(
                    76,
                    format!("Opening and ending tag mismatch: {name} and {inner}"),
                    end,
                );
            }
        }
        self.stack.truncate(pos);
    }
}

/// The result of an HTML parse.
#[derive(Debug)]
pub struct Parsed {
    /// The document (DocKind::Html).
    pub doc: NodeId,
    /// Errors and warnings in order.
    pub errors: Vec<XmlError>,
}

/// Finds a `<meta charset>` or `http-equiv` content charset declaration.
fn meta_charset(input: &[u8]) -> Option<String> {
    let lower: Vec<u8> = input.iter().map(|b| b.to_ascii_lowercase()).collect();
    let mut i = 0;
    while let Some(off) = memchr::memmem::find(&lower[i..], b"<meta") {
        let start = i + off;
        let end = memchr::memchr(b'>', &lower[start..]).map_or(lower.len(), |o| start + o);
        let tag = &lower[start..end];
        if let Some(c) = memchr::memmem::find(tag, b"charset") {
            let mut k = c + 7;
            while k < tag.len() && matches!(tag[k], b' ' | b'\t' | b'\n' | b'\r') {
                k += 1;
            }
            if k < tag.len() && tag[k] == b'=' {
                k += 1;
                while k < tag.len() && matches!(tag[k], b' ' | b'\t' | b'\n' | b'\r' | b'"' | b'\'')
                {
                    k += 1;
                }
                let vs = k;
                while k < tag.len()
                    && !matches!(
                        tag[k],
                        b' ' | b'\t' | b'\n' | b'\r' | b'"' | b'\'' | b';' | b'/'
                    )
                {
                    k += 1;
                }
                if k > vs {
                    let raw = &input[start + vs..start + k];
                    return Some(String::from_utf8_lossy(raw).into_owned());
                }
            }
        }
        i = end;
    }
    None
}

/// `DOMDocument::loadHTML()`: parses `input` into a new HTML document.
/// `flags` are LIBXML_* options (HTML_NOIMPLIED, HTML_NODEFDTD, NOERROR,
/// NOWARNING, ...); `encoding` overrides detection.
pub fn parse_html(
    arena: &mut Arena,
    input: &[u8],
    flags: u32,
    encoding: Option<&str>,
    url: Option<&str>,
) -> Parsed {
    let doc = arena.new_document(DocKind::Html);
    arena.doc_mut(doc).standalone = 1;
    arena.doc_mut(doc).url = url.map(str::to_string);
    // PHP truncates at the first NUL byte? No: NULs are dropped by the
    // tokenizer (observed on PHP 8.5).
    let (sniffed, _) = crate::encoding::sniff(input);
    let declared = encoding.map(str::to_string).or_else(|| meta_charset(input));
    let mut doc_encoding = None;
    let text: String = if let Some(e) = sniffed.filter(|e| *e != goro_charset::Encoding::Utf8) {
        let d = crate::encoding::decode(input, Some(e));
        String::from_utf8(d.text).unwrap_or_default()
    } else if let Some(name) = declared.as_deref().filter(|_| sniffed.is_none())
        && let Some(e) = crate::encoding::lookup(name)
    {
        doc_encoding = Some(name.to_string());
        decode_lossy(input, e)
    } else if sniffed == Some(goro_charset::Encoding::Utf8) {
        doc_encoding = Some("UTF-8".to_string());
        decode_lossy(&input[3..], goro_charset::Encoding::Utf8)
    } else if input.is_ascii() {
        String::from_utf8(input.to_vec()).unwrap()
    } else {
        // No declaration: Windows-1252 (the HTML default; observed on
        // PHP 8.5).
        doc_encoding = Some("windows-1252".to_string());
        decode_lossy(input, goro_charset::Encoding::Cp1252)
    };
    arena.doc_mut(doc).encoding = doc_encoding;
    let mut b = Builder {
        arena,
        doc,
        flags,
        stack: Vec::new(),
        html: None,
        head: None,
        body: None,
        body_seen: false,
        frameset: false,
        errors: Vec::new(),
        url: url.map(str::to_string),
        src: &text,
        seen_content: false,
    };
    let mut tok = Tokenizer {
        s: &text,
        b: text.as_bytes(),
        pos: 0,
    };
    let mut raw: Option<(String, bool)> = None;
    let mut doctype: Option<NodeId> = None;
    while let Some(t) = tok.next(raw.as_ref().map(|(n, r)| (n.as_str(), *r))) {
        match t {
            Token::Text(s) => {
                if let Some((name, rcdata)) = &raw {
                    if !s.is_empty() {
                        let cur = b.current();
                        if *rcdata || name == "textarea" || name == "title" {
                            let n = b.arena.new_text(doc, s);
                            b.arena.raw_append(cur, n);
                        } else {
                            let n = b.arena.create(doc, NodeData::CData(s));
                            b.arena.raw_append(cur, n);
                        }
                    }
                    let _ = rcdata;
                } else {
                    b.text(s);
                }
            }
            Token::Start { name, attrs, end } => {
                raw = b.start(name, attrs, end);
            }
            Token::End { name, end } => {
                if let Some((rn, _)) = &raw
                    && *rn == name
                {
                    raw = None;
                }
                b.end(name, end);
            }
            Token::Comment(c) => {
                let n = b.arena.create(doc, NodeData::Comment(c));
                let cur = b.current();
                b.arena.raw_append(cur, n);
            }
            Token::Doctype {
                name,
                public,
                system,
            } => {
                if doctype.is_none() && !b.seen_content {
                    let d = b.arena.create(
                        doc,
                        NodeData::Doctype(Box::new(DtdData {
                            name: Str::from(name.as_str()),
                            public_id: public,
                            system_id: system,
                            ..Default::default()
                        })),
                    );
                    b.arena.raw_append(doc, d);
                    doctype = Some(d);
                }
            }
        }
    }
    let errors = std::mem::take(&mut b.errors);
    drop(b);
    if doctype.is_none() && flags & HTML_NODEFDTD == 0 {
        let d = arena.create(
            doc,
            NodeData::Doctype(Box::new(DtdData {
                name: "html".into(),
                public_id: Some("-//W3C//DTD HTML 4.0 Transitional//EN".into()),
                system_id: Some("http://www.w3.org/TR/REC-html40/loose.dtd".into()),
                ..Default::default()
            })),
        );
        let first = arena.first_child(doc);
        arena.raw_insert_before(doc, d, first);
        doctype = Some(d);
    }
    arena.doc_mut(doc).doctype = doctype;
    Parsed { doc, errors }
}

fn decode_lossy(input: &[u8], e: goro_charset::Encoding) -> String {
    let cps = goro_charset::decode(input, e);
    cps.into_iter()
        .map(|c| char::from_u32(c).unwrap_or('\u{FFFD}'))
        .collect()
}

// ----- serializer ---------------------------------------------------------------

/// `saveHTML()` settings.
#[derive(Clone, Copy, Debug, Default)]
pub struct HtmlSaveOptions {
    /// `formatOutput`.
    pub format: bool,
}

struct HtmlWriter<'a> {
    arena: &'a Arena,
    out: String,
    format: bool,
    ascii: bool,
}

fn uri_escape(s: &str, out: &mut String) {
    for b in s.bytes() {
        let keep = b.is_ascii_alphanumeric() || b"-_.!~*'();/?:@&=+$,#%[]".contains(&b);
        if keep {
            if b == b'&' {
                out.push_str("&amp;");
            } else {
                out.push(b as char);
            }
        } else if b == b'"' {
            out.push_str("%22");
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
}

impl HtmlWriter<'_> {
    fn text_escape(&mut self, s: &str) {
        for c in s.chars() {
            match c {
                '<' => self.out.push_str("&lt;"),
                '>' => self.out.push_str("&gt;"),
                '&' => self.out.push_str("&amp;"),
                c if self.ascii && !c.is_ascii() => match entity_name(c) {
                    Some(n) => {
                        self.out.push('&');
                        self.out.push_str(n);
                        self.out.push(';');
                    }
                    None => self.out.push_str(&format!("&#{};", c as u32)),
                },
                c => self.out.push(c),
            }
        }
    }

    fn attr_escape(&mut self, s: &str) {
        for c in s.chars() {
            match c {
                '<' => self.out.push_str("&lt;"),
                '>' => self.out.push_str("&gt;"),
                '&' => self.out.push_str("&amp;"),
                '"' => self.out.push_str("&quot;"),
                c if self.ascii && !c.is_ascii() => match entity_name(c) {
                    Some(n) => {
                        self.out.push('&');
                        self.out.push_str(n);
                        self.out.push(';');
                    }
                    None => self.out.push_str(&format!("&#{};", c as u32)),
                },
                c => self.out.push(c),
            }
        }
    }

    fn is_texty(&self, n: NodeId) -> bool {
        matches!(
            self.arena.data(n),
            NodeData::Text(_) | NodeData::EntityRef(_)
        )
    }

    fn node(&mut self, n: NodeId) {
        let arena = self.arena;
        match arena.data(n) {
            NodeData::Document(_) | NodeData::Fragment => {
                for c in arena.children(n) {
                    self.node(c);
                }
            }
            NodeData::Doctype(d) => {
                self.out.push_str("<!DOCTYPE ");
                self.out.push_str(&d.name);
                match (&d.public_id, &d.system_id) {
                    (Some(p), Some(s)) => self.out.push_str(&format!(" PUBLIC \"{p}\" \"{s}\"")),
                    (Some(p), None) => self.out.push_str(&format!(" PUBLIC \"{p}\"")),
                    (None, Some(s)) => self.out.push_str(&format!(" SYSTEM \"{s}\"")),
                    (None, None) => {}
                }
                self.out.push('>');
            }
            NodeData::Element(e) => {
                let name = e.name.qualified();
                let lname = name.to_ascii_lowercase();
                let info = element_info(&lname);
                self.out.push('<');
                self.out.push_str(&name);
                for d in &e.ns_decls {
                    match &d.prefix {
                        Some(p) => self.out.push_str(&format!(" xmlns:{p}=\"")),
                        None => self.out.push_str(" xmlns=\""),
                    }
                    self.attr_escape(&d.uri);
                    self.out.push('"');
                }
                for a in arena.attrs(n) {
                    let an = arena.attr(a).unwrap().name.qualified();
                    self.out.push(' ');
                    self.out.push_str(&an);
                    if arena.first_child(a).is_none() || is_boolean_attr(&an.to_ascii_lowercase()) {
                        continue;
                    }
                    self.out.push_str("=\"");
                    let v = arena.attr_value(a);
                    if is_uri_attr(&lname, &an.to_ascii_lowercase()) {
                        let mut s = String::new();
                        uri_escape(&v, &mut s);
                        self.out.push_str(&s);
                    } else {
                        self.attr_escape(&v);
                    }
                    self.out.push('"');
                }
                if info.is_some_and(|i| i.void) && arena.first_child(n).is_none() {
                    self.out.push('>');
                    return;
                }
                self.out.push('>');
                let first = arena.first_child(n);
                let last = arena.last_child(n);
                let blocky = info.is_some_and(|i| !i.inline) && !lname.starts_with('p');
                if self.format
                    && blocky
                    && let (Some(f), Some(l)) = (first, last)
                    && f != l
                    && !self.is_texty(f)
                {
                    self.out.push('\n');
                }
                let raw = info.is_some_and(|i| i.raw);
                let mut c = first;
                while let Some(ch) = c {
                    if raw && let Some(t) = arena.char_data(ch) {
                        self.out.push_str(t);
                    } else {
                        self.node(ch);
                    }
                    let next = arena.next_sibling(ch);
                    if self.format
                        && let Some(nx) = next
                        && !self.is_texty(nx)
                        && !lname.starts_with('p')
                        && let Some(ce) = arena.element(ch)
                        && element_info(&ce.name.qualified().to_ascii_lowercase())
                            .is_some_and(|i| !i.inline)
                    {
                        self.out.push('\n');
                    }
                    c = next;
                }
                if self.format
                    && blocky
                    && let (Some(f), Some(l)) = (first, last)
                    && f != l
                    && !self.is_texty(l)
                {
                    self.out.push('\n');
                }
                self.out.push_str("</");
                self.out.push_str(&name);
                self.out.push('>');
            }
            NodeData::Text(t) => {
                let t = t.clone();
                self.text_escape(&t);
            }
            NodeData::CData(t) => self.out.push_str(t),
            NodeData::Comment(t) => {
                self.out.push_str("<!--");
                self.out.push_str(t);
                self.out.push_str("-->");
            }
            NodeData::Pi { target, data } => {
                self.out.push_str("<?");
                self.out.push_str(target);
                if !data.is_empty() {
                    self.out.push(' ');
                    self.out.push_str(data);
                }
                self.out.push('>');
            }
            NodeData::EntityRef(name) => {
                self.out.push('&');
                self.out.push_str(name);
                self.out.push(';');
            }
            NodeData::Attribute(_) => {
                let an = arena.attr(n).unwrap().name.qualified();
                self.out.push(' ');
                self.out.push_str(&an);
                self.out.push_str("=\"");
                let v = arena.attr_value(n);
                self.attr_escape(&v);
                self.out.push('"');
            }
            _ => {}
        }
    }
}

/// `saveHTML()` of a whole document or of one node: the bytes PHP returns.
pub fn save_html(arena: &Arena, n: NodeId, opts: HtmlSaveOptions) -> Vec<u8> {
    let doc = arena.doc_of(n);
    let enc = arena.doc(doc).encoding.clone();
    let mut w = HtmlWriter {
        arena,
        out: String::new(),
        format: opts.format,
        ascii: enc.is_none(),
    };
    if matches!(arena.data(n), NodeData::Document(_)) {
        for c in arena.children(n) {
            w.node(c);
            if matches!(arena.data(c), NodeData::Doctype(_)) {
                w.out.push('\n');
            }
        }
        w.out.push('\n');
    } else {
        w.node(n);
    }
    match enc {
        None => w.out.into_bytes(),
        Some(e) => crate::serialize::encode_output(&w.out, Some(&e)),
    }
}
