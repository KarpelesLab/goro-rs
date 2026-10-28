//! Date and time arguments of MessageFormatter (ICU user guide
//! "Formatting Messages": `date`/`time` with the short, medium, long and
//! full styles, a date pattern or a `::` skeleton), formatted in the
//! default time zone by goro-ext-intl's date formatter.

use goro_ext_date::TimeZone;
use goro_ext_intl::datefmt;

const FULL: i64 = 0;
const LONG: i64 = 1;
const MEDIUM: i64 = 2;
const SHORT: i64 = 3;
const NONE: i64 = -1;

/// Formats `ms` (milliseconds since the epoch) as a `date`/`time`
/// argument (`ty`: "date", "time" or "datetime" for a date value in a
/// plain argument, which ICU shows in the short styles).
pub(crate) fn format(locale: &str, tz: &TimeZone, ty: &str, style: Option<&str>, ms: f64) -> String {
    let tz_id = tz.name();
    let level = |s: Option<&str>| match s {
        None | Some("medium") => Some(MEDIUM),
        Some("short") => Some(SHORT),
        Some("long") => Some(LONG),
        Some("full") => Some(FULL),
        _ => None,
    };
    let style = style.map(str::trim);
    let pattern = match ty {
        "datetime" => datefmt::style_pattern(locale, SHORT, SHORT),
        _ => match level(style) {
            Some(l) if ty == "time" => datefmt::style_pattern(locale, NONE, l),
            Some(l) => datefmt::style_pattern(locale, l, NONE),
            None => {
                let s = style.unwrap_or("");
                match s.strip_prefix("::") {
                    Some(sk) => datefmt::skeleton_pattern(locale, sk),
                    None => s.to_string(),
                }
            }
        },
    };
    datefmt::format_pattern(locale, &pattern, ms, &tz_id)
}
