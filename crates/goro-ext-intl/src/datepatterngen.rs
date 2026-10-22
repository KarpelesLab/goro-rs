//! ext/intl/dateformat: IntlDatePatternGenerator (ICU's
//! DateTimePatternGenerator::getBestPattern() over the CLDR
//! availableFormats of the `intl` crate).

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use goro_core::value::Value;
use goro_core::vm::{NativeObjectHandlers, Vm};
use intl::datetime as dt;

use crate::dateformat::datefmt_data::{self as data, LocaleData, locale_data};
use crate::dateformat::datefmt_pattern::{self as pat, Item};
use crate::error::{self, IntlError};
use crate::{Call, Obj, obj, s, zpp};

crate::intl_functions! { pub FUNCTIONS: }

pub const METHODS: &[(&str, crate::Imp)] = &[
    ("IntlDatePatternGenerator::__construct", construct),
    ("IntlDatePatternGenerator::create", create),
    ("IntlDatePatternGenerator::getBestPattern", get_best_pattern),
];

pub const CLASSES: &[(&str, Option<crate::obj::CreateFn>)] =
    &[("IntlDatePatternGenerator", Some(create_object))];

#[derive(Clone, Debug, Default)]
pub struct Dtpg {
    /// The locale (None until constructed).
    pub locale: Option<String>,
    pub err: IntlError,
}

static HANDLERS: NativeObjectHandlers = NativeObjectHandlers {
    clone_obj: Some(clone_obj),
    ..NativeObjectHandlers::EMPTY
};

fn create_object(_vm: &mut Vm, o: &Obj) {
    obj::attach(o, Dtpg::default(), &HANDLERS);
}

fn clone_obj(vm: &mut Vm, src: &Obj, dst: &Obj) {
    let ready = obj::state::<Dtpg>(src).is_some_and(|s| s.borrow().locale.is_some());
    obj::clone_state::<Dtpg>(src, dst, |s| s.clone());
    if !ready {
        zpp::throw(vm, b"Error", "Cannot clone uninitialized IntlDatePatternGenerator");
    }
}

/// dtpg_ctor()
fn ctor(vm: &mut Vm, c: &Call, o: &Obj) -> Result<(), ()> {
    error::reset_global();
    let Some(st) = obj::state::<Dtpg>(o) else {
        return Err(());
    };
    if st.borrow().locale.is_some() {
        let mut e = st.borrow().err.clone();
        error::set_both(vm, &c.name, &mut e, error::U_ILLEGAL_ARGUMENT_ERROR, "Cannot call constructor twice");
        st.borrow_mut().err = e;
        return Err(());
    }
    let l = c.opt_str(0).unwrap_or_default();
    if l.len() > crate::common::MAX_LOCALE_LEN {
        let m = format!(
            "Locale string too long, should be no longer than {} characters",
            crate::common::MAX_LOCALE_LEN
        );
        error::set_global(vm, &c.name, error::U_ILLEGAL_ARGUMENT_ERROR, &m);
        return Err(());
    }
    let loc = crate::common::locale_or_default(vm, &l);
    st.borrow_mut().locale = Some(crate::dateformat::datefmt_ext::uloc_canonicalize(&loc));
    Ok(())
}

fn construct(vm: &mut Vm, c: &mut Call) -> Value {
    let o = c.obj();
    crate::dateformat::with_exceptions(vm, |vm| {
        let _ = ctor(vm, c, &o);
    });
    Value::Null
}

fn create(vm: &mut Vm, c: &mut Call) -> Value {
    let Some(o) = obj::object_init_ex(vm, b"intldatepatterngenerator") else {
        return Value::Null;
    };
    if ctor(vm, c, &o).is_err() {
        return Value::Null;
    }
    Value::Object(o)
}

fn get_best_pattern(vm: &mut Vm, c: &mut Call) -> Value {
    let Some(o) = c.this.clone() else {
        return Value::Null;
    };
    let Some(st) = obj::state::<Dtpg>(&o) else {
        return Value::Null;
    };
    st.borrow_mut().err.reset();
    let Some(locale) = st.borrow().locale.clone() else {
        zpp::throw(vm, b"Error", "Found unconstructed IntlDatePatternGenerator");
        return Value::Null;
    };
    let sk = c.str(0);
    let Ok(sk) = String::from_utf8(sk) else {
        let mut e = st.borrow().err.clone();
        error::set_both(
            vm,
            &c.name,
            &mut e,
            error::U_INVALID_CHAR_FOUND,
            "Skeleton is not a valid UTF-8 string",
        );
        st.borrow_mut().err = e;
        return Value::False;
    };
    error::set_global_code(error::U_ZERO_ERROR);
    s(best_pattern(&locale, &sk))
}

// ---------------------------------------------------------------------------
// DateTimePatternGenerator

/// The field type of a pattern letter (DTPG's field groups).
fn field_type(c: char) -> Option<u8> {
    Some(match c {
        'G' => 0,
        'y' | 'Y' | 'u' | 'U' | 'r' => 1,
        'Q' | 'q' => 2,
        'M' | 'L' => 3,
        'w' => 4,
        'W' => 5,
        'E' | 'e' | 'c' => 6,
        'D' => 7,
        'F' => 8,
        'd' => 9,
        'g' => 10,
        'a' | 'b' | 'B' => 11,
        'h' | 'H' | 'k' | 'K' => 12,
        'm' => 13,
        's' => 14,
        'S' => 15,
        'A' => 16,
        'z' | 'Z' | 'O' | 'v' | 'V' | 'X' | 'x' => 17,
        _ => return None,
    })
}

fn is_text(c: char, n: usize) -> bool {
    match c {
        'G' | 'a' | 'b' | 'B' | 'z' | 'v' | 'O' | 'V' => true,
        'M' | 'L' | 'E' | 'e' | 'c' | 'Q' | 'q' => n >= 3,
        _ => false,
    }
}

/// The fields of a skeleton (type -> (letter, count)), letters merged.
fn skeleton_fields(sk: &str) -> Vec<(u8, char, usize)> {
    let mut out: Vec<(u8, char, usize)> = Vec::new();
    for it in pat::compile(sk) {
        if let Item::Field(c, n) = it {
            let Some(t) = field_type(c) else {
                continue;
            };
            match out.iter_mut().find(|f| f.0 == t) {
                Some(f) => f.2 = f.2.max(n),
                None => out.push((t, c, n)),
            }
        }
    }
    out.sort_by_key(|f| f.0);
    out
}

/// The CLDR availableFormats skeletons looked up for each locale.
const SKELETONS: &[&str] = &[
    "d", "E", "Ed", "Ehm", "EHm", "Ehms", "EHms", "Gy", "GyMd", "GyMMM", "GyMMMd", "GyMMMEd", "h",
    "H", "hm", "Hm", "hms", "Hms", "hmsv", "Hmsv", "hmv", "Hmv", "M", "Md", "MEd", "MMM", "MMMd",
    "MMMEd", "MMMMd", "MMMMEd", "ms", "y", "yM", "yMd", "yMEd", "yMMM", "yMMMd", "yMMMEd", "yMMMM",
    "MMdd", "MMd", "yMM", "yMMdd", "MMMMdd", "yMMMMd", "yMMMMEEEEd", "MEEEEd", "MMMMEEEEd",
    "yMEEEEd", "yMMMEEEEd", "EEEEd", "Hmm", "hmm",
];

type Avail = Vec<(Vec<(u8, char, usize)>, String)>;

thread_local! {
    static AVAIL: RefCell<HashMap<String, Rc<Avail>>> = RefCell::new(HashMap::new());
}

fn part_sig(ps: &[dt::DateTimePart]) -> Vec<(String, String)> {
    ps.iter()
        .map(|p| (p.kind.as_str().to_string(), p.value.clone()))
        .collect()
}

/// The available patterns of a locale, recovered from intl's skeleton
/// formatting.
fn available(ld: &LocaleData) -> Rc<Avail> {
    if let Some(a) = AVAIL.with(|m| m.borrow().get(&ld.lang).cloned()) {
        return a;
    }
    let p1 = data::probe1();
    let p2 = data::probe2();
    let bogus = "GGGGGGGGGGQQQQQQ";
    let fallback = (
        part_sig(&dt::format_skeleton_to_parts(&ld.lang, &p1, bogus)),
        part_sig(&dt::format_skeleton_to_parts(&ld.lang, &p2, bogus)),
    );
    let mut v: Avail = Vec::new();
    for sk in SKELETONS {
        let a = dt::format_skeleton_to_parts(&ld.lang, &p1, sk);
        let b = dt::format_skeleton_to_parts(&ld.lang, &p2, sk);
        if (part_sig(&a), part_sig(&b)) == fallback && *sk != "yMMMd" {
            continue;
        }
        let mut p = data::pattern_from_parts(ld, &a, &b, "");
        if sk.contains('v') && !p.contains('v') {
            p = format!("{} v", p.trim_end());
        }
        v.push((skeleton_fields(sk), p));
    }
    let rc = Rc::new(v);
    AVAIL.with(|m| m.borrow_mut().insert(ld.lang.clone(), rc.clone()));
    rc
}

fn distance(req: &[(u8, char, usize)], cand: &[(u8, char, usize)]) -> u32 {
    let mut d = 0;
    for (t, c, n) in req {
        match cand.iter().find(|f| f.0 == *t) {
            None => d += 0x1000,
            Some((_, cc, cn)) => {
                if is_text(*c, *n) != is_text(*cc, *cn) {
                    d += 0x100;
                } else {
                    d += (*n as i32 - *cn as i32).unsigned_abs();
                }
                if *t == 12 && c != cc {
                    d += 0x100;
                }
            }
        }
    }
    for (t, _, _) in cand {
        if !req.iter().any(|f| f.0 == *t) {
            d += 0x10000;
        }
    }
    d
}

/// adjustFieldTypes(): the pattern's fields widened to the requested ones.
fn adjust(p: &str, req: &[(u8, char, usize)]) -> String {
    let mut out = String::new();
    for it in pat::compile(p) {
        match it {
            Item::Lit(l) => data::quote_literal(&l, &mut out),
            Item::Field(c, n) => {
                let (mut ch, mut cnt) = (c, n);
                if let Some(t) = field_type(c) {
                    if let Some((_, rc, rn)) = req.iter().find(|f| f.0 == t) {
                        if t == 1 {
                            ch = *rc;
                        }
                        // hours, minutes and seconds keep the pattern's width;
                        // numeric fields are only widened
                        if !matches!(t, 12..=14) && is_text(c, n) == is_text(*rc, *rn) {
                            cnt = if is_text(c, n) { *rn } else { n.max(*rn) };
                        }
                    }
                }
                for _ in 0..cnt {
                    out.push(ch);
                }
            }
        }
    }
    out
}

/// getBestRaw() + adjustFieldTypes() for one part (date or time fields).
fn best_part(ld: &LocaleData, req: &[(u8, char, usize)]) -> String {
    if req.is_empty() {
        return String::new();
    }
    let avail = available(ld);
    let mut best: Option<(u32, &String, &Vec<(u8, char, usize)>)> = None;
    for (f, p) in avail.iter() {
        let d = distance(req, f);
        if best.is_none_or(|(bd, _, _)| d < bd) {
            best = Some((d, p, f));
        }
    }
    let Some((_, p, f)) = best else {
        return String::new();
    };
    let mut out = adjust(p, req);
    // fields the best match lacks are appended
    for (t, c, n) in req {
        if !f.iter().any(|x| x.0 == *t) {
            out.push(' ');
            for _ in 0..*n {
                out.push(*c);
            }
        }
    }
    out
}

/// DateTimePatternGenerator::getBestPattern()
pub fn best_pattern(locale: &str, skeleton: &str) -> String {
    let ld = locale_data(locale);
    // the locale's hour cycle for 'j' / 'C' / 'J'
    let hour_char = ld.time[3]
        .chars()
        .find(|c| matches!(c, 'h' | 'H' | 'k' | 'K'))
        .unwrap_or('h');
    let mut sk = String::new();
    let mut need_ampm = false;
    for ch in skeleton.chars() {
        match ch {
            'j' | 'C' => {
                sk.push(hour_char);
                need_ampm |= matches!(hour_char, 'h' | 'K');
            }
            'J' => sk.push(hour_char),
            c => sk.push(c),
        }
    }
    if need_ampm && !sk.contains(['a', 'b', 'B']) {
        sk.push('a');
    }
    let fields = skeleton_fields(&sk);
    // a pattern for the whole skeleton
    let whole: Vec<_> = fields.iter().copied().filter(|f| f.0 != 11).collect();
    let avail = available(&ld);
    if let Some((_, p)) = avail
        .iter()
        .map(|(f, p)| (distance(&whole, f), p))
        .filter(|(d, _)| *d < 0x1000)
        .min_by_key(|(d, _)| *d)
    {
        return adjust(p, &whole);
    }
    let date: Vec<_> = fields.iter().copied().filter(|f| f.0 <= 10).collect();
    // the day period is implied by the hour field
    let time: Vec<_> = fields.iter().copied().filter(|f| f.0 > 11).collect();
    let dp = best_part(&ld, &date);
    let tp = best_part(&ld, &time);
    match (dp.is_empty(), tp.is_empty()) {
        (_, true) => dp,
        (true, false) => tp,
        (false, false) => {
            let month = date.iter().find(|f| f.0 == 3).map(|f| f.2).unwrap_or(0);
            let weekday = date.iter().any(|f| f.0 == 6);
            let idx = match month {
                4.. if weekday => 0,
                4.. => 1,
                3 => 2,
                _ => 3,
            };
            ld.glue[idx].replace("{1}", &dp).replace("{0}", &tp)
        }
    }
}
