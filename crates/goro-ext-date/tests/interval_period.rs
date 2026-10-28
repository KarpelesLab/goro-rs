//! DateInterval and DatePeriod: cases observed on PHP 8.5 and ported from
//! ext/date tests.

use goro_ext_date::interval::{self, Interval};
use goro_ext_date::period::{self, Period, PeriodError};
use goro_ext_date::{DateTime, TimeZone, parse, resolve};

fn dt(s: &str, zone: &str) -> DateTime {
    let p = parse::parse(s.as_bytes());
    assert!(p.errors.is_empty(), "{s}");
    resolve::resolve(
        &p,
        0,
        0,
        &TimeZone::parse(zone).unwrap(),
        resolve::Mode::Create,
    )
}

fn fields(i: &Interval) -> [i64; 7] {
    [i.y, i.m, i.d, i.h, i.i, i.s, i.us]
}

#[test]
fn iso_durations() {
    // observed on PHP 8.5
    let ok: &[(&str, [i64; 7])] = &[
        ("P1Y2M3DT4H5M6S", [1, 2, 3, 4, 5, 6, 0]),
        ("P1W", [0, 0, 7, 0, 0, 0, 0]),
        ("P2W3D", [0, 0, 17, 0, 0, 0, 0]),
        ("PT36H", [0, 0, 0, 36, 0, 0, 0]),
        ("P0D", [0; 7]),
        ("PT0S", [0; 7]),
        ("P1D ", [0, 0, 1, 0, 0, 0, 0]),
        (" P1D", [0, 0, 1, 0, 0, 0, 0]),
        ("P2147483648D", [0, 0, 2_147_483_648, 0, 0, 0, 0]),
        ("P0001-02-03T04:05:06", [1, 2, 3, 4, 5, 6, 0]),
        ("P1Y2M3W4D", [1, 2, 25, 0, 0, 0, 0]),
        ("P5W", [0, 0, 35, 0, 0, 0, 0]),
        ("R5/P1D", [0, 0, 1, 0, 0, 0, 0]),
        ("PT24H", [0, 0, 0, 24, 0, 0, 0]),
        ("P12M", [0, 12, 0, 0, 0, 0, 0]),
        ("PT60M", [0, 0, 0, 0, 60, 0, 0]),
        ("P0000-00-00T00:00:00", [0; 7]),
        ("P1WT1H", [0, 0, 7, 1, 0, 0, 0]),
    ];
    for (s, f) in ok {
        let i = Interval::parse_iso(s.as_bytes()).unwrap_or_else(|e| panic!("{s}: {e}"));
        assert_eq!(&fields(&i), f, "{s}");
    }
    for s in [
        "P1D1W",
        "P",
        "PT",
        "P1",
        "P1Y2",
        "PT1H2M3",
        "p1d",
        "P1d",
        "P1Dt1H",
        "P1DT1h",
        "PT1.5S",
        "PT0,5S",
        "P-1D",
        "P+1D",
        "P 1D",
        "P99999999999999999999D",
        "P9223372036854775807D",
        "P00010203T040506",
        "P0001-02-03",
        "P1Y1Y",
        "P1M1Y",
        "PT1S1M",
        "P1DT",
        "PT1M1H",
        "P1.5Y",
        "P1DT1H1M1S1",
        "P0001-13-01T00:00:00",
        "P10000-01-01T00:00:00",
        "P1Y-1M",
        "PT1H-1M",
        "P1E",
        "PT1W",
        "P1W2D3W",
    ] {
        assert_eq!(
            Interval::parse_iso(s.as_bytes()),
            Err(format!("Unknown or bad format ({s})")),
            "{s}"
        );
    }
}

#[test]
fn interval_format() {
    let i = Interval::parse_iso(b"P1Y2M3DT4H5M6S").unwrap();
    let f = |i: &Interval, s: &str| String::from_utf8(i.format(s.as_bytes())).unwrap();
    assert_eq!(
        f(
            &i,
            "%Y %y %M %m %D %d %H %h %I %i %S %s %F %f %R %r %a %% %x %"
        ),
        "01 1 02 2 03 3 04 4 05 5 06 6 000000 0 +  (unknown) % %x "
    );
    assert_eq!(f(&i, "%%Y"), "%Y");
    assert_eq!(f(&i, "%"), "");
    assert_eq!(f(&i, "%R%y"), "+1");
    assert_eq!(f(&i, "%z%Z%q"), "%z%Z%q");
    assert_eq!(f(&i, "%-y"), "%-y");
    let mut j = i.clone();
    j.invert = true;
    j.us = 123_456;
    assert_eq!(f(&j, "%R %r %F %f %a"), "- - 123456 123456 (unknown)");
    let k = Interval::parse_iso(b"P123Y45M67DT89H101M121S").unwrap();
    assert_eq!(
        f(&k, "%Y %y %M %m %D %d %H %h %I %i %S %s %a"),
        "123 123 45 45 67 67 89 89 101 101 121 121 (unknown)"
    );
    let d = interval::diff(
        &dt("2021-01-01", "UTC"),
        &dt("2021-03-04 05:06:07.5", "UTC"),
        false,
    );
    assert_eq!(
        f(&d, "%Y %y %M %m %D %d %H %h %I %i %S %s %F %f %R %r %a"),
        "00 0 02 2 03 3 05 5 06 6 07 7 500000 500000 +  62"
    );
}

#[test]
fn from_date_string() {
    let i = Interval::from_date_string(b"next monday +2 days 3 hours").unwrap();
    let f = |i: &Interval, s: &str| String::from_utf8(i.format(s.as_bytes())).unwrap();
    assert_eq!(f(&i, "%y %m %d %h %i %s %a"), "0 0 2 3 0 0 (unknown)");
    assert!(i.from_string.is_some());
    assert_eq!(
        Interval::from_date_string(b"foo"),
        Err("Unknown or bad format (foo) at position 0 (f): The timezone could not be found in the database".into())
    );
}

fn list(p: &Period) -> Vec<String> {
    p.iter()
        .take(10)
        .map(|d| String::from_utf8(d.format(b"Y-m-d H:i:s.u T")).unwrap())
        .collect()
}

#[test]
fn periods() {
    // observed on PHP 8.5
    let s = dt("2021-01-31 10:00", "UTC");
    let p1m = Interval::parse_iso(b"P1M").unwrap();
    let mk = |end: Option<&str>, rec: Option<i64>, opt: i64| {
        Period::new(
            s.clone(),
            p1m.clone(),
            end.map(|e| dt(e, "UTC")),
            rec,
            opt,
            "DatePeriod::__construct",
        )
        .unwrap()
    };
    let p = mk(None, Some(3), 0);
    assert_eq!(
        list(&p),
        [
            "2021-01-31 10:00:00.000000 UTC",
            "2021-03-03 10:00:00.000000 UTC",
            "2021-04-03 10:00:00.000000 UTC",
            "2021-05-03 10:00:00.000000 UTC"
        ]
    );
    assert_eq!(
        (p.get_recurrences(), p.recurrences_property()),
        (Some(3), 4)
    );
    let p = mk(None, Some(3), period::EXCLUDE_START_DATE);
    assert_eq!(list(&p).len(), 3);
    assert_eq!(p.recurrences_property(), 3);
    let p = mk(Some("2021-05-31 10:00"), None, 0);
    assert_eq!(list(&p).len(), 4);
    assert_eq!((p.get_recurrences(), p.recurrences_property()), (None, 1));
    assert_eq!(
        mk(Some("2021-05-31 10:00"), None, period::INCLUDE_END_DATE).recurrences_property(),
        2
    );
    assert_eq!(
        mk(Some("2021-04-03 10:00"), None, period::INCLUDE_END_DATE)
            .iter()
            .count(),
        3
    );
    assert_eq!(mk(Some("2021-04-03 10:00"), None, 0).iter().count(), 2);
    let mut it = p.iter();
    for _ in it.by_ref() {}
    assert_eq!(
        String::from_utf8(it.current().unwrap().format(b"c")).unwrap(),
        "2021-06-03T10:00:00+00:00"
    );
    for r in [0, -1, 2_147_483_640, 2_147_483_647] {
        assert_eq!(
            Period::new(s.clone(), p1m.clone(), None, Some(r), 0, "DatePeriod::__construct"),
            Err(PeriodError::MalformedPeriod(
                "DatePeriod::__construct(): Recurrence count must be greater or equal to 1 and lower than 2147483640"
                    .into()
            ))
        );
    }
    assert!(matches!(
        Period::new(
            s.clone(),
            p1m.clone(),
            None,
            Some(2_147_483_639),
            0,
            "DatePeriod::__construct"
        ),
        Err(PeriodError::Malformed(_))
    ));

    // DST: wall clock steps (America/New_York)
    let ny = "America/New_York";
    let pd = Interval::parse_iso(b"P1D").unwrap();
    let p = Period::new(
        dt("2021-03-13 02:30", ny),
        pd.clone(),
        None,
        Some(3),
        0,
        "x",
    )
    .unwrap();
    assert_eq!(
        list(&p),
        [
            "2021-03-13 02:30:00.000000 EST",
            "2021-03-14 03:30:00.000000 EDT",
            "2021-03-15 03:30:00.000000 EDT",
            "2021-03-16 03:30:00.000000 EDT"
        ]
    );
    let p = Period::new(
        dt("2021-11-06 01:30", ny),
        pd.clone(),
        None,
        Some(2),
        0,
        "x",
    )
    .unwrap();
    assert_eq!(
        list(&p),
        [
            "2021-11-06 01:30:00.000000 EDT",
            "2021-11-07 01:30:00.000000 EDT",
            "2021-11-08 01:30:00.000000 EST"
        ]
    );
    let p = Period::new(
        dt("2021-11-07 00:30", ny),
        Interval::parse_iso(b"PT30M").unwrap(),
        None,
        Some(5),
        0,
        "x",
    )
    .unwrap();
    assert_eq!(
        list(&p),
        [
            "2021-11-07 00:30:00.000000 EDT",
            "2021-11-07 01:00:00.000000 EDT",
            "2021-11-07 01:30:00.000000 EDT",
            "2021-11-07 02:00:00.000000 EST",
            "2021-11-07 02:30:00.000000 EST",
            "2021-11-07 03:00:00.000000 EST"
        ]
    );
    let p = Period::new(
        dt("2021-01-31", ny),
        Interval::from_date_string(b"last day of next month").unwrap(),
        None,
        Some(4),
        0,
        "x",
    )
    .unwrap();
    assert_eq!(
        list(&p)[1..3],
        [
            "2021-02-28 00:00:00.000000 EST",
            "2021-03-31 00:00:00.000000 EDT"
        ]
    );
    // invert is ignored
    let mut back = pd.clone();
    back.invert = true;
    let p = Period::new(dt("2021-01-31", ny), back, None, Some(3), 0, "x").unwrap();
    assert_eq!(list(&p)[1], "2021-02-01 00:00:00.000000 EST");
}

#[test]
fn iso_periods() {
    let f = "DatePeriod::createFromISO8601String";
    let p = Period::from_iso(b"R4/2012-07-01T00:00:00Z/P7D", 0, f).unwrap();
    assert_eq!(list(&p).len(), 5);
    assert_eq!(list(&p)[1], "2012-07-08 00:00:00.000000 GMT+0000");
    assert_eq!(
        (p.get_recurrences(), p.recurrences_property()),
        (Some(4), 5)
    );
    let p = Period::from_iso(b"R2/2012-07-01T00:00:00Z/P1D/2012-07-10T00:00:00Z", 0, f).unwrap();
    assert_eq!(p.iter().count(), 9);
    assert_eq!(
        (p.get_recurrences(), p.recurrences_property()),
        (Some(2), 3)
    );
    let p = Period::from_iso(b"2012-07-01T00:00:00Z/P1D/2012-07-05T00:00:00Z", 0, f).unwrap();
    assert_eq!(p.iter().count(), 4);
    assert_eq!(p.get_recurrences(), None);
    assert_eq!(
        Period::from_iso(b"R2147483647/2012-07-01T00:00:00Z/P7D", 0, f)
            .unwrap()
            .recurrences,
        Some(214_748_364)
    );
    assert_eq!(
        list(&Period::from_iso(b"R4/20120701T000000Z/P7D", 0, f).unwrap()).len(),
        5
    );
    assert_eq!(
        list(&Period::from_iso(b"R4/2012-07-01T24:00:00Z/P7D", 0, f).unwrap())[0],
        "2012-07-02 00:00:00.000000 GMT+0000"
    );
    assert_eq!(
        list(&Period::from_iso(b"R4/2012-07-01T00:00:00Z/P7D/P1D", 0, f).unwrap())[1],
        "2012-07-09 00:00:00.000000 GMT+0000"
    );
    assert_eq!(
        Period::from_iso(b"R4/2012-07-01T00:00:00Z/P7D/R3", 0, f)
            .unwrap()
            .recurrences,
        Some(3)
    );
    assert_eq!(
        Period::from_iso(b"R4/2012-07-01T00:00:00Z/2012-07-02T00:00:00Z/P7D", 0, f)
            .unwrap()
            .iter()
            .count(),
        1
    );
    for bad in [
        "R4/2012-07-01T00:00:00+02:00/P7D",
        "R/2012-07-01T00:00:00Z/P7D",
        "R4/2012-07-01/P7D",
        "R4/2012-07-01 00:00:00/P7D",
        "R4/2012-07-01T00:00:00Z/foo",
        "r4/2012-07-01T00:00:00Z/P7D",
        "R4/2012-07-01T00:00:00.5Z/P7D",
        "R4/2012-07-01T00:00Z/P7D",
        "R4/2012-07-01T00:00:00z/P7D",
        "R4/2012-7-1T0:0:0Z/P7D",
    ] {
        assert_eq!(
            Period::from_iso(bad.as_bytes(), 0, f),
            Err(PeriodError::MalformedPeriod(format!(
                "Unknown or bad format ({bad})"
            ))),
            "{bad}"
        );
    }
    // test date_period_bad_iso_format.phpt
    assert_eq!(
        Period::from_iso(b"R4", 0, f),
        Err(PeriodError::MalformedPeriod(
            "DatePeriod::createFromISO8601String(): ISO interval must contain a start date, \"R4\" given".into()
        ))
    );
    assert_eq!(
        Period::from_iso(b"R4/2012-07-01T00:00:00Z", 0, f),
        Err(PeriodError::MalformedPeriod(
            "DatePeriod::createFromISO8601String(): ISO interval must contain an interval, \"R4/2012-07-01T00:00:00Z\" given".into()
        ))
    );
    assert_eq!(
        Period::from_iso(b"2012-07-01T00:00:00Z/P7D", 0, f),
        Err(PeriodError::MalformedPeriod(
            "DatePeriod::createFromISO8601String(): Recurrence count must be greater or equal to 1 and lower than 2147483640".into()
        ))
    );
}

#[test]
fn phpt_date_period() {
    // test date_period.phpt
    let utc = TimeZone::from_id("UTC").unwrap();
    let start = dt("2008-01-01", "UTC");
    let p = Period::new(
        start,
        Interval::from_date_string(b"first day of next month").unwrap(),
        Some(dt("2008-12-31", "UTC")),
        None,
        0,
        "x",
    )
    .unwrap();
    let got: Vec<String> = p
        .iter()
        .map(|d| {
            let m = resolve::modify(b"3 tuesday", d.ts, d.us, &utc).unwrap();
            String::from_utf8(m.format(b"l Y-m-d")).unwrap()
        })
        .collect();
    assert_eq!(
        got,
        [
            "Tuesday 2008-01-15",
            "Tuesday 2008-02-19",
            "Tuesday 2008-03-18",
            "Tuesday 2008-04-15",
            "Tuesday 2008-05-20",
            "Tuesday 2008-06-17",
            "Tuesday 2008-07-15",
            "Tuesday 2008-08-19",
            "Tuesday 2008-09-16",
            "Tuesday 2008-10-21",
            "Tuesday 2008-11-18",
            "Tuesday 2008-12-16"
        ]
    );
    let p = Period::new(
        dt("2007-12-31", "UTC"),
        Interval::from_date_string(b"last thursday of next month").unwrap(),
        Some(dt("2009-12-31 23:59:59", "UTC")),
        None,
        period::EXCLUDE_START_DATE,
        "x",
    )
    .unwrap();
    let got: Vec<String> = p
        .iter()
        .map(|d| String::from_utf8(d.format(b"l Y-m-d H:i:s")).unwrap())
        .collect();
    assert_eq!(got.len(), 24);
    assert_eq!(got[0], "Thursday 2008-01-31 00:00:00");
    assert_eq!(got[1], "Thursday 2008-02-28 00:00:00");
    assert_eq!(got[23], "Thursday 2009-12-31 00:00:00");
}
