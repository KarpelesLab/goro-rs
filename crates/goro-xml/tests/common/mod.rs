#![allow(dead_code)]
//! Helpers for fixture tests.

/// Decodes standard base64.
pub fn b64(s: &str) -> Vec<u8> {
    let mut out = Vec::new();
    let mut acc = 0u32;
    let mut bits = 0;
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => continue,
        } as u32;
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    out
}

/// Reads a fixture file next to this crate's tests.
pub fn fixture(name: &str) -> String {
    let p = format!("{}/tests/fixtures/{}", env!("CARGO_MANIFEST_DIR"), name);
    std::fs::read_to_string(p).expect("fixture")
}

/// Shows bytes readably.
pub fn show(b: &[u8]) -> String {
    let s = String::from_utf8_lossy(b);
    s.replace('\n', "\\n")
}
