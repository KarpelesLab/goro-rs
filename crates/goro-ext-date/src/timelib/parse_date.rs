//! Port of timelib parse_date.re: strtotime() parser and parse_from_format().

use super::dow::{date_from_isodate, daynr_from_weeknr};
use super::regex::{parse_definitions, parse_expr, Scanner as ReScanner};
use super::tm2unixtime::do_normalize;
use super::tz::parse_tzfile;
use super::unixtime2tm::update_from_sse;
use super::*;
use std::cell::RefCell;

#[path = "../data/tzmap.rs"]
mod tzmap;
pub use tzmap::{FALLBACKMAP, TIMEZONEMAP};

// Error codes
pub const WARN_DOUBLE_TZ: i32 = 0x101;
pub const WARN_INVALID_TIME: i32 = 0x102;
pub const WARN_INVALID_DATE: i32 = 0x103;
pub const WARN_TRAILING_DATA: i32 = 0x11a;
pub const ERR_DOUBLE_TZ: i32 = 0x201;
pub const ERR_TZID_NOT_FOUND: i32 = 0x202;
pub const ERR_DOUBLE_TIME: i32 = 0x203;
pub const ERR_DOUBLE_DATE: i32 = 0x204;
pub const ERR_UNEXPECTED_CHARACTER: i32 = 0x205;
pub const ERR_EMPTY_STRING: i32 = 0x206;
pub const ERR_UNEXPECTED_DATA: i32 = 0x207;
pub const ERR_NO_TEXTUAL_DAY: i32 = 0x208;
pub const ERR_NO_TWO_DIGIT_DAY: i32 = 0x209;
pub const ERR_NO_THREE_DIGIT_DAY_OF_YEAR: i32 = 0x20a;
pub const ERR_NO_TWO_DIGIT_MONTH: i32 = 0x20b;
pub const ERR_NO_TEXTUAL_MONTH: i32 = 0x20c;
pub const ERR_NO_TWO_DIGIT_YEAR: i32 = 0x20d;
pub const ERR_NO_FOUR_DIGIT_YEAR: i32 = 0x20e;
pub const ERR_NO_TWO_DIGIT_HOUR: i32 = 0x20f;
pub const ERR_HOUR_LARGER_THAN_12: i32 = 0x210;
pub const ERR_MERIDIAN_BEFORE_HOUR: i32 = 0x211;
pub const ERR_NO_MERIDIAN: i32 = 0x212;
pub const ERR_NO_TWO_DIGIT_MINUTE: i32 = 0x213;
pub const ERR_NO_TWO_DIGIT_SECOND: i32 = 0x214;
pub const ERR_NO_SIX_DIGIT_MICROSECOND: i32 = 0x215;
pub const ERR_NO_SEP_SYMBOL: i32 = 0x216;
pub const ERR_EXPECTED_ESCAPE_CHAR: i32 = 0x217;
pub const ERR_NO_ESCAPED_CHAR: i32 = 0x218;
pub const ERR_WRONG_FORMAT_SEP: i32 = 0x219;
pub const ERR_TRAILING_DATA: i32 = 0x21a;
pub const ERR_DATA_MISSING: i32 = 0x21b;
pub const ERR_NO_THREE_DIGIT_MILLISECOND: i32 = 0x21c;
pub const ERR_NO_FOUR_DIGIT_YEAR_ISO: i32 = 0x21d;
pub const ERR_NO_TWO_DIGIT_WEEK: i32 = 0x21e;
pub const ERR_INVALID_WEEK: i32 = 0x21f;
pub const ERR_NO_DAY_OF_WEEK: i32 = 0x220;
pub const ERR_INVALID_DAY_OF_WEEK: i32 = 0x221;
pub const ERR_INVALID_SPECIFIER: i32 = 0x222;
pub const ERR_INVALID_TZ_OFFSET: i32 = 0x223;
pub const ERR_FORMAT_LITERAL_MISMATCH: i32 = 0x224;
pub const ERR_MIX_ISO_WITH_NATURAL: i32 = 0x225;
pub const ERR_NUMBER_OUT_OF_RANGE: i32 = 0x226;

const DEFINITIONS: &str = r#"
any = [\000-\377];
nbsp = [\302][\240];
nnbsp = [\342][\200][\257];
space = [ \t]+ | nbsp+ | nnbsp+;
frac = "."[0-9]+;
ago = 'ago';
hour24 = [01]?[0-9] | "2"[0-4];
hour24lz = [01][0-9] | "2"[0-4];
hour12 = "0"?[1-9] | "1"[0-2];
minute = [0-5]?[0-9];
minutelz = [0-5][0-9];
second = minute | "60";
secondlz = minutelz | "60";
meridian = ([AaPp] "."? [Mm] "."?) [\000\t ];
tz = "("? [A-Za-z]{1,6} ")"? | [A-Z][a-z]+([_/-][A-Za-z]+)+;
tzcorrection = "GMT"? [+-] ((hour24 (":"? minute)?) | (hour24lz minutelz secondlz) | (hour24lz ":" minutelz ":" secondlz));
daysuf = "st" | "nd" | "rd" | "th";
month = "0"? [0-9] | "1"[0-2];
day   = (([0-2]?[0-9]) | ("3"[01])) daysuf?;
year  = [0-9]{1,4};
year2 = [0-9]{2};
year4 = [0-9]{4};
year4withsign = [+-]? [0-9]{4};
yearx = [+-] [0-9]{5,19};
dayofyear = "00"[1-9] | "0"[1-9][0-9] | [1-2][0-9][0-9] | "3"[0-5][0-9] | "36"[0-6];
weekofyear = "0"[1-9] | [1-4][0-9] | "5"[0-3];
monthlz = "0" [0-9] | "1" [0-2];
daylz   = "0" [0-9] | [1-2][0-9] | "3" [01];
dayfulls = 'sundays' | 'mondays' | 'tuesdays' | 'wednesdays' | 'thursdays' | 'fridays' | 'saturdays';
dayfull = 'sunday' | 'monday' | 'tuesday' | 'wednesday' | 'thursday' | 'friday' | 'saturday';
dayabbr = 'sun' | 'mon' | 'tue' | 'wed' | 'thu' | 'fri' | 'sat' | 'sun';
dayspecial = 'weekday' | 'weekdays';
daytext = dayfulls | dayfull | dayabbr | dayspecial;
monthfull = 'january' | 'february' | 'march' | 'april' | 'may' | 'june' | 'july' | 'august' | 'september' | 'october' | 'november' | 'december';
monthabbr = 'jan' | 'feb' | 'mar' | 'apr' | 'may' | 'jun' | 'jul' | 'aug' | 'sep' | 'sept' | 'oct' | 'nov' | 'dec';
monthroman = "I" | "II" | "III" | "IV" | "V" | "VI" | "VII" | "VIII" | "IX" | "X" | "XI" | "XII";
monthtext = monthfull | monthabbr | monthroman;
timetiny12 = hour12 space? meridian;
timeshort12 = hour12[:.]minutelz space? meridian;
timelong12 = hour12[:.]minute[:.]secondlz space? meridian;
timetiny24 = 't' hour24;
timeshort24 = 't'? hour24[:.]minute;
timelong24 =  't'? hour24[:.]minute[:.]second;
iso8601long =  't'? hour24 [:.] minute [:.] second frac;
iso8601normtz =  't'? hour24 [:.] minute [:.] secondlz space? (tzcorrection | tz);
gnunocolon       = 't'? hour24lz minutelz;
iso8601nocolon   = 't'? hour24lz minutelz secondlz;
americanshort    = month "/" day;
american         = month "/" day "/" year;
iso8601dateslash = year4 "/" monthlz "/" daylz "/"?;
dateslash        = year4 "/" month "/" day;
iso8601date4     = year4withsign "-" monthlz "-" daylz;
iso8601date2     = year2 "-" monthlz "-" daylz;
iso8601datex     = yearx "-" monthlz "-" daylz;
gnudateshorter   = year4 "-" month;
gnudateshort     = year "-" month "-" day;
pointeddate4     = day [.\t-] month [.-] year4;
pointeddate2     = day [.\t] month "." year2;
datefull         = day ([ \t.-])* monthtext ([ \t.-])* year;
datenoday        = monthtext ([ .\t-])* year4;
datenodayrev     = year4 ([ .\t-])* monthtext;
datetextual      = monthtext ([ .\t-])* day [,.stndrh\t ]+ year;
datenoyear       = monthtext ([ .\t-])* day ([,.stndrh\t ]+|[\000]);
datenoyearrev    = day ([ .\t-])* monthtext;
datenocolon      = year4 monthlz daylz;
soap             = year4 "-" monthlz "-" daylz "T" hour24lz ":" minutelz ":" secondlz frac tzcorrection?;
xmlrpc           = year4 monthlz daylz "T" hour24 ":" minutelz ":" secondlz;
xmlrpcnocolon    = year4 monthlz daylz 't' hour24 minutelz secondlz;
wddx             = year4 "-" month "-" day "T" hour24 ":" minute ":" second;
pgydotd          = year4 [.-]? dayofyear;
pgtextshort      = monthabbr "-" daylz "-" year;
pgtextreverse    = year "-" monthabbr "-" daylz;
mssqltime        = hour12 ":" minutelz ":" secondlz [:.] [0-9]+ meridian;
isoweekday       = year4 "-"? "W" weekofyear "-"? [0-7];
isoweek          = year4 "-"? "W" weekofyear;
exif             = year4 ":" monthlz ":" daylz " " hour24lz ":" minutelz ":" secondlz;
firstdayof       = 'first day of';
lastdayof        = 'last day of';
backof           = 'back of ' hour24 (space? meridian)?;
frontof          = 'front of ' hour24 (space? meridian)?;
clf              = day "/" monthabbr "/" year4 ":" hour24lz ":" minutelz ":" secondlz space tzcorrection;
timestamp        = "@" "-"? [0-9]+;
timestampms      = "@" "-"? [0-9]+ "." [0-9]{0,6};
dateshortwithtimeshort12  = datenoyear timeshort12;
dateshortwithtimelong12   = datenoyear timelong12;
dateshortwithtimeshort  = datenoyear timeshort24;
dateshortwithtimelong   = datenoyear timelong24;
dateshortwithtimelongtz = datenoyear iso8601normtz;
reltextnumber = 'first'|'second'|'third'|'fourth'|'fifth'|'sixth'|'seventh'|'eight'|'eighth'|'ninth'|'tenth'|'eleventh'|'twelfth';
reltexttext = 'next'|'last'|'previous'|'this';
reltextunit = 'ms' | 'µs' | (('msec'|'millisecond'|'µsec'|'microsecond'|'usec'|'sec'|'second'|'min'|'minute'|'hour'|'day'|'fortnight'|'forthnight'|'month'|'year') 's'?) | 'weeks' | daytext;
relnumber = ([+-]*[ \t]*[0-9]{1,13});
relative = relnumber space? (reltextunit | 'week' );
relativetext = (reltextnumber|reltexttext) space reltextunit;
relativetextweek = reltexttext space 'week';
weekdayof        = (reltextnumber|reltexttext) space (dayfulls|dayfull|dayabbr) space 'of';
"#;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Rule {
    Yesterday,
    Now,
    Noon,
    MidnightToday,
    Tomorrow,
    Timestamp,
    TimestampMs,
    FirstLastDayOf,
    BackFrontOf,
    WeekdayOf,
    Time12,
    MssqlTime,
    Time24,
    GnuNoColon,
    Iso8601NoColon,
    American,
    IsoDate4,
    IsoDate2,
    IsoDateX,
    GnuDateShorter,
    GnuDateShort,
    DateFull,
    PointedDate4,
    PointedDate2,
    DateNoDay,
    DateNoDayRev,
    DateTextual,
    DateNoYearRev,
    DateNoColon,
    XmlRpc,
    PgYdotd,
    IsoWeekday,
    IsoWeek,
    PgTextShort,
    PgTextReverse,
    Clf,
    Year4,
    Ago,
    DayText,
    RelativeTextWeek,
    RelativeText,
    MonthText,
    TzCorrection,
    DateShortWithTime12,
    DateShortWithTime24,
    Relative,
    Punct,
    Space,
    Nul,
    Any,
}

const RULES: &[(&str, Rule)] = &[
    ("'yesterday'", Rule::Yesterday),
    ("'now'", Rule::Now),
    ("'noon'", Rule::Noon),
    ("'midnight' | 'today'", Rule::MidnightToday),
    ("'tomorrow'", Rule::Tomorrow),
    ("timestamp", Rule::Timestamp),
    ("timestampms", Rule::TimestampMs),
    ("firstdayof | lastdayof", Rule::FirstLastDayOf),
    ("backof | frontof", Rule::BackFrontOf),
    ("weekdayof", Rule::WeekdayOf),
    ("timetiny12 | timeshort12 | timelong12", Rule::Time12),
    ("mssqltime", Rule::MssqlTime),
    ("timetiny24 | timeshort24 | timelong24 | iso8601long", Rule::Time24),
    ("gnunocolon", Rule::GnuNoColon),
    ("iso8601nocolon", Rule::Iso8601NoColon),
    ("americanshort | american", Rule::American),
    ("iso8601date4 | iso8601dateslash | dateslash", Rule::IsoDate4),
    ("iso8601date2", Rule::IsoDate2),
    ("iso8601datex", Rule::IsoDateX),
    ("gnudateshorter", Rule::GnuDateShorter),
    ("gnudateshort", Rule::GnuDateShort),
    ("datefull", Rule::DateFull),
    ("pointeddate4", Rule::PointedDate4),
    ("pointeddate2", Rule::PointedDate2),
    ("datenoday", Rule::DateNoDay),
    ("datenodayrev", Rule::DateNoDayRev),
    ("datetextual | datenoyear", Rule::DateTextual),
    ("datenoyearrev", Rule::DateNoYearRev),
    ("datenocolon", Rule::DateNoColon),
    ("xmlrpc | xmlrpcnocolon | soap | wddx | exif", Rule::XmlRpc),
    ("pgydotd", Rule::PgYdotd),
    ("isoweekday", Rule::IsoWeekday),
    ("isoweek", Rule::IsoWeek),
    ("pgtextshort", Rule::PgTextShort),
    ("pgtextreverse", Rule::PgTextReverse),
    ("clf", Rule::Clf),
    ("year4", Rule::Year4),
    ("ago", Rule::Ago),
    ("daytext", Rule::DayText),
    ("relativetextweek", Rule::RelativeTextWeek),
    ("relativetext", Rule::RelativeText),
    ("monthfull | monthabbr", Rule::MonthText),
    ("tzcorrection | tz", Rule::TzCorrection),
    ("dateshortwithtimeshort12 | dateshortwithtimelong12", Rule::DateShortWithTime12),
    ("dateshortwithtimeshort | dateshortwithtimelong | dateshortwithtimelongtz", Rule::DateShortWithTime24),
    ("relative", Rule::Relative),
    ("[.,]", Rule::Punct),
    ("space", Rule::Space),
    ("\"\\000\"|\"\\n\"", Rule::Nul),
    ("any", Rule::Any),
];

thread_local! {
    static SCANNER: RefCell<Option<ReScanner>> = const { RefCell::new(None) };
}

fn with_scanner<R>(f: impl FnOnce(&mut ReScanner) -> R) -> R {
    SCANNER.with(|s| {
        let mut b = s.borrow_mut();
        if b.is_none() {
            let defs = parse_definitions(DEFINITIONS);
            let rules: Vec<_> = RULES.iter().map(|(e, _)| parse_expr(e, &defs)).collect();
            *b = Some(ReScanner::new(&rules));
        }
        f(b.as_mut().unwrap())
    })
}

/// A NUL-terminated byte cursor, mirroring `const char *ptr` in the C code.
#[derive(Clone, Copy)]
pub struct Ptr<'a> {
    pub s: &'a [u8],
    pub p: usize,
}

impl<'a> Ptr<'a> {
    pub fn new(s: &'a [u8]) -> Self {
        Ptr { s, p: 0 }
    }
    #[inline]
    pub fn c(&self) -> u8 {
        *self.s.get(self.p).unwrap_or(&0)
    }
    #[inline]
    pub fn at(&self, k: usize) -> u8 {
        *self.s.get(self.p + k).unwrap_or(&0)
    }
    #[inline]
    pub fn adv(&mut self) {
        self.p += 1;
    }
    pub fn rest(&self) -> &'a [u8] {
        if self.p >= self.s.len() { &[] } else { &self.s[self.p..] }
    }
}

struct State {
    /// The input string, NUL padded
    str: Vec<u8>,
    tok: usize,
    cur: usize,
    errors: ErrorContainer,
    time: Time,
}

impl State {
    fn add_error(&mut self, code: i32, msg: &str) {
        let ch = *self.str.get(self.tok).unwrap_or(&0);
        self.errors.error_messages.push(ErrorMessage { error_code: code, position: self.tok as i32, character: ch, message: msg.to_string() });
    }
    fn add_warning(&mut self, code: i32, msg: &str) {
        let ch = *self.str.get(self.tok).unwrap_or(&0);
        self.errors.warning_messages.push(ErrorMessage { error_code: code, position: self.tok as i32, character: ch, message: msg.to_string() });
    }
}

fn add_pbf_error(errors: &mut ErrorContainer, code: i32, msg: &str, string: &[u8], pos: usize) {
    errors.error_messages.push(ErrorMessage {
        error_code: code,
        position: pos as i32,
        character: *string.get(pos).unwrap_or(&0),
        message: msg.to_string(),
    });
}

fn add_pbf_warning(errors: &mut ErrorContainer, code: i32, msg: &str, string: &[u8], pos: usize) {
    errors.warning_messages.push(ErrorMessage {
        error_code: code,
        position: pos as i32,
        character: *string.get(pos).unwrap_or(&0),
        message: msg.to_string(),
    });
}

fn strtoll_prefix(s: &[u8]) -> (i64, bool) {
    // returns (value, overflowed)
    let mut i = 0;
    let mut neg = false;
    while i < s.len() && (s[i] == b' ' || s[i] == b'\t') {
        i += 1;
    }
    if i < s.len() && (s[i] == b'+' || s[i] == b'-') {
        neg = s[i] == b'-';
        i += 1;
    }
    let mut v: i128 = 0;
    let mut overflow = false;
    while i < s.len() && s[i].is_ascii_digit() {
        v = v * 10 + (s[i] - b'0') as i128;
        if v > (i64::MAX as i128) + 1 {
            overflow = true;
            v = (i64::MAX as i128) + 1;
        }
        i += 1;
    }
    if neg {
        v = -v;
    }
    if v > i64::MAX as i128 {
        return (i64::MAX, true);
    }
    if v < i64::MIN as i128 {
        return (i64::MIN, true);
    }
    (v as i64, overflow && !(neg && v == i64::MIN as i128))
}

fn strtol(s: &[u8]) -> i64 {
    strtoll_prefix(s).0
}

fn strtod_prefix(s: &[u8]) -> f64 {
    let mut end = 0;
    while end < s.len() && s[end].is_ascii_digit() {
        end += 1;
    }
    if end < s.len() && s[end] == b'.' {
        end += 1;
        while end < s.len() && s[end].is_ascii_digit() {
            end += 1;
        }
    }
    std::str::from_utf8(&s[..end]).ok().and_then(|x| x.parse::<f64>().ok()).unwrap_or(0.0)
}

fn timelib_meridian(ptr: &mut Ptr, h: i64) -> i64 {
    let mut retval = 0;
    while !b"AaPp".contains(&ptr.c()) {
        if ptr.c() == 0 && ptr.p >= ptr.s.len() {
            // strchr matches '\0' as part of the string; C would stop here
            break;
        }
        ptr.adv();
    }
    let c = ptr.c();
    if c == b'a' || c == b'A' {
        if h == 12 {
            retval = -12;
        }
    } else if h != 12 {
        retval = 12;
    }
    ptr.adv();
    if ptr.c() == b'.' {
        ptr.adv();
    }
    if ptr.c() == b'M' || ptr.c() == b'm' {
        ptr.adv();
    }
    if ptr.c() == b'.' {
        ptr.adv();
    }
    retval
}

fn timelib_meridian_with_check(ptr: &mut Ptr, h: i64) -> i64 {
    let mut retval = 0;
    while ptr.c() != 0 && !b"AaPp".contains(&ptr.c()) {
        ptr.adv();
    }
    if ptr.c() == 0 {
        return UNSET;
    }
    let c = ptr.c();
    if c == b'a' || c == b'A' {
        if h == 12 {
            retval = -12;
        }
    } else if h != 12 {
        retval = 12;
    }
    ptr.adv();
    if ptr.c() == b'.' {
        ptr.adv();
        if ptr.c() != b'm' && ptr.c() != b'M' {
            return UNSET;
        }
        ptr.adv();
        if ptr.c() != b'.' {
            return UNSET;
        }
        ptr.adv();
    } else if ptr.c() == b'm' || ptr.c() == b'M' {
        ptr.adv();
    } else {
        return UNSET;
    }
    retval
}

pub fn get_nr_ex(ptr: &mut Ptr, max_length: usize) -> (i64, usize) {
    while ptr.c() < b'0' || ptr.c() > b'9' {
        if ptr.c() == 0 {
            return (UNSET, 0);
        }
        ptr.adv();
    }
    let begin = ptr.p;
    let mut len = 0;
    while ptr.c().is_ascii_digit() && len < max_length {
        ptr.adv();
        len += 1;
    }
    (strtol(&ptr.s[begin..ptr.p]), ptr.p - begin)
}

pub fn get_nr(ptr: &mut Ptr, max_length: usize) -> i64 {
    get_nr_ex(ptr, max_length).0
}

fn skip_day_suffix(ptr: &mut Ptr) {
    if (ptr.c() as char).is_ascii_whitespace() || ptr.c() == 0x0b {
        return;
    }
    let a = ptr.c().to_ascii_lowercase();
    let b = ptr.at(1).to_ascii_lowercase();
    if (a == b'n' && b == b'd') || (a == b'r' && b == b'd') || (a == b's' && b == b't') || (a == b't' && b == b'h') {
        ptr.p += 2;
    }
}

fn get_frac_nr(ptr: &mut Ptr) -> i64 {
    while ptr.c() != b'.' && ptr.c() != b':' && !ptr.c().is_ascii_digit() {
        if ptr.c() == 0 {
            return UNSET;
        }
        ptr.adv();
    }
    let begin = ptr.p;
    while ptr.c() == b'.' || ptr.c() == b':' || ptr.c().is_ascii_digit() {
        ptr.adv();
    }
    let end = ptr.p;
    let s = &ptr.s[begin + 1..end];
    let v = strtod_prefix(s) * 10f64.powi(7 - (end - begin) as i32);
    v as i64
}

fn get_signed_nr(errors: &mut Vec<(i32, &'static str)>, ptr: &mut Ptr, max_length: usize) -> i64 {
    while !ptr.c().is_ascii_digit() && ptr.c() != b'+' && ptr.c() != b'-' {
        if ptr.c() == 0 {
            errors.push((ERR_UNEXPECTED_DATA, "Found unexpected data"));
            return 0;
        }
        ptr.adv();
    }
    let mut neg = false;
    while ptr.c() == b'+' || ptr.c() == b'-' {
        if ptr.c() == b'-' {
            neg = !neg;
        }
        ptr.adv();
    }
    while !ptr.c().is_ascii_digit() {
        if ptr.c() == 0 {
            errors.push((ERR_UNEXPECTED_DATA, "Found unexpected data"));
            return 0;
        }
        ptr.adv();
    }
    let mut buf = vec![if neg { b'-' } else { b'+' }];
    let mut len = 0;
    while ptr.c().is_ascii_digit() && len < max_length {
        buf.push(ptr.c());
        ptr.adv();
        len += 1;
    }
    let (v, overflow) = strtoll_prefix(&buf);
    if overflow {
        errors.push((ERR_NUMBER_OUT_OF_RANGE, "Number out of range"));
        return 0;
    }
    v
}

const RELTEXT_LOOKUP: &[(&str, i32, i64)] = &[
    ("first", 0, 1),
    ("next", 0, 1),
    ("second", 0, 2),
    ("third", 0, 3),
    ("fourth", 0, 4),
    ("fifth", 0, 5),
    ("sixth", 0, 6),
    ("seventh", 0, 7),
    ("eight", 0, 8),
    ("eighth", 0, 8),
    ("ninth", 0, 9),
    ("tenth", 0, 10),
    ("eleventh", 0, 11),
    ("twelfth", 0, 12),
    ("last", 0, -1),
    ("previous", 0, -1),
    ("this", 1, 0),
];

const MONTH_LOOKUP: &[(&str, i64)] = &[
    ("jan", 1),
    ("feb", 2),
    ("mar", 3),
    ("apr", 4),
    ("may", 5),
    ("jun", 6),
    ("jul", 7),
    ("aug", 8),
    ("sep", 9),
    ("sept", 9),
    ("oct", 10),
    ("nov", 11),
    ("dec", 12),
    ("i", 1),
    ("ii", 2),
    ("iii", 3),
    ("iv", 4),
    ("v", 5),
    ("vi", 6),
    ("vii", 7),
    ("viii", 8),
    ("ix", 9),
    ("x", 10),
    ("xi", 11),
    ("xii", 12),
    ("january", 1),
    ("february", 2),
    ("march", 3),
    ("april", 4),
    ("may", 5),
    ("june", 6),
    ("july", 7),
    ("august", 8),
    ("september", 9),
    ("october", 10),
    ("november", 11),
    ("december", 12),
];

pub const UNIT_MICROSEC: i32 = 9;
pub const UNIT_SECOND: i32 = 1;
pub const UNIT_MINUTE: i32 = 2;
pub const UNIT_HOUR: i32 = 3;
pub const UNIT_DAY: i32 = 4;
pub const UNIT_MONTH: i32 = 5;
pub const UNIT_YEAR: i32 = 6;
pub const UNIT_WEEKDAY: i32 = 7;
pub const UNIT_SPECIAL: i32 = 8;

pub const RELUNIT_LOOKUP: &[(&str, i32, i64)] = &[
    ("ms", UNIT_MICROSEC, 1000),
    ("msec", UNIT_MICROSEC, 1000),
    ("msecs", UNIT_MICROSEC, 1000),
    ("millisecond", UNIT_MICROSEC, 1000),
    ("milliseconds", UNIT_MICROSEC, 1000),
    ("µs", UNIT_MICROSEC, 1),
    ("usec", UNIT_MICROSEC, 1),
    ("usecs", UNIT_MICROSEC, 1),
    ("µsec", UNIT_MICROSEC, 1),
    ("µsecs", UNIT_MICROSEC, 1),
    ("microsecond", UNIT_MICROSEC, 1),
    ("microseconds", UNIT_MICROSEC, 1),
    ("sec", UNIT_SECOND, 1),
    ("secs", UNIT_SECOND, 1),
    ("second", UNIT_SECOND, 1),
    ("seconds", UNIT_SECOND, 1),
    ("min", UNIT_MINUTE, 1),
    ("mins", UNIT_MINUTE, 1),
    ("minute", UNIT_MINUTE, 1),
    ("minutes", UNIT_MINUTE, 1),
    ("hour", UNIT_HOUR, 1),
    ("hours", UNIT_HOUR, 1),
    ("day", UNIT_DAY, 1),
    ("days", UNIT_DAY, 1),
    ("week", UNIT_DAY, 7),
    ("weeks", UNIT_DAY, 7),
    ("fortnight", UNIT_DAY, 14),
    ("fortnights", UNIT_DAY, 14),
    ("forthnight", UNIT_DAY, 14),
    ("forthnights", UNIT_DAY, 14),
    ("month", UNIT_MONTH, 1),
    ("months", UNIT_MONTH, 1),
    ("year", UNIT_YEAR, 1),
    ("years", UNIT_YEAR, 1),
    ("mondays", UNIT_WEEKDAY, 1),
    ("monday", UNIT_WEEKDAY, 1),
    ("mon", UNIT_WEEKDAY, 1),
    ("tuesdays", UNIT_WEEKDAY, 2),
    ("tuesday", UNIT_WEEKDAY, 2),
    ("tue", UNIT_WEEKDAY, 2),
    ("wednesdays", UNIT_WEEKDAY, 3),
    ("wednesday", UNIT_WEEKDAY, 3),
    ("wed", UNIT_WEEKDAY, 3),
    ("thursdays", UNIT_WEEKDAY, 4),
    ("thursday", UNIT_WEEKDAY, 4),
    ("thu", UNIT_WEEKDAY, 4),
    ("fridays", UNIT_WEEKDAY, 5),
    ("friday", UNIT_WEEKDAY, 5),
    ("fri", UNIT_WEEKDAY, 5),
    ("saturdays", UNIT_WEEKDAY, 6),
    ("saturday", UNIT_WEEKDAY, 6),
    ("sat", UNIT_WEEKDAY, 6),
    ("sundays", UNIT_WEEKDAY, 0),
    ("sunday", UNIT_WEEKDAY, 0),
    ("sun", UNIT_WEEKDAY, 0),
    ("weekday", UNIT_SPECIAL, SPECIAL_WEEKDAY as i64),
    ("weekdays", UNIT_SPECIAL, SPECIAL_WEEKDAY as i64),
];

fn eq_ci(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.eq_ignore_ascii_case(y))
}

fn lookup_relative_text(ptr: &mut Ptr, behavior: &mut i32) -> i64 {
    let begin = ptr.p;
    while ptr.c().is_ascii_alphabetic() {
        ptr.adv();
    }
    let word = &ptr.s[begin..ptr.p];
    let mut value = 0;
    for (name, ty, v) in RELTEXT_LOOKUP {
        if eq_ci(word, name.as_bytes()) {
            value = *v;
            *behavior = *ty;
        }
    }
    value
}

fn get_relative_text(ptr: &mut Ptr, behavior: &mut i32) -> i64 {
    while matches!(ptr.c(), b' ' | b'\t' | b'-' | b'/') {
        ptr.adv();
    }
    lookup_relative_text(ptr, behavior)
}

pub fn lookup_month(ptr: &mut Ptr) -> i64 {
    let begin = ptr.p;
    while ptr.c().is_ascii_alphabetic() {
        ptr.adv();
    }
    let word = &ptr.s[begin..ptr.p];
    let mut value = 0;
    for (name, v) in MONTH_LOOKUP {
        if eq_ci(word, name.as_bytes()) {
            value = *v;
        }
    }
    value
}

fn get_month(ptr: &mut Ptr) -> i64 {
    while matches!(ptr.c(), b' ' | b'\t' | b'-' | b'.' | b'/') {
        ptr.adv();
    }
    lookup_month(ptr)
}

pub fn eat_spaces(ptr: &mut Ptr) {
    loop {
        if ptr.c() == b' ' || ptr.c() == b'\t' {
            ptr.adv();
            continue;
        }
        if ptr.c() == 0xe2 && ptr.at(1) == 0x80 && ptr.at(2) == 0xaf {
            ptr.p += 3;
            continue;
        }
        if ptr.c() == 0xc2 && ptr.at(1) == 0xa0 {
            ptr.p += 2;
            continue;
        }
        break;
    }
}

fn eat_until_separator(ptr: &mut Ptr) {
    ptr.adv();
    while !b" \t.,:;/-0123456789".contains(&ptr.c()) && ptr.c() != 0 {
        ptr.adv();
    }
}

pub fn lookup_relunit(ptr: &mut Ptr) -> Option<(i32, i64)> {
    let begin = ptr.p;
    while !matches!(ptr.c(), 0 | b' ' | b',' | b'\t' | b';' | b':' | b'/' | b'.' | b'-' | b'(' | b')') {
        ptr.adv();
    }
    let word = &ptr.s[begin..ptr.p];
    for (name, unit, mult) in RELUNIT_LOOKUP {
        if eq_ci(word, name.as_bytes()) {
            return Some((*unit, *mult));
        }
    }
    None
}

fn add_with_overflow(errs: &mut Vec<(i32, &'static str)>, e: &mut i64, amount: i64, multiplier: i64) {
    let prod = amount.wrapping_mul(multiplier);
    match e.checked_add(prod) {
        Some(v) => *e = v,
        None => {
            *e = e.wrapping_add(prod);
            errs.push((ERR_NUMBER_OUT_OF_RANGE, "Number out of range"));
        }
    }
}

fn unhave_time(t: &mut Time) {
    t.have_time = 0;
    t.h = 0;
    t.i = 0;
    t.s = 0;
    t.us = 0;
}

fn set_relative(ptr: &mut Ptr, amount: i64, behavior: i32, t: &mut Time, errs: &mut Vec<(i32, &'static str)>, keep_time: bool) {
    let (unit, mult) = match lookup_relunit(ptr) {
        Some(x) => x,
        None => return,
    };
    match unit {
        UNIT_MICROSEC => add_with_overflow(errs, &mut t.relative.us, amount, mult),
        UNIT_SECOND => add_with_overflow(errs, &mut t.relative.s, amount, mult),
        UNIT_MINUTE => add_with_overflow(errs, &mut t.relative.i, amount, mult),
        UNIT_HOUR => add_with_overflow(errs, &mut t.relative.h, amount, mult),
        UNIT_DAY => add_with_overflow(errs, &mut t.relative.d, amount, mult),
        UNIT_MONTH => add_with_overflow(errs, &mut t.relative.m, amount, mult),
        UNIT_YEAR => add_with_overflow(errs, &mut t.relative.y, amount, mult),
        UNIT_WEEKDAY => {
            t.have_relative = 1;
            t.relative.have_weekday_relative = 1;
            if !keep_time {
                unhave_time(t);
            }
            t.relative.d = t.relative.d.wrapping_add((if amount > 0 { amount - 1 } else { amount }).wrapping_mul(7));
            t.relative.weekday = mult as i32;
            t.relative.weekday_behavior = behavior;
        }
        UNIT_SPECIAL => {
            t.have_relative = 1;
            t.relative.have_special_relative = 1;
            if !keep_time {
                unhave_time(t);
            }
            t.relative.special_type = mult as u32;
            t.relative.special_amount = amount;
        }
        _ => {}
    }
}

/// abbr_search: returns (abbr, type(dst), gmtoffset, full_tz_name)
pub fn abbr_search(word: &[u8], gmtoffset: i64, isdst: i32) -> Option<(&'static str, i32, i32, Option<&'static str>)> {
    if eq_ci(word, b"utc") || eq_ci(word, b"gmt") {
        return Some(("utc", 0, 0, Some("UTC")));
    }
    let mut first_found = None;
    for e in TIMEZONEMAP {
        if eq_ci(word, e.0.as_bytes()) {
            if first_found.is_none() {
                first_found = Some(*e);
                if gmtoffset == -1 {
                    return Some(*e);
                }
            }
            if e.2 as i64 == gmtoffset {
                return Some(*e);
            }
        }
    }
    if first_found.is_some() {
        return first_found;
    }
    for e in FALLBACKMAP {
        if e.2 as i64 == gmtoffset && e.1 == isdst {
            return Some(*e);
        }
    }
    None
}

const MAX_ABBR_LEN: usize = 6;

fn lookup_abbr(ptr: &mut Ptr, dst: &mut i32, found: &mut bool) -> (i64, String) {
    let begin = ptr.p;
    while ptr.c().is_ascii_alphanumeric() || matches!(ptr.c(), b'/' | b'_' | b'-' | b'+') {
        ptr.adv();
    }
    let word = &ptr.s[begin..ptr.p];
    let mut value = 0;
    if word.len() < MAX_ABBR_LEN {
        if let Some(tp) = abbr_search(word, -1, 0) {
            value = tp.2 as i64;
            *dst = tp.1;
            value -= tp.1 as i64 * 3600;
            *found = true;
        } else {
            *found = false;
        }
    } else {
        *found = false;
    }
    (value, String::from_utf8_lossy(word).into_owned())
}

fn shour(a: i64) -> i64 {
    a * 3600
}
fn smin(a: i64) -> i64 {
    a * 60
}

fn parse_tz_cor(ptr: &mut Ptr, tz_not_found: &mut bool) -> i64 {
    let begin = ptr.p;
    *tz_not_found = true;
    while ptr.c().is_ascii_digit() || ptr.c() == b':' {
        ptr.adv();
    }
    let s = &ptr.s[begin..];
    let b = |k: usize| *s.get(k).unwrap_or(&0);
    let len = ptr.p - begin;
    match len {
        1 | 2 => {
            *tz_not_found = false;
            return shour(strtol(s));
        }
        3 | 4 => {
            if b(1) == b':' {
                *tz_not_found = false;
                return shour(strtol(s)) + smin(strtol(&s[2..]));
            } else if b(2) == b':' {
                *tz_not_found = false;
                return shour(strtol(s)) + smin(strtol(&s[3..]));
            } else {
                *tz_not_found = false;
                let tmp = strtol(s);
                return shour(tmp / 100) + smin(tmp % 100);
            }
        }
        5 => {
            if b(2) == b':' {
                *tz_not_found = false;
                return shour(strtol(s)) + smin(strtol(&s[3..]));
            }
        }
        6 => {
            *tz_not_found = false;
            let tmp = strtol(s);
            return shour(tmp / 10000) + smin((tmp / 100) % 100) + (tmp % 100);
        }
        8 => {
            if b(2) == b':' && b(5) == b':' {
                *tz_not_found = false;
                return shour(strtol(s)) + smin(strtol(&s[3..])) + strtol(&s[6..]);
            }
        }
        _ => {}
    }
    0
}

/// timelib_parse_zone
pub fn parse_zone(ptr: &mut Ptr, dst: &mut i32, t: &mut Time, tz_not_found: &mut bool) -> i64 {
    let mut paren_count = 0;
    *tz_not_found = false;
    while ptr.c() == b' ' || ptr.c() == b'\t' || ptr.c() == b'(' {
        if ptr.c() == b'(' {
            paren_count += 1;
        }
        ptr.adv();
    }
    if ptr.c() == b'G' && ptr.at(1) == b'M' && ptr.at(2) == b'T' && (ptr.at(3) == b'+' || ptr.at(3) == b'-') {
        ptr.p += 3;
    }
    let retval = if ptr.c() == b'+' {
        ptr.adv();
        t.is_localtime = 1;
        t.zone_type = ZONETYPE_OFFSET;
        t.dst = 0;
        parse_tz_cor(ptr, tz_not_found)
    } else if ptr.c() == b'-' {
        ptr.adv();
        t.is_localtime = 1;
        t.zone_type = ZONETYPE_OFFSET;
        t.dst = 0;
        -parse_tz_cor(ptr, tz_not_found)
    } else {
        let mut found = false;
        t.is_localtime = 1;
        let (offset, tz_abbr) = lookup_abbr(ptr, dst, &mut found);
        let mut found_count = found as i32;
        if found {
            t.zone_type = ZONETYPE_ABBR;
            t.dst = *dst;
            t.tz_abbr_update(&tz_abbr);
        }
        if !found || tz_abbr == "UTC" {
            if let Some(res) = parse_tzfile(&tz_abbr) {
                t.tz_info = Some(res);
                t.zone_type = ZONETYPE_ID;
                found_count += 1;
            }
        }
        *tz_not_found = found_count == 0;
        offset
    };
    while paren_count > 0 && ptr.c() == b')' {
        ptr.adv();
        paren_count -= 1;
    }
    retval
}

fn process_year(y: &mut i64, len: usize) {
    if *y == UNSET || len >= 4 {
    } else if *y < 100 {
        if *y < 70 {
            *y += 2000;
        } else {
            *y += 1900;
        }
    }
}

/// Result of a scan step
enum Step {
    Continue,
    Eoi,
}

fn scan(s: &mut State, input_len: usize) -> Step {
    loop {
        let cursor = s.cur;
        s.tok = cursor;
        // YYFILL check: the C scanner stops once fewer than YYMAXFILL bytes
        // remain, i.e. once we've moved past the first padding NUL.
        if cursor > input_len {
            return Step::Eoi;
        }
        let (rule_idx, end) = with_scanner(|sc| sc.longest_match(&s.str, cursor)).unwrap_or((RULES.len() - 1, cursor + 1));
        let rule = RULES[rule_idx].1;
        s.cur = end;
        let tok: Vec<u8> = s.str[s.tok..s.cur].to_vec();
        let mut ptr = Ptr::new(&tok);
        // C strings: content stops at the first NUL.
        if let Some(z) = tok.iter().position(|&c| c == 0) {
            ptr.s = &tok[..z];
        }
        let mut errs: Vec<(i32, &'static str)> = Vec::new();

        macro_rules! have_time {
            () => {
                if s.time.have_time != 0 {
                    s.add_error(ERR_DOUBLE_TIME, "Double time specification");
                    return Step::Continue;
                } else {
                    s.time.have_time = 1;
                    s.time.h = 0;
                    s.time.i = 0;
                    s.time.s = 0;
                    s.time.us = 0;
                }
            };
        }
        macro_rules! have_date {
            () => {
                if s.time.have_date != 0 {
                    s.add_error(ERR_DOUBLE_DATE, "Double date specification");
                    return Step::Continue;
                } else {
                    s.time.have_date = 1;
                }
            };
        }
        macro_rules! unhave_date {
            () => {
                s.time.have_date = 0;
                s.time.d = 0;
                s.time.m = 0;
                s.time.y = 0;
            };
        }
        macro_rules! have_relative {
            () => {
                s.time.have_relative = 1;
            };
        }
        macro_rules! have_tz {
            () => {
                if s.time.have_zone != 0 {
                    if s.time.have_zone > 1 {
                        s.add_error(ERR_DOUBLE_TZ, "Double timezone specification");
                    } else {
                        s.add_warning(WARN_DOUBLE_TZ, "Double timezone specification");
                    }
                    s.time.have_zone += 1;
                    return Step::Continue;
                } else {
                    s.time.have_zone += 1;
                }
            };
        }
        macro_rules! parse_zone_into_time {
            () => {{
                let mut tz_not_found = false;
                let mut dst = s.time.dst;
                let z = parse_zone(&mut ptr, &mut dst, &mut s.time, &mut tz_not_found);
                s.time.z = z;
                // parse_zone sets t.dst itself for abbreviations; the C code
                // passes &s->time->dst as well, so keep them in sync.
                if s.time.zone_type == ZONETYPE_ABBR {
                    s.time.dst = dst;
                } else if s.time.zone_type != ZONETYPE_OFFSET {
                    s.time.dst = dst;
                }
                if tz_not_found {
                    s.add_error(ERR_TZID_NOT_FOUND, "The timezone could not be found in the database");
                }
            }};
        }

        match rule {
            Rule::Yesterday => {
                have_relative!();
                unhave_time(&mut s.time);
                s.time.relative.d = -1;
            }
            Rule::Now => {}
            Rule::Noon => {
                unhave_time(&mut s.time);
                have_time!();
                s.time.h = 12;
            }
            Rule::MidnightToday => {
                unhave_time(&mut s.time);
            }
            Rule::Tomorrow => {
                have_relative!();
                unhave_time(&mut s.time);
                s.time.relative.d = 1;
            }
            Rule::Timestamp => {
                have_relative!();
                unhave_date!();
                unhave_time(&mut s.time);
                have_tz!();
                let i = get_signed_nr(&mut errs, &mut ptr, 24);
                s.time.y = 1970;
                s.time.m = 1;
                s.time.d = 1;
                s.time.h = 0;
                s.time.i = 0;
                s.time.s = 0;
                s.time.us = 0;
                s.time.relative.s = s.time.relative.s.wrapping_add(i);
                s.time.is_localtime = 1;
                s.time.zone_type = ZONETYPE_OFFSET;
                s.time.z = 0;
                s.time.dst = 0;
            }
            Rule::TimestampMs => {
                have_relative!();
                unhave_date!();
                unhave_time(&mut s.time);
                have_tz!();
                let is_negative = ptr.at(1) == b'-';
                let i = get_signed_nr(&mut errs, &mut ptr, 24);
                let ptr_before = ptr.p;
                let mut us = get_signed_nr(&mut errs, &mut ptr, 6);
                us = (us as f64 * 10f64.powi(7 - (ptr.p - ptr_before) as i32)) as i64;
                if is_negative {
                    us = -us;
                }
                s.time.y = 1970;
                s.time.m = 1;
                s.time.d = 1;
                s.time.h = 0;
                s.time.i = 0;
                s.time.s = 0;
                s.time.us = 0;
                s.time.relative.s = s.time.relative.s.wrapping_add(i);
                s.time.relative.us = us;
                s.time.is_localtime = 1;
                s.time.zone_type = ZONETYPE_OFFSET;
                s.time.z = 0;
                s.time.dst = 0;
            }
            Rule::FirstLastDayOf => {
                have_relative!();
                if ptr.c() == b'l' || ptr.c() == b'L' {
                    s.time.relative.first_last_day_of = SPECIAL_LAST_DAY_OF_MONTH;
                } else {
                    s.time.relative.first_last_day_of = SPECIAL_FIRST_DAY_OF_MONTH;
                }
            }
            Rule::BackFrontOf => {
                unhave_time(&mut s.time);
                have_time!();
                if ptr.c() == b'b' {
                    s.time.h = get_nr(&mut ptr, 2);
                    s.time.i = 15;
                } else {
                    s.time.h = get_nr(&mut ptr, 2) - 1;
                    s.time.i = 45;
                }
                if ptr.c() != 0 {
                    eat_spaces(&mut ptr);
                    let h = s.time.h;
                    s.time.h += timelib_meridian(&mut ptr, h);
                }
            }
            Rule::WeekdayOf => {
                have_relative!();
                s.time.relative.have_special_relative = 1;
                let mut behavior = 0;
                let i = get_relative_text(&mut ptr, &mut behavior);
                eat_spaces(&mut ptr);
                if i > 0 {
                    s.time.relative.special_type = SPECIAL_DAY_OF_WEEK_IN_MONTH;
                    set_relative(&mut ptr, i, 1, &mut s.time, &mut errs, false);
                } else {
                    s.time.relative.special_type = SPECIAL_LAST_DAY_OF_WEEK_IN_MONTH;
                    set_relative(&mut ptr, i, behavior, &mut s.time, &mut errs, false);
                }
            }
            Rule::Time12 => {
                have_time!();
                s.time.h = get_nr(&mut ptr, 2);
                if ptr.c() == b':' || ptr.c() == b'.' {
                    s.time.i = get_nr(&mut ptr, 2);
                    if ptr.c() == b':' || ptr.c() == b'.' {
                        s.time.s = get_nr(&mut ptr, 2);
                    }
                }
                eat_spaces(&mut ptr);
                let h = s.time.h;
                s.time.h += timelib_meridian(&mut ptr, h);
            }
            Rule::MssqlTime => {
                have_time!();
                s.time.h = get_nr(&mut ptr, 2);
                s.time.i = get_nr(&mut ptr, 2);
                if ptr.c() == b':' || ptr.c() == b'.' {
                    s.time.s = get_nr(&mut ptr, 2);
                    if ptr.c() == b':' || ptr.c() == b'.' {
                        s.time.us = get_frac_nr(&mut ptr);
                    }
                }
                eat_spaces(&mut ptr);
                let h = s.time.h;
                s.time.h += timelib_meridian(&mut ptr, h);
            }
            Rule::Time24 => {
                have_time!();
                s.time.h = get_nr(&mut ptr, 2);
                if ptr.c() == b':' || ptr.c() == b'.' {
                    s.time.i = get_nr(&mut ptr, 2);
                    if ptr.c() == b':' || ptr.c() == b'.' {
                        s.time.s = get_nr(&mut ptr, 2);
                        if ptr.c() == b'.' {
                            s.time.us = get_frac_nr(&mut ptr);
                        }
                    }
                }
                if ptr.c() != 0 {
                    parse_zone_into_time!();
                }
            }
            Rule::GnuNoColon => {
                match s.time.have_time {
                    0 => {
                        s.time.h = get_nr(&mut ptr, 2);
                        s.time.i = get_nr(&mut ptr, 2);
                        s.time.s = 0;
                    }
                    1 => {
                        s.time.y = get_nr(&mut ptr, 4);
                    }
                    _ => {
                        s.add_error(ERR_DOUBLE_TIME, "Double time specification");
                        return Step::Continue;
                    }
                }
                s.time.have_time += 1;
            }
            Rule::Iso8601NoColon => {
                have_time!();
                s.time.h = get_nr(&mut ptr, 2);
                s.time.i = get_nr(&mut ptr, 2);
                s.time.s = get_nr(&mut ptr, 2);
                if ptr.c() != 0 {
                    parse_zone_into_time!();
                }
            }
            Rule::American => {
                have_date!();
                s.time.m = get_nr(&mut ptr, 2);
                s.time.d = get_nr(&mut ptr, 2);
                if ptr.c() == b'/' {
                    let (y, len) = get_nr_ex(&mut ptr, 4);
                    s.time.y = y;
                    process_year(&mut s.time.y, len);
                }
            }
            Rule::IsoDate4 => {
                have_date!();
                s.time.y = get_signed_nr(&mut errs, &mut ptr, 4);
                s.time.m = get_nr(&mut ptr, 2);
                s.time.d = get_nr(&mut ptr, 2);
            }
            Rule::IsoDate2 => {
                have_date!();
                let (y, len) = get_nr_ex(&mut ptr, 4);
                s.time.y = y;
                s.time.m = get_nr(&mut ptr, 2);
                s.time.d = get_nr(&mut ptr, 2);
                process_year(&mut s.time.y, len);
            }
            Rule::IsoDateX => {
                have_date!();
                s.time.y = get_signed_nr(&mut errs, &mut ptr, 19);
                s.time.m = get_nr(&mut ptr, 2);
                s.time.d = get_nr(&mut ptr, 2);
            }
            Rule::GnuDateShorter => {
                have_date!();
                let (y, len) = get_nr_ex(&mut ptr, 4);
                s.time.y = y;
                s.time.m = get_nr(&mut ptr, 2);
                s.time.d = 1;
                process_year(&mut s.time.y, len);
            }
            Rule::GnuDateShort => {
                have_date!();
                let (y, len) = get_nr_ex(&mut ptr, 4);
                s.time.y = y;
                s.time.m = get_nr(&mut ptr, 2);
                s.time.d = get_nr(&mut ptr, 2);
                process_year(&mut s.time.y, len);
            }
            Rule::DateFull => {
                have_date!();
                s.time.d = get_nr(&mut ptr, 2);
                skip_day_suffix(&mut ptr);
                s.time.m = get_month(&mut ptr);
                let (y, len) = get_nr_ex(&mut ptr, 4);
                s.time.y = y;
                process_year(&mut s.time.y, len);
            }
            Rule::PointedDate4 => {
                have_date!();
                s.time.d = get_nr(&mut ptr, 2);
                s.time.m = get_nr(&mut ptr, 2);
                s.time.y = get_nr(&mut ptr, 4);
            }
            Rule::PointedDate2 => {
                have_date!();
                s.time.d = get_nr(&mut ptr, 2);
                s.time.m = get_nr(&mut ptr, 2);
                let (y, len) = get_nr_ex(&mut ptr, 2);
                s.time.y = y;
                process_year(&mut s.time.y, len);
            }
            Rule::DateNoDay => {
                have_date!();
                s.time.m = get_month(&mut ptr);
                let (y, len) = get_nr_ex(&mut ptr, 4);
                s.time.y = y;
                s.time.d = 1;
                process_year(&mut s.time.y, len);
            }
            Rule::DateNoDayRev => {
                have_date!();
                let (y, len) = get_nr_ex(&mut ptr, 4);
                s.time.y = y;
                s.time.m = get_month(&mut ptr);
                s.time.d = 1;
                process_year(&mut s.time.y, len);
            }
            Rule::DateTextual => {
                have_date!();
                s.time.m = get_month(&mut ptr);
                s.time.d = get_nr(&mut ptr, 2);
                let (y, len) = get_nr_ex(&mut ptr, 4);
                s.time.y = y;
                process_year(&mut s.time.y, len);
            }
            Rule::DateNoYearRev => {
                have_date!();
                s.time.d = get_nr(&mut ptr, 2);
                skip_day_suffix(&mut ptr);
                s.time.m = get_month(&mut ptr);
            }
            Rule::DateNoColon => {
                have_date!();
                s.time.y = get_nr(&mut ptr, 4);
                s.time.m = get_nr(&mut ptr, 2);
                s.time.d = get_nr(&mut ptr, 2);
            }
            Rule::XmlRpc => {
                have_time!();
                have_date!();
                s.time.y = get_nr(&mut ptr, 4);
                s.time.m = get_nr(&mut ptr, 2);
                s.time.d = get_nr(&mut ptr, 2);
                s.time.h = get_nr(&mut ptr, 2);
                s.time.i = get_nr(&mut ptr, 2);
                s.time.s = get_nr(&mut ptr, 2);
                if ptr.c() == b'.' {
                    s.time.us = get_frac_nr(&mut ptr);
                    if ptr.c() != 0 {
                        parse_zone_into_time!();
                    }
                }
            }
            Rule::PgYdotd => {
                have_date!();
                let (y, len) = get_nr_ex(&mut ptr, 4);
                s.time.y = y;
                s.time.d = get_nr(&mut ptr, 3);
                s.time.m = 1;
                process_year(&mut s.time.y, len);
            }
            Rule::IsoWeekday => {
                have_date!();
                have_relative!();
                s.time.y = get_nr(&mut ptr, 4);
                let w = get_nr(&mut ptr, 2);
                let d = get_nr(&mut ptr, 1);
                s.time.m = 1;
                s.time.d = 1;
                s.time.relative.d = daynr_from_weeknr(s.time.y, w, d);
            }
            Rule::IsoWeek => {
                have_date!();
                have_relative!();
                s.time.y = get_nr(&mut ptr, 4);
                let w = get_nr(&mut ptr, 2);
                s.time.m = 1;
                s.time.d = 1;
                s.time.relative.d = daynr_from_weeknr(s.time.y, w, 1);
            }
            Rule::PgTextShort => {
                have_date!();
                s.time.m = get_month(&mut ptr);
                s.time.d = get_nr(&mut ptr, 2);
                let (y, len) = get_nr_ex(&mut ptr, 4);
                s.time.y = y;
                process_year(&mut s.time.y, len);
            }
            Rule::PgTextReverse => {
                have_date!();
                let (y, len) = get_nr_ex(&mut ptr, 4);
                s.time.y = y;
                s.time.m = get_month(&mut ptr);
                s.time.d = get_nr(&mut ptr, 2);
                process_year(&mut s.time.y, len);
            }
            Rule::Clf => {
                have_time!();
                have_date!();
                s.time.d = get_nr(&mut ptr, 2);
                s.time.m = get_month(&mut ptr);
                s.time.y = get_nr(&mut ptr, 4);
                s.time.h = get_nr(&mut ptr, 2);
                s.time.i = get_nr(&mut ptr, 2);
                s.time.s = get_nr(&mut ptr, 2);
                eat_spaces(&mut ptr);
                parse_zone_into_time!();
            }
            Rule::Year4 => {
                s.time.y = get_nr(&mut ptr, 4);
            }
            Rule::Ago => {
                let r = &mut s.time.relative;
                r.y = 0i64.wrapping_sub(r.y);
                r.m = 0i64.wrapping_sub(r.m);
                r.d = 0i64.wrapping_sub(r.d);
                r.h = 0i64.wrapping_sub(r.h);
                r.i = 0i64.wrapping_sub(r.i);
                r.s = 0i64.wrapping_sub(r.s);
                r.weekday = -r.weekday;
                if r.weekday == 0 {
                    r.weekday = -7;
                }
                if r.have_special_relative != 0 && r.special_type == SPECIAL_WEEKDAY {
                    r.special_amount = 0i64.wrapping_sub(r.special_amount);
                }
            }
            Rule::DayText => {
                have_relative!();
                s.time.relative.have_weekday_relative = 1;
                unhave_time(&mut s.time);
                if let Some((_, mult)) = lookup_relunit(&mut ptr) {
                    s.time.relative.weekday = mult as i32;
                }
                if s.time.relative.weekday_behavior != 2 {
                    s.time.relative.weekday_behavior = 1;
                }
            }
            Rule::RelativeTextWeek => {
                have_relative!();
                while ptr.c() != 0 {
                    let mut behavior = 0;
                    let i = get_relative_text(&mut ptr, &mut behavior);
                    eat_spaces(&mut ptr);
                    set_relative(&mut ptr, i, behavior, &mut s.time, &mut errs, false);
                    s.time.relative.weekday_behavior = 2;
                    if s.time.relative.have_weekday_relative == 0 {
                        s.time.have_relative = 1;
                        s.time.relative.have_weekday_relative = 1;
                        s.time.relative.weekday = 1;
                    }
                }
            }
            Rule::RelativeText => {
                have_relative!();
                while ptr.c() != 0 {
                    let mut behavior = 0;
                    let i = get_relative_text(&mut ptr, &mut behavior);
                    eat_spaces(&mut ptr);
                    set_relative(&mut ptr, i, behavior, &mut s.time, &mut errs, false);
                }
            }
            Rule::MonthText => {
                have_date!();
                s.time.m = lookup_month(&mut ptr);
            }
            Rule::TzCorrection => {
                have_tz!();
                eat_spaces(&mut ptr);
                parse_zone_into_time!();
            }
            Rule::DateShortWithTime12 => {
                have_date!();
                s.time.m = get_month(&mut ptr);
                s.time.d = get_nr(&mut ptr, 2);
                have_time!();
                s.time.h = get_nr(&mut ptr, 2);
                s.time.i = get_nr(&mut ptr, 2);
                if ptr.c() == b':' || ptr.c() == b'.' {
                    s.time.s = get_nr(&mut ptr, 2);
                    if ptr.c() == b'.' {
                        s.time.us = get_frac_nr(&mut ptr);
                    }
                }
                let h = s.time.h;
                s.time.h += timelib_meridian(&mut ptr, h);
            }
            Rule::DateShortWithTime24 => {
                have_date!();
                s.time.m = get_month(&mut ptr);
                s.time.d = get_nr(&mut ptr, 2);
                have_time!();
                s.time.h = get_nr(&mut ptr, 2);
                s.time.i = get_nr(&mut ptr, 2);
                if ptr.c() == b':' {
                    s.time.s = get_nr(&mut ptr, 2);
                    if ptr.c() == b'.' {
                        s.time.us = get_frac_nr(&mut ptr);
                    }
                }
                if ptr.c() != 0 {
                    parse_zone_into_time!();
                }
            }
            Rule::Relative => {
                have_relative!();
                while ptr.c() != 0 {
                    let i = get_signed_nr(&mut errs, &mut ptr, 24);
                    eat_spaces(&mut ptr);
                    set_relative(&mut ptr, i, 1, &mut s.time, &mut errs, true);
                }
            }
            Rule::Punct | Rule::Space | Rule::Nul => {
                for (c, m) in errs.drain(..) {
                    s.add_error(c, m);
                }
                continue;
            }
            Rule::Any => {
                s.add_error(ERR_UNEXPECTED_CHARACTER, "Unexpected character");
                continue;
            }
        }
        for (c, m) in errs.drain(..) {
            s.add_error(c, m);
        }
        return Step::Continue;
    }
}

fn is_c_space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

/// timelib_strtotime
pub fn strtotime(input: &[u8]) -> (Time, ErrorContainer) {
    let mut s0 = 0usize;
    let len = input.len();
    let mut e: isize = len as isize - 1;
    if len > 0 {
        while is_c_space(input[s0]) && (s0 as isize) < e {
            s0 += 1;
        }
        while e >= 0 && is_c_space(input[e as usize]) && e > s0 as isize {
            e -= 1;
        }
    }
    let mut errors = ErrorContainer::default();
    if e - (s0 as isize) < 0 {
        let mut t = Time::new();
        errors.error_messages.push(ErrorMessage { error_code: ERR_EMPTY_STRING, position: 0, character: 0, message: "Empty string".to_string() });
        t.y = UNSET;
        t.d = UNSET;
        t.m = UNSET;
        t.h = UNSET;
        t.i = UNSET;
        t.s = UNSET;
        t.us = UNSET;
        t.dst = UNSET as i32;
        t.z = UNSET;
        t.is_localtime = 0;
        t.zone_type = 0;
        return (t, errors);
    }
    let e = (e + 1) as usize;
    let slice = &input[s0..e];
    let mut buf = slice.to_vec();
    buf.extend(std::iter::repeat_n(0u8, 36));
    let mut t = Time::new();
    t.y = UNSET;
    t.d = UNSET;
    t.m = UNSET;
    t.h = UNSET;
    t.i = UNSET;
    t.s = UNSET;
    t.us = UNSET;
    t.z = UNSET;
    t.dst = UNSET as i32;
    t.is_localtime = 0;
    t.zone_type = 0;
    t.relative.days = UNSET;
    let mut st = State { str: buf, tok: 0, cur: 0, errors, time: t };
    let input_len = slice.len();
    while let Step::Continue = scan(&mut st, input_len) {}
    if st.time.have_time != 0 && !valid_time(st.time.h, st.time.i, st.time.s) {
        st.add_warning(WARN_INVALID_TIME, "The parsed time was invalid");
    }
    if st.time.have_date != 0 && !valid_date(st.time.y, st.time.m, st.time.d) {
        st.add_warning(WARN_INVALID_DATE, "The parsed date was invalid");
    }
    (st.time, st.errors)
}

fn time_reset_fields(t: &mut Time) {
    t.y = 1970;
    t.m = 1;
    t.d = 1;
    t.h = 0;
    t.i = 0;
    t.s = 0;
    t.us = 0;
    t.tz_info = None;
}

fn time_reset_unset_fields(t: &mut Time) {
    if t.y == UNSET {
        t.y = 1970;
    }
    if t.m == UNSET {
        t.m = 1;
    }
    if t.d == UNSET {
        t.d = 1;
    }
    if t.h == UNSET {
        t.h = 0;
    }
    if t.i == UNSET {
        t.i = 0;
    }
    if t.s == UNSET {
        t.s = 0;
    }
    if t.us == UNSET {
        t.us = 0;
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Fmt {
    AllowExtra,
    AnySeparator,
    DayTwoDigit,
    DayOfYear,
    DaySuffix,
    EpochSeconds,
    Escape,
    Hour12,
    Hour24,
    Meridian,
    Microsecond,
    Millisecond,
    Minute,
    Month,
    RandomChar,
    ResetAll,
    ResetAllWhenNotSet,
    Second,
    Separator,
    SkipToSeparator,
    TextualDay,
    TextualMonth,
    TimezoneOffset,
    Whitespace,
    YearTwoDigit,
    YearFourDigit,
    YearExpanded,
    Literal,
}

fn lookup_format(c: u8) -> Fmt {
    match c {
        b'+' => Fmt::AllowExtra,
        b'#' => Fmt::AnySeparator,
        b'j' | b'd' => Fmt::DayTwoDigit,
        b'z' => Fmt::DayOfYear,
        b'S' => Fmt::DaySuffix,
        b'U' => Fmt::EpochSeconds,
        b'\\' => Fmt::Escape,
        b'h' | b'g' => Fmt::Hour12,
        b'H' | b'G' => Fmt::Hour24,
        b'a' | b'A' => Fmt::Meridian,
        b'u' => Fmt::Microsecond,
        b'v' => Fmt::Millisecond,
        b'i' => Fmt::Minute,
        b'n' | b'm' => Fmt::Month,
        b'?' => Fmt::RandomChar,
        b'!' => Fmt::ResetAll,
        b'|' => Fmt::ResetAllWhenNotSet,
        b's' => Fmt::Second,
        b';' | b':' | b'/' | b'.' | b',' | b'-' | b'(' | b')' => Fmt::Separator,
        b'*' => Fmt::SkipToSeparator,
        b'D' | b'l' => Fmt::TextualDay,
        b'M' | b'F' => Fmt::TextualMonth,
        b'e' | b'P' | b'p' | b'T' | b'O' => Fmt::TimezoneOffset,
        b' ' => Fmt::Whitespace,
        b'y' => Fmt::YearTwoDigit,
        b'Y' => Fmt::YearFourDigit,
        b'x' | b'X' => Fmt::YearExpanded,
        _ => Fmt::Literal,
    }
}

/// timelib_parse_from_format
pub fn parse_from_format(format: &[u8], string: &[u8]) -> (Time, ErrorContainer) {
    let mut errors = ErrorContainer::default();
    let mut t = Time::new();
    t.y = UNSET;
    t.d = UNSET;
    t.m = UNSET;
    t.h = UNSET;
    t.i = UNSET;
    t.s = UNSET;
    t.us = UNSET;
    t.z = UNSET;
    t.dst = UNSET as i32;
    t.is_localtime = 0;
    t.zone_type = 0;
    let iso_year = UNSET;
    let iso_week_of_year = UNSET;
    let iso_day_of_week = UNSET;
    let mut allow_extra = false;

    // C strings: both stop at first NUL
    let fz = format.iter().position(|&c| c == 0).unwrap_or(format.len());
    let format = &format[..fz];
    let sz = string.iter().position(|&c| c == 0).unwrap_or(string.len());
    let string = &string[..sz];

    let mut fi = 0usize;
    let mut ptr = Ptr::new(string);
    let fc = |i: usize| *format.get(i).unwrap_or(&0);

    while fc(fi) != 0 && ptr.c() != 0 {
        let begin = ptr.p;
        let mut errs: Vec<(i32, &'static str)> = Vec::new();
        macro_rules! check_number {
            () => {
                if !ptr.c().is_ascii_digit() {
                    add_pbf_error(&mut errors, ERR_UNEXPECTED_DATA, "Unexpected data found.", string, begin);
                }
            };
        }
        macro_rules! check_signed_number {
            () => {
                if !ptr.c().is_ascii_digit() && ptr.c() != b'+' && ptr.c() != b'-' {
                    add_pbf_error(&mut errors, ERR_UNEXPECTED_DATA, "Unexpected data found.", string, begin);
                }
            };
        }
        match lookup_format(fc(fi)) {
            Fmt::TextualDay => match lookup_relunit(&mut ptr) {
                None => {
                    add_pbf_error(&mut errors, ERR_NO_TEXTUAL_DAY, "A textual day could not be found", string, begin);
                }
                Some((_, mult)) => {
                    t.have_relative = 1;
                    t.relative.have_weekday_relative = 1;
                    t.relative.weekday = mult as i32;
                    t.relative.weekday_behavior = 1;
                }
            },
            Fmt::DayTwoDigit => {
                check_number!();
                t.d = get_nr(&mut ptr, 2);
                if t.d == UNSET {
                    add_pbf_error(&mut errors, ERR_NO_TWO_DIGIT_DAY, "A two digit day could not be found", string, begin);
                } else {
                    t.have_date = 1;
                }
            }
            Fmt::DaySuffix => skip_day_suffix(&mut ptr),
            Fmt::DayOfYear => {
                check_number!();
                if t.y == UNSET {
                    add_pbf_error(&mut errors, ERR_MERIDIAN_BEFORE_HOUR, "A 'day of year' can only come after a year has been found", string, begin);
                }
                let tmp = get_nr(&mut ptr, 3);
                if tmp == UNSET {
                    add_pbf_error(&mut errors, ERR_NO_THREE_DIGIT_DAY_OF_YEAR, "A three digit day-of-year could not be found", string, begin);
                } else if t.y != UNSET {
                    t.have_date = 1;
                    t.m = 1;
                    t.d = tmp + 1;
                    do_normalize(&mut t);
                }
            }
            Fmt::Month => {
                check_number!();
                t.m = get_nr(&mut ptr, 2);
                if t.m == UNSET {
                    add_pbf_error(&mut errors, ERR_NO_TWO_DIGIT_MONTH, "A two digit month could not be found", string, begin);
                } else {
                    t.have_date = 1;
                }
            }
            Fmt::TextualMonth => {
                let tmp = lookup_month(&mut ptr);
                if tmp == 0 {
                    add_pbf_error(&mut errors, ERR_NO_TEXTUAL_MONTH, "A textual month could not be found", string, begin);
                } else {
                    t.have_date = 1;
                    t.m = tmp;
                }
            }
            Fmt::YearTwoDigit => {
                check_number!();
                let (y, len) = get_nr_ex(&mut ptr, 2);
                t.y = y;
                if y == UNSET {
                    add_pbf_error(&mut errors, ERR_NO_TWO_DIGIT_YEAR, "A two digit year could not be found", string, begin);
                } else {
                    t.have_date = 1;
                    process_year(&mut t.y, len);
                }
            }
            Fmt::YearFourDigit => {
                check_number!();
                t.y = get_nr(&mut ptr, 4);
                if t.y == UNSET {
                    add_pbf_error(&mut errors, ERR_NO_FOUR_DIGIT_YEAR, "A four digit year could not be found", string, begin);
                } else {
                    t.have_date = 1;
                }
            }
            Fmt::YearExpanded => {
                check_signed_number!();
                t.y = get_signed_nr(&mut errs, &mut ptr, 19);
                for (c, m) in errs.drain(..) {
                    add_pbf_error(&mut errors, c, m, string, begin);
                }
                if t.y == UNSET {
                    add_pbf_error(&mut errors, ERR_NO_FOUR_DIGIT_YEAR, "An expanded digit year could not be found", string, begin);
                } else {
                    t.have_date = 1;
                }
            }
            Fmt::Hour12 => {
                check_number!();
                t.h = get_nr(&mut ptr, 2);
                if t.h == UNSET {
                    add_pbf_error(&mut errors, ERR_NO_TWO_DIGIT_HOUR, "A two digit hour could not be found", string, begin);
                } else if t.h > 12 {
                    add_pbf_error(&mut errors, ERR_HOUR_LARGER_THAN_12, "Hour cannot be higher than 12", string, begin);
                } else {
                    t.have_time = 1;
                }
            }
            Fmt::Hour24 => {
                check_number!();
                t.h = get_nr(&mut ptr, 2);
                if t.h == UNSET {
                    add_pbf_error(&mut errors, ERR_NO_TWO_DIGIT_HOUR, "A two digit hour could not be found", string, begin);
                } else {
                    t.have_time = 1;
                }
            }
            Fmt::Meridian => {
                if t.h == UNSET {
                    add_pbf_error(&mut errors, ERR_MERIDIAN_BEFORE_HOUR, "Meridian can only come after an hour has been found", string, begin);
                }
                let tmp = timelib_meridian_with_check(&mut ptr, t.h);
                if tmp == UNSET {
                    add_pbf_error(&mut errors, ERR_NO_MERIDIAN, "A meridian could not be found", string, begin);
                } else {
                    t.have_time = 1;
                    if t.h != UNSET {
                        t.h += tmp;
                    }
                }
            }
            Fmt::Minute => {
                check_number!();
                let (min, length) = get_nr_ex(&mut ptr, 2);
                if min == UNSET || length != 2 {
                    add_pbf_error(&mut errors, ERR_NO_TWO_DIGIT_MINUTE, "A two digit minute could not be found", string, begin);
                } else {
                    t.have_time = 1;
                    t.i = min;
                }
            }
            Fmt::Second => {
                check_number!();
                let (sec, length) = get_nr_ex(&mut ptr, 2);
                if sec == UNSET || length != 2 {
                    add_pbf_error(&mut errors, ERR_NO_TWO_DIGIT_SECOND, "A two digit second could not be found", string, begin);
                } else {
                    t.have_time = 1;
                    t.s = sec;
                }
            }
            Fmt::Microsecond => {
                check_number!();
                let tptr = ptr.p;
                let f = get_nr(&mut ptr, 6);
                if f == UNSET || ptr.p - tptr < 1 {
                    add_pbf_error(&mut errors, ERR_NO_SIX_DIGIT_MICROSECOND, "A six digit microsecond could not be found", string, begin);
                } else {
                    t.us = (f as f64 * 10f64.powi(6 - (ptr.p - tptr) as i32)) as i64;
                }
            }
            Fmt::Millisecond => {
                check_number!();
                let tptr = ptr.p;
                let f = get_nr(&mut ptr, 3);
                if f == UNSET || ptr.p - tptr < 1 {
                    add_pbf_error(&mut errors, ERR_NO_THREE_DIGIT_MILLISECOND, "A three digit millisecond could not be found", string, begin);
                } else {
                    t.us = (f as f64 * 10f64.powi(3 - (ptr.p - tptr) as i32) * 1000.0) as i64;
                }
            }
            Fmt::Whitespace => eat_spaces(&mut ptr),
            Fmt::EpochSeconds => {
                check_signed_number!();
                let tmp = get_signed_nr(&mut errs, &mut ptr, 24);
                for (c, m) in errs.drain(..) {
                    add_pbf_error(&mut errors, c, m, string, begin);
                }
                t.have_zone = 1;
                t.sse = tmp;
                t.is_localtime = 1;
                t.zone_type = ZONETYPE_OFFSET;
                t.z = 0;
                t.dst = 0;
                update_from_sse(&mut t);
            }
            Fmt::AnySeparator => {
                if lookup_format(ptr.c()) != Fmt::Separator {
                    add_pbf_error(&mut errors, ERR_NO_SEP_SYMBOL, "The separation symbol ([;:/.,-]) could not be found", string, begin);
                } else {
                    ptr.adv();
                }
            }
            Fmt::Separator => {
                if ptr.c() != fc(fi) {
                    add_pbf_error(&mut errors, ERR_NO_SEP_SYMBOL, "The separation symbol could not be found", string, begin);
                } else {
                    ptr.adv();
                }
            }
            Fmt::ResetAll => time_reset_fields(&mut t),
            Fmt::ResetAllWhenNotSet => time_reset_unset_fields(&mut t),
            Fmt::RandomChar => ptr.adv(),
            Fmt::Escape => {
                if fc(fi + 1) == 0 {
                    add_pbf_error(&mut errors, ERR_EXPECTED_ESCAPE_CHAR, "Escaped character expected", string, begin);
                } else {
                    fi += 1;
                    if ptr.c() != fc(fi) {
                        add_pbf_error(&mut errors, ERR_NO_ESCAPED_CHAR, "The escaped character could not be found", string, begin);
                    } else {
                        ptr.adv();
                    }
                }
            }
            Fmt::SkipToSeparator => eat_until_separator(&mut ptr),
            Fmt::AllowExtra => allow_extra = true,
            Fmt::TimezoneOffset => {
                let mut tz_not_found = false;
                let mut dst = t.dst;
                let z = parse_zone(&mut ptr, &mut dst, &mut t, &mut tz_not_found);
                t.z = z;
                if t.zone_type != ZONETYPE_OFFSET {
                    t.dst = dst;
                }
                if tz_not_found {
                    add_pbf_error(&mut errors, ERR_TZID_NOT_FOUND, "The timezone could not be found in the database", string, begin);
                } else {
                    t.have_zone = 1;
                }
            }
            Fmt::Literal => {
                if fc(fi) != ptr.c() {
                    add_pbf_error(&mut errors, ERR_WRONG_FORMAT_SEP, "The format separator does not match", string, begin);
                }
                ptr.adv();
            }
        }
        fi += 1;
    }
    if ptr.c() != 0 {
        if allow_extra {
            add_pbf_warning(&mut errors, WARN_TRAILING_DATA, "Trailing data", string, ptr.p);
        } else {
            add_pbf_error(&mut errors, ERR_TRAILING_DATA, "Trailing data", string, ptr.p);
        }
    }
    if fc(fi) != 0 {
        let mut done = false;
        while fc(fi) != 0 && !done {
            match lookup_format(fc(fi)) {
                Fmt::ResetAll => time_reset_fields(&mut t),
                Fmt::ResetAllWhenNotSet => time_reset_unset_fields(&mut t),
                Fmt::AllowExtra => {}
                _ => {
                    add_pbf_error(&mut errors, ERR_DATA_MISSING, "Not enough data available to satisfy format", string, ptr.p);
                    done = true;
                }
            }
            fi += 1;
        }
    }
    if t.h != UNSET || t.i != UNSET || t.s != UNSET || t.us != UNSET {
        if t.h == UNSET {
            t.h = 0;
        }
        if t.i == UNSET {
            t.i = 0;
        }
        if t.s == UNSET {
            t.s = 0;
        }
        if t.us == UNSET {
            t.us = 0;
        }
    }
    if t.y != UNSET && (iso_week_of_year != UNSET || iso_year != UNSET || iso_day_of_week != UNSET) {
        add_pbf_error(&mut errors, ERR_MIX_ISO_WITH_NATURAL, "Mixing of ISO dates with natural dates is not allowed", string, ptr.p);
    }
    if iso_year != UNSET && (t.y != UNSET || t.m != UNSET || t.d != UNSET) {
        add_pbf_error(&mut errors, ERR_MIX_ISO_WITH_NATURAL, "Mixing of ISO dates with natural dates is not allowed", string, ptr.p);
    }
    if iso_year != UNSET {
        let (y, m, d) = date_from_isodate(iso_year, if iso_week_of_year == UNSET { 1 } else { iso_week_of_year }, if iso_day_of_week == UNSET { 1 } else { iso_day_of_week });
        t.y = y;
        t.m = m;
        t.d = d;
    } else if iso_week_of_year != UNSET || iso_day_of_week != UNSET {
        add_pbf_warning(&mut errors, WARN_INVALID_DATE, "The parsed date was invalid", string, ptr.p);
    }
    if t.h != UNSET && t.i != UNSET && t.s != UNSET && !valid_time(t.h, t.i, t.s) {
        add_pbf_warning(&mut errors, WARN_INVALID_TIME, "The parsed time was invalid", string, ptr.p);
    }
    if t.y != UNSET && t.m != UNSET && t.d != UNSET && !valid_date(t.y, t.m, t.d) {
        add_pbf_warning(&mut errors, WARN_INVALID_DATE, "The parsed date was invalid", string, ptr.p);
    }
    (t, errors)
}

/// timelib_fill_holes
pub fn fill_holes(parsed: &mut Time, now: &Time, options: u32) {
    if options & OVERRIDE_TIME == 0 && parsed.have_date != 0 && parsed.have_time == 0 {
        parsed.h = 0;
        parsed.i = 0;
        parsed.s = 0;
        parsed.us = 0;
    }
    if parsed.y != UNSET || parsed.m != UNSET || parsed.d != UNSET || parsed.h != UNSET || parsed.i != UNSET || parsed.s != UNSET {
        if parsed.us == UNSET {
            parsed.us = 0;
        }
    } else if parsed.us == UNSET {
        parsed.us = if now.us != UNSET { now.us } else { 0 };
    }
    if parsed.y == UNSET {
        parsed.y = if now.y != UNSET { now.y } else { 0 };
    }
    if parsed.m == UNSET {
        parsed.m = if now.m != UNSET { now.m } else { 0 };
    }
    if parsed.d == UNSET {
        parsed.d = if now.d != UNSET { now.d } else { 0 };
    }
    if parsed.h == UNSET {
        parsed.h = if now.h != UNSET { now.h } else { 0 };
    }
    if parsed.i == UNSET {
        parsed.i = if now.i != UNSET { now.i } else { 0 };
    }
    if parsed.s == UNSET {
        parsed.s = if now.s != UNSET { now.s } else { 0 };
    }
    if parsed.tz_info.is_none() {
        parsed.tz_info = now.tz_info.clone();
        if parsed.z == UNSET {
            parsed.z = if now.z != UNSET { now.z } else { 0 };
        }
        if parsed.dst == UNSET as i32 {
            parsed.dst = if now.dst != UNSET as i32 { now.dst } else { 0 };
        }
        if parsed.tz_abbr.is_none() {
            parsed.tz_abbr = now.tz_abbr.clone();
        }
    }
    if parsed.zone_type == 0 && now.zone_type != 0 {
        parsed.zone_type = now.zone_type;
        parsed.is_localtime = 1;
    }
}

/// timelib_timezone_id_from_abbr
pub fn timezone_id_from_abbr(abbr: &[u8], gmtoffset: i64, isdst: i32) -> Option<&'static str> {
    abbr_search(abbr, gmtoffset, isdst).and_then(|t| t.3)
}
