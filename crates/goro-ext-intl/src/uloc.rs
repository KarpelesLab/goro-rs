//! ICU's C locale ID API (uloc.cpp, uloc_tag.cpp, loclikely.cpp,
//! locdispnames.cpp) on ICU locale IDs ("de_DE@collation=phonebook",
//! "sr_Latn_RS", "en_US_POSIX") with ICU's exact parsing quirks:
//! uloc_getLanguage() / getScript() / getCountry() / getVariant(),
//! uloc_canonicalize(), uloc_getName(), keywords, uloc_forLanguageTag(),
//! likely subtags and the display names.
//!
//! IDs are handled as bytes: ICU parses them as `char*` and keeps whatever
//! it does not recognise.

use crate::error::{U_ILLEGAL_ARGUMENT_ERROR, U_INTERNAL_PROGRAM_ERROR, U_INVALID_FORMAT_ERROR};

#[path = "uloc_data.rs"]
pub mod data;

/// ULOC_FULLNAME_CAPACITY
pub const FULLNAME_CAPACITY: usize = 157;
/// ULOC_KEYWORD_BUFFER_LEN
const KEYWORD_BUFFER_LEN: usize = 25;
/// ULOC_MAX_NO_KEYWORDS
const MAX_NO_KEYWORDS: usize = 25;

fn is_sep(c: u8) -> bool {
    c == b'_' || c == b'-'
}

/// _isTerminator(): '.' (POSIX codepage) and '@' (keywords) end the ID
/// proper.
fn is_term(c: u8) -> bool {
    c == b'.' || c == b'@'
}

fn at(s: &[u8], i: usize) -> u8 {
    s.get(i).copied().unwrap_or(0)
}

/// _isIDPrefix(): "i-", "x-" (any separator, any case).
fn is_id_prefix(s: &[u8]) -> bool {
    matches!(at(s, 0), b'x' | b'X' | b'i' | b'I') && is_sep(at(s, 1))
}

/// _isBCP47Extension(): "-t-", "-u-", "-x-".
fn is_bcp47_ext(s: &[u8]) -> bool {
    s.len() >= 3 && s[0] == b'-' && matches!(s[1], b't' | b'T' | b'u' | b'U' | b'x' | b'X') && s[2] == b'-'
}

/// The number of "va-posix" pairs in the BCP 47 extensions `s`
/// ("-u-ca-gregory-va-posix..."), which ICU reads as (key, type) pairs of
/// subtags after the first singleton, whatever the singletons between.
fn va_posix_count(s: &[u8]) -> usize {
    let end = s.iter().position(|&c| c == b'@').unwrap_or(s.len());
    let subs: Vec<&[u8]> = s[..end].split(|&c| c == b'-').skip(1).collect();
    // ICU looks at the subtags pairwise after the singleton.
    (1..subs.len())
        .step_by(2)
        .filter(|&i| {
            subs[i].eq_ignore_ascii_case(b"va")
                && subs
                    .get(i + 1)
                    .is_some_and(|x| x.eq_ignore_ascii_case(b"posix"))
        })
        .count()
}

fn lower(s: &[u8]) -> Vec<u8> {
    s.to_ascii_lowercase()
}

fn upper(s: &[u8]) -> Vec<u8> {
    s.to_ascii_uppercase()
}

fn s(v: &[u8]) -> String {
    String::from_utf8_lossy(v).into_owned()
}

/// getShortestSubtagLength()
fn shortest_subtag_len(id: &[u8]) -> usize {
    let mut length = id.len();
    let mut tmp = 0;
    let mut reset = true;
    for &c in id {
        if c != b'_' && c != b'-' {
            if reset {
                tmp = 0;
                reset = false;
            }
            tmp += 1;
        } else {
            if tmp != 0 && tmp < length {
                length = tmp;
            }
            reset = true;
        }
    }
    length
}

/// _hasBCP47Extension(): no '@' and a singleton subtag.
pub fn has_bcp47_extension(id: &[u8]) -> bool {
    !id.contains(&b'@') && shortest_subtag_len(id) == 1
}

/// The subtags of an ICU locale ID as ulocimp_getSubtags() splits them.
#[derive(Debug, Default, Clone)]
pub struct Subtags {
    pub language: Vec<u8>,
    pub script: Vec<u8>,
    pub region: Vec<u8>,
    pub variant: Vec<u8>,
    /// Where parsing stopped (the '.', '@', an extension or the end).
    pub end: usize,
}

/// _getLanguage(): (language, bytes consumed).
fn get_language_part(id: &[u8]) -> Result<(Vec<u8>, usize), i32> {
    let mut skip = 0;
    let mut s = id;
    if s.len() == 4 && s.eq_ignore_ascii_case(b"root") {
        skip = 4;
        s = &s[4..];
    } else if s.len() >= 3 && s[..3].eq_ignore_ascii_case(b"und") && (s.len() == 3 || matches!(s[3], b'-' | b'_' | b'@')) {
        skip = 3;
        s = &s[3..];
    }
    const MAXLEN: usize = 11;
    let mut len = if is_id_prefix(s) { 2 } else { 0 };
    while len < s.len() && !is_term(s[len]) && !is_sep(s[len]) {
        if len == MAXLEN {
            return Err(U_ILLEGAL_ARGUMENT_ERROR);
        }
        len += 1;
    }
    let mut out = lower(&s[..len]);
    if len >= 2 && is_sep(s[1]) {
        out[1] = b'-';
    }
    if len == 3 {
        if let Ok(i) = data::LANG3.binary_search_by(|(k, _)| k.as_bytes().cmp(&out[..])) {
            out = data::LANG3[i].1.as_bytes().to_vec();
        }
    }
    Ok((out, skip + len))
}

/// _getScript(): the length of a script subtag at the start of `s` (0 if
/// none).
fn script_len(s: &[u8]) -> usize {
    let mut len = 0;
    while len < s.len() && !is_term(s[len]) && !is_sep(s[len]) && s[len].is_ascii_alphabetic() {
        len += 1;
    }
    if len == 4 { 4 } else { 0 }
}

/// _getRegion(): (region, length) at the start of `s`.
fn region_part(s: &[u8]) -> Option<(Vec<u8>, usize)> {
    let mut len = 0;
    while len < s.len() && !is_term(s[len]) && !is_sep(s[len]) {
        if len == 3 {
            return None;
        }
        len += 1;
    }
    if len < 2 {
        return None;
    }
    let mut out = upper(&s[..len]);
    if len == 3 {
        if let Ok(i) = data::REGION3.binary_search_by(|(k, _)| k.as_bytes().cmp(&out[..])) {
            out = data::REGION3[i].1.as_bytes().to_vec();
        }
    }
    Some((out, len))
}

/// ulocimp_getSubtags()
pub fn get_subtags(id: &[u8]) -> Result<Subtags, i32> {
    let mut t = Subtags::default();
    if id.is_empty() {
        return Ok(t);
    }
    let (lang, n) = get_language_part(id)?;
    t.language = lang;
    let mut pos = n;
    t.end = pos;
    let rest = |p: usize| &id[p..];
    // script
    if pos < id.len() && is_sep(id[pos]) {
        let l = script_len(rest(pos + 1));
        if l > 0 {
            let sc = &id[pos + 1..pos + 1 + l];
            let mut v = lower(sc);
            v[0] = v[0].to_ascii_uppercase();
            t.script = v;
            pos += 1 + l;
            t.end = pos;
        }
    }
    // region
    let mut has_region = false;
    if pos < id.len() && is_sep(id[pos]) {
        if let Some((r, l)) = region_part(rest(pos + 1)) {
            t.region = r;
            has_region = true;
            pos += 1 + l;
            t.end = pos;
        }
    }
    // variant
    if pos < id.len() && is_sep(id[pos]) && !is_bcp47_ext(rest(pos)) {
        let mut p = pos;
        if !has_region && p + 1 < id.len() && is_sep(id[p + 1]) {
            p += 1;
        }
        // _getVariant(): '_'-separated segments up to a terminator or an
        // extension; a separator ending the ID belongs to the variant.
        let s = &id[p + 1..];
        let mut i = 0;
        let mut v = Vec::new();
        let mut first = true;
        loop {
            let sub = &s[i..];
            let next = sub
                .iter()
                .position(|&c| is_sep(c) || is_term(c));
            let finished = next.is_none_or(|n| n + 1 == sub.len());
            let limit = if finished { sub.len() } else { next.unwrap_or(0) };
            if !first {
                v.push(b'_');
            }
            first = false;
            v.extend(sub[..limit].iter().map(|c| c.to_ascii_uppercase()));
            if finished {
                i += limit;
                break;
            }
            let n = next.unwrap_or(0);
            i += n;
            if is_term(sub[n]) || is_bcp47_ext(&sub[n..]) {
                break;
            }
            i += 1;
        }
        if !v.is_empty() {
            t.variant = v;
            pos = p + 1 + i;
            t.end = pos;
        }
    }
    // A "va-posix" keyword of the BCP 47 extensions that follow reports
    // the POSIX variant.
    if pos < id.len() && is_bcp47_ext(rest(pos)) {
        let sep = !t.variant.is_empty();
        for _ in 0..va_posix_count(rest(pos)) {
            if sep {
                t.variant.push(b'_');
            }
            t.variant.extend_from_slice(b"POSIX");
        }
    }
    Ok(t)
}

/// uloc_getLanguage()
pub fn get_language(id: &[u8]) -> Result<String, i32> {
    Ok(s(&get_subtags(id)?.language))
}

/// uloc_getScript()
pub fn get_script(id: &[u8]) -> Result<String, i32> {
    Ok(s(&get_subtags(id)?.script))
}

/// uloc_getCountry()
pub fn get_country(id: &[u8]) -> Result<String, i32> {
    Ok(s(&get_subtags(id)?.region))
}

/// uloc_getVariant()
pub fn get_variant(id: &[u8]) -> Result<String, i32> {
    Ok(s(&get_subtags(id)?.variant))
}

/// _getVariant() after a POSIX '@': "@euro" → "EURO".
fn posix_at_variant(s: &[u8], out: &mut Vec<u8>, need_sep: bool) {
    let mut first = need_sep;
    for &c in s {
        if is_term(c) {
            break;
        }
        if first {
            out.push(b'_');
            first = false;
        }
        let c = c.to_ascii_uppercase();
        out.push(if c == b'-' || c == b',' { b'_' } else { c });
    }
}

/// ulocimp_getKeywords(): the sorted "key=value;..." (or only the keys)
/// of the keyword list after '@'.
fn get_keywords(s: &[u8], values_too: bool) -> Result<Vec<(Vec<u8>, Vec<u8>)>, i32> {
    let mut list: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
    let mut pos = 0;
    loop {
        while at(s, pos) == b' ' {
            pos += 1;
        }
        if pos >= s.len() {
            break;
        }
        if list.len() == MAX_NO_KEYWORDS {
            return Err(U_INTERNAL_PROGRAM_ERROR);
        }
        let rest = &s[pos..];
        let eq = rest.iter().position(|&c| c == b'=');
        let semi = rest.iter().position(|&c| c == b';');
        let Some(eq) = eq else {
            return Err(U_INVALID_FORMAT_ERROR);
        };
        if semi.is_some_and(|sc| sc < eq) {
            return Err(U_INVALID_FORMAT_ERROR);
        }
        if eq == 0 {
            return Err(U_INVALID_FORMAT_ERROR);
        }
        if eq >= KEYWORD_BUFFER_LEN {
            return Err(U_INTERNAL_PROGRAM_ERROR);
        }
        let key: Vec<u8> = rest[..eq]
            .iter()
            .filter(|&&c| c != b' ')
            .map(|c| c.to_ascii_lowercase())
            .collect();
        if key.is_empty() {
            return Err(U_INVALID_FORMAT_ERROR);
        }
        let mut vs = eq + 1;
        while at(rest, vs) == b' ' {
            vs += 1;
        }
        let vend_raw = semi.unwrap_or(rest.len());
        if vs >= vend_raw {
            return Err(U_INVALID_FORMAT_ERROR);
        }
        let mut ve = vend_raw;
        while ve > vs && rest[ve - 1] == b' ' {
            ve -= 1;
        }
        let value = rest[vs..ve].to_vec();
        if !list.iter().any(|(k, _)| *k == key) {
            list.push((key, value));
        }
        match semi {
            Some(sc) => pos += sc + 1,
            None => break,
        }
    }
    list.sort_by(|a, b| a.0.cmp(&b.0));
    if !values_too {
        for e in list.iter_mut() {
            e.1.clear();
        }
    }
    Ok(list)
}

fn join_keywords(list: &[(Vec<u8>, Vec<u8>)]) -> Vec<u8> {
    let mut out = Vec::new();
    for (i, (k, v)) in list.iter().enumerate() {
        if i > 0 {
            out.push(b';');
        }
        out.extend_from_slice(k);
        out.push(b'=');
        out.extend_from_slice(v);
    }
    out
}

/// CANONICALIZE_MAP
const CANONICALIZE_MAP: &[(&str, &str)] = &[
    ("art__LOJBAN", "jbo"),
    ("hy__AREVELA", "hy"),
    ("hy__AREVMDA", "hyw"),
    ("zh__GUOYU", "zh"),
    ("zh__HAKKA", "hak"),
    ("zh__XIANG", "hsn"),
    ("zh_GAN", "gan"),
    ("zh_MIN_NAN", "nan"),
    ("zh_WUU", "wuu"),
    ("zh_YUE", "yue"),
];

const OPT_CANONICALIZE: u32 = 1;
const OPT_STRIP_KEYWORDS: u32 = 2;

/// _canonicalize(): uloc_getName() (options 0), uloc_getBaseName()
/// (OPT_STRIP_KEYWORDS) and uloc_canonicalize() (OPT_CANONICALIZE).
fn canonicalize_impl(id: &[u8], options: u32) -> Result<Vec<u8>, i32> {
    let converted;
    let mut tmp: &[u8] = id;
    if has_bcp47_extension(id) {
        let mut hy = id.to_vec();
        if id.len() >= 2 && id.contains(&b'_') && !is_sep(id[1]) {
            for c in hy.iter_mut() {
                if *c == b'_' {
                    *c = b'-';
                }
            }
        }
        let t = for_language_tag(&hy).0;
        converted = if t.is_empty() { hy } else { t };
        tmp = &converted;
    }
    let orig = tmp;
    let st = get_subtags(tmp)?;
    let mut tag = st.language.clone();
    let mut field_count = 0;
    if tag.len() == 9 && orig.len() >= 9 && &orig[..9] == b"i-default" {
        tag = crate::common::icu_default_locale().into_bytes();
    } else {
        if !st.script.is_empty() {
            field_count += 1;
            tag.push(b'_');
            tag.extend_from_slice(&st.script);
        }
        if !st.region.is_empty() {
            field_count += 1;
            tag.push(b'_');
            tag.extend_from_slice(&st.region);
        }
        if !st.variant.is_empty() {
            field_count += 1;
            if st.region.is_empty() {
                tag.push(b'_');
            }
            tag.push(b'_');
            tag.extend_from_slice(&st.variant);
        }
    }
    let mut rest: &[u8] = &tmp[st.end..];
    if options & OPT_CANONICALIZE == 0 && rest.first() == Some(&b'.') {
        tag.push(b'.');
        rest = &rest[1..];
        let len = rest.iter().position(|&c| c == b'@').unwrap_or(rest.len());
        if len > 64 {
            return Err(U_ILLEGAL_ARGUMENT_ERROR);
        }
        tag.extend_from_slice(&rest[..len]);
        rest = &rest[len..];
    }
    // after this, rest starts at '@' or is empty
    let mut keyword_assign = None;
    let mut separator = None;
    match rest.iter().position(|&c| c == b'@') {
        Some(p) => {
            rest = &rest[p..];
            keyword_assign = rest.iter().position(|&c| c == b'=');
            separator = rest.iter().position(|&c| c == b';');
        }
        None => rest = &[],
    }
    if options & OPT_CANONICALIZE == 0 && !rest.is_empty() && keyword_assign.is_none() {
        tag.extend_from_slice(rest);
        rest = &[];
    }
    if options & OPT_CANONICALIZE != 0 {
        if !rest.is_empty() && keyword_assign.is_none() {
            if field_count < 2 || (field_count < 3 && !st.script.is_empty()) {
                loop {
                    tag.push(b'_');
                    field_count += 1;
                    if field_count >= 2 {
                        break;
                    }
                }
            }
            posix_at_variant(&rest[1..], &mut tag, !st.variant.is_empty());
        }
        for (k, v) in CANONICALIZE_MAP {
            if tag == k.as_bytes() {
                tag = v.as_bytes().to_vec();
                break;
            }
        }
    }
    if options & OPT_STRIP_KEYWORDS == 0 {
        if let Some(ka) = keyword_assign {
            if !rest.is_empty() && separator.is_none_or(|sp| sp > ka) {
                let kw = get_keywords(&rest[1..], true)?;
                tag.push(b'@');
                tag.extend_from_slice(&join_keywords(&kw));
            }
        }
    }
    Ok(tag)
}

/// uloc_canonicalize()
pub fn canonicalize(id: &[u8]) -> Result<String, i32> {
    canonicalize_impl(id, OPT_CANONICALIZE).map(|v| s(&v))
}

/// uloc_getName()
pub fn get_name(id: &[u8]) -> Result<String, i32> {
    canonicalize_impl(id, 0).map(|v| s(&v))
}

/// uloc_getBaseName()
pub fn get_base_name(id: &[u8]) -> Result<String, i32> {
    canonicalize_impl(id, OPT_STRIP_KEYWORDS).map(|v| s(&v))
}

/// The ID keyword functions read: a BCP 47 tag converted to an ICU ID.
fn keyword_source(id: &[u8]) -> Vec<u8> {
    if has_bcp47_extension(id) {
        let t = for_language_tag(id).0;
        if !t.is_empty() {
            return t;
        }
    }
    id.to_vec()
}

/// uloc_openKeywords(): the keyword names, sorted (empty when none).
/// None (no enumeration) when the ID has no '@'.
pub fn keyword_names(id: &[u8]) -> Result<Option<Vec<String>>, i32> {
    let src = keyword_source(id);
    let st = get_subtags(&src)?;
    let rest = &src[st.end..];
    match rest.iter().position(|&c| c == b'@') {
        Some(p) => Ok(Some(
            get_keywords(&rest[p + 1..], false)?
                .into_iter()
                .map(|(k, _)| s(&k))
                .collect(),
        )),
        None => Ok(None),
    }
}

fn ok_value_char(c: u8) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-' | b'+' | b'/')
}

/// uloc_getKeywordValue(): None when absent.
pub fn keyword_value(id: &[u8], keyword: &str) -> Result<Option<String>, i32> {
    if keyword.is_empty() {
        return Err(U_ILLEGAL_ARGUMENT_ERROR);
    }
    let src = keyword_source(id);
    let Some(start) = src.iter().position(|&c| c == b'@') else {
        return Ok(None);
    };
    // locale_canonKeywordName()
    if keyword.len() >= KEYWORD_BUFFER_LEN || !keyword.bytes().all(|c| c.is_ascii_alphanumeric()) {
        return Err(U_ILLEGAL_ARGUMENT_ERROR);
    }
    let want = keyword.to_ascii_lowercase();
    let mut pos = start;
    loop {
        pos += 1; // skip '@' or ';'
        let rest = &src[pos..];
        let Some(eq) = rest.iter().position(|&c| c == b'=') else {
            return Err(U_ILLEGAL_ARGUMENT_ERROR);
        };
        let mut ks = 0;
        while at(rest, ks) == b' ' {
            ks += 1;
        }
        let mut ke = eq;
        while ke > ks && rest[ke - 1] == b' ' {
            ke -= 1;
        }
        if ks == ke {
            return Err(U_ILLEGAL_ARGUMENT_ERROR);
        }
        let mut name = Vec::new();
        for &c in &rest[ks..ke] {
            if !c.is_ascii_alphanumeric() {
                return Err(U_ILLEGAL_ARGUMENT_ERROR);
            }
            name.push(c.to_ascii_lowercase());
        }
        let semi = rest[eq..].iter().position(|&c| c == b';').map(|p| p + eq);
        if name == want.as_bytes() {
            let mut vs = eq + 1;
            while at(rest, vs) == b' ' {
                vs += 1;
            }
            let mut ve = semi.unwrap_or(rest.len());
            while ve > vs && rest[ve - 1] == b' ' {
                ve -= 1;
            }
            if vs >= ve {
                return Err(U_ILLEGAL_ARGUMENT_ERROR);
            }
            let v = &rest[vs..ve];
            if !v.iter().all(|&c| ok_value_char(c)) {
                return Err(U_ILLEGAL_ARGUMENT_ERROR);
            }
            return Ok(Some(s(v)));
        }
        match semi {
            Some(sc) => pos += sc,
            None => return Ok(None),
        }
    }
}

// ---------------------------------------------------------------------------
// uloc_tag.cpp: uloc_forLanguageTag()

/// Legacy (grandfathered) tags and their replacements.
const LEGACY: &[(&str, &str)] = &[
    ("art-lojban", "jbo"),
    ("en-gb-oed", "en-gb-oxendict"),
    ("i-ami", "ami"),
    ("i-bnn", "bnn"),
    ("i-hak", "hak"),
    ("i-klingon", "tlh"),
    ("i-lux", "lb"),
    ("i-navajo", "nv"),
    ("i-pwn", "pwn"),
    ("i-tao", "tao"),
    ("i-tay", "tay"),
    ("i-tsu", "tsu"),
    ("no-bok", "nb"),
    ("no-nyn", "nn"),
    ("sgn-be-fr", "sfb"),
    ("sgn-be-nl", "vgt"),
    ("sgn-ch-de", "sgg"),
    ("zh-guoyu", "cmn"),
    ("zh-hakka", "hak"),
    ("zh-min-nan", "nan"),
    ("zh-xiang", "hsn"),
    ("cel-gaulish", "xtg-x-cel-gaulish"),
    ("i-default", "en-x-i-default"),
    ("i-enochian", "und-x-i-enochian"),
    ("i-mingo", "see-x-i-mingo"),
    ("zh-min", "nan-x-zh-min"),
];

/// Redundant tags with a preferred value.
const REDUNDANT: &[(&str, &str)] = &[
    ("sgn-br", "bzs"),
    ("sgn-co", "csn"),
    ("sgn-de", "gsg"),
    ("sgn-dk", "dsl"),
    ("sgn-es", "ssp"),
    ("sgn-fr", "fsl"),
    ("sgn-gb", "bfi"),
    ("sgn-gr", "gss"),
    ("sgn-ie", "isg"),
    ("sgn-it", "ise"),
    ("sgn-jp", "jsl"),
    ("sgn-mx", "mfs"),
    ("sgn-ni", "ncs"),
    ("sgn-nl", "dse"),
    ("sgn-no", "nsl"),
    ("sgn-pt", "psr"),
    ("sgn-se", "swl"),
    ("sgn-us", "ase"),
    ("sgn-za", "sfs"),
    ("zh-cmn", "cmn"),
    ("zh-cmn-hans", "cmn-hans"),
    ("zh-cmn-hant", "cmn-hant"),
    ("zh-gan", "gan"),
    ("zh-wuu", "wuu"),
    ("zh-yue", "yue"),
];

fn all(s: &[u8], f: fn(&u8) -> bool) -> bool {
    s.iter().all(f)
}

fn is_language_subtag(s: &[u8]) -> bool {
    (2..=8).contains(&s.len()) && all(s, u8::is_ascii_alphabetic)
}

fn is_extlang_subtag(s: &[u8]) -> bool {
    s.len() == 3 && all(s, u8::is_ascii_alphabetic)
}

fn is_script_subtag(s: &[u8]) -> bool {
    s.len() == 4 && all(s, u8::is_ascii_alphabetic)
}

fn is_region_subtag(s: &[u8]) -> bool {
    (s.len() == 2 && all(s, u8::is_ascii_alphabetic)) || (s.len() == 3 && all(s, u8::is_ascii_digit))
}

fn is_variant_subtag(s: &[u8]) -> bool {
    ((5..=8).contains(&s.len()) && all(s, u8::is_ascii_alphanumeric))
        || (s.len() == 4 && s[0].is_ascii_digit() && all(s, u8::is_ascii_alphanumeric))
}

fn is_privateuse_variant_subtag(s: &[u8]) -> bool {
    (1..=8).contains(&s.len()) && all(s, u8::is_ascii_alphanumeric)
}

fn is_extension_singleton(s: &[u8]) -> bool {
    s.len() == 1 && s[0].is_ascii_alphanumeric() && !s[0].eq_ignore_ascii_case(&b'x')
}

fn is_extension_subtag(s: &[u8]) -> bool {
    (2..=8).contains(&s.len()) && all(s, u8::is_ascii_alphanumeric)
}

fn is_privateuse_value_subtag(s: &[u8]) -> bool {
    (1..=8).contains(&s.len()) && all(s, u8::is_ascii_alphanumeric)
}

/// _isLDMLKey(): a -u- key ("ca", "kn", "1a").
fn is_ldml_key(s: &[u8]) -> bool {
    s.len() == 2 && s[0].is_ascii_alphanumeric() && s[1].is_ascii_alphabetic()
}

/// A parsed BCP 47 language tag (ULanguageTag).
#[derive(Debug, Default, Clone)]
pub struct LanguageTag {
    pub language: String,
    pub extlang: Vec<String>,
    pub script: String,
    pub region: String,
    pub variants: Vec<String>,
    /// (singleton, value) sorted by singleton.
    pub extensions: Vec<(u8, String)>,
    pub privateuse: String,
}

/// ultag_parse(): the tag and the length parsed.
pub fn parse_tag(tag: &[u8]) -> (LanguageTag, usize) {
    let mut t = LanguageTag::default();
    if tag.len() < 2 {
        return (t, 0);
    }
    let lower_tag = lower(tag);
    let mut buf: Vec<u8> = tag.to_vec();
    let mut delta: isize = 0;
    let mut legacy_len = 0;
    for (l, r) in LEGACY {
        let lb = l.as_bytes();
        if lower_tag.len() < lb.len() || (lower_tag.len() > lb.len() && lower_tag[lb.len()] != b'-') {
            continue;
        }
        if &lower_tag[..lb.len()] == lb {
            legacy_len = lb.len();
            let mut nb = r.as_bytes().to_vec();
            nb.extend_from_slice(&tag[lb.len()..]);
            delta = lb.len() as isize - r.len() as isize;
            buf = nb;
            break;
        }
    }
    if legacy_len == 0 {
        for (l, r) in REDUNDANT {
            let lb = l.as_bytes();
            if lower_tag.len() >= lb.len()
                && &lower_tag[..lb.len()] == lb
                && (lower_tag.len() == lb.len() || lower_tag[lb.len()] == b'-')
            {
                let mut nb = r.as_bytes().to_vec();
                nb.extend_from_slice(&tag[lb.len()..]);
                delta = lb.len() as isize - r.len() as isize;
                buf = nb;
                break;
            }
        }
    }
    const LANG: u32 = 1;
    const EXTL: u32 = 2;
    const SCRT: u32 = 4;
    const REGN: u32 = 8;
    const VART: u32 = 16;
    const EXTS: u32 = 32;
    const EXTV: u32 = 64;
    const PRIV: u32 = 128;
    let subtags: Vec<(usize, &[u8])> = {
        let mut v = Vec::new();
        let mut start = 0;
        for (i, &c) in buf.iter().enumerate() {
            if c == b'-' {
                v.push((start, &buf[start..i]));
                start = i + 1;
            }
        }
        v.push((start, &buf[start..]));
        v
    };
    let mut next = LANG | PRIV;
    let mut last_good = 0usize;
    let mut ext: Option<(u8, Vec<&[u8]>, usize)> = None;
    let mut privateuse_var = false;
    let mut i = 0;
    // finish the pending extension: false to stop parsing
    fn finish(t: &mut LanguageTag, ext: &mut Option<(u8, Vec<&[u8]>, usize)>, last_good: &mut usize) -> bool {
        if let Some((k, vals, end)) = ext.take() {
            if vals.is_empty() {
                return false;
            }
            if t.extensions.iter().any(|(kk, _)| *kk == k) {
                return false;
            }
            let v: Vec<String> = vals.iter().map(|x| String::from_utf8_lossy(&lower(x)).into_owned()).collect();
            let pos = t.extensions.partition_point(|(kk, _)| *kk < k);
            t.extensions.insert(pos, (k, v.join("-")));
            *last_good = end;
        }
        true
    }
    'outer: while i < subtags.len() {
        let (off, sub) = subtags[i];
        let end = off + sub.len();
        i += 1;
        if next & LANG != 0 && is_language_subtag(sub) {
            t.language = s(&lower(sub));
            last_good = end;
            // extended language subtags only follow a 2-3 letter language
            next = if sub.len() <= 3 {
                EXTL | SCRT | REGN | VART | EXTS | PRIV
            } else {
                SCRT | REGN | VART | EXTS | PRIV
            };
            continue;
        }
        if next & EXTL != 0 && is_extlang_subtag(sub) {
            t.extlang.push(s(&lower(sub)));
            last_good = end;
            next = if t.extlang.len() < 3 {
                EXTL | SCRT | REGN | VART | EXTS | PRIV
            } else {
                SCRT | REGN | VART | EXTS | PRIV
            };
            continue;
        }
        if next & SCRT != 0 && is_script_subtag(sub) {
            let mut v = lower(sub);
            v[0] = v[0].to_ascii_uppercase();
            t.script = s(&v);
            last_good = end;
            next = REGN | VART | EXTS | PRIV;
            continue;
        }
        if next & REGN != 0 && is_region_subtag(sub) {
            t.region = s(&upper(sub));
            last_good = end;
            next = VART | EXTS | PRIV;
            continue;
        }
        if next & VART != 0 && (is_variant_subtag(sub) || (privateuse_var && is_privateuse_variant_subtag(sub))) {
            let v = s(&upper(sub));
            if t.variants.contains(&v) {
                break;
            }
            let pos = t.variants.partition_point(|x| *x < v);
            t.variants.insert(pos, v);
            last_good = end;
            next = VART | EXTS | PRIV;
            continue;
        }
        if next & EXTS != 0 && is_extension_singleton(sub) {
            if ext.is_some() && !finish(&mut t, &mut ext, &mut last_good) {
                break;
            }
            ext = Some((sub[0].to_ascii_lowercase(), Vec::new(), end));
            next = EXTV;
            continue;
        }
        if next & EXTV != 0 && is_extension_subtag(sub) {
            if let Some((_, vals, e)) = ext.as_mut() {
                vals.push(sub);
                *e = end;
            }
            next = EXTS | EXTV | PRIV;
            continue;
        }
        if next & PRIV != 0 && sub.len() == 1 && sub[0].eq_ignore_ascii_case(&b'x') {
            if ext.is_some() && !finish(&mut t, &mut ext, &mut last_good) {
                break;
            }
            if i >= subtags.len() {
                break;
            }
            let mut vals: Vec<&[u8]> = Vec::new();
            while i < subtags.len() {
                let (o2, s2) = subtags[i];
                i += 1;
                if s2 == b"lvariant" {
                    next = VART;
                    privateuse_var = true;
                    continue 'outer;
                } else if is_privateuse_value_subtag(s2) {
                    vals.push(s2);
                    last_good = o2 + s2.len();
                } else {
                    break;
                }
            }
            if !vals.is_empty() {
                let v: Vec<String> = vals.iter().map(|x| s(&lower(x))).collect();
                t.privateuse = v.join("-");
            }
            break;
        }
        break;
    }
    if ext.is_some() {
        finish(&mut t, &mut ext, &mut last_good);
    }
    let parsed = if legacy_len > 0 {
        legacy_len
    } else {
        (last_good as isize + delta).max(0) as usize
    };
    (t, parsed)
}

/// BCP 47 -u- keys and their legacy (ICU keyword) names.
const KEY_MAP: &[(&str, &str)] = &[
    ("ca", "calendar"),
    ("co", "collation"),
    ("cu", "currency"),
    ("hc", "hours"),
    ("ka", "colalternate"),
    ("kb", "colbackwards"),
    ("kc", "colcaselevel"),
    ("kf", "colcasefirst"),
    ("kh", "colhiraganaquaternary"),
    ("kk", "colnormalization"),
    ("kn", "colnumeric"),
    ("kr", "colreorder"),
    ("ks", "colstrength"),
    ("ms", "measure"),
    ("nu", "numbers"),
    ("tz", "timezone"),
    ("vt", "variabletop"),
];

/// ulocimp_toLegacyKey()
pub fn to_legacy_key(key: &str) -> String {
    let k = key.to_ascii_lowercase();
    KEY_MAP
        .iter()
        .find(|(b, _)| *b == k)
        .map(|(_, l)| l.to_string())
        .unwrap_or(k)
}

/// ulocimp_toBcpKey()
pub fn to_bcp_key(key: &str) -> String {
    let k = key.to_ascii_lowercase();
    KEY_MAP
        .iter()
        .find(|(_, l)| *l == k)
        .map(|(b, _)| b.to_string())
        .unwrap_or(k)
}

/// (legacy key, BCP 47 type, legacy type)
const TYPE_MAP: &[(&str, &str, &str)] = &[
    ("calendar", "ethioaa", "ethiopic-amete-alem"),
    ("calendar", "gregory", "gregorian"),
    ("calendar", "islamicc", "islamic-civil"),
    ("colalternate", "noignore", "non-ignorable"),
    ("colstrength", "identic", "identical"),
    ("colstrength", "level1", "primary"),
    ("colstrength", "level2", "secondary"),
    ("colstrength", "level3", "tertiary"),
    ("colstrength", "level4", "quaternary"),
    ("collation", "dict", "dictionary"),
    ("collation", "gb2312", "gb2312han"),
    ("collation", "phonebk", "phonebook"),
    ("collation", "trad", "traditional"),
    ("measure", "uksystem", "imperial"),
];

const BOOL_KEYS: &[&str] = &[
    "colbackwards",
    "colcaselevel",
    "colcasefirst",
    "colhiraganaquaternary",
    "colnormalization",
    "colnumeric",
];

/// ulocimp_toLegacyType()
pub fn to_legacy_type(legacy_key: &str, ty: &str) -> String {
    let t = ty.to_ascii_lowercase();
    if BOOL_KEYS.contains(&legacy_key) {
        match t.as_str() {
            "true" => return "yes".into(),
            "false" => return "no".into(),
            _ => {}
        }
    }
    if legacy_key == "timezone" {
        if let Ok(i) = data::TIME_ZONES.binary_search_by(|(b, _)| (*b).cmp(t.as_str())) {
            return data::TIME_ZONES[i].1.to_string();
        }
    }
    TYPE_MAP
        .iter()
        .find(|(k, b, _)| *k == legacy_key && *b == t)
        .map(|(_, _, l)| l.to_string())
        .unwrap_or(t)
}

/// ulocimp_toBcpType()
pub fn to_bcp_type(legacy_key: &str, ty: &str) -> String {
    let t = ty.to_ascii_lowercase();
    if BOOL_KEYS.contains(&legacy_key) {
        match t.as_str() {
            "yes" => return "true".into(),
            "no" => return "false".into(),
            _ => {}
        }
    }
    if legacy_key == "timezone" {
        if let Some((b, _)) = data::TIME_ZONES.iter().find(|(_, l)| l.eq_ignore_ascii_case(ty)) {
            return b.to_string();
        }
    }
    TYPE_MAP
        .iter()
        .find(|(k, _, l)| *k == legacy_key && l.eq_ignore_ascii_case(&t))
        .map(|(_, b, _)| b.to_string())
        .unwrap_or(t)
}

/// _appendLDMLExtensionAsKeywords(): the keywords of a -u- extension
/// value; sets `posix` for -u-va-posix when there is no variant.
fn ldml_ext_keywords(value: &str, kw: &mut Vec<(String, String)>, variant_exists: bool, posix: &mut bool) {
    let parts: Vec<&str> = value.split('-').collect();
    let mut i = 0;
    let mut attrs: Vec<&str> = Vec::new();
    while i < parts.len() && !is_ldml_key(parts[i].as_bytes()) {
        if !attrs.contains(&parts[i]) {
            attrs.push(parts[i]);
        }
        i += 1;
    }
    let mut local: Vec<(String, String)> = Vec::new();
    if !attrs.is_empty() {
        attrs.sort();
        local.push(("attribute".to_string(), attrs.join("-")));
    }
    while i < parts.len() {
        let key = parts[i];
        i += 1;
        let mut types: Vec<&str> = Vec::new();
        while i < parts.len() && !is_ldml_key(parts[i].as_bytes()) {
            types.push(parts[i]);
            i += 1;
        }
        let lk = to_legacy_key(key);
        let lt = if types.is_empty() {
            "yes".to_string()
        } else {
            to_legacy_type(&lk, &types.join("-"))
        };
        if !variant_exists && lk == "va" && lt == "posix" {
            *posix = true;
            continue;
        }
        if !local.iter().any(|(k, _)| *k == lk) {
            local.push((lk, lt));
        }
    }
    for e in local {
        if !kw.iter().any(|(k, _)| *k == e.0) {
            kw.push(e);
        }
    }
}

/// uloc_forLanguageTag(): (ICU locale ID, parsed length).
pub fn for_language_tag(tag: &[u8]) -> (Vec<u8>, usize) {
    let (t, parsed) = parse_tag(tag);
    let mut out = String::new();
    let lang = t.extlang.first().unwrap_or(&t.language);
    if lang != "und" {
        out.push_str(lang);
    }
    if !t.script.is_empty() {
        out.push('_');
        out.push_str(&t.script);
    }
    if !t.region.is_empty() {
        out.push('_');
        out.push_str(&t.region);
    }
    if !t.variants.is_empty() {
        if t.region.is_empty() {
            out.push('_');
        }
        for v in &t.variants {
            out.push('_');
            out.push_str(v);
        }
    }
    if !t.extensions.is_empty() || !t.privateuse.is_empty() {
        if out.is_empty() && !t.extensions.is_empty() {
            out.push_str("und");
        }
        let mut kw: Vec<(String, String)> = Vec::new();
        let mut posix = false;
        let mut failed = false;
        for (k, v) in &t.extensions {
            if *k == b'u' {
                ldml_ext_keywords(v, &mut kw, !t.variants.is_empty(), &mut posix);
            } else {
                let key = (*k as char).to_string();
                if kw.iter().any(|(kk, _)| *kk == key) {
                    failed = true;
                    break;
                }
                kw.push((key, v.clone()));
            }
        }
        if !failed && !t.privateuse.is_empty() {
            if kw.iter().any(|(k, _)| k == "x") {
                failed = true;
            } else {
                kw.push(("x".to_string(), t.privateuse.clone()));
            }
        }
        if failed {
            return (Vec::new(), 0);
        }
        if posix {
            out.push_str("_POSIX");
        }
        kw.sort();
        for (i, (k, v)) in kw.iter().enumerate() {
            out.push(if i == 0 { '@' } else { ';' });
            out.push_str(k);
            out.push('=');
            out.push_str(v);
        }
    }
    (out.into_bytes(), parsed)
}

// ---------------------------------------------------------------------------
// loclikely.cpp

/// The base subtags of an ID as an `intl` locale (None when they do not
/// form a valid one).
fn intl_locale(lang: &str, script: &str, region: &str) -> Option<intl::locale::Locale> {
    let mut tag = if lang.is_empty() { "und".to_string() } else { lang.to_string() };
    if !script.is_empty() {
        tag.push('-');
        tag.push_str(script);
    }
    if !region.is_empty() {
        tag.push('-');
        tag.push_str(region);
    }
    intl::locale::Locale::parse(&tag).ok()
}

/// What follows the base subtags (variant, keywords) in uloc_getName()
/// form, for the likely-subtags functions.
fn id_tail(st: &Subtags, name: &str) -> (String, String) {
    let kw = name.find('@').map(|p| name[p..].to_string()).unwrap_or_default();
    (String::from_utf8_lossy(&st.variant).into_owned(), kw)
}

fn compose(lang: &str, script: &str, region: &str, variant: &str, kw: &str) -> String {
    let mut out = lang.to_string();
    if !script.is_empty() {
        out.push('_');
        out.push_str(script);
    }
    if !region.is_empty() || !variant.is_empty() {
        out.push('_');
        out.push_str(region);
    }
    if !variant.is_empty() {
        out.push('_');
        out.push_str(variant);
    }
    out.push_str(kw);
    out
}

fn likely_input(id: &[u8]) -> Result<(Subtags, String), i32> {
    let name = get_name(id)?;
    let st = get_subtags(name.as_bytes())?;
    // ICU refuses IDs whose subtags are not well-formed BCP 47 subtags
    let l = s(&st.language);
    if !l.is_empty() && !(is_language_subtag(l.as_bytes()) || l.len() == 3) {
        return Err(U_ILLEGAL_ARGUMENT_ERROR);
    }
    if name.len() >= FULLNAME_CAPACITY {
        return Err(U_ILLEGAL_ARGUMENT_ERROR);
    }
    Ok((st, name))
}

/// uloc_addLikelySubtags()
pub fn add_likely_subtags(id: &[u8]) -> Result<String, i32> {
    let (st, name) = likely_input(id)?;
    let (variant, kw) = id_tail(&st, &name);
    let (l, sc, r) = (s(&st.language), s(&st.script), s(&st.region));
    let Some(loc) = intl_locale(&l, &sc, &r) else {
        return Ok(name);
    };
    let m = loc.maximize();
    if m.language.is_empty() {
        return Ok(name);
    }
    Ok(compose(
        &m.language,
        m.script.as_deref().unwrap_or(""),
        m.region.as_deref().unwrap_or(""),
        &variant,
        &kw,
    ))
}

/// uloc_minimizeSubtags()
pub fn minimize_subtags(id: &[u8]) -> Result<String, i32> {
    let (st, name) = likely_input(id)?;
    let (variant, kw) = id_tail(&st, &name);
    let (l, sc, r) = (s(&st.language), s(&st.script), s(&st.region));
    let Some(loc) = intl_locale(&l, &sc, &r) else {
        return Ok(name);
    };
    let m = loc.minimize();
    if m.language.is_empty() {
        return Ok(name);
    }
    Ok(compose(
        &m.language,
        m.script.as_deref().unwrap_or(""),
        m.region.as_deref().unwrap_or(""),
        &variant,
        &kw,
    ))
}

/// Scripts written right to left (uscript_isRightToLeft()).
const RTL_SCRIPTS: &[&str] = &[
    "Adlm", "Arab", "Aran", "Armi", "Avst", "Chrs", "Cprt", "Elym", "Hatr", "Hebr", "Hung", "Khar", "Lydi", "Mand",
    "Mani", "Mend", "Merc", "Mero", "Narb", "Nbat", "Nkoo", "Orkh", "Ougr", "Palm", "Phli", "Phlp", "Phlv", "Phnx",
    "Prti", "Rohg", "Samr", "Sarb", "Sogd", "Sogo", "Syrc", "Syre", "Syrj", "Syrn", "Thaa", "Yezi",
];

/// uloc_isRightToLeft()
pub fn is_right_to_left(id: &[u8]) -> bool {
    let Ok(st) = get_subtags(id) else {
        return false;
    };
    let mut script = s(&st.script);
    if script.is_empty() {
        let lang = s(&st.language);
        if lang.is_empty() {
            return false;
        }
        let Some(loc) = intl_locale(&lang, "", &s(&st.region)) else {
            return false;
        };
        let m = loc.maximize();
        match m.script {
            Some(sc) => script = sc,
            None => return false,
        }
    }
    RTL_SCRIPTS.contains(&script.as_str())
}

// ---------------------------------------------------------------------------
// locdispnames.cpp

/// The display-data locales to search for display locale `display`: its
/// language (or, as ures_open() falls back, the default locale's), then
/// root.
fn disp_chain(display: &str) -> Vec<&'static str> {
    let find = |id: &str| -> Option<&'static str> {
        let l = get_language(id.as_bytes()).ok()?;
        data::LOCALES.iter().copied().find(|x| *x == l && *x != "root")
    };
    let mut v = Vec::new();
    if let Some(l) = find(display).or_else(|| find(&crate::common::icu_default_locale())) {
        v.push(l);
    }
    v.push("root");
    v
}

type Table = &'static [(&'static str, &'static str, &'static str)];

fn table_lookup(table: Table, loc: &str, key: &str) -> Option<&'static str> {
    table
        .binary_search_by(|(l, k, _)| (*l, *k).cmp(&(loc, key)))
        .ok()
        .map(|i| table[i].2)
}

fn chain_lookup(table: Table, display: &str, key: &str) -> Option<&'static str> {
    disp_chain(display)
        .into_iter()
        .find_map(|l| table_lookup(table, l, key))
}

/// uloc_getDisplayLanguage()
pub fn display_language(id: &[u8], display: &str) -> Result<String, i32> {
    let mut lang = get_subtags(id)?.language;
    if lang.is_empty() {
        lang = b"und".to_vec();
    }
    let code = s(&lang);
    // "Language code should not be a number" (strtol() != 0)
    let digits = code.trim_start_matches(['+', '-']);
    let numeric = digits
        .bytes()
        .take_while(|b| b.is_ascii_digit())
        .any(|b| b != b'0');
    if !numeric {
        if let Some(n) = chain_lookup(data::LANGUAGES, display, &code) {
            return Ok(n.to_string());
        }
        // a deprecated code: its replacement's name (uloc_getCurrentLanguageID())
        if let Some(l) = intl::locale::canonicalize(&code) {
            // only a replacement by a plain language
            let l = l.as_str();
            if l != code && !l.is_empty() && l != "und" && l.bytes().all(|b| b.is_ascii_lowercase()) {
                if let Some(n) = chain_lookup(data::LANGUAGES, display, l) {
                    return Ok(n.to_string());
                }
            }
        }
    }
    Ok(code)
}

/// uloc_getDisplayScript(): the stand-alone form first.
pub fn display_script(id: &[u8], display: &str) -> Result<String, i32> {
    let sc = s(&get_subtags(id)?.script);
    if sc.is_empty() {
        return Ok(sc);
    }
    if let Some(n) = chain_lookup(data::SCRIPTS_STANDALONE, display, &sc) {
        return Ok(n.to_string());
    }
    display_script_in_context(id, display)
}

/// uloc_getDisplayScriptInContext()
pub fn display_script_in_context(id: &[u8], display: &str) -> Result<String, i32> {
    let sc = s(&get_subtags(id)?.script);
    if sc.is_empty() {
        return Ok(sc);
    }
    Ok(chain_lookup(data::SCRIPTS, display, &sc)
        .map(str::to_string)
        .unwrap_or(sc))
}

const DEPRECATED_COUNTRIES: &[&str] = &[
    "AN", "BU", "CS", "DD", "DY", "FX", "HV", "NH", "RH", "SU", "TP", "UK", "VD", "YD", "YU", "ZR",
];
const REPLACEMENT_COUNTRIES: &[&str] = &[
    "CW", "MM", "RS", "DE", "BJ", "FR", "BF", "VU", "ZW", "RU", "TL", "GB", "VN", "YE", "RS", "CD",
];

/// uloc_getDisplayCountry()
pub fn display_country(id: &[u8], display: &str) -> Result<String, i32> {
    let r = s(&get_subtags(id)?.region);
    if r.is_empty() {
        return Ok(r);
    }
    if let Some(n) = chain_lookup(data::REGIONS, display, &r) {
        return Ok(n.to_string());
    }
    // a deprecated code: its replacement's name (uloc_getCurrentCountryID())
    if let Some(i) = DEPRECATED_COUNTRIES.iter().position(|d| *d == r) {
        if let Some(n) = chain_lookup(data::REGIONS, display, REPLACEMENT_COUNTRIES[i]) {
            return Ok(n.to_string());
        }
    }
    Ok(r)
}

/// uloc_getDisplayVariant()
pub fn display_variant(id: &[u8], display: &str) -> Result<String, i32> {
    let v = s(&get_subtags(id)?.variant);
    if v.is_empty() {
        return Ok(v);
    }
    Ok(chain_lookup(data::VARIANTS, display, &v)
        .map(str::to_string)
        .unwrap_or(v))
}

/// uloc_getDisplayKeyword()
pub fn display_keyword(kw: &str, display: &str) -> String {
    chain_lookup(data::KEYS, display, kw)
        .map(str::to_string)
        .unwrap_or_else(|| kw.to_string())
}

/// uloc_getDisplayKeywordValue()
pub fn display_keyword_value(id: &[u8], kw: &str, display: &str) -> Result<String, i32> {
    let v = keyword_value(id, kw)?.unwrap_or_default();
    if kw.eq_ignore_ascii_case("currency") {
        return Ok(chain_lookup(data::CURRENCIES, display, &v)
            .map(str::to_string)
            .unwrap_or(v));
    }
    let key = format!("{}/{}", kw, v);
    Ok(chain_lookup(data::TYPES, display, &key)
        .map(str::to_string)
        .unwrap_or(v))
}

/// uloc_getDisplayName()
pub fn display_name(id: &[u8], display: &str) -> Result<String, i32> {
    let pattern = chain_lookup(data::PATTERNS, display, "pattern").unwrap_or("{0} ({1})");
    let separator = chain_lookup(data::PATTERNS, display, "separator").unwrap_or("{0}, {1}");
    let sep = match (separator.find("{0}"), separator.find("{1}")) {
        (Some(a), Some(b)) if b > a + 3 => &separator[a + 3..b],
        _ => ", ",
    };
    let (open, open_r, close, close_r) = if pattern == "{0} ({1})" {
        ('(', '[', ')', ']')
    } else {
        ('\u{FF08}', '\u{FF3B}', '\u{FF09}', '\u{FF3D}')
    };
    let lang = display_language(id, display)?;
    let mut rest: Vec<String> = vec![
        display_script_in_context(id, display)?,
        display_country(id, display)?,
        display_variant(id, display)?,
    ];
    for kw in keyword_names(id)?.unwrap_or_default() {
        let k = display_keyword(&kw, display);
        let v = display_keyword_value(id, &kw, display)?;
        rest.push(if v.is_empty() {
            k
        } else {
            format!("{}={}", k, v)
        });
    }
    let rest: Vec<String> = rest
        .into_iter()
        .filter(|x| !x.is_empty())
        .map(|x| {
            x.chars()
                .map(|c| {
                    if c == open {
                        open_r
                    } else if c == close {
                        close_r
                    } else {
                        c
                    }
                })
                .collect()
        })
        .collect();
    if rest.is_empty() {
        return Ok(lang);
    }
    let r = rest.join(sep);
    Ok(pattern.replace("{0}", &lang).replace("{1}", &r))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical() {
        let c = |x: &str| canonicalize(x.as_bytes()).unwrap_or_else(|e| format!("ERR{}", e));
        assert_eq!(c("sl_IT_nedis_KIRTI"), "sl_IT_NEDIS_KIRTI");
        assert_eq!(c("i-enochian"), "@x=i-enochian");
        assert_eq!(c("zh-hakka"), "hak");
        assert_eq!(c("de-419-DE"), "de_419_DE");
        assert_eq!(c("x-whatever"), "@x=whatever");
        assert_eq!(c("qaa-Qaaa-QM-x-southern"), "qaa_Qaaa_QM@x=southern");
        assert_eq!(c("en-US-u-islamCal"), "en_US@attribute=islamcal");
        assert_eq!(c("zh-CN-a-myExt-x-private"), "zh_CN@a=myext;x=private");
        assert_eq!(c("sl_IT_x_foo_bar"), "sl_IT@x=foo-bar");
        assert_eq!(c("en_US_a_b_c"), "en_US");
        assert_eq!(c("en-US-u-ca-gregory-x-abc"), "en_US@calendar=gregorian;x=abc");
        assert_eq!(c("en_US_FOO-x-bar"), "en_US");
        assert_eq!(c("en-us-abcde-fghi"), "en_US_ABCDE_FGHI");
        assert_eq!(c("en_US.utf8@euro"), "en_US_EURO");
        assert_eq!(c("de@a"), "de__A");
        assert_eq!(c("en-u-va-posix"), "en__POSIX");
        assert_eq!(c("en-US-1901-u-va-posix"), "en_US_1901@va=posix");
        assert_eq!(c("en-fghij-abcde-a-bb"), "en__ABCDE_FGHIJ@a=bb");
        assert_eq!(c("art-lojban"), "jbo");
        assert_eq!(c("uk-ua_CALIFORNIA@currency=;currency=GRN"), "ERR3");
        assert_eq!(c("en-u-attr2-attr1-ca-x"), "en@attribute=attr1-attr2;calendar=yes");
        assert_eq!(c("en_Latn1_US"), "en_Latn");
        assert_eq!(c("en-US-u-ca-va-posix"), "en_US_POSIX@calendar=yes");
        assert_eq!(c("zh-abc-def-Hant-x-y"), "abc_Hant@x=y");
        assert_eq!(c("en-US-x-lvariant-abc"), "en_US_ABC");
    }

    #[test]
    fn variants() {
        let v = |x: &str| get_variant(x.as_bytes()).unwrap();
        assert_eq!(v("zh-CN-a-myExt-x-private"), "A_MYEXT");
        assert_eq!(v("en-a-myExt-b-another"), "A_MYEXT_B_ANOTHER");
        assert_eq!(v("en-US-POSIX-u-va-posix"), "POSIX_POSIX");
        assert_eq!(v("en-x-va-posix"), "POSIX");
        assert_eq!(v("en-US-u-ca-va-posix"), "");
        assert_eq!(v("en__POSIX"), "POSIX");
        assert_eq!(v("a_b_US"), "B_US");
    }
}
