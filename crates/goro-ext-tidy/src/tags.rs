//! The HTML elements Tidy knows, with their content model.
//!
//! The numeric ids are the `TIDY_TAG_*` constants: the PHP manual lists those
//! constants in alphabetical order, and the ids the tests show (test
//! 010.phpt: b 8, body 16, head 46, html 48, i 49) are that order's positions
//! counted from 1. The HTML5 additions follow the older elements; their
//! numbers aren't shown by any test.

use std::collections::HashMap;
use std::sync::OnceLock;

/// Phrasing content: printed on the line it occurs in.
pub const INLINE: u32 = 1 << 0;
/// Flow content: printed on its own lines.
pub const BLOCK: u32 = 1 << 1;
/// A void element.
pub const EMPTY: u32 = 1 << 2;
/// Belongs in the head.
pub const HEAD: u32 = 1 << 3;
/// `ul`, `ol`, `dir`, `menu`.
pub const LIST: u32 = 1 << 4;
/// Its start and end tags go on lines of their own whatever it holds.
pub const STRUCT: u32 = 1 << 5;
/// An inline element that may hold block content.
pub const MIXED: u32 = 1 << 6;
/// Its text content isn't indented on a line of its own.
pub const NO_INDENT: u32 = 1 << 7;
/// The end tag may be left out without a warning.
pub const OPT_END: u32 = 1 << 8;
/// Not part of any HTML version.
pub const PROPRIETARY: u32 = 1 << 9;
/// Removed from HTML5.
pub const OBSOLETE: u32 = 1 << 10;
/// Content is raw text up to the end tag.
pub const RAW: u32 = 1 << 11;
/// Content is text with entities up to the end tag.
pub const RCDATA: u32 = 1 << 12;
/// Whitespace is significant.
pub const PRE: u32 = 1 << 13;
/// `h1` to `h6`.
pub const HEADING: u32 = 1 << 14;
/// Holds phrasing content only (a block start tag ends it).
pub const PHRASING_ONLY: u32 = 1 << 15;
/// May only appear in a table.
pub const TABLE_PART: u32 = 1 << 16;
/// `dl`.
pub const DEFLIST: u32 = 1 << 17;

/// One element.
#[derive(Clone, Copy, Debug)]
pub struct Tag {
    pub name: &'static str,
    pub constant: &'static str,
    pub id: i64,
    pub flags: u32,
}

macro_rules! t {
    ($n:literal, $c:literal, $i:literal, $f:expr) => {
        Tag {
            name: $n,
            constant: concat!("TIDY_TAG_", $c),
            id: $i,
            flags: $f,
        }
    };
}

pub const TAG_COUNT: usize = 147;

#[rustfmt::skip]
pub const TAGS: [Tag; TAG_COUNT] = [
    t!("a", "A", 1, INLINE | MIXED),
    t!("abbr", "ABBR", 2, INLINE),
    t!("acronym", "ACRONYM", 3, INLINE | OBSOLETE),
    t!("address", "ADDRESS", 4, BLOCK | NO_INDENT),
    t!("align", "ALIGN", 5, BLOCK | PROPRIETARY),
    t!("applet", "APPLET", 6, INLINE | MIXED | OBSOLETE),
    t!("area", "AREA", 7, BLOCK | EMPTY),
    t!("b", "B", 8, INLINE),
    t!("base", "BASE", 9, HEAD | EMPTY),
    t!("basefont", "BASEFONT", 10, INLINE | EMPTY | OBSOLETE),
    t!("bdo", "BDO", 11, INLINE),
    t!("bgsound", "BGSOUND", 12, HEAD | EMPTY | PROPRIETARY),
    t!("big", "BIG", 13, INLINE | OBSOLETE),
    t!("blink", "BLINK", 14, INLINE | PROPRIETARY),
    t!("blockquote", "BLOCKQUOTE", 15, BLOCK),
    t!("body", "BODY", 16, STRUCT | OPT_END),
    t!("br", "BR", 17, INLINE | EMPTY),
    t!("button", "BUTTON", 18, INLINE | MIXED),
    t!("caption", "CAPTION", 19, BLOCK | NO_INDENT | TABLE_PART),
    t!("center", "CENTER", 20, BLOCK | OBSOLETE),
    t!("cite", "CITE", 21, INLINE),
    t!("code", "CODE", 22, INLINE),
    t!("col", "COL", 23, BLOCK | EMPTY | TABLE_PART),
    t!("colgroup", "COLGROUP", 24, BLOCK | STRUCT | OPT_END | TABLE_PART),
    t!("comment", "COMMENT", 25, INLINE | PROPRIETARY),
    t!("dd", "DD", 26, BLOCK | NO_INDENT | OPT_END),
    t!("del", "DEL", 27, INLINE | MIXED),
    t!("dfn", "DFN", 28, INLINE),
    t!("dir", "DIR", 29, BLOCK | LIST | STRUCT | OBSOLETE),
    t!("div", "DIV", 30, BLOCK),
    t!("dl", "DL", 31, BLOCK | DEFLIST | STRUCT),
    t!("dt", "DT", 32, BLOCK | NO_INDENT | OPT_END),
    t!("em", "EM", 33, INLINE),
    t!("embed", "EMBED", 34, INLINE | EMPTY),
    t!("fieldset", "FIELDSET", 35, BLOCK),
    t!("font", "FONT", 36, INLINE | OBSOLETE),
    t!("form", "FORM", 37, BLOCK),
    t!("frame", "FRAME", 38, BLOCK | EMPTY | OBSOLETE),
    t!("frameset", "FRAMESET", 39, BLOCK | STRUCT | OBSOLETE),
    t!("h1", "H1", 40, BLOCK | NO_INDENT | HEADING | PHRASING_ONLY),
    t!("h2", "H2", 41, BLOCK | NO_INDENT | HEADING | PHRASING_ONLY),
    t!("h3", "H3", 42, BLOCK | NO_INDENT | HEADING | PHRASING_ONLY),
    t!("h4", "H4", 43, BLOCK | NO_INDENT | HEADING | PHRASING_ONLY),
    t!("h5", "H5", 44, BLOCK | NO_INDENT | HEADING | PHRASING_ONLY),
    t!("h6", "H6", 45, BLOCK | NO_INDENT | HEADING | PHRASING_ONLY),
    t!("head", "HEAD", 46, STRUCT | OPT_END),
    t!("hr", "HR", 47, BLOCK | EMPTY),
    t!("html", "HTML", 48, STRUCT | OPT_END),
    t!("i", "I", 49, INLINE),
    t!("iframe", "IFRAME", 50, INLINE | MIXED),
    t!("ilayer", "ILAYER", 51, INLINE | PROPRIETARY),
    t!("img", "IMG", 52, INLINE | EMPTY),
    t!("input", "INPUT", 53, INLINE | EMPTY),
    t!("ins", "INS", 54, INLINE | MIXED),
    t!("isindex", "ISINDEX", 55, BLOCK | EMPTY | OBSOLETE),
    t!("kbd", "KBD", 56, INLINE),
    t!("keygen", "KEYGEN", 57, INLINE | EMPTY),
    t!("label", "LABEL", 58, INLINE),
    t!("layer", "LAYER", 59, BLOCK | PROPRIETARY),
    t!("legend", "LEGEND", 60, BLOCK | NO_INDENT),
    t!("li", "LI", 61, BLOCK | NO_INDENT | OPT_END),
    t!("link", "LINK", 62, HEAD | EMPTY),
    t!("listing", "LISTING", 63, BLOCK | PRE | RAW | OBSOLETE),
    t!("map", "MAP", 64, INLINE | MIXED),
    t!("marquee", "MARQUEE", 65, INLINE | MIXED | PROPRIETARY),
    t!("menu", "MENU", 66, BLOCK | LIST | STRUCT),
    t!("meta", "META", 67, HEAD | EMPTY),
    t!("multicol", "MULTICOL", 68, BLOCK | PROPRIETARY),
    t!("nobr", "NOBR", 69, INLINE | PROPRIETARY),
    t!("noembed", "NOEMBED", 70, INLINE | PROPRIETARY),
    t!("noframes", "NOFRAMES", 71, BLOCK | OBSOLETE),
    t!("nolayer", "NOLAYER", 72, BLOCK | PROPRIETARY),
    t!("nosave", "NOSAVE", 73, BLOCK | PROPRIETARY),
    t!("noscript", "NOSCRIPT", 74, BLOCK),
    t!("object", "OBJECT", 75, INLINE | MIXED),
    t!("ol", "OL", 76, BLOCK | LIST | STRUCT),
    t!("optgroup", "OPTGROUP", 77, BLOCK | STRUCT),
    t!("option", "OPTION", 78, BLOCK | NO_INDENT | OPT_END),
    t!("p", "P", 79, BLOCK | NO_INDENT | OPT_END | PHRASING_ONLY),
    t!("param", "PARAM", 80, BLOCK | EMPTY),
    t!("plaintext", "PLAINTEXT", 81, BLOCK | PRE | RAW | OBSOLETE),
    t!("pre", "PRE", 82, BLOCK | PRE | NO_INDENT),
    t!("q", "Q", 83, INLINE),
    t!("rb", "RB", 84, INLINE),
    t!("rbc", "RBC", 85, INLINE),
    t!("rp", "RP", 86, INLINE),
    t!("rt", "RT", 87, INLINE),
    t!("rtc", "RTC", 88, INLINE),
    t!("ruby", "RUBY", 89, INLINE),
    t!("s", "S", 90, INLINE),
    t!("samp", "SAMP", 91, INLINE),
    t!("script", "SCRIPT", 92, INLINE | HEAD | RAW),
    t!("select", "SELECT", 93, INLINE | STRUCT),
    t!("server", "SERVER", 94, INLINE | RAW | PROPRIETARY),
    t!("servlet", "SERVLET", 95, INLINE | PROPRIETARY),
    t!("small", "SMALL", 96, INLINE),
    t!("spacer", "SPACER", 97, INLINE | EMPTY | PROPRIETARY),
    t!("span", "SPAN", 98, INLINE),
    t!("strike", "STRIKE", 99, INLINE | OBSOLETE),
    t!("strong", "STRONG", 100, INLINE),
    t!("style", "STYLE", 101, HEAD | RAW),
    t!("sub", "SUB", 102, INLINE),
    t!("sup", "SUP", 103, INLINE),
    t!("table", "TABLE", 104, BLOCK | STRUCT),
    t!("tbody", "TBODY", 105, BLOCK | STRUCT | OPT_END | TABLE_PART),
    t!("td", "TD", 106, BLOCK | NO_INDENT | OPT_END | TABLE_PART),
    t!("textarea", "TEXTAREA", 107, INLINE | RCDATA | PRE),
    t!("tfoot", "TFOOT", 108, BLOCK | STRUCT | OPT_END | TABLE_PART),
    t!("th", "TH", 109, BLOCK | NO_INDENT | OPT_END | TABLE_PART),
    t!("thead", "THEAD", 110, BLOCK | STRUCT | OPT_END | TABLE_PART),
    t!("title", "TITLE", 111, HEAD | RCDATA),
    t!("tr", "TR", 112, BLOCK | STRUCT | OPT_END | TABLE_PART),
    t!("tt", "TT", 113, INLINE | OBSOLETE),
    t!("u", "U", 114, INLINE),
    t!("ul", "UL", 115, BLOCK | LIST | STRUCT),
    t!("var", "VAR", 116, INLINE),
    t!("wbr", "WBR", 117, INLINE | EMPTY),
    t!("xmp", "XMP", 118, BLOCK | PRE | RAW | OBSOLETE),
    t!("nextid", "NEXTID", 119, HEAD | EMPTY | OBSOLETE),
    t!("article", "ARTICLE", 120, BLOCK),
    t!("aside", "ASIDE", 121, BLOCK),
    t!("audio", "AUDIO", 122, INLINE | MIXED),
    t!("bdi", "BDI", 123, INLINE),
    t!("canvas", "CANVAS", 124, INLINE | MIXED),
    t!("command", "COMMAND", 125, HEAD | EMPTY),
    t!("datalist", "DATALIST", 126, INLINE | STRUCT),
    t!("details", "DETAILS", 127, BLOCK),
    t!("dialog", "DIALOG", 128, BLOCK),
    t!("figcaption", "FIGCAPTION", 129, BLOCK),
    t!("figure", "FIGURE", 130, BLOCK),
    t!("footer", "FOOTER", 131, BLOCK),
    t!("header", "HEADER", 132, BLOCK),
    t!("hgroup", "HGROUP", 133, BLOCK),
    t!("main", "MAIN", 134, BLOCK),
    t!("mark", "MARK", 135, INLINE),
    t!("menuitem", "MENUITEM", 136, BLOCK | EMPTY),
    t!("meter", "METER", 137, INLINE),
    t!("nav", "NAV", 138, BLOCK),
    t!("output", "OUTPUT", 139, INLINE),
    t!("progress", "PROGRESS", 140, INLINE),
    t!("section", "SECTION", 141, BLOCK),
    t!("source", "SOURCE", 142, BLOCK | EMPTY),
    t!("summary", "SUMMARY", 143, BLOCK),
    t!("template", "TEMPLATE", 144, BLOCK),
    t!("time", "TIME", 145, INLINE),
    t!("track", "TRACK", 146, BLOCK | EMPTY),
    t!("video", "VIDEO", 147, INLINE | MIXED),
];

/// The built-in element of that lower-case name.
pub fn lookup(name: &str) -> Option<&'static Tag> {
    static MAP: OnceLock<HashMap<&'static str, &'static Tag>> = OnceLock::new();
    static TABLE: [Tag; TAG_COUNT] = TAGS;
    MAP.get_or_init(|| TABLE.iter().map(|t| (t.name, t)).collect())
        .get(name)
        .copied()
}

/// A body-level element whose start tag belongs in the head.
pub fn moves_to_head(name: &str) -> bool {
    matches!(name, "title" | "meta" | "link" | "base" | "style")
}

/// Attributes whose presence alone is their value (HTML 4.01 and the HTML
/// Standard's boolean attributes).
pub fn is_boolean_attr(name: &str) -> bool {
    matches!(
        name,
        "allowfullscreen"
            | "async"
            | "autofocus"
            | "autoplay"
            | "checked"
            | "compact"
            | "controls"
            | "declare"
            | "default"
            | "defer"
            | "disabled"
            | "formnovalidate"
            | "hidden"
            | "ismap"
            | "loop"
            | "multiple"
            | "muted"
            | "nohref"
            | "nomodule"
            | "noresize"
            | "noshade"
            | "novalidate"
            | "nowrap"
            | "open"
            | "readonly"
            | "required"
            | "reversed"
            | "selected"
    )
}
