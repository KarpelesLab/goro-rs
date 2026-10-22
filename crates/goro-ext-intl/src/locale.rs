//! ext/intl/locale: Locale and the locale_*() functions (locale_methods.cpp).

#[path = "uloc.rs"]
pub mod uloc;

#[path = "localematcher.rs"]
pub mod localematcher;

use goro_core::array::{ArrayKey, PhpArray};
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::Vm;

use crate::common::{MAX_LOCALE_LEN, default_locale};
use crate::error::{self, U_ILLEGAL_ARGUMENT_ERROR, U_ZERO_ERROR};
use crate::{Call, arr, bool_v, s};

crate::intl_functions! { pub FUNCTIONS:
    "locale_get_default" => get_default,
    "locale_set_default" => set_default,
    "locale_get_primary_language" => get_primary_language,
    "locale_get_script" => get_script,
    "locale_get_region" => get_region,
    "locale_get_keywords" => get_keywords,
    "locale_get_display_script" => get_display_script,
    "locale_get_display_region" => get_display_region,
    "locale_get_display_name" => get_display_name,
    "locale_get_display_language" => get_display_language,
    "locale_get_display_variant" => get_display_variant,
    "locale_compose" => compose,
    "locale_parse" => parse,
    "locale_get_all_variants" => get_all_variants,
    "locale_filter_matches" => filter_matches,
    "locale_canonicalize" => canonicalize,
    "locale_lookup" => lookup,
    "locale_accept_from_http" => accept_from_http,
    "locale_is_right_to_left" => is_right_to_left,
    "locale_add_likely_subtags" => add_likely_subtags,
    "locale_minimize_subtags" => minimize_subtags,
}

pub const METHODS: &[(&str, crate::Imp)] = &[];

pub const CLASSES: &[(&str, Option<crate::obj::CreateFn>)] = &[("Locale", None)];

/// The IANA grandfathered tags (and redundant ones with a preferred value)
/// php-src special-cases.
const GRANDFATHERED: &[(&str, Option<&str>)] = &[
    ("art-lojban", Some("jbo")),
    ("cel-gaulish", None),
    ("en-GB-oed", Some("en-GB-oxendict")),
    ("i-ami", Some("ami")),
    ("i-bnn", Some("bnn")),
    ("i-default", None),
    ("i-enochian", None),
    ("i-hak", Some("hak")),
    ("i-klingon", Some("tlh")),
    ("i-lux", Some("lb")),
    ("i-mingo", None),
    ("i-navajo", Some("nv")),
    ("i-pwn", Some("pwn")),
    ("i-tao", Some("tao")),
    ("i-tay", Some("tay")),
    ("i-tsu", Some("tsu")),
    ("no-bok", Some("nb")),
    ("no-nyn", Some("nn")),
    ("sgn-BE-FR", Some("sfb")),
    ("sgn-BE-NL", Some("vgt")),
    ("sgn-BR", Some("bzs")),
    ("sgn-CH-DE", Some("sgg")),
    ("sgn-CO", Some("csn")),
    ("sgn-DE", Some("gsg")),
    ("sgn-DK", Some("dsl")),
    ("sgn-ES", Some("ssp")),
    ("sgn-FR", Some("fsl")),
    ("sgn-GB", Some("bfi")),
    ("sgn-GR", Some("gss")),
    ("sgn-IE", Some("isg")),
    ("sgn-IT", Some("ise")),
    ("sgn-JP", Some("jsl")),
    ("sgn-MX", Some("mfs")),
    ("sgn-NI", Some("ncs")),
    ("sgn-NL", Some("dse")),
    ("sgn-NO", Some("nsl")),
    ("sgn-PT", Some("psr")),
    ("sgn-SE", Some("swl")),
    ("sgn-US", Some("ase")),
    ("sgn-ZA", Some("sfs")),
    ("zh-cmn", Some("cmn")),
    ("zh-cmn-Hans", Some("cmn-Hans")),
    ("zh-cmn-Hant", Some("cmn-Hant")),
    ("zh-gan", Some("gan")),
    ("zh-guoyu", Some("cmn")),
    ("zh-hakka", Some("hak")),
    ("zh-min", None),
    ("zh-min-nan", Some("nan")),
    ("zh-wuu", Some("wuu")),
    ("zh-xiang", Some("hsn")),
];

/// findOffset(LOC_GRANDFATHERED, ...)
fn grandfathered(loc: &[u8]) -> Option<usize> {
    GRANDFATHERED
        .iter()
        .position(|(g, _)| g.as_bytes().eq_ignore_ascii_case(loc))
}

/// getPreferredTag()
fn preferred_tag(loc: &[u8]) -> Option<String> {
    let (g, p) = GRANDFATHERED[grandfathered(loc)?];
    Some(p.unwrap_or(g).to_string())
}

fn is_sep(c: u8) -> bool {
    c == b'_' || c == b'-'
}

fn at(s: &[u8], i: usize) -> u8 {
    s.get(i).copied().unwrap_or(0)
}

/// getSingletonPos()
fn singleton_pos(s: &[u8]) -> Option<usize> {
    for i in 0..s.len() {
        if is_sep(s[i]) {
            if i == 1 {
                return Some(0);
            }
            if is_sep(at(s, i + 2)) {
                return Some(i + 1);
            }
        }
    }
    None
}

/// getStrrtokenPos()
fn strrtoken_pos(s: &[u8], saved: usize) -> Option<usize> {
    let mut result: isize = -1;
    let mut i = saved as isize - 1;
    while i >= 0 {
        let c = s[i as usize];
        if is_sep(c) || c == b'@' {
            if i >= 2 && is_sep(s[i as usize - 2]) {
                result = i - 2;
            } else {
                result = i;
            }
            break;
        }
        i -= 1;
    }
    if result < 1 { None } else { Some(result as usize) }
}

/// php_strtok_r() over "-_": the tokens of `s`.
fn tokens(s: &str) -> Vec<&str> {
    s.split(['-', '_']).filter(|t| !t.is_empty()).collect()
}

/// A Z_PARAM_PATH argument: None (ValueError thrown) when it contains NUL.
fn path_arg(vm: &mut Vm, c: &Call, i: usize, pname: &str) -> Option<Vec<u8>> {
    let v = c.str(i);
    if v.contains(&0) {
        crate::zpp::value_error(vm, &c.name, c.argn(i), pname, "must not contain any null bytes");
        return None;
    }
    Some(v)
}

fn opt_path_arg(vm: &mut Vm, c: &Call, i: usize, pname: &str) -> Result<Option<Vec<u8>>, ()> {
    match c.opt_str(i) {
        None => Ok(None),
        Some(v) if v.contains(&0) => {
            crate::zpp::value_error(vm, &c.name, c.argn(i), pname, "must not contain any null bytes");
            Err(())
        }
        Some(v) => Ok(Some(v)),
    }
}

fn locale_too_long(vm: &mut Vm, c: &Call) {
    error::set_global(
        vm,
        &c.name,
        U_ILLEGAL_ARGUMENT_ERROR,
        &format!(
            "Locale string too long, should be no longer than {} characters",
            MAX_LOCALE_LEN
        ),
    );
}

fn skey(k: &str) -> ArrayKey {
    ArrayKey::String(PhpString::from_bytes(k.as_bytes()))
}

#[derive(Clone, Copy, PartialEq)]
enum Tag {
    Lang,
    Script,
    Region,
    Variant,
    Canon,
}

impl Tag {
    fn name(self) -> &'static str {
        match self {
            Tag::Lang => "language",
            Tag::Script => "script",
            Tag::Region => "region",
            Tag::Variant => "variant",
            Tag::Canon => "canonicalize",
        }
    }
}

/// get_icu_value_internal(): (result: 0 error, -1 no value, 1 found;
/// unchanged = 0 where php-src returns early) and the value.
fn icu_value(loc: &[u8], tag: Tag, from_parse: bool) -> (i32, Option<String>) {
    if loc.len() > MAX_LOCALE_LEN {
        return (0, None);
    }
    let mut id: &[u8] = loc;
    if tag != Tag::Canon {
        if grandfathered(loc).is_some() {
            if tag == Tag::Lang {
                return (0, Some(String::from_utf8_lossy(loc).into_owned()));
            }
            return (0, None);
        }
        if from_parse {
            if tag == Tag::Lang
                && loc.len() > 1
                && matches!(loc[0], b'x' | b'X' | b'i' | b'I')
                && is_sep(loc[1])
            {
                return (0, Some(String::from_utf8_lossy(loc).into_owned()));
            }
            match singleton_pos(loc) {
                Some(0) => return (0, None),
                Some(p) => id = &loc[..p - 1],
                None => {}
            }
        }
    }
    let r = match tag {
        Tag::Lang => uloc::get_language(id),
        Tag::Script => uloc::get_script(id),
        Tag::Region => uloc::get_country(id),
        Tag::Variant => uloc::get_variant(id),
        Tag::Canon => uloc::canonicalize(id),
    };
    match r {
        Err(_) => (0, None),
        Ok(v) if v.is_empty() => (-1, None),
        Ok(v) => (1, Some(v)),
    }
}

/// locale_get_default()
fn get_default(vm: &mut Vm, _c: &mut Call) -> Value {
    s(default_locale(vm))
}

/// zend_alter_ini_entry("intl.default_locale", ...)
fn alter_default_locale(vm: &mut Vm, v: &[u8]) -> bool {
    let key: &[u8] = b"intl.default_locale";
    if let Some(f) = vm.ini_validators.get(key).copied() {
        if !f(vm, v, false) {
            return false;
        }
    }
    if !vm.ini_originals.contains_key(key) {
        let cur = vm.constants.get(key).cloned();
        vm.ini_originals.insert(key.to_vec(), cur);
    }
    vm.constants.insert(key.to_vec(), s(v));
    true
}

/// locale_set_default()
fn set_default(vm: &mut Vm, c: &mut Call) -> Value {
    let Some(mut loc) = path_arg(vm, c, 0, "locale") else {
        return Value::Null;
    };
    if loc.is_empty() {
        loc = crate::common::icu_default_locale().into_bytes();
    }
    alter_default_locale(vm, &loc);
    Value::True
}

/// get_icu_value_src_php()
fn icu_value_php(vm: &mut Vm, c: &mut Call, tag: Tag) -> Value {
    error::reset_global();
    let Some(mut loc) = path_arg(vm, c, 0, "locale") else {
        return Value::Null;
    };
    if loc.is_empty() {
        loc = default_locale(vm).into_bytes();
    }
    if loc.len() > MAX_LOCALE_LEN {
        locale_too_long(vm, c);
        return Value::Null;
    }
    let (result, v) = icu_value(&loc, tag, false);
    if result == -1 {
        return s(b"");
    }
    if let Some(v) = v {
        return s(v);
    }
    if result == 0 {
        let msg = format!("unable to get locale {}", tag.name());
        error::set_global(vm, &c.name, U_ZERO_ERROR, &msg);
    }
    Value::Null
}

fn get_primary_language(vm: &mut Vm, c: &mut Call) -> Value {
    icu_value_php(vm, c, Tag::Lang)
}

fn get_script(vm: &mut Vm, c: &mut Call) -> Value {
    icu_value_php(vm, c, Tag::Script)
}

fn get_region(vm: &mut Vm, c: &mut Call) -> Value {
    icu_value_php(vm, c, Tag::Region)
}

fn canonicalize(vm: &mut Vm, c: &mut Call) -> Value {
    icu_value_php(vm, c, Tag::Canon)
}

#[derive(Clone, Copy, PartialEq)]
enum Disp {
    Name,
    Lang,
    Script,
    Region,
    Variant,
}

/// get_icu_disp_value_src_php()
fn disp_value_php(vm: &mut Vm, c: &mut Call, tag: Disp) -> Value {
    error::reset_global();
    let Some(mut loc) = path_arg(vm, c, 0, "locale") else {
        return Value::Null;
    };
    let Ok(disp) = opt_path_arg(vm, c, 1, "displayLocale") else {
        return Value::Null;
    };
    if loc.len() > uloc::FULLNAME_CAPACITY {
        error::set_global(vm, &c.name, U_ILLEGAL_ARGUMENT_ERROR, "name too long");
        return Value::False;
    }
    if loc.is_empty() {
        loc = default_locale(vm).into_bytes();
    }
    let mut id = loc.clone();
    if tag != Disp::Name && grandfathered(&loc).is_some() {
        if tag == Disp::Lang {
            id = preferred_tag(&loc).unwrap_or_default().into_bytes();
        } else {
            return Value::False;
        }
    }
    let disp = match disp {
        Some(d) => String::from_utf8_lossy(&d).into_owned(),
        None => default_locale(vm),
    };
    let r = match tag {
        Disp::Name => uloc::display_name(&id, &disp),
        Disp::Lang => uloc::display_language(&id, &disp),
        Disp::Script => uloc::display_script(&id, &disp),
        Disp::Region => uloc::display_country(&id, &disp),
        Disp::Variant => uloc::display_variant(&id, &disp),
    };
    match r {
        Ok(v) => s(v),
        Err(code) => {
            let name = match tag {
                Disp::Name => "name",
                Disp::Lang => "language",
                Disp::Script => "script",
                Disp::Region => "region",
                Disp::Variant => "variant",
            };
            error::set_global(vm, &c.name, code, &format!("unable to get locale {}", name));
            Value::False
        }
    }
}

fn get_display_name(vm: &mut Vm, c: &mut Call) -> Value {
    disp_value_php(vm, c, Disp::Name)
}

fn get_display_language(vm: &mut Vm, c: &mut Call) -> Value {
    disp_value_php(vm, c, Disp::Lang)
}

fn get_display_script(vm: &mut Vm, c: &mut Call) -> Value {
    disp_value_php(vm, c, Disp::Script)
}

fn get_display_region(vm: &mut Vm, c: &mut Call) -> Value {
    disp_value_php(vm, c, Disp::Region)
}

fn get_display_variant(vm: &mut Vm, c: &mut Call) -> Value {
    disp_value_php(vm, c, Disp::Variant)
}

/// locale_get_keywords()
fn get_keywords(vm: &mut Vm, c: &mut Call) -> Value {
    error::reset_global();
    let Some(mut loc) = path_arg(vm, c, 0, "locale") else {
        return Value::Null;
    };
    if loc.len() > MAX_LOCALE_LEN {
        locale_too_long(vm, c);
        return Value::Null;
    }
    if loc.is_empty() {
        loc = default_locale(vm).into_bytes();
    }
    let names = match uloc::keyword_names(&loc) {
        Ok(Some(n)) => n,
        _ => return Value::Null,
    };
    let mut a = PhpArray::new();
    for k in names {
        match uloc::keyword_value(&loc, &k) {
            Ok(v) => a.set(skey(&k), s(v.unwrap_or_default())),
            Err(_) => {
                error::set_global(
                    vm,
                    &c.name,
                    U_ILLEGAL_ARGUMENT_ERROR,
                    "Error encountered while getting the keyword value for the keyword",
                );
                return Value::False;
            }
        }
    }
    arr(a)
}

/// The value of `key` in the subtags array (dereferenced), if any.
fn subtag_value(h: &PhpArray, key: &str) -> Option<Value> {
    h.get(&skey(key)).map(|v| v.deref())
}

enum Append {
    Ok,
    NotFound,
    Failure,
}

/// append_key_value()
fn append_key_value(out: &mut Vec<u8>, h: &PhpArray, key: &str) -> Append {
    match subtag_value(h, key) {
        Some(Value::String(v)) => {
            if key != "language" && key != "grandfathered" {
                out.push(b'_');
            }
            out.extend_from_slice(v.as_bytes());
            Append::Ok
        }
        Some(_) => Append::Failure,
        None => Append::NotFound,
    }
}

/// append_multiple_key_values()
fn append_multiple(out: &mut Vec<u8>, h: &PhpArray, key: &str) -> Append {
    let prefix = |out: &mut Vec<u8>, k: &str| {
        if k.starts_with("private") {
            out.extend_from_slice(b"_x");
        }
    };
    match subtag_value(h, key) {
        Some(Value::String(v)) => {
            prefix(out, key);
            out.push(b'_');
            out.extend_from_slice(v.as_bytes());
            return Append::Ok;
        }
        Some(Value::Array(a)) => {
            let a = a.borrow();
            let mut first = true;
            for (_, v) in a.iter() {
                let Value::String(v) = v.deref() else {
                    return Append::Failure;
                };
                if first {
                    prefix(out, key);
                    first = false;
                }
                out.push(b'_');
                out.extend_from_slice(v.as_bytes());
            }
            return Append::Ok;
        }
        Some(_) => return Append::Failure,
        None => {}
    }
    let max = match key {
        "variant" => 15,
        "extlang" => 3,
        "private" => 15,
        _ => 0,
    };
    let mut first = true;
    for i in 0..max {
        let k = format!("{}{}", key, i);
        match subtag_value(h, &k) {
            Some(Value::String(v)) => {
                if first {
                    prefix(out, &k);
                    first = false;
                }
                out.push(b'_');
                out.extend_from_slice(v.as_bytes());
            }
            Some(_) => return Append::Failure,
            None => {}
        }
    }
    Append::Ok
}

/// locale_compose()
fn compose(vm: &mut Vm, c: &mut Call) -> Value {
    error::reset_global();
    let Value::Array(a) = c.arg(0) else {
        return Value::False;
    };
    let h = a.borrow();
    if h.is_empty() {
        return Value::False;
    }
    let mut out = Vec::new();
    let fail = |vm: &mut Vm| {
        error::set_global(
            vm,
            &c.name,
            U_ILLEGAL_ARGUMENT_ERROR,
            "parameter array element is not a string",
        );
        Value::False
    };
    match append_key_value(&mut out, &h, "grandfathered") {
        Append::Ok => return s(out),
        Append::Failure => return fail(vm),
        Append::NotFound => {}
    }
    match append_key_value(&mut out, &h, "language") {
        Append::NotFound => {
            crate::zpp::value_error(vm, &c.name, 1, "subtags", "must contain a \"language\" key");
            return Value::Null;
        }
        Append::Failure => return fail(vm),
        Append::Ok => {}
    }
    if let Append::Failure = append_multiple(&mut out, &h, "extlang") {
        return fail(vm);
    }
    if let Append::Failure = append_key_value(&mut out, &h, "script") {
        return fail(vm);
    }
    if let Append::Failure = append_key_value(&mut out, &h, "region") {
        return fail(vm);
    }
    if let Append::Failure = append_multiple(&mut out, &h, "variant") {
        return fail(vm);
    }
    if let Append::Failure = append_multiple(&mut out, &h, "private") {
        return fail(vm);
    }
    s(out)
}

/// get_private_subtags()
fn private_subtags(loc: &[u8]) -> Option<String> {
    let mut m = loc;
    while let Some(p) = singleton_pos(m) {
        if m[p] == b'x' || m[p] == b'X' {
            if p + 2 == m.len() {
                return None;
            }
            return Some(String::from_utf8_lossy(&m[(p + 2).min(m.len())..]).into_owned());
        }
        if p + 1 >= m.len() {
            break;
        }
        m = &m[p + 1..];
    }
    None
}

/// add_array_entry()
fn add_array_entry(loc: &[u8], h: &mut PhpArray, key: Tag, private: bool) {
    let (result, value) = if private {
        (1, private_subtags(loc))
    } else {
        icu_value(loc, key, true)
    };
    let name = if private { "private" } else { key.name() };
    if private || key == Tag::Variant {
        if result > 0 {
            if let Some(v) = value {
                let t = tokens(&v);
                for (i, tok) in t.iter().enumerate() {
                    if i > 0 && tok.len() <= 1 {
                        break;
                    }
                    h.set(skey(&format!("{}{}", name, i)), s(*tok));
                }
            }
        }
    } else if result == 1 {
        if let Some(v) = value {
            h.set(skey(name), s(v));
        }
    }
}

/// locale_parse()
fn parse(vm: &mut Vm, c: &mut Call) -> Value {
    error::reset_global();
    let Some(mut loc) = path_arg(vm, c, 0, "locale") else {
        return Value::Null;
    };
    if loc.len() > MAX_LOCALE_LEN {
        locale_too_long(vm, c);
        return Value::Null;
    }
    if loc.is_empty() {
        loc = default_locale(vm).into_bytes();
    }
    let mut h = PhpArray::new();
    if grandfathered(&loc).is_some() {
        h.set(skey("grandfathered"), s(&loc));
    } else {
        add_array_entry(&loc, &mut h, Tag::Lang, false);
        add_array_entry(&loc, &mut h, Tag::Script, false);
        add_array_entry(&loc, &mut h, Tag::Region, false);
        add_array_entry(&loc, &mut h, Tag::Variant, false);
        add_array_entry(&loc, &mut h, Tag::Lang, true);
    }
    arr(h)
}

/// locale_get_all_variants()
fn get_all_variants(vm: &mut Vm, c: &mut Call) -> Value {
    error::reset_global();
    let Some(mut loc) = path_arg(vm, c, 0, "locale") else {
        return Value::Null;
    };
    if loc.is_empty() {
        loc = default_locale(vm).into_bytes();
    }
    if loc.len() > MAX_LOCALE_LEN {
        locale_too_long(vm, c);
        return Value::Null;
    }
    let mut h = PhpArray::new();
    if grandfathered(&loc).is_none() {
        let (result, v) = icu_value(&loc, Tag::Variant, false);
        if result > 0 {
            if let Some(v) = v {
                for (i, tok) in tokens(&v).iter().enumerate() {
                    if i > 0 && tok.len() <= 1 {
                        break;
                    }
                    h.push(s(*tok));
                }
            }
        }
    }
    arr(h)
}

/// strToMatch(): lowercase, '-' → '_'.
fn str_to_match(s: &[u8]) -> Vec<u8> {
    s.iter()
        .map(|&c| if c == b'-' { b'_' } else { c.to_ascii_lowercase() })
        .collect()
}

/// locale_filter_matches()
fn filter_matches(vm: &mut Vm, c: &mut Call) -> Value {
    error::reset_global();
    let Some(lang_tag) = path_arg(vm, c, 0, "languageTag") else {
        return Value::Null;
    };
    let Some(mut range) = path_arg(vm, c, 1, "locale") else {
        return Value::Null;
    };
    let canonical = c.bool(2);
    if range.is_empty() {
        range = default_locale(vm).into_bytes();
    }
    if range == b"*" {
        return Value::True;
    }
    if range.len() > MAX_LOCALE_LEN || lang_tag.len() > MAX_LOCALE_LEN {
        locale_too_long(vm, c);
        return Value::Null;
    }
    let (tag, rng) = if canonical {
        let (r1, can_range) = icu_value(&range, Tag::Canon, false);
        if r1 <= 0 {
            error::set_global(vm, &c.name, U_ZERO_ERROR, "unable to canonicalize loc_range");
            return Value::False;
        }
        let (r2, can_tag) = icu_value(&lang_tag, Tag::Canon, false);
        if r2 <= 0 {
            error::set_global(vm, &c.name, U_ZERO_ERROR, "unable to canonicalize lang_tag");
            return Value::False;
        }
        (
            can_tag.unwrap_or_default().into_bytes(),
            can_range.unwrap_or_default().into_bytes(),
        )
    } else {
        (lang_tag, range)
    };
    if tag.is_empty() || rng.is_empty() {
        return Value::False;
    }
    let t = str_to_match(&tag);
    let r = str_to_match(&rng);
    if t.starts_with(&r) {
        let ch = at(&t, r.len());
        if is_sep(ch) || ch == 0 || (canonical && ch == b'@') {
            return Value::True;
        }
    }
    Value::False
}

/// lookup_loc_range(): None on error (exception thrown or error set) or no
/// match.
fn lookup_loc_range(
    vm: &mut Vm,
    c: &Call,
    range: &[u8],
    h: &PhpArray,
    canonicalize: bool,
) -> Option<String> {
    let mut cur: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
    for (_, v) in h.iter() {
        let Value::String(v) = v.deref() else {
            crate::zpp::arg_type_error(vm, &c.name, 2, "locale", "must only contain string values");
            return None;
        };
        let b = v.as_bytes();
        if b.contains(&0) {
            crate::zpp::value_error(vm, &c.name, 2, "locale", "must not contain any null bytes");
            return None;
        }
        if b.is_empty() {
            error::set_global(vm, &c.name, U_ILLEGAL_ARGUMENT_ERROR, "unable to canonicalize lang_tag");
            return None;
        }
        cur.push((str_to_match(b), b.to_vec()));
    }
    if canonicalize {
        for e in cur.iter_mut() {
            let (r, t) = icu_value(&e.0, Tag::Canon, false);
            match t {
                Some(t) if r == 1 && !t.is_empty() => e.0 = str_to_match(t.as_bytes()),
                _ => {
                    error::set_global(vm, &c.name, U_ILLEGAL_ARGUMENT_ERROR, "unable to canonicalize lang_tag");
                    return None;
                }
            }
        }
    }
    let range: Vec<u8> = if canonicalize {
        let (r, t) = icu_value(range, Tag::Canon, false);
        match t {
            Some(t) if r == 1 && !t.is_empty() => t.into_bytes(),
            _ => {
                error::set_global(vm, &c.name, U_ILLEGAL_ARGUMENT_ERROR, "unable to canonicalize loc_range");
                return None;
            }
        }
    } else {
        range.to_vec()
    };
    if range.is_empty() {
        error::set_global(vm, &c.name, U_ILLEGAL_ARGUMENT_ERROR, "unable to canonicalize lang_tag");
        return None;
    }
    let r = str_to_match(&range);
    let mut saved = r.len();
    while saved > 0 {
        for (m, orig) in &cur {
            if m.len() == saved && r[..saved] == m[..] {
                let v = if canonicalize { m } else { orig };
                return Some(String::from_utf8_lossy(v).into_owned());
            }
        }
        match strrtoken_pos(&r, saved) {
            Some(p) => saved = p,
            None => break,
        }
    }
    None
}

/// locale_lookup()
fn lookup(vm: &mut Vm, c: &mut Call) -> Value {
    error::reset_global();
    let Value::Array(a) = c.arg(0) else {
        return Value::Null;
    };
    let Some(mut range) = path_arg(vm, c, 1, "locale") else {
        return Value::Null;
    };
    let canonical = c.bool(2);
    let Ok(fallback) = opt_path_arg(vm, c, 3, "defaultLocale") else {
        return Value::Null;
    };
    if range.is_empty() {
        range = match &fallback {
            Some(f) => f.clone(),
            None => default_locale(vm).into_bytes(),
        };
    }
    if range.len() > MAX_LOCALE_LEN {
        locale_too_long(vm, c);
        return Value::Null;
    }
    let h = a.borrow().clone();
    if h.is_empty() {
        return s(b"");
    }
    let r = lookup_loc_range(vm, c, &range, &h, canonical);
    if vm.current_exception.is_some() {
        return Value::Null;
    }
    match r {
        Some(v) if !v.is_empty() => s(v),
        _ => match fallback {
            Some(f) => s(f),
            None => s(b""),
        },
    }
}

/// uloc_acceptLanguageFromHTTP() over ICU's installed locales: the
/// header's tags by decreasing q weight (LocalePriorityList), matched by
/// LocaleMatcher.
fn accept_language(header: &[u8]) -> Result<Option<String>, i32> {
    let s = header;
    let n = s.len();
    let skip = |mut i: usize| {
        while i < n && s[i] == b' ' {
            i += 1;
        }
        i
    };
    // LocalePriorityList
    let mut list: Vec<(Vec<u8>, f64, usize)> = Vec::new();
    let mut i = 0;
    loop {
        i = skip(i);
        if i == n {
            break;
        }
        let start = i;
        while i < n && !matches!(s[i], b' ' | b',' | b';') {
            i += 1;
        }
        let tag = &s[start..i];
        let mut weight = 1.0;
        i = skip(i);
        if i < n && s[i] == b';' {
            i = skip(i + 1);
            if i < n && (s[i] == b'q' || s[i] == b'Q') {
                i = skip(i + 1);
            } else {
                return Err(U_ILLEGAL_ARGUMENT_ERROR);
            }
            if i < n && s[i] == b'=' {
                i = skip(i + 1);
            } else {
                return Err(U_ILLEGAL_ARGUMENT_ERROR);
            }
            // parseWeight(): 0 or 1, then decimals
            if i < n && (s[i] == b'0' || s[i] == b'1') {
                weight = (s[i] - b'0') as f64;
                i += 1;
                if i < n && s[i] == b'.' {
                    i += 1;
                    let mut dec = 1.0;
                    while i < n && s[i].is_ascii_digit() {
                        dec /= 10.0;
                        weight += (s[i] - b'0') as f64 * dec;
                        i += 1;
                    }
                }
                if weight > 1.0 {
                    return Err(U_ILLEGAL_ARGUMENT_ERROR);
                }
            } else {
                return Err(U_ILLEGAL_ARGUMENT_ERROR);
            }
        }
        if weight > 0.0 && tag != b"*" {
            if let Ok(name) = uloc::get_name(tag) {
                // a repeated locale takes the new weight and position
                let name = name.into_bytes();
                list.retain(|(t, _, _)| *t != name);
                list.push((name, weight, i));
            }
        }
        i = skip(i);
        if i == n {
            break;
        } else if s[i] == b',' {
            i += 1;
        } else {
            return Err(U_ILLEGAL_ARGUMENT_ERROR);
        }
    }
    list.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.2.cmp(&b.2))
    });
    let desired: Vec<Vec<u8>> = list.into_iter().map(|(t, _, _)| t).collect();
    Ok(localematcher::best_match(&desired).map(str::to_string))
}

/// locale_accept_from_http()
fn accept_from_http(vm: &mut Vm, c: &mut Call) -> Value {
    let header = c.str(0);
    if header.len() > uloc::FULLNAME_CAPACITY {
        for frag in header.split(|&b| b == b',') {
            if frag.len() > uloc::FULLNAME_CAPACITY {
                error::set_global(vm, &c.name, U_ILLEGAL_ARGUMENT_ERROR, "locale string too long");
                return Value::False;
            }
        }
    }
    match accept_language(&header) {
        Ok(Some(r)) => s(r),
        Ok(None) => Value::False,
        Err(code) => {
            error::set_global(vm, &c.name, code, "failed to find acceptable locale");
            Value::False
        }
    }
}

/// locale_is_right_to_left()
fn is_right_to_left(vm: &mut Vm, c: &mut Call) -> Value {
    let Some(mut loc) = path_arg(vm, c, 0, "locale") else {
        return Value::Null;
    };
    if loc.is_empty() {
        loc = default_locale(vm).into_bytes();
    }
    bool_v(uloc::is_right_to_left(&loc))
}

fn likely(vm: &mut Vm, c: &mut Call, f: fn(&[u8]) -> Result<String, i32>) -> Value {
    let Some(mut loc) = path_arg(vm, c, 0, "locale") else {
        return Value::Null;
    };
    if loc.is_empty() {
        loc = default_locale(vm).into_bytes();
    }
    match f(&loc) {
        Ok(v) => {
            error::set_global_code(U_ZERO_ERROR);
            s(v)
        }
        Err(code) => {
            error::set_global(vm, &c.name, code, "invalid locale");
            Value::False
        }
    }
}

/// locale_add_likely_subtags()
fn add_likely_subtags(vm: &mut Vm, c: &mut Call) -> Value {
    likely(vm, c, uloc::add_likely_subtags)
}

/// locale_minimize_subtags()
fn minimize_subtags(vm: &mut Vm, c: &mut Call) -> Value {
    likely(vm, c, uloc::minimize_subtags)
}
