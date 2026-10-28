//! createFromFormat() / date_parse_from_format() against PHP 8.5.
//!
//! `fixtures/from_format_php85.tsv` is the output of
//! tools/date_corpus/from_format_driver.php and from_format_driver3.php,
//! each run once on PHP 8.5, reduced by from_format_trim.py (see the
//! driver's header for the line format; `GORO_CFF_FIXTURE` points the
//! test at another run, `GORO_CFF_VERBOSE` prints the differences). The
//! unit tests below port the inputs and expected results of the ext/date
//! tests that use these functions.

use goro_ext_date::TimeZone;
use goro_ext_date::from_format::{self, LastErrors, php_array};
use goro_ext_date::parse::DateParse;

/// Message codes of the fixture (the driver's `$MSG` table).
const MSG: &[&str] = &[
    "Trailing data",
    "Not enough data available to satisfy format",
    "Unexpected data found.",
    "The timezone could not be found in the database",
    "The format separator does not match",
    "The parsed time was invalid",
    "The parsed date was invalid",
    "A two digit second could not be found",
    "Hour cannot be higher than 12",
    "A textual day could not be found",
    "A two digit hour could not be found",
    "A four digit year could not be found",
    "A textual month could not be found",
    "A meridian could not be found",
    "A two digit minute could not be found",
    "The separation symbol could not be found",
    "A two digit day could not be found",
    "Found unexpected data",
    "Number out of range",
    "A two digit month could not be found",
    "A six digit microsecond could not be found",
    "A three digit millisecond could not be found",
    "A two digit year could not be found",
    "A three digit day-of-year could not be found",
    "The separation symbol ([;:/.,-]) could not be found",
    "A 'day of year' can only come after a year has been found",
    "The escaped character could not be found",
];

fn msg(m: &str) -> String {
    match MSG.iter().position(|x| *x == m) {
        Some(i) => format!("~{i}"),
        None => m.to_string(),
    }
}

fn list(l: &[(usize, String)]) -> String {
    php_array(l)
        .iter()
        .map(|(k, v)| format!("{k}={}", msg(v)))
        .collect::<Vec<_>>()
        .join(";")
}

fn we(w: &[(usize, String)], e: &[(usize, String)]) -> String {
    format!("{}:{}#{}:{}", w.len(), list(w), e.len(), list(e))
}

fn dp_canon(p: &DateParse) -> String {
    let o = |v: Option<i64>| v.map(|v| v.to_string()).unwrap_or_else(|| "F".into());
    let mut f = vec![
        o(p.year),
        o(p.month),
        o(p.day),
        o(p.hour),
        o(p.minute),
        o(p.second),
        o(p.fraction_us),
        we(&p.warnings, &p.errors),
    ];
    let d = |v: Option<String>| v.unwrap_or_else(|| "-".into());
    f.push(if p.is_localtime {
        format!(
            "{},{},{},{},{}",
            p.zone_type.unwrap_or(0),
            d(p.zone.map(|z| z.to_string())),
            d(p.is_dst.map(|b| (b as u8).to_string())),
            d(p.tz_abbr.clone()),
            d(p.tz_id.clone())
        )
    } else {
        String::new()
    });
    f.push(match &p.relative {
        None => String::new(),
        Some(r) => {
            let mut parts: Vec<String> = r.ymdhis.iter().map(|v| v.to_string()).collect();
            if let Some(w) = r.weekday {
                parts.push(format!("wd={w}"));
            }
            parts.join(",")
        }
    });
    f.join("|")
}

fn le_canon(le: &LastErrors) -> String {
    if le.is_empty() {
        "false".into()
    } else {
        we(&le.warnings, &le.errors)
    }
}

fn dec(s: &str) -> Vec<u8> {
    match s.strip_prefix('%') {
        Some(h) => (0..h.len() / 2)
            .map(|i| u8::from_str_radix(&h[2 * i..2 * i + 2], 16).unwrap())
            .collect(),
        None => s.as_bytes().to_vec(),
    }
}

fn fixture() -> String {
    let path = std::env::var("GORO_CFF_FIXTURE").unwrap_or_else(|_| {
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/from_format_php85.tsv"
        )
        .to_string()
    });
    std::fs::read_to_string(path).expect("fixture")
}

/// The reference build reads the system zone database, which lacks the
/// backward-compatibility names ("US/Eastern", "GB", "Zulu", "EST5EDT");
/// our embedded database has them.
fn legacy_name(p: &from_format::FormatParsed, php_dp: &str) -> bool {
    php_dp.contains("=~3")
        && matches!(&p.zone, Some(z) if matches!(&z.id,
            Some(TimeZone::Id(id)) if id.zone.is_link() || !id.name().contains('/')))
}

#[test]
fn php85_fixture() {
    let data = fixture();
    let verbose = std::env::var("GORO_CFF_VERBOSE").is_ok();
    let (mut total, mut bad) = (0, 0);
    for line in data.lines() {
        let c: Vec<&str> = line.split('\t').collect();
        if c.len() < 5 || line.starts_with('#') {
            continue;
        }
        total += 1;
        let zone = TimeZone::parse(c[0]).expect("zone");
        let (fmt, input) = (dec(c[1]), dec(c[2]));
        let parsed = from_format::parse_from_format(&fmt, &input);
        let dp = dp_canon(&parsed.date_parse());
        if dp != c[3] && legacy_name(&parsed, c[3]) {
            continue;
        }
        let (now_s, now_us0, now_us1): (i64, u32, u32) = if c.len() >= 8 {
            (
                c[5].parse().unwrap(),
                c[6].parse().unwrap(),
                c[7].parse().unwrap(),
            )
        } else {
            (1_700_000_000, 0, 0)
        };
        let dpwe = we(&parsed.warnings, &parsed.errors);
        let res = from_format::create_from_format(&fmt, &input, &zone, now_s, now_us0);
        let cr_ok = match (&res, c[4].split('|').collect::<Vec<_>>().as_slice()) {
            (Err(le), ["F", les]) => {
                let l = le_canon(le);
                (*les == "=" && (l == dpwe || (l == "false" && dpwe == "0:#0:"))) || l == *les
            }
            (Ok((dt, le)), ["O", date, zname, ztype, u, les]) => {
                let l = le_canon(le);
                let le_ok =
                    (*les == "=" && (l == dpwe || (l == "false" && dpwe == "0:#0:"))) || l == *les;
                let got = String::from_utf8(dt.format(b"Y-m-d H:i:s.u")).unwrap();
                let date_ok = got == *date
                    || (got[..20] == date[..20] && {
                        // microseconds of "now": anything the call could
                        // have seen
                        let us: u32 = date[20..].parse().unwrap_or(u32::MAX);
                        let gus: u32 = got[20..].parse().unwrap_or(0);
                        gus == now_us0 && us >= now_us0 && us <= now_us1.max(now_us0) + 50
                    });
                le_ok
                    && date_ok
                    && dt.zone.name() == *zname
                    && dt.zone.kind().to_string() == *ztype
                    && dt.ts.to_string() == *u
            }
            _ => false,
        };
        if dp != c[3] || !cr_ok {
            bad += 1;
            if verbose {
                println!(
                    "{} {:?} {:?}\n  php dp: {}\n  got dp: {}\n  php cr: {}\n  got cr: {}",
                    c[0],
                    String::from_utf8_lossy(&fmt),
                    String::from_utf8_lossy(&input),
                    c[3],
                    dp,
                    c[4],
                    match &res {
                        Ok((dt, le)) => format!(
                            "O|{}|{}|{}|{}|{}",
                            String::from_utf8_lossy(&dt.format(b"Y-m-d H:i:s.u")),
                            dt.zone.name(),
                            dt.zone.kind(),
                            dt.ts,
                            le_canon(le)
                        ),
                        Err(le) => format!("F|{}", le_canon(le)),
                    }
                );
            }
        }
    }
    println!("createFromFormat fixture: {bad} of {total} differ");
    assert!(total > 0);
    assert_eq!(bad, 0);
}

// ---- ports of ext/date/tests (inputs and expected output) ----

fn utc() -> TimeZone {
    TimeZone::from_id("UTC").unwrap()
}

fn create(fmt: &str, input: &str, zone: &TimeZone) -> Option<goro_ext_date::DateTime> {
    from_format::create_from_format(fmt.as_bytes(), input.as_bytes(), zone, 1_700_000_000, 0)
        .ok()
        .map(|r| r.0)
}

fn fmt(dt: &goro_ext_date::DateTime, f: &str) -> String {
    String::from_utf8(dt.format(f.as_bytes())).unwrap()
}

fn errors(fmt: &str, input: &str) -> Vec<(usize, String)> {
    from_format::parse_from_format(fmt.as_bytes(), input.as_bytes()).errors
}

#[test]
fn date_parse_by_format001() {
    for f in ["!m/d/y", "!m*d*y"] {
        let p = from_format::parse_from_format(f.as_bytes(), b"06/08/04").date_parse();
        assert_eq!(
            (
                p.year,
                p.month,
                p.day,
                p.hour,
                p.minute,
                p.second,
                p.fraction_us
            ),
            (
                Some(2004),
                Some(6),
                Some(8),
                Some(0),
                Some(0),
                Some(0),
                Some(0)
            )
        );
        assert!(p.warnings.is_empty() && p.errors.is_empty() && !p.is_localtime);
    }
}

#[test]
fn date_lenient() {
    let p = from_format::parse_from_format(b"m/d/y", b"06/08/04 12:00");
    assert_eq!(p.errors, vec![(8, "Trailing data".to_string())]);
    assert_eq!((p.h, p.us), (None, None));
    for f in ["m/d/y+", "+m/d/y", "m/d/y++"] {
        let p = from_format::parse_from_format(f.as_bytes(), b"06/08/04 12:00");
        assert_eq!(p.warnings, vec![(8, "Trailing data".to_string())]);
        assert!(p.errors.is_empty());
        assert_eq!((p.y, p.m, p.d), (Some(2004), Some(6), Some(8)));
    }
    for f in ["m/d/y+", "+m/d/y"] {
        let p = from_format::parse_from_format(f.as_bytes(), b"06/08/04");
        assert!(p.warnings.is_empty() && p.errors.is_empty());
    }
    // date-lenient-create.phpt
    let r = from_format::create_from_format(b"m/d/y+", b"06/08/04 12:00", &utc(), 0, 0).unwrap();
    assert_eq!(r.1.warnings_array(), vec![(8, "Trailing data")]);
}

#[test]
fn test_parse_from_format() {
    let oslo = TimeZone::from_id("Europe/Oslo").unwrap();
    let cases = [
        ("Y-m-d\\TH:i:sP", "2008-07-08T22:14:12+02:00"),
        ("l, d-M-Y H:i:s T", "Tuesday, 08-Jul-2008 22:14:12 GMT+0200"),
        ("Y-m-d\\TH:i:sO", "2008-07-08T22:14:12+0200"),
        ("X-m-d\\TH:i:sP", "+2008-07-08T22:14:12+02:00"),
        ("D, d M y H:i:s O", "Tue, 08 Jul 08 22:14:12 +0200"),
        ("l, d-M-y H:i:s T", "Tuesday, 08-Jul-08 22:14:12 GMT+0200"),
        ("D, d M Y H:i:s O", "Tue, 08 Jul 2008 22:14:12 +0200"),
        ("Y-m-d\\TH:i:s.vP", "2008-07-08T22:14:12.000+02:00"),
    ];
    for (f, s) in cases {
        let dt = create(f, s, &oslo).unwrap_or_else(|| panic!("{f} {s}"));
        assert_eq!(
            fmt(&dt, "Y-m-d H:i:s.u"),
            "2008-07-08 22:14:12.000000",
            "{f}"
        );
        assert_eq!((dt.zone.kind(), dt.zone.name()), (1, "+02:00".to_string()));
    }
}

#[test]
fn bugs_misc() {
    // bug53879.phpt
    let dt = create("l, d-M-Y H:i:s T", "Mon, 21-Jan-2041 15:24:52 GMT", &utc()).unwrap();
    assert_eq!(fmt(&dt, "Y-m-d H:i:s"), "2041-01-21 15:24:52");
    assert_eq!((dt.zone.kind(), dt.zone.name()), (2, "GMT".to_string()));
    // bug64157.phpt
    assert_eq!(
        errors("s", "0")[0].1,
        "A two digit second could not be found"
    );
    // bug51994.phpt
    let p = from_format::parse_from_format(b"yz", b"10153");
    assert_eq!((p.y, p.m, p.d, p.h), (Some(2010), Some(6), Some(3), None));
    // bug67251.phpt: two errors at position 0, the array shows one
    let p = from_format::parse_from_format(b"\\", b"AAAABBBB");
    assert_eq!(p.errors.len(), 2);
    assert_eq!(p.last_errors().errors_array(), vec![(0, "Trailing data")]);
    // bug54316.phpt
    let dt = create("Y-m-d|", "2011-02-02", &utc()).unwrap();
    assert_eq!(fmt(&dt, "Y-m-d H:i:s.u"), "2011-02-02 00:00:00.000000");
    let dt = create("Y-m-d!", "2011-02-02", &utc()).unwrap();
    assert_eq!(fmt(&dt, "Y-m-d H:i:s.u"), "1970-01-01 00:00:00.000000");
    // bug67253.phpt
    let p = from_format::parse_from_format(b"aHa0", b"0=G9UCNnF");
    assert_eq!(p.errors.len(), 3);
    assert_eq!(
        p.last_errors().errors_array(),
        vec![
            (0, "A meridian could not be found"),
            (9, "Not enough data available to satisfy format")
        ]
    );
    // bug79580.phpt, bug73426.phpt
    for v in ["31 2020", "60 2020", "91 2020", "121 2020", "130 2020"] {
        assert_eq!(
            errors("z Y", v)[0].1,
            "A 'day of year' can only come after a year has been found"
        );
    }
    let dt = create("Y H:i:s z", "2016 12:00:00 15", &utc()).unwrap();
    assert_eq!(fmt(&dt, "Y-m-d H:i:s"), "2016-01-16 12:00:00");
    assert!(create("z H:i:s Y", "16 12:00:00 2016", &utc()).is_none());
    // bug76770.phpt
    assert_eq!(create("U H", "3600 01", &utc()).unwrap().ts, 3600);
    // bug66836.phpt
    let berlin = TimeZone::from_id("Europe/Berlin").unwrap();
    for t in ["-1", "-86400", "-1000000"] {
        assert_eq!(fmt(&create("U", t, &berlin).unwrap(), "U"), t);
    }
    // bug81565.phpt
    let dt = create("Y-m-d H:i:sO", "0021-08-21 00:00:00+00:49:56", &utc()).unwrap();
    assert_eq!(fmt(&dt, "Y-m-d H:i:s"), "0021-08-21 00:00:00");
    assert_eq!(dt.zone.name(), "+00:49:56");
    // gh9700.phpt
    let dt = create(
        "Y-m-d\\TH:i:sP[e]",
        "2022-02-18T00:00:00+01:00[Europe/Berlin]",
        &utc(),
    )
    .unwrap();
    assert_eq!(
        (dt.zone.kind(), dt.zone.name()),
        (3, "Europe/Berlin".to_string())
    );
    // gh11854.phpt
    let dt = create("D M d H:i:s Y", "Wed Aug  2 08:37:50 2023", &utc()).unwrap();
    assert_eq!(fmt(&dt, "Y-m-d H:i:s"), "2023-08-02 08:37:50");
    // bug75232.phpt
    let dt = create("Ymd\\THis\\Z", "20170920T091600Z", &utc()).unwrap();
    assert_eq!(
        fmt(&dt, "Y-m-d H:i:s.u e"),
        "2017-09-20 09:16:00.000000 UTC"
    );
    // date_create_from_format_basic2.phpt
    assert!(create("Y-m-d", "2009---01", &utc()).is_none());
    // DateTime_fix_createFromFormat.phpt
    let london = TimeZone::from_id("Europe/London").unwrap();
    let dt = create("D., M# j, Y g:iA", "Thu., Nov. 29, 2012 5:00PM", &london).unwrap();
    assert_eq!(fmt(&dt, "D., M. j, Y g:iA"), "Thu., Nov. 29, 2012 5:00PM");
}

#[test]
fn bug51393_offsets() {
    let cases = [
        ("O", "+0800", 28800),
        ("P", "+08:00", 28800),
        ("O", "-0800", -28800),
        ("P", "-08:00", -28800),
        ("[O]", "[+0800]", 28800),
        ("[P]", "[-08:00]", -28800),
        ("O", "GMT+0800", 28800),
        ("P", "GMT-08:00", -28800),
        ("[O]", "[GMT+0800]", 28800),
        ("[P]", "[GMT-08:00]", -28800),
    ];
    for (f, s, off) in cases {
        let dt = create(f, s, &utc()).unwrap();
        assert_eq!(dt.zone.offset_at(dt.ts), off, "{f} {s}");
    }
    assert!(create("O", "invalid", &utc()).is_none());
}

#[test]
fn bug50392_and_fractions() {
    // bug50392.phpt
    let base = "2009-03-01 18:00:00";
    let expect = [
        None,
        Some(100_000),
        Some(220_000),
        Some(333_000),
        Some(444_400),
        Some(555_550),
        Some(666_666),
        Some(777_777),
    ];
    for (i, e) in expect.iter().enumerate() {
        let s = format!("{base}.{}", i.to_string().repeat(i));
        let p = from_format::parse_from_format(b"Y-m-d H:i:s.u", s.as_bytes());
        match e {
            None => assert_eq!(p.errors[0].1, "Not enough data available to satisfy format"),
            Some(us) => assert_eq!(p.us, Some(*us), "{s}"),
        }
    }
    let p = from_format::parse_from_format(b"Y-m-d H:i:s.u", b"2009-03-01 18:00:00.7777777");
    assert_eq!(p.errors, vec![(26, "Trailing data".to_string())]);
    // date_time_fractions_create_from_format.phpt
    let dt = create("Y-m-d H:i:s.u", "2016-10-03 12:47:18.819313", &utc()).unwrap();
    assert_eq!(fmt(&dt, "Y-m-d H:i:s.u"), "2016-10-03 12:47:18.819313");
    let dt = create("U.u", "1475500799.176312", &utc()).unwrap();
    assert_eq!(fmt(&dt, "Y-m-d H:i:s.u"), "2016-10-03 13:19:59.176312");
    assert_eq!((dt.zone.kind(), dt.zone.name()), (1, "+00:00".to_string()));
    // bug45554.phpt
    let f = "m-d-Y H:i:s.u T";
    for (s, out) in [
        (
            "03-15-2005 12:22:29.000000 PST",
            "03-15-2005 12:22:29.000000 PST",
        ),
        (
            "03-15-2005 12:22:29.001001 PST",
            "03-15-2005 12:22:29.001001 PST",
        ),
        (
            "03-15-2005 12:22:29.0010 PST",
            "03-15-2005 12:22:29.001000 PST",
        ),
    ] {
        assert_eq!(fmt(&create(f, s, &utc()).unwrap(), f), out);
    }
    // bug63435.phpt
    for i in 1..999 {
        let dt = create("u", &format!("{i:06}"), &utc()).unwrap();
        assert_eq!(dt.us, i);
    }
    // bug75577.phpt
    let london = TimeZone::from_id("Europe/London").unwrap();
    let dt = create("Y-m-d\\TH:i:s.vP", "2018-10-09T09:56:45.412+01:00", &london).unwrap();
    assert_eq!(fmt(&dt, "Y-m-d H:i:s.u"), "2018-10-09 09:56:45.412000");
}

#[test]
fn bug51866_lenient() {
    /// (is a warning, position, message)
    type Expect = Option<(bool, usize, &'static str)>;
    let cases: [(&str, &str, Expect); 6] = [
        (
            "Y-m-d",
            "2001-11-29 13:20:01",
            Some((false, 10, "Trailing data")),
        ),
        (
            "Y-m-d+",
            "2001-11-29 13:20:01",
            Some((true, 10, "Trailing data")),
        ),
        (
            "Y-m-d +",
            "2001-11-29 13:20:01",
            Some((true, 11, "Trailing data")),
        ),
        ("Y-m-d+", "2001-11-29", None),
        (
            "Y-m-d +",
            "2001-11-29",
            Some((false, 10, "Not enough data available to satisfy format")),
        ),
        ("Y-m-d +", "2001-11-29 ", None),
    ];
    for (f, s, e) in cases {
        let r = from_format::create_from_format(f.as_bytes(), s.as_bytes(), &utc(), 0, 0);
        match e {
            None => assert!(r.unwrap().1.is_empty(), "{f}"),
            Some((true, pos, m)) => assert_eq!(r.unwrap().1.warnings, vec![(pos, m.to_string())]),
            Some((false, pos, m)) => assert_eq!(r.unwrap_err().errors, vec![(pos, m.to_string())]),
        }
    }
}

#[test]
fn bug54851_day_names() {
    let a = create("!D d M Y", "Fri 17 may 2011", &utc()).unwrap();
    let b = create("!d M Y D", "17 may 2011 Fri", &utc()).unwrap();
    assert_eq!(fmt(&a, "r"), "Fri, 20 May 2011 00:00:00 +0000");
    assert_eq!(fmt(&b, "r"), "Fri, 20 May 2011 00:00:00 +0000");
    let a = create("!D d M Y", "Fri 19 November 2011", &utc()).unwrap();
    assert_eq!(fmt(&a, "r"), "Fri, 25 Nov 2011 00:00:00 +0000");
    let a = create("!D d M Y", "Sat 19 November 2011", &utc()).unwrap();
    assert_eq!(fmt(&a, "r"), "Sat, 19 Nov 2011 00:00:00 +0000");
    let a = create("D H i s", "Tuesday 0 00 00", &utc()).unwrap();
    assert_eq!(fmt(&a, "D H:i:s"), "Tue 00:00:00");
}

#[test]
fn now_fill_and_zones() {
    // bug68669.phpt: the given zone is used
    let la = TimeZone::from_id("America/Los_Angeles").unwrap();
    let dt = create("Y/m/d H:i:s", "1995/06/08 12:34:56", &la).unwrap();
    assert_eq!(
        fmt(&dt, "Y-m-d H:i:s.u e"),
        "1995-06-08 12:34:56.000000 America/Los_Angeles"
    );
    // bug80057.phpt: the time of day comes from now
    let now = 1_700_000_123;
    let r = from_format::create_from_format(b"Y-m-d", b"2020-09-04", &utc(), now, 777)
        .unwrap()
        .0;
    assert_eq!(fmt(&r, "Y-m-d H:i:s.u"), "2020-09-04 22:15:23.000000");
    // bug80610.phpt
    let berlin = TimeZone::from_id("Europe/Berlin").unwrap();
    let dt = create("Y-m-d H:i:s", "2020-04-12 12:00:00", &berlin).unwrap();
    assert_eq!(fmt(&dt, "Y-m-d H:i:s T"), "2020-04-12 12:00:00 CEST");
    // bug72963.phpt
    let dt = create("!m/d/Y", "8/8/2016", &utc()).unwrap();
    assert_eq!(fmt(&dt, "Y-m-d H:i:s.u"), "2016-08-08 00:00:00.000000");
}
