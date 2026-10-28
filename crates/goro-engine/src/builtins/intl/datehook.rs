//! Date and time arguments of MessageFormatter: CLDR's Gregorian date and
//! time formats (UTS #35 section 4 date field symbols) in the default
//! time zone.

use goro_ext_date::TimeZone;
use goro_ext_date::time::LocalTime;

/// Formats `ms` (milliseconds since the epoch) as a `date`/`time`
/// argument (`ty`: "date", "time" or "datetime" for a date value in a
/// plain argument) with an ICU style or pattern.
pub(crate) fn format(locale: &str, tz: &TimeZone, ty: &str, style: Option<&str>, ms: f64) -> String {
    let secs = (ms / 1000.0).floor() as i64;
    let lt = LocalTime::new(secs, 0, tz);
    let pattern = match (ty, style.map(str::trim)) {
        ("date", None) | ("date", Some("medium")) => "MMM d, y".to_string(),
        ("date", Some("short")) => "M/d/yy".to_string(),
        ("date", Some("long")) => "MMMM d, y".to_string(),
        ("date", Some("full")) => "EEEE, MMMM d, y".to_string(),
        ("time", None) | ("time", Some("medium")) => "h:mm:ss\u{202f}a".to_string(),
        ("time", Some("short")) => "h:mm\u{202f}a".to_string(),
        ("time", Some("long")) => "h:mm:ss\u{202f}a z".to_string(),
        ("time", Some("full")) => "h:mm:ss\u{202f}a zzzz".to_string(),
        ("datetime", _) => "M/d/yy, h:mm\u{202f}a".to_string(),
        (_, Some(p)) => p.strip_prefix("::").map_or_else(|| p.to_string(), skeleton),
        _ => "M/d/yy".to_string(),
    };
    let _ = locale;
    render(&pattern, &lt, ms)
}

fn skeleton(s: &str) -> String {
    s.to_string()
}

const MONTHS: [&str; 12] = [
    "January", "February", "March", "April", "May", "June", "July", "August", "September",
    "October", "November", "December",
];
const DAYS: [&str; 7] = [
    "Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday",
];

fn render(p: &str, lt: &LocalTime, ms: f64) -> String {
    let c: Vec<char> = p.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < c.len() {
        let ch = c[i];
        if ch == '\'' {
            i += 1;
            while i < c.len() && c[i] != '\'' {
                out.push(c[i]);
                i += 1;
            }
            i += 1;
            continue;
        }
        if !ch.is_ascii_alphabetic() {
            out.push(ch);
            i += 1;
            continue;
        }
        let mut n = 1;
        while i + n < c.len() && c[i + n] == ch {
            n += 1;
        }
        i += n;
        let h12 = if lt.h % 12 == 0 { 12 } else { lt.h % 12 };
        match ch {
            'y' => {
                if n == 2 {
                    out.push_str(&format!("{:02}", lt.y.rem_euclid(100)));
                } else {
                    out.push_str(&format!("{:0n$}", lt.y));
                }
            }
            'M' | 'L' => match n {
                1 => out.push_str(&lt.m.to_string()),
                2 => out.push_str(&format!("{:02}", lt.m)),
                3 => out.push_str(&MONTHS[lt.m as usize - 1][..3]),
                _ => out.push_str(MONTHS[lt.m as usize - 1]),
            },
            'd' => out.push_str(&format!("{:0n$}", lt.d)),
            'E' => {
                let d = DAYS[lt.weekday() as usize];
                out.push_str(if n >= 4 { d } else { &d[..3] });
            }
            'h' => out.push_str(&format!("{:0n$}", h12)),
            'H' => out.push_str(&format!("{:0n$}", lt.h)),
            'm' => out.push_str(&format!("{:0n$}", lt.i)),
            's' => out.push_str(&format!("{:0n$}", lt.s)),
            'S' => {
                let frac = (ms.rem_euclid(1000.0)) as u32;
                let s = format!("{frac:03}");
                out.push_str(&s[..n.min(3)]);
            }
            'a' => out.push_str(if lt.h < 12 { "AM" } else { "PM" }),
            'z' => out.push_str(lt.abbr.as_str()),
            _ => {}
        }
    }
    out
}
