//! intl_convert.c and other shared helpers: UTF-8 / UTF-16 conversion,
//! the default locale.

use goro_core::vm::Vm;

/// Whether `s` is well-formed UTF-8.
pub fn utf8_valid(s: &[u8]) -> bool {
    std::str::from_utf8(s).is_ok()
}

/// intl_convert_utf8_to_utf16(): Err(U_INVALID_CHAR_FOUND) on ill-formed
/// input.
pub fn to_utf16(s: &[u8]) -> Result<Vec<u16>, i32> {
    match std::str::from_utf8(s) {
        Ok(t) => Ok(t.encode_utf16().collect()),
        Err(_) => Err(crate::error::U_INVALID_CHAR_FOUND),
    }
}

/// intl_convert_utf16_to_utf8(): unpaired surrogates are an error
/// (U_INVALID_CHAR_FOUND).
pub fn from_utf16(s: &[u16]) -> Result<Vec<u8>, i32> {
    match String::from_utf16(s) {
        Ok(t) => Ok(t.into_bytes()),
        Err(_) => Err(crate::error::U_INVALID_CHAR_FOUND),
    }
}

/// UTF-16 to UTF-8 with U+FFFD for unpaired surrogates.
pub fn from_utf16_lossy(s: &[u16]) -> Vec<u8> {
    String::from_utf16_lossy(s).into_bytes()
}

/// The locale ICU reports by default (uloc_getDefault()), from the
/// environment as ICU reads it.
pub fn icu_default_locale() -> String {
    let mut v = String::new();
    for k in ["LC_ALL", "LC_MESSAGES", "LANG"] {
        if let Ok(x) = std::env::var(k) {
            if !x.is_empty() {
                v = x;
                break;
            }
        }
    }
    let base = v.split(['.', '@']).next().unwrap_or("").to_string();
    if base.is_empty() || base == "C" || base == "POSIX" {
        return "en_US_POSIX".to_string();
    }
    base.replace('-', "_")
}

/// intl_locale_get_default(): intl.default_locale, or ICU's default.
pub fn default_locale(vm: &Vm) -> String {
    match vm.constants.get(b"intl.default_locale".as_slice()) {
        Some(v) => {
            let s = v.to_php_string();
            if s.as_bytes().is_empty() {
                icu_default_locale()
            } else {
                s.to_string_lossy()
            }
        }
        None => icu_default_locale(),
    }
}

/// A locale argument: the default locale when empty.
pub fn locale_or_default(vm: &Vm, loc: &[u8]) -> String {
    if loc.is_empty() {
        default_locale(vm)
    } else {
        String::from_utf8_lossy(loc).into_owned()
    }
}

/// INTL_MAX_LOCALE_LEN
pub const MAX_LOCALE_LEN: usize = 156;

/// The UTF-8 sequence starting at byte `i` of a valid UTF-8 string:
/// (code point, length).
pub fn decode_at(s: &[u8], i: usize) -> (u32, usize) {
    let b = s[i];
    let (len, init) = if b < 0x80 {
        (1, b as u32)
    } else if b < 0xE0 {
        (2, (b & 0x1F) as u32)
    } else if b < 0xF0 {
        (3, (b & 0x0F) as u32)
    } else {
        (4, (b & 0x07) as u32)
    };
    let mut c = init;
    for k in 1..len {
        c = (c << 6) | (s.get(i + k).copied().unwrap_or(0x80) & 0x3F) as u32;
    }
    (c, len)
}

/// Append code point `c` as UTF-8.
pub fn push_utf8(out: &mut Vec<u8>, c: u32) {
    match char::from_u32(c) {
        Some(ch) => {
            let mut b = [0u8; 4];
            out.extend_from_slice(ch.encode_utf8(&mut b).as_bytes());
        }
        None => {
            // a surrogate code point (CESU-style 3-byte form)
            out.push(0xE0 | (c >> 12) as u8);
            out.push(0x80 | ((c >> 6) & 0x3F) as u8);
            out.push(0x80 | (c & 0x3F) as u8);
        }
    }
}

/// An ICU locale ID ("de_DE@collation=phonebook", "sr_Latn_RS",
/// "en_US_POSIX") as an ICU4X locale (BCP 47); `und` when it does not parse.
pub fn icu4x_locale(id: &str) -> icu_locale_core::Locale {
    let (base, kw) = match id.split_once('@') {
        Some((b, k)) => (b, k),
        None => (id, ""),
    };
    let mut tags: Vec<String> = Vec::new();
    for (i, p) in base.split(['_', '-']).enumerate() {
        if p.is_empty() {
            continue;
        }
        let l = p.to_ascii_lowercase();
        if i == 0 {
            if l == "root"
                || !(2..=8).contains(&l.len())
                || !l.bytes().all(|b| b.is_ascii_alphabetic())
            {
                tags.push("und".to_string());
            } else {
                tags.push(l);
            }
            continue;
        }
        tags.push(l);
    }
    if tags.is_empty() {
        tags.push("und".to_string());
    }
    let mut ext: Vec<(String, String)> = Vec::new();
    for pair in kw.split(';') {
        let Some((k, v)) = pair.split_once('=') else {
            continue;
        };
        let k = k.trim().to_ascii_lowercase();
        let v = v.trim().to_ascii_lowercase();
        let (bk, bv) = match k.as_str() {
            "collation" => (
                "co",
                match v.as_str() {
                    "phonebook" => "phonebk".to_string(),
                    "traditional" => "trad".to_string(),
                    "dictionary" => "dict".to_string(),
                    "gb2312han" => "gb2312".to_string(),
                    _ => v.clone(),
                },
            ),
            "calendar" => (
                "ca",
                match v.as_str() {
                    "gregorian" => "gregory".to_string(),
                    "ethiopic-amete-alem" => "ethioaa".to_string(),
                    _ => v.clone(),
                },
            ),
            "numbers" => ("nu", v.clone()),
            "currency" => ("cu", v.clone()),
            "colnumeric" => (
                "kn",
                if v == "yes" {
                    "true".into()
                } else {
                    "false".into()
                },
            ),
            "colcasefirst" => ("kf", v.clone()),
            "colstrength" => (
                "ks",
                match v.as_str() {
                    "primary" => "level1",
                    "secondary" => "level2",
                    "tertiary" => "level3",
                    "quaternary" => "level4",
                    "identical" => "identic",
                    _ => continue,
                }
                .to_string(),
            ),
            "colalternate" => (
                "ka",
                match v.as_str() {
                    "shifted" => "shifted",
                    _ => "noignore",
                }
                .to_string(),
            ),
            "colcaselevel" | "colbackwards" | "colnormalization" => (
                match k.as_str() {
                    "colcaselevel" => "kc",
                    "colbackwards" => "kb",
                    _ => "kk",
                },
                if v == "yes" || v == "on" {
                    "true".into()
                } else {
                    "false".into()
                },
            ),
            "hours" => ("hc", v.clone()),
            _ => continue,
        };
        ext.push((bk.to_string(), bv));
    }
    ext.sort();
    let mut s = tags.join("-");
    if !ext.is_empty() {
        s.push_str("-u");
        for (k, v) in &ext {
            s.push('-');
            s.push_str(k);
            for part in v.split(['-', '_']) {
                s.push('-');
                s.push_str(part);
            }
        }
    }
    if let Ok(l) = icu_locale_core::Locale::try_from_str(&s) {
        return l;
    }
    // drop what does not parse, from the end
    let mut t = tags.clone();
    while t.len() > 1 {
        t.pop();
        if let Ok(l) = icu_locale_core::Locale::try_from_str(&t.join("-")) {
            return l;
        }
    }
    icu_locale_core::Locale::UNKNOWN
}

/// A collator (from ICU4X's compiled data) for an ICU locale ID.
/// The collator options a locale's -u- keywords (or @colstrength=...
/// keywords) ask for.
pub fn collator_options(loc: &icu_locale_core::Locale) -> icu_collator::options::CollatorOptions {
    use icu_collator::options::{AlternateHandling, CaseLevel, Strength};
    let mut opts = icu_collator::options::CollatorOptions::default();
    for (k, v) in loc.extensions.unicode.keywords.iter() {
        let v = v.to_string();
        match (k.as_str(), v.as_str()) {
            ("ks", "level1") => opts.strength = Some(Strength::Primary),
            ("ks", "level2") => opts.strength = Some(Strength::Secondary),
            ("ks", "level3") => opts.strength = Some(Strength::Tertiary),
            ("ks", "level4") => opts.strength = Some(Strength::Quaternary),
            ("ks", "identic") => opts.strength = Some(Strength::Identical),
            ("ka", "shifted") => opts.alternate_handling = Some(AlternateHandling::Shifted),
            ("ka", "noignore") => opts.alternate_handling = Some(AlternateHandling::NonIgnorable),
            ("kc", "true" | "yes" | "") => opts.case_level = Some(CaseLevel::On),
            ("kc", "false" | "no") => opts.case_level = Some(CaseLevel::Off),
            _ => {}
        }
    }
    opts
}

/// Whether ICU can open a service object for locale ID `id`: its language
/// subtag must fit ULOC_LANG_CAPACITY.
pub fn locale_id_ok(id: &str) -> bool {
    let lang = id.split(['_', '-', '@', '.']).next().unwrap_or("");
    lang.len() < 12
}

/// A collator (from ICU4X's compiled data) for an ICU locale ID, with
/// `strength` overriding the locale's; Err(UErrorCode) when ICU would fail.
pub fn collator(
    id: &str,
    strength: Option<icu_collator::options::Strength>,
) -> Result<icu_collator::CollatorBorrowed<'static>, i32> {
    if !locale_id_ok(id) {
        return Err(crate::error::U_ILLEGAL_ARGUMENT_ERROR);
    }
    let loc = icu4x_locale(id);
    let mut opts = collator_options(&loc);
    if strength.is_some() {
        opts.strength = strength;
    }
    icu_collator::Collator::try_new((&loc).into(), opts)
        .or_else(|_| icu_collator::Collator::try_new(Default::default(), opts))
        .map_err(|_| crate::error::U_INTERNAL_PROGRAM_ERROR)
}
