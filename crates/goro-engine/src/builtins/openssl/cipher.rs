//! openssl_encrypt()/openssl_decrypt() and friends: ciphers, digests,
//! PBKDF2 and randomness (PHP manual: OpenSSL functions).
//!
//! The warnings, their order, the errors queued for openssl_error_string()
//! and the results of the edge cases were observed on PHP 8.5 (with
//! OpenSSL 3.5) and checked against ext/openssl/tests: openssl_encrypt_*,
//! openssl_decrypt_*, openssl_cipher_*_length_*, openssl_digest_basic,
//! openssl_pbkdf2_basic, openssl_random_pseudo_bytes_*, bug54060, bug54061,
//! bug61124, bug74099, bug78391, gh19994.

use goro_ext_openssl::cipher::{self as ci, CipherInfo, Mode, errs};
use goro_ext_openssl::digest::{self, Md};
use goro_ext_openssl::err;
use goro_ext_standard::url::{base64_decode, base64_encode};
use goro_runtime::{PhpArray, PhpStr, Value};

use crate::func::Args;
use crate::vm::{R, Vm};

/// PHP manual: OpenSSL options constants.
const OPENSSL_RAW_DATA: i64 = 1;
const OPENSSL_ZERO_PADDING: i64 = 2;
const OPENSSL_DONT_ZERO_PAD_KEY: i64 = 4;

fn str_val(b: &[u8]) -> Value {
    Value::Str(PhpStr::new(b))
}

pub(super) fn openssl_get_cipher_methods(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let aliases = a.bool_or(0, false);
    Ok(Value::Array(PhpArray::from_values(
        ci::methods(aliases).into_iter().map(Value::from),
    )))
}

fn cipher_by_arg(vm: &mut Vm, a: &Args) -> R<Option<&'static CipherInfo>> {
    let name = a.str(0);
    if name.is_empty() {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #1 ($cipher_algo) must not be empty",
        );
    }
    match ci::lookup_bytes(name.as_bytes()) {
        Some(c) => Ok(Some(c)),
        None => {
            vm.fwarn(a, "Unknown cipher algorithm")?;
            err::push(errs::UNSUPPORTED);
            Ok(None)
        }
    }
}

pub(super) fn openssl_cipher_iv_length(vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(match cipher_by_arg(vm, a)? {
        Some(c) => Value::Int(c.iv_len() as i64),
        None => Value::Bool(false),
    })
}

pub(super) fn openssl_cipher_key_length(vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(match cipher_by_arg(vm, a)? {
        Some(c) => Value::Int(c.key_len() as i64),
        None => Value::Bool(false),
    })
}

/// The key and IV ready for the cipher, after PHP's checks and warnings
/// (observed on PHP 8.5, in this order: the IV — padded or truncated with
/// a warning, or for AEAD modes a length control that may fail —, the
/// AEAD tag length, then the key — truncated with a queued error, or
/// zero-padded unless OPENSSL_DONT_ZERO_PAD_KEY).
fn prepare(
    vm: &mut Vm,
    a: &Args,
    c: &CipherInfo,
    encrypting: bool,
    options: i64,
    tag_len: Option<usize>,
) -> R<Option<(Vec<u8>, Vec<u8>)>> {
    let pw = a.str(2);
    let iv_in = a.str(4);
    let mut iv = iv_in.as_bytes().to_vec();
    if c.is_aead() {
        if !c.aead_iv_ok(iv.len()) {
            vm.fwarn(a, "Setting of IV length for AEAD mode failed")?;
            if c.mode() != Mode::Ocb {
                err::push(errs::INVALID_IV_LENGTH);
            }
            return Ok(None);
        }
        // CCM and OCB fix the tag length before the key (encryption: the
        // $tag_length argument; decryption: OCB takes the tag's length).
        // GCM and ChaCha20-Poly1305 check the length when the tag is read;
        // OCB takes a zero length here and fails on the AAD later.
        if let Some(tl) = tag_len
            && !c.tag_len_ok(tl as i64)
        {
            let ocb = c.mode() == Mode::Ocb;
            if (encrypting && !matches!(c.mode(), Mode::Ccm | Mode::Ocb)) || (ocb && tl == 0) {
            } else if ocb || encrypting {
                vm.fwarn(a, "Setting tag length for AEAD cipher failed")?;
                err::push(errs::INVALID_TAG_LENGTH);
                return Ok(None);
            } else {
                vm.fwarn(a, "Setting tag for AEAD cipher decryption failed")?;
                if c.mode() == Mode::Gcm {
                    err::push(errs::TOO_SMALL_BUFFER);
                    err::push(errs::FAILED_TO_GET_PARAMETER);
                } else {
                    err::push(errs::INVALID_TAG_LENGTH);
                }
                return Ok(None);
            }
        }
    } else {
        let want = c.iv_len();
        if iv.len() != want {
            if iv.is_empty() {
                if encrypting {
                    vm.fwarn(
                        a,
                        "Using an empty Initialization Vector (iv) is potentially insecure and not recommended",
                    )?;
                }
            } else if iv.len() < want {
                vm.fwarn(
                    a,
                    &format!(
                        "IV passed is only {} bytes long, cipher expects an IV of precisely {} bytes, padding with \\0",
                        iv.len(),
                        want
                    ),
                )?;
            } else {
                vm.fwarn(
                    a,
                    &format!(
                        "IV passed is {} bytes long which is longer than the {} expected by selected cipher, truncating",
                        iv.len(),
                        want
                    ),
                )?;
            }
            iv.resize(want, 0);
        }
    }
    let kl = c.key_len();
    let mut key = pw.as_bytes().to_vec();
    if key.len() > kl {
        err::push(errs::INVALID_KEY_LENGTH);
        key.truncate(kl);
    } else if key.len() < kl {
        if options & OPENSSL_DONT_ZERO_PAD_KEY != 0 {
            vm.fwarn(a, "Key length cannot be set for the cipher algorithm")?;
            err::push(errs::INVALID_KEY_LENGTH);
            return Ok(None);
        }
        key.resize(kl, 0);
    }
    Ok(Some((key, iv)))
}

fn lookup_cipher(vm: &mut Vm, a: &Args) -> R<Option<&'static CipherInfo>> {
    let name = a.str(1);
    let Some(c) = ci::lookup_bytes(name.as_bytes()) else {
        vm.fwarn(a, "Unknown cipher algorithm")?;
        err::push(errs::UNSUPPORTED);
        return Ok(None);
    };
    if c.is_legacy() {
        // Known to OpenSSL, but its provider is not loaded (observed on
        // PHP 8.5: no warning, "unsupported" queued).
        err::push(errs::UNSUPPORTED);
        return Ok(None);
    }
    Ok(Some(c))
}

fn output(out: Vec<u8>, options: i64) -> Value {
    if options & OPENSSL_RAW_DATA != 0 {
        Value::Str(PhpStr::from(out))
    } else {
        Value::Str(PhpStr::from(base64_encode(&out)))
    }
}

pub(super) fn openssl_encrypt(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let options = a.int(3);
    let tag_len = a.int_or(7, 16);
    if tag_len > i32::MAX as i64 || tag_len < i32::MIN as i64 {
        return vm.throw("ValueError", "tag_len is too long");
    }
    let Some(c) = lookup_cipher(vm, a)? else {
        return Ok(Value::Bool(false));
    };
    let tl = if tag_len < 0 {
        usize::MAX
    } else {
        tag_len as usize
    };
    let Some((key, iv)) = prepare(vm, a, c, true, options, Some(tl))? else {
        return Ok(Value::Bool(false));
    };
    let has_tag = a.ref_at(5).is_some();
    let data = a.str(0);
    let raw = options & OPENSSL_RAW_DATA != 0;
    if !c.is_aead() {
        let padding = options & OPENSSL_ZERO_PADDING == 0;
        let res = if raw && ci::in_place_ok(c, padding) {
            // Encrypt in the result string itself (saves a copy).
            let mut s = PhpStr::new(data.as_bytes());
            ci::crypt_in_place(c, &key, &iv, s.make_mut(), false).map(|()| Value::Str(s))
        } else if raw && c.pads() {
            let pad = ci::padding_for(c, data.len());
            let mut s = PhpStr::with_capacity(data.len() + pad.len());
            s.push_bytes(data.as_bytes());
            s.push_bytes(&pad);
            ci::crypt_in_place(c, &key, &iv, s.make_mut(), false).map(|()| Value::Str(s))
        } else {
            ci::encrypt(c, &key, &iv, data.as_bytes(), padding).map(|out| output(out, options))
        };
        return Ok(match res {
            Ok(v) => {
                if has_tag {
                    a.set_ref(5, Value::Null);
                }
                v
            }
            Err(e) => {
                e.push();
                Value::Bool(false)
            }
        });
    }
    if !has_tag {
        vm.fwarn(a, "A tag should be provided when using AEAD mode")?;
        return Ok(Value::Bool(false));
    }
    if c.mode() == Mode::Ocb && tag_len == 0 {
        vm.fwarn(a, "Setting of additional application data failed")?;
        return Ok(Value::Bool(false));
    }
    if !c.tag_len_ok(tag_len) {
        // GCM and ChaCha20-Poly1305 encrypt, then fail to read the tag.
        vm.fwarn(a, "Retrieving verification tag failed")?;
        err::push(if c.mode() == Mode::Gcm {
            errs::INVALID_TAG
        } else {
            errs::INVALID_TAG_LENGTH
        });
        return Ok(Value::Bool(false));
    }
    let aad = a.str(6);
    let mut s = PhpStr::new(data.as_bytes());
    match ci::aead_encrypt_in_place(c, &key, &iv, aad.as_bytes(), s.make_mut(), tl) {
        Ok(tag) => {
            a.set_ref(5, str_val(&tag));
            Ok(if raw {
                Value::Str(s)
            } else {
                Value::Str(PhpStr::from(base64_encode(s.as_bytes())))
            })
        }
        Err(e) => {
            e.push();
            Ok(Value::Bool(false))
        }
    }
}

pub(super) fn openssl_decrypt(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let options = a.int(3);
    if a.str(1).is_empty() {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #2 ($cipher_algo) must not be empty",
        );
    }
    let Some(c) = lookup_cipher(vm, a)? else {
        return Ok(Value::Bool(false));
    };
    let tag = a.opt_str(5).filter(|t| !t.is_empty());
    if tag.is_some() && !c.is_aead() {
        vm.fwarn(
            a,
            "The tag cannot be used because the cipher algorithm does not support AEAD",
        )?;
    }
    let raw = a.str(0);
    let decoded;
    let data: &[u8] = if options & OPENSSL_RAW_DATA != 0 {
        raw.as_bytes()
    } else {
        match base64_decode(raw.as_bytes(), false) {
            Some(d) => {
                decoded = d;
                &decoded
            }
            None => {
                vm.fwarn(a, "Failed to base64 decode the input")?;
                return Ok(Value::Bool(false));
            }
        }
    };
    let tl = if c.is_aead() {
        tag.as_ref().map(|t| t.len())
    } else {
        None
    };
    let Some((key, iv)) = prepare(vm, a, c, false, options, tl)? else {
        return Ok(Value::Bool(false));
    };
    let padding = options & OPENSSL_ZERO_PADDING == 0;
    let res = if c.is_aead() {
        if c.mode() == Mode::Ocb && tag.is_none() {
            vm.fwarn(a, "Setting of additional application data failed")?;
            return Ok(Value::Bool(false));
        }
        let aad = a.str(6);
        // Decrypt in the result string itself (saves a copy).
        let mut s = PhpStr::new(data);
        ci::aead_decrypt_in_place(
            c,
            &key,
            &iv,
            aad.as_bytes(),
            s.make_mut(),
            tag.as_ref().map(|t| t.as_bytes()),
        )
        .map(|()| s)
    } else if ci::in_place_ok(c, padding) {
        let mut s = PhpStr::new(data);
        ci::crypt_in_place(c, &key, &iv, s.make_mut(), true).map(|()| s)
    } else if c.pads() {
        // Padded ECB/CBC: the length check first, then decrypt in place and
        // strip the padding.
        if data.is_empty() || !data.len().is_multiple_of(c.cipher_block_size()) {
            Err(ci::CipherError::WrongFinalBlockLength)
        } else {
            let mut s = PhpStr::new(data);
            ci::crypt_in_place(c, &key, &iv, s.make_mut(), true)
                .and_then(|()| ci::unpadded_len(c, s.as_bytes()))
                .map(|l| {
                    s.truncate(l);
                    s
                })
        }
    } else {
        ci::decrypt(c, &key, &iv, data, padding).map(PhpStr::from)
    };
    Ok(match res {
        Ok(s) => Value::Str(s),
        Err(e) => {
            e.push();
            Value::Bool(false)
        }
    })
}

pub(super) fn openssl_get_md_methods(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let list = if a.bool_or(0, false) {
        digest::MD_METHODS_ALIASES
    } else {
        digest::MD_METHODS
    };
    Ok(Value::Array(PhpArray::from_values(
        list.iter().map(|s| Value::from(*s)),
    )))
}

/// A digest by name, with OpenSSL's name rules (the name ends at a NUL
/// byte, trailing spaces are dropped; observed on PHP 8.5).
fn md_by_name(name: &[u8]) -> Option<Md> {
    let name = match name.iter().position(|&b| b == 0) {
        Some(p) => &name[..p],
        None => name,
    };
    let mut end = name.len();
    while end > 0 && name[end - 1] == b' ' {
        end -= 1;
    }
    Md::by_name(std::str::from_utf8(&name[..end]).ok()?)
}

/// MD4 and Whirlpool live in OpenSSL's legacy provider, which PHP 8.5's
/// reference build does not load (observed: "unsupported").
fn md_legacy(md: Md) -> bool {
    matches!(md.name(), "md4" | "whirlpool")
}

pub(super) fn openssl_digest(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let data = a.str(0);
    let binary = a.bool_or(2, false);
    let Some(md) = md_by_name(a.str(1).as_bytes()) else {
        vm.fwarn(a, "Unknown digest algorithm")?;
        err::push(errs::UNSUPPORTED);
        return Ok(Value::Bool(false));
    };
    if md_legacy(md) {
        err::push(errs::UNSUPPORTED);
        err::push(errs::INITIALIZATION_ERROR);
        return Ok(Value::Bool(false));
    }
    if matches!(md, Md::Shake128 | Md::Shake256) {
        // An XOF without an output length: the digest fails silently
        // (observed on PHP 8.5).
        return Ok(Value::Bool(false));
    }
    let d = md.digest(data.as_bytes());
    Ok(if binary {
        Value::Str(PhpStr::from(d))
    } else {
        Value::Str(PhpStr::from(goro_ext_standard::encoding::bin2hex(&d)))
    })
}

pub(super) fn openssl_pbkdf2(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let key_length = a.int(2);
    let iterations = a.int(3);
    if key_length <= 0 {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #3 ($key_length) must be greater than 0",
        );
    }
    if key_length > i32::MAX as i64 {
        return vm.fthrow(a, "ValueError", "Argument #3 ($key_length) key is too long");
    }
    if iterations > i32::MAX as i64 || iterations < i32::MIN as i64 {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #4 ($iterations) iterations is too long",
        );
    }
    let algo = if a.has(4) {
        a.str(4)
    } else {
        PhpStr::from("sha1")
    };
    // An empty name selects the default, SHA-1 (observed on PHP 8.5).
    let algo = if algo.is_empty() {
        PhpStr::from("sha1")
    } else {
        algo
    };
    let md = {
        match md_by_name(algo.as_bytes()) {
            Some(m) => m,
            None => {
                vm.fwarn(a, "Unknown digest algorithm")?;
                err::push(errs::UNSUPPORTED);
                return Ok(Value::Bool(false));
            }
        }
    };
    if md_legacy(md) {
        err::push(errs::UNSUPPORTED);
        return Ok(Value::Bool(false));
    }
    if matches!(md, Md::Shake128 | Md::Shake256) {
        err::push(errs::XOF_DIGESTS_NOT_ALLOWED);
        return Ok(Value::Bool(false));
    }
    if iterations == 0 {
        err::push(errs::INVALID_ITERATION_COUNT);
        return Ok(Value::Bool(false));
    }
    if iterations < 0 {
        err::push(errs::PARAM_NEGATIVE);
        return Ok(Value::Bool(false));
    }
    let pw = a.str(0);
    let salt = a.str(1);
    let out = digest::pbkdf2(
        md,
        pw.as_bytes(),
        salt.as_bytes(),
        iterations as u32,
        key_length as usize,
    );
    Ok(Value::Str(PhpStr::from(out)))
}

pub(super) fn openssl_random_pseudo_bytes(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let len = a.int(0);
    if len < 1 {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #1 ($length) must be greater than 0",
        );
    }
    if len > i32::MAX as i64 {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #1 ($length) must be less than or equal to 2147483647",
        );
    }
    let mut buf = vec![0u8; len as usize];
    goro_ext_openssl::random_bytes(&mut buf);
    if a.ref_at(1).is_some() {
        a.set_ref(1, Value::Bool(true));
    }
    Ok(Value::Str(PhpStr::from(buf)))
}
