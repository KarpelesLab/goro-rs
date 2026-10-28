//! The date/time string grammar, after the PHP manual's "Supported Date
//! and Time Formats" (its symbol table and the time, date, compound and
//! relative format tables), adjusted where PHP 8.5 was observed to read
//! strings differently.

use super::pat::*;

fn is_alpha(c: u8) -> bool {
    c.is_ascii_alphabetic()
}
fn is_upper(c: u8) -> bool {
    c.is_ascii_uppercase()
}
fn is_lower(c: u8) -> bool {
    c.is_ascii_lowercase()
}

// ---- symbols ----

/// space: [ \t]+
pub(crate) fn space() -> impl Pat {
    rep(|c| c == b' ' || c == b'\t', 1, usize::MAX)
}

fn space_opt() -> impl Pat {
    rep(|c| c == b' ' || c == b'\t', 0, usize::MAX)
}

/// frac: "." [0-9]+
pub(crate) fn frac() -> impl Pat {
    seq!(set(b"."), digits(1, usize::MAX))
}

/// hour24: [01]?[0-9] | "2"[0-4]
pub(crate) fn hour24() -> impl Pat {
    alt!(
        seq!(opt(set(b"01")), digits(1, 1)),
        seq!(set(b"2"), set(b"01234"))
    )
}

/// hour24lz: [01][0-9] | "2"[0-4]
pub(crate) fn hour24lz() -> impl Pat {
    alt!(
        seq!(set(b"01"), digits(1, 1)),
        seq!(set(b"2"), set(b"01234"))
    )
}

/// hour12: "0"?[1-9] | "1"[0-2]
pub(crate) fn hour12() -> impl Pat {
    alt!(
        seq!(opt(set(b"0")), set(b"123456789")),
        seq!(set(b"1"), set(b"012"))
    )
}

/// minute: [0-5]?[0-9]
pub(crate) fn minute() -> impl Pat {
    seq!(opt(set(b"012345")), digits(1, 1))
}

/// minutelz: [0-5][0-9]
pub(crate) fn minutelz() -> impl Pat {
    seq!(set(b"012345"), digits(1, 1))
}

/// second: minute | "60"
pub(crate) fn second() -> impl Pat {
    alt!(minute(), lit("60"))
}

/// secondlz: minutelz | "60"
pub(crate) fn secondlz() -> impl Pat {
    alt!(minutelz(), lit("60"))
}

/// meridian: ([AaPp] "."? [Mm] "."?) [\0\t ]
pub(crate) fn meridian() -> impl Pat {
    seq!(
        set(b"AaPp"),
        opt(set(b".")),
        set(b"Mm"),
        opt(set(b".")),
        alt!(set(b"\t "), peek(|_| false, true))
    )
}

/// tz: "("? [A-Za-z]{1,6} ")"? | [A-Z][a-z]+([_/-][A-Za-z]+)+
pub(crate) fn tz() -> impl Pat {
    alt!(
        seq!(opt(set(b"(")), rep(is_alpha, 1, 6), opt(set(b")"))),
        seq!(byte(is_upper), rep(is_lower, 1, usize::MAX), plus_ident())
    )
}

fn plus_ident() -> impl Pat {
    let part = || seq!(set(b"_/-"), rep(is_alpha, 1, usize::MAX));
    seq2(part(), star(part()))
}

/// tzcorrection: "GMT"? [+-] hour24 ":"? minute? (seconds observed on
/// PHP 8.5: "+05:30:15", "+123456")
pub(crate) fn tzcorrection() -> impl Pat {
    // "+9959" is read as "+99" and "+12345" as "+1234": seconds need
    // either colons or exactly six digits (observed on PHP 8.5).
    seq!(
        opt(lit_cs("GMT")),
        set(b"+-"),
        alt!(
            seq!(hour24(), opt(seq!(opt(set(b":")), minute()))),
            seq!(hour24lz(), minutelz(), secondlz()),
            seq!(hour24(), set(b":"), minutelz(), set(b":"), secondlz())
        )
    )
}

fn daysuf() -> impl Pat {
    words(&["st", "nd", "rd", "th"])
}

/// month: "0"?[0-9] | "1"[0-2]
pub(crate) fn month() -> impl Pat {
    alt!(
        seq!(opt(set(b"0")), digits(1, 1)),
        seq!(set(b"1"), set(b"012"))
    )
}

/// day: (([0-2]?[0-9]) | ("3"[01])) daysuf?
pub(crate) fn day() -> impl Pat {
    seq!(
        alt!(
            seq!(opt(set(b"012")), digits(1, 1)),
            seq!(set(b"3"), set(b"01"))
        ),
        opt(daysuf())
    )
}

/// year: [0-9]{1,4}
pub(crate) fn year() -> impl Pat {
    digits(1, 4)
}
pub(crate) fn year2() -> impl Pat {
    digits(2, 2)
}
pub(crate) fn year4() -> impl Pat {
    digits(4, 4)
}
fn year4withsign() -> impl Pat {
    seq!(opt(set(b"+-")), digits(4, 4))
}
fn yearx() -> impl Pat {
    seq!(set(b"+-"), digits(5, 19))
}

/// dayofyear: 001-366
fn dayofyear() -> impl Pat {
    alt!(
        seq!(lit("00"), set(b"123456789")),
        seq!(set(b"0"), set(b"123456789"), digits(1, 1)),
        seq!(set(b"12"), digits(2, 2)),
        seq!(set(b"3"), set(b"012345"), digits(1, 1)),
        seq!(lit("36"), set(b"0123456"))
    )
}

/// weekofyear: 01-53
fn weekofyear() -> impl Pat {
    alt!(
        seq!(set(b"0"), set(b"123456789")),
        seq!(set(b"1234"), digits(1, 1)),
        seq!(set(b"5"), set(b"0123"))
    )
}

fn monthlz() -> impl Pat {
    alt!(seq!(set(b"0"), digits(1, 1)), seq!(set(b"1"), set(b"012")))
}

fn daylz() -> impl Pat {
    alt!(
        seq!(set(b"0"), digits(1, 1)),
        seq!(set(b"12"), digits(1, 1)),
        seq!(set(b"3"), set(b"01"))
    )
}

pub(crate) const DAY_FULL: &[&str] = &[
    "sunday",
    "monday",
    "tuesday",
    "wednesday",
    "thursday",
    "friday",
    "saturday",
];
pub(crate) const DAY_ABBR: &[&str] = &["sun", "mon", "tue", "wed", "thu", "fri", "sat"];

fn dayfull() -> impl Pat {
    words(DAY_FULL)
}
fn dayabbr() -> impl Pat {
    words(DAY_ABBR)
}
fn dayspecial() -> impl Pat {
    words(&["weekday", "weekdays"])
}
fn daytext() -> impl Pat {
    alt!(dayfull(), dayabbr(), dayspecial())
}

pub(crate) const MONTH_FULL: &[&str] = &[
    "january",
    "february",
    "march",
    "april",
    "may",
    "june",
    "july",
    "august",
    "september",
    "october",
    "november",
    "december",
];
pub(crate) const MONTH_ABBR: &[&str] = &[
    "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "sept", "oct", "nov", "dec",
];
pub(crate) const MONTH_ROMAN: &[&str] = &[
    "i", "ii", "iii", "iv", "v", "vi", "vii", "viii", "ix", "x", "xi", "xii",
];

fn monthfull() -> impl Pat {
    words(MONTH_FULL)
}
fn monthabbr() -> impl Pat {
    words(MONTH_ABBR)
}
fn monthroman() -> impl Pat {
    // Roman numerals are upper case in the manual's grammar.
    move |s: &[u8], p: usize, out: &mut Ends| {
        for w in MONTH_ROMAN {
            let w = w.as_bytes();
            if s.len() >= p + w.len()
                && s[p..p + w.len()]
                    .iter()
                    .zip(w)
                    .all(|(a, b)| *a == b.to_ascii_uppercase())
            {
                out.push(p + w.len());
            }
        }
    }
}
pub(crate) fn monthtext() -> impl Pat {
    alt!(monthfull(), monthabbr(), monthroman())
}

/// reltextnumber
pub(crate) const RELTEXT_NUMBER: &[&str] = &[
    "first", "second", "third", "fourth", "fifth", "sixth", "seventh", "eight", "eighth", "ninth",
    "tenth", "eleventh", "twelfth",
];
/// reltexttext
pub(crate) const RELTEXT_TEXT: &[&str] = &["next", "last", "previous", "this"];

fn reltextnumber() -> impl Pat {
    words(RELTEXT_NUMBER)
}
fn reltexttext() -> impl Pat {
    words(RELTEXT_TEXT)
}

/// reltextunit: 'ms' | 'µs' | (('msec'|'millisecond'|'µsec'|'microsecond'|
/// 'usec'|'sec'|'second'|'min'|'minute'|'hour'|'day'|'fortnight'|
/// 'forthnight'|'month'|'year') 's'?) | 'weeks' | daytext
pub(crate) fn reltextunit() -> impl Pat {
    alt!(
        words(&["ms", "\u{b5}s"]),
        seq!(
            words(&[
                "msec",
                "millisecond",
                "\u{b5}sec",
                "microsecond",
                "usec",
                "sec",
                "second",
                "min",
                "minute",
                "hour",
                "day",
                "fortnight",
                "forthnight",
                "month",
                "year",
            ]),
            opt(set(b"sS"))
        ),
        lit("weeks"),
        // "4 sundays" is accepted (observed on PHP 8.5)
        seq!(daytext(), opt(set(b"sS")))
    )
}

/// relnumber: ([+-]*[ \t]*[0-9]{1,13})
pub(crate) fn relnumber() -> impl Pat {
    seq!(
        rep(|c| c == b'+' || c == b'-', 0, usize::MAX),
        space_opt(),
        digits(1, 13)
    )
}

// ---- formats ----

/// The scanner's rules, in priority order (earlier wins a tie in length).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Rule {
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
    TimeTiny12,
    TimeShort12,
    TimeLong12,
    MsSqlTime,
    TimeTiny24,
    TimeShort24,
    TimeLong24,
    Iso8601Long,
    GnuNoColon,
    Iso8601NoColon,
    AmericanShort,
    American,
    Iso8601DateSlash,
    DateSlash,
    Iso8601Date4,
    Iso8601Date2,
    Iso8601DateX,
    GnuDateShorter,
    GnuDateShort,
    PointedDate4,
    PointedDate2,
    DateFull,
    DateNoDay,
    DateNoDayRev,
    DateTextual,
    DateNoYear,
    DateNoYearRev,
    DateNoColon,
    XmlRpc,
    XmlRpcNoColon,
    Soap,
    Wddx,
    Exif,
    PgYearDotDay,
    PgTextShort,
    PgTextReverse,
    Clf,
    Year4,
    IsoWeekDay,
    IsoWeek,
    DateShortWithTimeShort12,
    DateShortWithTimeLong12,
    DateShortWithTimeShort,
    DateShortWithTimeLong,
    RelativeTextWeek,
    RelativeText,
    MonthFull,
    MonthAbbr,
    DayFull,
    DayAbbr,
    WeekdaySpecial,
    Ago,
    Relative,
    TzCorrection,
    Tz,
}

pub(crate) const RULES: &[Rule] = &[
    Rule::Yesterday,
    Rule::Now,
    Rule::Noon,
    Rule::MidnightToday,
    Rule::Tomorrow,
    Rule::Timestamp,
    Rule::TimestampMs,
    Rule::FirstLastDayOf,
    Rule::BackFrontOf,
    Rule::WeekdayOf,
    Rule::TimeTiny12,
    Rule::TimeShort12,
    Rule::TimeLong12,
    Rule::MsSqlTime,
    Rule::TimeTiny24,
    Rule::TimeShort24,
    Rule::TimeLong24,
    Rule::Iso8601Long,
    Rule::GnuNoColon,
    Rule::Iso8601NoColon,
    Rule::AmericanShort,
    Rule::American,
    Rule::Iso8601DateSlash,
    Rule::DateSlash,
    Rule::Iso8601Date4,
    Rule::Iso8601Date2,
    Rule::Iso8601DateX,
    Rule::GnuDateShorter,
    Rule::GnuDateShort,
    Rule::PointedDate4,
    Rule::PointedDate2,
    Rule::DateFull,
    Rule::DateNoDay,
    Rule::DateNoDayRev,
    Rule::DateTextual,
    Rule::DateNoYear,
    Rule::DateNoYearRev,
    Rule::DateNoColon,
    Rule::XmlRpc,
    Rule::XmlRpcNoColon,
    Rule::Soap,
    Rule::Wddx,
    Rule::Exif,
    Rule::PgYearDotDay,
    Rule::PgTextShort,
    Rule::PgTextReverse,
    Rule::Clf,
    Rule::Year4,
    Rule::IsoWeekDay,
    Rule::IsoWeek,
    Rule::DateShortWithTimeShort12,
    Rule::DateShortWithTimeLong12,
    Rule::DateShortWithTimeShort,
    Rule::DateShortWithTimeLong,
    Rule::RelativeTextWeek,
    Rule::RelativeText,
    Rule::MonthFull,
    Rule::MonthAbbr,
    Rule::DayFull,
    Rule::DayAbbr,
    Rule::WeekdaySpecial,
    Rule::Ago,
    Rule::Relative,
    Rule::TzCorrection,
    Rule::Tz,
];

fn t_opt() -> impl Pat {
    opt(set(b"tT"))
}

fn timetiny12() -> impl Pat {
    seq!(hour12(), space_opt(), meridian())
}
fn timeshort12() -> impl Pat {
    seq!(hour12(), set(b":."), minute(), space_opt(), meridian())
}
fn timelong12() -> impl Pat {
    seq!(
        hour12(),
        set(b":."),
        minute(),
        set(b":."),
        second(),
        space_opt(),
        meridian()
    )
}
fn timeshort24() -> impl Pat {
    seq!(t_opt(), hour24(), set(b":."), minute())
}
fn timelong24() -> impl Pat {
    seq!(
        t_opt(),
        hour24(),
        set(b":."),
        minute(),
        set(b":."),
        second()
    )
}
fn iso8601long() -> impl Pat {
    seq!(
        t_opt(),
        hour24(),
        set(b":."),
        minute(),
        set(b":."),
        second(),
        frac()
    )
}
/// datenoyear: the day must be followed by one of [,.stndrh\t ] or the
/// end: "Jan 10:00" is January plus 10:00 (observed on PHP 8.5).
fn datenoyear() -> impl Pat {
    seq!(
        monthtext(),
        star(set(b" .\t-")),
        day_digits(),
        alt!(
            seq!(set(b",.stndrh\t "), star(set(b",.stndrh\t "))),
            peek(|_| false, true)
        )
    )
}

fn plus_set(chars: &'static [u8]) -> impl Pat {
    seq!(set(chars), star(set(chars)))
}

fn day_digits() -> impl Pat {
    alt!(
        seq!(opt(set(b"012")), digits(1, 1)),
        seq!(set(b"3"), set(b"01"))
    )
}

/// Matches `rule` at `p`, returning the longest match length.
pub(crate) fn match_rule(rule: Rule, s: &[u8], p: usize) -> Option<usize> {
    use Rule::*;
    match rule {
        Yesterday => lit("yesterday").longest(s, p),
        Now => lit("now").longest(s, p),
        Noon => lit("noon").longest(s, p),
        MidnightToday => words(&["midnight", "today"]).longest(s, p),
        Tomorrow => lit("tomorrow").longest(s, p),
        Timestamp => seq!(set(b"@"), opt(set(b"-")), digits(1, usize::MAX)).longest(s, p),
        TimestampMs => seq!(
            set(b"@"),
            opt(set(b"-")),
            digits(1, usize::MAX),
            set(b"."),
            digits(0, 6)
        )
        .longest(s, p),
        FirstLastDayOf => seq!(
            words(&["first", "last"]),
            space(),
            lit("day"),
            space(),
            lit("of")
        )
        .longest(s, p),
        BackFrontOf => seq!(
            words(&["back", "front"]),
            space(),
            lit("of"),
            space(),
            hour24(),
            opt(seq!(space_opt(), meridian()))
        )
        .longest(s, p),
        WeekdayOf => seq!(
            alt!(reltextnumber(), reltexttext()),
            space(),
            alt!(dayfull(), dayabbr()),
            space(),
            lit("of")
        )
        .longest(s, p),
        TimeTiny12 => timetiny12().longest(s, p),
        TimeShort12 => timeshort12().longest(s, p),
        TimeLong12 => timelong12().longest(s, p),
        MsSqlTime => seq!(
            hour12(),
            set(b":"),
            minutelz(),
            set(b":"),
            secondlz(),
            set(b":."),
            digits(1, usize::MAX),
            meridian()
        )
        .longest(s, p),
        TimeTiny24 => seq!(set(b"tT"), hour24()).longest(s, p),
        TimeShort24 => timeshort24().longest(s, p),
        TimeLong24 => timelong24().longest(s, p),
        Iso8601Long => iso8601long().longest(s, p),
        GnuNoColon => seq!(t_opt(), hour24lz(), minutelz()).longest(s, p),
        Iso8601NoColon => seq!(t_opt(), hour24lz(), minutelz(), secondlz()).longest(s, p),
        AmericanShort => seq!(month(), set(b"/"), day()).longest(s, p),
        American => seq!(month(), set(b"/"), day(), set(b"/"), year()).longest(s, p),
        Iso8601DateSlash => seq!(
            year4(),
            set(b"/"),
            monthlz(),
            set(b"/"),
            daylz(),
            opt(set(b"/"))
        )
        .longest(s, p),
        DateSlash => seq!(year4(), set(b"/"), month(), set(b"/"), day()).longest(s, p),
        Iso8601Date4 => {
            seq!(year4withsign(), set(b"-"), monthlz(), set(b"-"), daylz()).longest(s, p)
        }
        Iso8601Date2 => seq!(year2(), set(b"-"), monthlz(), set(b"-"), daylz()).longest(s, p),
        Iso8601DateX => seq!(yearx(), set(b"-"), monthlz(), set(b"-"), daylz()).longest(s, p),
        GnuDateShorter => seq!(year4(), set(b"-"), month()).longest(s, p),
        GnuDateShort => seq!(year(), set(b"-"), month(), set(b"-"), day()).longest(s, p),
        PointedDate4 => seq!(day(), set(b".\t-"), month(), set(b".-"), year4()).longest(s, p),
        PointedDate2 => seq!(day(), set(b".\t"), month(), set(b"."), year2()).longest(s, p),
        DateFull => seq!(
            day(),
            star(set(b" \t.-")),
            monthtext(),
            star(set(b" \t.-")),
            year()
        )
        .longest(s, p),
        DateNoDay => seq!(monthtext(), star(set(b" .\t-")), year4()).longest(s, p),
        DateNoDayRev => seq!(year4(), star(set(b" .\t-")), monthtext()).longest(s, p),
        // PHP manual: m ([ .\t-])* dd [,.stndrh\t ]+ y
        DateTextual => seq!(
            monthtext(),
            star(set(b" .\t-")),
            day_digits(),
            plus_set(b",.stndrh\t "),
            year()
        )
        .longest(s, p),
        DateNoYear => datenoyear().longest(s, p),
        DateNoYearRev => seq!(day(), star(set(b" .\t-")), monthtext()).longest(s, p),
        DateNoColon => seq!(year4(), monthlz(), daylz()).longest(s, p),
        XmlRpc => seq!(
            year4(),
            monthlz(),
            daylz(),
            set(b"tT"),
            hour24(),
            set(b":"),
            minutelz(),
            set(b":"),
            secondlz()
        )
        .longest(s, p),
        XmlRpcNoColon => seq!(
            year4(),
            monthlz(),
            daylz(),
            set(b"t"),
            hour24(),
            minutelz(),
            secondlz()
        )
        .longest(s, p),
        Soap => seq!(
            year4(),
            set(b"-"),
            monthlz(),
            set(b"-"),
            daylz(),
            set(b"T"),
            hour24lz(),
            set(b":"),
            minutelz(),
            set(b":"),
            secondlz(),
            frac(),
            opt(tzcorrection())
        )
        .longest(s, p),
        Wddx => seq!(
            year4(),
            set(b"-"),
            month(),
            set(b"-"),
            day(),
            set(b"T"),
            hour24(),
            set(b":"),
            minute(),
            set(b":"),
            second()
        )
        .longest(s, p),
        Exif => seq!(
            year4(),
            set(b":"),
            monthlz(),
            set(b":"),
            daylz(),
            set(b" "),
            hour24lz(),
            set(b":"),
            minutelz(),
            set(b":"),
            secondlz()
        )
        .longest(s, p),
        PgYearDotDay => seq!(year4(), opt(set(b".-")), dayofyear()).longest(s, p),
        PgTextShort => seq!(monthabbr(), set(b"-"), daylz(), set(b"-"), year()).longest(s, p),
        PgTextReverse => seq!(year(), set(b"-"), monthabbr(), set(b"-"), daylz()).longest(s, p),
        Clf => seq!(
            day(),
            set(b"/"),
            monthabbr(),
            set(b"/"),
            year4(),
            set(b":"),
            hour24lz(),
            set(b":"),
            minutelz(),
            set(b":"),
            secondlz(),
            space(),
            tzcorrection()
        )
        .longest(s, p),
        Year4 => year4().longest(s, p),
        IsoWeekDay => seq!(
            year4(),
            opt(set(b"-")),
            set(b"W"),
            weekofyear(),
            opt(set(b"-")),
            set(b"01234567")
        )
        .longest(s, p),
        IsoWeek => seq!(year4(), opt(set(b"-")), set(b"W"), weekofyear()).longest(s, p),
        DateShortWithTimeShort12 => seq!(datenoyear(), timeshort12()).longest(s, p),
        DateShortWithTimeLong12 => seq!(datenoyear(), timelong12()).longest(s, p),
        DateShortWithTimeShort => seq!(datenoyear(), timeshort24()).longest(s, p),
        // with an optional zone: "Jan 22 10:30:45 UTC"
        DateShortWithTimeLong => seq!(
            datenoyear(),
            timelong24(),
            opt(seq!(space_opt(), alt!(tzcorrection(), tz())))
        )
        .longest(s, p),
        RelativeTextWeek => seq!(reltexttext(), space(), lit("week")).longest(s, p),
        RelativeText => {
            seq!(alt!(reltextnumber(), reltexttext()), space(), reltextunit()).longest(s, p)
        }
        MonthFull => monthfull().longest(s, p),
        // roman numerals are months only inside dates ("I" alone is a
        // military zone, observed on PHP 8.5)
        MonthAbbr => monthabbr().longest(s, p),
        DayFull => dayfull().longest(s, p),
        DayAbbr => dayabbr().longest(s, p),
        WeekdaySpecial => dayspecial().longest(s, p),
        Ago => lit("ago").longest(s, p),
        Relative => seq!(relnumber(), space_opt(), alt!(reltextunit(), lit("week"))).longest(s, p),
        TzCorrection => tzcorrection().longest(s, p),
        Tz => tz().longest(s, p),
    }
}
