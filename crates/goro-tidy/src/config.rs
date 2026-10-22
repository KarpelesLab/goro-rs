//! config.c: the configuration options (names, types, defaults, pick
//! lists, parsers), configuration files and option values.

use crate::configdefs::OPTION_DEFS;
use crate::doc::*;
use crate::enums::code::*;
use crate::enums::{N_TIDY_OPTIONS, OptId};
use crate::lexer;
use crate::message;
use crate::streamio::{self, InSel, StreamIn};
use crate::tags::{self, UserTagType};
use crate::tmbstr;

/// TidyOptionType
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OptionType {
    String,
    Integer,
    Boolean,
}

/// TidyConfigCategory
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ConfigCategory {
    Diagnostics = 301,
    Display,
    DocumentIO,
    Encoding,
    FileIO,
    MarkupCleanup,
    MarkupEntities,
    MarkupRepair,
    MarkupTeach,
    MarkupXForm,
    PrettyPrint,
    InternalCategory,
}

/// `ParseProperty`
pub type ParsePropertyFn = fn(&mut Doc, &'static OptionImpl) -> bool;

/// PickListItem
#[derive(Debug)]
pub struct PickListItem {
    pub label: &'static str,
    pub value: i32,
    pub inputs: &'static [&'static str],
}

/// _tidy_option (TidyOptionImpl)
#[derive(Debug)]
pub struct OptionImpl {
    pub id: OptId,
    pub category: ConfigCategory,
    pub name: &'static str,
    pub type_: OptionType,
    /// Default value for TidyInteger and TidyBoolean
    pub dflt: u64,
    /// Function to parse input; read-only if None.
    pub parser: Option<ParsePropertyFn>,
    pub pick_list: Option<&'static [PickListItem]>,
    /// Default value for TidyString.
    pub pdflt: Option<&'static str>,
}

/// TidyConfigImpl
#[derive(Clone, Debug)]
pub struct ConfigImpl {
    pub value: Vec<OptValue>,
    pub snapshot: Vec<OptValue>,
    pub defined_tags: u32,
    /// Current char in input stream for reading options.
    pub c: u32,
    /// Current input source for reading options.
    pub cfg_in: Option<StreamIn>,
}

impl Default for ConfigImpl {
    fn default() -> Self {
        ConfigImpl {
            value: vec![OptValue::default(); N_TIDY_OPTIONS + 1],
            snapshot: vec![OptValue::default(); N_TIDY_OPTIONS + 1],
            defined_tags: 0,
            c: 0,
            cfg_in: None,
        }
    }
}

/// InitConfig()
pub fn init_config(doc: &mut Doc) {
    doc.config = ConfigImpl::default();
    reset_config_to_default(doc);
}

/// FreeConfig()
pub fn free_config(doc: &mut Doc) {
    reset_config_to_default(doc);
    take_config_snapshot(doc);
}

/// lookupOption()
pub fn lookup_option(s: &[u8]) -> Option<&'static OptionImpl> {
    OPTION_DEFS[..N_TIDY_OPTIONS]
        .iter()
        .find(|np| tmbstr::tmbstrcasecmp(s, np.name.as_bytes()) == 0)
}

/// getOption()
pub fn get_option(opt_id: OptId) -> &'static OptionImpl {
    &OPTION_DEFS[opt_id as usize]
}

/// getOptionIsList()
pub fn get_option_is_list(opt_id: OptId) -> bool {
    get_option(opt_id)
        .parser
        .is_some_and(|p| std::ptr::fn_addr_eq(p, parse_list as ParsePropertyFn))
}

/// getOptionList() / getNextOption(): every option after TidyUnknownOption
pub fn get_option_list(_doc: &Doc) -> Vec<&'static OptionImpl> {
    OPTION_DEFS[1..N_TIDY_OPTIONS].iter().collect()
}

/// getOptionPickList() / getNextOptionPick(): the pick labels
pub fn get_option_pick_list(option: &'static OptionImpl) -> Vec<&'static str> {
    option
        .pick_list
        .map(|p| p.iter().map(|i| i.label).collect())
        .unwrap_or_default()
}

/// SetOptionValue(): only a value with length is stored (Issue #218)
fn set_option_value(doc: &mut Doc, opt_id: OptId, val: Option<&[u8]>) -> bool {
    let v = val.map(tmbstr::cstr).filter(|v| !v.is_empty());
    doc.config.value[opt_id as usize].p = v.map(|v| v.to_vec());
    true
}

/// GetPickListLabelForPick()
pub fn get_pick_list_label_for_pick(opt_id: OptId, pick: u32) -> Option<&'static str> {
    let option = get_option(opt_id);
    let pl = option.pick_list?;
    pl.get(pick as usize).map(|i| i.label)
}

/// SetOptionInt()
pub fn set_option_int(doc: &mut Doc, opt_id: OptId, val: u64) -> bool {
    if (opt_id as usize) < N_TIDY_OPTIONS {
        doc.config.value[opt_id as usize].v = val;
        true
    } else {
        false
    }
}

/// SetOptionBool()
pub fn set_option_bool(doc: &mut Doc, opt_id: OptId, val: bool) -> bool {
    set_option_int(doc, opt_id, val as u64)
}

fn option_default(option: &OptionImpl) -> OptValue {
    if option.type_ == OptionType::String {
        OptValue {
            v: 0,
            p: option.pdflt.map(|s| s.as_bytes().to_vec()),
        }
    } else {
        OptValue {
            v: option.dflt,
            p: None,
        }
    }
}

fn copy_option_value(option: &OptionImpl, oldval: &mut OptValue, newval: &OptValue) {
    if option.type_ == OptionType::String {
        oldval.p = newval.p.clone();
    } else {
        oldval.v = newval.v;
    }
}

/// ResetOptionToDefault()
pub fn reset_option_to_default(doc: &mut Doc, opt_id: OptId) -> bool {
    let status = (opt_id as usize) > 0 && (opt_id as usize) < N_TIDY_OPTIONS;
    if status {
        let option = get_option(opt_id);
        let dflt = option_default(option);
        copy_option_value(option, &mut doc.config.value[opt_id as usize], &dflt);
    }
    status
}

/// ReparseTagType()
fn reparse_tag_type(doc: &mut Doc, opt_id: OptId) {
    let tagdecl = doc.cfg_str(opt_id).map(|s| s.to_vec());
    parse_config_value_opt(doc, opt_id, tagdecl.as_deref());
}

fn option_value_identical(option: &OptionImpl, a: &OptValue, b: &OptValue) -> bool {
    if option.type_ == OptionType::String {
        a.p == b.p
    } else {
        a.v == b.v
    }
}

const USER_TAG_OPTS: [(OptId, UserTagType); 4] = [
    (OptId::InlineTags, UserTagType::Inline),
    (OptId::BlockTags, UserTagType::Block),
    (OptId::EmptyTags, UserTagType::Empty),
    (OptId::PreTags, UserTagType::Pre),
];

/// NeedReparseTagDecls(): the changed user tag types
fn need_reparse_tag_decls(current: &[OptValue], new: &[OptValue]) -> Vec<(OptId, UserTagType)> {
    USER_TAG_OPTS
        .iter()
        .filter(|(id, _)| {
            let option = get_option(*id);
            !option_value_identical(option, &current[*id as usize], &new[*id as usize])
        })
        .copied()
        .collect()
}

/// ReparseTagDecls()
fn reparse_tag_decls(doc: &mut Doc, changed: &[(OptId, UserTagType)]) {
    for &(id, t) in changed {
        tags::free_declared_tags(doc, t);
        reparse_tag_type(doc, id);
    }
}

/// ResetConfigToDefault()
pub fn reset_config_to_default(doc: &mut Doc) {
    for (ix, option) in OPTION_DEFS[..N_TIDY_OPTIONS].iter().enumerate() {
        let dflt = option_default(option);
        copy_option_value(option, &mut doc.config.value[ix], &dflt);
    }
    tags::free_declared_tags(doc, UserTagType::Null);
}

/// TakeConfigSnapshot()
pub fn take_config_snapshot(doc: &mut Doc) {
    for (ix, option) in OPTION_DEFS[..N_TIDY_OPTIONS].iter().enumerate() {
        let v = doc.config.value[ix].clone();
        copy_option_value(option, &mut doc.config.snapshot[ix], &v);
    }
}

/// ResetConfigToSnapshot()
pub fn reset_config_to_snapshot(doc: &mut Doc) {
    let changed = need_reparse_tag_decls(&doc.config.value, &doc.config.snapshot);
    for (ix, option) in OPTION_DEFS[..N_TIDY_OPTIONS].iter().enumerate() {
        let s = doc.config.snapshot[ix].clone();
        copy_option_value(option, &mut doc.config.value[ix], &s);
    }
    if !changed.is_empty() {
        reparse_tag_decls(doc, &changed);
    }
}

/// CopyConfig()
pub fn copy_config(doc_to: &mut Doc, doc_from: &Doc) {
    let changed = need_reparse_tag_decls(&doc_to.config.value, &doc_from.config.value);
    take_config_snapshot(doc_to);
    for (ix, option) in OPTION_DEFS[..N_TIDY_OPTIONS].iter().enumerate() {
        copy_option_value(
            option,
            &mut doc_to.config.value[ix],
            &doc_from.config.value[ix],
        );
    }
    if !changed.is_empty() {
        reparse_tag_decls(doc_to, &changed);
    }
}

// ---------------------------------------------------------------------------
// reading characters of a configuration value / file

fn get_c(doc: &mut Doc) -> u32 {
    if doc.config.cfg_in.is_some() {
        streamio::read_char(doc, InSel::Cfg)
    } else {
        END_OF_STREAM
    }
}

fn first_char(doc: &mut Doc) -> u32 {
    let c = get_c(doc);
    doc.config.c = c;
    c
}

fn advance_char(doc: &mut Doc) -> u32 {
    if doc.config.c != END_OF_STREAM {
        let c = get_c(doc);
        doc.config.c = c;
    }
    doc.config.c
}

fn skip_white(doc: &mut Doc) -> u32 {
    while lexer::is_white(doc.config.c) && !lexer::is_newline(doc.config.c) {
        let c = get_c(doc);
        doc.config.c = c;
    }
    doc.config.c
}

/// NextProperty(): skip over line continuations to start of next property
fn next_property(doc: &mut Doc) -> u32 {
    loop {
        while doc.config.c != '\n' as u32
            && doc.config.c != '\r' as u32
            && doc.config.c != END_OF_STREAM
        {
            let c = get_c(doc);
            doc.config.c = c;
        }
        if doc.config.c == '\r' as u32 {
            let c = get_c(doc);
            doc.config.c = c;
        }
        if doc.config.c == '\n' as u32 {
            let c = get_c(doc);
            doc.config.c = c;
        }
        if !lexer::is_white(doc.config.c) {
            break;
        }
    }
    doc.config.c
}

/// ExpandTilde()
fn expand_tilde(filename: &[u8]) -> Vec<u8> {
    if filename.first() != Some(&b'~') {
        return filename.to_vec();
    }
    let mut home_dir: Option<Vec<u8>> = None;
    let mut rest = filename;
    if filename.get(1) == Some(&b'/') {
        if let Some(h) = std::env::var_os("HOME") {
            use std::os::unix::ffi::OsStrExt;
            home_dir = Some(h.as_bytes().to_vec());
            rest = &filename[1..];
        }
    }
    match home_dir {
        Some(mut h) => {
            h.extend_from_slice(rest);
            h
        }
        None => rest.to_vec(),
    }
}

/// ParseConfigFile()
pub fn parse_config_file(doc: &mut Doc, cfgfil: &[u8]) -> i32 {
    parse_config_file_enc(doc, cfgfil, b"ascii")
}

/// ParseConfigFileEnc(): open the file and parse its contents
pub fn parse_config_file_enc(doc: &mut Doc, file: &[u8], charenc: &[u8]) -> i32 {
    use std::os::unix::ffi::OsStrExt;
    let opterrs = doc.option_errors;
    let fname = expand_tilde(tmbstr::cstr(file));
    let path = std::path::Path::new(std::ffi::OsStr::from_bytes(&fname));
    let data = if path.is_dir() {
        None
    } else {
        std::fs::read(path).ok()
    };
    let enc = char_encoding_id(doc, charenc);
    let Some(data) = data.filter(|_| enc >= 0) else {
        message::report_file_error(doc, &fname, FILE_CANT_OPEN_CFG);
        return -1;
    };
    doc.config.cfg_in = Some(streamio::buffer_input(data, enc as u32));
    first_char(doc);
    let mut c = skip_white(doc);
    while c != END_OF_STREAM {
        // `// or # start a comment`
        if c != '/' as u32 && c != '#' as u32 {
            let mut name: Vec<u8> = Vec::new();
            while name.len() < 63 && c != '\n' as u32 && c != END_OF_STREAM && c != ':' as u32 {
                name.push(c as u8);
                c = advance_char(doc);
            }
            if c == ':' as u32 {
                let option = lookup_option(&name);
                advance_char(doc);
                match option {
                    Some(o) => {
                        if let Some(p) = o.parser {
                            p(doc, o);
                        }
                    }
                    None => message::report_unknown_option(doc, &name),
                }
            }
        }
        c = next_property(doc);
    }
    doc.config.cfg_in = None;
    if doc.option_errors > opterrs { 1 } else { 0 }
}

/// ParseConfigOption(): returns false if unknown option, missing parameter,
/// or option doesn't use parameter
pub fn parse_config_option(doc: &mut Doc, optnam: &[u8], optval: &[u8]) -> bool {
    match lookup_option(optnam) {
        None => {
            message::report_unknown_option(doc, optnam);
            false
        }
        Some(option) => parse_config_value(doc, option.id, optval),
    }
}

/// ParseConfigValue(): option id and value
pub fn parse_config_value(doc: &mut Doc, opt_id: OptId, optval: &[u8]) -> bool {
    parse_config_value_opt(doc, opt_id, Some(optval))
}

fn parse_config_value_opt(doc: &mut Doc, opt_id: OptId, optval: Option<&[u8]>) -> bool {
    let option = get_option(opt_id);
    let parser = match (option.parser, optval) {
        (Some(p), Some(_)) if (opt_id as usize) < N_TIDY_OPTIONS => p,
        _ => {
            message::report_bad_argument(doc, option.name.as_bytes());
            return false;
        }
    };
    // the buffer includes the terminating NUL (tmbstrlen + 1)
    let mut data = tmbstr::cstr(optval.unwrap_or_default()).to_vec();
    data.push(0);
    doc.config.cfg_in = Some(streamio::buffer_input(data, RAW));
    let c = get_c(doc);
    doc.config.c = c;
    let status = parser(doc, option);
    doc.config.cfg_in = None;
    status
}

/// AdjustCharEncoding(): ensure that char encodings are self consistent
pub fn adjust_char_encoding(doc: &mut Doc, encoding: i32) -> bool {
    let (inenc, outenc): (i32, i32) = match encoding as u32 {
        MACROMAN => (MACROMAN as i32, ASCII as i32),
        WIN1252 => (WIN1252 as i32, ASCII as i32),
        IBM858 => (IBM858 as i32, ASCII as i32),
        ASCII => (LATIN1 as i32, ASCII as i32),
        LATIN0 => (LATIN0 as i32, ASCII as i32),
        RAW | LATIN1 | UTF8 | ISO2022 | UTF16LE | UTF16BE | UTF16 | SHIFTJIS | BIG5 => {
            (encoding, encoding)
        }
        _ => (-1, -1),
    };
    if inenc >= 0 {
        set_option_int(doc, OptId::CharEncoding, encoding as u64);
        set_option_int(doc, OptId::InCharEncoding, inenc as u64);
        set_option_int(doc, OptId::OutCharEncoding, outenc as u64);
        return true;
    }
    false
}

/// AdjustConfig(): ensure that config is self consistent
pub fn adjust_config(doc: &mut Doc) {
    if doc.cfg_bool(OptId::EncloseBlockText) {
        set_option_bool(doc, OptId::EncloseBodyText, true);
    }
    if doc.cfg_auto_bool(OptId::IndentContent) == TIDY_NO_STATE {
        set_option_int(doc, OptId::IndentSpaces, 0);
    }
    if doc.cfg(OptId::WrapLen) == 0 {
        set_option_int(doc, OptId::WrapLen, 0x7FFF_FFFF);
    }
    if doc.cfg_bool(OptId::Word2000) {
        doc.config.defined_tags |= UserTagType::Inline as u32;
        tags::define_tag(doc, UserTagType::Inline, b"o:p");
    }
    if doc.cfg_bool(OptId::XmlTags) {
        set_option_bool(doc, OptId::XhtmlOut, false);
    }
    if doc.cfg_bool(OptId::XhtmlOut) {
        set_option_bool(doc, OptId::XmlOut, true);
        set_option_bool(doc, OptId::UpperCaseTags, false);
        set_option_int(doc, OptId::UpperCaseAttrs, 0);
    }
    if doc.cfg_bool(OptId::XmlTags) {
        set_option_bool(doc, OptId::XmlOut, true);
        set_option_bool(doc, OptId::XmlPIs, true);
    }
    let oe = doc.cfg(OptId::OutCharEncoding);
    if oe != ASCII
        && oe != UTF8
        && oe != UTF16
        && oe != UTF16BE
        && oe != UTF16LE
        && oe != RAW
        && doc.cfg_bool(OptId::XmlOut)
    {
        set_option_bool(doc, OptId::XmlDecl, true);
    }
    if doc.cfg_bool(OptId::XmlOut) {
        let enc = doc.cfg(OptId::OutCharEncoding);
        if enc == UTF16LE || enc == UTF16BE || enc == UTF16 {
            set_option_int(doc, OptId::OutputBOM, 1);
        }
        set_option_bool(doc, OptId::QuoteAmpersand, true);
        set_option_bool(doc, OptId::OmitOptionalTags, false);
    }
}

/// DeclareListItem(): keeps option values nicely formatted and coordinates
/// additions to the internal lists
pub fn declare_list_item(doc: &mut Doc, opt: &'static OptionImpl, name: &[u8]) {
    let prvval = doc.cfg_str(opt.id).map(|s| s.to_vec());
    let theval = match prvval {
        Some(mut p) => {
            p.extend_from_slice(b", ");
            p.extend_from_slice(name);
            p
        }
        None => name.to_vec(),
    };
    match opt.id {
        OptId::PriorityAttributes => crate::attrs::define_priority_attribute(doc, name),
        OptId::MuteReports => message::define_muted_message(doc, opt.id, name),
        OptId::InlineTags
        | OptId::BlockTags
        | OptId::EmptyTags
        | OptId::PreTags
        | OptId::CustomTags => tags::declare_user_tag(doc, opt.id, name),
        _ => {}
    }
    set_option_value(doc, opt.id, Some(&theval));
}

/// ParseList(): a space or comma separated list of items
pub fn parse_list(doc: &mut Doc, option: &'static OptionImpl) -> bool {
    let mut buf: Vec<u8> = Vec::new();
    let mut n_items = 0;
    set_option_value(doc, option.id, None);
    if doc.config.c == END_OF_STREAM {
        return true;
    }
    let mut c = skip_white(doc);
    loop {
        'body: {
            if c == ' ' as u32 || c == '\t' as u32 || c == ',' as u32 {
                c = advance_char(doc);
                break 'body;
            }
            if c == '\r' as u32 || c == '\n' as u32 {
                let c2 = advance_char(doc);
                if c == '\r' as u32 && c2 == '\n' as u32 {
                    c = advance_char(doc);
                } else {
                    c = c2;
                }
                if !lexer::is_white(c) {
                    streamio::unget_char(doc, InSel::Cfg, c);
                    streamio::unget_char(doc, InSel::Cfg, '\n' as u32);
                    // break out of the do-while
                    return finish_list(doc, option, &buf, n_items);
                }
            }
            while buf.len() < 1022 && c != END_OF_STREAM && !lexer::is_white(c) && c != ',' as u32 {
                buf.push(c as u8);
                c = advance_char(doc);
            }
            if buf.is_empty() {
                break 'body;
            }
            declare_list_item(doc, option, &buf);
            buf.clear();
            n_items += 1;
        }
        if c == END_OF_STREAM {
            break;
        }
    }
    finish_list(doc, option, &buf, n_items)
}

fn finish_list(doc: &mut Doc, option: &'static OptionImpl, buf: &[u8], n_items: u32) -> bool {
    if !buf.is_empty() {
        declare_list_item(doc, option, buf);
    }
    n_items > 0
}

/// ParseInt(): unsigned integers
pub fn parse_int(doc: &mut Doc, entry: &'static OptionImpl) -> bool {
    let mut number: u64 = 0;
    let mut digits = false;
    let mut c = skip_white(doc);
    while lexer::is_digit(c) {
        number = (c as u64 - '0' as u64).wrapping_add(number.wrapping_mul(10));
        digits = true;
        c = advance_char(doc);
    }
    if !digits {
        message::report_bad_argument(doc, entry.name.as_bytes());
    } else {
        set_option_int(doc, entry.id, number);
    }
    digits
}

/// ParseName(): a string excluding whitespace (unused)
pub fn parse_name(doc: &mut Doc, option: &'static OptionImpl) -> bool {
    let mut buf: Vec<u8> = Vec::new();
    let mut c = skip_white(doc);
    while buf.len() < 1022 && c != END_OF_STREAM && !lexer::is_white(c) {
        buf.push(c as u8);
        c = advance_char(doc);
    }
    if buf.is_empty() {
        message::report_bad_argument(doc, option.name.as_bytes());
    } else {
        set_option_value(doc, option.id, Some(&buf));
    }
    !buf.is_empty()
}

/// ParseCSS1Selector(): #508936 - CSS class naming for -clean option
pub fn parse_css1_selector(doc: &mut Doc, option: &'static OptionImpl) -> bool {
    if doc.config.c == END_OF_STREAM {
        set_option_value(doc, option.id, None);
        return true;
    }
    let mut buf: Vec<u8> = Vec::new();
    let mut c = skip_white(doc);
    while buf.len() < 254 && c != END_OF_STREAM && !lexer::is_white(c) {
        buf.push(c as u8);
        c = advance_char(doc);
    }
    if buf.is_empty() {
        return false;
    } else if !crate::attrs::is_css1_selector(&buf) {
        message::report_bad_argument(doc, option.name.as_bytes());
        return false;
    }
    set_option_value(doc, option.id, Some(&buf));
    true
}

/// GetPickListValue()
fn get_pick_list_value(value: &[u8], pick_list: &[PickListItem]) -> Option<u32> {
    for (ix, item) in pick_list.iter().enumerate() {
        for input in item.inputs {
            if tmbstr::tmbstrcasecmp(value, input.as_bytes()) == 0 {
                return Some(ix as u32);
            }
        }
    }
    None
}

/// GetParsePickListValue()
fn get_parse_pick_list_value(doc: &mut Doc, entry: &'static OptionImpl) -> Option<u32> {
    let mut c = skip_white(doc);
    let mut work: Vec<u8> = Vec::new();
    while c != END_OF_STREAM
        && work.len() < 16
        && !lexer::is_white(c)
        && c != '\r' as u32
        && c != '\n' as u32
    {
        work.push(c as u8);
        c = advance_char(doc);
    }
    match get_pick_list_value(&work, entry.pick_list.unwrap_or_default()) {
        Some(v) => Some(v),
        None => {
            message::report_bad_argument(doc, entry.name.as_bytes());
            None
        }
    }
}

/// ParsePickList()
pub fn parse_pick_list(doc: &mut Doc, entry: &'static OptionImpl) -> bool {
    if let Some(value) = get_parse_pick_list_value(doc, entry) {
        if entry.type_ == OptionType::Boolean {
            set_option_bool(doc, entry.id, value != 0);
        } else if entry.type_ == OptionType::Integer {
            set_option_int(doc, entry.id, value as u64);
        }
        return true;
    }
    message::report_bad_argument(doc, entry.name.as_bytes());
    false
}

/// ParseTabs(): Issue #108
pub fn parse_tabs(doc: &mut Doc, entry: &'static OptionImpl) -> bool {
    match get_parse_pick_list_value(doc, entry) {
        Some(flag) => {
            let tabs = flag != 0;
            set_option_bool(doc, entry.id, tabs);
            if tabs {
                set_option_int(doc, OptId::IndentSpaces, 1);
            }
            true
        }
        None => false,
    }
}

/// ParseString(): a string including whitespace; munges whitespace
/// sequences
pub fn parse_string(doc: &mut Doc, option: &'static OptionImpl) -> bool {
    let mut buf: Vec<u8> = Vec::new();
    let mut delim = 0u32;
    let mut waswhite = true;
    let mut c = skip_white(doc);
    if c == '"' as u32 || c == '\'' as u32 {
        delim = c;
        c = advance_char(doc);
    }
    while buf.len() < 8190 && c != END_OF_STREAM && c != '\r' as u32 && c != '\n' as u32 {
        if delim != 0 && c == delim {
            break;
        }
        if lexer::is_white(c) {
            if waswhite {
                c = advance_char(doc);
                continue;
            }
            c = ' ' as u32;
        } else {
            waswhite = false;
        }
        buf.push(c as u8);
        c = advance_char(doc);
    }
    set_option_value(doc, option.id, Some(&buf));
    true
}

/// ParseCharEnc()
pub fn parse_char_enc(doc: &mut Doc, option: &'static OptionImpl) -> bool {
    let mut buf: Vec<u8> = Vec::new();
    let mut c = skip_white(doc);
    while buf.len() < 62 && c != END_OF_STREAM && !lexer::is_white(c) {
        buf.push(lexer::to_lower(c) as u8);
        c = advance_char(doc);
    }
    let enc = char_encoding_id(doc, &buf);
    let valid = enc >= 0;
    if !valid {
        message::report_bad_argument(doc, option.name.as_bytes());
    } else {
        set_option_int(doc, option.id, enc as u64);
    }
    if valid && option.id == OptId::CharEncoding {
        adjust_char_encoding(doc, enc);
    }
    valid
}

/// CharEncodingId()
pub fn char_encoding_id(_doc: &mut Doc, charenc: &[u8]) -> i32 {
    streamio::get_char_encoding_from_opt_name(charenc)
}

/// CharEncodingName()
pub fn char_encoding_name(encoding: i32) -> &'static str {
    streamio::get_encoding_name_from_tidy_id(encoding as u32).unwrap_or("unknown")
}

/// CharEncodingOptName()
pub fn char_encoding_opt_name(encoding: i32) -> &'static str {
    streamio::get_encoding_opt_name_from_tidy_id(encoding as u32).unwrap_or("unknown")
}

/// ParseDocType(): html5 | omit | auto | strict | loose | <fpi>
pub fn parse_doc_type(doc: &mut Doc, option: &'static OptionImpl) -> bool {
    if doc.config.c == END_OF_STREAM {
        set_option_value(doc, option.id, None);
        return true;
    }
    let c = skip_white(doc);
    if c == '"' as u32 || c == '\'' as u32 || c == '-' as u32 || c == '+' as u32 {
        let status = parse_string(doc, option);
        if status {
            set_option_int(doc, OptId::DoctypeMode, TIDY_DOCTYPE_USER as u64);
        }
        return status;
    }
    match get_parse_pick_list_value(doc, option) {
        Some(value) => {
            set_option_int(doc, OptId::DoctypeMode, value as u64);
            true
        }
        None => {
            message::report_bad_argument(doc, option.name.as_bytes());
            false
        }
    }
}

/// ConfigDiffThanSnapshot()
pub fn config_diff_than_snapshot(doc: &mut Doc) -> bool {
    doc.config.value[..N_TIDY_OPTIONS]
        .iter()
        .zip(&doc.config.snapshot[..N_TIDY_OPTIONS])
        .any(|(a, b)| a.v != b.v)
}

/// ConfigDiffThanDefault()
pub fn config_diff_than_default(doc: &mut Doc) -> bool {
    OPTION_DEFS[1..N_TIDY_OPTIONS].iter().any(|option| {
        let val = &doc.config.value[option.id as usize];
        if option.type_ == OptionType::String {
            val.p.as_deref() != option.pdflt.map(|s| s.as_bytes())
        } else {
            val.v != option.dflt
        }
    })
}
