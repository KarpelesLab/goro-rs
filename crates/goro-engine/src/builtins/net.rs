//! Network helpers of ext/standard: ip2long(), long2ip(), inet_pton(),
//! inet_ntop(), gethostbyname(), gethostbynamel(), gethostbyaddr(),
//! getprotobyname(), getprotobynumber(), the DNS functions and syslog
//! (PHP manual pages of these functions; observed on PHP 8.5). Names are
//! resolved by the C library; DNS queries go to the nameservers of
//! /etc/resolv.conf (goro_ext_standard::dns builds and parses them).

use std::ffi::{CStr, CString};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use goro_ext_standard::dns;
use goro_runtime::{Key, PhpArray, PhpStr, Value};

use crate::func::Args;
use crate::registry::{ConstVal, Module};
use crate::vm::{R, Vm};

pub static MODULE: Module = Module {
    name: "standard",
    functions: &[
        ("function ip2long(string $ip): int|false", ip2long),
        ("function long2ip(int $ip): string", long2ip),
        ("function inet_pton(string $ip): string|false", inet_pton),
        ("function inet_ntop(string $ip): string|false", inet_ntop),
        (
            "function gethostbyname(string $hostname): string",
            gethostbyname,
        ),
        (
            "function gethostbynamel(string $hostname): array|false",
            gethostbynamel,
        ),
        (
            "function gethostbyaddr(string $ip): string|false",
            gethostbyaddr,
        ),
        (
            "function getprotobyname(string $protocol): int|false",
            getprotobyname,
        ),
        (
            "function getprotobynumber(int $protocol): string|false",
            getprotobynumber,
        ),
        (
            "function dns_check_record(string $hostname, string $type = \"MX\"): bool",
            dns_check_record,
        ),
        (
            "function checkdnsrr(string $hostname, string $type = \"MX\"): bool",
            dns_check_record,
        ),
        (
            "function dns_get_mx(string $hostname, &$hosts, &$weights = null): bool",
            getmxrr,
        ),
        (
            "function getmxrr(string $hostname, &$hosts, &$weights = null): bool",
            getmxrr,
        ),
        (
            "function dns_get_record(string $hostname, int $type = DNS_ANY, &$authoritative_name_servers = null, &$additional_records = null, bool $raw = false): array|false",
            dns_get_record,
        ),
        (
            "function openlog(string $prefix, int $flags, int $facility): true",
            openlog,
        ),
        ("function closelog(): true", closelog),
        (
            "function syslog(int $priority, string $message): true",
            syslog,
        ),
    ],
    constants: &[
        ("DNS_A", ConstVal::Int(1)),
        ("DNS_NS", ConstVal::Int(2)),
        ("DNS_CNAME", ConstVal::Int(16)),
        ("DNS_SOA", ConstVal::Int(32)),
        ("DNS_PTR", ConstVal::Int(2048)),
        ("DNS_HINFO", ConstVal::Int(4096)),
        ("DNS_CAA", ConstVal::Int(8192)),
        ("DNS_MX", ConstVal::Int(16384)),
        ("DNS_TXT", ConstVal::Int(32768)),
        ("DNS_SRV", ConstVal::Int(33554432)),
        ("DNS_NAPTR", ConstVal::Int(67108864)),
        ("DNS_AAAA", ConstVal::Int(134217728)),
        ("DNS_A6", ConstVal::Int(16777216)),
        ("DNS_ANY", ConstVal::Int(dns::DNS_ANY)),
        ("DNS_ALL", ConstVal::Int(dns::DNS_ALL)),
        ("LOG_EMERG", ConstVal::Int(0)),
        ("LOG_ALERT", ConstVal::Int(1)),
        ("LOG_CRIT", ConstVal::Int(2)),
        ("LOG_ERR", ConstVal::Int(3)),
        ("LOG_WARNING", ConstVal::Int(4)),
        ("LOG_NOTICE", ConstVal::Int(5)),
        ("LOG_INFO", ConstVal::Int(6)),
        ("LOG_DEBUG", ConstVal::Int(7)),
        ("LOG_KERN", ConstVal::Int(0)),
        ("LOG_USER", ConstVal::Int(8)),
        ("LOG_MAIL", ConstVal::Int(16)),
        ("LOG_DAEMON", ConstVal::Int(24)),
        ("LOG_AUTH", ConstVal::Int(32)),
        ("LOG_SYSLOG", ConstVal::Int(40)),
        ("LOG_LPR", ConstVal::Int(48)),
        ("LOG_NEWS", ConstVal::Int(56)),
        ("LOG_UUCP", ConstVal::Int(64)),
        ("LOG_CRON", ConstVal::Int(72)),
        ("LOG_AUTHPRIV", ConstVal::Int(80)),
        ("LOG_LOCAL0", ConstVal::Int(128)),
        ("LOG_LOCAL1", ConstVal::Int(136)),
        ("LOG_LOCAL2", ConstVal::Int(144)),
        ("LOG_LOCAL3", ConstVal::Int(152)),
        ("LOG_LOCAL4", ConstVal::Int(160)),
        ("LOG_LOCAL5", ConstVal::Int(168)),
        ("LOG_LOCAL6", ConstVal::Int(176)),
        ("LOG_LOCAL7", ConstVal::Int(184)),
        ("LOG_PID", ConstVal::Int(1)),
        ("LOG_CONS", ConstVal::Int(2)),
        ("LOG_ODELAY", ConstVal::Int(4)),
        ("LOG_NDELAY", ConstVal::Int(8)),
        ("LOG_NOWAIT", ConstVal::Int(16)),
        ("LOG_PERROR", ConstVal::Int(32)),
    ],
    listed: false,
    ..Module::EMPTY
};

fn sv(s: &str) -> Value {
    Value::Str(PhpStr::new(s.as_bytes()))
}

/// ValueError for a string argument holding a NUL byte.
fn no_nul(vm: &mut Vm, a: &Args, i: usize, name: &str) -> R<bool> {
    if a.bytes(i).contains(&0) {
        vm.fthrow::<()>(
            a,
            "ValueError",
            &format!(
                "Argument #{} (${name}) must not contain any null bytes",
                i + 1
            ),
        )?;
        return Ok(false);
    }
    Ok(true)
}

fn parse_v4(b: &[u8]) -> Option<Ipv4Addr> {
    std::str::from_utf8(b).ok()?.parse().ok()
}

fn ip2long(vm: &mut Vm, a: &mut Args) -> R<Value> {
    if !no_nul(vm, a, 0, "ip")? {
        return Ok(Value::Null);
    }
    Ok(match parse_v4(a.bytes(0)) {
        Some(ip) => Value::Int(u32::from(ip) as i64),
        None => Value::Bool(false),
    })
}

fn long2ip(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(sv(&Ipv4Addr::from(a.int(0) as u32).to_string()))
}

fn inet_pton(vm: &mut Vm, a: &mut Args) -> R<Value> {
    if !no_nul(vm, a, 0, "ip")? {
        return Ok(Value::Null);
    }
    let s = a.bytes(0);
    let Ok(t) = std::str::from_utf8(s) else {
        return Ok(Value::Bool(false));
    };
    if t.contains(':') {
        if let Ok(ip) = t.parse::<Ipv6Addr>() {
            return Ok(Value::Str(PhpStr::new(&ip.octets())));
        }
    } else if let Ok(ip) = t.parse::<Ipv4Addr>() {
        return Ok(Value::Str(PhpStr::new(&ip.octets())));
    }
    Ok(Value::Bool(false))
}

fn inet_ntop(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let b = a.bytes(0);
    Ok(match b.len() {
        4 => sv(&Ipv4Addr::new(b[0], b[1], b[2], b[3]).to_string()),
        16 => {
            let mut o = [0u8; 16];
            o.copy_from_slice(b);
            sv(&dns::ipv6_text(&o))
        }
        _ => Value::Bool(false),
    })
}

/// The longest host name the resolver accepts.
const MAX_HOST: usize = 255;

fn host_arg(vm: &mut Vm, a: &Args) -> R<Option<Vec<u8>>> {
    if !no_nul(vm, a, 0, "hostname")? {
        return Ok(None);
    }
    let h = a.bytes(0).to_vec();
    if h.len() > MAX_HOST {
        vm.fwarn(
            a,
            &format!("Host name cannot be longer than {MAX_HOST} characters"),
        )?;
        return Ok(None);
    }
    Ok(Some(h))
}

/// The IPv4 addresses of `host`, through getaddrinfo().
fn resolve_v4(host: &[u8]) -> Vec<Ipv4Addr> {
    let Ok(c) = CString::new(host) else {
        return Vec::new();
    };
    let mut hints: libc::addrinfo = unsafe { std::mem::zeroed() };
    hints.ai_family = libc::AF_INET;
    hints.ai_socktype = libc::SOCK_STREAM;
    let mut res: *mut libc::addrinfo = std::ptr::null_mut();
    let rc = unsafe { libc::getaddrinfo(c.as_ptr(), std::ptr::null(), &hints, &mut res) };
    if rc != 0 {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut p = res;
    while !p.is_null() {
        let ai = unsafe { &*p };
        if ai.ai_family == libc::AF_INET && !ai.ai_addr.is_null() {
            let sa = unsafe { &*(ai.ai_addr as *const libc::sockaddr_in) };
            let ip = Ipv4Addr::from(u32::from_be(sa.sin_addr.s_addr));
            if !out.contains(&ip) {
                out.push(ip);
            }
        }
        p = ai.ai_next;
    }
    unsafe { libc::freeaddrinfo(res) };
    out
}

fn gethostbyname(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let Some(h) = host_arg(vm, a)? else {
        return Ok(Value::Str(a.str(0)));
    };
    Ok(match resolve_v4(&h).first() {
        Some(ip) => sv(&ip.to_string()),
        None => Value::Str(PhpStr::from(h)),
    })
}

fn gethostbynamel(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let Some(h) = host_arg(vm, a)? else {
        return Ok(Value::Bool(false));
    };
    let ips = resolve_v4(&h);
    if ips.is_empty() {
        return Ok(Value::Bool(false));
    }
    Ok(Value::Array(PhpArray::from_values(
        ips.iter().map(|ip| sv(&ip.to_string())),
    )))
}

fn gethostbyaddr(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let s = String::from_utf8_lossy(a.bytes(0)).into_owned();
    let Ok(ip) = s.parse::<IpAddr>() else {
        vm.fwarn(a, "Address is not a valid IPv4 or IPv6 address")?;
        return Ok(Value::Bool(false));
    };
    let sa = std::net::SocketAddr::new(ip, 0);
    let (storage, len) = sockaddr(&sa);
    let mut host = [0 as libc::c_char; 1025];
    let rc = unsafe {
        libc::getnameinfo(
            &storage as *const libc::sockaddr_storage as *const libc::sockaddr,
            len,
            host.as_mut_ptr(),
            host.len() as libc::socklen_t,
            std::ptr::null_mut(),
            0,
            libc::NI_NAMEREQD,
        )
    };
    if rc != 0 {
        return Ok(sv(&s));
    }
    let name = unsafe { CStr::from_ptr(host.as_ptr()) };
    Ok(Value::Str(PhpStr::new(name.to_bytes())))
}

fn sockaddr(sa: &std::net::SocketAddr) -> (libc::sockaddr_storage, libc::socklen_t) {
    let mut st: libc::sockaddr_storage = unsafe { std::mem::zeroed() };
    match sa {
        std::net::SocketAddr::V4(v) => {
            let p = &mut st as *mut _ as *mut libc::sockaddr_in;
            unsafe {
                (*p).sin_family = libc::AF_INET as libc::sa_family_t;
                (*p).sin_addr.s_addr = u32::from(*v.ip()).to_be();
            }
            (
                st,
                std::mem::size_of::<libc::sockaddr_in>() as libc::socklen_t,
            )
        }
        std::net::SocketAddr::V6(v) => {
            let p = &mut st as *mut _ as *mut libc::sockaddr_in6;
            unsafe {
                (*p).sin6_family = libc::AF_INET6 as libc::sa_family_t;
                (*p).sin6_addr.s6_addr = v.ip().octets();
            }
            (
                st,
                std::mem::size_of::<libc::sockaddr_in6>() as libc::socklen_t,
            )
        }
    }
}

fn getprotobyname(vm: &mut Vm, a: &mut Args) -> R<Value> {
    if !no_nul(vm, a, 0, "protocol")? {
        return Ok(Value::Null);
    }
    let Ok(c) = CString::new(a.bytes(0)) else {
        return Ok(Value::Bool(false));
    };
    let p = unsafe { libc::getprotobyname(c.as_ptr()) };
    if p.is_null() {
        return Ok(Value::Bool(false));
    }
    Ok(Value::Int(unsafe { (*p).p_proto } as i64))
}

fn getprotobynumber(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let Ok(n) = i32::try_from(a.int(0)) else {
        return Ok(Value::Bool(false));
    };
    let p = unsafe { libc::getprotobynumber(n) };
    if p.is_null() {
        return Ok(Value::Bool(false));
    }
    let name = unsafe { CStr::from_ptr((*p).p_name) };
    Ok(Value::Str(PhpStr::new(name.to_bytes())))
}

// ---------------------------------------------------------------- DNS

/// Sends one query to the configured nameservers (UDP, then TCP when the
/// answer is truncated).
fn query(name: &[u8], qtype: u16) -> Option<dns::Response> {
    use std::io::{Read, Write};
    use std::time::Duration;
    let conf = std::fs::read("/etc/resolv.conf").unwrap_or_default();
    let mut servers = dns::nameservers(&conf);
    if servers.is_empty() {
        servers.push(IpAddr::V4(Ipv4Addr::LOCALHOST));
    }
    let id = (std::process::id() as u16) ^ (qtype.rotate_left(7));
    let q = dns::build_query(id, name, qtype)?;
    for srv in servers {
        let addr = std::net::SocketAddr::new(srv, 53);
        let bind: std::net::SocketAddr = if srv.is_ipv4() {
            "0.0.0.0:0".parse().ok()?
        } else {
            "[::]:0".parse().ok()?
        };
        let Ok(sock) = std::net::UdpSocket::bind(bind) else {
            continue;
        };
        let _ = sock.set_read_timeout(Some(Duration::from_secs(5)));
        if sock.send_to(&q, addr).is_err() {
            continue;
        }
        let mut buf = vec![0u8; 65535];
        let mut resp = None;
        for _ in 0..4 {
            match sock.recv_from(&mut buf) {
                Ok((n, from)) if from.ip() == srv => {
                    if let Some(r) = dns::parse_response(&buf[..n])
                        && r.id == id
                    {
                        resp = Some(r);
                        break;
                    }
                }
                Ok(_) => continue,
                Err(_) => break,
            }
        }
        let Some(r) = resp else {
            continue;
        };
        if !r.truncated {
            return Some(r);
        }
        // the whole answer over TCP
        let Ok(mut t) = std::net::TcpStream::connect_timeout(&addr, Duration::from_secs(5)) else {
            return Some(r);
        };
        let _ = t.set_read_timeout(Some(Duration::from_secs(5)));
        let mut msg = (q.len() as u16).to_be_bytes().to_vec();
        msg.extend_from_slice(&q);
        if t.write_all(&msg).is_err() {
            return Some(r);
        }
        let mut lb = [0u8; 2];
        if t.read_exact(&mut lb).is_err() {
            return Some(r);
        }
        let mut body = vec![0u8; u16::from_be_bytes(lb) as usize];
        if t.read_exact(&mut body).is_err() {
            return Some(r);
        }
        return dns::parse_response(&body).or(Some(r));
    }
    None
}

fn dns_check_record(vm: &mut Vm, a: &mut Args) -> R<Value> {
    if a.bytes(0).is_empty() {
        return vm.fthrow(a, "ValueError", "Argument #1 ($hostname) must not be empty");
    }
    if !no_nul(vm, a, 0, "hostname")? {
        return Ok(Value::Null);
    }
    let t = a.opt_str(1).map(|s| s.to_vec()).unwrap_or(b"MX".to_vec());
    let Some(qt) = dns::type_by_name(&t) else {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #2 ($type) must be a valid DNS record type",
        );
    };
    let ok = query(a.bytes(0), qt).is_some_and(|r| r.rcode == 0 && !r.answers.is_empty());
    Ok(Value::Bool(ok))
}

fn getmxrr(vm: &mut Vm, a: &mut Args) -> R<Value> {
    if !no_nul(vm, a, 0, "hostname")? {
        return Ok(Value::Null);
    }
    let mut hosts = PhpArray::new();
    let mut weights = PhpArray::new();
    if let Some(r) = query(a.bytes(0), 15) {
        for rec in &r.answers {
            if let dns::RData::Mx(pri, target) = &rec.data {
                let _ = hosts.push(Value::Str(PhpStr::new(target)));
                let _ = weights.push(Value::Int(*pri as i64));
            }
        }
    }
    let ok = !hosts.is_empty();
    a.set_ref(1, Value::Array(hosts));
    if a.has(2) {
        a.set_ref(2, Value::Array(weights));
    }
    Ok(Value::Bool(ok))
}

fn record_array(rec: &dns::Record, raw: bool) -> PhpArray {
    let mut o = PhpArray::new();
    let k = |s: &str| Key::from_bytes(s.as_bytes());
    let st = |b: &[u8]| Value::Str(PhpStr::new(b));
    o.insert(k("host"), st(&rec.host));
    o.insert(k("class"), sv(if rec.class == 1 { "IN" } else { "" }));
    o.insert(k("ttl"), Value::Int(rec.ttl as i64));
    let name = dns::type_name(rec.rtype);
    if raw || name.is_empty() {
        o.insert(k("type"), Value::Int(rec.rtype as i64));
    } else {
        o.insert(k("type"), sv(name));
    }
    match &rec.data {
        dns::RData::A(ip) => {
            o.insert(k("ip"), sv(&Ipv4Addr::from(*ip).to_string()));
        }
        dns::RData::Aaaa(ip) => {
            o.insert(k("ipv6"), sv(&dns::ipv6_text(ip)));
        }
        dns::RData::Name(n) => {
            o.insert(k("target"), st(n));
        }
        dns::RData::Mx(p, t) => {
            o.insert(k("pri"), Value::Int(*p as i64));
            o.insert(k("target"), st(t));
        }
        dns::RData::Txt(parts) => {
            o.insert(k("txt"), st(&parts.concat()));
            o.insert(
                k("entries"),
                Value::Array(PhpArray::from_values(parts.iter().map(|p| st(p)))),
            );
        }
        dns::RData::Hinfo(cpu, os) => {
            o.insert(k("cpu"), st(cpu));
            o.insert(k("os"), st(os));
        }
        dns::RData::Soa {
            mname,
            rname,
            serial,
            refresh,
            retry,
            expire,
            minimum,
        } => {
            o.insert(k("mname"), st(mname));
            o.insert(k("rname"), st(rname));
            o.insert(k("serial"), Value::Int(*serial as i64));
            o.insert(k("refresh"), Value::Int(*refresh as i64));
            o.insert(k("retry"), Value::Int(*retry as i64));
            o.insert(k("expire"), Value::Int(*expire as i64));
            o.insert(k("minimum-ttl"), Value::Int(*minimum as i64));
        }
        dns::RData::Srv {
            pri,
            weight,
            port,
            target,
        } => {
            o.insert(k("pri"), Value::Int(*pri as i64));
            o.insert(k("weight"), Value::Int(*weight as i64));
            o.insert(k("target"), st(target));
            o.insert(k("port"), Value::Int(*port as i64));
        }
        dns::RData::Naptr {
            order,
            pref,
            flags,
            services,
            regex,
            replacement,
        } => {
            o.insert(k("order"), Value::Int(*order as i64));
            o.insert(k("pref"), Value::Int(*pref as i64));
            o.insert(k("flags"), st(flags));
            o.insert(k("services"), st(services));
            o.insert(k("regex"), st(regex));
            o.insert(k("replacement"), st(replacement));
        }
        dns::RData::Caa { flags, tag, value } => {
            o.insert(k("flags"), Value::Int(*flags as i64));
            o.insert(k("tag"), st(tag));
            o.insert(k("value"), st(value));
        }
        dns::RData::A6 {
            masklen,
            addr,
            chain,
        } => {
            o.insert(k("masklen"), Value::Int(*masklen as i64));
            o.insert(k("ipv6"), sv(&dns::ipv6_text(addr)));
            o.insert(k("chain"), st(chain));
        }
        dns::RData::Raw(d) => {
            o.insert(k("data"), st(d));
        }
    }
    o
}

fn dns_get_record(vm: &mut Vm, a: &mut Args) -> R<Value> {
    if !no_nul(vm, a, 0, "hostname")? {
        return Ok(Value::Null);
    }
    let t = a.int_or(1, dns::DNS_ANY);
    let raw = a.bool_or(4, false);
    let types: Vec<u16> = if raw {
        if !(1..=65535).contains(&t) {
            return vm.fthrow(
                a,
                "ValueError",
                "Argument #2 ($type) must be between 1 and 65535 when argument #5 ($raw) is true",
            );
        }
        vec![t as u16]
    } else if t == dns::DNS_ANY {
        vec![255]
    } else {
        if t & !dns::DNS_ALL != 0 || t == 0 {
            return vm.fthrow(
                a,
                "ValueError",
                "Argument #2 ($type) must be a DNS_* constant",
            );
        }
        dns::DNS_TYPES
            .iter()
            .filter(|(bit, _)| t & bit != 0)
            .map(|(_, q)| *q)
            .collect()
    };
    let mut out = PhpArray::new();
    let mut auth = PhpArray::new();
    let mut add = PhpArray::new();
    let mut any = false;
    for qt in types {
        let Some(r) = query(a.bytes(0), qt) else {
            continue;
        };
        if r.rcode != 0 && r.rcode != 3 {
            continue;
        }
        any = true;
        for rec in &r.answers {
            // only the asked type, unless asking for anything
            if qt != 255 && rec.rtype != qt {
                continue;
            }
            let _ = out.push(Value::Array(record_array(rec, raw)));
        }
        for rec in &r.authority {
            let _ = auth.push(Value::Array(record_array(rec, raw)));
        }
        for rec in &r.additional {
            if rec.rtype == 41 {
                continue;
            }
            let _ = add.push(Value::Array(record_array(rec, raw)));
        }
    }
    if a.has(2) {
        a.set_ref(2, Value::Array(auth));
    }
    if a.has(3) {
        a.set_ref(3, Value::Array(add));
    }
    if !any {
        vm.fwarn(a, "A temporary server error occurred.")?;
        return Ok(Value::Bool(false));
    }
    Ok(Value::Array(out))
}

// ---------------------------------------------------------------- syslog

thread_local! {
    /// openlog() keeps a pointer to its ident: it lives here.
    static IDENT: std::cell::RefCell<Option<CString>> = const { std::cell::RefCell::new(None) };
}

fn openlog(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let ident = CString::new(
        a.bytes(0)
            .iter()
            .copied()
            .filter(|&c| c != 0)
            .collect::<Vec<u8>>(),
    )
    .unwrap_or_default();
    let (flags, fac) = (a.int(1) as libc::c_int, a.int(2) as libc::c_int);
    IDENT.with(|i| {
        let mut i = i.borrow_mut();
        unsafe { libc::openlog(ident.as_ptr(), flags, fac) };
        *i = Some(ident);
    });
    Ok(Value::Bool(true))
}

fn closelog(_vm: &mut Vm, _a: &mut Args) -> R<Value> {
    unsafe { libc::closelog() };
    IDENT.with(|i| *i.borrow_mut() = None);
    Ok(Value::Bool(true))
}

fn syslog(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let pri = a.int(0) as libc::c_int;
    let msg = a.bytes(1).to_vec();
    let filter = vm
        .get_ini(b"syslog.filter")
        .map(|v| v.to_vec())
        .unwrap_or(b"no-ctrl".to_vec());
    vm.out.flush();
    let lines: Vec<&[u8]> = if filter == b"raw" {
        vec![&msg]
    } else {
        msg.split(|&c| c == b'\n').collect()
    };
    for line in lines {
        let mut o = Vec::with_capacity(line.len());
        for &c in line {
            let keep = match filter.as_slice() {
                b"all" | b"raw" => c != 0,
                b"ascii" => (0x20..0x7f).contains(&c) || c == b'\t',
                _ => c >= 0x20 || c == b'\t',
            };
            if keep {
                o.push(c);
            } else {
                o.extend_from_slice(format!("\\x{c:02x}").as_bytes());
            }
        }
        let Ok(c) = CString::new(o) else {
            continue;
        };
        unsafe { libc::syslog(pri, c"%s".as_ptr(), c.as_ptr()) };
    }
    Ok(Value::Bool(true))
}
