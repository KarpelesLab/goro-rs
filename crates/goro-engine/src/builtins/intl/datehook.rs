//! Date and time arguments of MessageFormatter (ICU user guide
//! "Formatting Messages": `date`/`time` with the short, medium, long and
//! full styles, a date pattern or a `::` skeleton), formatted in the
//! default time zone by goro-ext-intl's date formatter.

use goro_ext_date::TimeZone;
use goro_ext_intl::datefmt;


/// Formats `ms` (milliseconds since the epoch) as a `date`/`time`
/// argument (`ty`: "date", "time" or "datetime" for a date value in a
/// plain argument, which ICU shows in the short styles).
pub(crate) fn format(locale: &str, tz: &TimeZone, ty: &str, style: Option<&str>, ms: f64) -> String {
    let tz_id = tz.name();
    let pattern = goro_ext_intl::msgfmt::date_pattern(locale, ty, style);
    datefmt::format_pattern(locale, &pattern, ms, &tz_id)
}
