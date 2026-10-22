//! message.c + messageobj.c + language.c: the report/dialogue messages,
//! their formatting ("line N column M - Warning: ...") and counting.

use crate::doc::*;
use crate::enums::code::*;
use crate::enums::{CODE_KEYS, OptId};
use crate::language_en::LANGUAGE_EN;
use crate::lexer;
use crate::streamio;
use crate::tags;

/// ReleaseDate()
pub fn release_date() -> &'static str {
    "2021/07/10"
}

/// tidyLibraryVersion()
pub fn library_version() -> &'static str {
    "5.8.0"
}

// ---------------------------------------------------------------------------
// language strings

/// tidyLocalizedStringImpl() over language_en (the only language)
fn localized_impl(key: u32, plural_form: u32) -> Option<&'static [u8]> {
    LANGUAGE_EN
        .iter()
        .find(|e| e.0 == key && e.1 == plural_form)
        .map(|e| e.2)
}

/// tidyLocalizedStringN(): whichPluralForm_en(n) is `n != 1`
pub fn localized_string_n(key: u32, quantity: u32) -> Option<&'static [u8]> {
    localized_impl(key, (quantity != 1) as u32).or_else(|| localized_impl(key, 0))
}

/// tidyLocalizedString()
pub fn localized_string(key: u32) -> Option<&'static [u8]> {
    localized_string_n(key, 1)
}

/// tidyDefaultString()
pub fn default_string(key: u32) -> Option<&'static [u8]> {
    localized_impl(key, 0)
}

/// tidyOptGetDoc(): the (English) documentation of an option
pub fn opt_get_doc(opt: OptId) -> Option<&'static str> {
    localized_string(opt as u32).and_then(|s| std::str::from_utf8(s).ok())
}

// ---------------------------------------------------------------------------
// printf

/// An argument of the printf-style formatter.
#[derive(Clone, Debug)]
enum FArg {
    S(Option<Vec<u8>>),
    I(i64),
    U(u32),
}

fn s(v: &[u8]) -> FArg {
    FArg::S(Some(v.to_vec()))
}

fn so(v: Option<&[u8]>) -> FArg {
    FArg::S(v.map(|x| x.to_vec()))
}

/// vsnprintf() with the conversions tidy's strings use (%s %d %u %X %c,
/// flags, width, precision), truncated to `max - 2` bytes (tmbvsnprintf() passes `count - 1` to vsnprintf()).
fn format(fmt: &[u8], args: &[FArg], max: usize) -> Vec<u8> {
    let mut out = Vec::new();
    let mut ai = 0;
    let mut i = 0;
    while i < fmt.len() {
        let c = fmt[i];
        if c != b'%' {
            out.push(c);
            i += 1;
            continue;
        }
        i += 1;
        if i < fmt.len() && fmt[i] == b'%' {
            out.push(b'%');
            i += 1;
            continue;
        }
        let mut zero = false;
        let mut left = false;
        while i < fmt.len() && matches!(fmt[i], b'0' | b'-' | b'+' | b' ' | b'#') {
            if fmt[i] == b'0' {
                zero = true;
            }
            if fmt[i] == b'-' {
                left = true;
            }
            i += 1;
        }
        let mut width = 0usize;
        while i < fmt.len() && fmt[i].is_ascii_digit() {
            width = width * 10 + (fmt[i] - b'0') as usize;
            i += 1;
        }
        let mut prec: Option<usize> = None;
        if i < fmt.len() && fmt[i] == b'.' {
            i += 1;
            let mut p = 0usize;
            while i < fmt.len() && fmt[i].is_ascii_digit() {
                p = p * 10 + (fmt[i] - b'0') as usize;
                i += 1;
            }
            prec = Some(p);
        }
        while i < fmt.len() && matches!(fmt[i], b'l' | b'h' | b'z') {
            i += 1;
        }
        if i >= fmt.len() {
            break;
        }
        let conv = fmt[i];
        i += 1;
        let arg = args.get(ai).cloned();
        ai += 1;
        let mut body: Vec<u8> = match conv {
            b's' => {
                let v = match arg {
                    Some(FArg::S(Some(v))) => v,
                    Some(FArg::S(None)) => b"(null)".to_vec(),
                    Some(FArg::I(n)) => n.to_string().into_bytes(),
                    Some(FArg::U(n)) => n.to_string().into_bytes(),
                    None => Vec::new(),
                };
                let v = crate::tmbstr::cstr(&v).to_vec();
                match prec {
                    Some(p) if p < v.len() => v[..p].to_vec(),
                    _ => v,
                }
            }
            b'd' | b'i' => {
                let n = match arg {
                    Some(FArg::I(n)) => n as i32 as i64,
                    Some(FArg::U(n)) => n as i32 as i64,
                    _ => 0,
                };
                n.to_string().into_bytes()
            }
            b'u' => {
                let n = match arg {
                    Some(FArg::I(n)) => n as u32,
                    Some(FArg::U(n)) => n,
                    _ => 0,
                };
                n.to_string().into_bytes()
            }
            b'X' | b'x' => {
                let n = match arg {
                    Some(FArg::I(n)) => n as u32,
                    Some(FArg::U(n)) => n,
                    _ => 0,
                };
                if conv == b'X' {
                    format!("{n:X}").into_bytes()
                } else {
                    format!("{n:x}").into_bytes()
                }
            }
            b'c' => {
                let n = match arg {
                    Some(FArg::I(n)) => n as u8,
                    Some(FArg::U(n)) => n as u8,
                    _ => 0,
                };
                vec![n]
            }
            _ => Vec::new(),
        };
        if body.len() < width {
            let pad = width - body.len();
            if left {
                body.extend(std::iter::repeat_n(b' ', pad));
            } else if zero && conv != b's' && conv != b'c' {
                let mut v = vec![b'0'; pad];
                v.extend_from_slice(&body);
                body = v;
            } else {
                let mut v = vec![b' '; pad];
                v.extend_from_slice(&body);
                body = v;
            }
        }
        out.extend_from_slice(&body);
    }
    if out.len() > max.saturating_sub(2) {
        out.truncate(max.saturating_sub(2));
    }
    out
}

const SIZE_MESSAGE_BUF: usize = 2048;

// ---------------------------------------------------------------------------
// the message object

/// The part of TidyMessageImpl that is used for output.
struct Message {
    code: u32,
    level: u32,
    muted: bool,
    output: Vec<u8>,
}

/// tidyMessageCreateInitV()
fn message_create_init(
    doc: &mut Doc,
    code: u32,
    line: i32,
    column: i32,
    level: u32,
    args: &[FArg],
) -> Message {
    let muted = doc.muted.contains(&code);
    let fmt = localized_string(code).unwrap_or_default();
    let message = format(fmt, args, SIZE_MESSAGE_BUF);

    let emacs_file = doc.cfg_str(OptId::EmacsFile).map(|v| v.to_vec());
    let message_pos = if doc.cfg_bool(OptId::Emacs) && emacs_file.is_some() {
        format(
            b"%s:%d:%d: ",
            &[
                so(emacs_file.as_deref()),
                FArg::I(line as i64),
                FArg::I(column as i64),
            ],
            SIZE_MESSAGE_BUF,
        )
    } else if doc.cfg_bool(OptId::ShowFilename) && emacs_file.is_some() {
        format(
            localized_string(FN_LINE_COLUMN_STRING).unwrap_or_default(),
            &[
                so(emacs_file.as_deref()),
                FArg::I(line as i64),
                FArg::I(column as i64),
            ],
            SIZE_MESSAGE_BUF,
        )
    } else {
        format(
            localized_string(LINE_COLUMN_STRING).unwrap_or_default(),
            &[FArg::I(line as i64), FArg::I(column as i64)],
            SIZE_MESSAGE_BUF,
        )
    };
    let message_prefix = localized_string(level).map(|v| v.to_vec());

    let mut pattern: &[u8] = if line > 0 && column > 0 {
        b"%s%s%s"
    } else {
        b"%.0s%s%s"
    };
    if level > TIDY_FATAL {
        pattern = b"%.0s%.0s%s";
    }
    let mut output = format(
        pattern,
        &[
            FArg::S(Some(message_pos)),
            FArg::S(message_prefix),
            FArg::S(Some(message)),
        ],
        SIZE_MESSAGE_BUF,
    );
    if doc.cfg_bool(OptId::MuteShow) && level <= TIDY_FATAL {
        let pc = error_code_as_key(code);
        let i = output.len();
        if i < SIZE_MESSAGE_BUF {
            let extra = format(b" (%s)", &[s(pc.as_bytes())], SIZE_MESSAGE_BUF - i);
            output.extend_from_slice(&extra);
        }
    }
    Message {
        code,
        level,
        muted,
        output,
    }
}

/// tidyMessageCreate(): no position
fn message_create(doc: &mut Doc, code: u32, level: u32, args: &[FArg]) -> Message {
    message_create_init(doc, code, 0, 0, level, args)
}

/// tidyMessageCreateWithNode()
fn message_create_with_node(
    doc: &mut Doc,
    node: Option<NodeId>,
    code: u32,
    level: u32,
    args: &[FArg],
) -> Message {
    let (line, col) = match node {
        Some(n) => (doc[n].line as i32, doc[n].column as i32),
        None => match doc.lexer.as_ref() {
            Some(l) => (l.lines as i32, l.columns as i32),
            None => (0, 0),
        },
    };
    message_create_init(doc, code, line, col, level, args)
}

/// tidyMessageCreateWithLexer()
fn message_create_with_lexer(doc: &mut Doc, code: u32, level: u32, args: &[FArg]) -> Message {
    let (line, col) = match doc.lexer.as_ref() {
        Some(l) => (l.lines as i32, l.columns as i32),
        None => (0, 0),
    };
    message_create_init(doc, code, line, col, level, args)
}

/// messageOut()
fn message_out(doc: &mut Doc, message: Option<Message>) {
    let Some(message) = message else {
        return;
    };
    let mut go = true;
    match message.level {
        TIDY_INFO => doc.info_messages += 1,
        TIDY_WARNING => doc.warnings += 1,
        TIDY_CONFIG => doc.option_errors += 1,
        TIDY_ACCESS => doc.access_errors += 1,
        TIDY_ERROR => doc.errors += 1,
        TIDY_BAD_DOCUMENT => doc.doc_errors += 1,
        _ => {}
    }
    go &= !message.muted;
    if message.level <= TIDY_FATAL {
        go &= doc.errors < doc.cfg(OptId::ShowErrors);
    }
    if doc.cfg_bool(OptId::Quiet) {
        go = go && message.code != STRING_DOCTYPE_GIVEN;
        go = go && message.code != STRING_CONTENT_LOOKS;
        go = go && message.code != STRING_NO_SYSID;
        go = go && message.level != TIDY_DIALOGUE_INFO;
        go = go && message.level != TIDY_CONFIG;
        go = go && message.level != TIDY_INFO;
        go = go
            && !(message.level >= TIDY_DIALOGUE_SUMMARY
                && message.code != STRING_NEEDS_INTERVENTION);
    }
    if !doc.cfg_bool(OptId::ShowInfo) {
        go = go && message.level != TIDY_INFO;
        go = go && message.level != TIDY_DIALOGUE_INFO;
    }
    if !doc.cfg_bool(OptId::ShowWarnings) {
        go = go && message.level != TIDY_WARNING;
    }
    if go {
        for &b in crate::tmbstr::cstr(&message.output) {
            if b == b'\n' {
                streamio::write_char(b as u32, &mut doc.errout);
            } else {
                put_byte(&mut doc.errout, b);
            }
        }
        streamio::write_char('\n' as u32, &mut doc.errout);
    }
}

/// putByte on the error sink (#383 - no encoding)
fn put_byte(out: &mut streamio::StreamOut, b: u8) {
    match &mut out.sink {
        streamio::Sink::Buffer(v) => v.push(b),
        streamio::Sink::Stderr => {
            use std::io::Write;
            let _ = std::io::stderr().write_all(&[b]);
        }
    }
}

// ---------------------------------------------------------------------------
// formatters

/// TagToString()
fn tag_to_string(doc: &Doc, tag: Option<NodeId>) -> Vec<u8> {
    let Some(t) = tag else {
        return Vec::new();
    };
    let el = doc[t].element.clone();
    let elb = || crate::tmbstr::cstr(el.as_deref().unwrap_or(b"(null)")).to_vec();
    if tags::node_is_element(doc, Some(t)) {
        let mut v = b"<".to_vec();
        v.extend(elb());
        v.push(b'>');
        v
    } else if doc[t].type_ == NodeType::EndTag {
        let mut v = b"</".to_vec();
        v.extend(elb());
        v.push(b'>');
        v
    } else if doc[t].type_ == NodeType::DocTypeTag {
        b"<!DOCTYPE>".to_vec()
    } else if doc[t].type_ == NodeType::TextNode {
        localized_string(STRING_PLAIN_TEXT)
            .unwrap_or_default()
            .to_vec()
    } else if doc[t].type_ == NodeType::XmlDecl {
        localized_string(STRING_XML_DECLARATION)
            .unwrap_or_default()
            .to_vec()
    } else if let Some(e) = el {
        crate::tmbstr::cstr(&e).to_vec()
    } else {
        Vec::new()
    }
}

/// tmbsnprintf into a char[count] buffer
fn trunc(mut v: Vec<u8>, count: usize) -> Vec<u8> {
    if v.len() > count - 2 {
        v.truncate(count - 2);
    }
    v
}

/// HTMLVersion() (message.c's)
fn html_version_str(doc: &Doc) -> Vec<u8> {
    let lx = doc.lxr();
    let version = if lx.version_emitted == 0 {
        lx.doctype
    } else {
        lx.version_emitted
    };
    match lexer::html_version_name_from_code(version, false) {
        Some(r) => r.as_bytes().to_vec(),
        None => localized_string(STRING_HTML_PROPRIETARY)
            .unwrap_or_default()
            .to_vec(),
    }
}

fn element_name(doc: &Doc, n: Option<NodeId>) -> FArg {
    FArg::S(n.and_then(|n| doc[n].element.clone()))
}

fn arg_str(args: &[RArg], i: usize) -> Option<Vec<u8>> {
    match args.get(i) {
        Some(RArg::Str(s)) => Some(s.clone()),
        _ => None,
    }
}

fn arg_uint(args: &[RArg], i: usize) -> u32 {
    match args.get(i) {
        Some(RArg::Uint(u)) => *u,
        Some(RArg::Int(u)) => *u as u32,
        Some(RArg::Bool(b)) => *b as u32,
        _ => 0,
    }
}

fn arg_bool(args: &[RArg], i: usize) -> bool {
    match args.get(i) {
        Some(RArg::Bool(b)) => *b,
        Some(RArg::Uint(u)) => *u != 0,
        Some(RArg::Int(u)) => *u != 0,
        _ => false,
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Formatter {
    Access,
    Attribute,
    Encoding,
    Standard,
    StandardDynamic,
}

/// dispatchTable[]: (level, formatter, next)
fn dispatch(code: u32) -> Option<(u32, Formatter, u32)> {
    use Formatter::*;
    let r = match code {
        ADDED_MISSING_CHARSET => (TIDY_INFO, Standard),
        ANCHOR_NOT_UNIQUE | ANCHOR_DUPLICATED => (TIDY_WARNING, Attribute),
        APOS_UNDEFINED => (TIDY_WARNING, Standard),
        ATTR_VALUE_NOT_LCASE => (TIDY_WARNING, Attribute),
        ATTRIBUTE_VALUE_REPLACED => (TIDY_INFO, Attribute),
        ATTRIBUTE_IS_NOT_ALLOWED
        | BACKSLASH_IN_URI
        | BAD_ATTRIBUTE_VALUE_REPLACED
        | BAD_ATTRIBUTE_VALUE => (TIDY_WARNING, Attribute),
        BAD_CDATA_CONTENT | BAD_SUMMARY_HTML5 | BAD_SURROGATE_LEAD | BAD_SURROGATE_PAIR
        | BAD_SURROGATE_TAIL | CANT_BE_NESTED | COERCE_TO_ENDTAG | CONTENT_AFTER_BODY => {
            (TIDY_WARNING, Standard)
        }
        CUSTOM_TAG_DETECTED => (TIDY_INFO, Standard),
        DISCARDING_UNEXPECTED => (0, StandardDynamic),
        DOCTYPE_AFTER_TAGS => (TIDY_WARNING, Standard),
        DUPLICATE_FRAMESET => (TIDY_ERROR, Standard),
        ELEMENT_NOT_EMPTY => (TIDY_WARNING, Standard),
        ELEMENT_VERS_MISMATCH_ERROR => (TIDY_ERROR, Standard),
        ELEMENT_VERS_MISMATCH_WARN => (TIDY_WARNING, Standard),
        ENCODING_MISMATCH => (TIDY_WARNING, Encoding),
        ESCAPED_ILLEGAL_URI => (TIDY_WARNING, Attribute),
        FILE_CANT_OPEN | FILE_CANT_OPEN_CFG | FILE_NOT_FILE => (TIDY_BAD_DOCUMENT, Standard),
        FIXED_BACKSLASH => (TIDY_WARNING, Attribute),
        FOUND_STYLE_IN_BODY => (TIDY_WARNING, Standard),
        ID_NAME_MISMATCH => (TIDY_WARNING, Attribute),
        ILLEGAL_NESTING => (TIDY_WARNING, Standard),
        ILLEGAL_URI_CODEPOINT | ILLEGAL_URI_REFERENCE | INSERTING_AUTO_ATTRIBUTE => {
            (TIDY_WARNING, Attribute)
        }
        INSERTING_TAG => (TIDY_WARNING, Standard),
        INVALID_ATTRIBUTE => (TIDY_WARNING, Attribute),
        INVALID_NCR | INVALID_SGML_CHARS | INVALID_UTF8 | INVALID_UTF16 => (TIDY_WARNING, Encoding),
        INVALID_XML_ID | JOINING_ATTRIBUTE => (TIDY_WARNING, Attribute),
        MALFORMED_COMMENT => (TIDY_INFO, Standard),
        MALFORMED_COMMENT_EOS => (TIDY_ERROR, Standard),
        MALFORMED_COMMENT_DROPPING | MALFORMED_COMMENT_WARN | MALFORMED_DOCTYPE => {
            (TIDY_WARNING, Standard)
        }
        MISMATCHED_ATTRIBUTE_ERROR => (TIDY_ERROR, Attribute),
        MISMATCHED_ATTRIBUTE_WARN | MISSING_ATTR_VALUE => (TIDY_WARNING, Attribute),
        MISSING_ATTRIBUTE | MISSING_DOCTYPE | MISSING_ENDTAG_BEFORE | MISSING_ENDTAG_FOR => {
            (TIDY_WARNING, Standard)
        }
        MISSING_ENDTAG_OPTIONAL => (TIDY_INFO, Standard),
        MISSING_IMAGEMAP | MISSING_QUOTEMARK => (TIDY_WARNING, Attribute),
        MISSING_QUOTEMARK_OPEN => (TIDY_INFO, Attribute),
        MISSING_SEMICOLON_NCR
        | MISSING_SEMICOLON
        | MISSING_STARTTAG
        | MISSING_TITLE_ELEMENT
        | MOVED_STYLE_TO_HEAD
        | NESTED_EMPHASIS
        | NESTED_QUOTATION => (TIDY_WARNING, Standard),
        NEWLINE_IN_URI => (TIDY_WARNING, Attribute),
        NOFRAMES_CONTENT | NON_MATCHING_ENDTAG | OBSOLETE_ELEMENT => (TIDY_WARNING, Standard),
        OPTION_REMOVED | OPTION_REMOVED_APPLIED | OPTION_REMOVED_UNAPPLIED => {
            (TIDY_CONFIG, Standard)
        }
        PREVIOUS_LOCATION => (TIDY_INFO, Standard),
        PROPRIETARY_ATTR_VALUE | PROPRIETARY_ATTRIBUTE => (TIDY_WARNING, Attribute),
        PROPRIETARY_ELEMENT | REMOVED_HTML5 => (TIDY_WARNING, Standard),
        REPEATED_ATTRIBUTE => (TIDY_WARNING, Attribute),
        REPLACING_ELEMENT | REPLACING_UNEX_ELEMENT | SPACE_PRECEDING_XMLDECL => {
            (TIDY_WARNING, Standard)
        }
        STRING_ARGUMENT_BAD => (TIDY_CONFIG, Standard),
        STRING_CONTENT_LOOKS | STRING_DOCTYPE_GIVEN => (TIDY_INFO, Standard),
        STRING_MISSING_MALFORMED => (TIDY_CONFIG, Standard),
        STRING_MUTING_TYPE | STRING_NO_SYSID => (TIDY_INFO, Standard),
        STRING_UNKNOWN_OPTION => (TIDY_CONFIG, Standard),
        SUSPECTED_MISSING_QUOTE => (TIDY_WARNING, Standard),
        TAG_NOT_ALLOWED_IN | TOO_MANY_ELEMENTS_IN => {
            return Some((TIDY_WARNING, Standard, PREVIOUS_LOCATION));
        }
        TOO_MANY_ELEMENTS | TRIM_EMPTY_ELEMENT | UNESCAPED_AMPERSAND => (TIDY_WARNING, Standard),
        UNEXPECTED_END_OF_FILE_ATTR => (TIDY_WARNING, Attribute),
        UNEXPECTED_END_OF_FILE => (TIDY_WARNING, Standard),
        UNEXPECTED_ENDTAG_IN => (TIDY_ERROR, Standard),
        UNEXPECTED_ENDTAG => (TIDY_WARNING, Standard),
        UNEXPECTED_ENDTAG_ERR => (TIDY_ERROR, Standard),
        UNEXPECTED_EQUALSIGN | UNEXPECTED_GT | UNEXPECTED_QUOTEMARK => (TIDY_WARNING, Attribute),
        UNKNOWN_ELEMENT_LOOKS_CUSTOM | UNKNOWN_ELEMENT => (TIDY_ERROR, Standard),
        UNKNOWN_ENTITY | USING_BR_INPLACE_OF => (TIDY_WARNING, Standard),
        VENDOR_SPECIFIC_CHARS => (TIDY_WARNING, Encoding),
        WHITE_IN_URI => (TIDY_WARNING, Attribute),
        XML_DECLARATION_DETECTED => (TIDY_WARNING, Standard),
        XML_ID_SYNTAX => (TIDY_WARNING, Attribute),
        BLANK_TITLE_ELEMENT => (TIDY_WARNING, Standard),
        c if is_access_code(c) => (TIDY_ACCESS, Access),
        _ => return None,
    };
    Some((r.0, r.1, 0))
}

/// The access codes listed in the dispatch table.
fn is_access_code(c: u32) -> bool {
    const ACCESS: &[u32] = &[
        APPLET_MISSING_ALT,
        AREA_MISSING_ALT,
        ASCII_REQUIRES_DESCRIPTION,
        ASSOCIATE_LABELS_EXPLICITLY,
        ASSOCIATE_LABELS_EXPLICITLY_FOR,
        ASSOCIATE_LABELS_EXPLICITLY_ID,
        AUDIO_MISSING_TEXT_AIFF,
        AUDIO_MISSING_TEXT_AU,
        AUDIO_MISSING_TEXT_RA,
        AUDIO_MISSING_TEXT_RM,
        AUDIO_MISSING_TEXT_SND,
        AUDIO_MISSING_TEXT_WAV,
        COLOR_CONTRAST_ACTIVE_LINK,
        COLOR_CONTRAST_LINK,
        COLOR_CONTRAST_TEXT,
        COLOR_CONTRAST_VISITED_LINK,
        DATA_TABLE_MISSING_HEADERS,
        DATA_TABLE_MISSING_HEADERS_COLUMN,
        DATA_TABLE_MISSING_HEADERS_ROW,
        DATA_TABLE_REQUIRE_MARKUP_COLUMN_HEADERS,
        DATA_TABLE_REQUIRE_MARKUP_ROW_HEADERS,
        DOCTYPE_MISSING,
        ENSURE_PROGRAMMATIC_OBJECTS_ACCESSIBLE_APPLET,
        ENSURE_PROGRAMMATIC_OBJECTS_ACCESSIBLE_EMBED,
        ENSURE_PROGRAMMATIC_OBJECTS_ACCESSIBLE_OBJECT,
        ENSURE_PROGRAMMATIC_OBJECTS_ACCESSIBLE_SCRIPT,
        FRAME_MISSING_LONGDESC,
        FRAME_MISSING_NOFRAMES,
        FRAME_MISSING_TITLE,
        FRAME_SRC_INVALID,
        FRAME_TITLE_INVALID_NULL,
        FRAME_TITLE_INVALID_SPACES,
        HEADER_USED_FORMAT_TEXT,
        HEADERS_IMPROPERLY_NESTED,
        IMAGE_MAP_SERVER_SIDE_REQUIRES_CONVERSION,
        IMG_ALT_SUSPICIOUS_FILE_SIZE,
        IMG_ALT_SUSPICIOUS_FILENAME,
        IMG_ALT_SUSPICIOUS_PLACEHOLDER,
        IMG_ALT_SUSPICIOUS_TOO_LONG,
        IMG_BUTTON_MISSING_ALT,
        IMG_MAP_CLIENT_MISSING_TEXT_LINKS,
        IMG_MAP_SERVER_REQUIRES_TEXT_LINKS,
        IMG_MISSING_ALT,
        IMG_MISSING_DLINK,
        IMG_MISSING_LONGDESC,
        IMG_MISSING_LONGDESC_DLINK,
        INFORMATION_NOT_CONVEYED_APPLET,
        INFORMATION_NOT_CONVEYED_IMAGE,
        INFORMATION_NOT_CONVEYED_INPUT,
        INFORMATION_NOT_CONVEYED_OBJECT,
        INFORMATION_NOT_CONVEYED_SCRIPT,
        LANGUAGE_INVALID,
        LANGUAGE_NOT_IDENTIFIED,
        LAYOUT_TABLE_INVALID_MARKUP,
        LAYOUT_TABLES_LINEARIZE_PROPERLY,
        LINK_TEXT_MISSING,
        LINK_TEXT_NOT_MEANINGFUL,
        LINK_TEXT_NOT_MEANINGFUL_CLICK_HERE,
        LINK_TEXT_TOO_LONG,
        LIST_USAGE_INVALID_LI,
        LIST_USAGE_INVALID_OL,
        LIST_USAGE_INVALID_UL,
        METADATA_MISSING,
        METADATA_MISSING_REDIRECT_AUTOREFRESH,
        MULTIMEDIA_REQUIRES_TEXT,
        NEW_WINDOWS_REQUIRE_WARNING_BLANK,
        NEW_WINDOWS_REQUIRE_WARNING_NEW,
        NOFRAMES_INVALID_CONTENT,
        NOFRAMES_INVALID_LINK,
        NOFRAMES_INVALID_NO_VALUE,
        OBJECT_MISSING_ALT,
        POTENTIAL_HEADER_BOLD,
        POTENTIAL_HEADER_ITALICS,
        POTENTIAL_HEADER_UNDERLINE,
        PROGRAMMATIC_OBJECTS_REQUIRE_TESTING_APPLET,
        PROGRAMMATIC_OBJECTS_REQUIRE_TESTING_EMBED,
        PROGRAMMATIC_OBJECTS_REQUIRE_TESTING_OBJECT,
        PROGRAMMATIC_OBJECTS_REQUIRE_TESTING_SCRIPT,
        REMOVE_AUTO_REDIRECT,
        REMOVE_AUTO_REFRESH,
        REMOVE_BLINK_MARQUEE,
        REMOVE_FLICKER_ANIMATED_GIF,
        REMOVE_FLICKER_APPLET,
        REMOVE_FLICKER_EMBED,
        REMOVE_FLICKER_OBJECT,
        REMOVE_FLICKER_SCRIPT,
        REPLACE_DEPRECATED_HTML_APPLET,
        REPLACE_DEPRECATED_HTML_BASEFONT,
        REPLACE_DEPRECATED_HTML_CENTER,
        REPLACE_DEPRECATED_HTML_DIR,
        REPLACE_DEPRECATED_HTML_FONT,
        REPLACE_DEPRECATED_HTML_ISINDEX,
        REPLACE_DEPRECATED_HTML_MENU,
        REPLACE_DEPRECATED_HTML_S,
        REPLACE_DEPRECATED_HTML_STRIKE,
        REPLACE_DEPRECATED_HTML_U,
        SCRIPT_MISSING_NOSCRIPT,
        SCRIPT_NOT_KEYBOARD_ACCESSIBLE_ON_CLICK,
        SCRIPT_NOT_KEYBOARD_ACCESSIBLE_ON_MOUSE_DOWN,
        SCRIPT_NOT_KEYBOARD_ACCESSIBLE_ON_MOUSE_MOVE,
        SCRIPT_NOT_KEYBOARD_ACCESSIBLE_ON_MOUSE_OUT,
        SCRIPT_NOT_KEYBOARD_ACCESSIBLE_ON_MOUSE_OVER,
        SCRIPT_NOT_KEYBOARD_ACCESSIBLE_ON_MOUSE_UP,
        SKIPOVER_ASCII_ART,
        STYLE_SHEET_CONTROL_PRESENTATION,
        STYLESHEETS_REQUIRE_TESTING_LINK,
        STYLESHEETS_REQUIRE_TESTING_STYLE_ATTR,
        STYLESHEETS_REQUIRE_TESTING_STYLE_ELEMENT,
        TABLE_MAY_REQUIRE_HEADER_ABBR,
        TABLE_MAY_REQUIRE_HEADER_ABBR_NULL,
        TABLE_MAY_REQUIRE_HEADER_ABBR_SPACES,
        TABLE_MISSING_CAPTION,
        TABLE_MISSING_SUMMARY,
        TABLE_SUMMARY_INVALID_NULL,
        TABLE_SUMMARY_INVALID_PLACEHOLDER,
        TABLE_SUMMARY_INVALID_SPACES,
        TEXT_EQUIVALENTS_REQUIRE_UPDATING_APPLET,
        TEXT_EQUIVALENTS_REQUIRE_UPDATING_OBJECT,
        TEXT_EQUIVALENTS_REQUIRE_UPDATING_SCRIPT,
    ];
    ACCESS.contains(&c)
}

/// formatAccessReport()
fn format_access_report(
    doc: &mut Doc,
    node: Option<NodeId>,
    code: u32,
    level: u32,
) -> Option<Message> {
    doc.bad_access |= BA_WAI;
    Some(message_create_with_node(doc, node, code, level, &[]))
}

/// formatAttributeReport()
fn format_attribute_report(
    doc: &mut Doc,
    node: Option<NodeId>,
    code: u32,
    level: u32,
    args: &[RArg],
) -> Option<Message> {
    let tagdesc = trunc(tag_to_string(doc, node), 64);
    let mut name: Vec<u8> = b"NULL".to_vec();
    let mut value: Vec<u8> = b"NULL".to_vec();
    if let Some(RArg::Att(av)) = args.first() {
        if let Some(a) = &doc[*av].attribute {
            name = a.clone();
        }
        if let Some(v) = &doc[*av].value {
            value = v.clone();
        }
    }
    let td = s(&tagdesc);
    let m = match code {
        MISSING_QUOTEMARK_OPEN => message_create_with_node(doc, node, code, level, &[s(&name)]),
        BACKSLASH_IN_URI
        | ESCAPED_ILLEGAL_URI
        | FIXED_BACKSLASH
        | ID_NAME_MISMATCH
        | ILLEGAL_URI_CODEPOINT
        | ILLEGAL_URI_REFERENCE
        | INVALID_XML_ID
        | MISSING_IMAGEMAP
        | MISSING_QUOTEMARK
        | NEWLINE_IN_URI
        | UNEXPECTED_EQUALSIGN
        | UNEXPECTED_GT
        | UNEXPECTED_QUOTEMARK
        | WHITE_IN_URI => message_create_with_node(doc, node, code, level, &[td]),
        ATTRIBUTE_IS_NOT_ALLOWED
        | JOINING_ATTRIBUTE
        | MISSING_ATTR_VALUE
        | PROPRIETARY_ATTRIBUTE => {
            message_create_with_node(doc, node, code, level, &[td, s(&name)])
        }
        ATTRIBUTE_VALUE_REPLACED
        | BAD_ATTRIBUTE_VALUE
        | BAD_ATTRIBUTE_VALUE_REPLACED
        | INSERTING_AUTO_ATTRIBUTE
        | INVALID_ATTRIBUTE => {
            message_create_with_node(doc, node, code, level, &[td, s(&name), s(&value)])
        }
        MISMATCHED_ATTRIBUTE_ERROR | MISMATCHED_ATTRIBUTE_WARN => {
            let hv = html_version_str(doc);
            message_create_with_node(doc, node, code, level, &[td, s(&name), s(&hv)])
        }
        ANCHOR_NOT_UNIQUE
        | ANCHOR_DUPLICATED
        | ATTR_VALUE_NOT_LCASE
        | PROPRIETARY_ATTR_VALUE
        | XML_ID_SYNTAX => message_create_with_node(doc, node, code, level, &[td, s(&value)]),
        REPEATED_ATTRIBUTE => {
            message_create_with_node(doc, node, code, level, &[td, s(&value), s(&name)])
        }
        UNEXPECTED_END_OF_FILE_ATTR => {
            if let Some(i) = doc.doc_in.as_ref() {
                let (l, c) = (i.curline as u32, i.curcol as u32);
                let lx = doc.lx();
                lx.lines = l;
                lx.columns = c;
            }
            message_create_with_lexer(doc, code, level, &[td])
        }
        _ => return None,
    };
    Some(m)
}

/// NtoS()
fn ntos(n: u32) -> Vec<u8> {
    (n as i32).to_string().into_bytes()
}

/// formatEncodingReport()
fn format_encoding_report(doc: &mut Doc, code: u32, level: u32, args: &[RArg]) -> Option<Message> {
    let c = arg_uint(args, 0);
    let discarded = arg_bool(args, 1);
    let action = localized_string(if discarded {
        STRING_DISCARDING
    } else {
        STRING_REPLACING
    })
    .unwrap_or_default()
    .to_vec();
    let mut buf: Vec<u8> = Vec::new();
    match code {
        INVALID_NCR => {
            buf = ntos(c);
            doc.bad_chars |= BC_INVALID_NCR;
        }
        INVALID_SGML_CHARS => {
            buf = ntos(c);
            doc.bad_chars |= BC_INVALID_SGML_CHARS;
        }
        INVALID_UTF8 => {
            buf = format!("U+{c:04X}").into_bytes();
            doc.bad_chars |= BC_INVALID_UTF8;
        }
        INVALID_UTF16 => {
            buf = format!("U+{c:04X}").into_bytes();
            doc.bad_chars |= BC_INVALID_UTF16;
        }
        VENDOR_SPECIFIC_CHARS => {
            buf = ntos(c);
            doc.bad_chars |= BC_VENDOR_SPECIFIC_CHARS;
        }
        ENCODING_MISMATCH => {
            doc.bad_chars |= BC_ENCODING_MISMATCH;
            let inenc = doc.doc_in.as_ref().map_or(0, |i| i.encoding) as i32;
            let a = crate::config::char_encoding_name(inenc);
            let b = crate::config::char_encoding_name(c as i32);
            return Some(message_create_with_lexer(
                doc,
                code,
                level,
                &[s(a.as_bytes()), s(b.as_bytes())],
            ));
        }
        _ => {}
    }
    Some(message_create_with_lexer(
        doc,
        code,
        level,
        &[s(&action), s(&buf)],
    ))
}

/// formatStandard()
fn format_standard(
    doc: &mut Doc,
    element: Option<NodeId>,
    node: Option<NodeId>,
    code: u32,
    level: u32,
    args: &[RArg],
) -> Option<Message> {
    let nodedesc = trunc(tag_to_string(doc, node), 256);
    let elemdesc = if element.is_some() {
        trunc(tag_to_string(doc, element), 256)
    } else {
        Vec::new()
    };
    let rpt = element.or(node);
    let nd = s(&nodedesc);
    let ed = s(&elemdesc);
    let m = match code {
        CUSTOM_TAG_DETECTED => {
            let tagtype = match doc.cfg(OptId::UseCustomTags) {
                TIDY_CUSTOM_BLOCKLEVEL => localized_string(TIDYCUSTOMBLOCKLEVEL_STRING),
                TIDY_CUSTOM_EMPTY => localized_string(TIDYCUSTOMEMPTY_STRING),
                TIDY_CUSTOM_INLINE => localized_string(TIDYCUSTOMINLINE_STRING),
                _ => localized_string(TIDYCUSTOMPRE_STRING),
            };
            message_create_with_node(doc, element, code, level, &[ed, so(tagtype)])
        }
        STRING_NO_SYSID => message_create(doc, code, level, &[]),
        FILE_CANT_OPEN
        | FILE_CANT_OPEN_CFG
        | FILE_NOT_FILE
        | STRING_CONTENT_LOOKS
        | STRING_DOCTYPE_GIVEN
        | STRING_MISSING_MALFORMED
        | STRING_MUTING_TYPE => {
            let st = arg_str(args, 0)?;
            message_create(doc, code, level, &[s(&st)])
        }
        APOS_UNDEFINED
        | MISSING_SEMICOLON_NCR
        | MISSING_SEMICOLON
        | UNESCAPED_AMPERSAND
        | UNKNOWN_ENTITY => {
            let en = arg_str(args, 0).unwrap_or_else(|| b"NULL".to_vec());
            message_create_with_lexer(doc, code, level, &[s(&en)])
        }
        MISSING_ATTRIBUTE => {
            let name = arg_str(args, 0)?;
            message_create_with_node(doc, node, code, level, &[nd, s(&name)])
        }
        STRING_UNKNOWN_OPTION | OPTION_REMOVED => {
            let st = arg_str(args, 0)?;
            message_create_with_lexer(doc, code, level, &[s(&st)])
        }
        OPTION_REMOVED_UNAPPLIED | STRING_ARGUMENT_BAD => {
            let s1 = FArg::S(arg_str(args, 0));
            let s2 = FArg::S(arg_str(args, 1));
            message_create_with_lexer(doc, code, level, &[s1, s2])
        }
        OPTION_REMOVED_APPLIED => {
            let s1 = FArg::S(arg_str(args, 0));
            let s2 = FArg::S(arg_str(args, 1));
            let s3 = FArg::S(arg_str(args, 2));
            message_create_with_lexer(doc, code, level, &[s1, s2, s3])
        }
        BAD_SURROGATE_LEAD | BAD_SURROGATE_PAIR | BAD_SURROGATE_TAIL => {
            let c1 = arg_uint(args, 0);
            let c2 = arg_uint(args, 1);
            message_create_with_lexer(doc, code, level, &[FArg::U(c1), FArg::U(c2)])
        }
        SPACE_PRECEDING_XMLDECL => message_create_with_node(doc, node, code, level, &[]),
        CANT_BE_NESTED | NOFRAMES_CONTENT | USING_BR_INPLACE_OF => {
            message_create_with_node(doc, node, code, level, &[nd])
        }
        ELEMENT_VERS_MISMATCH_ERROR | ELEMENT_VERS_MISMATCH_WARN => {
            let hv = html_version_str(doc);
            message_create_with_node(doc, node, code, level, &[nd, s(&hv)])
        }
        TAG_NOT_ALLOWED_IN => {
            let en = element_name(doc, element);
            message_create_with_node(doc, node, code, level, &[nd, en])
        }
        INSERTING_TAG
        | MISSING_STARTTAG
        | TOO_MANY_ELEMENTS
        | UNEXPECTED_ENDTAG
        | UNEXPECTED_ENDTAG_ERR => {
            let nn = element_name(doc, node);
            message_create_with_node(doc, node, code, level, &[nn])
        }
        UNEXPECTED_ENDTAG_IN => {
            let nn = element_name(doc, node);
            let en = element_name(doc, element);
            message_create_with_node(doc, node, code, level, &[nn, en])
        }
        BAD_CDATA_CONTENT
        | CONTENT_AFTER_BODY
        | DOCTYPE_AFTER_TAGS
        | DUPLICATE_FRAMESET
        | MALFORMED_COMMENT
        | MALFORMED_COMMENT_DROPPING
        | MALFORMED_COMMENT_EOS
        | MALFORMED_COMMENT_WARN
        | MALFORMED_DOCTYPE
        | MISSING_DOCTYPE
        | MISSING_TITLE_ELEMENT
        | NESTED_QUOTATION
        | SUSPECTED_MISSING_QUOTE
        | XML_DECLARATION_DETECTED
        | BLANK_TITLE_ELEMENT => message_create_with_node(doc, rpt, code, level, &[]),
        ELEMENT_NOT_EMPTY
        | FOUND_STYLE_IN_BODY
        | ILLEGAL_NESTING
        | MOVED_STYLE_TO_HEAD
        | TRIM_EMPTY_ELEMENT
        | UNEXPECTED_END_OF_FILE => message_create_with_node(doc, rpt, code, level, &[ed]),
        OBSOLETE_ELEMENT | REPLACING_ELEMENT | REPLACING_UNEX_ELEMENT => {
            message_create_with_node(doc, rpt, code, level, &[ed, nd])
        }
        ADDED_MISSING_CHARSET
        | BAD_SUMMARY_HTML5
        | NESTED_EMPHASIS
        | PROPRIETARY_ELEMENT
        | REMOVED_HTML5
        | UNKNOWN_ELEMENT
        | UNKNOWN_ELEMENT_LOOKS_CUSTOM => message_create_with_node(doc, rpt, code, level, &[nd]),
        MISSING_ENDTAG_FOR | MISSING_ENDTAG_OPTIONAL | PREVIOUS_LOCATION => {
            let en = element_name(doc, element);
            message_create_with_node(doc, rpt, code, level, &[en])
        }
        MISSING_ENDTAG_BEFORE => {
            let en = element_name(doc, element);
            message_create_with_node(doc, rpt, code, level, &[en, nd])
        }
        COERCE_TO_ENDTAG | NON_MATCHING_ENDTAG => {
            let nn = element_name(doc, node);
            message_create_with_node(doc, rpt, code, level, &[nn.clone(), nn])
        }
        TOO_MANY_ELEMENTS_IN => {
            let nn = element_name(doc, node);
            let en = element_name(doc, element);
            message_create_with_node(doc, rpt, code, level, &[nn, en])
        }
        _ => return None,
    };
    Some(m)
}

/// formatStandardDynamic()
fn format_standard_dynamic(doc: &mut Doc, node: Option<NodeId>, code: u32) -> Option<Message> {
    let nodedesc = trunc(tag_to_string(doc, node), 256);
    match code {
        DISCARDING_UNEXPECTED => {
            let level = if doc.bad_form != 0 {
                TIDY_ERROR
            } else {
                TIDY_WARNING
            };
            Some(message_create_with_node(
                doc,
                node,
                code,
                level,
                &[s(&nodedesc)],
            ))
        }
        _ => None,
    }
}

/// vReport()
fn v_report(
    doc: &mut Doc,
    element: Option<NodeId>,
    node: Option<NodeId>,
    code: u32,
    args: &[RArg],
) {
    let Some((level, handler, next)) = dispatch(code) else {
        return;
    };
    let message = match handler {
        Formatter::Access => format_access_report(doc, node, code, level),
        Formatter::Attribute => format_attribute_report(doc, node, code, level, args),
        Formatter::Encoding => format_encoding_report(doc, code, level, args),
        Formatter::Standard => format_standard(doc, element, node, code, level, args),
        Formatter::StandardDynamic => format_standard_dynamic(doc, node, code),
    };
    message_out(doc, message);
    if next != 0 {
        v_report(doc, element, node, next, args);
    }
}

/// Report(): the general report function; `args` are the va_list
/// arguments the message code expects.
pub fn report(
    doc: &mut Doc,
    element: Option<NodeId>,
    node: Option<NodeId>,
    code: u32,
    args: &[RArg],
) {
    v_report(doc, element, node, code, args);
}

/// ReportAccessError()
pub fn report_access_error(doc: &mut Doc, node: Option<NodeId>, code: u32) {
    report(doc, None, node, code, &[]);
}

/// ReportAttrError()
pub fn report_attr_error(doc: &mut Doc, node: NodeId, av: Option<AttId>, code: u32) {
    let args: Vec<RArg> = av.map(RArg::Att).into_iter().collect();
    report(doc, None, Some(node), code, &args);
}

/// ReportBadArgument()
pub fn report_bad_argument(doc: &mut Doc, option: &[u8]) {
    report(
        doc,
        None,
        None,
        STRING_MISSING_MALFORMED,
        &[RArg::Str(option.to_vec())],
    );
}

/// ReportEntityError()
pub fn report_entity_error(doc: &mut Doc, code: u32, entity: Option<&[u8]>, c: i32) {
    let mut args = Vec::new();
    match entity {
        Some(e) => args.push(RArg::Str(e.to_vec())),
        None => args.push(RArg::Node(None)),
    }
    args.push(RArg::Int(c));
    report(doc, None, None, code, &args);
}

/// ReportFileError()
pub fn report_file_error(doc: &mut Doc, file: &[u8], code: u32) {
    report(doc, None, None, code, &[RArg::Str(file.to_vec())]);
}

/// ReportEncodingError()
pub fn report_encoding_error(doc: &mut Doc, code: u32, c: u32, discarded: bool) {
    report(
        doc,
        None,
        None,
        code,
        &[RArg::Uint(c), RArg::Bool(discarded)],
    );
}

/// ReportEncodingWarning()
pub fn report_encoding_warning(doc: &mut Doc, code: u32, encoding: u32) {
    report(
        doc,
        None,
        None,
        code,
        &[RArg::Uint(encoding), RArg::Bool(false)],
    );
}

/// ReportMissingAttr()
pub fn report_missing_attr(doc: &mut Doc, node: NodeId, name: &[u8]) {
    report(
        doc,
        None,
        Some(node),
        MISSING_ATTRIBUTE,
        &[RArg::Str(name.to_vec())],
    );
}

/// ReportSurrogateError()
pub fn report_surrogate_error(doc: &mut Doc, code: u32, c1: u32, c2: u32) {
    report(doc, None, None, code, &[RArg::Uint(c1), RArg::Uint(c2)]);
}

/// ReportUnknownOption()
pub fn report_unknown_option(doc: &mut Doc, option: &[u8]) {
    report(
        doc,
        None,
        None,
        STRING_UNKNOWN_OPTION,
        &[RArg::Str(option.to_vec())],
    );
}

/// dialogueDispatchTable[]
fn dialogue_level(code: u32) -> Option<u32> {
    Some(match code {
        STRING_HELLO_ACCESS | TEXT_GENERAL_INFO | TEXT_GENERAL_INFO_PLEA => TIDY_DIALOGUE_INFO,
        STRING_NEEDS_INTERVENTION
        | STRING_ERROR_COUNT
        | STRING_NO_ERRORS
        | STRING_NOT_ALL_SHOWN => TIDY_DIALOGUE_SUMMARY,
        FOOTNOTE_TRIM_EMPTY_ELEMENT
        | TEXT_ACCESS_ADVICE1
        | TEXT_ACCESS_ADVICE2
        | TEXT_BAD_FORM
        | TEXT_BAD_MAIN
        | TEXT_HTML_T_ALGORITHM
        | TEXT_INVALID_URI
        | TEXT_INVALID_UTF8
        | TEXT_INVALID_UTF16
        | TEXT_M_IMAGE_ALT
        | TEXT_M_IMAGE_MAP
        | TEXT_M_LINK_ALT
        | TEXT_M_SUMMARY
        | TEXT_SGML_CHARS
        | TEXT_USING_BODY
        | TEXT_USING_FONT
        | TEXT_USING_FRAMES
        | TEXT_USING_LAYER
        | TEXT_USING_NOBR
        | TEXT_USING_SPACER
        | TEXT_VENDOR_CHARS => TIDY_DIALOGUE_FOOTNOTE,
        _ => return None,
    })
}

/// Dialogue()
pub fn dialogue(doc: &mut Doc, code: u32, args: &[RArg]) {
    let Some(level) = dialogue_level(code) else {
        return;
    };
    let message = match code {
        TEXT_SGML_CHARS | TEXT_VENDOR_CHARS => {
            let st = FArg::S(arg_str(args, 0));
            message_create(doc, code, level, &[st])
        }
        STRING_ERROR_COUNT | STRING_NOT_ALL_SHOWN => {
            let w = doc.warnings;
            let e = doc.errors;
            let ws = so(localized_string_n(STRING_ERROR_COUNT_WARNING, w));
            let es = so(localized_string_n(STRING_ERROR_COUNT_ERROR, e));
            message_create(doc, code, level, &[FArg::U(w), ws, FArg::U(e), es])
        }
        _ => message_create(doc, code, level, &[]),
    };
    message_out(doc, Some(message));
}

/// ErrorSummary(): the footnotes and other dialogue information
pub fn error_summary(doc: &mut Doc) {
    let mut encnam: Vec<u8> = localized_string(STRING_SPECIFIED)
        .unwrap_or_default()
        .to_vec();
    let charenc = doc.cfg(OptId::CharEncoding);
    if charenc == WIN1252 {
        encnam = b"Windows-1252".to_vec();
    } else if charenc == MACROMAN {
        encnam = b"MacRoman".to_vec();
    } else if charenc == IBM858 {
        encnam = b"ibm858".to_vec();
    } else if charenc == LATIN0 {
        encnam = b"latin0".to_vec();
    }

    if doc.bad_access & (BA_USING_FRAMES | BA_USING_NOFRAMES) != 0
        && !((doc.bad_access & BA_USING_FRAMES) != 0 && (doc.bad_access & BA_USING_NOFRAMES) == 0)
    {
        doc.bad_access &= !(BA_USING_FRAMES | BA_USING_NOFRAMES);
    }

    if doc.bad_chars != 0 {
        if doc.bad_chars & BC_VENDOR_SPECIFIC_CHARS != 0 {
            dialogue(doc, TEXT_VENDOR_CHARS, &[RArg::Str(encnam.clone())]);
        }
        if doc.bad_chars & BC_INVALID_SGML_CHARS != 0 || doc.bad_chars & BC_INVALID_NCR != 0 {
            dialogue(doc, TEXT_SGML_CHARS, &[RArg::Str(encnam.clone())]);
        }
        if doc.bad_chars & BC_INVALID_UTF8 != 0 {
            dialogue(doc, TEXT_INVALID_UTF8, &[]);
        }
        if doc.bad_chars & BC_INVALID_UTF16 != 0 {
            dialogue(doc, TEXT_INVALID_UTF16, &[]);
        }
        if doc.bad_chars & BC_INVALID_URI != 0 {
            dialogue(doc, TEXT_INVALID_URI, &[]);
        }
    }

    if doc.bad_form != 0 {
        if doc.bad_form & FLG_BAD_FORM != 0 {
            dialogue(doc, TEXT_BAD_FORM, &[]);
        }
        if doc.bad_form & FLG_BAD_MAIN != 0 {
            dialogue(doc, TEXT_BAD_MAIN, &[]);
        }
    }

    if doc.bad_access != 0 {
        if doc.cfg(OptId::AccessibilityCheckLevel) == 0 {
            if doc.bad_access & BA_MISSING_SUMMARY != 0 {
                dialogue(doc, TEXT_M_SUMMARY, &[]);
            }
            if doc.bad_access & BA_MISSING_IMAGE_ALT != 0 {
                dialogue(doc, TEXT_M_IMAGE_ALT, &[]);
            }
            if doc.bad_access & BA_MISSING_IMAGE_MAP != 0 {
                dialogue(doc, TEXT_M_IMAGE_MAP, &[]);
            }
            if doc.bad_access & BA_MISSING_LINK_ALT != 0 {
                dialogue(doc, TEXT_M_LINK_ALT, &[]);
            }
            if doc.bad_access & BA_USING_FRAMES != 0 && doc.bad_access & BA_USING_NOFRAMES == 0 {
                dialogue(doc, TEXT_USING_FRAMES, &[]);
            }
        }
        if doc.cfg(OptId::AccessibilityCheckLevel) > 0 {
            dialogue(doc, TEXT_ACCESS_ADVICE2, &[]);
        } else {
            dialogue(doc, TEXT_ACCESS_ADVICE1, &[]);
        }
    }

    if doc.bad_layout != 0 {
        if doc.bad_layout & USING_LAYER != 0 {
            dialogue(doc, TEXT_USING_LAYER, &[]);
        }
        if doc.bad_layout & USING_SPACER != 0 {
            dialogue(doc, TEXT_USING_SPACER, &[]);
        }
        if doc.bad_layout & USING_FONT != 0 {
            dialogue(doc, TEXT_USING_FONT, &[]);
        }
        if doc.bad_layout & USING_NOBR != 0 {
            dialogue(doc, TEXT_USING_NOBR, &[]);
        }
        if doc.bad_layout & USING_BODY != 0 {
            dialogue(doc, TEXT_USING_BODY, &[]);
        }
    }

    if doc.footnotes != 0 && doc.footnotes & FN_TRIM_EMPTY_ELEMENT != 0 {
        dialogue(doc, FOOTNOTE_TRIM_EMPTY_ELEMENT, &[]);
    }
}

/// ReportMarkupVersion(): the document HTML version and version-related
/// information
pub fn report_markup_version(doc: &mut Doc) {
    if let Some(gd) = doc.given_doctype.clone() {
        report(doc, None, None, STRING_DOCTYPE_GIVEN, &[RArg::Str(gd)]);
    }
    if !doc.cfg_bool(OptId::XmlTags) {
        let is_xhtml = doc.lxr().isvoyager;
        let apparent_vers = lexer::apparent_version(doc);
        let vers = match lexer::html_version_name_from_code(apparent_vers, is_xhtml) {
            Some(v) => v.as_bytes().to_vec(),
            None => localized_string(STRING_HTML_PROPRIETARY)
                .unwrap_or_default()
                .to_vec(),
        };
        report(doc, None, None, STRING_CONTENT_LOOKS, &[RArg::Str(vers)]);
        if lexer::warn_missing_si_in_emitted_doc_type(doc) {
            report(doc, None, None, STRING_NO_SYSID, &[]);
        }
    }
}

/// ReportNumWarnings(): the number of warnings and errors found
pub fn report_num_warnings(doc: &mut Doc) {
    if doc.warnings > 0 || doc.errors > 0 {
        if doc.errors > doc.cfg(OptId::ShowErrors) || !doc.cfg_bool(OptId::ShowWarnings) {
            dialogue(doc, STRING_NOT_ALL_SHOWN, &[]);
        } else {
            dialogue(doc, STRING_ERROR_COUNT, &[]);
        }
    } else {
        dialogue(doc, STRING_NO_ERRORS, &[]);
    }
}

/// FreeMutedMessageList()
pub fn free_muted_message_list(doc: &mut Doc) {
    doc.muted.clear();
}

/// DefineMutedMessage()
pub fn define_muted_message(doc: &mut Doc, opt: OptId, name: &[u8]) {
    let message = error_code_from_key(name);
    if message <= REPORT_MESSAGE_FIRST || message >= REPORT_MESSAGE_LAST {
        let on = crate::config::get_option(opt).name.as_bytes().to_vec();
        report(
            doc,
            None,
            None,
            STRING_ARGUMENT_BAD,
            &[RArg::Str(on), RArg::Str(name.to_vec())],
        );
        return;
    }
    doc.muted.push(message);
    report(
        doc,
        None,
        None,
        STRING_MUTING_TYPE,
        &[RArg::Str(name.to_vec())],
    );
}

/// getMutedMessageList() / getNextMutedMessage(): the keys
pub fn get_muted_messages(doc: &Doc) -> Vec<&'static str> {
    doc.muted.iter().map(|&c| error_code_as_key(c)).collect()
}

/// The config categories of tidyStringsKeys[] (FOREACH_TIDYCONFIGCATEGORY).
const CATEGORY_KEYS: &[(&str, u32)] = &[
    ("TidyDiagnostics", 301),
    ("TidyDisplay", 302),
    ("TidyDocumentIO", 303),
    ("TidyEncoding", 304),
    ("TidyFileIO", 305),
    ("TidyMarkupCleanup", 306),
    ("TidyMarkupEntities", 307),
    ("TidyMarkupRepair", 308),
    ("TidyMarkupTeach", 309),
    ("TidyMarkupXForm", 310),
    ("TidyPrettyPrint", 311),
    ("TidyInternalCategory", 312),
];

/// tidyErrorCodeAsKey()
pub fn error_code_as_key(code: u32) -> &'static str {
    for &(k, v) in CATEGORY_KEYS {
        if v == code {
            return k;
        }
    }
    for &(k, v) in CODE_KEYS {
        if k == "REPORT_MESSAGE_FIRST" || k == "REPORT_MESSAGE_LAST" || k == "TIDYSTRINGS_LAST" {
            continue;
        }
        if v == code {
            return k;
        }
    }
    if code == TIDYSTRINGS_FIRST {
        return "TIDYSTRINGS_FIRST";
    }
    if code == TIDYSTRINGS_LAST {
        return "TIDYSTRINGS_LAST";
    }
    "UNDEFINED"
}

/// tidyErrorCodeFromKey()
pub fn error_code_from_key(code: &[u8]) -> u32 {
    let code = crate::tmbstr::cstr(code);
    for &(k, v) in CATEGORY_KEYS {
        if k.as_bytes() == code {
            return v;
        }
    }
    for &(k, v) in CODE_KEYS {
        if k == "REPORT_MESSAGE_FIRST" || k == "REPORT_MESSAGE_LAST" || k == "TIDYSTRINGS_LAST" {
            continue;
        }
        if k.as_bytes() == code {
            return v;
        }
    }
    if code == b"TIDYSTRINGS_FIRST" {
        return TIDYSTRINGS_FIRST;
    }
    if code == b"TIDYSTRINGS_LAST" {
        return TIDYSTRINGS_LAST;
    }
    u32::MAX
}
