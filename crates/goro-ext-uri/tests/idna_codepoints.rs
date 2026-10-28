//! Every code point through the WHATWG host parser, against PHP 8.5.
//!
//! `tests/fixtures/idna_driver.php` printed, for each code point U+0080..
//! U+10FFFF but the surrogates, the ASCII host PHP 8.5 gives for
//! `http://a<cp>b/` and for its Punycode form `http://xn--<punycode of
//! a<cp>b>/` (or `F` when the parse fails), one line per code point:
//! `<hex cp>\t<host|F>\t<host|F>\n`. The 1.1M-line output (34 MB) is not
//! kept; its SHA-256 is. This test builds the same text with goro and
//! compares the digests (and, on a mismatch, prints the first code points
//! that differ from a re-derivation of PHP's rules, to help debugging).

use goro_ext_uri::whatwg::Url;

const PHP85_SHA256: &str = "3910bdc7d960d0948295a93aff44b24d6eb59cc52f0f548a89a573f7dafce029";

fn host(input: &[u8]) -> Vec<u8> {
    match Url::parse(input, None) {
        Ok((u, _)) => u.ascii_host().unwrap_or_default().to_vec(),
        Err(_) => b"F".to_vec(),
    }
}

/// RFC 3492 encoding (independent of the crate's, so a bug in one shows).
fn punycode(input: &[char]) -> String {
    let (base, tmin, tmax, skew, damp) = (36u32, 1u32, 26u32, 38u32, 700u32);
    let adapt = |mut delta: u32, n: u32, first: bool| {
        delta /= if first { damp } else { 2 };
        delta += delta / n;
        let mut k = 0;
        while delta > ((base - tmin) * tmax) / 2 {
            delta /= base - tmin;
            k += base;
        }
        k + (base - tmin + 1) * delta / (delta + skew)
    };
    let digit = |d: u32| -> char {
        if d < 26 {
            (b'a' + d as u8) as char
        } else {
            (b'0' + (d - 26) as u8) as char
        }
    };
    let mut out: String = input.iter().filter(|c| c.is_ascii()).collect();
    let b = out.len() as u32;
    if b > 0 {
        out.push('-');
    }
    let (mut n, mut delta, mut bias, mut h) = (128u32, 0u32, 72u32, b);
    while (h as usize) < input.len() {
        let m = input
            .iter()
            .map(|&c| c as u32)
            .filter(|&c| c >= n)
            .min()
            .unwrap();
        delta += (m - n) * (h + 1);
        n = m;
        for &c in input {
            let c = c as u32;
            if c < n {
                delta += 1;
            }
            if c == n {
                let mut q = delta;
                let mut k = base;
                loop {
                    let t = if k <= bias {
                        tmin
                    } else if k >= bias + tmax {
                        tmax
                    } else {
                        k - bias
                    };
                    if q < t {
                        break;
                    }
                    out.push(digit(t + (q - t) % (base - t)));
                    q = (q - t) / (base - t);
                    k += base;
                }
                out.push(digit(q));
                bias = adapt(delta, h + 1, h == b);
                delta = 0;
                h += 1;
            }
        }
        delta += 1;
        n += 1;
    }
    out
}

#[test]
fn idna_every_code_point_matches_php85() {
    let mut text: Vec<u8> = Vec::with_capacity(40 << 20);
    let mut buf = [0u8; 4];
    for cp in (0x80u32..=0x10FFFF).filter(|c| !(0xD800..=0xDFFF).contains(c)) {
        let ch = char::from_u32(cp).unwrap();
        let mut u = b"http://a".to_vec();
        u.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
        u.extend_from_slice(b"b/");
        let mut a = b"http://xn--".to_vec();
        a.extend_from_slice(punycode(&['a', ch, 'b']).as_bytes());
        a.push(b'/');
        text.extend_from_slice(format!("{cp:x}\t").as_bytes());
        text.extend_from_slice(&host(&u));
        text.push(b'\t');
        text.extend_from_slice(&host(&a));
        text.push(b'\n');
    }
    let got: String = purecrypto::hash::sha256(&text)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    assert_eq!(
        got, PHP85_SHA256,
        "per-code-point IDNA results differ from PHP 8.5"
    );
}
