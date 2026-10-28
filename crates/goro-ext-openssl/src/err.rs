//! The error queue behind `openssl_error_string()` (PHP manual:
//! openssl_error_string — "returns the last error from the openSSL
//! library"; messages are read one at a time until the queue is empty).
//!
//! Messages use the `error:CODE:library::reason` form PHP 8.5 prints with
//! OpenSSL 3 (observed on PHP 8.5); the codes and texts in [`E`] were
//! observed on the reference build for the failures they stand for.

use std::cell::RefCell;
use std::collections::VecDeque;

/// Capacity of the queue: older entries are dropped (observed on PHP 8.5:
/// at most 16 messages are kept).
const CAP: usize = 16;

thread_local! {
    static QUEUE: RefCell<VecDeque<String>> = const { RefCell::new(VecDeque::new()) };
}

/// One library error: its packed code, library and reason text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct E {
    pub code: u32,
    pub lib: &'static str,
    pub reason: &'static str,
}

impl E {
    pub const fn new(code: u32, lib: &'static str, reason: &'static str) -> E {
        E { code, lib, reason }
    }
    pub fn message(&self) -> String {
        if self.code & 0x8000_0000 != 0 {
            // System errors print sign-extended (observed on PHP 8.5:
            // "error:FFFFFFFF80000002:system library::...").
            format!(
                "error:{:016X}:{}::{}",
                self.code as i32 as i64 as u64, self.lib, self.reason
            )
        } else {
            format!("error:{:08X}:{}::{}", self.code, self.lib, self.reason)
        }
    }
}

/// Queues an error.
pub fn push(e: E) {
    push_msg(e.message());
}

/// Queues a preformatted message.
pub fn push_msg(m: String) {
    QUEUE.with(|q| {
        let mut q = q.borrow_mut();
        if q.len() == CAP {
            q.pop_front();
        }
        q.push_back(m);
    });
}

/// The oldest queued message, removed.
pub fn pop() -> Option<String> {
    QUEUE.with(|q| q.borrow_mut().pop_front())
}

pub fn clear() {
    QUEUE.with(|q| q.borrow_mut().clear());
}

pub fn is_empty() -> bool {
    QUEUE.with(|q| q.borrow().is_empty())
}

/// Library names.
pub mod lib {
    pub const PEM: &str = "PEM routines";
    pub const ASN1: &str = "asn1 encoding routines";
    pub const X509: &str = "x509 certificate routines";
    pub const X509V3: &str = "X509 V3 routines";
    pub const EVP: &str = "digital envelope routines";
    pub const RSA: &str = "rsa routines";
    pub const BIO: &str = "BIO routines";
    pub const SYS: &str = "system library";
    pub const PROV: &str = "Provider routines";
    pub const DECODER: &str = "DECODER routines";
    pub const ENCODER: &str = "ENCODER routines";
    pub const CONF: &str = "configuration file routines";
    pub const PKCS7: &str = "PKCS7 routines";
    pub const CMS: &str = "CMS routines";
    pub const PKCS12: &str = "PKCS12 routines";
    pub const EC: &str = "elliptic curve routines";
    pub const DH: &str = "dh routines";
    pub const DSA: &str = "dsa routines";
    pub const OBJ: &str = "object identifier routines";
}

/// Errors used by the extension (codes observed on PHP 8.5 / OpenSSL 3.5;
/// the tests check some of them, ext/openssl/tests/
/// openssl_error_string_basic_openssl3.phpt).
pub mod errs {
    use super::{E, lib};
    pub const PEM_NO_START_LINE: E = E::new(0x0480006C, lib::PEM, "no start line");
    pub const PEM_BAD_DECRYPT: E = E::new(0x04800065, lib::PEM, "bad decrypt");
    pub const PEM_BAD_PASSWORD_READ: E = E::new(0x04800068, lib::PEM, "bad password read");
    pub const BIO_NO_SUCH_FILE: E = E::new(0x10000080, lib::BIO, "no such file");
    pub const SYS_NOENT: E = E::new(0x80000002, lib::SYS, "No such file or directory");
    pub const BIO_SYS: E = E::new(0x10080002, lib::BIO, "system lib");
    pub const DECODER_UNSUPPORTED: E = E::new(0x1E08010C, lib::DECODER, "unsupported");
    pub const EVP_BAD_DECRYPT: E = E::new(0x1C800064, lib::PROV, "bad decrypt");
    pub const PROV_ILLEGAL_PADDING: E =
        E::new(0x1C8000A5, lib::PROV, "illegal or unsupported padding mode");
    pub const RSA_OAEP_DECODING: E = E::new(0x02000079, lib::RSA, "oaep decoding error");
    pub const X509V3_INVALID_PURPOSE: E = E::new(0x05800079, lib::X509, "unknown purpose id");
    pub const RSA_PADDING_CHECK_FAILED: E = E::new(0x02000072, lib::RSA, "padding check failed");
    pub const RSA_BLOCK_TYPE_NOT_01: E = E::new(0x0200006A, lib::RSA, "block type is not 01");
    pub const RSA_BLOCK_TYPE_NOT_02: E = E::new(0x0200006B, lib::RSA, "block type is not 02");
    pub const RSA_DATA_TOO_LARGE_FOR_KEY: E =
        E::new(0x0200006E, lib::RSA, "data too large for key size");
    pub const RSA_DATA_TOO_LARGE_FOR_MOD: E =
        E::new(0x02000084, lib::RSA, "data too large for modulus");
    pub const RSA_DATA_TOO_SMALL: E = E::new(0x0200007A, lib::RSA, "data too small for key size");
    pub const RSA_WRONG_SIG_LEN: E = E::new(0x02000077, lib::RSA, "wrong signature length");
    pub const PROV_RSA_LIB: E = E::new(0x1C880004, lib::PROV, "RSA lib");
    pub const RSA_INVALID_PADDING: E = E::new(0x0200008A, lib::RSA, "invalid padding");
    pub const EVP_PROVIDER_SIG_FAILURE: E =
        E::new(0x030000EA, lib::EVP, "provider signature failure");
    pub const EVP_OP_NOT_SUPPORTED_KEYTYPE: E = E::new(
        0x03000096,
        lib::EVP,
        "operation not supported for this keytype",
    );
    pub const EVP_UNSUPPORTED: E = E::new(0x0308010C, lib::EVP, "unsupported");
    pub const PROV_KEY_SIZE_TOO_SMALL: E = E::new(0x1C8000AB, lib::PROV, "key size too small");
    pub const DSA_BAD_FFC_PARAMETERS: E = E::new(0x05000072, lib::DSA, "bad ffc parameters");
    pub const PROV_INVALID_DIGEST: E = E::new(0x1C80007A, lib::PROV, "invalid digest");
    pub const OBJ_UNKNOWN_OBJECT_NAME: E = E::new(
        0x04000067,
        "object identifier routines",
        "unknown object name",
    );
    pub const X509_INVALID_FIELD_NAME: E = E::new(0x05800077, lib::X509, "invalid field name");
    pub const ASN1_STRING_TOO_LONG: E = E::new(0x06800097, lib::ASN1, "string too long");
    pub const ASN1_STRING_TOO_SHORT: E = E::new(0x06800098, lib::ASN1, "string too short");
    pub const ASN1_UNKNOWN_SIG_ALG: E =
        E::new(0x068000C7, lib::ASN1, "unknown signature algorithm");
    pub const X509V3_NO_ISSUER_CERT: E = E::new(0x11000079, lib::X509V3, "no issuer certificate");
    pub const X509V3_ERROR_IN_EXTENSION: E = E::new(0x11000080, lib::X509V3, "error in extension");
    pub const RSA_DATA_GREATER_THAN_MOD: E =
        E::new(0x02000084, lib::RSA, "data too large for modulus");
    pub const RSA_PKCS_DECODING: E = E::new(0x02000077, lib::RSA, "pkcs decoding error");
    pub const EVP_WRONG_FINAL_BLOCK_LENGTH: E =
        E::new(0x1C80006B, lib::PROV, "wrong final block length");
    pub const EVP_BAD_DECRYPT_PROV: E = E::new(0x1C800064, lib::PROV, "bad decrypt");
    pub const ASN1_WRONG_TAG: E = E::new(0x0680008E, lib::ASN1, "wrong tag");
    pub const ASN1_HEADER_TOO_LONG: E = E::new(0x0680009B, lib::ASN1, "header too long");
    pub const ASN1_NESTED_ERROR: E = E::new(0x0688010A, lib::ASN1, "nested asn1 error");
    pub const X509_KEY_VALUES_MISMATCH: E = E::new(0x05800074, lib::X509, "key values mismatch");
    pub const X509_CERT_ALREADY_IN_HASH_TABLE: E =
        E::new(0x05800065, lib::X509, "cert already in hash table");
}
