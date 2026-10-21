//! ext/standard/dns.c (dns_check_record(), dns_get_record(), dns_get_mx(),
//! gethostbyname(), gethostbynamel(), gethostbyaddr()) and the network
//! database / address conversion functions of ext/standard/basic_functions.c
//! (getservbyname(), getservbyport(), getprotobyname(), getprotobynumber(),
//! ip2long(), long2ip(), inet_pton(), inet_ntop()).
//!
//! php-src uses the system stub resolver (res_nsearch()/dn_expand()) for
//! the DNS functions.  This module carries a pure-Rust port of the glibc
//! stub resolver pieces PHP relies on: resolv.conf parsing (res_ninit),
//! the search-list logic (res_nsearch/res_nquerydomain), query building
//! (res_nmkquery/ns_name_pton), the UDP-with-TCP-fallback transport
//! (res_nsend), and name decompression (dn_expand/dn_skipname), so that
//! h_errno-driven results and warnings match PHP on Linux.

use goro_core::array::{ArrayKey, PhpArray};
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};
use std::cell::RefCell;
use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV6, TcpStream, UdpSocket};
use std::rc::Rc;
use std::time::{Duration, Instant};

// ---------------------------------------------------------------------------
// Constants (ext/standard/php_dns.h)
// ---------------------------------------------------------------------------

const PHP_DNS_A: i64 = 0x0000_0001;
const PHP_DNS_NS: i64 = 0x0000_0002;
const PHP_DNS_CNAME: i64 = 0x0000_0010;
const PHP_DNS_SOA: i64 = 0x0000_0020;
const PHP_DNS_PTR: i64 = 0x0000_0800;
const PHP_DNS_HINFO: i64 = 0x0000_1000;
const PHP_DNS_CAA: i64 = 0x0000_2000;
const PHP_DNS_MX: i64 = 0x0000_4000;
const PHP_DNS_TXT: i64 = 0x0000_8000;
const PHP_DNS_A6: i64 = 0x0100_0000;
const PHP_DNS_SRV: i64 = 0x0200_0000;
const PHP_DNS_NAPTR: i64 = 0x0400_0000;
const PHP_DNS_AAAA: i64 = 0x0800_0000;
const PHP_DNS_ANY: i64 = 0x1000_0000;
const PHP_DNS_NUM_TYPES: i32 = 13;
const PHP_DNS_ALL: i64 = PHP_DNS_A
    | PHP_DNS_NS
    | PHP_DNS_CNAME
    | PHP_DNS_SOA
    | PHP_DNS_PTR
    | PHP_DNS_HINFO
    | PHP_DNS_CAA
    | PHP_DNS_MX
    | PHP_DNS_TXT
    | PHP_DNS_A6
    | PHP_DNS_SRV
    | PHP_DNS_NAPTR
    | PHP_DNS_AAAA;

const DNS_T_A: u16 = 1;
const DNS_T_NS: u16 = 2;
const DNS_T_CNAME: u16 = 5;
const DNS_T_SOA: u16 = 6;
const DNS_T_PTR: u16 = 12;
const DNS_T_HINFO: u16 = 13;
const DNS_T_MX: u16 = 15;
const DNS_T_TXT: u16 = 16;
const DNS_T_AAAA: u16 = 28;
const DNS_T_SRV: u16 = 33;
const DNS_T_NAPTR: u16 = 35;
const DNS_T_A6: u16 = 38;
const DNS_T_ANY: u16 = 255;
const DNS_T_CAA: u16 = 257;
const C_IN: u16 = 1;

/// MAXFQDNLEN (main/php_network.h)
const MAXFQDNLEN: usize = 255;
/// MAXHOSTNAMELEN as redefined by dns.c
const MAXHOSTNAMELEN: usize = 1024;
const HFIXEDSZ: usize = 12;
const QFIXEDSZ: usize = 4;
/// sizeof(querybuf)
const ANSWER_SIZE: usize = 65536;

// netdb.h h_errno values
const HOST_NOT_FOUND: i32 = 1;
const TRY_AGAIN: i32 = 2;
const NO_RECOVERY: i32 = 3;
const NO_DATA: i32 = 4;

// DNS rcodes
const NOERROR: u8 = 0;
const FORMERR: u8 = 1;
const SERVFAIL: u8 = 2;
const NXDOMAIN: u8 = 3;
const NOTIMP: u8 = 4;
const REFUSED: u8 = 5;

pub fn register(vm: &mut Vm) {
    vm.register_function(b"dns_check_record", dns_check_record_fn);
    vm.register_function(b"checkdnsrr", checkdnsrr_fn);
    vm.register_function(b"dns_get_record", dns_get_record_fn);
    vm.register_function(b"dns_get_mx", dns_get_mx_fn);
    vm.register_function(b"getmxrr", getmxrr_fn);
    vm.register_function(b"gethostbyname", gethostbyname_fn);
    vm.register_function(b"gethostbynamel", gethostbynamel_fn);
    vm.register_function(b"gethostbyaddr", gethostbyaddr_fn);
    vm.register_function(b"getprotobyname", getprotobyname_fn);
    vm.register_function(b"getprotobynumber", getprotobynumber_fn);
    vm.register_function(b"getservbyname", getservbyname_fn);
    vm.register_function(b"getservbyport", getservbyport_fn);
    vm.register_function(b"ip2long", ip2long_fn);
    vm.register_function(b"long2ip", long2ip_fn);
    vm.register_function(b"inet_pton", inet_pton_fn);
    vm.register_function(b"inet_ntop", inet_ntop_fn);

    for (name, val) in [
        (&b"DNS_A"[..], PHP_DNS_A),
        (b"DNS_NS", PHP_DNS_NS),
        (b"DNS_CNAME", PHP_DNS_CNAME),
        (b"DNS_SOA", PHP_DNS_SOA),
        (b"DNS_PTR", PHP_DNS_PTR),
        (b"DNS_HINFO", PHP_DNS_HINFO),
        (b"DNS_CAA", PHP_DNS_CAA),
        (b"DNS_MX", PHP_DNS_MX),
        (b"DNS_TXT", PHP_DNS_TXT),
        (b"DNS_SRV", PHP_DNS_SRV),
        (b"DNS_NAPTR", PHP_DNS_NAPTR),
        (b"DNS_AAAA", PHP_DNS_AAAA),
        (b"DNS_A6", PHP_DNS_A6),
        (b"DNS_ANY", PHP_DNS_ANY),
        (b"DNS_ALL", PHP_DNS_ALL),
    ] {
        vm.constants.insert(name.to_vec(), Value::Long(val));
    }
}

// ---------------------------------------------------------------------------
// Argument helpers
// ---------------------------------------------------------------------------

fn throw(vm: &mut Vm, class: &[u8], msg: String) -> VmError {
    let exc = vm.create_exception(class, &msg, vm.current_line);
    vm.current_exception = Some(exc);
    VmError {
        message: msg,
        line: vm.current_line,
    }
}

fn pending(vm: &Vm) -> Result<(), VmError> {
    if vm.current_exception.is_some() {
        Err(VmError {
            message: String::new(),
            line: vm.current_line,
        })
    } else {
        Ok(())
    }
}

/// Z_PARAM_STRING
fn str_arg(vm: &mut Vm, args: &[Value], idx: usize) -> Result<Vec<u8>, VmError> {
    let v = args.get(idx).map(|v| v.deref()).unwrap_or(Value::Null);
    let s = match &v {
        Value::String(s) => s.as_bytes().to_vec(),
        _ => vm.value_to_string(&v).as_bytes().to_vec(),
    };
    pending(vm)?;
    Ok(s)
}

/// Z_PARAM_PATH: a string that must not contain NUL bytes.
fn path_arg(
    vm: &mut Vm,
    func: &str,
    args: &[Value],
    idx: usize,
    name: &str,
) -> Result<Vec<u8>, VmError> {
    let s = str_arg(vm, args, idx)?;
    if s.contains(&0) {
        return Err(throw(
            vm,
            b"ValueError",
            format!(
                "{}(): Argument #{} (${}) must not contain any null bytes",
                func,
                idx + 1,
                name
            ),
        ));
    }
    Ok(s)
}

/// Z_PARAM_LONG
fn long_arg(args: &[Value], idx: usize) -> i64 {
    args.get(idx).map(|v| v.deref().to_long()).unwrap_or(0)
}

fn value_error(vm: &mut Vm, func: &str, n: usize, name: &str, msg: &str) -> VmError {
    throw(
        vm,
        b"ValueError",
        format!("{}(): Argument #{} (${}) {}", func, n, name, msg),
    )
}

/// zend_try_array_init(): reset a by-reference argument to an empty array
/// and hand back the array that now lives inside it.
fn try_array_init(arg: Option<&Value>) -> Option<Rc<RefCell<PhpArray>>> {
    let arg = arg?;
    let arr = Rc::new(RefCell::new(PhpArray::new()));
    if let Value::Reference(r) = arg {
        *r.borrow_mut() = Value::Array(arr.clone());
    }
    Some(arr)
}

fn bstr(b: &[u8]) -> Value {
    Value::String(PhpString::from_vec(b.to_vec()))
}

fn assoc(arr: &mut PhpArray, key: &str, v: Value) {
    arr.set(ArrayKey::String(PhpString::from_bytes(key.as_bytes())), v);
}

fn arr_val(arr: PhpArray) -> Value {
    Value::Array(Rc::new(RefCell::new(arr)))
}

// ---------------------------------------------------------------------------
// inet_pton()/inet_ntop() (glibc resolv/inet_pton.c, inet_ntop.c)
// ---------------------------------------------------------------------------

fn inet_pton4(src: &[u8]) -> Option<[u8; 4]> {
    let mut tmp = [0u8; 4];
    let mut tp = 0usize;
    let mut saw_digit = false;
    let mut octets = 0;
    for &ch in src {
        if ch.is_ascii_digit() {
            let new = tmp[tp] as u32 * 10 + (ch - b'0') as u32;
            if saw_digit && tmp[tp] == 0 {
                return None;
            }
            if new > 255 {
                return None;
            }
            tmp[tp] = new as u8;
            if !saw_digit {
                octets += 1;
                if octets > 4 {
                    return None;
                }
                saw_digit = true;
            }
        } else if ch == b'.' && saw_digit {
            if octets == 4 {
                return None;
            }
            tp += 1;
            tmp[tp] = 0;
            saw_digit = false;
        } else {
            return None;
        }
    }
    if octets < 4 {
        return None;
    }
    Some(tmp)
}

fn inet_pton6(src: &[u8]) -> Option<[u8; 16]> {
    let mut tmp = [0u8; 16];
    let mut tp = 0usize;
    let mut colonp: Option<usize> = None;
    let mut i = 0usize;
    if src.is_empty() {
        return None;
    }
    if src[0] == b':' {
        i += 1;
        if i == src.len() || src[i] != b':' {
            return None;
        }
    }
    let mut curtok = i;
    let mut xdigits_seen = 0;
    let mut val: u32 = 0;
    while i < src.len() {
        let ch = src[i];
        i += 1;
        if let Some(d) = (ch as char).to_digit(16) {
            if xdigits_seen == 4 {
                return None;
            }
            val = (val << 4) | d;
            if val > 0xffff {
                return None;
            }
            xdigits_seen += 1;
            continue;
        }
        if ch == b':' {
            curtok = i;
            if xdigits_seen == 0 {
                if colonp.is_some() {
                    return None;
                }
                colonp = Some(tp);
                continue;
            } else if i == src.len() {
                return None;
            }
            if tp + 2 > 16 {
                return None;
            }
            tmp[tp] = (val >> 8) as u8;
            tmp[tp + 1] = val as u8;
            tp += 2;
            xdigits_seen = 0;
            val = 0;
            continue;
        }
        if ch == b'.' && tp + 4 <= 16 {
            if let Some(v4) = inet_pton4(&src[curtok..]) {
                tmp[tp..tp + 4].copy_from_slice(&v4);
                tp += 4;
                xdigits_seen = 0;
                break;
            }
        }
        return None;
    }
    if xdigits_seen > 0 {
        if tp + 2 > 16 {
            return None;
        }
        tmp[tp] = (val >> 8) as u8;
        tmp[tp + 1] = val as u8;
        tp += 2;
    }
    if let Some(cp) = colonp {
        if tp == 16 {
            return None;
        }
        let n = tp - cp;
        let moved: Vec<u8> = tmp[cp..tp].to_vec();
        for b in &mut tmp[cp..16] {
            *b = 0;
        }
        tmp[16 - n..16].copy_from_slice(&moved);
        tp = 16;
    }
    if tp != 16 {
        return None;
    }
    Some(tmp)
}

fn inet_ntop4(a: &[u8]) -> String {
    format!("{}.{}.{}.{}", a[0], a[1], a[2], a[3])
}

fn inet_ntop6(src: &[u8]) -> String {
    let mut words = [0u16; 8];
    for (i, w) in words.iter_mut().enumerate() {
        *w = ((src[2 * i] as u16) << 8) | src[2 * i + 1] as u16;
    }
    let (mut best_base, mut best_len): (i32, i32) = (-1, 0);
    let (mut cur_base, mut cur_len): (i32, i32) = (-1, 0);
    for (i, &w) in words.iter().enumerate() {
        if w == 0 {
            if cur_base == -1 {
                cur_base = i as i32;
                cur_len = 1;
            } else {
                cur_len += 1;
            }
        } else if cur_base != -1 {
            if best_base == -1 || cur_len > best_len {
                best_base = cur_base;
                best_len = cur_len;
            }
            cur_base = -1;
        }
    }
    if cur_base != -1 && (best_base == -1 || cur_len > best_len) {
        best_base = cur_base;
        best_len = cur_len;
    }
    if best_base != -1 && best_len < 2 {
        best_base = -1;
    }
    let mut out = String::new();
    let mut i = 0i32;
    while i < 8 {
        if best_base != -1 && i >= best_base && i < best_base + best_len {
            if i == best_base {
                out.push(':');
            }
            i += 1;
            continue;
        }
        if i != 0 {
            out.push(':');
        }
        if i == 6
            && best_base == 0
            && (best_len == 6
                || (best_len == 7 && words[7] != 0x0001)
                || (best_len == 5 && words[5] == 0xffff))
        {
            out.push_str(&inet_ntop4(&src[12..16]));
            return out;
        }
        out.push_str(&format!("{:x}", words[i as usize]));
        i += 1;
    }
    if best_base != -1 && best_base + best_len == 8 {
        out.push(':');
    }
    out
}

fn ip2long_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let addr = path_arg(vm, "ip2long", args, 0, "ip")?;
    if addr.is_empty() {
        return Ok(Value::False);
    }
    match inet_pton4(&addr) {
        Some(a) => Ok(Value::Long(u32::from_be_bytes(a) as i64)),
        None => Ok(Value::False),
    }
}

fn long2ip_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let ip = long_arg(args, 0) as u64 as u32;
    Ok(bstr(inet_ntop4(&ip.to_be_bytes()).as_bytes()))
}

fn inet_pton_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let addr = path_arg(vm, "inet_pton", args, 0, "ip")?;
    if addr.contains(&b':') {
        return Ok(match inet_pton6(&addr) {
            Some(a) => bstr(&a),
            None => Value::False,
        });
    }
    if !addr.contains(&b'.') {
        return Ok(Value::False);
    }
    Ok(match inet_pton4(&addr) {
        Some(a) => bstr(&a),
        None => Value::False,
    })
}

fn inet_ntop_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let addr = str_arg(vm, args, 0)?;
    Ok(match addr.len() {
        16 => bstr(inet_ntop6(&addr).as_bytes()),
        4 => bstr(inet_ntop4(&addr).as_bytes()),
        _ => Value::False,
    })
}

// ---------------------------------------------------------------------------
// getservby*/getprotoby* (libc netdb)
// ---------------------------------------------------------------------------

fn cstring(b: &[u8]) -> std::ffi::CString {
    // Callers have already rejected embedded NUL bytes.
    std::ffi::CString::new(b.to_vec()).unwrap_or_default()
}

fn getservbyname_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let name = path_arg(vm, "getservbyname", args, 0, "service")?;
    let proto = path_arg(vm, "getservbyname", args, 1, "protocol")?;
    let (n, p) = (cstring(&name), cstring(&proto));
    // SAFETY: both arguments are valid NUL-terminated strings; the result
    // points into libc's static storage and is read immediately.
    let port = unsafe {
        let serv = libc::getservbyname(n.as_ptr(), p.as_ptr());
        if serv.is_null() {
            return Ok(Value::False);
        }
        (*serv).s_port
    };
    Ok(Value::Long(u16::from_be(port as u16) as i64))
}

fn getservbyport_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let port = long_arg(args, 0);
    let proto = path_arg(vm, "getservbyport", args, 1, "protocol")?;
    let p = cstring(&proto);
    // SAFETY: see getservbyname_fn.
    let name = unsafe {
        let serv = libc::getservbyport((port as u16).to_be() as libc::c_int, p.as_ptr());
        if serv.is_null() || (*serv).s_name.is_null() {
            return Ok(Value::False);
        }
        std::ffi::CStr::from_ptr((*serv).s_name).to_bytes().to_vec()
    };
    Ok(bstr(&name))
}

fn getprotobyname_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let name = path_arg(vm, "getprotobyname", args, 0, "protocol")?;
    let n = cstring(&name);
    // SAFETY: valid C string; static result read immediately.
    let proto = unsafe {
        let ent = libc::getprotobyname(n.as_ptr());
        if ent.is_null() {
            return Ok(Value::False);
        }
        (*ent).p_proto
    };
    Ok(Value::Long(proto as i64))
}

fn getprotobynumber_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let proto = long_arg(args, 0) as i32;
    // SAFETY: static result read immediately.
    let name = unsafe {
        let ent = libc::getprotobynumber(proto);
        if ent.is_null() || (*ent).p_name.is_null() {
            return Ok(Value::False);
        }
        std::ffi::CStr::from_ptr((*ent).p_name).to_bytes().to_vec()
    };
    Ok(bstr(&name))
}

// ---------------------------------------------------------------------------
// gethostbyname()/gethostbynamel()/gethostbyaddr()
// ---------------------------------------------------------------------------

unsafe extern "C" {
    fn gethostbyname_r(
        name: *const libc::c_char,
        ret: *mut libc::hostent,
        buf: *mut libc::c_char,
        buflen: libc::size_t,
        result: *mut *mut libc::hostent,
        h_errnop: *mut libc::c_int,
    ) -> libc::c_int;
}

/// php_network_gethostbyname(): the IPv4 addresses of `name`, or None when
/// the lookup fails.
fn network_gethostbyname(name: &[u8]) -> Option<Vec<[u8; 4]>> {
    let cname = cstring(name);
    let mut size = 1024usize;
    loop {
        let mut buf = vec![0 as libc::c_char; size];
        // SAFETY: hostent is plain data; all pointers handed to
        // gethostbyname_r are valid for the duration of the call, and the
        // result only points into `buf`, which outlives its use below.
        unsafe {
            let mut he: libc::hostent = std::mem::zeroed();
            let mut result: *mut libc::hostent = std::ptr::null_mut();
            let mut herr: libc::c_int = 0;
            let rc = gethostbyname_r(
                cname.as_ptr(),
                &mut he,
                buf.as_mut_ptr(),
                size,
                &mut result,
                &mut herr,
            );
            if rc == libc::ERANGE && size < (1 << 22) {
                size *= 2;
                continue;
            }
            if rc != 0 || result.is_null() {
                return None;
            }
            let mut out = Vec::new();
            if he.h_length == 4 && !he.h_addr_list.is_null() {
                let mut i = 0;
                loop {
                    let p = *he.h_addr_list.add(i);
                    if p.is_null() {
                        break;
                    }
                    let mut a = [0u8; 4];
                    std::ptr::copy_nonoverlapping(p as *const u8, a.as_mut_ptr(), 4);
                    out.push(a);
                    i += 1;
                }
            }
            return Some(out);
        }
    }
}

fn gethostbyname_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let host = path_arg(vm, "gethostbyname", args, 0, "hostname")?;
    if host.len() > MAXFQDNLEN {
        vm.emit_warning(&format!(
            "gethostbyname(): Host name cannot be longer than {} characters",
            MAXFQDNLEN
        ));
        return Ok(bstr(&host));
    }
    match network_gethostbyname(&host).and_then(|l| l.into_iter().next()) {
        Some(a) => Ok(bstr(inet_ntop4(&a).as_bytes())),
        None => Ok(bstr(&host)),
    }
}

fn gethostbynamel_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let host = path_arg(vm, "gethostbynamel", args, 0, "hostname")?;
    if host.len() > MAXFQDNLEN {
        vm.emit_warning(&format!(
            "gethostbynamel(): Host name cannot be longer than {} characters",
            MAXFQDNLEN
        ));
        return Ok(Value::False);
    }
    let Some(list) = network_gethostbyname(&host) else {
        return Ok(Value::False);
    };
    let mut arr = PhpArray::new();
    for a in list {
        arr.push(bstr(inet_ntop4(&a).as_bytes()));
    }
    Ok(arr_val(arr))
}

/// getnameinfo(NI_NAMEREQD) for a numeric address.
fn reverse_lookup(ip: IpAddr) -> Option<Vec<u8>> {
    let mut host = [0 as libc::c_char; 1025];
    // SAFETY: the sockaddr structs are fully initialised and sized
    // correctly; `host` is a writable buffer of the advertised length.
    unsafe {
        let rc = match ip {
            IpAddr::V4(v4) => {
                let mut sa: libc::sockaddr_in = std::mem::zeroed();
                sa.sin_family = libc::AF_INET as libc::sa_family_t;
                sa.sin_addr.s_addr = u32::from_ne_bytes(v4.octets());
                libc::getnameinfo(
                    &sa as *const _ as *const libc::sockaddr,
                    std::mem::size_of::<libc::sockaddr_in>() as libc::socklen_t,
                    host.as_mut_ptr(),
                    host.len() as libc::socklen_t,
                    std::ptr::null_mut(),
                    0,
                    libc::NI_NAMEREQD,
                )
            }
            IpAddr::V6(v6) => {
                let mut sa: libc::sockaddr_in6 = std::mem::zeroed();
                sa.sin6_family = libc::AF_INET6 as libc::sa_family_t;
                sa.sin6_addr.s6_addr = v6.octets();
                libc::getnameinfo(
                    &sa as *const _ as *const libc::sockaddr,
                    std::mem::size_of::<libc::sockaddr_in6>() as libc::socklen_t,
                    host.as_mut_ptr(),
                    host.len() as libc::socklen_t,
                    std::ptr::null_mut(),
                    0,
                    libc::NI_NAMEREQD,
                )
            }
        };
        if rc == 0 {
            Some(std::ffi::CStr::from_ptr(host.as_ptr()).to_bytes().to_vec())
        } else {
            None
        }
    }
}

fn gethostbyaddr_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let addr = path_arg(vm, "gethostbyaddr", args, 0, "ip")?;
    let ip = if let Some(a6) = inet_pton6(&addr) {
        IpAddr::V6(Ipv6Addr::from(a6))
    } else if let Some(a4) = inet_pton4(&addr) {
        IpAddr::V4(Ipv4Addr::from(a4))
    } else {
        vm.emit_warning("gethostbyaddr(): Address is not a valid IPv4 or IPv6 address");
        return Ok(Value::False);
    };
    Ok(bstr(&reverse_lookup(ip).unwrap_or(addr)))
}

// ---------------------------------------------------------------------------
// Stub resolver: configuration (glibc res_init.c)
// ---------------------------------------------------------------------------

struct ResConf {
    servers: Vec<SocketAddr>,
    search: Vec<Vec<u8>>,
    ndots: u32,
    retrans: u32,
    retry: u32,
    rotate: bool,
    use_vc: bool,
    edns0: bool,
    no_tld_query: bool,
    ign_tc: bool,
}

const MAXNS: usize = 3;
const NAMESERVER_PORT: u16 = 53;

fn res_setoptions(conf: &mut ResConf, options: &[u8]) {
    for word in options
        .split(|&c| c == b' ' || c == b'\t' || c == b'\n')
        .filter(|w| !w.is_empty())
    {
        let num = |prefix: &[u8]| -> Option<i64> {
            let rest = word.strip_prefix(prefix)?;
            // strtol(): optional sign followed by leading digits.
            let (neg, digits) = match rest.first() {
                Some(b'-') => (true, &rest[1..]),
                Some(b'+') => (false, &rest[1..]),
                _ => (false, rest),
            };
            let mut v: i64 = 0;
            for &d in digits.iter().take_while(|d| d.is_ascii_digit()) {
                v = v.saturating_mul(10).saturating_add((d - b'0') as i64);
            }
            Some(if neg { -v } else { v })
        };
        if let Some(i) = num(b"ndots:") {
            conf.ndots = i.clamp(0, 15) as u32;
        } else if let Some(i) = num(b"timeout:") {
            conf.retrans = i.clamp(0, 30) as u32;
        } else if let Some(i) = num(b"attempts:") {
            conf.retry = i.clamp(0, 5) as u32;
        } else {
            let (negate, w) = match word.strip_prefix(b"-") {
                Some(w) => (true, w),
                None => (false, word),
            };
            let flag: Option<&mut bool> = if w.starts_with(b"rotate") {
                Some(&mut conf.rotate)
            } else if w.starts_with(b"edns0") {
                Some(&mut conf.edns0)
            } else if w.starts_with(b"no_tld_query") || w.starts_with(b"no-tld-query") {
                Some(&mut conf.no_tld_query)
            } else if w.starts_with(b"use-vc") {
                Some(&mut conf.use_vc)
            } else {
                None
            };
            if let Some(f) = flag {
                *f = !negate;
            }
        }
    }
}

fn parse_nameserver(s: &[u8]) -> Option<SocketAddr> {
    let s = std::str::from_utf8(s).ok()?;
    if let Ok(v4) = s.parse::<Ipv4Addr>() {
        return Some(SocketAddr::new(IpAddr::V4(v4), NAMESERVER_PORT));
    }
    let (addr, scope) = match s.find('%') {
        Some(p) => (&s[..p], Some(&s[p + 1..])),
        None => (s, None),
    };
    let a6 = inet_pton6(addr.as_bytes())?;
    let scope_id = scope
        .map(|sc| {
            sc.parse::<u32>().unwrap_or_else(|_| {
                let c = cstring(sc.as_bytes());
                // SAFETY: valid C string.
                unsafe { libc::if_nametoindex(c.as_ptr()) }
            })
        })
        .unwrap_or(0);
    Some(SocketAddr::V6(SocketAddrV6::new(
        Ipv6Addr::from(a6),
        NAMESERVER_PORT,
        0,
        scope_id,
    )))
}

fn split_names(s: &[u8]) -> Vec<Vec<u8>> {
    s.split(|&c| c == b' ' || c == b'\t' || c == b'\n')
        .filter(|w| !w.is_empty())
        .map(|w| w.to_vec())
        .collect()
}

/// res_ninit()
fn res_ninit() -> ResConf {
    let mut conf = ResConf {
        servers: Vec::new(),
        search: Vec::new(),
        ndots: 1,
        retrans: 5,
        retry: 2,
        rotate: false,
        use_vc: false,
        edns0: false,
        no_tld_query: false,
        ign_tc: false,
    };
    let mut haveenv = false;
    if let Some(ld) = std::env::var_os("LOCALDOMAIN") {
        use std::os::unix::ffi::OsStrExt;
        let ld = ld.as_bytes();
        let ld = ld.split(|&c| c == b'\n').next().unwrap_or(&[]);
        conf.search = split_names(ld);
        haveenv = true;
    }
    let is_kw = |line: &[u8], kw: &[u8]| {
        line.starts_with(kw) && matches!(line.get(kw.len()), Some(b' ') | Some(b'\t'))
    };
    let trim = |b: &[u8]| -> Vec<u8> {
        let start = b
            .iter()
            .position(|&c| c != b' ' && c != b'\t')
            .unwrap_or(b.len());
        b[start..].to_vec()
    };
    if let Ok(data) = std::fs::read("/etc/resolv.conf") {
        for line in data.split(|&c| c == b'\n') {
            if line.first() == Some(&b';') || line.first() == Some(&b'#') {
                continue;
            }
            if is_kw(line, b"domain") {
                if haveenv {
                    continue;
                }
                let rest = trim(&line[6..]);
                if rest.is_empty() {
                    continue;
                }
                let end = rest
                    .iter()
                    .position(|&c| c == b' ' || c == b'\t')
                    .unwrap_or(rest.len());
                conf.search = vec![rest[..end].to_vec()];
            } else if is_kw(line, b"search") {
                if haveenv {
                    continue;
                }
                let rest = trim(&line[6..]);
                if rest.is_empty() {
                    continue;
                }
                conf.search = split_names(&rest);
            } else if is_kw(line, b"nameserver") {
                let rest = trim(&line[10..]);
                let end = rest
                    .iter()
                    .position(|&c| c == b' ' || c == b'\t')
                    .unwrap_or(rest.len());
                if let Some(sa) = parse_nameserver(&rest[..end]) {
                    conf.servers.push(sa);
                }
            } else if is_kw(line, b"options") {
                res_setoptions(&mut conf, &line[7..]);
            }
        }
    }
    if conf.servers.is_empty() {
        conf.servers.push(SocketAddr::new(
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            NAMESERVER_PORT,
        ));
    }
    conf.servers.truncate(MAXNS);
    if conf.search.is_empty() {
        // domain_from_hostname()
        let mut buf = [0u8; 256];
        // SAFETY: buf is writable for its full length.
        let rc = unsafe { libc::gethostname(buf.as_mut_ptr() as *mut libc::c_char, buf.len()) };
        if rc == 0 {
            let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
            if let Some(dot) = buf[..len].iter().position(|&c| c == b'.') {
                let d = buf[dot + 1..len].to_vec();
                conf.search.push(d);
            }
        }
    }
    if let Some(opts) = std::env::var_os("RES_OPTIONS") {
        use std::os::unix::ffi::OsStrExt;
        res_setoptions(&mut conf, opts.as_bytes());
    }
    conf
}

// ---------------------------------------------------------------------------
// Stub resolver: names (glibc ns_name.c, ns_name_pton.c, ...)
// ---------------------------------------------------------------------------

const NS_MAXCDNAME: usize = 255;

/// ns_name_pton() with a NS_MAXCDNAME output buffer.
fn ns_name_pton(src: &[u8]) -> Option<Vec<u8>> {
    let eom = NS_MAXCDNAME;
    let mut dst: Vec<u8> = vec![0];
    let mut label = 0usize;
    let mut escaped = false;
    let mut i = 0usize;
    while i < src.len() {
        let mut c = src[i];
        i += 1;
        if escaped {
            if c.is_ascii_digit() {
                let mut n = (c - b'0') as u32 * 100;
                let c2 = *src.get(i).filter(|c| c.is_ascii_digit())?;
                i += 1;
                n += (c2 - b'0') as u32 * 10;
                let c3 = *src.get(i).filter(|c| c.is_ascii_digit())?;
                i += 1;
                n += (c3 - b'0') as u32;
                if n > 255 {
                    return None;
                }
                c = n as u8;
            }
            escaped = false;
        } else if c == b'\\' {
            escaped = true;
            continue;
        } else if c == b'.' {
            let len = dst.len() - label - 1;
            if len & 0xc0 != 0 || label >= eom {
                return None;
            }
            dst[label] = len as u8;
            if i == src.len() {
                if len != 0 {
                    if dst.len() >= eom {
                        return None;
                    }
                    dst.push(0);
                }
                if dst.len() > NS_MAXCDNAME {
                    return None;
                }
                return Some(dst);
            }
            if len == 0 || src[i] == b'.' {
                return None;
            }
            label = dst.len();
            dst.push(0);
            continue;
        }
        if dst.len() >= eom {
            return None;
        }
        dst.push(c);
    }
    if escaped {
        return None;
    }
    let len = dst.len() - label - 1;
    if len & 0xc0 != 0 || label >= eom {
        return None;
    }
    dst[label] = len as u8;
    if len != 0 {
        if dst.len() >= eom {
            return None;
        }
        dst.push(0);
    }
    if dst.len() > NS_MAXCDNAME {
        return None;
    }
    Some(dst)
}

/// ns_name_unpack(): decompress the name at `src` into wire format.
/// Returns (wire name, bytes consumed at `src`).
fn ns_name_unpack(msg: &[u8], src: usize) -> Option<(Vec<u8>, usize)> {
    let eom = msg.len();
    if src >= eom {
        return None;
    }
    let mut srcp = src;
    let mut dst = Vec::new();
    let mut len: Option<usize> = None;
    let mut checked = 0usize;
    loop {
        if srcp == eom {
            return None;
        }
        let n = msg[srcp];
        srcp += 1;
        if n >= 0xc0 {
            if srcp == eom {
                return None;
            }
            if len.is_none() {
                len = Some(srcp + 1 - src);
            }
            let ptr = (((n & 0x3f) as usize) << 8) | msg[srcp] as usize;
            srcp = ptr;
            if srcp >= eom {
                return None;
            }
            checked += 2;
            if checked >= eom {
                return None;
            }
        } else if n == 0 {
            if dst.len() == NS_MAXCDNAME {
                return None;
            }
            dst.push(0);
            return Some((dst, len.unwrap_or(srcp - src)));
        } else if n <= 63 {
            if NS_MAXCDNAME - dst.len() < 1 + n as usize {
                return None;
            }
            if eom - srcp < n as usize {
                return None;
            }
            dst.push(n);
            dst.extend_from_slice(&msg[srcp..srcp + n as usize]);
            srcp += n as usize;
        } else {
            return None;
        }
    }
}

/// ns_name_ntop() into a buffer of `dstsiz` bytes (NUL included).
fn ns_name_ntop(src: &[u8], dstsiz: usize) -> Option<Vec<u8>> {
    let mut out: Vec<u8> = Vec::new();
    let mut cp = 0usize;
    loop {
        let l = *src.get(cp)? as usize;
        cp += 1;
        if l == 0 {
            break;
        }
        if l >= 64 {
            return None;
        }
        if !out.is_empty() {
            if out.len() >= dstsiz {
                return None;
            }
            out.push(b'.');
        }
        for _ in 0..l {
            let c = *src.get(cp)?;
            cp += 1;
            let room = dstsiz.saturating_sub(out.len());
            if matches!(c, b'"' | b'.' | b';' | b'\\' | b'(' | b')' | b'@' | b'$') {
                if room < 2 {
                    return None;
                }
                out.push(b'\\');
                out.push(c);
            } else if c <= 0x20 || c >= 0x7f {
                if room < 4 {
                    return None;
                }
                out.push(b'\\');
                out.push(b'0' + c / 100);
                out.push(b'0' + (c % 100) / 10);
                out.push(b'0' + c % 10);
            } else {
                if room < 2 {
                    return None;
                }
                out.push(c);
            }
        }
    }
    if out.is_empty() {
        if out.len() >= dstsiz {
            return None;
        }
        out.push(b'.');
    }
    if out.len() >= dstsiz {
        return None;
    }
    Some(out)
}

/// dn_expand(): returns (presentation name, bytes consumed).
fn dn_expand(msg: &[u8], src: usize, dstsiz: usize) -> Option<(Vec<u8>, usize)> {
    let (wire, n) = ns_name_unpack(msg, src)?;
    let mut name = ns_name_ntop(&wire, dstsiz)?;
    if name.first() == Some(&b'.') {
        // dn_expand() turns the root "." into an empty string.
        name.clear();
    }
    Some((name, n))
}

/// dn_skipname() / ns_name_skip()
fn dn_skipname(msg: &[u8], start: usize) -> Option<usize> {
    let eom = msg.len();
    let mut cp = start;
    while cp < eom {
        let n = msg[cp];
        cp += 1;
        if n == 0 {
            break;
        }
        match n & 0xc0 {
            0 => {
                cp += n as usize;
                continue;
            }
            0xc0 => {
                cp += 1;
                break;
            }
            _ => return None,
        }
    }
    if cp > eom {
        return None;
    }
    Some(cp - start)
}

fn ns_samename(a: &[u8], b: &[u8]) -> bool {
    a.eq_ignore_ascii_case(b)
}

// ---------------------------------------------------------------------------
// Stub resolver: transport (glibc res_send.c)
// ---------------------------------------------------------------------------

struct Resolver {
    conf: ResConf,
    h_errno: i32,
    errno: i32,
    /// rcode of the answer buffer header (UHEADER *hp in res_query.c).
    rcode: u8,
}

fn random_id() -> u16 {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    let mut h = RandomState::new().build_hasher();
    h.write_u128(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0),
    );
    h.finish() as u16
}

static NS_ROTATE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// res_queriesmatch(): does `ans` answer the question(s) in `query`?
fn queries_match(query: &[u8], ans: &[u8]) -> bool {
    if query.len() < HFIXEDSZ || ans.len() < HFIXEDSZ {
        return false;
    }
    let qd = u16::from_be_bytes([query[4], query[5]]);
    if qd != u16::from_be_bytes([ans[4], ans[5]]) {
        return false;
    }
    let mut qp = HFIXEDSZ;
    let mut ap = HFIXEDSZ;
    for _ in 0..qd {
        let Some((qn, ql)) = ns_name_unpack(query, qp) else {
            return false;
        };
        let Some((an, al)) = ns_name_unpack(ans, ap) else {
            return false;
        };
        qp += ql;
        ap += al;
        if qp + 4 > query.len() || ap + 4 > ans.len() {
            return false;
        }
        if !ns_samename(&qn, &an) || query[qp..qp + 4] != ans[ap..ap + 4] {
            return false;
        }
        qp += 4;
        ap += 4;
    }
    true
}

enum DgResult {
    /// Recoverable failure: try the next server.
    NextNs,
    /// Truncated answer: retry this server over TCP.
    Tcp,
    Answer(Vec<u8>),
}

impl Resolver {
    fn new() -> Self {
        Resolver {
            conf: res_ninit(),
            h_errno: 0,
            errno: 0,
            rcode: NOERROR,
        }
    }

    /// send_dg()
    fn send_dg(&mut self, query: &[u8], ns: usize, gotsomewhere: &mut bool) -> DgResult {
        let nsap = self.conf.servers[ns];
        let nscount = self.conf.servers.len() as u64;
        let mut seconds = (self.conf.retrans as u64) << ns;
        if ns > 0 {
            seconds /= nscount;
        }
        if seconds == 0 {
            seconds = 1;
        }
        let bind: SocketAddr = match nsap {
            SocketAddr::V4(_) => (Ipv4Addr::UNSPECIFIED, 0).into(),
            SocketAddr::V6(_) => (Ipv6Addr::UNSPECIFIED, 0).into(),
        };
        let sock = match UdpSocket::bind(bind) {
            Ok(s) => s,
            Err(_) => return DgResult::NextNs,
        };
        if sock.connect(nsap).is_err() {
            return DgResult::NextNs;
        }
        if sock.send(query).map(|n| n != query.len()).unwrap_or(true) {
            return DgResult::NextNs;
        }
        let finish = Instant::now() + Duration::from_secs(seconds);
        let mut buf = vec![0u8; ANSWER_SIZE];
        loop {
            let now = Instant::now();
            if now >= finish {
                *gotsomewhere = true;
                return DgResult::NextNs;
            }
            let _ = sock.set_read_timeout(Some(finish - now));
            let n = match sock.recv(&mut buf) {
                Ok(n) => n,
                Err(e)
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::WouldBlock
                            | std::io::ErrorKind::TimedOut
                            | std::io::ErrorKind::Interrupted
                    ) =>
                {
                    continue;
                }
                Err(_) => return DgResult::NextNs,
            };
            *gotsomewhere = true;
            if n < HFIXEDSZ {
                return DgResult::NextNs;
            }
            let ans = &buf[..n];
            let rcode = ans[3] & 0x0f;
            let is_error = matches!(rcode, SERVFAIL | NOTIMP | REFUSED);
            let qdcount = u16::from_be_bytes([ans[4], ans[5]]);
            let skip_match = n == HFIXEDSZ && qdcount == 0 && is_error;
            if ans[0..2] != query[0..2] || !(skip_match || queries_match(query, ans)) {
                // Spurious packet: keep waiting.
                continue;
            }
            // The response lands in the caller's answer buffer.
            self.rcode = rcode;
            if is_error {
                return DgResult::NextNs;
            }
            let ancount = u16::from_be_bytes([ans[6], ans[7]]);
            let arcount = u16::from_be_bytes([ans[10], ans[11]]);
            let aa = ans[2] & 0x04 != 0;
            let ra = ans[3] & 0x80 != 0;
            if rcode == NOERROR && ancount == 0 && !aa && !ra && arcount == 0 {
                // Lame referral.
                return DgResult::NextNs;
            }
            if !self.conf.ign_tc && ans[2] & 0x02 != 0 {
                return DgResult::Tcp;
            }
            return DgResult::Answer(ans.to_vec());
        }
    }

    /// send_vc(); Err carries the errno for terrno.
    fn send_vc(&mut self, query: &[u8], ns: usize) -> Result<Vec<u8>, i32> {
        let nsap = self.conf.servers[ns];
        let tmo = Duration::from_secs((self.conf.retrans.max(1) as u64) * 2);
        let errno_of = |e: &std::io::Error| e.raw_os_error().unwrap_or(libc::ETIMEDOUT);
        let mut s = TcpStream::connect_timeout(&nsap, tmo).map_err(|e| errno_of(&e))?;
        let _ = s.set_read_timeout(Some(tmo));
        let _ = s.set_write_timeout(Some(tmo));
        let mut msg = Vec::with_capacity(query.len() + 2);
        msg.extend_from_slice(&(query.len() as u16).to_be_bytes());
        msg.extend_from_slice(query);
        s.write_all(&msg).map_err(|e| errno_of(&e))?;
        loop {
            let mut lb = [0u8; 2];
            s.read_exact(&mut lb).map_err(|e| errno_of(&e))?;
            let rlen = u16::from_be_bytes(lb) as usize;
            if rlen < HFIXEDSZ {
                return Err(libc::EMSGSIZE);
            }
            let mut ans = vec![0u8; rlen];
            s.read_exact(&mut ans).map_err(|e| errno_of(&e))?;
            if ans[0..2] != query[0..2] {
                continue;
            }
            self.rcode = ans[3] & 0x0f;
            return Ok(ans);
        }
    }

    /// res_nsend(); Err(()) leaves errno set.
    fn res_nsend(&mut self, query: &[u8]) -> Result<Vec<u8>, ()> {
        let nscount = self.conf.servers.len();
        let mut v_circuit = self.conf.use_vc || query.len() > 512;
        let mut gotsomewhere = false;
        let mut terrno = libc::ETIMEDOUT;
        let ns_offset = if self.conf.rotate {
            NS_ROTATE.fetch_add(1, std::sync::atomic::Ordering::Relaxed) % nscount
        } else {
            0
        };
        let mut attempt = 0;
        while attempt < self.conf.retry {
            for ns_shift in 0..nscount {
                let ns = (ns_shift + ns_offset) % nscount;
                loop {
                    if v_circuit {
                        attempt = self.conf.retry;
                        match self.send_vc(query, ns) {
                            Ok(ans) => return Ok(ans),
                            Err(e) => {
                                terrno = e;
                                break;
                            }
                        }
                    } else {
                        match self.send_dg(query, ns, &mut gotsomewhere) {
                            DgResult::Answer(ans) => return Ok(ans),
                            DgResult::NextNs => break,
                            DgResult::Tcp => {
                                v_circuit = true;
                                continue;
                            }
                        }
                    }
                }
            }
            attempt += 1;
        }
        self.errno = if !v_circuit {
            if !gotsomewhere {
                libc::ECONNREFUSED
            } else {
                libc::ETIMEDOUT
            }
        } else {
            terrno
        };
        Err(())
    }

    /// res_nmkquery() (+ __res_nopt() with "options edns0")
    fn mkquery(&self, name: &[u8], qtype: u16) -> Option<Vec<u8>> {
        let wire = ns_name_pton(name)?;
        let mut q = Vec::with_capacity(HFIXEDSZ + wire.len() + QFIXEDSZ + 11);
        q.extend_from_slice(&random_id().to_be_bytes());
        q.extend_from_slice(&[0x01, 0x00]); // RD
        q.extend_from_slice(&[0, 1, 0, 0, 0, 0, 0, 0]);
        q.extend_from_slice(&wire);
        q.extend_from_slice(&qtype.to_be_bytes());
        q.extend_from_slice(&C_IN.to_be_bytes());
        if self.conf.edns0 {
            q.push(0);
            q.extend_from_slice(&41u16.to_be_bytes());
            q.extend_from_slice(&1200u16.to_be_bytes());
            q.extend_from_slice(&[0, 0, 0, 0, 0, 0]);
            q[11] = 1;
        }
        Some(q)
    }

    /// __res_context_query()
    fn res_nquery(&mut self, name: &[u8], qtype: u16) -> Option<Vec<u8>> {
        self.rcode = NOERROR;
        let Some(q) = self.mkquery(name, qtype) else {
            self.h_errno = NO_RECOVERY;
            return None;
        };
        let Ok(ans) = self.res_nsend(&q) else {
            self.h_errno = TRY_AGAIN;
            return None;
        };
        let rcode = ans[3] & 0x0f;
        let ancount = u16::from_be_bytes([ans[6], ans[7]]);
        if rcode != NOERROR || ancount == 0 {
            self.h_errno = match rcode {
                NXDOMAIN => HOST_NOT_FOUND,
                SERVFAIL => TRY_AGAIN,
                NOERROR => NO_DATA,
                FORMERR | NOTIMP | REFUSED => NO_RECOVERY,
                _ => NO_RECOVERY,
            };
            return None;
        }
        Some(ans)
    }

    /// __res_context_querydomain()
    fn res_nquerydomain(
        &mut self,
        name: &[u8],
        domain: Option<&[u8]>,
        qtype: u16,
    ) -> Option<Vec<u8>> {
        const MAXDNAME: usize = 1025;
        let longname = match domain {
            None => {
                if name.is_empty() || name.len() >= MAXDNAME {
                    self.h_errno = NO_RECOVERY;
                    return None;
                }
                name.to_vec()
            }
            Some(d) => {
                if name.len() + d.len() + 1 >= MAXDNAME {
                    self.h_errno = NO_RECOVERY;
                    return None;
                }
                let mut n = name.to_vec();
                n.push(b'.');
                n.extend_from_slice(d);
                n
            }
        };
        self.res_nquery(&longname, qtype)
    }

    /// __res_context_search()
    fn res_nsearch(&mut self, name: &[u8], qtype: u16) -> Option<Vec<u8>> {
        self.errno = 0;
        self.h_errno = HOST_NOT_FOUND;
        let dots = name.iter().filter(|&&c| c == b'.').count() as u32;
        let trailing_dot = name.last() == Some(&b'.');
        let mut saved_herrno = -1;
        let mut tried_as_is = false;
        let mut searched = false;
        let mut root_on_list = false;
        let mut got_nodata = false;
        let mut got_servfail = false;

        if dots >= self.conf.ndots || trailing_dot {
            let r = self.res_nquerydomain(name, None, qtype);
            if r.is_some() || trailing_dot {
                return r;
            }
            saved_herrno = self.h_errno;
            tried_as_is = true;
        }

        if dots == 0 || !trailing_dot {
            let search = self.conf.search.clone();
            for dname in &search {
                searched = true;
                let mut d: &[u8] = dname;
                if d.first() == Some(&b'.') {
                    d = &d[1..];
                }
                if d.is_empty() {
                    root_on_list = true;
                }
                if let Some(r) = self.res_nquerydomain(name, Some(d), qtype) {
                    return Some(r);
                }
                if self.errno == libc::ECONNREFUSED {
                    self.h_errno = TRY_AGAIN;
                    return None;
                }
                let done = match self.h_errno {
                    NO_DATA => {
                        got_nodata = true;
                        false
                    }
                    HOST_NOT_FOUND => false,
                    TRY_AGAIN if self.rcode == SERVFAIL => {
                        got_servfail = true;
                        false
                    }
                    _ => true,
                };
                if done {
                    break;
                }
            }
        }

        if (dots > 0 || !searched || !self.conf.no_tld_query) && !(tried_as_is || root_on_list) {
            if let Some(r) = self.res_nquerydomain(name, None, qtype) {
                return Some(r);
            }
        }

        if saved_herrno != -1 {
            self.h_errno = saved_herrno;
        } else if got_nodata {
            self.h_errno = NO_DATA;
        } else if got_servfail {
            self.h_errno = TRY_AGAIN;
        }
        None
    }
}

/// php_dns_search() on a fresh res_ninit() handle.
fn dns_search(name: &[u8], qtype: u16) -> Result<Vec<u8>, i32> {
    let mut res = Resolver::new();
    match res.res_nsearch(name, qtype) {
        Some(mut ans) => {
            ans.truncate(ANSWER_SIZE);
            Ok(ans)
        }
        None => Err(res.h_errno),
    }
}

// ---------------------------------------------------------------------------
// dns_check_record() / checkdnsrr()
// ---------------------------------------------------------------------------

fn dns_check_record_impl(vm: &mut Vm, args: &[Value], func: &str) -> Result<Value, VmError> {
    let hostname = path_arg(vm, func, args, 0, "hostname")?;
    let rectype = if args.len() > 1 {
        Some(str_arg(vm, args, 1)?)
    } else {
        None
    };
    if hostname.is_empty() {
        return Err(value_error(vm, func, 1, "hostname", "must not be empty"));
    }
    let mut qtype = DNS_T_MX;
    if let Some(rt) = rectype {
        let types: [(&[u8], u16); 13] = [
            (b"A", DNS_T_A),
            (b"NS", DNS_T_NS),
            (b"MX", DNS_T_MX),
            (b"PTR", DNS_T_PTR),
            (b"ANY", DNS_T_ANY),
            (b"SOA", DNS_T_SOA),
            (b"CAA", DNS_T_CAA),
            (b"TXT", DNS_T_TXT),
            (b"CNAME", DNS_T_CNAME),
            (b"AAAA", DNS_T_AAAA),
            (b"SRV", DNS_T_SRV),
            (b"NAPTR", DNS_T_NAPTR),
            (b"A6", DNS_T_A6),
        ];
        match types.iter().find(|(n, _)| n.eq_ignore_ascii_case(&rt)) {
            Some(&(_, t)) => qtype = t,
            None => {
                return Err(value_error(
                    vm,
                    func,
                    2,
                    "type",
                    "must be a valid DNS record type",
                ));
            }
        }
    }
    match dns_search(&hostname, qtype) {
        Ok(ans) => Ok(if u16::from_be_bytes([ans[6], ans[7]]) != 0 {
            Value::True
        } else {
            Value::False
        }),
        Err(_) => Ok(Value::False),
    }
}

fn dns_check_record_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    dns_check_record_impl(vm, args, "dns_check_record")
}

fn checkdnsrr_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    dns_check_record_impl(vm, args, "checkdnsrr")
}

// ---------------------------------------------------------------------------
// php_parserr()
// ---------------------------------------------------------------------------

fn get16(b: &[u8], p: usize) -> u16 {
    ((*b.get(p).unwrap_or(&0) as u16) << 8) | *b.get(p + 1).unwrap_or(&0) as u16
}

fn get32(b: &[u8], p: usize) -> u32 {
    ((get16(b, p) as u32) << 16) | get16(b, p + 2) as u32
}

/// The IPv6 text formatter shared by the AAAA and A6 cases of php_parserr().
#[derive(Default)]
struct V6Fmt {
    out: Vec<u8>,
    have_break: bool,
    in_break: bool,
}

impl V6Fmt {
    fn short(&mut self, s: u16) {
        if s != 0 {
            if !self.out.is_empty() {
                self.in_break = false;
                self.out.push(b':');
            }
            self.out.extend_from_slice(format!("{:x}", s).as_bytes());
        } else if !self.have_break {
            self.have_break = true;
            self.in_break = true;
            self.out.push(b':');
        } else if !self.in_break {
            self.out.extend_from_slice(b":0");
        }
    }

    fn finish(mut self) -> Vec<u8> {
        if self.have_break && self.in_break {
            self.out.push(b':');
        }
        self.out
    }
}

const NAME_DSTSIZ: usize = MAXHOSTNAMELEN - 2;

/// php_parserr(): parse the resource record at `cp`.  Returns the new
/// position (None = stop parsing) and the record array, if any.
fn parserr(
    msg: &[u8],
    cp: usize,
    type_to_fetch: u16,
    store: bool,
    raw: bool,
) -> (Option<usize>, Option<PhpArray>) {
    let end = msg.len();
    let Some((name, n)) = dn_expand(msg, cp, NAME_DSTSIZ) else {
        return (None, None);
    };
    let mut cp = cp + n;
    if cp + 10 > end {
        return (None, None);
    }
    let rtype = get16(msg, cp);
    let ttl = get32(msg, cp + 4);
    let dlen = get16(msg, cp + 8) as usize;
    cp += 10;
    if cp + dlen > end {
        return (None, None);
    }
    if dlen == 0 {
        return (None, None);
    }
    if type_to_fetch != DNS_T_ANY && rtype != type_to_fetch {
        return (Some(cp + dlen), None);
    }
    if !store {
        return (Some(cp + dlen), None);
    }

    let mut sub = PhpArray::new();
    assoc(&mut sub, "host", bstr(&name));
    assoc(&mut sub, "class", bstr(b"IN"));
    assoc(&mut sub, "ttl", Value::Long(ttl as i64));

    if raw {
        assoc(&mut sub, "type", Value::Long(rtype as i64));
        assoc(&mut sub, "data", bstr(&msg[cp..cp + dlen]));
        return (Some(cp + dlen), Some(sub));
    }

    macro_rules! checkcp {
        ($n:expr) => {
            if cp + ($n) > end {
                return (None, Some(sub));
            }
        };
    }
    macro_rules! expand {
        () => {
            match dn_expand(msg, cp, NAME_DSTSIZ) {
                Some((nm, n)) => {
                    cp += n;
                    nm
                }
                None => return (None, Some(sub)),
            }
        };
    }
    macro_rules! counted {
        () => {{
            checkcp!(1);
            let n = msg[cp] as usize;
            cp += 1;
            checkcp!(n);
            let s = &msg[cp..cp + n];
            cp += n;
            bstr(s)
        }};
    }

    match rtype {
        DNS_T_A => {
            checkcp!(4);
            assoc(&mut sub, "type", bstr(b"A"));
            assoc(
                &mut sub,
                "ip",
                bstr(inet_ntop4(&msg[cp..cp + 4]).as_bytes()),
            );
            cp += dlen;
        }
        DNS_T_MX | DNS_T_CNAME | DNS_T_NS | DNS_T_PTR => {
            if rtype == DNS_T_MX {
                checkcp!(2);
                assoc(&mut sub, "type", bstr(b"MX"));
                assoc(&mut sub, "pri", Value::Long(get16(msg, cp) as i64));
                cp += 2;
            } else {
                let t: &[u8] = match rtype {
                    DNS_T_CNAME => b"CNAME",
                    DNS_T_NS => b"NS",
                    _ => b"PTR",
                };
                assoc(&mut sub, "type", bstr(t));
            }
            let target = expand!();
            assoc(&mut sub, "target", bstr(&target));
        }
        DNS_T_HINFO => {
            assoc(&mut sub, "type", bstr(b"HINFO"));
            let cpu = counted!();
            assoc(&mut sub, "cpu", cpu);
            let os = counted!();
            assoc(&mut sub, "os", os);
        }
        DNS_T_CAA => {
            assoc(&mut sub, "type", bstr(b"CAA"));
            checkcp!(1);
            assoc(&mut sub, "flags", Value::Long(msg[cp] as i64));
            cp += 1;
            checkcp!(1);
            let n = msg[cp] as usize;
            cp += 1;
            checkcp!(n);
            assoc(&mut sub, "tag", bstr(&msg[cp..cp + n]));
            cp += n;
            if dlen < n + 2 {
                return (None, Some(sub));
            }
            let n = dlen - n - 2;
            checkcp!(n);
            assoc(&mut sub, "value", bstr(&msg[cp..cp + n]));
            cp += n;
        }
        DNS_T_TXT => {
            assoc(&mut sub, "type", bstr(b"TXT"));
            let data = &msg[cp..cp + dlen];
            let mut txt = Vec::with_capacity(dlen);
            let mut entries = PhpArray::new();
            let mut l1 = 0usize;
            while l1 < dlen {
                let mut n = data[l1] as usize;
                if l1 + n >= dlen {
                    // Invalid chunk length, truncate
                    n = dlen - (l1 + 1);
                }
                if n > 0 {
                    let chunk = &data[l1 + 1..l1 + 1 + n];
                    txt.extend_from_slice(chunk);
                    entries.push(bstr(chunk));
                }
                l1 += n + 1;
            }
            cp += dlen;
            assoc(&mut sub, "txt", bstr(&txt));
            assoc(&mut sub, "entries", arr_val(entries));
        }
        DNS_T_SOA => {
            assoc(&mut sub, "type", bstr(b"SOA"));
            let mname = expand!();
            assoc(&mut sub, "mname", bstr(&mname));
            let rname = expand!();
            assoc(&mut sub, "rname", bstr(&rname));
            checkcp!(5 * 4);
            for key in ["serial", "refresh", "retry", "expire", "minimum-ttl"] {
                assoc(&mut sub, key, Value::Long(get32(msg, cp) as i64));
                cp += 4;
            }
        }
        DNS_T_AAAA => {
            checkcp!(16);
            let mut f = V6Fmt::default();
            for _ in 0..8 {
                f.short(get16(msg, cp));
                cp += 2;
            }
            assoc(&mut sub, "type", bstr(b"AAAA"));
            assoc(&mut sub, "ipv6", bstr(&f.finish()));
        }
        DNS_T_A6 => {
            let p = cp;
            assoc(&mut sub, "type", bstr(b"A6"));
            checkcp!(1);
            let n = msg[cp] as usize;
            cp += 1;
            assoc(&mut sub, "masklen", Value::Long(n as i64));
            let mut f = V6Fmt::default();
            if n > 15 {
                f.have_break = true;
                f.in_break = true;
                f.out.push(b':');
            }
            if n % 16 > 8 {
                // Partial short.  php-src prints the digits without
                // advancing its cursor, so they never survive.
                if *msg.get(cp).unwrap_or(&0) != 0 {
                    if !f.out.is_empty() {
                        f.in_break = false;
                        f.out.push(b':');
                    }
                } else if !f.have_break {
                    f.have_break = true;
                    f.in_break = true;
                    f.out.push(b':');
                } else if !f.in_break {
                    f.out.extend_from_slice(b":0");
                }
                cp += 1;
            }
            for _ in (n + 8) / 16..8 {
                checkcp!(2);
                f.short(get16(msg, cp));
                cp += 2;
            }
            assoc(&mut sub, "ipv6", bstr(&f.finish()));
            if cp < p + dlen {
                let chain = expand!();
                assoc(&mut sub, "chain", bstr(&chain));
            }
        }
        DNS_T_SRV => {
            checkcp!(3 * 2);
            assoc(&mut sub, "type", bstr(b"SRV"));
            for key in ["pri", "weight", "port"] {
                assoc(&mut sub, key, Value::Long(get16(msg, cp) as i64));
                cp += 2;
            }
            let target = expand!();
            assoc(&mut sub, "target", bstr(&target));
        }
        DNS_T_NAPTR => {
            checkcp!(2 * 2);
            assoc(&mut sub, "type", bstr(b"NAPTR"));
            assoc(&mut sub, "order", Value::Long(get16(msg, cp) as i64));
            cp += 2;
            assoc(&mut sub, "pref", Value::Long(get16(msg, cp) as i64));
            cp += 2;
            let flags = counted!();
            assoc(&mut sub, "flags", flags);
            let services = counted!();
            assoc(&mut sub, "services", services);
            let regex = counted!();
            assoc(&mut sub, "regex", regex);
            let repl = expand!();
            assoc(&mut sub, "replacement", bstr(&repl));
        }
        _ => {
            return (Some(cp + dlen), None);
        }
    }
    (Some(cp), Some(sub))
}

// ---------------------------------------------------------------------------
// dns_get_record()
// ---------------------------------------------------------------------------

fn dns_get_record_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    const F: &str = "dns_get_record";
    let hostname = path_arg(vm, F, args, 0, "hostname")?;
    let type_param = if args.len() > 1 {
        long_arg(args, 1)
    } else {
        PHP_DNS_ANY
    };
    let raw = args.get(4).map(|v| v.deref().is_truthy()).unwrap_or(false);
    let authns = try_array_init(args.get(2));
    let addtl = try_array_init(args.get(3));

    if !raw {
        if (type_param & !PHP_DNS_ALL) != 0 && type_param != PHP_DNS_ANY {
            return Err(value_error(vm, F, 2, "type", "must be a DNS_* constant"));
        }
    } else if !(1..=0xFFFF).contains(&type_param) {
        return Err(value_error(
            vm,
            F,
            2,
            "type",
            "must be between 1 and 65535 when argument #5 ($raw) is true",
        ));
    }

    let mut result = PhpArray::new();
    let mut ty: i32 = if raw {
        -1
    } else if type_param == PHP_DNS_ANY {
        PHP_DNS_NUM_TYPES + 1
    } else {
        0
    };
    let limit = if addtl.is_some() {
        PHP_DNS_NUM_TYPES + 2
    } else {
        PHP_DNS_NUM_TYPES
    };
    let mut first_query = true;
    let mut store_results = true;
    let has = |mask: i64, t: u16| if type_param & mask != 0 { t } else { 0 };

    while ty < limit || first_query {
        first_query = false;
        let type_to_fetch: u16 = match ty {
            -1 => {
                ty = PHP_DNS_NUM_TYPES - 1;
                type_param as u16
            }
            0 => has(PHP_DNS_A, DNS_T_A),
            1 => has(PHP_DNS_NS, DNS_T_NS),
            2 => has(PHP_DNS_CNAME, DNS_T_CNAME),
            3 => has(PHP_DNS_SOA, DNS_T_SOA),
            4 => has(PHP_DNS_PTR, DNS_T_PTR),
            5 => has(PHP_DNS_HINFO, DNS_T_HINFO),
            6 => has(PHP_DNS_MX, DNS_T_MX),
            7 => has(PHP_DNS_TXT, DNS_T_TXT),
            8 => has(PHP_DNS_AAAA, DNS_T_AAAA),
            9 => has(PHP_DNS_SRV, DNS_T_SRV),
            10 => has(PHP_DNS_NAPTR, DNS_T_NAPTR),
            11 => has(PHP_DNS_A6, DNS_T_A6),
            12 => has(PHP_DNS_CAA, DNS_T_CAA),
            PHP_DNS_NUM_TYPES => {
                store_results = false;
                ty += 1;
                continue;
            }
            _ => DNS_T_ANY,
        };
        ty += 1;
        if type_to_fetch == 0 {
            continue;
        }

        let answer = match dns_search(&hostname, type_to_fetch) {
            Ok(a) => a,
            Err(herr) => {
                match herr {
                    NO_DATA | HOST_NOT_FOUND => continue,
                    NO_RECOVERY => {
                        vm.emit_warning("dns_get_record(): An unexpected server failure occurred.")
                    }
                    TRY_AGAIN => {
                        vm.emit_warning("dns_get_record(): A temporary server error occurred.")
                    }
                    _ => vm.emit_warning("dns_get_record(): DNS Query failed"),
                }
                return Ok(Value::False);
            }
        };

        let end = answer.len();
        let mut qd = get16(&answer, 4);
        let mut an = get16(&answer, 6);
        let mut ns = get16(&answer, 8);
        let mut ar = get16(&answer, 10);
        let mut cp: Option<usize> = Some(HFIXEDSZ);

        // Skip QD entries, they're only used by dn_expand later on
        while qd > 0 {
            qd -= 1;
            let pos = cp.unwrap_or(HFIXEDSZ);
            match dn_skipname(&answer, pos) {
                Some(n) => cp = Some(pos + n + QFIXEDSZ),
                None => {
                    vm.emit_warning("dns_get_record(): Unable to parse DNS data received");
                    return Ok(Value::False);
                }
            }
        }

        // YAY! Our real answers!
        while an > 0 {
            an -= 1;
            let Some(pos) = cp.filter(|&p| p < end) else {
                break;
            };
            let (next, rec) = parserr(&answer, pos, type_to_fetch, store_results, raw);
            cp = next;
            if let Some(rec) = rec {
                if store_results {
                    result.push(arr_val(rec));
                }
            }
        }

        if authns.is_some() || addtl.is_some() {
            while ns > 0 {
                ns -= 1;
                let Some(pos) = cp.filter(|&p| p < end) else {
                    break;
                };
                let (next, rec) = parserr(&answer, pos, DNS_T_ANY, authns.is_some(), raw);
                cp = next;
                if let (Some(rec), Some(a)) = (rec, &authns) {
                    a.borrow_mut().push(arr_val(rec));
                }
            }
        }

        if let Some(a) = &addtl {
            while ar > 0 {
                ar -= 1;
                let Some(pos) = cp.filter(|&p| p < end) else {
                    break;
                };
                let (next, rec) = parserr(&answer, pos, DNS_T_ANY, true, raw);
                cp = next;
                if let Some(rec) = rec {
                    a.borrow_mut().push(arr_val(rec));
                }
            }
        }
    }
    Ok(arr_val(result))
}

// ---------------------------------------------------------------------------
// dns_get_mx() / getmxrr()
// ---------------------------------------------------------------------------

fn dns_get_mx_impl(vm: &mut Vm, args: &[Value], func: &str) -> Result<Value, VmError> {
    let hostname = path_arg(vm, func, args, 0, "hostname")?;
    let Some(mx_list) = try_array_init(args.get(1)) else {
        return Ok(Value::False);
    };
    let weight_list = try_array_init(args.get(2));

    let Ok(answer) = dns_search(&hostname, DNS_T_MX) else {
        return Ok(Value::False);
    };
    let end = answer.len();
    let mut cp = HFIXEDSZ;
    for _ in 0..get16(&answer, 4) {
        match dn_skipname(&answer, cp) {
            Some(i) => cp += i + QFIXEDSZ,
            None => return Ok(Value::False),
        }
    }
    let mut count = get16(&answer, 6) as i32;
    loop {
        count -= 1;
        if count < 0 || cp >= end {
            break;
        }
        let Some(i) = dn_skipname(&answer, cp) else {
            return Ok(Value::False);
        };
        cp += i;
        let rtype = get16(&answer, cp);
        cp += 2 + 2 + 4;
        let dlen = get16(&answer, cp) as usize;
        cp += 2;
        if rtype != DNS_T_MX {
            cp += dlen;
            continue;
        }
        let weight = get16(&answer, cp);
        cp += 2;
        let Some((name, i)) = dn_expand(&answer, cp, MAXHOSTNAMELEN - 1) else {
            return Ok(Value::False);
        };
        cp += i;
        mx_list.borrow_mut().push(bstr(&name));
        if let Some(w) = &weight_list {
            w.borrow_mut().push(Value::Long(weight as i64));
        }
    }
    let n = mx_list.borrow().len();
    Ok(if n != 0 { Value::True } else { Value::False })
}

fn dns_get_mx_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    dns_get_mx_impl(vm, args, "dns_get_mx")
}

fn getmxrr_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    dns_get_mx_impl(vm, args, "getmxrr")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pton4_strict() {
        assert_eq!(inet_pton4(b"1.2.3.4"), Some([1, 2, 3, 4]));
        assert_eq!(inet_pton4(b"01.2.3.4"), None);
        assert_eq!(inet_pton4(b"1.2.3"), None);
        assert_eq!(inet_pton4(b"1.2.3.4."), None);
        assert_eq!(inet_pton4(b"256.2.3.4"), None);
        assert_eq!(inet_pton4(b" 1.2.3.4"), None);
    }

    #[test]
    fn ntop6_forms() {
        let f = |s: &str| inet_ntop6(&inet_pton6(s.as_bytes()).unwrap());
        assert_eq!(f("::1.2.3.4"), "::1.2.3.4");
        assert_eq!(f("::ffff:1.2.3.4"), "::ffff:1.2.3.4");
        assert_eq!(f("1:0:0:2:0:0:0:3"), "1:0:0:2::3");
        assert_eq!(
            f("2001:0db8:1234:0000:0000:0000:0000:0000"),
            "2001:db8:1234::"
        );
        assert_eq!(f("::"), "::");
        assert!(inet_pton6(b":1::").is_none());
        assert!(inet_pton6(b"1::2::3").is_none());
    }

    #[test]
    fn names() {
        assert_eq!(ns_name_pton(b"."), Some(vec![0]));
        assert_eq!(ns_name_pton(b"a.b"), Some(vec![1, b'a', 1, b'b', 0]));
        assert_eq!(ns_name_pton(b"a.b."), Some(vec![1, b'a', 1, b'b', 0]));
        assert!(ns_name_pton(b".a").is_none());
        assert!(ns_name_pton(b"a..b").is_none());
        let msg = [0u8, 0, 1, b'a', 0, 0xc0, 2];
        let (n, l) = dn_expand(&msg, 5, 100).unwrap();
        assert_eq!((n.as_slice(), l), (&b"a"[..], 2));
        let (n, _) = dn_expand(&[0u8], 0, 100).unwrap();
        assert!(n.is_empty());
    }

    #[test]
    fn refused_nameserver() {
        let mut res = Resolver::new();
        res.conf.servers = vec![SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 1)];
        res.conf.search = vec![b"example".to_vec()];
        assert!(res.res_nsearch(b"x", DNS_T_A).is_none());
        assert_eq!(res.h_errno, TRY_AGAIN);
        assert_eq!(res.errno, libc::ECONNREFUSED);
        assert!(res.res_nsearch(b"", DNS_T_A).is_none());
        assert_eq!(res.h_errno, NO_RECOVERY);
    }

    #[test]
    fn aaaa_format() {
        let f = |s: [u16; 8]| {
            let mut v = V6Fmt::default();
            for x in s {
                v.short(x);
            }
            String::from_utf8(v.finish()).unwrap()
        };
        assert_eq!(f([0x2001, 0xdb8, 0, 0, 1, 0, 0, 1]), "2001:db8::1:0:0:1");
        assert_eq!(f([0, 0, 0, 0, 0, 0, 0, 1]), "::1");
        assert_eq!(f([0; 8]), "::");
        assert_eq!(f([1, 0, 0, 0, 0, 0, 0, 0]), "1::");
    }
}
