//! DateTimeZone::getTransitions() for every zone against PHP 8.5
//! (php85_transitions_{all,tail}.php, condensed to counts and CRC-32 by
//! tools/date_corpus/compact_transitions.py): the full list up to 2038 and
//! the 2038-2106 range generated from each zone's ongoing rules.

use goro_ext_date::api;
use goro_ext_date::tz::{TimeZone, ZoneId};

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

#[test]
fn php85_transitions() {
    let data = include_str!("fixtures/php85_transitions.txt");
    let mut checked = 0;
    for line in data.lines() {
        let c: Vec<&str> = line.split('\t').collect();
        let (tag, name, count, crc) = (c[0], c[1], c[2], c[3]);
        if ZoneId::find(name).is_none() {
            // files of the reference system's zoneinfo directory that are
            // not zones ("localtime", "posixrules")
            continue;
        }
        let zone = TimeZone::from_id(name).unwrap();
        let (begin, end) = if tag == "all" {
            (i64::MIN, api::TRANSITIONS_DEFAULT_END)
        } else {
            (2_147_483_647, 4_300_000_000)
        };
        let t = api::transitions(&zone, begin, end).unwrap();
        let lines: Vec<String> = t
            .iter()
            .map(|x| format!("{} {} {} {}", x.ts, x.offset, x.isdst as u8, x.abbr))
            .collect();
        let got = format!("{:08x}", crc32(lines.join("\n").as_bytes()));
        assert_eq!(
            (lines.len().to_string(), got.as_str()),
            (count.to_string(), crc),
            "{tag} {name}"
        );
        checked += 1;
    }
    assert!(checked > 900, "{checked}");
}
