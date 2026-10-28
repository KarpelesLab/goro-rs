//! Helpers shared by the fixture tests.
#![allow(dead_code)]

use goro_ext_date::parse::DateParse;

/// Decodes a hex string.
pub fn unhex(s: &str) -> Vec<u8> {
    (0..s.len() / 2)
        .map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap())
        .collect()
}

/// The canonical one-line form of a `date_parse()` result used by the
/// fixtures (see tools/date_fixture_canon.py).
pub fn canon(p: &DateParse) -> String {
    let o = |v: Option<i64>| v.map(|v| v.to_string()).unwrap_or_else(|| "F".into());
    let mut f = vec![
        o(p.year),
        o(p.month),
        o(p.day),
        o(p.hour),
        o(p.minute),
        o(p.second),
        o(p.fraction_us),
    ];
    let msgs = |l: &[(usize, String)]| {
        l.iter()
            .map(|(a, b)| format!("{a}:{b}"))
            .collect::<Vec<_>>()
            .join(";")
    };
    f.push(msgs(&p.warnings));
    f.push(msgs(&p.errors));
    f.push(if p.is_localtime { "1" } else { "0" }.into());
    f.push(
        p.zone_type
            .map(|v| v.to_string())
            .unwrap_or_else(|| "-".into()),
    );
    f.push(p.zone.map(|v| v.to_string()).unwrap_or_else(|| "-".into()));
    f.push(
        p.is_dst
            .map(|v| if v { "1" } else { "0" }.to_string())
            .unwrap_or_else(|| "-".into()),
    );
    f.push(p.tz_abbr.clone().unwrap_or_else(|| "-".into()));
    f.push(p.tz_id.clone().unwrap_or_else(|| "-".into()));
    match &p.relative {
        None => f.push("-".into()),
        Some(r) => {
            let mut parts: Vec<String> = r.ymdhis.iter().map(|v| v.to_string()).collect();
            if let Some(w) = r.weekday {
                parts.push(format!("wd={w}"));
            }
            if let Some(w) = r.weekdays {
                parts.push(format!("wds={w}"));
            }
            if r.first_day_of_month {
                parts.push("fdm".into());
            }
            if r.last_day_of_month {
                parts.push("ldm".into());
            }
            f.push(parts.join(","));
        }
    }
    f.join("|")
}
