//! Sunrise/sunset: the ext/date/tests expectations about the sun functions,
//! and a batch of PHP 8.5 black-box results (tests/fixtures/sun_php85.txt).

use goro_ext_date::civil::{Civil, civil_to_ts};
use goro_ext_date::sun::{
    self, DEFAULT_LATITUDE, DEFAULT_LONGITUDE, DEFAULT_ZENITH, SunInfo, SunResult, SunTime,
    default_utc_offset_hours,
};

type SunFn = fn(i64, i64, f64, f64, f64, f64, i64) -> Result<SunResult, sun::SunError>;

const RET_TIMESTAMP: i64 = 0;
const RET_STRING: i64 = 1;
const RET_DOUBLE: i64 = 2;

fn at(t: SunTime) -> i64 {
    match t {
        SunTime::At(ts) => ts,
        other => panic!("expected a timestamp, got {other:?}"),
    }
}

/// `date('H:i:s', $ts)` in a zone of fixed offset `off`.
fn hms(ts: i64, off: i64) -> String {
    let c = Civil::from_ts(ts + off);
    format!("{:02}:{:02}:{:02}", c.h, c.i, c.s)
}

fn info(ts: i64, lat: f64, lon: f64, off: i64) -> SunInfo {
    sun::sun_info(ts, lat, lon, off).unwrap()
}

fn s(r: SunResult) -> String {
    match r {
        SunResult::String(s) => s,
        other => panic!("expected a string, got {other:?}"),
    }
}

// test ext/date/tests/bug-gh18076.phpt
#[test]
fn gh18076_greenwich_equinox() {
    let i = info(1_742_515_200, 51.48, 0.0, 0);
    assert_eq!(hms(at(i.sunrise), 0), "05:59:21");
    assert_eq!(hms(at(i.sunset), 0), "18:14:48");
}

// test ext/date/tests/date_sun_info_001.phpt
#[test]
fn sun_info_001() {
    let i = info(1_165_881_600, 31.7667, 35.2333, 0);
    let got: Vec<i64> = i.entries().iter().map(|(_, t)| at(*t)).collect();
    assert_eq!(
        got,
        [
            1165897761, 1165934160, 1165915961, 1165896156, 1165935765, 1165894334, 1165937588,
            1165892551, 1165939371
        ]
    );
    assert_eq!(SunInfo::KEYS[0], "sunrise");
    assert_eq!(SunInfo::KEYS[8], "astronomical_twilight_end");
}

// test ext/date/tests/date_sun_info_002.phpt (Europe/Oslo, CEST = +2)
#[test]
fn sun_info_002() {
    let i = info(1_176_453_075, 59.21, 9.61, 7200);
    let want = [
        "06:13:31", "20:30:51", "13:22:11", "05:29:22", "21:15:00", "04:31:43", "22:12:39",
        "03:17:01", "23:27:21",
    ];
    for ((key, t), w) in i.entries().iter().zip(want) {
        let ts = at(*t);
        let c = Civil::from_ts(ts + 7200);
        assert_eq!((c.y, c.m, c.d), (2007, 4, 13), "{key}");
        assert_eq!(hms(ts, 7200), w, "{key}");
    }
}

// test ext/date/tests/date_sun_info_003.phpt (America/Sao_Paulo; 2015-01-12
// 00:00 UTC is 22:00 on the 11th at -02, 2015-09-12 00:00 UTC 21:00 at -03)
#[test]
fn sun_info_003_polar() {
    let i = info(1_421_020_800, 89.0, 1.0, -7200);
    assert_eq!(hms(at(i.transit), -7200), "10:03:48");
    for (key, t) in i.entries() {
        if key != "transit" {
            assert_eq!(t, SunTime::AlwaysBelow, "{key}");
        }
    }
    let i = info(1_442_016_000, 89.0, 1.0, -10800);
    assert_eq!(hms(at(i.transit), -10800), "08:52:44");
    for (key, t) in i.entries() {
        if key != "transit" {
            assert_eq!(t, SunTime::AlwaysAbove, "{key}");
        }
    }
}

// test ext/date/tests/gh14732.phpt
#[test]
fn non_finite_coordinates() {
    let lat = "date_sun_info(): Argument #2 ($latitude) must be finite";
    let lon = "date_sun_info(): Argument #3 ($longitude) must be finite";
    assert_eq!(sun::sun_info(1, f64::NAN, 1.0, 0).unwrap_err().0, lat);
    assert_eq!(
        sun::sun_info(1, f64::NEG_INFINITY, 1.0, 0).unwrap_err().0,
        lat
    );
    assert_eq!(sun::sun_info(1, 1.0, f64::NAN, 0).unwrap_err().0, lon);
    assert_eq!(sun::sun_info(1, 1.0, f64::INFINITY, 0).unwrap_err().0, lon);
    let r = sun::sunset(1, RET_STRING, f64::NAN, 1.0, DEFAULT_ZENITH, 0.0, 0);
    assert_eq!(r, Ok(SunResult::False));
    let r = sun::sunrise(1, RET_STRING, 1.0, f64::NAN, DEFAULT_ZENITH, 0.0, 0);
    assert_eq!(r, Ok(SunResult::False));
    // observed on PHP 8.5: false in every format, the timestamp one included
    for f in [RET_TIMESTAMP, RET_STRING, RET_DOUBLE] {
        for x in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(
                sun::sunrise(0, f, x, 0.0, 90.0, 0.0, 0),
                Ok(SunResult::False)
            );
            assert_eq!(
                sun::sunrise(0, f, 0.0, x, 90.0, 0.0, 0),
                Ok(SunResult::False)
            );
        }
    }
}

// observed on PHP 8.5: a NaN/INF zenith gives PHP_INT_MIN as a timestamp and
// false otherwise; a non-finite offset leaves the timestamp alone
#[test]
fn non_finite_zenith_and_offset() {
    for x in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let r = sun::sunrise(0, RET_TIMESTAMP, 0.0, 0.0, x, 0.0, 0);
        assert_eq!(r, Ok(SunResult::Timestamp(i64::MIN)));
        assert_eq!(
            sun::sunrise(0, RET_STRING, 0.0, 0.0, x, 0.0, 0),
            Ok(SunResult::False)
        );
        assert_eq!(
            sun::sunrise(0, RET_DOUBLE, 0.0, 0.0, x, 0.0, 0),
            Ok(SunResult::False)
        );
        let r = sun::sunrise(0, RET_TIMESTAMP, 0.0, 0.0, 90.0, x, 0);
        assert_eq!(r, Ok(SunResult::Timestamp(21739)));
        let r = sun::sunset(0, RET_TIMESTAMP, 0.0, 0.0, 90.0, x, 0);
        assert_eq!(r, Ok(SunResult::Timestamp(65081)));
        assert_eq!(
            sun::sunrise(0, RET_DOUBLE, 0.0, 0.0, 90.0, x, 0),
            Ok(SunResult::False)
        );
        assert_eq!(
            sun::sunset(0, RET_STRING, 0.0, 0.0, 90.0, x, 0),
            Ok(SunResult::False)
        );
    }
}

// tests ext/date/tests/gh16454.phpt and gh18481.phpt
#[test]
fn huge_offsets() {
    for off in [f64::MAX, -f64::MAX] {
        assert_eq!(
            sun::sunrise(0, RET_STRING, 61.0, -150.0, 90.0, off, 0),
            Ok(SunResult::False)
        );
        assert_eq!(
            sun::sunset(0, RET_STRING, 61.0, -150.0, 90.0, off, 0),
            Ok(SunResult::False)
        );
    }
    for off in [f64::NAN, -f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let r = sun::sunrise(1_700_000_000, RET_STRING, 38.4, -9.0, 90.0, off, 0);
        assert_eq!(r, Ok(SunResult::False));
    }
}

// test ext/date/tests/date_sunrise_and_sunset_error.phpt (and -1, 100
// observed on PHP 8.5)
#[test]
fn bad_return_format() {
    let tail = "(): Argument #2 ($returnFormat) must be one of SUNFUNCS_RET_TIMESTAMP, \
                SUNFUNCS_RET_STRING, or SUNFUNCS_RET_DOUBLE";
    let r = sun::sunrise(0, 3, DEFAULT_LATITUDE, DEFAULT_LONGITUDE, 90.0, 0.0, 0);
    assert_eq!(r.unwrap_err().0, format!("date_sunrise{tail}"));
    for f in [4, -1, 100] {
        let r = sun::sunset(0, f, DEFAULT_LATITUDE, DEFAULT_LONGITUDE, 90.0, 0.0, 0);
        assert_eq!(r.unwrap_err().0, format!("date_sunset{tail}"));
    }
}

// test ext/date/tests/date_sunrise_and_sunset_basic.phpt (shape only)
#[test]
fn basic_shapes() {
    let r = s(sun::sunrise(1_700_000_000, RET_STRING, 38.4, -9.0, 90.0, 1.0, 0).unwrap());
    assert_eq!(r.len(), 5);
    assert_eq!(&r[2..3], ":");
    let r = sun::sunset(1_700_000_000, RET_STRING, 38.4, -9.0, 90.0, 1.0, 0).unwrap();
    assert!(matches!(r, SunResult::String(_)));
}

// test ext/date/tests/sunfuncts.phpt (Asia/Jerusalem, mktime(1,1,1,$a,1,2003))
#[test]
fn sunfuncts() {
    let want = [
        (
            1041395858,
            "06:37",
            "6.6274307083",
            1041432434,
            "16:47",
            "16.787476435",
        ),
        (
            1044073871,
            "06:31",
            "6.5198435465",
            1044112440,
            "17:14",
            "17.233560530",
        ),
        (
            1046491524,
            "06:05",
            "6.0901858662",
            1046533056,
            "17:37",
            "17.626724279",
        ),
        (
            1049167610,
            "05:26",
            "5.4473483216",
            1049212759,
            "17:59",
            "17.988854003",
        ),
        (
            1051757554,
            "04:52",
            "4.8761196793",
            1051805991,
            "18:19",
            "18.330987470",
        ),
        (
            1054434782,
            "04:33",
            "4.5506171719",
            1054485634,
            "18:40",
            "18.676282498",
        ),
        (
            1057026940,
            "04:35",
            "4.5946862151",
            1057078197,
            "18:49",
            "18.832651294",
        ),
        (
            1059706395,
            "04:53",
            "4.8875657219",
            1059755854,
            "18:37",
            "18.626282855",
        ),
        (
            1062385985,
            "05:13",
            "5.2182752537",
            1062432319,
            "18:05",
            "18.088728929",
        ),
        (
            1064979083,
            "05:31",
            "5.5233314120",
            1065021981,
            "17:26",
            "17.439368589",
        ),
        (
            1067658825,
            "05:53",
            "5.8959513887",
            1067698295,
            "16:51",
            "16.859972460",
        ),
        (
            1070252367,
            "06:19",
            "6.3241991396",
            1070289384,
            "16:36",
            "16.606870705",
        ),
    ];
    for (a, w) in want.iter().enumerate() {
        let month = a as i64 + 1;
        // IST +2, IDT +3 from 2003-03-28 to 2003-10-03
        let off = if (4..=10).contains(&month) {
            10800
        } else {
            7200
        };
        let ts = civil_to_ts(2003, month, 1, 1, 1, 1) - off;
        let call = |f: SunFn, fmt| f(ts, fmt, 31.7667, 35.2333, 90.83, 2.0, off).unwrap();
        for (f, ts_w, str_w, dbl_w) in [
            (sun::sunrise as SunFn, w.0, w.1, w.2),
            (sun::sunset, w.3, w.4, w.5),
        ] {
            assert_eq!(
                call(f, RET_TIMESTAMP),
                SunResult::Timestamp(ts_w),
                "month {month}"
            );
            assert_eq!(s(call(f, RET_STRING)), str_w, "month {month}");
            let SunResult::Double(d) = call(f, RET_DOUBLE) else {
                panic!("double expected")
            };
            // precision=13 output with a %d tail
            let shown = format!("{d:.15}");
            assert!(
                shown.starts_with(dbl_w),
                "month {month}: {shown} vs {dbl_w}"
            );
        }
    }
}

// tests ext/date/tests/date_sunrise_variation7.phpt, date_sunset_variation7.phpt
#[test]
fn variation7() {
    let places: [(f64, f64, f64, [&str; 4]); 9] = [
        (-14.24, -170.72, -11.0, ["06:42", "06:41", "18:13", "18:13"]),
        (61.0, -150.0, -9.0, ["05:07", "05:09", "21:02", "20:59"]),
        (41.85, -87.65, -5.0, ["05:58", "05:59", "19:52", "19:51"]),
        (-34.88, -56.18, -3.0, ["07:31", "07:30", "18:08", "18:08"]),
        (33.65, -7.58, 0.0, ["05:52", "05:53", "19:18", "19:17"]),
        (55.75, 37.58, 4.0, ["05:59", "06:01", "21:10", "21:08"]),
        (22.28, 114.15, 8.0, ["06:01", "06:02", "18:55", "18:54"]),
        (-27.46, 153.2, 10.0, ["06:23", "06:22", "17:21", "17:21"]),
        (-13.3, -176.16, 12.0, ["06:03", "06:02", "17:36", "17:36"]),
    ];
    for (lat, lon, gmt, w) in places {
        // only the local date (Aug 11 / 12) matters
        let d11 = civil_to_ts(2008, 8, 11, 8, 8, 8);
        let d12 = civil_to_ts(2008, 8, 12, 8, 8, 8);
        let got = [
            s(sun::sunrise(d11, RET_STRING, lat, lon, 90.0, gmt, 0).unwrap()),
            s(sun::sunrise(d12, RET_STRING, lat, lon, 90.0, gmt, 0).unwrap()),
            s(sun::sunset(d11, RET_STRING, lat, lon, 90.0, gmt, 0).unwrap()),
            s(sun::sunset(d12, RET_STRING, lat, lon, 90.0, gmt, 0).unwrap()),
        ];
        assert_eq!(got, w, "{lat} {lon}");
    }
}

// tests ext/date/tests/date_sunrise_variation8.phpt, date_sunset_variation8.phpt
#[test]
fn variation8_poles() {
    for (m, d) in [(8, 12), (8, 13), (11, 12), (11, 13)] {
        let ts = civil_to_ts(2008, m, d, 8, 8, 8);
        for f in [RET_STRING, RET_DOUBLE, RET_TIMESTAMP] {
            for lat in [90.0, -90.0] {
                let r = sun::sunrise(ts, f, lat, 0.0, DEFAULT_ZENITH, 0.0, 0);
                assert_eq!(r, Ok(SunResult::False));
                let r = sun::sunset(ts, f, lat, 0.0, DEFAULT_ZENITH, 0.0, 0);
                assert_eq!(r, Ok(SunResult::False));
            }
        }
    }
}

// test ext/date/tests/sunfuncts_partial_hour_utc_offset.phpt (Asia/Kolkata)
#[test]
fn partial_hour_offset() {
    let ts = civil_to_ts(2025, 2, 9, 0, 0, 0) - 19800;
    let off = default_utc_offset_hours(19800);
    let r = sun::sunrise(ts, RET_STRING, 22.57, 88.36, DEFAULT_ZENITH, off, 19800).unwrap();
    assert_eq!(s(r), "06:10");
    let i = info(ts, 22.57, 88.36, 19800);
    assert_eq!(&hms(at(i.sunrise), 19800)[..5], "06:11");
}

// observed on PHP 8.5: the default $utcOffset is the zone's offset at Unix
// time 0, not at $timestamp; the date is the local date at $timestamp
#[test]
fn default_offset_is_offset_at_epoch() {
    // (ts, offset at ts, offset at epoch, sunrise double at 10N 20E)
    let cases = [
        (1590992100, 20700, 19800, 9.777193362020226), // Asia/Kathmandu
        (1577829600, 50400, -38400, 18.260105556749807), // Pacific/Kiritimati
        (1593613800, -9000, -12600, 0.8650426511179647), // America/St_Johns
        (-631106130, -2670, -2670, 4.187870417984415), // Africa/Monrovia
        (1590981300, 31500, 31500, 13.027193362020226), // Australia/Eucla
    ];
    for (ts, at_ts, at_epoch, want) in cases {
        let off = default_utc_offset_hours(at_epoch);
        let r = sun::sunrise(ts, RET_DOUBLE, 10.0, 20.0, DEFAULT_ZENITH, off, at_ts).unwrap();
        assert_eq!(r, SunResult::Double(want), "{ts}");
    }
    // Pacific/Kiritimati 2020-01-01 05:00 local: still January 1st
    let r = sun::sunrise(
        1577804400,
        RET_TIMESTAMP,
        10.0,
        20.0,
        DEFAULT_ZENITH,
        0.0,
        50400,
    );
    assert_eq!(r, Ok(SunResult::Timestamp(1577854536)));
    assert_eq!(at(info(1577804400, 10.0, 20.0, 50400).transit), 1577875398);
}

// --- the PHP 8.5 batch ------------------------------------------------------

fn parse_f(s: &str) -> f64 {
    s.parse().unwrap_or_else(|_| panic!("bad float {s}"))
}

fn sun_time_str(t: SunTime) -> String {
    match t {
        SunTime::At(ts) => ts.to_string(),
        SunTime::AlwaysAbove => "T".into(),
        SunTime::AlwaysBelow => "F".into(),
    }
}

/// Whether `got` is what PHP printed as `want` (doubles compared bit for bit).
fn result_matches(got: &SunResult, want: &str) -> bool {
    match got {
        SunResult::False => want == "F",
        SunResult::Timestamp(t) => want == t.to_string(),
        SunResult::String(s) => want == format!("'{s}'"),
        SunResult::Double(d) => want
            .parse::<f64>()
            .is_ok_and(|w| w.to_bits() == d.to_bits()),
    }
}

#[test]
fn php85_batch() {
    let data = include_str!("fixtures/sun_php85.txt");
    let (mut n, mut bad) = (0, Vec::new());
    for line in data.lines().filter(|l| !l.starts_with('#')) {
        let (head, want) = line.split_once(" | ").unwrap();
        let f: Vec<&str> = head.split_whitespace().collect();
        let want: Vec<&str> = want.split_whitespace().collect();
        match f[0] {
            "I" => {
                let i = sun::sun_info(
                    f[1].parse().unwrap(),
                    parse_f(f[3]),
                    parse_f(f[4]),
                    f[2].parse().unwrap(),
                )
                .unwrap();
                let got: Vec<String> = i.entries().iter().map(|(_, t)| sun_time_str(*t)).collect();
                n += 1;
                if got != want {
                    bad.push(format!("{line}\n    got {}", got.join(" ")));
                }
            }
            "R" | "G" => {
                let (calls, rest): (Vec<(bool, i64)>, &[&str]) = if f[0] == "R" {
                    (
                        vec![(f[1] == "rise", f[3].parse().unwrap())],
                        &[f[2], f[4], f[5], f[6], f[7], f[8], f[9]],
                    )
                } else {
                    let all = [0, 1, 2]
                        .iter()
                        .flat_map(|&fmt| [(true, fmt), (false, fmt)])
                        .collect();
                    (all, &f[1..8])
                };
                let ts: i64 = rest[0].parse().unwrap();
                let (lat, lon, zen) = (parse_f(rest[1]), parse_f(rest[2]), parse_f(rest[3]));
                let local: i64 = rest[5].parse().unwrap();
                let off = if rest[4] == "D" {
                    default_utc_offset_hours(rest[6].parse().unwrap())
                } else {
                    parse_f(rest[4])
                };
                for ((rise, fmt), w) in calls.iter().zip(&want) {
                    let f: SunFn = if *rise { sun::sunrise } else { sun::sunset };
                    let got = f(ts, *fmt, lat, lon, zen, off, local).unwrap();
                    n += 1;
                    if !result_matches(&got, w) {
                        bad.push(format!("{line}\n    [{rise} {fmt}] got {got:?} want {w}"));
                    }
                }
            }
            other => panic!("bad fixture line kind {other}"),
        }
    }
    assert!(n > 3700, "only {n} cases");
    assert!(
        bad.is_empty(),
        "{} of {n} differ:\n{}",
        bad.len(),
        bad[..bad.len().min(20)].join("\n")
    );
}
