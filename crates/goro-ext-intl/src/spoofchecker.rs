//! ext/intl/spoofchecker: Spoofchecker (ICU's uspoof, UTS #39) over intl's
//! confusable skeletons and script data: restriction levels, mixed
//! numbers, invisible characters, hidden overlays, allowed characters /
//! locales and confusability.

use std::rc::Rc;

use goro_core::value::Value;
use goro_core::vm::{NativeObjectHandlers, Vm};
use intl::unicode as un;
use intl::unicode::{Script, ScriptExtensions};

use crate::error::IntlError;
use crate::uset::{self, USet};
use crate::zpp::Obj;
use crate::{Call, obj};

const SINGLE_SCRIPT_CONFUSABLE: i64 = 1;
const MIXED_SCRIPT_CONFUSABLE: i64 = 2;
const WHOLE_SCRIPT_CONFUSABLE: i64 = 4;
const RESTRICTION_LEVEL: i64 = 16;
const INVISIBLE: i64 = 32;
const CHAR_LIMIT: i64 = 64;
const MIXED_NUMBERS: i64 = 128;
const HIDDEN_OVERLAY: i64 = 256;
const ALL_CHECKS: i64 = 0xFFFF;
const AUX_INFO: i64 = 0x4000_0000;

const ASCII: i64 = 0x1000_0000;
const SINGLE_SCRIPT_RESTRICTIVE: i64 = 0x2000_0000;
const HIGHLY_RESTRICTIVE: i64 = 0x3000_0000;
const MODERATELY_RESTRICTIVE: i64 = 0x4000_0000;
const MINIMALLY_RESTRICTIVE: i64 = 0x5000_0000;
const UNRESTRICTIVE: i64 = 0x6000_0000;

#[derive(Clone)]
pub struct Spoof {
    checks: i64,
    level: i64,
    /// None: all characters allowed.
    allowed: Option<Rc<USet>>,
    pub err: IntlError,
}

impl Default for Spoof {
    fn default() -> Spoof {
        Spoof {
            checks: ALL_CHECKS,
            level: HIGHLY_RESTRICTIVE,
            allowed: None,
            err: IntlError::default(),
        }
    }
}

/// An augmented script set (UTS #39 §5.1); None = all scripts.
#[derive(Clone, PartialEq, Debug)]
enum Tok {
    Jpan,
    Kore,
    Hanb,
    Scr(Script),
}

fn augmented(c: char) -> Option<Vec<Tok>> {
    let scx = un::script_extensions(c);
    if matches!(
        scx,
        ScriptExtensions::Single(Script::Common) | ScriptExtensions::Single(Script::Inherited)
    ) {
        return None;
    }
    let mut out = Vec::new();
    let mut push = |t: Tok| {
        if !out.contains(&t) {
            out.push(t);
        }
    };
    for s in scx.iter() {
        match s {
            Script::Han => {
                push(Tok::Jpan);
                push(Tok::Kore);
                push(Tok::Hanb);
            }
            Script::Hiragana | Script::Katakana => push(Tok::Jpan),
            Script::Hangul => push(Tok::Kore),
            Script::Bopomofo => push(Tok::Hanb),
            o => push(Tok::Scr(o)),
        }
    }
    Some(out)
}

/// getResolvedScriptSet(), optionally ignoring a script: None = all
/// scripts (no constraint).
fn resolved(s: &str, without: Option<Script>) -> Option<Vec<Tok>> {
    let mut acc: Option<Vec<Tok>> = None;
    for c in s.chars() {
        if without.is_some_and(|w| un::script_extensions(c).contains(w)) {
            continue;
        }
        let Some(aug) = augmented(c) else { continue };
        acc = Some(match acc {
            None => aug,
            Some(prev) => prev.into_iter().filter(|t| aug.contains(t)).collect(),
        });
    }
    acc
}

fn non_empty(r: &Option<Vec<Tok>>) -> bool {
    r.as_ref().is_none_or(|v| !v.is_empty())
}

impl Spoof {
    fn allowed(&self, c: char) -> bool {
        self.allowed.as_ref().is_none_or(|a| a.has(c))
    }

    /// getRestrictionLevel()
    fn restriction_level(&self, s: &str) -> i64 {
        if !s.chars().all(|c| self.allowed(c)) {
            return UNRESTRICTIVE;
        }
        if s.chars().all(|c| (c as u32) < 0x7F) {
            return ASCII;
        }
        if non_empty(&resolved(s, None)) {
            return SINGLE_SCRIPT_RESTRICTIVE;
        }
        let nl = resolved(s, Some(Script::Latin));
        if let Some(v) = &nl {
            if v.iter().any(|t| matches!(t, Tok::Jpan | Tok::Kore | Tok::Hanb)) {
                return HIGHLY_RESTRICTIVE;
            }
        }
        if non_empty(&nl)
            && !nl.as_ref().is_some_and(|v| {
                v.iter().any(|t| {
                    matches!(
                        t,
                        Tok::Scr(Script::Cyrillic | Script::Greek | Script::Cherokee)
                    )
                })
            })
        {
            return MODERATELY_RESTRICTIVE;
        }
        MINIMALLY_RESTRICTIVE
    }

    /// uspoof_check2()
    fn check(&self, s: &str) -> i64 {
        let mut r = 0;
        if self.checks & RESTRICTION_LEVEL != 0 && self.restriction_level(s) > self.level {
            r |= RESTRICTION_LEVEL;
        }
        if self.checks & MIXED_NUMBERS != 0 {
            let mut zeros: Vec<u32> = Vec::new();
            for c in s.chars() {
                if un::general_category(c) == un::GeneralCategory::DecimalNumber {
                    let v = un::numeric_value(c).and_then(|n| n.to_i64()).unwrap_or(0) as u32;
                    let z = c as u32 - v;
                    if !zeros.contains(&z) {
                        zeros.push(z);
                    }
                }
            }
            if zeros.len() > 1 {
                r |= MIXED_NUMBERS;
            }
        }
        if self.checks & HIDDEN_OVERLAY != 0 && hidden_overlay(s) {
            r |= HIDDEN_OVERLAY;
        }
        if self.checks & CHAR_LIMIT != 0 && !s.chars().all(|c| self.allowed(c)) {
            r |= CHAR_LIMIT;
        }
        if self.checks & INVISIBLE != 0 && invisible(s) {
            r |= INVISIBLE;
        }
        r
    }
}

pub fn is_soft_dotted(c: char) -> bool {
    matches!(
        c as u32,
        0x69 | 0x6A
            | 0x12F
            | 0x249
            | 0x268
            | 0x29D
            | 0x2B2
            | 0x3F3
            | 0x456
            | 0x458
            | 0x1D62
            | 0x1D96
            | 0x1DA4
            | 0x1DA8
            | 0x1E2D
            | 0x1ECB
            | 0x2071
            | 0x2148
            | 0x2149
            | 0x2C7C
            | 0x1D422
            | 0x1D423
            | 0x1D456
            | 0x1D457
            | 0x1D48A
            | 0x1D48B
            | 0x1D4BE
            | 0x1D4BF
            | 0x1D4F2
            | 0x1D4F3
            | 0x1D526
            | 0x1D527
            | 0x1D55A
            | 0x1D55B
            | 0x1D58E
            | 0x1D58F
            | 0x1D5C2
            | 0x1D5C3
            | 0x1D5F6
            | 0x1D5F7
            | 0x1D62A
            | 0x1D62B
            | 0x1D65E
            | 0x1D65F
            | 0x1D692
            | 0x1D693
            | 0x1DF1A
            | 0x1E04C
            | 0x1E04D
            | 0x1E068
    )
}

fn dot_lead_nolookup(c: char) -> bool {
    matches!(c, 'i' | 'j' | 'ı' | 'ȷ' | 'l') || is_soft_dotted(c)
}

fn dot_lead(c: char) -> bool {
    if dot_lead_nolookup(c) {
        return true;
    }
    let sk = un::spoof::skeleton(&c.to_string());
    sk.chars()
        .last()
        .is_some_and(|f| f != c && dot_lead_nolookup(f))
}

/// findHiddenOverlay()
fn hidden_overlay(s: &str) -> bool {
    let mut lead = false;
    for c in s.chars() {
        if lead && c == '\u{307}' {
            return true;
        }
        let cc = un::canonical_combining_class(c);
        if cc == 0 || cc == 230 {
            lead = dot_lead(c);
        }
    }
    false
}

/// The INVISIBLE check: a nonspacing mark repeated in a mark sequence.
fn invisible(s: &str) -> bool {
    let mut first: Option<char> = None;
    let mut multiple = false;
    let mut seen: Vec<char> = Vec::new();
    for c in un::nfd(s.chars()) {
        if un::general_category(c) != un::GeneralCategory::NonspacingMark {
            first = None;
            if multiple {
                seen.clear();
                multiple = false;
            }
            continue;
        }
        let Some(f) = first else {
            first = Some(c);
            continue;
        };
        if !multiple {
            seen.push(f);
            multiple = true;
        }
        if seen.contains(&c) {
            return true;
        }
        seen.push(c);
    }
    false
}

/// uscript_getCode() for a locale: its scripts.
fn locale_scripts(loc: &str) -> Vec<Script> {
    let from = |lang: &str, script: Option<&str>| -> Vec<Script> {
        match lang {
            "ja" => return vec![Script::Katakana, Script::Hiragana, Script::Han],
            "ko" => return vec![Script::Hangul, Script::Han],
            _ => {}
        }
        if lang == "zh" && script == Some("Hant") {
            return vec![Script::Han, Script::Bopomofo];
        }
        match script {
            Some("Hans" | "Hant") => vec![Script::Han],
            Some(s) => uset::script_lookup(s).into_iter().collect(),
            None => Vec::new(),
        }
    };
    let base = loc.split('@').next().unwrap_or("");
    let parts: Vec<&str> = base.split(['_', '-']).collect();
    let lang = parts.first().map(|l| l.to_ascii_lowercase()).unwrap_or_default();
    let script = parts.get(1).filter(|p| p.len() == 4).map(|s| {
        let mut t = s.to_ascii_lowercase();
        t[..1].make_ascii_uppercase();
        t
    });
    let r = from(&lang, script.as_deref());
    if !r.is_empty() {
        return r;
    }
    // likely subtags (with the und_REGION fallback)
    let Ok(l) = intl::locale::Locale::parse(&base.replace('_', "-")) else {
        return Vec::new();
    };
    let mut m = l.maximize();
    if m.script.is_none() {
        if let Some(reg) = &l.region {
            if let Ok(u) = intl::locale::Locale::parse(&format!("und-{}", reg)) {
                m.script = u.maximize().script;
            }
        }
    }
    from(&m.language, m.script.as_deref())
}

// ---------------------------------------------------------------------------
// The class

pub static HANDLERS: NativeObjectHandlers = NativeObjectHandlers {
    clone_obj: Some(clone_obj),
    ..NativeObjectHandlers::EMPTY
};

fn create(_vm: &mut Vm, o: &Obj) {
    obj::attach(o, Spoof::default(), &HANDLERS);
}

fn clone_obj(_vm: &mut Vm, src: &Obj, dst: &Obj) {
    obj::clone_state::<Spoof>(src, dst, |s| Spoof {
        err: IntlError::default(),
        ..s.clone()
    });
}

crate::intl_functions! { pub FUNCTIONS: }

pub const METHODS: &[(&str, crate::Imp)] = &[
    ("Spoofchecker::__construct", construct),
    ("Spoofchecker::isSuspicious", is_suspicious),
    ("Spoofchecker::areConfusable", are_confusable),
    ("Spoofchecker::setAllowedLocales", set_allowed_locales),
    ("Spoofchecker::setChecks", set_checks),
    ("Spoofchecker::setRestrictionLevel", set_restriction_level),
    ("Spoofchecker::setAllowedChars", set_allowed_chars),
];

pub const CLASSES: &[(&str, Option<crate::obj::CreateFn>)] = &[("Spoofchecker", Some(create))];

fn state(c: &Call) -> Option<std::rc::Rc<std::cell::RefCell<Spoof>>> {
    let st = obj::state::<Spoof>(&c.obj())?;
    st.borrow_mut().err.reset();
    Some(st)
}

fn warn(vm: &mut Vm, c: &Call, code: i32) {
    let m = format!(
        "{}(): ({}) {}",
        c.name,
        code,
        crate::error::error_name(code as i64)
    );
    vm.emit_warning(&m);
}

fn construct(_vm: &mut Vm, c: &mut Call) -> Value {
    if let Some(st) = state(c) {
        *st.borrow_mut() = Spoof::default();
    }
    Value::Null
}

fn is_suspicious(vm: &mut Vm, c: &mut Call) -> Value {
    let text = c.str(0);
    let Some(st) = state(c) else {
        return Value::Null;
    };
    let t = String::from_utf8_lossy(&text).into_owned();
    let r = st.borrow().check(&t);
    if c.passed(1) {
        c.set_ref(vm, 1, Value::Long(r));
    }
    crate::bool_v(r != 0)
}

fn are_confusable(vm: &mut Vm, c: &mut Call) -> Value {
    let a = String::from_utf8_lossy(&c.str(0)).into_owned();
    let b = String::from_utf8_lossy(&c.str(1)).into_owned();
    let Some(st) = state(c) else {
        return Value::Null;
    };
    let checks = st.borrow().checks;
    if checks & (SINGLE_SCRIPT_CONFUSABLE | MIXED_SCRIPT_CONFUSABLE | WHOLE_SCRIPT_CONFUSABLE) == 0 {
        st.borrow_mut().err.code = crate::error::U_INVALID_STATE_ERROR;
        warn(vm, c, crate::error::U_INVALID_STATE_ERROR);
        return Value::True;
    }
    let mut r = 0;
    if un::spoof::skeleton(&a) == un::spoof::skeleton(&b) {
        let (ra, rb) = (resolved(&a, None), resolved(&b, None));
        let inter = match (&ra, &rb) {
            (None, _) | (_, None) => {
                let other = ra.as_ref().or(rb.as_ref());
                other.is_none_or(|v| !v.is_empty())
            }
            (Some(x), Some(y)) => x.iter().any(|t| y.contains(t)),
        };
        if inter {
            r |= SINGLE_SCRIPT_CONFUSABLE;
        } else {
            r |= MIXED_SCRIPT_CONFUSABLE;
            if non_empty(&ra) && non_empty(&rb) {
                r |= WHOLE_SCRIPT_CONFUSABLE;
            }
        }
        r &= checks;
    }
    if c.passed(2) {
        c.set_ref(vm, 2, Value::Long(r));
    }
    crate::bool_v(r != 0)
}

fn set_allowed_locales(_vm: &mut Vm, c: &mut Call) -> Value {
    let list = String::from_utf8_lossy(&c.str(0)).into_owned();
    let Some(st) = state(c) else {
        return Value::Null;
    };
    let mut scripts: Vec<Script> = Vec::new();
    let mut count = 0;
    for loc in list.split(',') {
        let loc = loc.trim_matches(' ');
        if loc.is_empty() {
            break;
        }
        count += 1;
        for s in locale_scripts(loc) {
            if !scripts.contains(&s) {
                scripts.push(s);
            }
        }
    }
    let mut b = st.borrow_mut();
    if count == 0 {
        b.allowed = None;
        b.checks &= !CHAR_LIMIT;
        return Value::Null;
    }
    let mut sets: Vec<USet> = scripts.into_iter().map(USet::Script).collect();
    sets.push(USet::Script(Script::Common));
    sets.push(USet::Script(Script::Inherited));
    b.allowed = Some(Rc::new(USet::Union(sets)));
    b.checks |= CHAR_LIMIT;
    Value::Null
}

fn set_checks(vm: &mut Vm, c: &mut Call) -> Value {
    let checks = c.long(0);
    let Some(st) = state(c) else {
        return Value::Null;
    };
    if checks & !(ALL_CHECKS | AUX_INFO) != 0 {
        st.borrow_mut().err.code = crate::error::U_ILLEGAL_ARGUMENT_ERROR;
        warn(vm, c, crate::error::U_ILLEGAL_ARGUMENT_ERROR);
        return Value::Null;
    }
    st.borrow_mut().checks = checks;
    Value::Null
}

fn set_restriction_level(vm: &mut Vm, c: &mut Call) -> Value {
    let level = c.long(0);
    let Some(st) = state(c) else {
        return Value::Null;
    };
    if !matches!(
        level,
        ASCII
            | SINGLE_SCRIPT_RESTRICTIVE
            | HIGHLY_RESTRICTIVE
            | MODERATELY_RESTRICTIVE
            | MINIMALLY_RESTRICTIVE
            | UNRESTRICTIVE
    ) {
        crate::zpp::value_error(
            vm,
            &c.name,
            1,
            "level",
            "must be one of Spoofchecker::ASCII, Spoofchecker::SINGLE_SCRIPT_RESTRICTIVE, Spoofchecker::HIGHLY_RESTRICTIVE, Spoofchecker::MODERATELY_RESTRICTIVE, Spoofchecker::MINIMALLY_RESTRICTIVE, or Spoofchecker::UNRESTRICTIVE",
        );
        return Value::Null;
    }
    st.borrow_mut().level = level;
    Value::Null
}

fn set_allowed_chars(vm: &mut Vm, c: &mut Call) -> Value {
    let pat = c.str(0);
    let opt = c.long(1);
    let Some(st) = state(c) else {
        return Value::Null;
    };
    if pat.first() != Some(&b'[') || pat.last() != Some(&b']') {
        crate::zpp::value_error(
            vm,
            &c.name,
            1,
            "pattern",
            "must be a valid regular expression character set pattern",
        );
        return Value::Null;
    }
    let Ok(p) = std::str::from_utf8(&pat) else {
        let m = format!(
            "string conversion to unicode encoding failed ({}) {}",
            crate::error::U_INVALID_CHAR_FOUND,
            crate::error::error_name(crate::error::U_INVALID_CHAR_FOUND as i64)
        );
        crate::zpp::value_error(vm, &c.name, 1, "pattern", &m);
        return Value::Null;
    };
    if opt != 0 && opt != 1 && opt != 3 && opt != 5 && opt != 7 {
        crate::zpp::value_error(
            vm,
            &c.name,
            2,
            "patternOptions",
            "must be a valid pattern option, 0 or (SpoofChecker::IGNORE_SPACE|(<none> or SpoofChecker::USET_CASE_INSENSITIVE or SpoofChecker::USET_ADD_CASE_MAPPINGS or SpoofChecker::USET_SIMPLE_CASE_INSENSITIVE))",
        );
        return Value::Null;
    }
    let chars: Vec<char> = p.chars().collect();
    let mut pos = 0;
    let set = match uset::parse(&chars, &mut pos, &|_| None) {
        Ok(s) if pos == chars.len() => s,
        _ => {
            let m = format!(
                "must be a valid regular expression character set pattern ({}) {}",
                crate::translit_rules::U_MALFORMED_SET,
                crate::error::error_name(crate::translit_rules::U_MALFORMED_SET as i64)
            );
            crate::zpp::value_error(vm, &c.name, 1, "pattern", &m);
            return Value::Null;
        }
    };
    let set = if opt & 6 != 0 {
        // case closure
        let r = set.ranges();
        let mut extra: Vec<(u32, u32)> = Vec::new();
        for (a, b) in &r {
            for cp in *a..=*b {
                let Some(ch) = char::from_u32(cp) else { continue };
                for v in un::to_lowercase(ch)
                    .chain(un::to_uppercase(ch))
                    .chain(un::to_titlecase(ch))
                    .chain(un::case_fold(ch))
                {
                    extra.push((v as u32, v as u32));
                }
            }
        }
        let mut all = r;
        all.extend(extra);
        USet::Ranges(all)
    } else {
        set
    };
    let mut b = st.borrow_mut();
    b.allowed = Some(Rc::new(set));
    b.checks |= CHAR_LIMIT;
    Value::Null
}
