//! EXSLT functions (exslt.org): common, dates-and-times (the parts reading a
//! date), math, sets and strings. Results checked against test
//! ext/xsl/tests/xslt010_gt10129.phpt.

use super::transform::Tx;
use crate::tree::{Arena, DocKind, NodeId};
use crate::xpath::{self, CallContext, Value, XPathError, XPathErrorKind};

const COMMON: &str = "http://exslt.org/common";
const DATES: &str = "http://exslt.org/dates-and-times";
const MATH: &str = "http://exslt.org/math";
const SETS: &str = "http://exslt.org/sets";
const STRINGS: &str = "http://exslt.org/strings";
const SAXON: &str = "http://icl.com/saxon";

/// Whether `{ns}local` is implemented here.
pub(super) fn is_known(ns: &str, local: &str) -> bool {
    match ns {
        COMMON => matches!(local, "node-set" | "object-type"),
        SAXON => matches!(local, "node-set"),
        DATES => matches!(
            local,
            "date-time"
                | "date"
                | "time"
                | "year"
                | "leap-year"
                | "month-in-year"
                | "month-name"
                | "month-abbreviation"
                | "week-in-year"
                | "day-in-year"
                | "day-in-month"
                | "day-of-week-in-month"
                | "day-in-week"
                | "day-name"
                | "day-abbreviation"
                | "hour-in-day"
                | "minute-in-hour"
                | "second-in-minute"
        ),
        MATH => matches!(
            local,
            "min"
                | "max"
                | "highest"
                | "lowest"
                | "abs"
                | "sqrt"
                | "power"
                | "log"
                | "exp"
                | "sin"
                | "cos"
                | "tan"
                | "asin"
                | "acos"
                | "atan"
                | "atan2"
                | "constant"
                | "random"
        ),
        SETS => matches!(
            local,
            "difference" | "intersection" | "distinct" | "has-same-node" | "leading" | "trailing"
        ),
        STRINGS => matches!(
            local,
            "tokenize" | "split" | "concat" | "padding" | "align" | "replace"
        ),
        _ => false,
    }
}

fn arity(ok: bool) -> Result<(), XPathError> {
    if ok {
        Ok(())
    } else {
        Err(XPathError::new(XPathErrorKind::Arity))
    }
}

/// Calls an EXSLT function; `None` when it isn't one.
pub(super) fn call(
    tx: &mut Tx<'_>,
    ar: &mut Arena,
    ns: &str,
    local: &str,
    args: Vec<Value>,
    _c: CallContext,
) -> Option<Result<Value, XPathError>> {
    if !is_known(ns, local) {
        return None;
    }
    Some(match ns {
        COMMON | SAXON => common(tx, ar, local, args),
        DATES => dates(ar, local, args),
        MATH => math(ar, local, args),
        SETS => sets(ar, local, args),
        STRINGS => strings(tx, ar, local, args),
        _ => return None,
    })
}

fn common(
    tx: &mut Tx<'_>,
    ar: &mut Arena,
    local: &str,
    args: Vec<Value>,
) -> Result<Value, XPathError> {
    arity(args.len() == 1)?;
    let v = args.into_iter().next().unwrap();
    if local == "object-type" {
        return Ok(Value::String(
            match &v {
                Value::NodeSet(n) => {
                    if n.len() == 1 && tx.rtf_roots.contains(&n[0]) {
                        "RTF"
                    } else {
                        "node-set"
                    }
                }
                Value::Boolean(_) => "boolean",
                Value::Number(_) => "number",
                Value::String(_) => "string",
            }
            .to_string(),
        ));
    }
    // node-set(): a string or the like becomes a text node.
    match v {
        Value::NodeSet(n) => Ok(Value::NodeSet(n)),
        other => {
            let s = other.to_string_value(ar);
            let doc = fragment(tx, ar);
            let t = ar.new_text(doc, s);
            ar.raw_append(doc, t);
            Ok(Value::NodeSet(vec![t]))
        }
    }
}

fn fragment(tx: &mut Tx<'_>, ar: &mut Arena) -> NodeId {
    let doc = ar.new_document(DocKind::Xml);
    tx.keep_fragment(doc);
    doc
}

// ----- dates ----------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default)]
struct Dt {
    year: Option<i64>,
    month: Option<i64>,
    day: Option<i64>,
    /// hour, minute, second
    time: Option<(i64, i64, f64)>,
}

fn is_leap(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

fn month_len(y: i64, m: i64) -> i64 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ => {
            if is_leap(y) {
                29
            } else {
                28
            }
        }
    }
}

fn digits(s: &[u8], n: usize) -> Option<(i64, &[u8])> {
    if s.len() < n || !s[..n].iter().all(u8::is_ascii_digit) {
        return None;
    }
    let v = std::str::from_utf8(&s[..n]).ok()?.parse().ok()?;
    Some((v, &s[n..]))
}

fn zone_ok(s: &[u8]) -> bool {
    if s.is_empty() || s == b"Z" {
        return true;
    }
    if s[0] != b'+' && s[0] != b'-' {
        return false;
    }
    let Some((h, r)) = digits(&s[1..], 2) else {
        return false;
    };
    if r.first() != Some(&b':') {
        return false;
    }
    let Some((m, r)) = digits(&r[1..], 2) else {
        return false;
    };
    r.is_empty() && h <= 14 && m < 60
}

fn parse_time(s: &[u8]) -> Option<((i64, i64, f64), &[u8])> {
    let (h, r) = digits(s, 2)?;
    if r.first() != Some(&b':') {
        return None;
    }
    let (m, r) = digits(&r[1..], 2)?;
    if r.first() != Some(&b':') {
        return None;
    }
    let (sec, mut r) = digits(&r[1..], 2)?;
    let mut secs = sec as f64;
    if r.first() == Some(&b'.') {
        let n = r[1..].iter().take_while(|c| c.is_ascii_digit()).count();
        if n == 0 {
            return None;
        }
        let frac: f64 = std::str::from_utf8(&r[..n + 1]).ok()?.parse().ok()?;
        secs += frac;
        r = &r[n + 1..];
    }
    if h > 24 || m > 59 || secs >= 60.0 {
        return None;
    }
    Some(((h, m, secs), r))
}

/// The lexical forms of XML Schema's date types EXSLT reads. A negative
/// year is rejected (test xslt010_gt10129.phpt: `-0004-02-29` gives NaN).
fn parse_date(s: &str) -> Option<Dt> {
    let s = s.trim().as_bytes();
    let mut d = Dt::default();
    if let Some(r) = s.strip_prefix(b"---") {
        let (day, r) = digits(r, 2)?;
        if !(1..=31).contains(&day) || !zone_ok(r) {
            return None;
        }
        d.day = Some(day);
        return Some(d);
    }
    if let Some(r) = s.strip_prefix(b"--") {
        let (m, r) = digits(r, 2)?;
        if !(1..=12).contains(&m) {
            return None;
        }
        d.month = Some(m);
        if let Some(r2) = r.strip_prefix(b"-") {
            let (day, r3) = digits(r2, 2)?;
            if !(1..=month_len(2000, m)).contains(&day) || !zone_ok(r3) {
                return None;
            }
            d.day = Some(day);
            return Some(d);
        }
        return zone_ok(r).then_some(d);
    }
    if s.len() >= 3 && s[2] == b':' {
        let (t, r) = parse_time(s)?;
        if !zone_ok(r) {
            return None;
        }
        d.time = Some(t);
        return Some(d);
    }
    let n = s.iter().take_while(|c| c.is_ascii_digit()).count();
    if n < 4 {
        return None;
    }
    let (y, r) = digits(s, n)?;
    if y == 0 {
        return None;
    }
    d.year = Some(y);
    let Some(r) = r.strip_prefix(b"-") else {
        return zone_ok(r).then_some(d);
    };
    let (m, r) = digits(r, 2)?;
    if !(1..=12).contains(&m) {
        return None;
    }
    d.month = Some(m);
    let Some(r) = r.strip_prefix(b"-") else {
        return zone_ok(r).then_some(d);
    };
    let (day, r) = digits(r, 2)?;
    if !(1..=month_len(y, m)).contains(&day) {
        return None;
    }
    d.day = Some(day);
    if let Some(r) = r.strip_prefix(b"T") {
        let (t, r) = parse_time(r)?;
        if !zone_ok(r) {
            return None;
        }
        d.time = Some(t);
        return Some(d);
    }
    zone_ok(r).then_some(d)
}

/// Days from 0001-01-01 (proleptic Gregorian).
fn day_number(y: i64, m: i64, d: i64) -> i64 {
    let yy = y - 1;
    let mut n = yy * 365 + yy / 4 - yy / 100 + yy / 400;
    for i in 1..m {
        n += month_len(y, i);
    }
    n + d - 1
}

/// 1 = Sunday ... 7 = Saturday. 0001-01-01 was a Monday.
fn weekday(y: i64, m: i64, d: i64) -> i64 {
    (day_number(y, m, d) + 1) % 7 + 1
}

fn day_of_year(y: i64, m: i64, d: i64) -> i64 {
    day_number(y, m, d) - day_number(y, 1, 1) + 1
}

/// The ISO 8601 week number.
fn iso_week(y: i64, m: i64, d: i64) -> i64 {
    let wd = (weekday(y, m, d) + 5) % 7 + 1; // 1 = Monday
    let w = (day_of_year(y, m, d) - wd + 10) / 7;
    if w < 1 {
        let py = y - 1;
        let pd = if is_leap(py) { 366 } else { 365 };
        return (day_of_year(y, m, d) + pd - wd + 10) / 7;
    }
    if w == 53 {
        let len = if is_leap(y) { 366 } else { 365 };
        if day_of_year(y, m, d) - wd + 4 > len {
            return 1;
        }
    }
    w
}

const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];
const DAYS: [&str; 7] = [
    "Sunday",
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
];

/// The current UTC time.
fn now() -> Dt {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs()) as i64;
    let days = secs / 86400;
    let mut y = 1970;
    let mut left = days;
    loop {
        let len = if is_leap(y) { 366 } else { 365 };
        if left < len {
            break;
        }
        left -= len;
        y += 1;
    }
    let mut m = 1;
    while left >= month_len(y, m) {
        left -= month_len(y, m);
        m += 1;
    }
    let rem = secs % 86400;
    Dt {
        year: Some(y),
        month: Some(m),
        day: Some(left + 1),
        time: Some((rem / 3600, rem % 3600 / 60, (rem % 60) as f64)),
    }
}

fn dates(ar: &Arena, local: &str, args: Vec<Value>) -> Result<Value, XPathError> {
    arity(args.len() <= 1)?;
    if local == "date-time" {
        arity(args.is_empty())?;
        let d = now();
        let (h, m, s) = d.time.unwrap_or((0, 0, 0.0));
        return Ok(Value::String(format!(
            "{:04}-{:02}-{:02}T{h:02}:{m:02}:{:02}Z",
            d.year.unwrap_or(0),
            d.month.unwrap_or(0),
            d.day.unwrap_or(0),
            s as i64
        )));
    }
    let input = args.first().map(|a| a.to_string_value(ar));
    let parsed = match &input {
        Some(s) => parse_date(s),
        None => Some(now()),
    };
    let nan = Value::Number(f64::NAN);
    let empty = Value::String(String::new());
    let Some(d) = parsed else {
        return Ok(match local {
            "date" | "time" | "month-name" | "month-abbreviation" | "day-name"
            | "day-abbreviation" => empty,
            _ => nan,
        });
    };
    let ymd = match (d.year, d.month, d.day) {
        (Some(y), Some(m), Some(day)) => Some((y, m, day)),
        _ => None,
    };
    let num = |o: Option<i64>| o.map_or(Value::Number(f64::NAN), |v| Value::Number(v as f64));
    Ok(match local {
        "date" => match ymd {
            Some((y, m, day)) => Value::String(format!("{y:04}-{m:02}-{day:02}")),
            None => empty,
        },
        "time" => match d.time {
            Some((h, m, s)) => Value::String(format!("{h:02}:{m:02}:{:02}", s as i64)),
            None => empty,
        },
        "year" => num(d.year),
        "leap-year" => match d.year {
            Some(y) => Value::Boolean(is_leap(y)),
            None => nan,
        },
        "month-in-year" => num(d.month),
        "month-name" => match d.month {
            Some(m) => Value::String(MONTHS[m as usize - 1].into()),
            None => empty,
        },
        "month-abbreviation" => match d.month {
            Some(m) => Value::String(MONTHS[m as usize - 1][..3].into()),
            None => empty,
        },
        "week-in-year" => num(ymd.map(|(y, m, day)| iso_week(y, m, day))),
        "day-in-year" => num(ymd.map(|(y, m, day)| day_of_year(y, m, day))),
        "day-in-month" => num(d.day),
        "day-of-week-in-month" => num(ymd.map(|(_, _, day)| (day - 1) / 7 + 1)),
        "day-in-week" => num(ymd.map(|(y, m, day)| weekday(y, m, day))),
        "day-name" => match ymd {
            Some((y, m, day)) => Value::String(DAYS[weekday(y, m, day) as usize - 1].into()),
            None => empty,
        },
        "day-abbreviation" => match ymd {
            Some((y, m, day)) => Value::String(DAYS[weekday(y, m, day) as usize - 1][..3].into()),
            None => empty,
        },
        "hour-in-day" => num(d.time.map(|t| t.0)),
        "minute-in-hour" => num(d.time.map(|t| t.1)),
        "second-in-minute" => match d.time {
            Some(t) => Value::Number(t.2),
            None => nan,
        },
        _ => nan,
    })
}

// ----- math -----------------------------------------------------------------

fn node_numbers(ar: &Arena, v: &[NodeId]) -> Vec<f64> {
    v.iter()
        .map(|&n| xpath::string_to_number(&xpath::string_value(ar, n)))
        .collect()
}

fn nodes_arg(v: Option<&Value>) -> Result<&[NodeId], XPathError> {
    match v {
        Some(Value::NodeSet(n)) => Ok(n),
        Some(_) => Err(XPathError::new(XPathErrorKind::InvalidType)),
        None => Err(XPathError::new(XPathErrorKind::Arity)),
    }
}

fn math(ar: &Arena, local: &str, args: Vec<Value>) -> Result<Value, XPathError> {
    let n1 = |args: &[Value]| -> Result<f64, XPathError> {
        arity(args.len() == 1)?;
        Ok(args[0].to_number(ar))
    };
    Ok(match local {
        "min" | "max" | "highest" | "lowest" => {
            arity(args.len() == 1)?;
            let nodes = nodes_arg(args.first())?;
            let nums = node_numbers(ar, nodes);
            let want_max = local == "max" || local == "highest";
            let mut best: Option<f64> = None;
            let mut has_nan = false;
            for &x in &nums {
                if x.is_nan() {
                    has_nan = true;
                    continue;
                }
                best = Some(match best {
                    None => x,
                    Some(b) => {
                        if want_max {
                            b.max(x)
                        } else {
                            b.min(x)
                        }
                    }
                });
            }
            if local == "min" || local == "max" {
                return Ok(Value::Number(if has_nan {
                    f64::NAN
                } else {
                    best.unwrap_or(f64::NAN)
                }));
            }
            if has_nan {
                return Ok(Value::NodeSet(Vec::new()));
            }
            let Some(b) = best else {
                return Ok(Value::NodeSet(Vec::new()));
            };
            Value::NodeSet(
                nodes
                    .iter()
                    .zip(&nums)
                    .filter(|(_, x)| **x == b)
                    .map(|(n, _)| *n)
                    .collect(),
            )
        }
        "abs" => Value::Number(n1(&args)?.abs()),
        "sqrt" => Value::Number(n1(&args)?.sqrt()),
        "log" => Value::Number(n1(&args)?.ln()),
        "exp" => Value::Number(n1(&args)?.exp()),
        "sin" => Value::Number(n1(&args)?.sin()),
        "cos" => Value::Number(n1(&args)?.cos()),
        "tan" => Value::Number(n1(&args)?.tan()),
        "asin" => Value::Number(n1(&args)?.asin()),
        "acos" => Value::Number(n1(&args)?.acos()),
        "atan" => Value::Number(n1(&args)?.atan()),
        "power" => {
            arity(args.len() == 2)?;
            Value::Number(args[0].to_number(ar).powf(args[1].to_number(ar)))
        }
        "atan2" => {
            arity(args.len() == 2)?;
            Value::Number(args[0].to_number(ar).atan2(args[1].to_number(ar)))
        }
        "constant" => {
            arity(args.len() == 2)?;
            let name = args[0].to_string_value(ar);
            Value::Number(match name.as_str() {
                "PI" => std::f64::consts::PI,
                "E" => std::f64::consts::E,
                "SQRRT2" => std::f64::consts::SQRT_2,
                "LN2" => std::f64::consts::LN_2,
                "LN10" => std::f64::consts::LN_10,
                "LOG2E" => std::f64::consts::LOG2_E,
                "SQRT1_2" => std::f64::consts::FRAC_1_SQRT_2,
                _ => f64::NAN,
            })
        }
        "random" => {
            arity(args.is_empty())?;
            let t = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.subsec_nanos());
            let mix = (t as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 11;
            Value::Number(mix as f64 / (1u64 << 53) as f64)
        }
        _ => Value::Number(f64::NAN),
    })
}

// ----- sets -----------------------------------------------------------------

fn sets(ar: &Arena, local: &str, args: Vec<Value>) -> Result<Value, XPathError> {
    if local == "distinct" {
        arity(args.len() == 1)?;
        let a = nodes_arg(args.first())?;
        let mut seen = std::collections::HashSet::new();
        return Ok(Value::NodeSet(
            a.iter()
                .copied()
                .filter(|&n| seen.insert(xpath::string_value(ar, n)))
                .collect(),
        ));
    }
    arity(args.len() == 2)?;
    let a = nodes_arg(args.first())?;
    let b = nodes_arg(args.get(1))?;
    Ok(match local {
        "difference" => Value::NodeSet(a.iter().copied().filter(|n| !b.contains(n)).collect()),
        "intersection" => Value::NodeSet(a.iter().copied().filter(|n| b.contains(n)).collect()),
        "has-same-node" => Value::Boolean(a.iter().any(|n| b.contains(n))),
        "leading" => match b.first() {
            None => Value::NodeSet(a.to_vec()),
            Some(f) => match a.iter().position(|x| x == f) {
                Some(p) => Value::NodeSet(a[..p].to_vec()),
                None => Value::NodeSet(Vec::new()),
            },
        },
        _ => match b.first() {
            None => Value::NodeSet(a.to_vec()),
            Some(f) => match a.iter().position(|x| x == f) {
                Some(p) => Value::NodeSet(a[p + 1..].to_vec()),
                None => Value::NodeSet(Vec::new()),
            },
        },
    })
}

// ----- strings --------------------------------------------------------------

fn token_nodes(tx: &mut Tx<'_>, ar: &mut Arena, tokens: Vec<String>) -> Value {
    let doc = fragment(tx, ar);
    let mut out = Vec::with_capacity(tokens.len());
    for t in tokens {
        let el = ar.new_element(doc, crate::tree::QName::local("token"));
        let tn = ar.new_text(doc, t);
        ar.raw_append(el, tn);
        ar.raw_append(doc, el);
        out.push(el);
    }
    Value::NodeSet(out)
}

fn strings(
    tx: &mut Tx<'_>,
    ar: &mut Arena,
    local: &str,
    args: Vec<Value>,
) -> Result<Value, XPathError> {
    Ok(match local {
        "tokenize" => {
            arity(!args.is_empty() && args.len() <= 2)?;
            let s = args[0].to_string_value(ar);
            let d = match args.get(1) {
                Some(v) => v.to_string_value(ar),
                None => " \t\n\r".to_string(),
            };
            let toks: Vec<String> = if d.is_empty() {
                s.chars().map(|c| c.to_string()).collect()
            } else {
                s.split(|c| d.contains(c))
                    .filter(|t| !t.is_empty())
                    .map(str::to_string)
                    .collect()
            };
            token_nodes(tx, ar, toks)
        }
        "split" => {
            arity(!args.is_empty() && args.len() <= 2)?;
            let s = args[0].to_string_value(ar);
            let d = match args.get(1) {
                Some(v) => v.to_string_value(ar),
                None => " ".to_string(),
            };
            let toks: Vec<String> = if d.is_empty() {
                s.chars().map(|c| c.to_string()).collect()
            } else {
                s.split(d.as_str()).map(str::to_string).collect()
            };
            token_nodes(tx, ar, toks)
        }
        "concat" => {
            arity(args.len() == 1)?;
            let mut s = String::new();
            for &n in nodes_arg(args.first())? {
                s.push_str(&xpath::string_value(ar, n));
            }
            Value::String(s)
        }
        "padding" => {
            arity(!args.is_empty() && args.len() <= 2)?;
            let len = args[0].to_number(ar);
            let pad = match args.get(1) {
                Some(v) => v.to_string_value(ar),
                None => " ".to_string(),
            };
            if pad.is_empty() || len.is_nan() || len <= 0.0 {
                return Ok(Value::String(String::new()));
            }
            Value::String(pad.chars().cycle().take(len as usize).collect())
        }
        "align" => {
            arity(args.len() >= 2 && args.len() <= 3)?;
            let s: Vec<char> = args[0].to_string_value(ar).chars().collect();
            let p: Vec<char> = args[1].to_string_value(ar).chars().collect();
            let how = match args.get(2) {
                Some(v) => v.to_string_value(ar),
                None => "left".into(),
            };
            if s.len() >= p.len() {
                return Ok(Value::String(s[..p.len()].iter().collect()));
            }
            let gap = p.len() - s.len();
            let start = match how.as_str() {
                "right" => gap,
                "center" => gap / 2,
                _ => 0,
            };
            let mut out: Vec<char> = p.clone();
            out[start..start + s.len()].copy_from_slice(&s);
            Value::String(out.iter().collect())
        }
        _ => {
            // str:replace with strings or node-sets of search values.
            arity(args.len() == 3)?;
            let s = args[0].to_string_value(ar);
            let list = |v: &Value| -> Vec<String> {
                match v {
                    Value::NodeSet(n) => n.iter().map(|&x| xpath::string_value(ar, x)).collect(),
                    v => vec![v.to_string_value(ar)],
                }
            };
            let from = list(&args[1]);
            let to = list(&args[2]);
            let mut out = s;
            for (i, f) in from.iter().enumerate() {
                if f.is_empty() {
                    continue;
                }
                let r = to.get(i).map_or("", String::as_str);
                out = out.replace(f.as_str(), r);
            }
            Value::String(out)
        }
    })
}
