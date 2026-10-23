//! Locale data for the number formatters (NumberFormatter, the number
//! arguments of MessageFormatter): ICU's DecimalFormatSymbols and the CLDR
//! number patterns of a locale, its currency, and the locale resolution of
//! an ICU service object (valid / actual locale).
//!
//! The `intl` crate keeps its CLDR number tables private and only exposes
//! formatters, so the symbols and patterns are recovered by formatting
//! probe values with `intl::number::format_to_parts()` and reading the
//! typed parts back.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use intl::number::{
    CurrencyDisplay, CurrencyUsage, NumberFormatOptions, NumberPart, NumberPartType as P,
    NumberStyle, SignDisplay, UseGrouping, format_to_parts,
};

pub use crate::numdata_tables::{ACCOUNTING, AVAILABLE, REGION_CURRENCY};

/// UNumberFormatSymbol indices.
pub const SYM_DECIMAL: usize = 0;
pub const SYM_GROUPING: usize = 1;
pub const SYM_PATTERN_SEPARATOR: usize = 2;
pub const SYM_PERCENT: usize = 3;
pub const SYM_ZERO_DIGIT: usize = 4;
pub const SYM_DIGIT: usize = 5;
pub const SYM_MINUS: usize = 6;
pub const SYM_PLUS: usize = 7;
pub const SYM_CURRENCY: usize = 8;
pub const SYM_INTL_CURRENCY: usize = 9;
pub const SYM_MONETARY_SEPARATOR: usize = 10;
pub const SYM_EXPONENTIAL: usize = 11;
pub const SYM_PERMILL: usize = 12;
pub const SYM_PAD_ESCAPE: usize = 13;
pub const SYM_INFINITY: usize = 14;
pub const SYM_NAN: usize = 15;
pub const SYM_SIGNIFICANT_DIGIT: usize = 16;
pub const SYM_MONETARY_GROUPING: usize = 17;
pub const SYM_ONE_DIGIT: usize = 18;
pub const SYM_EXPONENT_MULTIPLICATION: usize = 27;
pub const SYM_APPROXIMATELY: usize = 28;
/// UNUM_FORMAT_SYMBOL_COUNT
pub const SYM_COUNT: usize = 29;

/// ICU's alias bundles (%%ALIAS): locale ID -> the bundle it stands for.
pub const ALIASES: &[(&str, &str)] = &[
    ("az_AZ", "az_Latn_AZ"),
    ("bs_BA", "bs_Latn_BA"),
    ("en_NH", "en_VU"),
    ("en_RH", "en_ZW"),
    ("ff_CM", "ff_Latn_CM"),
    ("ff_GN", "ff_Latn_GN"),
    ("ff_MR", "ff_Latn_MR"),
    ("ff_SN", "ff_Latn_SN"),
    ("in", "id"),
    ("in_ID", "id_ID"),
    ("iw", "he"),
    ("iw_IL", "he_IL"),
    ("ks_IN", "ks_Arab_IN"),
    ("kxv_IN", "kxv_Latn_IN"),
    ("mni_IN", "mni_Beng_IN"),
    ("mo", "ro"),
    ("no_NO_NY", "nn_NO"),
    ("pa_IN", "pa_Guru_IN"),
    ("pa_PK", "pa_Arab_PK"),
    ("sat_IN", "sat_Olck_IN"),
    ("sd_IN", "sd_Deva_IN"),
    ("sd_PK", "sd_Arab_PK"),
    ("sh", "sr_Latn"),
    ("sh_BA", "sr_Latn_BA"),
    ("sh_CS", "sr_Latn_RS"),
    ("sh_YU", "sr_Latn_RS"),
    ("shi_MA", "shi_Tfng_MA"),
    ("sr_BA", "sr_Cyrl_BA"),
    ("sr_CS", "sr_Cyrl_RS"),
    ("sr_ME", "sr_Latn_ME"),
    ("sr_RS", "sr_Cyrl_RS"),
    ("sr_XK", "sr_Cyrl_XK"),
    ("sr_YU", "sr_Cyrl_RS"),
    ("su_ID", "su_Latn_ID"),
    ("tl", "fil"),
    ("tl_PH", "fil_PH"),
    ("uz_AF", "uz_Arab_AF"),
    ("uz_UZ", "uz_Latn_UZ"),
    ("vai_LR", "vai_Vaii_LR"),
    ("yue_CN", "yue_Hans_CN"),
    ("yue_HK", "yue_Hant_HK"),
    ("zh_CN", "zh_Hans_CN"),
    ("zh_HK", "zh_Hant_HK"),
    ("zh_MO", "zh_Hant_MO"),
    ("zh_SG", "zh_Hans_SG"),
    ("zh_TW", "zh_Hant_TW"),
    ("kok_IN", "kok_Deva_IN"),
];

/// A simplified uloc_canonicalize(): "pt-PT" / "pt_PT.utf8" -> "pt_PT",
/// "fr_CA@euro" -> "fr_CA@currency=EUR"; subtag case fixed, keywords
/// lowercased and sorted.
pub fn canonicalize(id: &str) -> String {
    let id = id.trim();
    let (base, kw) = match id.find('@') {
        Some(i) => (&id[..i], &id[i + 1..]),
        None => (id, ""),
    };
    let base = match base.find('.') {
        Some(i) => &base[..i],
        None => base,
    };
    let mut parts: Vec<String> = Vec::new();
    for (i, p) in base.split(['_', '-']).enumerate() {
        if i == 0 {
            parts.push(p.to_ascii_lowercase());
            continue;
        }
        if p.is_empty() {
            parts.push(String::new());
            continue;
        }
        let s = if i == 1 && p.len() == 4 && p.bytes().all(|b| b.is_ascii_alphabetic()) {
            let mut s = p[..1].to_ascii_uppercase();
            s.push_str(&p[1..].to_ascii_lowercase());
            s
        } else {
            p.to_ascii_uppercase()
        };
        parts.push(s);
    }
    let mut out = parts.join("_");
    let mut kws: Vec<(String, String)> = Vec::new();
    for k in kw.split(';') {
        let k = k.trim();
        if k.is_empty() {
            continue;
        }
        match k.split_once('=') {
            Some((a, b)) => kws.push((a.trim().to_ascii_lowercase(), b.trim().to_string())),
            None => {
                if k.eq_ignore_ascii_case("euro") {
                    kws.push(("currency".into(), "EUR".into()));
                } else if k.eq_ignore_ascii_case("pinyin") {
                    kws.push(("collation".into(), "pinyin".into()));
                } else if k.eq_ignore_ascii_case("stroke") {
                    kws.push(("collation".into(), "stroke".into()));
                }
            }
        }
    }
    if !kws.is_empty() {
        kws.sort();
        out.push('@');
        let s: Vec<String> = kws.iter().map(|(a, b)| format!("{}={}", a, b)).collect();
        out.push_str(&s.join(";"));
    }
    out
}

/// The base name (no keywords) of a locale ID.
pub fn base_name(id: &str) -> &str {
    match id.find('@') {
        Some(i) => &id[..i],
        None => id,
    }
}

/// The value of keyword `key` of a locale ID.
pub fn keyword(id: &str, key: &str) -> Option<String> {
    let (_, kw) = id.split_once('@')?;
    for k in kw.split(';') {
        if let Some((a, b)) = k.split_once('=') {
            if a.trim().eq_ignore_ascii_case(key) {
                return Some(b.trim().to_string());
            }
        }
    }
    None
}

/// uloc_getISO3Language() is non-empty: the language subtag is a known
/// ISO 639 code.
pub fn has_iso3_language(id: &str) -> bool {
    let lang = id.split(['_', '-', '@', '.']).next().unwrap_or("");
    let lang = lang.to_ascii_lowercase();
    if !(2..=3).contains(&lang.len()) || !lang.bytes().all(|b| b.is_ascii_lowercase()) {
        return false;
    }
    if matches!(
        lang.as_str(),
        "in" | "iw" | "ji" | "jw" | "mo" | "sh" | "tl" | "no"
    ) {
        return true;
    }
    intl::display::language_name("en", &lang).is_some()
}

fn available(id: &str) -> bool {
    AVAILABLE.contains(&id)
}

/// The locale bundle an ICU service resolves `id` (canonicalized) to, by
/// truncation fallback (ures_open): None when nothing but root matches.
pub fn resolve_bundle(id: &str) -> Option<String> {
    let mut cur = base_name(id).to_string();
    // empty trailing subtags ("en__POSIX")
    for _ in 0..8 {
        if cur.is_empty() {
            return None;
        }
        if let Some((_, to)) = ALIASES.iter().find(|(a, _)| *a == cur) {
            cur = to.to_string();
            continue;
        }
        if available(&cur) {
            return Some(cur);
        }
        let i = cur.rfind('_')?;
        cur.truncate(i);
        while cur.ends_with('_') {
            cur.pop();
        }
    }
    None
}

/// The valid locale of a formatter opened for `id` (falls back to the
/// default locale when no bundle matches).
pub fn valid_locale(id: &str, default: &str) -> String {
    match resolve_bundle(id) {
        Some(v) => v,
        None => match resolve_bundle(&canonicalize(default)) {
            Some(v) => v,
            None => "root".to_string(),
        },
    }
}

/// The `intl` crate locale tag of an ICU locale ID (BCP 47 with the
/// numbering system keyword).
pub fn intl_tag(id: &str) -> String {
    let base = base_name(id);
    let mut t = base.replace('_', "-");
    if t.is_empty() || t == "root" {
        t = "en".to_string();
    }
    if let Some(nu) = keyword(id, "numbers") {
        t.push_str("-u-nu-");
        t.push_str(&nu.to_ascii_lowercase());
    }
    t
}

/// The language subtag of a locale ID / tag.
pub fn language(id: &str) -> String {
    id.split(['_', '-', '@'])
        .next()
        .unwrap_or("")
        .to_ascii_lowercase()
}

/// The region subtag of a locale ID, if any.
pub fn region(id: &str) -> Option<String> {
    for (i, p) in base_name(id).split(['_', '-']).enumerate() {
        if i == 0 {
            continue;
        }
        if (p.len() == 2 && p.bytes().all(|b| b.is_ascii_alphabetic()))
            || (p.len() == 3 && p.bytes().all(|b| b.is_ascii_digit()))
        {
            return Some(p.to_ascii_uppercase());
        }
    }
    None
}

/// ucurr_forLocale(): the @currency keyword, or the currency of the region.
pub fn locale_currency(id: &str) -> Option<String> {
    if let Some(c) = keyword(id, "currency") {
        return Some(c.to_ascii_uppercase());
    }
    let r = region(id)?;
    REGION_CURRENCY
        .iter()
        .find(|(k, _)| *k == r)
        .map(|(_, c)| c.to_string())
}

/// The symbols and patterns of a locale.
#[derive(Clone, Debug)]
pub struct LocaleData {
    /// The intl tag the data was read with.
    pub tag: String,
    pub symbols: Vec<String>,
    pub decimal: String,
    pub percent: String,
    pub currency: String,
    pub accounting: String,
    pub scientific: String,
}

fn opts() -> NumberFormatOptions {
    NumberFormatOptions::default()
}

fn parts(tag: &str, v: f64, o: &NumberFormatOptions) -> Vec<NumberPart> {
    format_to_parts(tag, v, o)
}

fn part_value(ps: &[NumberPart], k: P) -> Option<String> {
    ps.iter().find(|p| p.kind == k).map(|p| p.value.clone())
}

/// Quote affix literal text for a pattern.
pub fn quote_affix_literal(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        match c {
            '\'' => out.push_str("''"),
            '#' | '0'..='9' | '@' | ',' | '.' | ';' | '*' | '%' | '‰' | '¤' | '+' | '-' => {
                out.push('\'');
                out.push(c);
                out.push('\'');
            }
            _ => out.push(c),
        }
    }
    out
}

/// A pattern from the parts of a formatted positive and negative probe.
fn pattern_from_parts(pos: &[NumberPart], neg: &[NumberPart], frac: &str) -> String {
    fn affixes(ps: &[NumberPart]) -> (String, String, Vec<usize>) {
        let mut pre = String::new();
        let mut suf = String::new();
        let mut runs = Vec::new();
        let mut seen_num = false;
        for p in ps {
            let piece = match p.kind {
                P::Integer => {
                    seen_num = true;
                    runs.push(p.value.chars().count());
                    continue;
                }
                P::Group | P::Decimal | P::Fraction => {
                    seen_num = true;
                    continue;
                }
                P::MinusSign => "-".to_string(),
                P::PlusSign => "+".to_string(),
                P::PercentSign => "%".to_string(),
                P::Currency => "¤".to_string(),
                _ => quote_affix_literal(&p.value),
            };
            if seen_num {
                suf.push_str(&piece);
            } else {
                pre.push_str(&piece);
            }
        }
        (pre, suf, runs)
    }
    let (pp, ps, runs) = affixes(pos);
    let (np, ns, _) = affixes(neg);
    let n = runs.len();
    let primary = runs.last().copied().unwrap_or(3);
    let secondary = if n >= 3 { runs[n - 2] } else { primary };
    let mut num = String::from("#,");
    if secondary != primary {
        num.push_str(&"#".repeat(secondary));
        num.push(',');
    }
    num.push_str(&"#".repeat(primary.saturating_sub(1)));
    num.push('0');
    num.push_str(frac);
    let mut out = format!("{}{}{}", pp, num, ps);
    if !(np == format!("-{}", pp) && ns == ps) {
        out.push(';');
        out.push_str(&np);
        out.push_str(&num);
        out.push_str(&ns);
    }
    out
}

fn probe(tag: &str) -> LocaleData {
    let mut sy: Vec<String> = vec![String::new(); SYM_COUNT];
    let o = opts();
    let dec_pos = parts(tag, 1234567.891, &o);
    let dec_neg = parts(tag, -1234567.891, &o);
    sy[SYM_DECIMAL] = part_value(&dec_pos, P::Decimal).unwrap_or_else(|| ".".into());
    sy[SYM_GROUPING] = part_value(&dec_pos, P::Group).unwrap_or_else(|| ",".into());
    sy[SYM_PATTERN_SEPARATOR] = ";".into();
    sy[SYM_MINUS] = part_value(&dec_neg, P::MinusSign).unwrap_or_else(|| "-".into());
    let mut po = opts();
    po.sign_display = SignDisplay::Always;
    sy[SYM_PLUS] = part_value(&parts(tag, 1.0, &po), P::PlusSign).unwrap_or_else(|| "+".into());
    let mut pc = opts();
    pc.style = NumberStyle::Percent;
    let pct_pos = parts(tag, 12345.67891, &pc);
    let pct_neg = parts(tag, -12345.67891, &pc);
    sy[SYM_PERCENT] = part_value(&pct_pos, P::PercentSign).unwrap_or_else(|| "%".into());
    // digits
    let mut dg = opts();
    dg.use_grouping = UseGrouping::Never;
    let digits: Vec<char> = parts(tag, 1234567890.0, &dg)
        .iter()
        .filter(|p| p.kind == P::Integer)
        .flat_map(|p| p.value.chars().collect::<Vec<_>>())
        .collect();
    let ascii = ['1', '2', '3', '4', '5', '6', '7', '8', '9', '0'];
    let mut map = ['0'; 10];
    if digits.len() == 10 {
        for (i, a) in ascii.iter().enumerate() {
            map[(*a as u8 - b'0') as usize] = digits[i];
        }
    } else {
        for (i, m) in map.iter_mut().enumerate() {
            *m = (b'0' + i as u8) as char;
        }
    }
    sy[SYM_ZERO_DIGIT] = map[0].to_string();
    for i in 1..10 {
        sy[SYM_ONE_DIGIT + i - 1] = map[i].to_string();
    }
    sy[SYM_DIGIT] = "#".into();
    let mut so = opts();
    so.notation = intl::number::Notation::Scientific;
    sy[SYM_EXPONENTIAL] =
        part_value(&parts(tag, 1234.0, &so), P::ExponentSeparator).unwrap_or_else(|| "E".into());
    sy[SYM_PERMILL] = if matches!(map[0], '٠' | '۰') {
        "؉".into()
    } else {
        "‰".into()
    };
    sy[SYM_PAD_ESCAPE] = "*".into();
    sy[SYM_INFINITY] =
        part_value(&parts(tag, f64::INFINITY, &o), P::Infinity).unwrap_or_else(|| "∞".into());
    sy[SYM_NAN] = part_value(&parts(tag, f64::NAN, &o), P::Nan).unwrap_or_else(|| "NaN".into());
    sy[SYM_SIGNIFICANT_DIGIT] = "@".into();
    sy[SYM_EXPONENT_MULTIPLICATION] = "×".into();
    sy[SYM_APPROXIMATELY] = "~".into();
    let mut co = opts();
    co.style = NumberStyle::Currency;
    co.currency = Some("EUR");
    let cur_pos = parts(tag, 1234567.891, &co);
    let cur_neg = parts(tag, -1234567.891, &co);
    sy[SYM_MONETARY_SEPARATOR] =
        part_value(&cur_pos, P::Decimal).unwrap_or_else(|| sy[SYM_DECIMAL].clone());
    sy[SYM_MONETARY_GROUPING] =
        part_value(&cur_pos, P::Group).unwrap_or_else(|| sy[SYM_GROUPING].clone());
    sy[SYM_CURRENCY] = "¤".into();
    sy[SYM_INTL_CURRENCY] = "¤¤".into();
    let decimal = pattern_from_parts(&dec_pos, &dec_neg, ".###");
    let percent = pattern_from_parts(&pct_pos, &pct_neg, "");
    let currency = pattern_from_parts(&cur_pos, &cur_neg, ".00");
    LocaleData {
        tag: tag.to_string(),
        symbols: sy,
        decimal,
        percent,
        accounting: currency.clone(),
        currency,
        scientific: "#E0".into(),
    }
}

thread_local! {
    static CACHE: RefCell<HashMap<String, Rc<LocaleData>>> = RefCell::new(HashMap::new());
}

/// The data of the (valid) locale `valid` with the keywords of `id`.
pub fn locale_data(valid: &str, id: &str) -> Rc<LocaleData> {
    let mut key = valid.to_string();
    if let Some(nu) = keyword(id, "numbers") {
        key.push_str("@numbers=");
        key.push_str(&nu);
    }
    if let Some(d) = CACHE.with(|c| c.borrow().get(&key).cloned()) {
        return d;
    }
    let mut d = if valid == "en_US_POSIX" {
        posix_data()
    } else {
        probe(&intl_tag(&key))
    };
    // accounting
    let mut cur = valid.to_string();
    loop {
        if let Some((_, p)) = ACCOUNTING.iter().find(|(l, _)| *l == cur) {
            if !p.is_empty() {
                d.accounting = p.to_string();
            }
            break;
        }
        match cur.rfind('_') {
            Some(i) => cur.truncate(i),
            None => break,
        }
    }
    let d = Rc::new(d);
    CACHE.with(|c| c.borrow_mut().insert(key, d.clone()));
    d
}

/// en_US_POSIX: its own symbols and patterns.
fn posix_data() -> LocaleData {
    let mut d = probe("en");
    d.symbols[SYM_INFINITY] = "INF".into();
    d.symbols[SYM_PERMILL] = "0/00".into();
    d.decimal = "0.######".into();
    d.percent = "0%".into();
    d.currency = "¤ 0.00".into();
    d.accounting = "¤ 0.00".into();
    d.scientific = "0.000000E+000".into();
    d
}

/// The symbol of currency `code` in locale `tag` (ucurr_getName()).
pub fn currency_symbol(tag: &str, code: &str, display: CurrencyDisplay) -> String {
    let code_s: &'static str = intern(code);
    let mut o = opts();
    o.style = NumberStyle::Currency;
    o.currency = Some(code_s);
    o.currency_display = display;
    part_value(&parts(tag, 1.0, &o), P::Currency).unwrap_or_else(|| code.to_string())
}

/// The plural display name of currency `code` for value `v`.
pub fn currency_name(tag: &str, code: &str, v: f64) -> String {
    let code_s: &'static str = intern(code);
    let mut o = opts();
    o.style = NumberStyle::Currency;
    o.currency = Some(code_s);
    o.currency_display = CurrencyDisplay::Name;
    part_value(&parts(tag, v, &o), P::Currency).unwrap_or_else(|| code.to_string())
}

/// The fraction digits of currency `code`: its standard digits, or its CLDR
/// cash digits with `cash` (NumberFormatter::CASH_CURRENCY).
pub fn currency_digits(code: &str, cash: bool) -> i32 {
    if code == "XXX" || code.len() != 3 {
        return 2;
    }
    let code_s: &'static str = intern(code);
    let mut o = opts();
    o.style = NumberStyle::Currency;
    o.currency = Some(code_s);
    if cash {
        o.currency_usage = CurrencyUsage::Cash;
    }
    part_value(&parts("en", 1.5, &o), P::Fraction)
        .map(|f| f.chars().count() as i32)
        .unwrap_or(0)
}

/// Leak-once interning of currency codes (the intl options take
/// `&'static str`).
fn intern(s: &str) -> &'static str {
    thread_local! {
        static SET: RefCell<HashMap<String, &'static str>> = RefCell::new(HashMap::new());
    }
    SET.with(|m| {
        if let Some(v) = m.borrow().get(s) {
            return *v;
        }
        let v: &'static str = Box::leak(s.to_string().into_boxed_str());
        m.borrow_mut().insert(s.to_string(), v);
        v
    })
}

/// Common ISO 4217 codes, for matching currency symbols when parsing.
pub const COMMON_CURRENCIES: &[&str] = &[
    "USD", "EUR", "GBP", "JPY", "CHF", "CNY", "RUB", "UAH", "INR", "BRL", "CAD", "AUD", "KRW",
    "MXN", "PLN", "SEK", "NOK", "DKK", "CZK", "HUF", "TRY", "ILS", "THB", "HKD", "TWD", "SGD",
    "NZD", "ZAR",
];
