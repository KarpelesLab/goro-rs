//! DateTimeZone::getLocation() and listIdentifiers() for every zone.tab
//! name against PHP 8.5 (fixtures/php85_locations.php output).

use goro_ext_date::TimeZone;
use goro_ext_date::api::{self, groups};

#[test]
fn php85_locations() {
    let data = include_str!("fixtures/php85_locations.txt");
    let mut n = 0;
    for line in data.lines() {
        let c: Vec<&str> = line.split('\t').collect();
        match c[0] {
            "L" => {
                let l = TimeZone::from_id(c[1]).unwrap().location().unwrap();
                // var_export() prints integral floats with ".0"
                let f = |v: f64| {
                    if v.fract() == 0.0 {
                        format!("{v:.1}")
                    } else {
                        format!("{v}")
                    }
                };
                let got = format!(
                    "{}\t{}\t{}\t{}",
                    l.country_code,
                    f(l.latitude),
                    f(l.longitude),
                    l.comments
                );
                assert_eq!(got, c[2..].join("\t"), "{}", c[1]);
                n += 1;
            }
            "ALL" => {
                assert_eq!(api::list_identifiers(groups::ALL, None).join(","), c[1]);
            }
            _ => {}
        }
    }
    assert_eq!(n, 419);
}
