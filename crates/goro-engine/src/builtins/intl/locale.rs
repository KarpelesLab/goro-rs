//! The `Locale` class and the `locale_*()` functions (PHP manual: Locale).
//! The methods are static and share the functions' handlers.

use goro_ext_intl::error as ue;
use goro_ext_intl::locale as loc;
use goro_runtime::{Key, PhpArray, PhpStr, Value};

use super::{IntlState, default_locale, fail, reset};
use crate::func::Args;
use crate::registry::Module;
use crate::vm::{R, Vm};

pub static MODULE: Module = Module {
    name: "intl",
    functions: &[
        ("function locale_get_default(): string", get_default),
        ("function locale_set_default(string $locale): bool", set_default),
        (
            "function locale_get_primary_language(string $locale): ?string",
            get_primary_language,
        ),
        ("function locale_get_script(string $locale): ?string", get_script),
        ("function locale_get_region(string $locale): ?string", get_region),
        (
            "function locale_get_keywords(string $locale): array|false|null",
            get_keywords,
        ),
        (
            "function locale_get_display_script(string $locale, ?string $displayLocale = null): string|false",
            display_script,
        ),
        (
            "function locale_get_display_region(string $locale, ?string $displayLocale = null): string|false",
            display_region,
        ),
        (
            "function locale_get_display_name(string $locale, ?string $displayLocale = null): string|false",
            display_name,
        ),
        (
            "function locale_get_display_language(string $locale, ?string $displayLocale = null): string|false",
            display_language,
        ),
        (
            "function locale_get_display_variant(string $locale, ?string $displayLocale = null): string|false",
            display_variant,
        ),
        ("function locale_compose(array $subtags): string|false", compose),
        ("function locale_parse(string $locale): ?array", parse_locale),
        (
            "function locale_get_all_variants(string $locale): ?array",
            get_all_variants,
        ),
        (
            "function locale_filter_matches(string $languageTag, string $locale, bool $canonicalize = false): ?bool",
            filter_matches,
        ),
        (
            "function locale_canonicalize(string $locale): ?string",
            canonicalize,
        ),
        (
            "function locale_lookup(array $languageTag, string $locale, bool $canonicalize = false, ?string $defaultLocale = null): ?string",
            lookup,
        ),
        (
            "function locale_accept_from_http(string $header): string|false",
            accept_from_http,
        ),
        (
            "function locale_is_right_to_left(string $locale): bool",
            is_right_to_left,
        ),
        (
            "function locale_add_likely_subtags(string $locale): string|false",
            add_likely_subtags,
        ),
        (
            "function locale_minimize_subtags(string $locale): string|false",
            minimize_subtags,
        ),
    ],
    classes: &[("locale", LOCALE_CLASS)],
    methods: &[
        ("locale::getdefault", get_default),
        ("locale::setdefault", set_default),
        ("locale::getprimarylanguage", get_primary_language),
        ("locale::getscript", get_script),
        ("locale::getregion", get_region),
        ("locale::getkeywords", get_keywords),
        ("locale::getdisplayscript", display_script),
        ("locale::getdisplayregion", display_region),
        ("locale::getdisplayname", display_name),
        ("locale::getdisplaylanguage", display_language),
        ("locale::getdisplayvariant", display_variant),
        ("locale::composelocale", compose),
        ("locale::parselocale", parse_locale),
        ("locale::getallvariants", get_all_variants),
        ("locale::filtermatches", filter_matches),
        ("locale::lookup", lookup),
        ("locale::canonicalize", canonicalize),
        ("locale::acceptfromhttp", accept_from_http),
        ("locale::isrighttoleft", is_right_to_left),
        ("locale::addlikelysubtags", add_likely_subtags),
        ("locale::minimizesubtags", minimize_subtags),
    ],
    ..Module::EMPTY
};

const LOCALE_CLASS: &str = r#"<?php class Locale {
    public const int ACTUAL_LOCALE = 0;
    public const int VALID_LOCALE = 1;
    public const null DEFAULT_LOCALE = null;
    public const string LANG_TAG = "language";
    public const string EXTLANG_TAG = "extlang";
    public const string SCRIPT_TAG = "script";
    public const string REGION_TAG = "region";
    public const string VARIANT_TAG = "variant";
    public const string GRANDFATHERED_LANG_TAG = "grandfathered";
    public const string PRIVATE_TAG = "private";
    public static function getDefault(): string {}
    public static function setDefault(string $locale): true {}
    public static function getPrimaryLanguage(string $locale): ?string {}
    public static function getScript(string $locale): ?string {}
    public static function getRegion(string $locale): ?string {}
    public static function getKeywords(string $locale): array|false|null {}
    public static function getDisplayScript(string $locale, ?string $displayLocale = null): string|false {}
    public static function getDisplayRegion(string $locale, ?string $displayLocale = null): string|false {}
    public static function getDisplayName(string $locale, ?string $displayLocale = null): string|false {}
    public static function getDisplayLanguage(string $locale, ?string $displayLocale = null): string|false {}
    public static function getDisplayVariant(string $locale, ?string $displayLocale = null): string|false {}
    public static function composeLocale(array $subtags): string|false {}
    public static function parseLocale(string $locale): ?array {}
    public static function getAllVariants(string $locale): ?array {}
    public static function filterMatches(string $languageTag, string $locale, bool $canonicalize = false): ?bool {}
    public static function lookup(array $languageTag, string $locale, bool $canonicalize = false, ?string $defaultLocale = null): ?string {}
    public static function canonicalize(string $locale): ?string {}
    public static function acceptFromHttp(string $header): string|false {}
    public static function isRightToLeft(string $locale): bool {}
    public static function addLikelySubtags(string $locale): string|false {}
    public static function minimizeSubtags(string $locale): string|false {}
}"#;

/// The maximum length of a locale ID (INTL_MAX_LOCALE_LEN).
pub(crate) const MAX_LOCALE_LEN: usize = 156;

/// Argument `i` as a locale string: a ValueError for NUL bytes ("must
/// not contain any null bytes": ext/intl/tests/locale_get_region.phpt).
fn locale_arg(vm: &mut Vm, a: &Args, i: usize, name: &str) -> R<String> {
    let b = a.bytes(i);
    if b.contains(&0) {
        return vm.fthrow(
            a,
            "ValueError",
            &format!("Argument #{} (${name}) must not contain any null bytes", i + 1),
        );
    }
    Ok(String::from_utf8_lossy(b).into_owned())
}

/// The locale argument `i`, the default locale when empty.
fn locale_or_default(vm: &mut Vm, a: &Args, i: usize, name: &str) -> R<String> {
    let s = locale_arg(vm, a, i, name)?;
    Ok(if s.is_empty() { default_locale(vm) } else { s })
}

fn opt_str(s: String) -> Value {
    Value::from(s.as_str())
}

fn get_default(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    let l = default_locale(vm);
    Ok(Value::from(l.as_str()))
}

fn set_default(vm: &mut Vm, a: &mut Args) -> R<Value> {
    reset(vm);
    let l = locale_arg(vm, a, 0, "locale")?;
    let l = if l.is_empty() {
        "en_US_POSIX".to_string()
    } else {
        l
    };
    vm.ext::<IntlState>().default_locale = Some(l);
    Ok(Value::Bool(true))
}

/// Whether the PHP functions treat `id` as a grandfathered tag (kept
/// whole).
fn grandfathered(id: &str) -> bool {
    let main = id.split('@').next().unwrap_or("");
    loc::grandfathered(main).is_some()
}

fn get_primary_language(vm: &mut Vm, a: &mut Args) -> R<Value> {
    reset(vm);
    let l = locale_or_default(vm, a, 0, "locale")?;
    // ICU's language buffer is bounded (bug #72241)
    if l.len() > MAX_LOCALE_LEN {
        return Ok(Value::Null);
    }
    if grandfathered(&l) {
        return Ok(Value::from(l.to_ascii_lowercase().as_str()));
    }
    Ok(opt_str(loc::language(&l)))
}

fn get_script(vm: &mut Vm, a: &mut Args) -> R<Value> {
    reset(vm);
    let l = locale_or_default(vm, a, 0, "locale")?;
    if grandfathered(&l) {
        return Ok(Value::from(""));
    }
    Ok(opt_str(loc::parse(&l).script))
}

fn get_region(vm: &mut Vm, a: &mut Args) -> R<Value> {
    reset(vm);
    let l = locale_or_default(vm, a, 0, "locale")?;
    if grandfathered(&l) {
        return Ok(Value::from(""));
    }
    Ok(opt_str(loc::parse(&l).region))
}

fn get_keywords(vm: &mut Vm, a: &mut Args) -> R<Value> {
    reset(vm);
    let l = locale_or_default(vm, a, 0, "locale")?;
    let p = loc::parse(&l);
    if p.keywords.is_empty() {
        return Ok(Value::Array(PhpArray::new()));
    }
    let mut arr = PhpArray::new();
    for (k, v) in &p.keywords {
        arr.insert(Key::from_bytes(k.as_bytes()), Value::from(v.as_str()));
    }
    Ok(Value::Array(arr))
}

fn display_args(vm: &mut Vm, a: &Args) -> R<Option<(String, String)>> {
    reset(vm);
    let l = locale_arg(vm, a, 0, "locale")?;
    let d = match a.opt_str(1) {
        Some(s) => {
            if s.as_bytes().contains(&0) {
                return vm.fthrow(
                    a,
                    "ValueError",
                    "Argument #2 ($displayLocale) must not contain any null bytes",
                );
            }
            s.to_string_lossy()
        }
        None => String::new(),
    };
    if l.len() > MAX_LOCALE_LEN {
        fail(vm, a, ue::U_ILLEGAL_ARGUMENT_ERROR, "name too long")?;
        return Ok(None);
    }
    let l = if l.is_empty() { default_locale(vm) } else { l };
    let d = if d.is_empty() { default_locale(vm) } else { d };
    Ok(Some((l, d)))
}

fn display_script(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let Some((l, d)) = display_args(vm, a)? else {
        return Ok(Value::Bool(false));
    };
    let s = loc::parse(&l).script;
    if s.is_empty() || grandfathered(&l) {
        return Ok(Value::from(""));
    }
    Ok(opt_str(loc::display_script(&s, &d, true)))
}

fn display_region(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let Some((l, d)) = display_args(vm, a)? else {
        return Ok(Value::Bool(false));
    };
    let r = loc::parse(&l).region;
    if r.is_empty() || grandfathered(&l) {
        return Ok(Value::from(""));
    }
    Ok(opt_str(loc::display_region(&r, &d)))
}

fn display_variant(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let Some((l, d)) = display_args(vm, a)? else {
        return Ok(Value::Bool(false));
    };
    let v = loc::parse(&l).variant;
    if v.is_empty() || grandfathered(&l) {
        return Ok(Value::from(""));
    }
    Ok(opt_str(loc::display_variant(&v, &d)))
}

fn display_language(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let Some((l, d)) = display_args(vm, a)? else {
        return Ok(Value::Bool(false));
    };
    // art-lojban is Lojban (ICU's alias of art_LOJBAN), the other
    // grandfathered tags show their first subtags
    let lang = loc::display_language_code(&l);
    Ok(opt_str(loc::display_language(&lang, &d)))
}

fn display_name(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let Some((l, d)) = display_args(vm, a)? else {
        return Ok(Value::Bool(false));
    };
    match loc::display_name(&l, &d) {
        Some(n) => Ok(opt_str(n)),
        None => {
            fail(
                vm,
                a,
                ue::U_ILLEGAL_ARGUMENT_ERROR,
                "locale_get_display_name : error getting the display name",
            )?;
            Ok(Value::Bool(false))
        }
    }
}

/// `Locale::composeLocale()`: the subtags joined with `_` (PHP manual:
/// "extlang", "variant" and "private" keys may hold arrays or be numbered
/// `variant0`...; private subtags follow an `x`).
fn compose(vm: &mut Vm, a: &mut Args) -> R<Value> {
    reset(vm);
    let arr = a.array(0);
    let get = |k: &str| arr.get_bytes(k.as_bytes()).map(|v| v.deref());
    let as_string = |vm: &mut Vm, v: &Value| -> R<Option<String>> {
        match v {
            Value::Str(s) => Ok(Some(s.to_string_lossy())),
            _ => {
                let _ = vm;
                Ok(None)
            }
        }
    };
    if let Some(g) = get("grandfathered") {
        if let Some(s) = as_string(vm, &g)? {
            return Ok(Value::from(s.as_str()));
        }
    }
    let Some(lang) = get("language") else {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #1 ($subtags) must contain a \"language\" key",
        );
    };
    let Some(lang) = as_string(vm, &lang)? else {
        fail(
            vm,
            a,
            ue::U_ILLEGAL_ARGUMENT_ERROR,
            "parameter array element is not a string",
        )?;
        return Ok(Value::Bool(false));
    };
    let mut parts = vec![lang];
    // multi-valued tags: an array under the bare key, else key0..key14
    let multi = |vm: &mut Vm, name: &str, max: usize| -> R<Result<Vec<String>, ()>> {
        let mut out = Vec::new();
        if let Some(v) = get(name) {
            match v {
                Value::Array(items) => {
                    for (_, x) in items.iter() {
                        match as_string(vm, &x.deref())? {
                            Some(s) => out.push(s),
                            None => return Ok(Err(())),
                        }
                    }
                }
                other => match as_string(vm, &other)? {
                    Some(s) => out.push(s),
                    None => return Ok(Err(())),
                },
            }
            return Ok(Ok(out));
        }
        for i in 0..max {
            if let Some(v) = get(&format!("{name}{i}")) {
                match as_string(vm, &v)? {
                    Some(s) => out.push(s),
                    None => return Ok(Err(())),
                }
            }
        }
        Ok(Ok(out))
    };
    let bad = |vm: &mut Vm, a: &Args| -> R<Value> {
        fail(
            vm,
            a,
            ue::U_ILLEGAL_ARGUMENT_ERROR,
            "parameter array element is not a string",
        )?;
        Ok(Value::Bool(false))
    };
    match multi(vm, "extlang", 3)? {
        Ok(v) => parts.extend(v),
        Err(()) => return bad(vm, a),
    }
    for k in ["script", "region"] {
        if let Some(v) = get(k) {
            match as_string(vm, &v)? {
                Some(s) => parts.push(s),
                None => return bad(vm, a),
            }
        }
    }
    match multi(vm, "variant", 15)? {
        Ok(v) => parts.extend(v),
        Err(()) => return bad(vm, a),
    }
    match multi(vm, "private", 15)? {
        Ok(v) if !v.is_empty() => {
            parts.push("x".to_string());
            parts.extend(v);
        }
        Ok(_) => {}
        Err(()) => return bad(vm, a),
    }
    Ok(Value::from(parts.join("_").as_str()))
}

fn parse_locale(vm: &mut Vm, a: &mut Args) -> R<Value> {
    reset(vm);
    let l = locale_or_default(vm, a, 0, "locale")?;
    let mut arr = PhpArray::new();
    for (k, v) in loc::parse_locale(&l) {
        arr.insert(Key::from_bytes(k.as_bytes()), Value::from(v.as_str()));
    }
    Ok(Value::Array(arr))
}

fn get_all_variants(vm: &mut Vm, a: &mut Args) -> R<Value> {
    reset(vm);
    let l = locale_or_default(vm, a, 0, "locale")?;
    let vs = loc::variants(&l);
    Ok(Value::Array(PhpArray::from_values(
        vs.iter().map(|v| Value::from(v.as_str())),
    )))
}

fn canonical_lower(s: &str) -> String {
    loc::canonicalize(s).to_ascii_lowercase()
}

fn filter_matches(vm: &mut Vm, a: &mut Args) -> R<Value> {
    reset(vm);
    let tag = locale_arg(vm, a, 0, "languageTag")?;
    let range = locale_arg(vm, a, 1, "locale")?;
    let range = if range.is_empty() {
        default_locale(vm)
    } else {
        range
    };
    let canon = a.bool_or(2, false);
    let r = if canon {
        let t = canonical_lower(&tag);
        let r = canonical_lower(&range);
        let t = t.split('@').next().unwrap_or("").to_string();
        let r = r.split('@').next().unwrap_or("").to_string();
        loc::filter_matches(&t, &r)
    } else {
        loc::filter_matches(&tag, &range)
    };
    Ok(Value::Bool(r))
}

fn canonicalize(vm: &mut Vm, a: &mut Args) -> R<Value> {
    reset(vm);
    let l = locale_arg(vm, a, 0, "locale")?;
    let l = if l.is_empty() { default_locale(vm) } else { l };
    Ok(opt_str(loc::canonicalize(&l)))
}

fn lookup(vm: &mut Vm, a: &mut Args) -> R<Value> {
    reset(vm);
    let range = locale_arg(vm, a, 1, "locale")?;
    let canon = a.bool_or(2, false);
    let def = match a.opt_str(3) {
        Some(s) => {
            if s.as_bytes().contains(&0) {
                return vm.fthrow(
                    a,
                    "ValueError",
                    "Argument #4 ($defaultLocale) must not contain any null bytes",
                );
            }
            Some(s.to_string_lossy())
        }
        None => None,
    };
    // an empty locale gives the default (bug #66289:
    // ext/intl/tests/locale_bug66289.phpt)
    if range.is_empty()
        && let Some(d) = &def
    {
        let d = if canon { canonical_lower(d) } else { d.clone() };
        return Ok(Value::from(d.as_str()));
    }
    let range = if range.is_empty() {
        default_locale(vm)
    } else {
        range
    };
    let arr = a.array(0);
    let mut tags = Vec::new();
    for (_, v) in arr.iter() {
        let v = v.deref();
        let s: PhpStr = match &v {
            Value::Str(s) => s.clone(),
            _ => vm.to_php_str(&v)?,
        };
        if s.as_bytes().contains(&0) {
            return vm.fthrow(
                a,
                "ValueError",
                "Argument #2 ($locale) must not contain any null bytes",
            );
        }
        tags.push(s.to_string_lossy());
    }
    let (cands, r) = if canon {
        let c: Vec<String> = tags.iter().map(|t| canonical_lower(t)).collect();
        (c, canonical_lower(&range))
    } else {
        (tags.clone(), range)
    };
    match loc::lookup(&cands, &r) {
        Some(i) => Ok(Value::from(cands[i].as_str())),
        None => Ok(match def {
            Some(d) => Value::from(d.as_str()),
            None => Value::Null,
        }),
    }
}

fn accept_from_http(vm: &mut Vm, a: &mut Args) -> R<Value> {
    reset(vm);
    let h = String::from_utf8_lossy(a.bytes(0)).into_owned();
    // each language range is bounded by ICU's locale ID capacity (bug
    // #72533)
    if h.split(',').any(|r| r.split(';').next().unwrap_or("").trim().len() > MAX_LOCALE_LEN) {
        fail(vm, a, ue::U_ILLEGAL_ARGUMENT_ERROR, "locale string too long")?;
        return Ok(Value::Bool(false));
    }
    match loc::accept_from_http(&h) {
        Some(l) => Ok(Value::from(l.as_str())),
        None => Ok(Value::Bool(false)),
    }
}

fn is_right_to_left(vm: &mut Vm, a: &mut Args) -> R<Value> {
    reset(vm);
    let l = locale_or_default(vm, a, 0, "locale")?;
    Ok(Value::Bool(loc::is_right_to_left(&l)))
}

fn likely(vm: &mut Vm, a: &Args, max: bool) -> R<Value> {
    reset(vm);
    let l = locale_or_default(vm, a, 0, "locale")?;
    let r = if l.len() > MAX_LOCALE_LEN {
        None
    } else if max {
        loc::add_likely(&l)
    } else {
        loc::minimize(&l)
    };
    match r {
        Some(s) => Ok(Value::from(s.as_str())),
        None => {
            fail(vm, a, ue::U_ILLEGAL_ARGUMENT_ERROR, "invalid locale")?;
            Ok(Value::Bool(false))
        }
    }
}

fn add_likely_subtags(vm: &mut Vm, a: &mut Args) -> R<Value> {
    likely(vm, a, true)
}

fn minimize_subtags(vm: &mut Vm, a: &mut Args) -> R<Value> {
    likely(vm, a, false)
}
