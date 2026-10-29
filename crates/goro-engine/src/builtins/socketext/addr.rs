//! Socket addresses: building sockaddr structures from PHP strings,
//! name resolution (getaddrinfo(3)), decoding addresses for
//! socket_getsockname()/socket_recvfrom(), and the AddressInfo functions
//! (PHP manual: socket_addrinfo_lookup() and friends).

use std::ffi::CString;
use std::net::{Ipv4Addr, Ipv6Addr};

use goro_runtime::{Key, KeyRef, PhpArray, Value};

use super::{lookup_error, new_obj, new_socket, with_native};
use crate::func::Args;
use crate::vm::{R, Vm};

/// The size of `sun_path` (the limit socket_bind() and friends report).
pub const SUN_PATH: usize = 108;

pub type Addr = (libc::sockaddr_storage, libc::socklen_t);

fn zeroed() -> libc::sockaddr_storage {
    // SAFETY: plain old data.
    unsafe { std::mem::zeroed() }
}

pub fn inet_sockaddr(ip: Ipv4Addr, port: u16) -> Addr {
    let mut st = zeroed();
    // SAFETY: sockaddr_storage is large enough and suitably aligned.
    let sin = unsafe { &mut *(&mut st as *mut libc::sockaddr_storage).cast::<libc::sockaddr_in>() };
    sin.sin_family = libc::AF_INET as libc::sa_family_t;
    sin.sin_port = port.to_be();
    sin.sin_addr.s_addr = u32::from_ne_bytes(ip.octets());
    (
        st,
        std::mem::size_of::<libc::sockaddr_in>() as libc::socklen_t,
    )
}

pub fn inet6_sockaddr(ip: Ipv6Addr, port: u16, flowinfo: u32, scope: u32) -> Addr {
    let mut st = zeroed();
    // SAFETY: as above.
    let sin =
        unsafe { &mut *(&mut st as *mut libc::sockaddr_storage).cast::<libc::sockaddr_in6>() };
    sin.sin6_family = libc::AF_INET6 as libc::sa_family_t;
    sin.sin6_port = port.to_be();
    sin.sin6_flowinfo = flowinfo.to_be();
    sin.sin6_scope_id = scope;
    sin.sin6_addr.s6_addr = ip.octets();
    (
        st,
        std::mem::size_of::<libc::sockaddr_in6>() as libc::socklen_t,
    )
}

/// A Unix socket address; None when the path is too long. A path
/// starting with NUL is in Linux's abstract namespace (its length counts,
/// test socket_abstract_path.phpt).
pub fn unix_sockaddr(path: &[u8]) -> Option<Addr> {
    if path.len() >= SUN_PATH {
        return None;
    }
    let mut st = zeroed();
    // SAFETY: as above.
    let sun = unsafe { &mut *(&mut st as *mut libc::sockaddr_storage).cast::<libc::sockaddr_un>() };
    sun.sun_family = libc::AF_UNIX as libc::sa_family_t;
    for (i, &b) in path.iter().enumerate() {
        sun.sun_path[i] = b as libc::c_char;
    }
    let base = std::mem::size_of::<libc::sa_family_t>();
    let len = if path.first() == Some(&0) {
        base + path.len()
    } else {
        base + path.len() + 1
    };
    Some((st, len as libc::socklen_t))
}

pub fn if_index(name: &[u8]) -> u32 {
    let Ok(c) = CString::new(name) else {
        return 0;
    };
    // SAFETY: a NUL-terminated name.
    unsafe { libc::if_nametoindex(c.as_ptr()) }
}

pub fn if_name(index: u32) -> Option<Vec<u8>> {
    let mut buf = [0 as libc::c_char; libc::IF_NAMESIZE];
    // SAFETY: a buffer of IF_NAMESIZE bytes.
    let p = unsafe { libc::if_indextoname(index, buf.as_mut_ptr()) };
    if p.is_null() {
        return None;
    }
    Some(
        buf.iter()
            .take_while(|&&c| c != 0)
            .map(|&c| c as u8)
            .collect(),
    )
}

/// An AF_PACKET address for an interface name (socket_bind() of a packet
/// socket).
pub fn packet_sockaddr(name: &[u8]) -> Option<Addr> {
    let idx = if_index(name);
    if idx == 0 {
        return None;
    }
    let mut st = zeroed();
    // SAFETY: as above.
    let sll = unsafe { &mut *(&mut st as *mut libc::sockaddr_storage).cast::<libc::sockaddr_ll>() };
    sll.sll_family = libc::AF_PACKET as u16;
    sll.sll_ifindex = idx as i32;
    Some((
        st,
        std::mem::size_of::<libc::sockaddr_ll>() as libc::socklen_t,
    ))
}

/// The resolver error code PHP reports for a getaddrinfo() failure
/// (`-10000 - h_errno`: 1 unknown host, 2 try again, 3 no recovery, 4 no
/// data).
fn gai_code(e: i32) -> i32 {
    let h = match e {
        libc::EAI_NONAME => 1,
        libc::EAI_AGAIN => 2,
        libc::EAI_FAIL => 3,
        libc::EAI_NODATA => 4,
        _ => 3,
    };
    -10000 - h
}

/// Looks `host` up for `family`: the first address, or the resolver code.
fn lookup(host: &[u8], family: i32) -> Result<libc::sockaddr_storage, i32> {
    if host.is_empty() || host.contains(&0) {
        return Err(-10003);
    }
    let c = CString::new(host).map_err(|_| -10003)?;
    // SAFETY: zeroed hints.
    let mut hints: libc::addrinfo = unsafe { std::mem::zeroed() };
    hints.ai_family = family;
    let mut res: *mut libc::addrinfo = std::ptr::null_mut();
    // SAFETY: valid strings and out pointer.
    let r = unsafe { libc::getaddrinfo(c.as_ptr(), std::ptr::null(), &hints, &mut res) };
    if r != 0 || res.is_null() {
        return Err(gai_code(r));
    }
    let mut st = zeroed();
    // SAFETY: res is a valid list from getaddrinfo.
    unsafe {
        let ai = &*res;
        let n = (ai.ai_addrlen as usize).min(std::mem::size_of::<libc::sockaddr_storage>());
        std::ptr::copy_nonoverlapping(
            ai.ai_addr.cast::<u8>(),
            (&mut st as *mut libc::sockaddr_storage).cast::<u8>(),
            n,
        );
        libc::freeaddrinfo(res);
    }
    Ok(st)
}

/// An IPv4 address: dotted quad or resolved name.
pub fn resolve_v4(host: &[u8]) -> Result<Ipv4Addr, i32> {
    if let Some(ip) = std::str::from_utf8(host).ok().and_then(|s| s.parse().ok()) {
        return Ok(ip);
    }
    let st = lookup(host, libc::AF_INET)?;
    // SAFETY: an AF_INET result.
    let sin = unsafe { &*(&st as *const libc::sockaddr_storage).cast::<libc::sockaddr_in>() };
    Ok(Ipv4Addr::from(sin.sin_addr.s_addr.to_ne_bytes()))
}

/// An IPv6 address: literal or resolved name (IPv4 literals don't map).
pub fn resolve_v6(host: &[u8]) -> Result<(Ipv6Addr, u32), i32> {
    if let Some(ip) = std::str::from_utf8(host).ok().and_then(|s| s.parse().ok()) {
        return Ok((ip, 0));
    }
    let st = lookup(host, libc::AF_INET6)?;
    // SAFETY: an AF_INET6 result.
    let sin = unsafe { &*(&st as *const libc::sockaddr_storage).cast::<libc::sockaddr_in6>() };
    Ok((Ipv6Addr::from(sin.sin6_addr.s6_addr), sin.sin6_scope_id))
}

/// The address for `host`:`port` in the socket's family, warning "Host
/// lookup failed" when it doesn't resolve.
pub fn resolve_for(
    vm: &mut Vm,
    a: &Args,
    sock: Option<usize>,
    family: i32,
    host: &[u8],
    port: u16,
) -> R<Option<Addr>> {
    let r = if family == libc::AF_INET6 {
        resolve_v6(host).map(|(ip, scope)| inet6_sockaddr(ip, port, 0, scope))
    } else {
        resolve_v4(host).map(|ip| inet_sockaddr(ip, port))
    };
    match r {
        Ok(t) => Ok(Some(t)),
        Err(code) => {
            lookup_error(vm, a, sock, code)?;
            Ok(None)
        }
    }
}

/// A decoded socket address.
pub enum Decoded {
    Inet(String, u16),
    Unix(Vec<u8>),
    /// AF_PACKET: interface name and index.
    Packet(Vec<u8>, i64),
    Other,
}

pub fn decode(st: &libc::sockaddr_storage, len: libc::socklen_t) -> Decoded {
    match st.ss_family as i32 {
        libc::AF_INET => {
            // SAFETY: an AF_INET address.
            let sin =
                unsafe { &*(st as *const libc::sockaddr_storage).cast::<libc::sockaddr_in>() };
            Decoded::Inet(
                Ipv4Addr::from(sin.sin_addr.s_addr.to_ne_bytes()).to_string(),
                u16::from_be(sin.sin_port),
            )
        }
        libc::AF_INET6 => {
            // SAFETY: an AF_INET6 address.
            let sin =
                unsafe { &*(st as *const libc::sockaddr_storage).cast::<libc::sockaddr_in6>() };
            Decoded::Inet(
                Ipv6Addr::from(sin.sin6_addr.s6_addr).to_string(),
                u16::from_be(sin.sin6_port),
            )
        }
        libc::AF_UNIX => {
            // SAFETY: an AF_UNIX address.
            let sun =
                unsafe { &*(st as *const libc::sockaddr_storage).cast::<libc::sockaddr_un>() };
            let base = std::mem::size_of::<libc::sa_family_t>();
            let n = (len as usize).saturating_sub(base).min(SUN_PATH);
            let raw: Vec<u8> = sun.sun_path[..n].iter().map(|&c| c as u8).collect();
            if raw.first() == Some(&0) {
                Decoded::Unix(raw)
            } else {
                Decoded::Unix(raw.into_iter().take_while(|&c| c != 0).collect())
            }
        }
        libc::AF_PACKET => {
            // SAFETY: an AF_PACKET address.
            let sll =
                unsafe { &*(st as *const libc::sockaddr_storage).cast::<libc::sockaddr_ll>() };
            let idx = sll.sll_ifindex;
            Decoded::Packet(if_name(idx as u32).unwrap_or_default(), idx as i64)
        }
        _ => {
            if len as usize <= std::mem::size_of::<libc::sa_family_t>() {
                Decoded::Unix(Vec::new())
            } else {
                Decoded::Other
            }
        }
    }
}

// ------------------------------------------------------------ AddressInfo

/// One getaddrinfo() result.
pub struct AddrInfo {
    flags: i32,
    family: i32,
    socktype: i32,
    protocol: i32,
    canonname: Option<Vec<u8>>,
    addr: libc::sockaddr_storage,
    addrlen: libc::socklen_t,
}

impl goro_runtime::NativeData for AddrInfo {}

const HINT_KEYS: &str =
    "must only contain array keys \"ai_flags\", \"ai_socktype\", \"ai_protocol\", or \"ai_family\"";

pub fn socket_addrinfo_lookup(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let host = a.str(0);
    let service = a.opt_str(1);
    let hints_arr = a.array(2);
    // SAFETY: zeroed hints.
    let mut hints: libc::addrinfo = unsafe { std::mem::zeroed() };
    for (k, v) in hints_arr.iter() {
        let name = match k {
            KeyRef::Str(s) => s.as_bytes().to_vec(),
            KeyRef::Int(_) => Vec::new(),
        };
        let name = String::from_utf8_lossy(&name).into_owned();
        if !matches!(
            name.as_str(),
            "ai_flags" | "ai_socktype" | "ai_protocol" | "ai_family"
        ) {
            return vm.fthrow(
                a,
                "ValueError",
                &format!("Argument #3 ($hints) {HINT_KEYS}"),
            );
        }
        let v = v.deref();
        let Value::Int(n) = v else {
            return vm.fthrow(
                a,
                "TypeError",
                &format!(
                    "Argument #3 ($hints) \"{name}\" key must be of type int, {} given",
                    Vm::type_name_given(&v)
                ),
            );
        };
        if name == "ai_family" {
            if n != libc::AF_INET as i64 && n != libc::AF_INET6 as i64 {
                return vm.fthrow(
                    a,
                    "ValueError",
                    "Argument #3 ($hints) \"ai_family\" key must be AF_INET or AF_INET6",
                );
            }
            hints.ai_family = n as i32;
            continue;
        }
        if !(0..=i32::MAX as i64).contains(&n) {
            return vm.fthrow(
                a,
                "ValueError",
                &format!(
                    "Argument #3 ($hints) \"{name}\" key must be between 0 and {}",
                    i32::MAX
                ),
            );
        }
        match name.as_str() {
            "ai_flags" => hints.ai_flags = n as i32,
            "ai_socktype" => hints.ai_socktype = n as i32,
            _ => hints.ai_protocol = n as i32,
        }
    }
    let Ok(h) = CString::new(host.as_bytes()) else {
        return Ok(Value::Bool(false));
    };
    let svc = match &service {
        Some(s) => match CString::new(s.as_bytes()) {
            Ok(c) => Some(c),
            Err(_) => return Ok(Value::Bool(false)),
        },
        None => None,
    };
    let mut res: *mut libc::addrinfo = std::ptr::null_mut();
    // SAFETY: valid strings, hints and out pointer.
    let r = unsafe {
        libc::getaddrinfo(
            h.as_ptr(),
            svc.as_ref().map_or(std::ptr::null(), |c| c.as_ptr()),
            &hints,
            &mut res,
        )
    };
    if r != 0 {
        return Ok(Value::Bool(false));
    }
    let mut infos = Vec::new();
    let mut p = res;
    while !p.is_null() {
        // SAFETY: a node of the getaddrinfo list.
        let ai = unsafe { &*p };
        let mut st = zeroed();
        let n = (ai.ai_addrlen as usize).min(std::mem::size_of::<libc::sockaddr_storage>());
        // SAFETY: ai_addr holds ai_addrlen bytes.
        unsafe {
            std::ptr::copy_nonoverlapping(
                ai.ai_addr.cast::<u8>(),
                (&mut st as *mut libc::sockaddr_storage).cast::<u8>(),
                n,
            )
        };
        let canonname = if ai.ai_canonname.is_null() {
            None
        } else {
            // SAFETY: a NUL-terminated name.
            Some(
                unsafe { std::ffi::CStr::from_ptr(ai.ai_canonname) }
                    .to_bytes()
                    .to_vec(),
            )
        };
        infos.push(AddrInfo {
            flags: ai.ai_flags,
            family: ai.ai_family,
            socktype: ai.ai_socktype,
            protocol: ai.ai_protocol,
            canonname,
            addr: st,
            addrlen: n as libc::socklen_t,
        });
        p = ai.ai_next;
    }
    // SAFETY: the list getaddrinfo returned.
    unsafe { libc::freeaddrinfo(res) };
    let mut out = PhpArray::new();
    for info in infos {
        let o = new_obj(vm, "AddressInfo", Box::new(info))?;
        let _ = out.push(o);
    }
    Ok(Value::Array(out))
}

fn info_of(a: &Args) -> Option<(i32, i32, i32, libc::sockaddr_storage, libc::socklen_t)> {
    let o = a.obj(0)?;
    with_native::<AddrInfo, _>(&o, |i| {
        (i.family, i.socktype, i.protocol, i.addr, i.addrlen)
    })
}

fn addrinfo_socket(vm: &mut Vm, a: &mut Args, bind: bool) -> R<Value> {
    let Some((family, socktype, protocol, st, len)) = info_of(a) else {
        return Ok(Value::Bool(false));
    };
    // SAFETY: plain socket(2).
    let fd = unsafe { libc::socket(family, socktype, protocol) };
    if fd < 0 {
        super::global_error(vm, a, "Unable to create socket", super::errno())?;
        return Ok(Value::Bool(false));
    }
    let p = (&st as *const libc::sockaddr_storage).cast();
    // SAFETY: an address of the given length.
    let r = unsafe {
        if bind {
            libc::bind(fd, p, len)
        } else {
            libc::connect(fd, p, len)
        }
    };
    if r != 0 {
        let e = super::errno();
        // SAFETY: our descriptor.
        unsafe { libc::close(fd) };
        let what = if bind {
            "Unable to bind address"
        } else {
            "Unable to connect address"
        };
        super::global_error(vm, a, what, e)?;
        return Ok(Value::Bool(false));
    }
    new_socket(vm, fd, family, socktype)
}

pub fn socket_addrinfo_bind(vm: &mut Vm, a: &mut Args) -> R<Value> {
    addrinfo_socket(vm, a, true)
}

pub fn socket_addrinfo_connect(vm: &mut Vm, a: &mut Args) -> R<Value> {
    addrinfo_socket(vm, a, false)
}

pub fn socket_addrinfo_explain(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let Some(o) = a.obj(0) else {
        return Ok(Value::Array(PhpArray::new()));
    };
    let arr = with_native::<AddrInfo, PhpArray>(&o, |i| {
        let mut m = PhpArray::new();
        m.insert(Key::from("ai_flags"), Value::Int(i.flags as i64));
        m.insert(Key::from("ai_family"), Value::Int(i.family as i64));
        m.insert(Key::from("ai_socktype"), Value::Int(i.socktype as i64));
        m.insert(Key::from("ai_protocol"), Value::Int(i.protocol as i64));
        if let Some(c) = &i.canonname {
            m.insert(Key::from("ai_canonname"), Value::from(c.as_slice()));
        }
        let mut ad = PhpArray::new();
        match decode(&i.addr, i.addrlen) {
            Decoded::Inet(host, port) if i.family == libc::AF_INET6 => {
                ad.insert(Key::from("sin6_port"), Value::Int(port as i64));
                ad.insert(Key::from("sin6_addr"), Value::from(host.as_str()));
            }
            Decoded::Inet(host, port) => {
                ad.insert(Key::from("sin_port"), Value::Int(port as i64));
                ad.insert(Key::from("sin_addr"), Value::from(host.as_str()));
            }
            Decoded::Unix(p) => {
                ad.insert(Key::from("sun_path"), Value::from(p.as_slice()));
            }
            _ => {}
        }
        m.insert(Key::from("ai_addr"), Value::Array(ad));
        m
    })
    .unwrap_or_default();
    Ok(Value::Array(arr))
}
