//! X.509 certificate / CSR / SPKI PHP functions (openssl.c ports).

use crate::asn1;
use crate::bn::Bn;
use crate::cert::{self, Cert, Csr, Ext};
use crate::digest::Md;
use crate::name::{self, Ava, Name};
use crate::pkey::PKey;
use crate::pkey_fns;
use crate::req;
use crate::x509v3::{self, V3Ctx};
use crate::{err, obj, zpp};
use goro_core::array::{ArrayKey, PhpArray};
use goro_core::object::PhpObject;
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

// -------------------------------------------------------------------------
// Object registries
// -------------------------------------------------------------------------

thread_local! {
    static CERTS: RefCell<HashMap<u64, Rc<Cert>>> = RefCell::new(HashMap::new());
    static CSRS: RefCell<HashMap<u64, Rc<Csr>>> = RefCell::new(HashMap::new());
    static NEXT_ID: std::cell::Cell<u64> = const { std::cell::Cell::new(1) };
}

fn next_id() -> u64 {
    NEXT_ID.with(|n| {
        let id = n.get();
        n.set(id + 1);
        id
    })
}

pub fn make_cert_object(vm: &mut Vm, c: Rc<Cert>) -> Value {
    let id = next_id();
    CERTS.with(|r| r.borrow_mut().insert(id, c));
    let oid = vm.next_object_id();
    let mut obj = PhpObject::new(b"OpenSSLCertificate".to_vec(), oid);
    obj.set_property(b"__cert_id".to_vec(), Value::Long(id as i64));
    Value::Object(Rc::new(RefCell::new(obj)))
}

pub fn make_csr_object(vm: &mut Vm, c: Csr) -> Value {
    let id = next_id();
    CSRS.with(|r| r.borrow_mut().insert(id, Rc::new(c)));
    let oid = vm.next_object_id();
    let mut obj = PhpObject::new(b"OpenSSLCertificateSigningRequest".to_vec(), oid);
    obj.set_property(b"__csr_id".to_vec(), Value::Long(id as i64));
    Value::Object(Rc::new(RefCell::new(obj)))
}

fn class_is(v: &Value, class: &str) -> bool {
    match v.deref() {
        Value::Object(o) => o.borrow().class_name.eq_ignore_ascii_case(class.as_bytes()),
        _ => false,
    }
}

pub fn cert_from_object(v: &Value) -> Option<Rc<Cert>> {
    if let Value::Object(o) = v.deref() {
        let o = o.borrow();
        if o.class_name.eq_ignore_ascii_case(b"OpenSSLCertificate") {
            let id = o.get_property(b"__cert_id").to_long() as u64;
            return CERTS.with(|r| r.borrow().get(&id).cloned());
        }
    }
    None
}

fn csr_from_object(v: &Value) -> Option<Rc<Csr>> {
    if let Value::Object(o) = v.deref() {
        let o = o.borrow();
        if o.class_name
            .eq_ignore_ascii_case(b"OpenSSLCertificateSigningRequest")
        {
            let id = o.get_property(b"__csr_id").to_long() as u64;
            return CSRS.with(|r| r.borrow().get(&id).cloned());
        }
    }
    None
}

// -------------------------------------------------------------------------
// Parameter parsing helpers
// -------------------------------------------------------------------------

pub enum ObjOrStr<T> {
    Obj(Rc<T>),
    Str(PhpString),
}

/// Z_PARAM_OBJ_OF_CLASS_OR_STR(_OR_NULL)
#[allow(clippy::too_many_arguments)]
fn obj_or_str<T>(
    vm: &mut Vm,
    args: &[Value],
    idx: usize,
    f: &str,
    pname: &str,
    class: &str,
    nullable: bool,
    get: fn(&Value) -> Option<Rc<T>>,
) -> Result<Option<ObjOrStr<T>>, VmError> {
    let v = zpp::arg(args, idx).unwrap_or(Value::Null);
    if class_is(&v, class) {
        return match get(&v) {
            Some(o) => Ok(Some(ObjOrStr::Obj(o))),
            None => Ok(None),
        };
    }
    let expected = if nullable {
        format!("{}|string|null", class)
    } else {
        format!("{}|string", class)
    };
    match &v {
        Value::Null | Value::Undef => {
            if nullable {
                return Ok(None);
            }
            if idx < args.len() {
                vm.emit_deprecated(&format!(
                    "{}(): Passing null to parameter #{} (${}) of type {} is deprecated",
                    f,
                    idx + 1,
                    pname,
                    expected
                ));
            }
            Ok(Some(ObjOrStr::Str(PhpString::empty())))
        }
        Value::Array(_) | Value::Resource(_) => {
            Err(zpp::arg_type_error(vm, f, idx + 1, pname, &expected, &v))
        }
        Value::Object(o) => {
            let lc = o.borrow().class_name.to_ascii_lowercase();
            let has = vm
                .get_class_def(&lc)
                .map(|c| c.get_method(b"__tostring").is_some())
                .unwrap_or(false);
            if has {
                let s = vm.value_to_string(&v);
                Ok(Some(ObjOrStr::Str(s)))
            } else {
                Err(zpp::arg_type_error(vm, f, idx + 1, pname, &expected, &v))
            }
        }
        other => {
            vm.warn_nan_coercion(other, "string", vm.current_line);
            Ok(Some(ObjOrStr::Str(other.to_php_string())))
        }
    }
}

fn cert_param(
    vm: &mut Vm,
    args: &[Value],
    idx: usize,
    f: &str,
    pname: &str,
    nullable: bool,
) -> Result<Option<ObjOrStr<Cert>>, VmError> {
    obj_or_str(
        vm,
        args,
        idx,
        f,
        pname,
        "OpenSSLCertificate",
        nullable,
        cert_from_object,
    )
}

fn csr_param(
    vm: &mut Vm,
    args: &[Value],
    idx: usize,
    f: &str,
    pname: &str,
) -> Result<Option<ObjOrStr<Csr>>, VmError> {
    obj_or_str(
        vm,
        args,
        idx,
        f,
        pname,
        "OpenSSLCertificateSigningRequest",
        false,
        csr_from_object,
    )
}

/// php_openssl_check_path_ex() for plain paths (no file:// prefix).
pub fn check_plain_path(
    vm: &mut Vm,
    f: &str,
    arg_num: usize,
    path: &[u8],
    from_array: bool,
    option: Option<&str>,
) -> Result<Option<String>, VmError> {
    if path.is_empty() {
        return Ok(Some(String::new()));
    }
    let pname = vm
        .builtin_param_names
        .get(f.as_bytes())
        .and_then(|n| n.get(arg_num.saturating_sub(1)))
        .map(|n| String::from_utf8_lossy(n).to_string())
        .unwrap_or_default();
    let what = |msg: &str| -> String {
        match (from_array, option) {
            (true, Some(o)) => format!("option {} array item {}", o, msg),
            (true, None) => format!("array item {}", msg),
            (false, Some(o)) => format!("option {} {}", o, msg),
            (false, None) => msg.to_string(),
        }
    };
    if path.contains(&0) {
        if arg_num == 0 {
            vm.emit_warning(&format!(
                "{}(): Path for {} {} must not contain any null bytes",
                f,
                option.unwrap_or("unknown"),
                if from_array { "array item" } else { "option" }
            ));
            return Ok(None);
        }
        return Err(zpp::arg_value_error(
            vm,
            f,
            arg_num,
            &pname,
            &what("must not contain any null bytes"),
        ));
    }
    let p = String::from_utf8_lossy(path).to_string();
    let expanded = goro_core::basedir::expand_filepath(&p).filter(|e| e.len() < 4096);
    let Some(real) = expanded else {
        if arg_num == 0 {
            vm.emit_warning(&format!(
                "{}(): Path for {} {} must be a valid file path",
                f,
                option.unwrap_or("unknown"),
                if from_array { "array item" } else { "option" }
            ));
        } else {
            vm.emit_warning(&format!(
                "{}({}): {}",
                f,
                pname,
                what("must be a valid file path")
            ));
        }
        return Ok(None);
    };
    if !vm.check_open_basedir(f, &real) {
        return Ok(None);
    }
    Ok(Some(real))
}

/// php_openssl_x509_from_str()
pub fn cert_from_str(
    vm: &mut Vm,
    f: &str,
    arg_num: usize,
    s: &[u8],
    from_array: bool,
) -> Result<Option<Cert>, VmError> {
    cert_from_str_opt(vm, f, arg_num, s, from_array, None)
}

fn cert_from_str_opt(
    vm: &mut Vm,
    f: &str,
    arg_num: usize,
    s: &[u8],
    from_array: bool,
    option: Option<&str>,
) -> Result<Option<Cert>, VmError> {
    let (data, is_file) = if s.len() > 7 && &s[..7] == b"file://" {
        let Some(path) = check_plain_path(vm, f, arg_num, &s[7..], from_array, option)? else {
            return Ok(None);
        };
        match std::fs::read(&path) {
            Ok(d) => (d, true),
            Err(e) => {
                err::bio_new_file_failed(&e);
                return Ok(None);
            }
        }
    } else {
        (s.to_vec(), false)
    };
    let labels: &[&str] = if is_file {
        &["CERTIFICATE", "X509 CERTIFICATE", "TRUSTED CERTIFICATE"]
    } else {
        &["CERTIFICATE", "X509 CERTIFICATE"]
    };
    let block = asn1::pem_find(&data, labels);
    if let Some(b) = &block {
        if cert::push_short_time_errors(&b.der) {
            err::push(err::LIB_PEM, err::RFLAG_COMMON | 13, "ASN1 lib");
            return Ok(None);
        }
    }
    match block.and_then(|b| {
        let der = if b.label == "TRUSTED CERTIFICATE" {
            asn1::parse(&b.der)
                .map(|(t, _)| t.raw.to_vec())
                .unwrap_or_default()
        } else {
            b.der
        };
        Cert::parse(&der)
    }) {
        Some(c) => Ok(Some(c)),
        None => {
            err::pem_no_start_line();
            Ok(None)
        }
    }
}

fn cert_from_arg(
    vm: &mut Vm,
    f: &str,
    arg_num: usize,
    a: Option<ObjOrStr<Cert>>,
) -> Result<Option<Rc<Cert>>, VmError> {
    match a {
        Some(ObjOrStr::Obj(c)) => Ok(Some(c)),
        Some(ObjOrStr::Str(s)) => {
            Ok(cert_from_str(vm, f, arg_num, s.as_bytes(), false)?.map(Rc::new))
        }
        None => Ok(None),
    }
}

/// php_openssl_x509_from_zval()
pub fn cert_from_zval(
    vm: &mut Vm,
    f: &str,
    arg_num: usize,
    v: &Value,
    from_array: bool,
) -> Result<Option<Rc<Cert>>, VmError> {
    cert_from_zval_opt(vm, f, arg_num, v, from_array, None)
}

/// php_openssl_x509_from_zval() with an option name for error messages.
pub fn cert_from_zval_opt(
    vm: &mut Vm,
    f: &str,
    arg_num: usize,
    v: &Value,
    from_array: bool,
    option: Option<&str>,
) -> Result<Option<Rc<Cert>>, VmError> {
    if let Some(c) = cert_from_object(v) {
        return Ok(Some(c));
    }
    let s = match v.deref() {
        Value::Array(_) => {
            vm.emit_warning("Array to string conversion");
            PhpString::from_bytes(b"Array")
        }
        other => vm.value_to_string_checked(&other)?,
    };
    Ok(cert_from_str_opt(vm, f, arg_num, s.as_bytes(), from_array, option)?.map(Rc::new))
}

/// Z_PARAM_OBJ_OF_CLASS_OR_STR for an OpenSSLCertificate argument.
pub fn cert_param_pub(
    vm: &mut Vm,
    args: &[Value],
    idx: usize,
    f: &str,
    pname: &str,
) -> Result<Option<ObjOrStr<Cert>>, VmError> {
    cert_param(vm, args, idx, f, pname, false)
}

/// php_openssl_x509_from_param()
pub fn cert_from_arg_pub(
    vm: &mut Vm,
    f: &str,
    arg_num: usize,
    a: Option<ObjOrStr<Cert>>,
) -> Result<Option<Rc<Cert>>, VmError> {
    cert_from_arg(vm, f, arg_num, a)
}

/// php_openssl_csr_from_str()
fn csr_from_str(vm: &mut Vm, f: &str, arg_num: usize, s: &[u8]) -> Result<Option<Csr>, VmError> {
    let data = if s.len() > 7 && &s[..7] == b"file://" {
        let Some(path) = check_plain_path(vm, f, arg_num, &s[7..], false, None)? else {
            return Ok(None);
        };
        match std::fs::read(&path) {
            Ok(d) => d,
            Err(e) => {
                err::bio_new_file_failed(&e);
                return Ok(None);
            }
        }
    } else {
        s.to_vec()
    };
    match asn1::pem_find(&data, &["CERTIFICATE REQUEST", "NEW CERTIFICATE REQUEST"])
        .and_then(|b| Csr::parse(&b.der))
    {
        Some(c) => Ok(Some(c)),
        None => {
            err::pem_no_start_line();
            Ok(None)
        }
    }
}

fn csr_from_arg(
    vm: &mut Vm,
    f: &str,
    arg_num: usize,
    a: Option<ObjOrStr<Csr>>,
) -> Result<Option<Rc<Csr>>, VmError> {
    match a {
        Some(ObjOrStr::Obj(c)) => Ok(Some(c)),
        Some(ObjOrStr::Str(s)) => Ok(csr_from_str(vm, f, arg_num, s.as_bytes())?.map(Rc::new)),
        None => Ok(None),
    }
}

// -------------------------------------------------------------------------
// Compatibility helpers for pkey_fns / pkcs7
// -------------------------------------------------------------------------

pub fn cert_der_from_object(v: &Value) -> Option<Vec<u8>> {
    cert_from_object(v).map(|c| c.der.clone())
}

pub fn cert_der_from_str(
    vm: &mut Vm,
    fname: &str,
    arg_num: usize,
    s: &[u8],
) -> Result<Option<Vec<u8>>, VmError> {
    Ok(cert_from_str(vm, fname, arg_num, s, false)?.map(|c| c.der))
}

pub fn cert_spki(der: &[u8]) -> Option<Vec<u8>> {
    Cert::parse(der).map(|c| c.spki)
}

// -------------------------------------------------------------------------
// Text printing (X509_print / X509_REQ_print)
// -------------------------------------------------------------------------

fn hex_block(b: &[u8], indent: usize) -> String {
    let mut out = String::new();
    for (i, chunk) in b.chunks(15).enumerate() {
        if i > 0 {
            out.push('\n');
        }
        out.push_str(&" ".repeat(indent));
        let line: Vec<String> = chunk.iter().map(|x| format!("{:02x}", x)).collect();
        out.push_str(&line.join(":"));
        if (i + 1) * 15 < b.len() {
            out.push(':');
        }
    }
    out.push('\n');
    out
}

fn print_pubkey(k: Option<PKey>, spki: &[u8], indent: usize) -> String {
    let pad = " ".repeat(indent);
    let alg_oid = asn1::parse_one(spki)
        .and_then(|t| t.children())
        .and_then(|c| {
            c.first()
                .and_then(|a| a.children())
                .and_then(|a| a.first().and_then(|o| o.oid()))
        })
        .unwrap_or_default();
    let mut s = format!("{}Public Key Algorithm: {}\n", pad, obj::oid_ln(&alg_oid));
    let pad2 = " ".repeat(indent + 4);
    match k {
        Some(PKey::Rsa(r)) => {
            s.push_str(&format!(
                "{}Public-Key: ({} bit)\n{}Modulus:\n",
                pad2,
                r.n.bits(),
                pad2
            ));
            let mut nb = r.n.to_vec();
            if nb.first().map(|b| b & 0x80 != 0).unwrap_or(false) {
                nb.insert(0, 0);
            }
            s.push_str(&hex_block(&nb, indent + 8));
            s.push_str(&format!(
                "{}Exponent: {} (0x{})\n",
                pad2,
                r.e.to_dec(),
                r.e.to_hex_lower()
            ));
        }
        Some(PKey::Ec(e)) => {
            s.push_str(&format!(
                "{}Public-Key: ({} bit)\n{}pub:\n",
                pad2,
                e.group.order_bits(),
                pad2
            ));
            if let Some(p) = &e.pub_key {
                s.push_str(&hex_block(&e.group.encode(p, false), indent + 8));
            }
            if let Some(n) = e.group.name {
                s.push_str(&format!("{}ASN1 OID: {}\n", pad2, n));
                // EC_curve_nid2nist()
                let nist = match n {
                    "prime192v1" => Some("P-192"),
                    "secp224r1" => Some("P-224"),
                    "prime256v1" => Some("P-256"),
                    "secp384r1" => Some("P-384"),
                    "secp521r1" => Some("P-521"),
                    "sect163k1" => Some("K-163"),
                    "sect233k1" => Some("K-233"),
                    "sect283k1" => Some("K-283"),
                    "sect409k1" => Some("K-409"),
                    "sect571k1" => Some("K-571"),
                    "sect163r2" => Some("B-163"),
                    "sect233r1" => Some("B-233"),
                    "sect283r1" => Some("B-283"),
                    "sect409r1" => Some("B-409"),
                    "sect571r1" => Some("B-571"),
                    _ => None,
                };
                if let Some(c) = nist {
                    s.push_str(&format!("{}NIST CURVE: {}\n", pad2, c));
                }
            }
        }
        Some(PKey::Dsa(f)) => {
            // do_dsa_print(), public part
            s.push_str(&format!("{}Public-Key: ({} bit)\n", pad2, f.p.bits()));
            if let Some(y) = &f.pub_key {
                s.push_str(&bn_print("pub: ", y, indent + 4));
            }
            s.push_str(&bn_print("P:   ", &f.p, indent + 4));
            if let Some(q) = &f.q {
                s.push_str(&bn_print("Q:   ", q, indent + 4));
            }
            s.push_str(&bn_print("G:   ", &f.g, indent + 4));
        }
        Some(k) => {
            s.push_str(&format!("{}Public-Key: ({} bit)\n", pad2, k.bits()));
        }
        None => s.push_str(&format!("{}Unable to load Public Key\n", pad2)),
    }
    s
}


/// Two's-complement big-endian INTEGER content as (negative, magnitude).
fn signed_int(b: &[u8]) -> (bool, Bn) {
    if b.first().is_some_and(|&c| c & 0x80 != 0) {
        // magnitude = ~b + 1
        let mut m: Vec<u8> = b.iter().map(|&c| !c).collect();
        for c in m.iter_mut().rev() {
            let (v, carry) = c.overflowing_add(1);
            *c = v;
            if !carry {
                break;
            }
        }
        (true, Bn::from_be(&m))
    } else {
        (false, Bn::from_be(b))
    }
}
/// ASN1_STRING_print(): printable ASCII (and CR/LF) as is, the rest as '.'.
fn asn1_string_print(v: &[u8]) -> String {
    v.iter()
        .map(|&c| {
            if c > b'~' || (c < b' ' && c != b'\n' && c != b'\r') {
                '.'
            } else {
                c as char
            }
        })
        .collect()
}

/// x509v3_add_len_value() refuses an IA5 GeneralName value with an
/// embedded NUL, which makes the whole SAN/IAN print fail.
fn gennames_have_nul(value: &[u8]) -> bool {
    asn1::parse_one(value)
        .and_then(|t| t.children())
        .is_some_and(|names| {
            names.iter().any(|g| {
                matches!(g.tag, 0x81 | 0x82 | 0x86)
                    && g.content.len() > 1
                    && g.content[..g.content.len() - 1].contains(&0)
            })
        })
}

/// X509V3_extensions_print() body: each extension's name line, then
/// X509V3_EXT_print() at `indent + 4` (ASN1_STRING_print() of the raw
/// value when it cannot be printed) and a newline.
fn print_exts(exts: &[Ext], indent: usize) -> String {
    let mut s = String::new();
    let pad = " ".repeat(indent + 4);
    for e in exts {
        s.push_str(&format!(
            "{}{}: {}\n",
            " ".repeat(indent),
            obj::oid_ln(&e.oid),
            if e.critical { "critical" } else { "" }
        ));
        let alt = matches!(e.oid.as_str(), "2.5.29.17" | "2.5.29.18");
        let body = if alt && gennames_have_nul(&e.value) {
            None
        } else {
            x509v3::ext_print(e)
        };
        match body {
            Some(b) => {
                // The body is X509V3_EXT_print() at indent 0; indent its
                // lines (an i2r printer's trailing newline is kept).
                let b = String::from_utf8_lossy(&b).to_string();
                let mut first = true;
                for line in b.split('\n') {
                    if !first {
                        s.push('\n');
                    }
                    first = false;
                    if !line.is_empty() {
                        s.push_str(&pad);
                        s.push_str(line);
                    }
                }
            }
            None => {
                s.push_str(&pad);
                s.push_str(&asn1_string_print(&e.value));
            }
        }
        s.push('\n');
    }
    s
}

/// ossl_serial_number_print(): decimal and hex when it fits an int64,
/// otherwise the magnitude's bytes on the next line.
fn serial_text(serial: &[u8]) -> String {
    if serial.is_empty() {
        return " (Empty)".into();
    }
    let (neg, mag) = signed_int(serial);
    let mb = mag.be();
    let fits = mb.len() < 8 || (mb.len() == 8 && (mb[0] & 0x80 == 0 || (neg && mb[0] == 0x80 && mb[1..].iter().all(|&b| b == 0))));
    if fits {
        let mut v: u64 = 0;
        for &b in mb {
            v = (v << 8) | b as u64;
        }
        let n = if neg { "-" } else { "" };
        return format!(" {n}{v} ({n}0x{v:x})");
    }
    let hex: Vec<String> = mb.iter().map(|b| format!("{:02x}", b)).collect();
    format!(
        "\n{}{}{}",
        " ".repeat(12),
        if neg { " (Negative)" } else { "" },
        hex.join(":")
    )
}

fn sig_alg_oid(alg: &[u8]) -> String {
    asn1::parse_one(alg)
        .and_then(|t| t.children())
        .and_then(|c| c.first().and_then(|o| o.oid()))
        .unwrap_or_default()
}

/// X509_signature_dump(): 18 bytes per line.
fn sig_dump(b: &[u8], indent: usize) -> String {
    let mut out = String::new();
    for (i, x) in b.iter().enumerate() {
        if i % 18 == 0 {
            if i > 0 {
                out.push('\n');
            }
            out.push_str(&" ".repeat(indent));
        }
        out.push_str(&format!("{:02x}", x));
        if i + 1 != b.len() {
            out.push(':');
        }
    }
    out.push('\n');
    out
}

/// ASN1_bn_print()
fn bn_print(label: &str, n: &Bn, indent: usize) -> String {
    let pad = " ".repeat(indent);
    if n.is_zero() {
        return format!("{pad}{label} 0\n");
    }
    let b = n.be();
    if b.len() <= 8 {
        let mut v: u64 = 0;
        for &x in b {
            v = (v << 8) | x as u64;
        }
        return format!("{pad}{label} {v} (0x{v:x})\n");
    }
    let mut nb = b.to_vec();
    if nb[0] & 0x80 != 0 {
        nb.insert(0, 0);
    }
    format!("{pad}{label}\n{}", hex_block(&nb, indent + 4))
}

/// X509_signature_print(): the algorithm, then (with a signature) the
/// key type's sig_print or a plain dump.
fn signature_print(alg: &[u8], sig: Option<&[u8]>) -> String {
    let oid = sig_alg_oid(alg);
    let mut s = format!("    Signature Algorithm: {}", obj::oid_ln(&oid));
    if sig.is_some() {
        s.push_str("\n    Signature Value:");
    }
    s.push('\n');
    let Some(sig) = sig else { return s };
    let is_dsa = matches!(
        oid.as_str(),
        "1.2.840.10040.4.3" | "2.16.840.1.101.3.4.3.1" | "2.16.840.1.101.3.4.3.2"
            | "2.16.840.1.101.3.4.3.3" | "2.16.840.1.101.3.4.3.4" | "2.16.840.1.101.3.4.3.5"
            | "2.16.840.1.101.3.4.3.6" | "2.16.840.1.101.3.4.3.7" | "2.16.840.1.101.3.4.3.8"
            | "1.3.14.3.2.27"
    );
    if is_dsa {
        // dsa_sig_print()
        let rs = asn1::parse_one(sig).and_then(|t| {
            let c = t.children()?;
            (t.tag == asn1::SEQUENCE && c.len() == 2 && t.raw.len() == sig.len())
                .then(|| (c[0].uint(), c[1].uint()))
        });
        if let Some((r, ss)) = rs {
            s.push_str(&bn_print("r:   ", &r, 8));
            s.push_str(&bn_print("s:   ", &ss, 8));
            return s;
        }
    }
    s.push_str(&sig_dump(sig, 8));
    s
}

/// X509_print() (XN_FLAG_COMPAT names, X509_FLAG_COMPAT).
pub fn x509_print(c: &Cert) -> String {
    let mut s = String::from("Certificate:\n    Data:\n");
    if (0..=2).contains(&c.version) {
        s.push_str(&format!(
            "        Version: {} (0x{:x})\n",
            c.version + 1,
            c.version
        ));
    } else {
        s.push_str(&format!("        Version: Unknown ({})\n", c.version));
    }
    s.push_str(&format!("        Serial Number:{}\n", serial_text(&c.serial)));
    s.push_str("    ");
    s.push_str(&signature_print(&c.tbs_sig_alg, None));
    s.push_str(&format!(
        "        Issuer: {}\n",
        x509v3::name_print_compat(&c.issuer)
    ));
    s.push_str("        Validity\n");
    s.push_str(&format!(
        "            Not Before: {}\n",
        x509v3::time_print(c.not_before.0, &c.not_before.1)
    ));
    s.push_str(&format!(
        "            Not After : {}\n",
        x509v3::time_print(c.not_after.0, &c.not_after.1)
    ));
    s.push_str(&format!(
        "        Subject: {}\n",
        x509v3::name_print_compat(&c.subject)
    ));
    s.push_str("        Subject Public Key Info:\n");
    s.push_str(&print_pubkey(c.public_key(), &c.spki, 12));
    if !c.extensions.is_empty() {
        s.push_str("        X509v3 extensions:\n");
        s.push_str(&print_exts(&c.extensions, 12));
    }
    s.push_str(&signature_print(&c.sig_alg, Some(&c.signature)));
    s
}

/// X509_REQ_print() (XN_FLAG_COMPAT names, X509_FLAG_COMPAT).
pub fn x509_req_print(c: &Csr) -> String {
    let mut s = String::from("Certificate Request:\n    Data:\n");
    if c.version == 0 {
        s.push_str("        Version: 1 (0x0)\n");
    } else {
        s.push_str(&format!("        Version: Unknown ({})\n", c.version));
    }
    s.push_str(&format!(
        "        Subject: {}\n",
        x509v3::name_print_compat(&c.subject)
    ));
    s.push_str("        Subject Public Key Info:\n");
    s.push_str(&print_pubkey(c.public_key(), &c.spki, 12));
    s.push_str("        Attributes:\n");
    let is_ext_req = |oid: &str| oid == "1.2.840.113549.1.9.14" || oid == "1.3.6.1.4.1.311.2.1.14";
    if c.attributes.is_empty() {
        s.push_str("            (none)\n");
    }
    for (oid, vals) in &c.attributes {
        if is_ext_req(oid) {
            continue;
        }
        let name = obj::oid_ln(oid);
        let values = asn1::parse_all(vals).unwrap_or_default();
        let mut width = name.len();
        s.push_str("            ");
        s.push_str(&name);
        for (i, t) in values.iter().enumerate() {
            if i > 0 {
                width = 0;
            }
            s.push_str(&" ".repeat(25usize.saturating_sub(width)));
            s.push(':');
            if matches!(t.tag, 0x13 | 0x14 | 0x12 | 0x0c | 0x16) {
                s.push_str(&String::from_utf8_lossy(t.content));
                s.push('\n');
            } else {
                s.push_str("unable to print attribute\n");
            }
        }
    }
    if c.attributes.iter().any(|(o, _)| is_ext_req(o)) {
        s.push_str("            Requested Extensions:\n");
        s.push_str(&print_exts(&c.extensions(), 16));
    }
    s.push_str(&signature_print(&c.sig_alg, Some(&c.signature)));
    s
}

// -------------------------------------------------------------------------
// openssl_x509_* functions
// -------------------------------------------------------------------------

/// A certificate argument that failed with a pending exception (null
/// bytes in a path, failed string conversion) still reaches the caller's
/// "X.509 Certificate cannot be retrieved" warning in php-src; the user
/// error handler cannot run while the exception is pending.
pub fn cert_failed<T>(vm: &mut Vm, f: &str, r: Result<T, VmError>) -> Result<T, VmError> {
    if r.is_err() && vm.current_exception.is_some() && vm.error_handler.is_none() {
        vm.emit_warning(&format!("{}(): X.509 Certificate cannot be retrieved", f));
    }
    r
}

pub fn openssl_x509_read(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let f = "openssl_x509_read";
    let a = cert_param(vm, args, 0, f, "certificate", false)?;
    let r = cert_from_arg(vm, f, 1, a);
    match cert_failed(vm, f, r)? {
        Some(c) => Ok(make_cert_object(vm, Rc::new((*c).clone()))),
        None => {
            vm.emit_warning(&format!("{}(): X.509 Certificate cannot be retrieved", f));
            Ok(Value::False)
        }
    }
}

pub fn openssl_x509_free(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let f = "openssl_x509_free";
    vm.emit_function_deprecation(
        "Function openssl_x509_free() is deprecated since 8.0, as OpenSSLCertificate objects are freed automatically",
    )?;
    let args = &vm.zpp_self_check(b"openssl_x509_free", args)?[..];
    let v = args.first().cloned().unwrap_or(Value::Null);
    if !class_is(&v, "OpenSSLCertificate") {
        return Err(zpp::arg_type_error(
            vm,
            f,
            1,
            "certificate",
            "OpenSSLCertificate",
            &v.deref(),
        ));
    }
    Ok(Value::Null)
}

fn bn_hex(v: &(bool, Bn)) -> String {
    let (neg, mag) = v;
    let mut s: String = if mag.is_zero() {
        "0".to_string()
    } else {
        mag.be().iter().map(|b| format!("{:02X}", b)).collect()
    };
    if *neg && !mag.is_zero() {
        s.insert(0, '-');
    }
    s
}

/// i2s_ASN1_INTEGER()
fn i2s_integer(v: &(bool, Bn)) -> String {
    let (neg, mag) = v;
    let neg = *neg && !mag.is_zero();
    if mag.bits() < 128 {
        format!("{}{}", if neg { "-" } else { "" }, mag.to_dec())
    } else {
        let hex: String = mag.be().iter().map(|b| format!("{:02X}", b)).collect();
        if neg {
            format!("-0x{}", hex)
        } else {
            format!("0x{}", hex)
        }
    }
}

fn c_atoi(s: &[u8]) -> i64 {
    let mut i = 0;
    while i < s.len() && (s[i] == b' ' || (9..=13).contains(&s[i])) {
        i += 1;
    }
    let mut neg = false;
    if i < s.len() && (s[i] == b'+' || s[i] == b'-') {
        neg = s[i] == b'-';
        i += 1;
    }
    let mut v: i64 = 0;
    while i < s.len() && s[i].is_ascii_digit() {
        v = v.wrapping_mul(10).wrapping_add((s[i] - b'0') as i64);
        i += 1;
    }
    if neg { -v } else { v }
}

/// php_openssl_asn1_time_to_time_t()
fn asn1_time_to_time_t(vm: &mut Vm, f: &str, tag: u8, v: &[u8]) -> i64 {
    if tag != asn1::UTC_TIME && tag != asn1::GENERALIZED_TIME {
        vm.emit_warning(&format!("{}(): Illegal ASN1 data type for timestamp", f));
        return -1;
    }
    if v.contains(&0) {
        vm.emit_warning(&format!("{}(): Illegal length in timestamp", f));
        return -1;
    }
    let len = v.len();
    if len < 13 || (tag == asn1::GENERALIZED_TIME && len < 15) {
        vm.emit_warning(&format!(
            "{}(): Unable to parse time string {} correctly",
            f,
            String::from_utf8_lossy(v)
        ));
        return -1;
    }
    let mut buf = v.to_vec();
    let mut p = len - 3;
    let sec = c_atoi(&buf[p..]);
    buf.truncate(p);
    p -= 2;
    let min = c_atoi(&buf[p..]);
    buf.truncate(p);
    p -= 2;
    let hour = c_atoi(&buf[p..]);
    buf.truncate(p);
    p -= 2;
    let mday = c_atoi(&buf[p..]);
    buf.truncate(p);
    p -= 2;
    let mon = c_atoi(&buf[p..]) - 1;
    buf.truncate(p);
    let year = if tag == asn1::UTC_TIME {
        p -= 2;
        let mut y = c_atoi(&buf[p..]);
        if y < 68 {
            y += 100;
        }
        y + 1900
    } else {
        p -= 4;
        c_atoi(&buf[p..])
    };
    // timegm() with normalisation of out-of-range fields
    let total_months = year * 12 + mon;
    let y = total_months.div_euclid(12);
    let m = total_months.rem_euclid(12) + 1;
    let days = cert::days_from_civil(y, m, 1) + mday - 1;
    days * 86400 + hour * 3600 + min * 60 + sec
}

fn key(s: &str) -> ArrayKey {
    ArrayKey::String(PhpString::from_bytes(s.as_bytes()))
}

fn sval(s: &str) -> Value {
    Value::String(PhpString::from_bytes(s.as_bytes()))
}

fn arr(a: PhpArray) -> Value {
    Value::Array(Rc::new(RefCell::new(a)))
}

fn vbool(b: bool) -> Value {
    if b { Value::True } else { Value::False }
}

pub fn openssl_x509_parse(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let f = "openssl_x509_parse";
    let a = cert_param(vm, args, 0, f, "certificate", false)?;
    let short = if zpp::is_passed(args, 1) {
        zpp::bool(vm, args, 1, f, "short_names")?
    } else {
        true
    };
    let Some(c) = cert_from_arg(vm, f, 1, a)? else {
        return Ok(Value::False);
    };
    let mut r = PhpArray::new();
    r.set(key("name"), sval(&c.subject.oneline()));
    let mut subj = PhpArray::new();
    c.subject.add_to_array(&mut subj, short);
    r.set(key("subject"), arr(subj));
    r.set(key("hash"), sval(&format!("{:08x}", c.subject.hash())));
    let mut iss = PhpArray::new();
    c.issuer.add_to_array(&mut iss, short);
    r.set(key("issuer"), arr(iss));
    r.set(key("version"), Value::Long(c.version));
    let serial = signed_int(&c.serial);
    r.set(key("serialNumber"), sval(&i2s_integer(&serial)));
    r.set(key("serialNumberHex"), sval(&bn_hex(&serial)));
    r.set(key("validFrom"), zpp::bytes(c.not_before.1.clone()));
    r.set(key("validTo"), zpp::bytes(c.not_after.1.clone()));
    let vf = asn1_time_to_time_t(vm, f, c.not_before.0, &c.not_before.1);
    r.set(key("validFrom_time_t"), Value::Long(vf));
    let vt = asn1_time_to_time_t(vm, f, c.not_after.0, &c.not_after.1);
    r.set(key("validTo_time_t"), Value::Long(vt));
    let (sn, ln, nid) = match obj::by_oid(&c.sig_alg_oid) {
        Some(o) => (o.sn, o.ln, o.nid),
        None => ("UNDEF".to_string(), "undefined".to_string(), 0),
    };
    r.set(key("signatureTypeSN"), sval(&sn));
    r.set(key("signatureTypeLN"), sval(&ln));
    r.set(key("signatureTypeNID"), Value::Long(nid));
    let mut purposes = PhpArray::new();
    for (id, sname, lname) in x509v3::PURPOSES {
        let mut sub = PhpArray::new();
        sub.push(vbool(x509v3::check_purpose(&c, *id, false) != 0));
        sub.push(vbool(x509v3::check_purpose(&c, *id, true) != 0));
        sub.push(sval(if short { sname } else { lname }));
        purposes.set(ArrayKey::Int(*id), arr(sub));
    }
    r.set(key("purposes"), arr(purposes));
    let mut exts = PhpArray::new();
    for e in &c.extensions {
        let extname = match obj::by_oid(&e.oid) {
            Some(o) if o.nid != 0 => o.sn,
            _ => e.oid.clone(),
        };
        let val = if e.oid == x509v3::OID_SAN {
            match x509v3::subject_alt_name(&e.value) {
                Some(v) => v,
                None => return Ok(Value::False),
            }
        } else {
            x509v3::ext_print(e).unwrap_or_else(|| e.value.clone())
        };
        exts.set(key(&extname), zpp::bytes(val));
    }
    r.set(key("extensions"), arr(exts));
    Ok(arr(r))
}

pub fn openssl_x509_export(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let f = "openssl_x509_export";
    let a = cert_param(vm, args, 0, f, "certificate", false)?;
    let notext = if zpp::is_passed(args, 2) {
        zpp::bool(vm, args, 2, f, "no_text")?
    } else {
        true
    };
    let r = cert_from_arg(vm, f, 1, a);
    let Some(c) = cert_failed(vm, f, r)? else {
        vm.emit_warning(&format!("{}(): X.509 Certificate cannot be retrieved", f));
        return Ok(Value::False);
    };
    let mut out = String::new();
    if !notext {
        out.push_str(&x509_print(&c));
    }
    out.push_str(&c.pem());
    zpp::set_ref(args, 1, Value::String(PhpString::from_string(out)));
    Ok(Value::True)
}

pub fn openssl_x509_export_to_file(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let f = "openssl_x509_export_to_file";
    let a = cert_param(vm, args, 0, f, "certificate", false)?;
    let filename = zpp::path(vm, args, 1, f, "output_filename")?;
    let notext = if zpp::is_passed(args, 2) {
        zpp::bool(vm, args, 2, f, "no_text")?
    } else {
        true
    };
    let r = cert_from_arg(vm, f, 1, a);
    let Some(c) = cert_failed(vm, f, r)? else {
        vm.emit_warning(&format!("{}(): X.509 Certificate cannot be retrieved", f));
        return Ok(Value::False);
    };
    let Some(path) = check_plain_path(vm, f, 2, filename.as_bytes(), false, None)? else {
        return Ok(Value::False);
    };
    let mut out = String::new();
    if !notext {
        out.push_str(&x509_print(&c));
    }
    out.push_str(&c.pem());
    match std::fs::write(&path, out) {
        Ok(()) => Ok(Value::True),
        Err(e) => {
            err::bio_new_file_failed(&e);
            vm.emit_warning(&format!("{}(): Error opening file {}", f, path));
            Ok(Value::False)
        }
    }
}

pub fn openssl_x509_fingerprint(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let f = "openssl_x509_fingerprint";
    let a = cert_param(vm, args, 0, f, "certificate", false)?;
    let method = if zpp::is_passed(args, 1) {
        zpp::str(vm, args, 1, f, "digest_algo")?.to_string_lossy()
    } else {
        "sha1".to_string()
    };
    let raw = if zpp::is_passed(args, 2) {
        zpp::bool(vm, args, 2, f, "binary")?
    } else {
        false
    };
    let r = cert_from_arg(vm, f, 1, a);
    let Some(c) = cert_failed(vm, f, r)? else {
        vm.emit_warning(&format!("{}(): X.509 Certificate cannot be retrieved", f));
        return Ok(Value::False);
    };
    let Some(md) = Md::by_name(&method) else {
        vm.emit_warning(&format!("{}(): Unknown digest algorithm", f));
        return Ok(Value::False);
    };
    let d = md.digest(&c.der);
    if raw {
        Ok(zpp::bytes(d))
    } else {
        Ok(sval(&crate::hex_lower(&d)))
    }
}

pub fn openssl_x509_check_private_key(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let f = "openssl_x509_check_private_key";
    let a = cert_param(vm, args, 0, f, "certificate", false)?;
    let key_arg = args.get(1).cloned().unwrap_or(Value::Null);
    let Some(c) = cert_from_arg(vm, f, 1, a)? else {
        return Ok(Value::False);
    };
    let Some(k) = pkey_fns::from_zval(vm, f, &key_arg, false, Some(&[]), 2)? else {
        return Ok(Value::False);
    };
    Ok(vbool(check_private_key(&c.spki, &k)))
}

/// X509_check_private_key()
pub(crate) fn check_private_key(spki: &[u8], k: &PKey) -> bool {
    let theirs = k.spki_der();
    let ok = theirs == spki;
    if !ok {
        // EVP_PKEY_eq(): -1 for keys of different types.
        let alg = |d: &[u8]| {
            asn1::parse_one(d)
                .and_then(|t| t.children())
                .and_then(|c| c.first().and_then(|a| a.children()))
                .and_then(|a| a.first().and_then(|o| o.oid()))
        };
        if alg(&theirs) != alg(spki) {
            err::push(err::LIB_EVP, 101, "different key types");
            err::push(err::LIB_X509, 115, "key type mismatch");
        } else {
            err::push(err::LIB_X509, 116, "key values mismatch");
        }
    }
    ok
}

pub fn openssl_x509_verify(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let f = "openssl_x509_verify";
    let a = cert_param(vm, args, 0, f, "certificate", false)?;
    let key_arg = args.get(1).cloned().unwrap_or(Value::Null);
    let Some(c) = cert_from_arg(vm, f, 1, a)? else {
        return Ok(Value::Long(-1));
    };
    let Some(k) = pkey_fns::from_zval(vm, f, &key_arg, true, None, 2)? else {
        return Ok(Value::Long(-1));
    };
    Ok(Value::Long(c.verify(&k)))
}

// -------------------------------------------------------------------------
// Chain verification (X509_verify_cert subset)
// -------------------------------------------------------------------------

pub struct Store {
    pub certs: Vec<Rc<Cert>>,
    pub dirs: Vec<String>,
}

impl Store {
    pub fn lookup(&self, subject: &Name) -> Vec<Rc<Cert>> {
        let mut v: Vec<Rc<Cert>> = self
            .certs
            .iter()
            .filter(|c| c.subject.eq_canon(subject))
            .cloned()
            .collect();
        for d in &self.dirs {
            let h = subject.hash();
            for i in 0..10 {
                let p = format!("{}/{:08x}.{}", d, h, i);
                let Ok(data) = std::fs::read(&p) else { break };
                for b in asn1::pem_blocks(&data) {
                    if b.label.contains("CERTIFICATE") {
                        if let Some(c) = Cert::parse(&b.der) {
                            if c.subject.eq_canon(subject) {
                                v.push(Rc::new(c));
                            }
                        }
                    }
                }
            }
        }
        v
    }

    pub fn contains(&self, c: &Cert) -> bool {
        self.lookup(&c.subject).iter().any(|x| x.der == c.der)
    }
}

/// Load every certificate of a PEM file (X509_LOOKUP_load_file).
pub fn load_pem_certs(path: &str) -> Option<Vec<Rc<Cert>>> {
    let data = std::fs::read(path).ok()?;
    let mut out = Vec::new();
    for b in asn1::pem_blocks(&data) {
        if b.label == "CERTIFICATE"
            || b.label == "X509 CERTIFICATE"
            || b.label == "TRUSTED CERTIFICATE"
        {
            if let Some(c) = Cert::parse(&b.der) {
                out.push(Rc::new(c));
            }
        }
    }
    Some(out)
}

thread_local! {
    static DEFAULT_CERTS: RefCell<Option<Vec<Rc<Cert>>>> = const { RefCell::new(None) };
}

pub fn default_cert_file() -> String {
    std::env::var("SSL_CERT_FILE").unwrap_or_else(|_| "/etc/ssl/cert.pem".to_string())
}

pub fn default_cert_dir() -> String {
    std::env::var("SSL_CERT_DIR").unwrap_or_else(|_| "/etc/ssl/certs".to_string())
}

/// php_openssl_setup_verify()
pub fn setup_verify(
    vm: &mut Vm,
    f: &str,
    calist: Option<&PhpArray>,
    arg_num: usize,
) -> Result<Option<Store>, VmError> {
    let mut store = Store {
        certs: Vec::new(),
        dirs: Vec::new(),
    };
    let mut nfiles = 0;
    let mut ndirs = 0;
    if let Some(list) = calist {
        let items: Vec<Value> = list.iter().map(|(_, v)| v.clone()).collect();
        for item in items {
            let s = vm.value_to_string(&item.deref());
            if vm.current_exception.is_some() {
                return Err(VmError {
                    message: "exception".into(),
                    line: vm.current_line,
                });
            }
            let Some(path) = check_plain_path(vm, f, arg_num, s.as_bytes(), true, None)? else {
                continue;
            };
            match std::fs::metadata(&path) {
                Err(_) => {
                    vm.emit_warning(&format!("{}(): Unable to stat {}", f, path));
                    continue;
                }
                Ok(md) => {
                    if md.is_file() {
                        match load_pem_certs(&path) {
                            Some(c) if !c.is_empty() => {
                                store.certs.extend(c);
                                nfiles += 1;
                            }
                            _ => {
                                err::push(err::LIB_X509, 124, "no certificate or crl found");
                                vm.emit_warning(&format!("{}(): Error loading file {}", f, path));
                            }
                        }
                    } else {
                        store.dirs.push(path);
                        ndirs += 1;
                    }
                }
            }
        }
    }
    if nfiles == 0 {
        let certs = DEFAULT_CERTS.with(|d| {
            let mut d = d.borrow_mut();
            if d.is_none() {
                *d = Some(load_pem_certs(&default_cert_file()).unwrap_or_default());
            }
            d.clone().unwrap_or_default()
        });
        store.certs.extend(certs);
    }
    if ndirs == 0 {
        store.dirs.push(default_cert_dir());
    }
    Ok(Some(store))
}

/// X509_verify_cert() with a purpose: 1 valid, 0 invalid.
pub fn verify_chain(store: &Store, leaf: &Rc<Cert>, untrusted: &[Rc<Cert>], purpose: i64) -> i32 {
    let mut chain: Vec<Rc<Cert>> = vec![leaf.clone()];
    while !store.contains(leaf) {
        let cur = chain.last().unwrap().clone();
        let fl = x509v3::ex_flags(&cur);
        if fl.self_signed {
            return 0;
        }
        let issued_by = |cand: &Rc<Cert>| -> bool {
            cand.subject.eq_canon(&cur.issuer)
                && cand
                    .public_key()
                    .map(|k| cur.verify(&k) == 1)
                    .unwrap_or(false)
        };
        if let Some(c) = store.lookup(&cur.issuer).into_iter().find(issued_by) {
            chain.push(c);
            break;
        }
        match untrusted
            .iter()
            .find(|c| issued_by(c) && !chain.iter().any(|x| x.der == c.der))
        {
            Some(c) => chain.push(c.clone()),
            None => return 0,
        }
        if chain.len() > 100 {
            return 0;
        }
    }
    // signatures
    for i in 0..chain.len() {
        let last = i + 1 == chain.len();
        if last && !chain[i].subject.eq_canon(&chain[i].issuer) {
            continue;
        }
        let issuer = if last { &chain[i] } else { &chain[i + 1] };
        let Some(k) = issuer.public_key() else {
            return 0;
        };
        if chain[i].verify(&k) != 1 {
            return 0;
        }
    }
    // validity
    let now = cert::now();
    for c in &chain {
        let nb = cert::asn1_time_to_unix(c.not_before.0, &c.not_before.1);
        let na = cert::asn1_time_to_unix(c.not_after.0, &c.not_after.1);
        match (nb, na) {
            (Some(nb), Some(na)) if nb <= now && now <= na => {}
            _ => return 0,
        }
    }
    // CA and purpose checks
    for (i, c) in chain.iter().enumerate() {
        if i > 0 {
            let fl = x509v3::ex_flags(c);
            if fl.bcons && !fl.ca {
                return 0;
            }
        }
        if purpose > 0 && x509v3::check_purpose(c, purpose, i > 0) <= 0 {
            return 0;
        }
    }
    1
}

/// X509_verify_cert() for a TLS peer, returning the verify result
/// (X509_V_OK = 0, or the X509_V_* code of the failure / of the last
/// error the callback forgave). `check_cert` is the verify_callback: it is
/// called with each error (and with 0 for every certificate of the chain)
/// and its depth; Ok forgives/accepts, Err(code) aborts with `code`.
pub fn verify_peer_chain(
    store: &Store,
    leaf: &Rc<Cert>,
    untrusted: &[Rc<Cert>],
    check_cert: &mut dyn FnMut(i32, usize, &Cert) -> Result<(), i32>,
) -> i32 {
    let mut last_err = 0;
    macro_rules! report {
        ($err:expr, $depth:expr, $c:expr) => {
            match check_cert($err, $depth, $c) {
                Ok(()) => {
                    if $err != 0 {
                        last_err = $err;
                    }
                }
                Err(code) => return code,
            }
        };
    }
    let mut chain: Vec<Rc<Cert>> = vec![leaf.clone()];
    let mut trusted = store.contains(leaf);
    while !trusted {
        let cur = chain.last().unwrap().clone();
        let issued_by = |cand: &Rc<Cert>| -> bool {
            cand.subject.eq_canon(&cur.issuer)
                && cand
                    .public_key()
                    .map(|k| cur.verify(&k) == 1)
                    .unwrap_or(false)
        };
        if let Some(c) = store.lookup(&cur.issuer).into_iter().find(issued_by) {
            chain.push(c);
            trusted = true;
            break;
        }
        let fl = x509v3::ex_flags(&cur);
        if fl.self_signed {
            let depth = chain.len() - 1;
            report!(if depth == 0 { 18 } else { 19 }, depth, &cur);
            break;
        }
        match untrusted
            .iter()
            .find(|c| issued_by(c) && !chain.iter().any(|x| x.der == c.der))
        {
            Some(c) => chain.push(c.clone()),
            None => {
                let depth = chain.len() - 1;
                report!(if chain.len() == 1 { 21 } else { 20 }, depth, &cur);
                break;
            }
        }
        if chain.len() > 100 {
            return 22;
        }
    }
    let now = cert::now();
    for (i, c) in chain.iter().enumerate() {
        let last = i + 1 == chain.len();
        // signature (a trust anchor's own signature is not checked)
        if !last {
            let ok = chain[i + 1]
                .public_key()
                .map(|k| c.verify(&k) == 1)
                .unwrap_or(false);
            if !ok {
                report!(7, i, c);
            }
        }
        let nb = cert::asn1_time_to_unix(c.not_before.0, &c.not_before.1);
        let na = cert::asn1_time_to_unix(c.not_after.0, &c.not_after.1);
        if let (Some(nb), Some(na)) = (nb, na) {
            if now < nb {
                report!(9, i, c);
            }
            if now > na {
                report!(10, i, c);
            }
        }
        if i > 0 && !(last && trusted) {
            let fl = x509v3::ex_flags(c);
            if fl.bcons && !fl.ca {
                report!(24, i, c);
            }
        }
        report!(0, i, c);
    }
    last_err
}

/// X509_verify_cert_error_string()
pub fn verify_error_string(code: i32) -> &'static str {
    match code {
        0 => "ok",
        2 => "unable to get issuer certificate",
        7 => "certificate signature failure",
        9 => "certificate is not yet valid",
        10 => "certificate has expired",
        18 => "self-signed certificate",
        19 => "self-signed certificate in certificate chain",
        20 => "unable to get local issuer certificate",
        21 => "unable to verify the first certificate",
        22 => "certificate chain too long",
        24 => "invalid CA certificate",
        26 => "unsupported certificate purpose",
        66 => "EE certificate key too weak",
        67 => "CA certificate key too weak",
        _ => "unspecified certificate verification error",
    }
}

pub fn load_all_certs_from_file(
    vm: &mut Vm,
    f: &str,
    file: &[u8],
    arg_num: usize,
) -> Result<Option<Vec<Rc<Cert>>>, VmError> {
    let Some(path) = check_plain_path(vm, f, arg_num, file, false, None)? else {
        return Ok(None);
    };
    let data = match std::fs::read(&path) {
        Ok(d) => d,
        Err(e) => {
            err::bio_new_file_failed(&e);
            vm.emit_warning(&format!("{}(): Error opening the file, {}", f, path));
            return Ok(None);
        }
    };
    let mut out = Vec::new();
    for b in asn1::pem_blocks(&data) {
        if b.label == "CERTIFICATE"
            || b.label == "X509 CERTIFICATE"
            || b.label == "TRUSTED CERTIFICATE"
        {
            if let Some(c) = Cert::parse(&b.der) {
                out.push(Rc::new(c));
            }
        }
    }
    if out.is_empty() {
        vm.emit_warning(&format!("{}(): No certificates in file, {}", f, path));
        return Ok(None);
    }
    Ok(Some(out))
}

pub fn openssl_x509_checkpurpose(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let f = "openssl_x509_checkpurpose";
    let a = cert_param(vm, args, 0, f, "certificate", false)?;
    let purpose = zpp::long(vm, args, 1, f, "purpose")?;
    let cainfo = if zpp::is_passed(args, 2) {
        Some(zpp::array(vm, args, 2, f, "ca_info")?)
    } else {
        None
    };
    let untrusted = zpp::str_nullable(vm, args, 3, f, "untrusted_certificates_file")?;
    let mut chain = Vec::new();
    if let Some(u) = &untrusted {
        match load_all_certs_from_file(vm, f, u.as_bytes(), 4)? {
            Some(c) => chain = c,
            None => return Ok(Value::Long(-1)),
        }
    }
    let ca = cainfo.map(|c| c.borrow().clone());
    let Some(store) = setup_verify(vm, f, ca.as_ref(), 3)? else {
        return Ok(Value::Long(-1));
    };
    let Some(c) = cert_from_arg(vm, f, 1, a)? else {
        return Ok(Value::Long(-1));
    };
    if purpose >= 0 && !(1..=9).contains(&purpose) {
        err::push(err::LIB_X509, 121, "unknown purpose id");
    }
    let r = verify_chain(&store, &c, &chain, purpose);
    Ok(vbool(r == 1))
}

// -------------------------------------------------------------------------
// CSR functions
// -------------------------------------------------------------------------

fn csr_add_entry(
    vm: &mut Vm,
    f: &str,
    subj: &mut Name,
    o: &obj::Obj,
    item: &Value,
    mask: u64,
) -> Result<bool, VmError> {
    let s = vm.value_to_string(&item.deref());
    if vm.current_exception.is_some() {
        return Err(VmError {
            message: "exception".into(),
            line: vm.current_line,
        });
    }
    match name::make_string(&o.oid, s.as_bytes(), mask) {
        Ok((tag, value)) => {
            subj.push(Ava {
                oid: o.oid.clone(),
                tag,
                value,
            });
            Ok(true)
        }
        Err(e) => {
            name::push_str_err(&e);
            vm.emit_warning(&format!(
                "{}(): dn: add_entry_by_NID {} -> {} (failed; check error queue and value of string_mask OpenSSL option if illegal characters are reported)",
                f,
                o.nid,
                s.to_string_lossy()
            ));
            Ok(false)
        }
    }
}

fn attr_der(oid: &str, value: &[u8], mask: u64) -> Result<Vec<u8>, name::StrErr> {
    let (tag, v) = name::make_string(oid, value, mask)?;
    Ok(asn1::seq(&[
        asn1::oid(oid),
        asn1::set(&[asn1::tlv(tag, &v)]),
    ]))
}

type CsrParts = (Name, Vec<(String, Vec<u8>)>);

/// php_openssl_csr_make(): Ok(Some((subject, attributes))) or Ok(None) on FAILURE.
fn csr_make(
    vm: &mut Vm,
    f: &str,
    r: &req::Req,
    dn: &PhpArray,
    attribs: Option<&PhpArray>,
) -> Result<Option<CsrParts>, VmError> {
    let Some(dn_sect) = r.conf.get_string(Some(&r.section), "distinguished_name") else {
        err::push(err::LIB_CONF, 108, "no value");
        return Ok(None);
    };
    let Some(dn_sk) = r.conf.section(&dn_sect).cloned() else {
        err::push(err::LIB_CONF, 109, "no section");
        return Ok(None);
    };
    let attr_sk = match r.conf.get_string(Some(&r.section), "attributes") {
        None => None,
        Some(s) => match r.conf.section(&s) {
            Some(v) => Some(v.clone()),
            None => {
                err::push(err::LIB_CONF, 109, "no section");
                return Ok(None);
            }
        },
    };
    let mask = r.string_mask;
    let mut subj = Name::default();
    let entries: Vec<(ArrayKey, Value)> = dn.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    for (k, item) in entries {
        let ArrayKey::String(ks) = k else { continue };
        let kname = ks.to_string_lossy();
        match obj::txt2obj(&kname).filter(|o| o.nid != 0) {
            Some(o) => {
                if let Value::Array(sub) = item.deref() {
                    let subs: Vec<Value> = sub.borrow().iter().map(|(_, v)| v.clone()).collect();
                    for s in subs {
                        if !csr_add_entry(vm, f, &mut subj, &o, &s, mask)? {
                            return Ok(None);
                        }
                    }
                } else if !csr_add_entry(vm, f, &mut subj, &o, &item, mask)? {
                    return Ok(None);
                }
            }
            None => {
                err::obj_unknown_name(&kname);
                vm.emit_warning(&format!("{}(): dn: {} is not a recognized name", f, kname))
            }
        }
    }
    for (name_, value) in &dn_sk {
        if name_.len() < 9 || !name_.ends_with("_default") {
            continue;
        }
        let mut t = &name_[..name_.len() - 8];
        if t.len() > 200 {
            t = &t[..200];
        }
        let mut ty = t;
        if let Some(pos) = t.find([':', ',', '.']) {
            if pos + 1 < t.len() {
                ty = &t[pos + 1..];
            }
        }
        let o = obj::txt2obj(ty);
        if let Some(o) = &o {
            if o.nid != 0 && subj.has_oid(&o.oid) {
                continue;
            }
        }
        let Some(o) = o else {
            // OBJ_txt2nid(), then X509_NAME_add_entry_by_txt()
            err::obj_unknown_name(ty);
            err::obj_unknown_name(ty);
            err::push(err::LIB_X509, 119, "invalid field name");
            vm.emit_warning(&format!(
                "{}(): add_entry_by_txt {} -> {} (failed)",
                f, ty, value
            ));
            return Ok(None);
        };
        match name::make_string(&o.oid, value.as_bytes(), mask) {
            Ok((tag, v)) => subj.push(Ava {
                oid: o.oid.clone(),
                tag,
                value: v,
            }),
            Err(e) => {
                name::push_str_err(&e);
                vm.emit_warning(&format!(
                    "{}(): add_entry_by_txt {} -> {} (failed)",
                    f, ty, value
                ));
                return Ok(None);
            }
        }
        if subj.entry_count() == 0 {
            vm.emit_warning(&format!("{}(): No objects specified in config file", f));
            return Ok(None);
        }
    }
    let mut attrs: Vec<(String, Vec<u8>)> = Vec::new();
    if let Some(at) = attribs {
        let entries: Vec<(ArrayKey, Value)> =
            at.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        for (k, item) in entries {
            let ArrayKey::String(ks) = k else {
                vm.emit_warning(&format!(
                    "{}(): attributes: numeric fild names are not supported",
                    f
                ));
                continue;
            };
            let kname = ks.to_string_lossy();
            match obj::txt2obj(&kname).filter(|o| o.nid != 0) {
                Some(o) => {
                    let s = vm.value_to_string(&item.deref());
                    if vm.current_exception.is_some() {
                        return Err(VmError {
                            message: "exception".into(),
                            line: vm.current_line,
                        });
                    }
                    match attr_der(&o.oid, s.as_bytes(), mask) {
                        Ok(d) => attrs.push((o.oid.clone(), d)),
                        Err(e) => {
                            name::push_str_err(&e);
                            vm.emit_warning(&format!(
                                "{}(): attributes: add_attr_by_NID {} -> {} (failed)",
                                f,
                                o.nid,
                                s.to_string_lossy()
                            ));
                            return Ok(None);
                        }
                    }
                }
                None => {
                    err::obj_unknown_name(&kname);
                    vm.emit_warning(&format!(
                        "{}(): attributes: {} is not a recognized attribute name",
                        f, kname
                    ))
                }
            }
        }
        if let Some(sk) = &attr_sk {
            for (n, v) in sk {
                let o = obj::txt2obj(n);
                if let Some(o) = &o {
                    if o.nid != 0 && attrs.iter().any(|(oid, _)| *oid == o.oid) {
                        continue;
                    }
                }
                let res = match &o {
                    Some(o) => attr_der(&o.oid, v.as_bytes(), mask)
                        .map(|d| (o.oid.clone(), d))
                        .map_err(|e| {
                            name::push_str_err(&e);
                        }),
                    None => {
                        // OBJ_txt2nid(), then X509_REQ_add1_attr_by_txt()
                        err::obj_unknown_name(n);
                        err::obj_unknown_name(n);
                        err::push(err::LIB_X509, 119, "invalid field name");
                        Err(())
                    }
                };
                match res {
                    Ok(a) => attrs.push(a),
                    Err(()) => {
                        vm.emit_warning(&format!(
                            "{}(): add1_attr_by_txt {} -> {} (failed; check error queue and value of string_mask OpenSSL option if illegal characters are reported)",
                            f, n, v
                        ));
                        return Ok(None);
                    }
                }
            }
        }
    }
    Ok(Some((subj, attrs)))
}

fn build_csr(
    subject: &Name,
    spki: &[u8],
    attrs: &[Vec<u8>],
    key: &PKey,
    md: Option<Md>,
) -> Option<Csr> {
    let mut a = attrs.to_vec();
    a.sort();
    let info = asn1::seq(&[
        asn1::int_i64(0),
        subject.to_der(),
        spki.to_vec(),
        asn1::tlv(0xa0, &a.concat()),
    ]);
    let (alg, sig) = cert::sign_tbs(&info, key, md)?;
    let der = asn1::seq(&[info, alg, asn1::bitstr(&sig)]);
    Csr::parse(&der)
}

fn req_digest(r: &req::Req) -> Option<Md> {
    if r.md_null { None } else { r.md }
}

pub fn openssl_csr_new(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let f = "openssl_csr_new";
    let dn = zpp::array(vm, args, 0, f, "distinguished_names")?;
    let opts = zpp::array_nullable(vm, args, 2, f, "options")?;
    let attribs = zpp::array_nullable(vm, args, 3, f, "extra_attributes")?;
    let opts_arr = opts.map(|a| a.borrow().clone());
    let Some(r) = req::parse(vm, opts_arr.as_ref()) else {
        return Ok(Value::False);
    };
    let out_val = args.get(1).map(|v| v.deref()).unwrap_or(Value::Null);
    let mut key: Option<PKey> = None;
    // php-src keeps going after php_openssl_pkey_from_zval() threw.
    let mut pending: Option<VmError> = None;
    if !matches!(out_val, Value::Null) {
        match pkey_fns::from_zval(vm, f, &out_val, false, None, 2) {
            Ok(k) => key = k.map(|k| (*k).clone()),
            Err(e) => pending = Some(e),
        }
    }
    let saved_exc = if pending.is_some() {
        vm.current_exception.take()
    } else {
        None
    };
    let res = csr_new_rest(vm, args, f, &r, key, dn, attribs);
    if let Some(e) = pending {
        vm.current_exception = saved_exc;
        return Err(e);
    }
    res
}

fn csr_new_rest(
    vm: &mut Vm,
    args: &[Value],
    f: &str,
    r: &req::Req,
    mut key: Option<PKey>,
    dn: Rc<RefCell<PhpArray>>,
    attribs: Option<Rc<RefCell<PhpArray>>>,
) -> Result<Value, VmError> {
    let mut we_made_the_key = false;
    if key.is_none() {
        key = pkey_fns::generate_private_key(vm, f, r);
        we_made_the_key = true;
    }
    let Some(key) = key else {
        vm.emit_warning(&format!("{}(): Unable to generate a private key", f));
        return Ok(Value::False);
    };
    let dn_arr = dn.borrow().clone();
    let at_arr = attribs.map(|a| a.borrow().clone());
    let Some((subject, mut attrs)) = csr_make(vm, f, r, &dn_arr, at_arr.as_ref())? else {
        return Ok(Value::False);
    };
    let spki = key.spki_der();
    if let Some(sect) = &r.request_extensions_section {
        let mut exts = Vec::new();
        let mut ctx = V3Ctx {
            test: false,
            issuer: None,
            subject_pubkey_bits: cert::spki_bits(&spki),
            subject_name: Some(&subject),
            subject_exts: &[],
            self_signed: false,
            self_issuer_name: None,
            self_serial: None,
        };
        if !x509v3::add_section(&r.conf, &mut ctx, sect, &mut exts) {
            vm.emit_warning(&format!(
                "{}(): Error loading extension section {}",
                f, sect
            ));
            return Ok(Value::False);
        }
        let ext_der = cert::encode_extensions(&exts);
        attrs.push((
            "1.2.840.113549.1.9.14".to_string(),
            asn1::seq(&[asn1::oid("1.2.840.113549.1.9.14"), asn1::set(&[ext_der])]),
        ));
    }
    let attr_ders: Vec<Vec<u8>> = attrs.iter().map(|(_, d)| d.clone()).collect();
    let result = match build_csr(&subject, &spki, &attr_ders, &key, req_digest(r)) {
        Some(csr) => make_csr_object(vm, csr),
        None => {
            vm.emit_warning(&format!("{}(): Error signing request", f));
            Value::True
        }
    };
    if we_made_the_key {
        let obj = pkey_fns::make_object(vm, key, true);
        zpp::set_ref(args, 1, obj);
    }
    Ok(result)
}

pub fn openssl_csr_export(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let f = "openssl_csr_export";
    let a = csr_param(vm, args, 0, f, "csr")?;
    let notext = if zpp::is_passed(args, 2) {
        zpp::bool(vm, args, 2, f, "no_text")?
    } else {
        true
    };
    let Some(c) = csr_from_arg(vm, f, 1, a)? else {
        vm.emit_warning(&format!(
            "{}(): X.509 Certificate Signing Request cannot be retrieved",
            f
        ));
        return Ok(Value::False);
    };
    let mut out = String::new();
    if !notext {
        out.push_str(&x509_req_print(&c));
    }
    out.push_str(&c.pem());
    zpp::set_ref(args, 1, Value::String(PhpString::from_string(out)));
    Ok(Value::True)
}

pub fn openssl_csr_export_to_file(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let f = "openssl_csr_export_to_file";
    let a = csr_param(vm, args, 0, f, "csr")?;
    let filename = zpp::path(vm, args, 1, f, "output_filename")?;
    let notext = if zpp::is_passed(args, 2) {
        zpp::bool(vm, args, 2, f, "no_text")?
    } else {
        true
    };
    let Some(c) = csr_from_arg(vm, f, 1, a)? else {
        vm.emit_warning(&format!(
            "{}(): X.509 Certificate Signing Request cannot be retrieved",
            f
        ));
        return Ok(Value::False);
    };
    let Some(path) = check_plain_path(vm, f, 2, filename.as_bytes(), false, None)? else {
        return Ok(Value::False);
    };
    let mut out = String::new();
    if !notext {
        out.push_str(&x509_req_print(&c));
    }
    out.push_str(&c.pem());
    match std::fs::write(&path, out) {
        Ok(()) => Ok(Value::True),
        Err(e) => {
            err::bio_new_file_failed(&e);
            vm.emit_warning(&format!("{}(): Error opening file {}", f, path));
            Ok(Value::False)
        }
    }
}

pub fn openssl_csr_get_subject(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let f = "openssl_csr_get_subject";
    let a = csr_param(vm, args, 0, f, "csr")?;
    let short = if zpp::is_passed(args, 1) {
        zpp::bool(vm, args, 1, f, "short_names")?
    } else {
        true
    };
    let Some(c) = csr_from_arg(vm, f, 1, a)? else {
        return Ok(Value::False);
    };
    let mut r = PhpArray::new();
    c.subject.add_to_array(&mut r, short);
    Ok(arr(r))
}

pub fn openssl_csr_get_public_key(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let f = "openssl_csr_get_public_key";
    let a = csr_param(vm, args, 0, f, "csr")?;
    if zpp::is_passed(args, 1) {
        zpp::bool(vm, args, 1, f, "short_names")?;
    }
    let Some(c) = csr_from_arg(vm, f, 1, a)? else {
        return Ok(Value::False);
    };
    match c.public_key() {
        Some(k) => Ok(pkey_fns::make_object(vm, k.public_only(), false)),
        None => Ok(Value::False),
    }
}

/// a2i_ASN1_INTEGER() on a single string.
/// a2i_ASN1_INTEGER() on the first line; Err(reason) as it raises.
fn parse_hex_serial(s: &[u8]) -> Result<Vec<u8>, (u32, &'static str)> {
    const SHORT: (u32, &str) = (150, "short line");
    let line = &s[..s.iter().position(|&c| c == b'\n').map_or(s.len(), |p| p + 1)];
    let mut i = line.len();
    if i > 0 && line[i - 1] == b'\n' {
        i -= 1;
    }
    if i == 0 {
        return Err(SHORT);
    }
    if line[i - 1] == b'\r' {
        i -= 1;
    }
    if i == 0 {
        return Err(SHORT);
    }
    let again = line[i - 1] == b'\\';
    let j = line[..i].iter().take_while(|c| c.is_ascii_hexdigit()).count();
    let mut buf = &line[..j];
    if buf.len() < 2 {
        return Err(SHORT);
    }
    if buf[0] == b'0' && buf[1] == b'0' {
        buf = &buf[2..];
    }
    if again {
        // a continued line has no next line here (one-line BIO)
        return Err(SHORT);
    }
    if !buf.len().is_multiple_of(2) {
        return Err((145, "odd number of chars"));
    }
    let out: Vec<u8> = buf
        .chunks(2)
        .map(|c| u8::from_str_radix(std::str::from_utf8(c).unwrap_or("0"), 16).unwrap_or(0))
        .collect();
    let mut b = Bn::from_be(&out).to_vec();
    if b.is_empty() {
        b.push(0);
    }
    if b[0] & 0x80 != 0 {
        b.insert(0, 0);
    }
    Ok(b)
}

fn int_content(v: i64) -> Vec<u8> {
    let d = asn1::int_i64(v);
    d[2..].to_vec()
}

pub fn openssl_csr_sign(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let f = "openssl_csr_sign";
    let csr_a = csr_param(vm, args, 0, f, "csr")?;
    let ca_a = cert_param(vm, args, 1, f, "ca_certificate", true)?;
    let key_arg = args.get(2).cloned().unwrap_or(Value::Null);
    let days = zpp::long(vm, args, 3, f, "days")?;
    let opts = zpp::array_nullable(vm, args, 4, f, "options")?;
    let serial = if zpp::is_passed(args, 5) {
        zpp::long(vm, args, 5, f, "serial")?
    } else {
        0
    };
    let serial_hex = zpp::str_nullable(vm, args, 6, f, "serial_hex")?;
    let Some(csr) = csr_from_arg(vm, f, 1, csr_a)? else {
        vm.emit_warning(&format!(
            "{}(): X.509 Certificate Signing Request cannot be retrieved",
            f
        ));
        return Ok(Value::False);
    };
    let mut ca: Option<Rc<Cert>> = None;
    if ca_a.is_some() {
        match cert_from_arg(vm, f, 2, ca_a)? {
            Some(c) => ca = Some(c),
            None => {
                vm.emit_warning(&format!("{}(): X.509 Certificate cannot be retrieved", f));
                return Ok(Value::False);
            }
        }
    }
    let Some(priv_key) = pkey_fns::from_zval(vm, f, &key_arg, false, Some(&[]), 3)? else {
        if vm.current_exception.is_none() {
            vm.emit_warning(&format!("{}(): Cannot get private key from parameter 3", f));
        }
        return Ok(Value::False);
    };
    if let Some(c) = &ca {
        if !check_private_key(&c.spki, &priv_key) {
            vm.emit_warning(&format!(
                "{}(): Private key does not correspond to signing cert",
                f
            ));
            return Ok(Value::False);
        }
    }
    let max_days = i64::MAX / 86400;
    if days < 0 || days > max_days {
        vm.emit_warning(&format!("{}(): Days must be between 0 and {}", f, max_days));
        return Ok(Value::False);
    }
    let opts_arr = opts.map(|a| a.borrow().clone());
    let Some(r) = req::parse(vm, opts_arr.as_ref()) else {
        return Ok(Value::False);
    };
    let Some(req_key) = csr.public_key() else {
        err::push(err::LIB_X509, 129, "unknown public key type");
        vm.emit_warning(&format!("{}(): Error unpacking public key", f));
        return Ok(Value::False);
    };
    match csr.verify(&req_key) {
        v if v < 0 => {
            vm.emit_warning(&format!("{}(): Signature verification problems", f));
            return Ok(Value::False);
        }
        0 => {
            vm.emit_warning(&format!(
                "{}(): Signature did not match the certificate request",
                f
            ));
            return Ok(Value::False);
        }
        _ => {}
    }
    let serial_content = if let Some(h) = &serial_hex {
        if h.len() > 200 {
            vm.emit_warning(&format!(
                "{}(): Error parsing serial number because it is too long",
                f
            ));
            return Ok(Value::False);
        }
        match parse_hex_serial(h.as_bytes()) {
            Ok(s) => s,
            Err((reason, text)) => {
                err::push(err::LIB_ASN1, reason, text);
                vm.emit_warning(&format!("{}(): Error parsing serial number", f));
                return Ok(Value::False);
            }
        }
    } else {
        int_content(serial)
    };
    let subject = csr.subject.clone();
    let issuer_name = match &ca {
        Some(c) => c.subject.clone(),
        None => subject.clone(),
    };
    let now = cert::now();
    let nb = cert::asn1_time_from_unix(now);
    let na = cert::asn1_time_from_unix(now.saturating_add(days.saturating_mul(86400)));
    let spki = req_key.spki_der();
    let mut exts: Vec<Ext> = Vec::new();
    if let Some(sect) = &r.extensions_section {
        let mut ctx = V3Ctx {
            test: false,
            issuer: ca.as_deref(),
            subject_pubkey_bits: cert::spki_bits(&spki),
            subject_name: Some(&subject),
            subject_exts: &[],
            self_signed: ca.is_none(),
            self_issuer_name: Some(&issuer_name),
            self_serial: Some(&serial_content),
        };
        if !x509v3::add_section(&r.conf, &mut ctx, sect, &mut exts) {
            return Ok(Value::False);
        }
    }
    let md = req_digest(&r);
    let Some(alg) = cert::sig_alg_for(&priv_key, md) else {
        err::push(err::LIB_ASN1, 199, "unknown signature algorithm");
        vm.emit_warning(&format!("{}(): Failed to sign it", f));
        return Ok(Value::False);
    };
    let mut tbs_items = vec![
        asn1::explicit(0, &asn1::int_i64(2)),
        asn1::tlv(asn1::INTEGER, &serial_content),
        alg.clone(),
        issuer_name.to_der(),
        asn1::seq(&[asn1::tlv(nb.0, &nb.1), asn1::tlv(na.0, &na.1)]),
        subject.to_der(),
        spki,
    ];
    if !exts.is_empty() {
        tbs_items.push(asn1::explicit(3, &cert::encode_extensions(&exts)));
    }
    let tbs = asn1::seq(&tbs_items);
    let Ok(sig) = priv_key.sign(md, &tbs, false) else {
        vm.emit_warning(&format!("{}(): Failed to sign it", f));
        return Ok(Value::False);
    };
    let der = asn1::seq(&[tbs, alg, asn1::bitstr(&sig)]);
    match Cert::parse(&der) {
        Some(c) => Ok(make_cert_object(vm, Rc::new(c))),
        None => Ok(Value::False),
    }
}

// -------------------------------------------------------------------------
// SPKI (Netscape SPKAC)
// -------------------------------------------------------------------------

struct Spki {
    spkac: Vec<u8>,
    spki: Vec<u8>,
    challenge: Vec<u8>,
    sig_alg: Vec<u8>,
    sig: Vec<u8>,
}

/// EVP_DecodeBlock(): leading blanks and trailing non-base64 bytes are
/// trimmed, the rest must be whole 4-character groups ('=' decodes as 0).
fn evp_decode_block(f: &[u8]) -> Option<Vec<u8>> {
    fn val(c: u8) -> u8 {
        match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => 0,
            b'\t' | b' ' => 0xE0,
            b'\n' => 0xF0,
            b'\r' => 0xF1,
            b'-' => 0xF2,
            _ => 0xFF,
        }
    }
    let mut f = f;
    while !f.is_empty() && val(f[0]) == 0xE0 {
        f = &f[1..];
    }
    let mut n = f.len();
    while n > 3 && (val(f[n - 1]) | 0x13) == 0xF3 {
        n -= 1;
    }
    if n % 4 != 0 {
        return None;
    }
    let mut out = Vec::with_capacity(n / 4 * 3);
    for g in f[..n].chunks(4) {
        let v: Vec<u8> = g.iter().map(|&c| val(c)).collect();
        if (v[0] | v[1] | v[2] | v[3]) & 0x80 != 0 {
            return None;
        }
        let l = (v[0] as u32) << 18 | (v[1] as u32) << 12 | (v[2] as u32) << 6 | v[3] as u32;
        out.extend_from_slice(&[(l >> 16) as u8, (l >> 8) as u8, l as u8]);
    }
    if n >= 4 {
        let pad = (f[n - 1] == b'=') as usize + (f[n - 2] == b'=') as usize;
        out.truncate(out.len() - pad);
    }
    Some(out)
}

fn spki_decode(b64: &[u8]) -> Option<Spki> {
    let Some(der) = evp_decode_block(b64) else {
        // NETSCAPE_SPKI_b64_decode()
        err::push(err::LIB_X509, 118, "base64 decode error");
        return None;
    };
    let r = spki_parse(&der);
    if r.is_none() {
        err::push(err::LIB_ASN1, err::R_UNSUPPORTED, "unsupported");
    }
    r
}

/// d2i_NETSCAPE_SPKI()
fn spki_parse(der: &[u8]) -> Option<Spki> {
    let t = asn1::parse_one(der)?;
    let c = t.children()?;
    if c.len() != 3 {
        return None;
    }
    let pkac = c[0].children()?;
    if pkac.len() != 2 {
        return None;
    }
    Some(Spki {
        spkac: c[0].raw.to_vec(),
        spki: pkac[0].raw.to_vec(),
        challenge: pkac[1].content.to_vec(),
        sig_alg: c[1].raw.to_vec(),
        sig: c[2].bits()?.to_vec(),
    })
}

pub fn openssl_spki_new(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let f = "openssl_spki_new";
    let kv = args.first().cloned().unwrap_or(Value::Null);
    if !pkey_fns::is_pkey_object(&kv) {
        return Err(zpp::arg_type_error(
            vm,
            f,
            1,
            "private_key",
            "OpenSSLAsymmetricKey",
            &kv.deref(),
        ));
    }
    let challenge = zpp::str(vm, args, 1, f, "challenge")?;
    let algo = if zpp::is_passed(args, 2) {
        zpp::long(vm, args, 2, f, "digest_algo")?
    } else {
        2
    };
    let Some(key) = pkey_fns::from_zval(vm, f, &kv, false, Some(challenge.as_bytes()), 1)? else {
        if vm.current_exception.is_none() {
            vm.emit_warning(&format!("{}(): Unable to use supplied private key", f));
        }
        return Ok(Value::False);
    };
    let Some(md) = pkey_fns::md_from_algo(algo) else {
        vm.emit_warning(&format!("{}(): Unknown digest algorithm", f));
        return Ok(Value::False);
    };
    let pkac = asn1::seq(&[
        key.spki_der(),
        asn1::tlv(asn1::IA5_STRING, challenge.as_bytes()),
    ]);
    let Some((alg, sig)) = cert::sign_tbs(&pkac, &key, Some(md)) else {
        vm.emit_warning(&format!(
            "{}(): Unable to sign with specified digest algorithm",
            f
        ));
        return Ok(Value::False);
    };
    let der = asn1::seq(&[pkac, alg, asn1::bitstr(&sig)]);
    Ok(sval(&format!("SPKAC={}", crate::base64_encode(&der))))
}

fn spki_input(vm: &mut Vm, args: &[Value], f: &str) -> Result<Option<Spki>, VmError> {
    let s = zpp::str(vm, args, 0, f, "spki")?;
    let cleaned: Vec<u8> = s
        .as_bytes()
        .iter()
        .copied()
        .filter(|&c| c != b'\n' && c != b'\r')
        .collect();
    if cleaned.is_empty() {
        vm.emit_warning(&format!("{}(): Invalid SPKAC", f));
        return Ok(None);
    }
    match spki_decode(&cleaned) {
        Some(x) => Ok(Some(x)),
        None => {
            let msg = if f == "openssl_spki_export_challenge" {
                "Unable to decode SPKAC"
            } else {
                "Unable to decode supplied SPKAC"
            };
            vm.emit_warning(&format!("{}(): {}", f, msg));
            Ok(None)
        }
    }
}

pub fn openssl_spki_verify(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let f = "openssl_spki_verify";
    let Some(sp) = spki_input(vm, args, f)? else {
        return Ok(Value::False);
    };
    let Some(k) = crate::pkey::parse_spki(&sp.spki) else {
        vm.emit_warning(&format!("{}(): Unable to acquire signed public key", f));
        return Ok(Value::False);
    };
    Ok(vbool(
        cert::verify_signed(&sp.spkac, &sp.sig_alg, &sp.sig, &k) == 1,
    ))
}

pub fn openssl_spki_export(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let f = "openssl_spki_export";
    let Some(sp) = spki_input(vm, args, f)? else {
        return Ok(Value::False);
    };
    let Some(k) = crate::pkey::parse_spki(&sp.spki) else {
        vm.emit_warning(&format!("{}(): Unable to acquire signed public key", f));
        return Ok(Value::False);
    };
    Ok(sval(&k.public_pem()))
}

pub fn openssl_spki_export_challenge(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let f = "openssl_spki_export_challenge";
    let Some(sp) = spki_input(vm, args, f)? else {
        return Ok(Value::False);
    };
    let end = sp
        .challenge
        .iter()
        .position(|&b| b == 0)
        .unwrap_or(sp.challenge.len());
    Ok(zpp::bytes(sp.challenge[..end].to_vec()))
}
