//! EXSLT crypto (libexslt/crypto.c, the libgcrypt flavour)

use crate::transform::{State, Transform};
use crate::xpath::{self, PCtx, XObj};

const RC4_KEY_LENGTH: usize = 128;

pub fn exists(name: &str) -> bool {
    matches!(name, "md4" | "md5" | "sha1" | "rc4_encrypt" | "rc4_decrypt")
}

pub fn call(t: &mut Transform, p: &mut PCtx, name: &str, nargs: usize) -> bool {
    match name {
        "md4" | "md5" | "sha1" => hash(t, p, name, nargs),
        "rc4_encrypt" => rc4(t, p, nargs, true),
        "rc4_decrypt" => rc4(t, p, nargs, false),
        _ => return false,
    }
    true
}

fn bin2hex(b: &[u8]) -> String {
    let mut s = String::with_capacity(b.len() * 2);
    for c in b {
        s.push_str(&format!("{:02x}", c));
    }
    s
}

/// exsltCryptoHex2Bin
fn hex2bin(h: &[u8]) -> Vec<u8> {
    let v = |c: u8| match c {
        b'0'..=b'9' => c - b'0',
        b'a'..=b'f' => 10 + c - b'a',
        _ => 0,
    };
    let mut out = Vec::new();
    let mut i = 0;
    while i < h.len() {
        let hi = v(h[i]);
        let lo = h.get(i + 1).map(|&c| v(c)).unwrap_or(0);
        out.push((hi << 4).wrapping_add(lo));
        i += 2;
    }
    out
}

/// exsltCryptoPopString + the hash functions
fn hash(t: &mut Transform, p: &mut PCtx, name: &str, nargs: usize) {
    if !(1..=2).contains(&nargs) {
        xpath::x_arity_error(t, p);
        return;
    }
    let s = xpath::x_pop_string(t, p).unwrap_or_default();
    if s.is_empty() {
        p.push(XObj::Str(String::new()));
        return;
    }
    let d = goro_ext_hash::digest(name, s.as_bytes()).unwrap_or_default();
    p.push(XObj::Str(bin2hex(&d)));
}

fn rc4_apply(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut st: [u8; 256] = [0; 256];
    for (i, x) in st.iter_mut().enumerate() {
        *x = i as u8;
    }
    let mut j: u8 = 0;
    for i in 0..256 {
        j = j.wrapping_add(st[i]).wrapping_add(key[i % key.len()]);
        st.swap(i, j as usize);
    }
    let (mut i, mut j) = (0u8, 0u8);
    data.iter()
        .map(|&b| {
            i = i.wrapping_add(1);
            j = j.wrapping_add(st[i as usize]);
            st.swap(i as usize, j as usize);
            b ^ st[(st[i as usize].wrapping_add(st[j as usize])) as usize]
        })
        .collect()
}

fn rc4(t: &mut Transform, p: &mut PCtx, nargs: usize, encrypt: bool) {
    if nargs != 2 {
        xpath::x_arity_error(t, p);
        return;
    }
    let s = xpath::x_pop_string(t, p).unwrap_or_default();
    if s.is_empty() {
        p.push(XObj::Str(String::new()));
        return;
    }
    let key = xpath::x_pop_string(t, p).unwrap_or_default();
    if key.is_empty() {
        p.push(XObj::Str(String::new()));
        return;
    }
    let inst = t.inst;
    if key.len() > RC4_KEY_LENGTH {
        t.error(
            inst,
            "exsltCryptoRc4EncryptFunction: key size too long or key broken\n",
        );
        t.state = State::Stopped;
        p.push(XObj::Str(String::new()));
        return;
    }
    let mut padkey = vec![0u8; RC4_KEY_LENGTH];
    padkey[..key.len()].copy_from_slice(key.as_bytes());
    if encrypt {
        let bin = rc4_apply(&padkey, s.as_bytes());
        p.push(XObj::Str(bin2hex(&bin)));
    } else {
        let bin = hex2bin(s.as_bytes());
        let ret = rc4_apply(&padkey, &bin);
        let ret = match ret.iter().position(|&c| c == 0) {
            Some(i) => ret[..i].to_vec(),
            None => ret,
        };
        match String::from_utf8(ret) {
            Ok(r) => p.push(XObj::Str(r)),
            Err(_) => {
                t.error(inst, "exsltCryptoRc4DecryptFunction: Invalid UTF-8\n");
                p.push(XObj::Str(String::new()));
            }
        }
    }
}
