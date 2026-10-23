//! PECL uuid 1.3: UUID generation and inspection.
//!
//! The PECL extension is a thin binding of util-linux's libuuid. This crate
//! is a clean-room implementation of the same PHP API: the UUID layouts and
//! algorithms come from RFC 9562 (which obsoletes RFC 4122), the library
//! semantics (parsing, comparison, type/variant/time decoding, which
//! generator each type uses) from libuuid's documented behaviour, and the
//! constant values from <uuid/uuid.h> (util-linux 2.41, which has the
//! version 6 and 7 generators).

use std::cell::Cell;

use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};

/// The extension name (`extension_loaded('uuid')`).
pub const NAME: &[u8] = b"uuid";
/// PHP_UUID_VERSION
pub const VERSION: &str = "1.3.0";

type R = Result<Value, VmError>;

// <uuid/uuid.h>
const UUID_VARIANT_NCS: i64 = 0;
const UUID_VARIANT_DCE: i64 = 1;
const UUID_VARIANT_MICROSOFT: i64 = 2;
const UUID_VARIANT_OTHER: i64 = 3;

const UUID_TYPE_DCE_TIME: i64 = 1;
const UUID_TYPE_DCE_SECURITY: i64 = 2;
const UUID_TYPE_DCE_MD5: i64 = 3;
const UUID_TYPE_DCE_RANDOM: i64 = 4;
const UUID_TYPE_DCE_SHA1: i64 = 5;
const UUID_TYPE_DCE_TIME_V6: i64 = 6;
const UUID_TYPE_DCE_TIME_V7: i64 = 7;
const UUID_TYPE_DCE_VENDOR: i64 = 8;

// The extension's own values.
const UUID_TYPE_DEFAULT: i64 = 0;
const UUID_TYPE_NULL: i64 = -1;
const UUID_TYPE_INVALID: i64 = -42;

const CONSTANTS: &[(&str, i64)] = &[
    ("UUID_VARIANT_NCS", UUID_VARIANT_NCS),
    ("UUID_VARIANT_DCE", UUID_VARIANT_DCE),
    ("UUID_VARIANT_MICROSOFT", UUID_VARIANT_MICROSOFT),
    ("UUID_VARIANT_OTHER", UUID_VARIANT_OTHER),
    ("UUID_TYPE_DEFAULT", UUID_TYPE_DEFAULT),
    // deprecated aliases: random and time based
    ("UUID_TYPE_DCE", UUID_TYPE_DCE_RANDOM),
    ("UUID_TYPE_NAME", UUID_TYPE_DCE_TIME),
    ("UUID_TYPE_TIME", UUID_TYPE_DCE_TIME),
    ("UUID_TYPE_TIME_V6", UUID_TYPE_DCE_TIME_V6),
    ("UUID_TYPE_TIME_V7", UUID_TYPE_DCE_TIME_V7),
    ("UUID_TYPE_VENDOR", UUID_TYPE_DCE_VENDOR),
    ("UUID_TYPE_SECURITY", UUID_TYPE_DCE_SECURITY),
    ("UUID_TYPE_MD5", UUID_TYPE_DCE_MD5),
    ("UUID_TYPE_RANDOM", UUID_TYPE_DCE_RANDOM),
    ("UUID_TYPE_SHA1", UUID_TYPE_DCE_SHA1),
    ("UUID_TYPE_NULL", UUID_TYPE_NULL),
    ("UUID_TYPE_INVALID", UUID_TYPE_INVALID),
];

const DEPRECATED_CONSTANTS: &[&str] = &["UUID_TYPE_DCE", "UUID_TYPE_NAME"];

const FUNCTIONS: &[(&str, goro_core::vm::BuiltinFn)] = &[
    ("uuid_create", uuid_create),
    ("uuid_is_valid", uuid_is_valid),
    ("uuid_compare", uuid_compare),
    ("uuid_is_null", uuid_is_null),
    ("uuid_generate_md5", uuid_generate_md5),
    ("uuid_generate_sha1", uuid_generate_sha1),
    ("uuid_type", uuid_type),
    ("uuid_variant", uuid_variant),
    ("uuid_time", uuid_time),
    ("uuid_mac", uuid_mac),
    ("uuid_parse", uuid_parse),
    ("uuid_unparse", uuid_unparse),
];

/// MINIT
pub fn register(vm: &mut Vm) {
    if vm.loaded_extensions.contains(NAME) {
        return;
    }
    vm.register_extension(NAME);
    vm.phpinfo_hooks.push(|_| {
        format!(
            "\nuuid\n\nUUID extension => enabled\nVersion => {} (stable)\nReleased => 2025-05-12\nAuthors => Hartmut Holzgraefe, Remi Collet\n",
            VERSION
        )
    });
    for &(name, f) in FUNCTIONS {
        let params: Vec<&[u8]> = goro_core::stubs::find_function(name.as_bytes())
            .map(|s| s.args.iter().map(|a| a.name.as_bytes()).collect())
            .unwrap_or_default();
        vm.register_function_with_params(name.as_bytes(), f, &params);
        vm.zpp_extension_verified.insert(
            name.as_bytes().to_vec(),
            goro_core::arginfo::CHECK_ARITY | goro_core::arginfo::CHECK_TYPES,
        );
    }
    for &(name, v) in CONSTANTS {
        vm.constants
            .insert(name.as_bytes().to_vec(), Value::Long(v));
    }
    for &name in DEPRECATED_CONSTANTS {
        vm.internal_deprecated_constants.insert(
            name.as_bytes().to_vec(),
            format!("Constant {} is deprecated", name),
        );
    }
    vm.module_constants.push((
        NAME.to_vec(),
        CONSTANTS
            .iter()
            .map(|(n, _)| n.as_bytes().to_vec())
            .collect(),
    ));
}

// ---------------------------------------------------------------------------
// The UUID model (RFC 9562 section 4: big-endian fields)
// ---------------------------------------------------------------------------

type Uuid = [u8; 16];

fn time_low(u: &Uuid) -> u32 {
    u32::from_be_bytes([u[0], u[1], u[2], u[3]])
}
fn time_mid(u: &Uuid) -> u16 {
    u16::from_be_bytes([u[4], u[5]])
}
fn time_hi_and_version(u: &Uuid) -> u16 {
    u16::from_be_bytes([u[6], u[7]])
}
fn clock_seq(u: &Uuid) -> u16 {
    u16::from_be_bytes([u[8], u[9]])
}

/// Set the version nibble and the RFC 9562 variant (0b10).
fn set_version(u: &mut Uuid, version: u8) {
    u[6] = (u[6] & 0x0f) | (version << 4);
    u[8] = (u[8] & 0x3f) | 0x80;
}

/// The textual form: exactly 36 characters, hyphens at 8, 13, 18 and 23 and
/// hex digits elsewhere. The argument is a C string to the library, so
/// anything after a NUL byte is not part of it.
fn parse(s: &[u8]) -> Option<Uuid> {
    let s = match s.iter().position(|&b| b == 0) {
        Some(n) => &s[..n],
        None => s,
    };
    if s.len() != 36 {
        return None;
    }
    let mut out = [0u8; 16];
    let mut j = 0;
    let mut i = 0;
    while i < 36 {
        if matches!(i, 8 | 13 | 18 | 23) {
            if s[i] != b'-' {
                return None;
            }
            i += 1;
            continue;
        }
        let hi = hexval(s[i])?;
        let lo = hexval(s[i + 1])?;
        out[j] = (hi << 4) | lo;
        j += 1;
        i += 2;
    }
    Some(out)
}

fn hexval(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

/// Lower-case textual form.
fn unparse(u: &Uuid) -> String {
    let mut s = String::with_capacity(36);
    for (i, b) in u.iter().enumerate() {
        if matches!(i, 4 | 6 | 8 | 10) {
            s.push('-');
        }
        s.push_str(&format!("{:02x}", b));
    }
    s
}

fn is_null(u: &Uuid) -> bool {
    u.iter().all(|&b| b == 0)
}

fn variant_of(u: &Uuid) -> i64 {
    let v = clock_seq(u);
    if v & 0x8000 == 0 {
        UUID_VARIANT_NCS
    } else if v & 0x4000 == 0 {
        UUID_VARIANT_DCE
    } else if v & 0x2000 == 0 {
        UUID_VARIANT_MICROSOFT
    } else {
        UUID_VARIANT_OTHER
    }
}

fn type_of(u: &Uuid) -> i64 {
    ((time_hi_and_version(u) >> 12) & 0xf) as i64
}

/// Field-wise comparison: the four integer fields, then the node bytes
/// with memcmp() semantics (the difference of the first differing bytes).
fn compare(a: &Uuid, b: &Uuid) -> i64 {
    use std::cmp::Ordering;
    let ord = time_low(a)
        .cmp(&time_low(b))
        .then(time_mid(a).cmp(&time_mid(b)))
        .then(time_hi_and_version(a).cmp(&time_hi_and_version(b)))
        .then(clock_seq(a).cmp(&clock_seq(b)));
    match ord {
        Ordering::Less => -1,
        Ordering::Greater => 1,
        Ordering::Equal => a[10..]
            .iter()
            .zip(&b[10..])
            .find(|(x, y)| x != y)
            .map_or(0, |(x, y)| *x as i64 - *y as i64),
    }
}

/// 100 ns intervals between 1582-10-15 (the Gregorian reform) and the Unix
/// epoch.
const GREGORIAN_OFFSET: u64 = 0x01B2_1DD2_1381_4000;

/// The creation time of a version 1 UUID, in seconds since the epoch.
fn time_v1(u: &Uuid) -> i64 {
    let ts = time_low(u) as u64
        | (time_mid(u) as u64) << 32
        | ((time_hi_and_version(u) & 0x0fff) as u64) << 48;
    (ts.wrapping_sub(GREGORIAN_OFFSET) as i64).div_euclid(10_000_000)
}

// ---------------------------------------------------------------------------
// Generators
// ---------------------------------------------------------------------------

fn random_bytes<const N: usize>() -> [u8; N] {
    let mut b = [0u8; N];
    goro_core::random::csprng_bytes(&mut b);
    b
}

/// Version 4: random.
fn generate_random() -> Uuid {
    let mut u: Uuid = random_bytes();
    set_version(&mut u, 4);
    u
}

fn now_micros() -> (u64, u64) {
    let d = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    (d.as_secs(), d.subsec_micros() as u64)
}

thread_local! {
    /// The clock state of the time based generator: last timestamp, the
    /// sub-microsecond adjustment and the clock sequence (random at start).
    static CLOCK: Cell<Option<(u64, u64, u16)>> = const { Cell::new(None) };
    static NODE: Cell<Option<[u8; 6]>> = const { Cell::new(None) };
}

/// Upper bound of the per-microsecond counter (the clock's 100 ns ticks).
const MAX_ADJUSTMENT: u64 = 10;

/// The Gregorian timestamp (100 ns units) and clock sequence of the next
/// time based UUID: unique and increasing within the process; the clock
/// sequence changes when the system clock goes backwards.
fn next_clock() -> (u64, u16) {
    loop {
        let (sec, usec) = now_micros();
        let now = sec * 10_000_000 + usec * 10 + GREGORIAN_OFFSET;
        let state = CLOCK.with(|c| c.get());
        let (last, adj, seq) = match state {
            Some(s) => s,
            None => {
                let r: [u8; 2] = random_bytes();
                (0, 0, u16::from_be_bytes(r) & 0x3fff)
            }
        };
        let (adj, seq) = if now < last {
            (0, (seq + 1) & 0x3fff)
        } else if now == last {
            if adj >= MAX_ADJUSTMENT - 1 {
                // more than 10 UUIDs this microsecond: wait for the next one
                std::thread::yield_now();
                continue;
            }
            (adj + 1, seq)
        } else {
            (0, seq)
        };
        CLOCK.with(|c| c.set(Some((now, adj, seq))));
        return (now + adj, seq);
    }
}

/// The node of version 1 UUIDs: the hardware address of a network
/// interface, or a random multicast address when there is none.
fn node_id() -> [u8; 6] {
    if let Some(n) = NODE.with(|n| n.get()) {
        return n;
    }
    let node = hardware_address().unwrap_or_else(|| {
        let mut n: [u8; 6] = random_bytes();
        n[0] |= 0x01;
        n
    });
    NODE.with(|n| n.set(Some(node)));
    node
}

/// The first (by interface index) non-loopback Ethernet-like interface
/// with a non-zero address.
fn hardware_address() -> Option<[u8; 6]> {
    let mut found: Vec<(i64, [u8; 6])> = Vec::new();
    for e in std::fs::read_dir("/sys/class/net").ok()?.flatten() {
        let p = e.path();
        let read = |f: &str| std::fs::read_to_string(p.join(f)).ok();
        if read("type").map(|t| t.trim().to_string()).as_deref() != Some("1") {
            continue;
        }
        let Some(addr) = read("address") else {
            continue;
        };
        let bytes: Vec<u8> = addr
            .trim()
            .split(':')
            .filter_map(|h| u8::from_str_radix(h, 16).ok())
            .collect();
        if bytes.len() != 6 || bytes.iter().all(|&b| b == 0) {
            continue;
        }
        let idx = read("ifindex")
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(i64::MAX);
        let mut n = [0u8; 6];
        n.copy_from_slice(&bytes);
        found.push((idx, n));
    }
    found.sort();
    found.first().map(|(_, n)| *n)
}

/// Version 1: Gregorian time, clock sequence, node.
fn generate_time() -> Uuid {
    let (ts, seq) = next_clock();
    let mut u = [0u8; 16];
    u[0..4].copy_from_slice(&(ts as u32).to_be_bytes());
    u[4..6].copy_from_slice(&((ts >> 32) as u16).to_be_bytes());
    u[6..8].copy_from_slice(&(((ts >> 48) as u16) & 0x0fff).to_be_bytes());
    u[8..10].copy_from_slice(&seq.to_be_bytes());
    u[10..16].copy_from_slice(&node_id());
    set_version(&mut u, 1);
    u
}

/// Version 6: the version 1 timestamp in big-endian order, random clock
/// sequence and node.
fn generate_time_v6() -> Uuid {
    let (ts, _) = next_clock();
    let mut u = [0u8; 16];
    u[0..4].copy_from_slice(&((ts >> 28) as u32).to_be_bytes());
    u[4..6].copy_from_slice(&((ts >> 12) as u16).to_be_bytes());
    u[6..8].copy_from_slice(&((ts & 0x0fff) as u16).to_be_bytes());
    let r: [u8; 8] = random_bytes();
    u[8..16].copy_from_slice(&r);
    set_version(&mut u, 6);
    u
}

/// Version 7: Unix time in milliseconds, then random bits.
fn generate_time_v7() -> Uuid {
    let (sec, usec) = now_micros();
    let ms = sec * 1000 + usec / 1000;
    let mut u: Uuid = random_bytes();
    u[0..6].copy_from_slice(&ms.to_be_bytes()[2..8]);
    set_version(&mut u, 7);
    u
}

/// Versions 3 and 5: hash of the namespace UUID followed by the name.
fn generate_name(algo: &str, version: u8, ns: &Uuid, name: &[u8]) -> Uuid {
    let mut data = Vec::with_capacity(16 + name.len());
    data.extend_from_slice(ns);
    data.extend_from_slice(name);
    let digest = goro_ext_hash::digest(algo, &data).unwrap_or_default();
    let mut u = [0u8; 16];
    u.copy_from_slice(&digest[..16]);
    set_version(&mut u, version);
    u
}

// ---------------------------------------------------------------------------
// PHP functions
// ---------------------------------------------------------------------------

fn arg(args: &[Value], i: usize) -> Value {
    match args.get(i) {
        Some(v) => v.deref(),
        None => Value::Undef,
    }
}

fn str_arg(args: &[Value], i: usize) -> PhpString {
    arg(args, i).to_php_string()
}

fn bool_val(b: bool) -> Value {
    if b { Value::True } else { Value::False }
}

fn string(s: String) -> Value {
    Value::String(PhpString::from_string(s))
}

/// zend_argument_value_error()
fn value_error(vm: &mut Vm, func: &str, n: usize, what: &str) -> R {
    Err(value_error_e(vm, func, n, what))
}

fn value_error_e(vm: &mut Vm, func: &str, n: usize, what: &str) -> VmError {
    let name = goro_core::stubs::find_function(func.as_bytes())
        .and_then(|f| f.args.get(n - 1))
        .map(|a| a.name)
        .unwrap_or("");
    let msg = format!("{}(): Argument #{} (${}) {}", func, n, name, what);
    let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
    vm.current_exception = Some(exc);
    VmError {
        message: msg,
        line: vm.current_line,
    }
}

/// The UUID argument #n, or the "UUID expected" ValueError.
fn uuid_arg(vm: &mut Vm, func: &str, args: &[Value], n: usize) -> Result<Uuid, VmError> {
    match parse(str_arg(args, n - 1).as_bytes()) {
        Some(u) => Ok(u),
        None => Err(value_error_e(vm, func, n, "UUID expected")),
    }
}

fn uuid_create(vm: &mut Vm, args: &[Value]) -> R {
    let ty = match arg(args, 0) {
        Value::Undef => UUID_TYPE_DEFAULT,
        v => v.to_long(),
    };
    let u = match ty {
        UUID_TYPE_DCE_TIME => generate_time(),
        UUID_TYPE_DCE_TIME_V6 => generate_time_v6(),
        UUID_TYPE_DCE_TIME_V7 => generate_time_v7(),
        // uuid_generate() uses the random generator when a random source
        // is available, which it always is here.
        UUID_TYPE_DCE_RANDOM | UUID_TYPE_DEFAULT => generate_random(),
        _ => {
            return value_error(
                vm,
                "uuid_create",
                1,
                &format!("Unknown/invalid UUID type '{}'", ty),
            );
        }
    };
    Ok(string(unparse(&u)))
}

fn uuid_is_valid(_vm: &mut Vm, args: &[Value]) -> R {
    Ok(bool_val(parse(str_arg(args, 0).as_bytes()).is_some()))
}

fn uuid_compare(vm: &mut Vm, args: &[Value]) -> R {
    let a = uuid_arg(vm, "uuid_compare", args, 1)?;
    let b = uuid_arg(vm, "uuid_compare", args, 2)?;
    Ok(Value::Long(compare(&a, &b)))
}

fn uuid_is_null(vm: &mut Vm, args: &[Value]) -> R {
    let u = uuid_arg(vm, "uuid_is_null", args, 1)?;
    Ok(bool_val(is_null(&u)))
}

fn uuid_generate_md5(vm: &mut Vm, args: &[Value]) -> R {
    let ns = uuid_arg(vm, "uuid_generate_md5", args, 1)?;
    let name = str_arg(args, 1);
    Ok(string(unparse(&generate_name(
        "md5",
        3,
        &ns,
        name.as_bytes(),
    ))))
}

fn uuid_generate_sha1(vm: &mut Vm, args: &[Value]) -> R {
    let ns = uuid_arg(vm, "uuid_generate_sha1", args, 1)?;
    let name = str_arg(args, 1);
    Ok(string(unparse(&generate_name(
        "sha1",
        5,
        &ns,
        name.as_bytes(),
    ))))
}

fn uuid_type(vm: &mut Vm, args: &[Value]) -> R {
    let u = uuid_arg(vm, "uuid_type", args, 1)?;
    if is_null(&u) {
        return Ok(Value::Long(UUID_TYPE_NULL));
    }
    Ok(Value::Long(type_of(&u)))
}

fn uuid_variant(vm: &mut Vm, args: &[Value]) -> R {
    let u = uuid_arg(vm, "uuid_variant", args, 1)?;
    if is_null(&u) {
        return Ok(Value::Long(UUID_TYPE_NULL));
    }
    Ok(Value::Long(variant_of(&u)))
}

/// A version 1 (DCE time) UUID argument, else the ValueError.
fn time_uuid_arg(vm: &mut Vm, func: &str, args: &[Value]) -> Result<Uuid, VmError> {
    match parse(str_arg(args, 0).as_bytes()) {
        Some(u) if variant_of(&u) == UUID_VARIANT_DCE && type_of(&u) == UUID_TYPE_DCE_TIME => Ok(u),
        _ => Err(value_error_e(vm, func, 1, "UUID DCE TIME expected")),
    }
}

fn uuid_time(vm: &mut Vm, args: &[Value]) -> R {
    let u = time_uuid_arg(vm, "uuid_time", args)?;
    Ok(Value::Long(time_v1(&u)))
}

fn uuid_mac(vm: &mut Vm, args: &[Value]) -> R {
    let u = time_uuid_arg(vm, "uuid_mac", args)?;
    Ok(string(unparse(&u)[24..].to_string()))
}

fn uuid_parse(vm: &mut Vm, args: &[Value]) -> R {
    let u = uuid_arg(vm, "uuid_parse", args, 1)?;
    Ok(Value::String(PhpString::from_bytes(&u)))
}

fn uuid_unparse(vm: &mut Vm, args: &[Value]) -> R {
    let s = str_arg(args, 0);
    let Ok(u) = <Uuid>::try_from(s.as_bytes()) else {
        return value_error(vm, "uuid_unparse", 1, "UUID expected");
    };
    Ok(string(unparse(&u)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_unparse() {
        let u = parse(b"B691C99C-7FC5-11D8-9FA8-00065B896488").unwrap();
        assert_eq!(unparse(&u), "b691c99c-7fc5-11d8-9fa8-00065b896488");
        assert!(parse(b"b691c99c-7fc5-11d8-9fa8-00065b89648").is_none());
        assert!(parse(b"b691c99c-7fc5-11d8-9fa8-00065b896488x").is_none());
        assert!(parse(b"b691c99c-7fc5-11d8-9fa8-00065b896488\0x").is_some());
        assert!(parse(b"b691c99c+7fc5-11d8-9fa8-00065b896488").is_none());
    }

    #[test]
    fn time_and_fields() {
        let u = parse(b"b691c99c-7fc5-11d8-9fa8-00065b896488").unwrap();
        assert_eq!(time_v1(&u), 1080374815);
        assert_eq!(type_of(&u), 1);
        assert_eq!(variant_of(&u), UUID_VARIANT_DCE);
    }

    #[test]
    fn name_based() {
        // RFC 9562 appendix A.2 / A.4 (DNS namespace, "www.example.com")
        let ns = parse(b"6ba7b810-9dad-11d1-80b4-00c04fd430c8").unwrap();
        assert_eq!(
            unparse(&generate_name("md5", 3, &ns, b"www.example.com")),
            "5df41881-3aed-3515-88a7-2f4a814cf09e"
        );
        assert_eq!(
            unparse(&generate_name("sha1", 5, &ns, b"www.example.com")),
            "2ed6657d-e927-568b-95e1-2665a8aea6a2"
        );
    }

    #[test]
    fn generated_versions() {
        for (u, v) in [
            (generate_random(), 4),
            (generate_time(), 1),
            (generate_time_v6(), 6),
            (generate_time_v7(), 7),
        ] {
            assert_eq!(type_of(&u), v);
            assert_eq!(variant_of(&u), UUID_VARIANT_DCE);
        }
        let a = generate_time();
        let b = generate_time();
        assert!(compare(&a, &b) != 0);
    }
}
