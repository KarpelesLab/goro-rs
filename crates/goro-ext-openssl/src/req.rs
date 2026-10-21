//! `struct php_x509_request` and php_openssl_parse_config().

use crate::cipher::{self, CipherSpec};
use crate::conf::{self, Conf};
use crate::digest::Md;
use crate::err;
use goro_core::array::{ArrayKey, PhpArray};
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::Vm;

pub const OPENSSL_KEYTYPE_RSA: i64 = 0;
pub const OPENSSL_KEYTYPE_DSA: i64 = 1;
pub const OPENSSL_KEYTYPE_DH: i64 = 2;
pub const OPENSSL_KEYTYPE_EC: i64 = 3;
pub const OPENSSL_KEYTYPE_X25519: i64 = 4;
pub const OPENSSL_KEYTYPE_ED25519: i64 = 5;
pub const OPENSSL_KEYTYPE_X448: i64 = 6;
pub const OPENSSL_KEYTYPE_ED448: i64 = 7;

pub struct Req {
    pub conf: Conf,
    pub config_filename: String,
    pub section: String,
    /// Message digest; `None` with `md_null` = the "null" digest.
    pub md: Option<Md>,
    pub md_null: bool,
    pub extensions_section: Option<String>,
    pub request_extensions_section: Option<String>,
    pub priv_key_bits: i64,
    pub priv_key_type: i64,
    pub priv_key_encrypt: bool,
    pub priv_key_encrypt_cipher: Option<&'static CipherSpec>,
    pub curve_name: Option<String>,
    /// ASN1 string mask (B_ASN1_* bits) for DN entries.
    pub string_mask: u64,
}

pub fn opt<'a>(args: Option<&'a PhpArray>, key: &str) -> Option<&'a Value> {
    args?.get(&ArrayKey::String(PhpString::from_bytes(key.as_bytes())))
}

fn opt_str(args: Option<&PhpArray>, key: &str) -> Option<String> {
    match opt(args, key) {
        Some(Value::String(s)) => Some(s.to_string_lossy()),
        _ => None,
    }
}

fn opt_long(args: Option<&PhpArray>, key: &str) -> Option<i64> {
    match opt(args, key) {
        Some(Value::Long(l)) => Some(*l),
        _ => None,
    }
}

/// B_ASN1 masks used by ASN1_STRING_set_default_mask_asc().
pub const B_ASN1_PRINTABLESTRING: u64 = 0x0002;
pub const B_ASN1_T61STRING: u64 = 0x0004;
pub const B_ASN1_IA5STRING: u64 = 0x0010;
pub const B_ASN1_UNIVERSALSTRING: u64 = 0x0100;
pub const B_ASN1_BMPSTRING: u64 = 0x0800;
pub const B_ASN1_UTF8STRING: u64 = 0x2000;

fn parse_string_mask(s: &str) -> Option<u64> {
    if let Some(n) = s.strip_prefix("MASK:") {
        let v = n.parse::<u64>().ok()?;
        return Some(v);
    }
    Some(match s {
        "nombstr" => !(B_ASN1_BMPSTRING | B_ASN1_UTF8STRING) & 0xFFFF_FFFF,
        "pkix" => !B_ASN1_T61STRING & 0xFFFF_FFFF,
        "utf8only" => B_ASN1_UTF8STRING,
        "default" => 0xFFFF_FFFF,
        _ => return None,
    })
}

/// Default string mask (OpenSSL 3: "utf8only").
pub const DEFAULT_STRING_MASK: u64 = B_ASN1_UTF8STRING;

thread_local! {
    static STRING_MASK: std::cell::Cell<u64> = const { std::cell::Cell::new(DEFAULT_STRING_MASK) };
}

/// The global ASN1 string mask (set as a side effect of config parsing).
pub fn global_string_mask() -> u64 {
    STRING_MASK.with(|m| m.get())
}

/// php_openssl_parse_config(). Emits the PHP warnings itself; returns None
/// on FAILURE.
pub fn parse(vm: &mut Vm, args: Option<&PhpArray>) -> Option<Req> {
    let default_file = conf::default_conf_filename();
    let config_filename = opt_str(args, "config").unwrap_or_else(|| default_file.clone());
    let section = opt_str(args, "config_section_name").unwrap_or_else(|| "req".to_string());
    if Conf::load_file(&default_file).is_none() {
        err::push(err::LIB_CONF, 114, "no such file");
    }
    let conf = Conf::load_file(&config_filename)?;

    // oid_file
    if let Some(f) = conf.get_string(None, "oid_file") {
        let fname = crate::zpp::current_fn();
        let checked =
            crate::x509::check_plain_path(vm, fname, 0, f.as_bytes(), false, Some("oid_file"))
                .ok()
                .flatten();
        if let Some(Ok(data)) = checked.map(std::fs::read) {
            for line in String::from_utf8_lossy(&data).lines() {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 2 {
                    crate::obj::create(
                        parts[0],
                        parts[1],
                        parts.get(2).copied().unwrap_or(parts[1]),
                    );
                }
            }
        }
    }
    // oid_section
    if let Some(sname) = conf.get_string(None, "oid_section") {
        match conf.section(&sname) {
            None => {
                crate::zpp::warn(vm, &format!("Problem loading oid section {}", sname));
                return None;
            }
            Some(items) => {
                for (name, value) in items.clone() {
                    if crate::obj::by_sn(&name).is_none()
                        && crate::obj::by_ln(&name).is_none()
                        && !crate::obj::create(value.trim(), &name, &name)
                    {
                        crate::zpp::warn(
                            vm,
                            &format!("Problem creating object {}={}", name, value),
                        );
                        return None;
                    }
                }
            }
        }
    }
    let digest_name =
        opt_str(args, "digest_alg").or_else(|| conf.get_string(Some(&section), "default_md"));
    let extensions_section = opt_str(args, "x509_extensions")
        .or_else(|| conf.get_string(Some(&section), "x509_extensions"));
    let request_extensions_section = opt_str(args, "req_extensions")
        .or_else(|| conf.get_string(Some(&section), "req_extensions"));
    let priv_key_bits = opt_long(args, "private_key_bits")
        .unwrap_or_else(|| conf.get_number(Some(&section), "default_bits"));
    let priv_key_type = opt_long(args, "private_key_type").unwrap_or(OPENSSL_KEYTYPE_RSA);

    let priv_key_encrypt = match opt(args, "encrypt_key") {
        Some(v) => matches!(v, Value::True),
        None => {
            let s = conf
                .get_string(Some(&section), "encrypt_rsa_key")
                .or_else(|| conf.get_string(Some(&section), "encrypt_key"));
            !matches!(s.as_deref(), Some("no"))
        }
    };
    let mut priv_key_encrypt_cipher = None;
    if priv_key_encrypt {
        if let Some(Value::Long(algo)) = opt(args, "encrypt_key_cipher") {
            match cipher_from_algo(*algo) {
                Some(c) => priv_key_encrypt_cipher = Some(c),
                None => {
                    crate::zpp::warn(vm, "Unknown cipher algorithm for private key");
                    return None;
                }
            }
        }
    }
    let mut md = None;
    let mut md_null = false;
    if let Some(dn) = &digest_name {
        if dn == "null" {
            md_null = true;
        } else {
            md = Md::by_name(dn);
        }
    }
    if md.is_none() && !md_null {
        md = Some(Md::Sha1);
    }

    let mut req = Req {
        conf,
        config_filename,
        section,
        md,
        md_null,
        extensions_section,
        request_extensions_section,
        priv_key_bits,
        priv_key_type,
        priv_key_encrypt,
        priv_key_encrypt_cipher,
        curve_name: None,
        string_mask: global_string_mask(),
    };
    if let Some(sec) = req.extensions_section.clone() {
        if !crate::x509v3::check_section_syntax(&req.conf, &sec) {
            crate::zpp::warn(
                vm,
                &format!(
                    "Error loading extensions_section section {} of {}",
                    sec, req.config_filename
                ),
            );
            return None;
        }
    }
    if let Some(Value::String(s)) = opt(args, "curve_name") {
        let name = s.to_string_lossy();
        if crate::ec::Group::by_name(&name).is_none() {
            crate::zpp::warn(vm, &format!("Unknown elliptic curve (short) name {}", name));
            return None;
        }
        req.curve_name = Some(name);
    }
    if let Some(sm) = req.conf.get_string(Some(&req.section), "string_mask") {
        match parse_string_mask(&sm) {
            Some(m) => {
                STRING_MASK.with(|g| g.set(m));
                req.string_mask = m;
            }
            None => {
                crate::zpp::warn(vm, &format!("Invalid global string mask setting {}", sm));
                return None;
            }
        }
    }
    if let Some(sec) = req.request_extensions_section.clone() {
        if !crate::x509v3::check_section_syntax(&req.conf, &sec) {
            crate::zpp::warn(
                vm,
                &format!(
                    "Error loading request_extensions_section section {} of {}",
                    sec, req.config_filename
                ),
            );
            return None;
        }
    }
    Some(req)
}

/// php_openssl_get_evp_cipher_from_algo()
pub fn cipher_from_algo(algo: i64) -> Option<&'static CipherSpec> {
    let name = match algo {
        0 => "rc2-40-cbc",
        1 => "rc2-cbc",
        2 => "rc2-64-cbc",
        3 => "des-cbc",
        4 => "des-ede3-cbc",
        5 => "aes-128-cbc",
        6 => "aes-192-cbc",
        7 => "aes-256-cbc",
        _ => return None,
    };
    cipher::by_name(name)
}
