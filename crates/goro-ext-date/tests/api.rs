//! mktime, checkdate, idate, getdate, localtime, strftime, setters and
//! listings: expectations observed on PHP 8.5 (default zone
//! America/New_York unless noted).

use goro_ext_date::api::{self, IdateError, groups};
use goro_ext_date::{DateTime, TimeZone, parse, resolve};

fn ny() -> TimeZone {
    TimeZone::from_id("America/New_York").unwrap()
}

#[test]
fn mktime_cases() {
    let utc = TimeZone::from_id("UTC").unwrap();
    let cases: &[([i64; 6], i64, i64)] = &[
        ([0, 0, 0, 1, 1, 2021], 1609477200, 1609459200),
        ([12, 30, 45, 6, 15, 2021], 1623774645, 1623760245),
        ([0, 0, 0, 13, 1, 2021], 1641013200, 1640995200),
        ([0, 0, 0, 0, 0, 2021], 1606712400, 1606694400),
        ([25, 61, 61, 2, 30, 2021], 1614754921, 1614736921),
        ([0, 0, 0, 1, 1, 69], 3124242000, 3124224000),
        ([0, 0, 0, 1, 1, 70], 18000, 0),
        ([0, 0, 0, 1, 1, 100], 946702800, 946684800),
        ([0, 0, 0, 1, 1, 101], -58979905438, -58979923200),
        ([0, 0, 0, 1, 1, 0], 946702800, 946684800),
        ([0, 0, 0, 1, 1, -1], -62198737438, -62198755200),
        ([0, 0, 0, 1, 1, 1969], -31518000, -31536000),
        ([2, 30, 0, 3, 14, 2021], 1615707000, 1615689000),
        ([1, 30, 0, 11, 7, 2021], 1636263000, 1636248600),
        ([0, 0, 0, 1, 1, 10000], 253402318800, 253402300800),
        ([0, 0, 0, 1, 1, -10000], -377736721438, -377736739200),
        ([0, 0, 0, -1, -1, 2021], 1604030400, 1604016000),
        ([-1, -1, -1, 1, 1, 2021], 1609473539, 1609455539),
    ];
    for (a, local, gm) in cases {
        assert_eq!(
            api::mktime(a[0], a[1], a[2], a[3], a[4], a[5], &ny()),
            *local,
            "{a:?}"
        );
        assert_eq!(
            api::mktime(a[0], a[1], a[2], a[3], a[4], a[5], &utc),
            *gm,
            "{a:?}"
        );
    }
}

#[test]
fn checkdate_cases() {
    for (a, want) in [
        ([2, 29, 2020], true),
        ([2, 29, 2021], false),
        ([2, 29, 2000], true),
        ([2, 29, 1900], false),
        ([12, 31, 32767], true),
        ([1, 1, 32768], false),
        ([1, 1, 0], false),
        ([1, 1, 1], true),
        ([0, 1, 2020], false),
        ([13, 1, 2020], false),
        ([1, 0, 2020], false),
        ([1, 32, 2020], false),
        ([4, 31, 2020], false),
    ] {
        assert_eq!(api::checkdate(a[0], a[1], a[2]), want, "{a:?}");
    }
}

#[test]
fn idate_cases() {
    let ts = 1625403909;
    let want = "B=586 d=4 h=9 H=9 i=5 I=1 L=0 m=7 N=7 o=2021 s=9 t=31 U=1625403909 w=0 W=26 y=21 Y=2021 z=184 Z=-14400";
    let got: Vec<String> = "BdhHiILmNostUwWyYzZ"
        .chars()
        .map(|c| format!("{c}={}", api::idate(&[c as u8], ts, &ny()).unwrap()))
        .collect();
    assert_eq!(got.join(" "), want);
    assert_eq!(api::idate(b"", ts, &ny()), Err(IdateError::NotOneChar));
    assert_eq!(api::idate(b"dd", ts, &ny()), Err(IdateError::NotOneChar));
    assert_eq!(api::idate(b"x", ts, &ny()), Err(IdateError::Unrecognized));
    assert_eq!(
        IdateError::Unrecognized.message(),
        "Unrecognized date format token"
    );
    let y2005 = api::mktime(0, 0, 0, 1, 1, 2005, &ny());
    assert_eq!(api::idate(b"y", y2005, &ny()), Ok(5));
}

#[test]
fn getdate_localtime() {
    let g = api::getdate(1625403909, &ny());
    assert_eq!(
        (
            g.seconds, g.minutes, g.hours, g.mday, g.wday, g.mon, g.year, g.yday, g.weekday,
            g.month, g.ts
        ),
        (9, 5, 9, 4, 0, 7, 2021, 184, "Sunday", "July", 1625403909)
    );
    let g = api::getdate(-1, &ny());
    assert_eq!(
        (g.hours, g.mday, g.year, g.yday, g.weekday),
        (18, 31, 1969, 364, "Wednesday")
    );
    assert_eq!(
        api::localtime(1625403909, &ny()),
        [9, 5, 9, 4, 6, 121, 0, 184, 1]
    );
    assert_eq!(
        api::localtime(1609459200, &ny()),
        [0, 0, 19, 31, 11, 120, 4, 365, 0]
    );
}

#[test]
fn strftime_cases() {
    let f = "%a %A %b %B %c %C %d %D %e %F %g %G %h %H %I %j %k %l %m %M %n %p %P %r %R %s %S %t %T %u %U %V %w %W %x %X %y %Y %z %Z %%";
    let utc = TimeZone::from_id("UTC").unwrap();
    assert_eq!(
        String::from_utf8(api::strftime(f.as_bytes(), 1625403909, &ny(), false)).unwrap(),
        "Sun Sunday Jul July Sun Jul  4 09:05:09 2021 20 04 07/04/21  4 2021-07-04 21 2021 Jul 09 09 185  9  9 07 05 \n AM am 09:05:09 AM 09:05 1625385909 09 \t 09:05:09 7 27 26 0 26 07/04/21 09:05:09 21 2021 -0400 EDT %"
    );
    assert_eq!(
        String::from_utf8(api::strftime(f.as_bytes(), 1625403909, &utc, true)).unwrap(),
        "Sun Sunday Jul July Sun Jul  4 13:05:09 2021 20 04 07/04/21  4 2021-07-04 21 2021 Jul 13 01 185 13  1 07 05 \n PM pm 01:05:09 PM 13:05 1625403909 09 \t 13:05:09 7 27 26 0 26 07/04/21 13:05:09 21 2021 +0000 GMT %"
    );
    for (fmt, want) in [
        ("%E", "%E"),
        ("%O", "%O"),
        ("%q", "%q"),
        ("%", "%"),
        ("x%", "x%"),
    ] {
        assert_eq!(
            String::from_utf8(api::strftime(fmt.as_bytes(), 1625403909, &ny(), false)).unwrap(),
            want
        );
    }
    assert_eq!(
        String::from_utf8(api::strftime(
            b"%a %b %e %V %G %g %U %W %j",
            1609459200,
            &ny(),
            false
        ))
        .unwrap(),
        "Thu Dec 31 53 2020 20 52 52 366"
    );
}

fn base() -> DateTime {
    let p = parse::parse(b"2021-06-15 13:45:56.654321");
    resolve::resolve(&p, 0, 0, &ny(), resolve::Mode::Create)
}

fn show(d: &DateTime) -> String {
    String::from_utf8(d.format(b"Y-m-d H:i:s.u T")).unwrap()
}

#[test]
fn setters() {
    let d = base();
    for (a, want) in [
        ([2021, 2, 30], "2021-03-02 13:45:56.654321 EST"),
        ([2021, 13, 1], "2022-01-01 13:45:56.654321 EST"),
        ([2021, 0, 0], "2020-11-30 13:45:56.654321 EST"),
        ([-5, 1, 1], "-0005-01-01 13:45:56.654321 LMT"),
        ([2021, -1, -1], "2020-10-30 13:45:56.654321 EDT"),
    ] {
        assert_eq!(show(&d.set_date(a[0], a[1], a[2])), want, "{a:?}");
    }
    for (a, want) in [
        ([2021, 1, 1], "2021-01-04 13:45:56.654321 EST"),
        ([2021, 53, 1], "2022-01-03 13:45:56.654321 EST"),
        ([2021, 1, 0], "2021-01-03 13:45:56.654321 EST"),
        ([2021, 1, 8], "2021-01-11 13:45:56.654321 EST"),
        ([2021, 0, 1], "2020-12-28 13:45:56.654321 EST"),
        ([2020, 53, 7], "2021-01-03 13:45:56.654321 EST"),
        ([2021, -1, 1], "2020-12-21 13:45:56.654321 EST"),
    ] {
        assert_eq!(show(&d.set_iso_date(a[0], a[1], a[2])), want, "{a:?}");
    }
    for (a, want) in [
        ([25, 0, 0, 0], "2021-06-16 01:00:00.000000 EDT"),
        ([0, 0, 0, 1_000_001], "2021-06-15 00:00:01.000001 EDT"),
        ([-1, 0, 0, 0], "2021-06-14 23:00:00.000000 EDT"),
        ([10, 70, 70, 0], "2021-06-15 11:11:10.000000 EDT"),
        ([10, 0, 0, -1], "2021-06-15 09:59:59.999999 EDT"),
    ] {
        assert_eq!(show(&d.set_time(a[0], a[1], a[2], a[3])), want, "{a:?}");
    }
    assert_eq!(show(&d.set_timestamp(0)), "1969-12-31 19:00:00.000000 EST");
    assert_eq!(
        show(&d.set_timezone(TimeZone::from_id("Asia/Tokyo").unwrap())),
        "2021-06-16 02:45:56.654321 JST"
    );
    assert_eq!((d.ts, d.offset()), (1623779156, -14400));
    assert_eq!(d.dump_date(), "2021-06-15 13:45:56.654321");
    let f = |t: f64| {
        DateTime::from_float_timestamp(t)
            .map(|d| String::from_utf8(d.format(b"Y-m-d H:i:s.u T e")).unwrap())
    };
    assert_eq!(
        f(0.0).unwrap(),
        "1970-01-01 00:00:00.000000 GMT+0000 +00:00"
    );
    assert_eq!(
        f(1.5).unwrap(),
        "1970-01-01 00:00:01.500000 GMT+0000 +00:00"
    );
    assert_eq!(
        f(-1.5).unwrap(),
        "1969-12-31 23:59:58.500000 GMT+0000 +00:00"
    );
    assert_eq!(
        f(-0.000001).unwrap(),
        "1969-12-31 23:59:59.999999 GMT+0000 +00:00"
    );
    assert_eq!(
        f("1625403909.123456789".parse().unwrap()).unwrap(),
        "2021-07-04 13:05:09.123457 GMT+0000 +00:00"
    );
    assert_eq!(f(1e20), None);
    assert_eq!(f(f64::NAN), None);
}

#[test]
fn listings() {
    let counts: Vec<usize> = [
        groups::AFRICA,
        groups::AMERICA,
        groups::ANTARCTICA,
        groups::ARCTIC,
        groups::ASIA,
        groups::ATLANTIC,
        groups::AUSTRALIA,
        groups::EUROPE,
        groups::INDIAN,
        groups::PACIFIC,
        groups::UTC,
        groups::ALL,
        0,
        3,
        4097,
    ]
    .iter()
    .map(|g| api::list_identifiers(*g, None).len())
    .collect();
    // observed on PHP 8.5 (tzdata 2026e)
    assert_eq!(
        counts,
        [52, 144, 11, 1, 82, 10, 11, 58, 11, 38, 1, 419, 0, 196, 52]
    );
    assert_eq!(api::list_identifiers(groups::UTC, None), ["UTC"]);
    assert_eq!(
        api::list_identifiers(groups::ARCTIC, None),
        ["Arctic/Longyearbyen"]
    );
    let us = api::list_identifiers(groups::PER_COUNTRY, Some("US"));
    assert_eq!(us.len(), 29);
    assert_eq!(us[0], "America/Adak");
    assert_eq!(us[28], "Pacific/Honolulu");
    assert!(api::list_identifiers(groups::PER_COUNTRY, Some("us")).is_empty());
    assert_eq!(
        api::list_identifiers(groups::PER_COUNTRY, Some("FR")),
        ["Europe/Paris"]
    );
    let eu = api::list_identifiers(groups::EUROPE | groups::UTC, None);
    assert_eq!(eu.len(), 59);
    assert_eq!(eu[0], "Europe/Amsterdam");
    assert_eq!(eu[58], "UTC");
    // test timezones-list.phpt
    let all = api::list_identifiers(groups::ALL, None);
    let bc = api::list_identifiers(groups::ALL_WITH_BC, None);
    assert!(
        all.contains(&"Europe/Oslo") && bc.contains(&"US/Eastern") && !all.contains(&"US/Eastern")
    );
}

#[test]
fn transitions_get() {
    let paris = TimeZone::from_id("Europe/Paris").unwrap();
    let t = api::transitions(&paris, 1700000000, 1730000000).unwrap();
    assert_eq!(t.len(), 3);
    assert_eq!(
        (
            t[0].ts,
            t[0].time.as_str(),
            t[0].offset,
            t[0].isdst,
            t[0].abbr.as_str()
        ),
        (1700000000, "2023-11-14T22:13:20+00:00", 3600, false, "CET")
    );
    assert_eq!((t[1].ts, t[1].offset, t[1].isdst), (1711846800, 7200, true));
    assert_eq!(
        api::transitions(&paris, 1700000000, 1700000000)
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        api::transitions(&paris, 2000000000, 1000000000)
            .unwrap()
            .len(),
        1
    );
    let all = api::transitions(&paris, i64::MIN, api::TRANSITIONS_DEFAULT_END).unwrap();
    assert_eq!(all.len(), 185);
    assert_eq!(all[0].time, "-292277022657-01-27T08:29:52+00:00");
    assert_eq!((all[0].offset, all[0].abbr.as_str()), (561, "LMT"));
    assert!(api::transitions(&TimeZone::parse("+05:00").unwrap(), 0, 1).is_none());
    // test DateTimeZone_getTransitions_basic1 shape: New York has 237
    // entries up to 2037 (observed on PHP 8.5)
    assert_eq!(
        api::transitions(&ny(), i64::MIN, api::TRANSITIONS_DEFAULT_END)
            .unwrap()
            .len(),
        237
    );
}
