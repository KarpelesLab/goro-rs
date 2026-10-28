//! A debugging aid, not a test: prints how single code points encode, in the
//! format of tools/charset/fixtures/probe.php, for diffing against php85.
//!
//! `GORO_CHARSET_PROBE="ENC:LO-HI[,ENC:LO-HI...]" cargo test --test probe -- --ignored --nocapture`

use goro_charset::{Encoding, Subst, convert};

#[test]
#[ignore]
fn probe() {
    let Ok(spec) = std::env::var("GORO_CHARSET_PROBE") else {
        return;
    };
    let sub = Subst::char(0x25);
    for item in spec.split(',') {
        let (name, range) = item.rsplit_once(':').unwrap();
        let (lo, hi) = range.split_once('-').unwrap();
        let lo = u32::from_str_radix(lo, 16).unwrap();
        let hi = u32::from_str_radix(hi, 16).unwrap();
        let enc = *goro_charset::encoding::ALL
            .iter()
            .find(|e| e.name() == name)
            .unwrap();
        for cp in lo..=hi {
            let c = convert(&cp.to_be_bytes(), enc, Encoding::Utf32Be, &sub);
            let hex: String = c.bytes.iter().map(|b| format!("{b:02x}")).collect();
            println!("{name} {cp:04x} {} {hex}", c.illegal);
        }
    }
}
