//! OpenSSL-style error queue (`ERR_*` + php-src's `php_openssl_store_errors`).
//!
//! Errors are formatted the way OpenSSL 3.0's `ERR_error_string_n()` does:
//! `error:XXXXXXXX:<lib string>::<reason string>` where the code is
//! `(lib << 23) | reason`.

use std::cell::RefCell;
use std::collections::VecDeque;

pub const LIB_SYS: u32 = 2;
pub const LIB_BN: u32 = 3;
pub const LIB_RSA: u32 = 4;
pub const LIB_DH: u32 = 5;
pub const LIB_EVP: u32 = 6;
pub const LIB_OBJ: u32 = 8;
pub const LIB_PEM: u32 = 9;
pub const LIB_DSA: u32 = 10;
pub const LIB_X509: u32 = 11;
pub const LIB_ASN1: u32 = 13;
pub const LIB_CONF: u32 = 14;
pub const LIB_CRYPTO: u32 = 15;
pub const LIB_EC: u32 = 16;
pub const LIB_BIO: u32 = 32;
pub const LIB_PKCS7: u32 = 33;
pub const LIB_X509V3: u32 = 34;
pub const LIB_PKCS12: u32 = 35;
pub const LIB_CMS: u32 = 46;
pub const LIB_PROV: u32 = 57;
pub const LIB_DECODER: u32 = 60;

/// `ERR_RFLAG_COMMON`: reasons shared across libraries.
pub const RFLAG_COMMON: u32 = 0x2 << 18;
pub const R_SYS_LIB: u32 = LIB_SYS | RFLAG_COMMON;
pub const R_UNSUPPORTED: u32 = 0x10C | RFLAG_COMMON;

fn lib_string(lib: u32) -> &'static str {
    match lib {
        LIB_SYS => "system library",
        LIB_BN => "bignum routines",
        LIB_RSA => "rsa routines",
        LIB_DH => "Diffie-Hellman routines",
        LIB_EVP => "digital envelope routines",
        LIB_OBJ => "object identifier routines",
        LIB_PEM => "PEM routines",
        LIB_DSA => "dsa routines",
        LIB_X509 => "x509 certificate routines",
        LIB_ASN1 => "asn1 encoding routines",
        LIB_CONF => "configuration file routines",
        LIB_CRYPTO => "common libcrypto routines",
        LIB_EC => "elliptic curve routines",
        LIB_BIO => "BIO routines",
        LIB_PKCS7 => "PKCS7 routines",
        LIB_X509V3 => "X509 V3 routines",
        LIB_PKCS12 => "PKCS12 routines",
        LIB_CMS => "CMS routines",
        LIB_PROV => "Provider routines",
        LIB_DECODER => "DECODER routines",
        _ => "unknown library",
    }
}

thread_local! {
    static QUEUE: RefCell<VecDeque<String>> = const { RefCell::new(VecDeque::new()) };
}

/// php-src keeps at most `ERR_NUM_ERRORS - 1` (15) errors.
const MAX_ERRORS: usize = 15;

/// Push a preformatted error string.
pub fn push_str(msg: impl Into<String>) {
    QUEUE.with(|q| {
        let mut q = q.borrow_mut();
        if q.len() >= MAX_ERRORS {
            q.pop_front();
        }
        q.push_back(msg.into());
    });
}

/// Push an error with an OpenSSL library and reason code.
pub fn push(lib: u32, reason: u32, reason_str: &str) {
    let code = (lib << 23) | (reason & 0x7F_FFFF);
    push_str(format!(
        "error:{:08X}:{}::{}",
        code,
        lib_string(lib),
        reason_str
    ));
}

/// Pop the oldest error (openssl_error_string()).
pub fn pop() -> Option<String> {
    QUEUE.with(|q| q.borrow_mut().pop_front())
}

/// Snapshot of the queue (php_openssl_errors_set_mark).
pub fn mark() -> VecDeque<String> {
    QUEUE.with(|q| q.borrow().clone())
}

/// Restore a snapshot (php_openssl_errors_restore_mark).
pub fn restore(m: VecDeque<String>) {
    QUEUE.with(|q| *q.borrow_mut() = m);
}

// Frequently raised errors.

pub fn evp_invalid_key_length() {
    push(LIB_EVP, 130, "invalid key length");
}

pub fn prov_wrong_final_block_length() {
    push(LIB_PROV, 107, "wrong final block length");
}

pub fn prov_bad_decrypt() {
    push(LIB_PROV, 100, "bad decrypt");
}

/// A system (errno) error: OpenSSL 3 packs these as `ERR_SYSTEM_FLAG | errno`,
/// which ERR_error_string_n() prints sign-extended to 64 bits.
pub fn sys(errno: i32, text: &str) {
    push_str(format!(
        "error:{:08X}:system library::{}",
        0xFFFF_FFFF_8000_0000u64 | errno as u32 as u64,
        text
    ));
}

/// BIO_new_file() failure for a file that cannot be opened.
pub fn bio_new_file_failed(e: &std::io::Error) {
    let errno = e.raw_os_error().unwrap_or(2);
    let text = match errno {
        2 => "No such file or directory".to_string(),
        13 => "Permission denied".to_string(),
        21 => "Is a directory".to_string(),
        _ => e.to_string(),
    };
    sys(errno, &text);
    if errno == 2 {
        push(LIB_BIO, 128, "no such file");
    } else {
        push(LIB_BIO, R_SYS_LIB, "system lib");
    }
}

/// OBJ_txt2obj() on a name that is neither a known short/long name nor
/// dotted digits.
pub fn obj_unknown_name(name: &str) {
    if !name.starts_with(|c: char| c.is_ascii_digit()) {
        push(LIB_OBJ, 103, "unknown object name");
    }
}

pub fn pem_no_start_line() {
    push(LIB_PEM, 108, "no start line");
}
