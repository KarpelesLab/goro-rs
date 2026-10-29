//! socket_get_option() / socket_set_option() (PHP manual: socket_get_option,
//! socket_set_option; the option structures of socket(7), ip(7), ipv6(7),
//! tcp(7) and udp(7)). Argument checks and messages follow
//! ext/sockets/tests and PHP 8.5's observed output.

use std::os::fd::RawFd;

use goro_runtime::{Key, PhpArray, PhpStr, Value};

use super::addr;
use super::{sk_info, sock_error};
use crate::func::Args;
use crate::vm::{R, Vm};

const SO_DETACH_REUSEPORT_BPF: i32 = 68;
const SO_ATTACH_REUSEPORT_CBPF: i32 = 51;
const SO_MEMINFO: i32 = 55;
const UDP_SEGMENT: i32 = 103;
const SOL_UDP: i32 = 17;

fn getsockopt_raw(fd: RawFd, level: i32, opt: i32, buf: &mut [u8]) -> Result<usize, i32> {
    let mut len = buf.len() as libc::socklen_t;
    // SAFETY: a buffer of `len` bytes.
    let r = unsafe { libc::getsockopt(fd, level, opt, buf.as_mut_ptr().cast(), &mut len) };
    if r != 0 {
        return Err(super::errno());
    }
    Ok(len as usize)
}

fn setsockopt_raw(fd: RawFd, level: i32, opt: i32, data: &[u8]) -> Result<(), i32> {
    // SAFETY: `data` is valid for its length.
    let r = unsafe {
        libc::setsockopt(
            fd,
            level,
            opt,
            data.as_ptr().cast(),
            data.len() as libc::socklen_t,
        )
    };
    if r != 0 {
        return Err(super::errno());
    }
    Ok(())
}

fn as_bytes<T>(v: &T) -> &[u8] {
    // SAFETY: viewing a plain C structure as bytes.
    unsafe { std::slice::from_raw_parts((v as *const T).cast::<u8>(), std::mem::size_of::<T>()) }
}

fn i32_at(b: &[u8], i: usize) -> i64 {
    let mut w = [0u8; 4];
    w.copy_from_slice(&b[i * 4..i * 4 + 4]);
    i32::from_ne_bytes(w) as i64
}

fn get_fail(vm: &mut Vm, a: &Args, e: i32) -> R<Value> {
    sock_error(vm, a, 0, "Unable to retrieve socket option", e)?;
    Ok(Value::Bool(false))
}

pub fn socket_get_option(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let level = a.int(1) as i32;
    let opt = a.int(2) as i32;
    let fd = sk_info(vm, a, 0)?.0;
    let mut buf = [0u8; 256];
    match (level, opt) {
        (libc::SOL_SOCKET, libc::SO_LINGER) => {
            match getsockopt_raw(fd, level, opt, &mut buf[..8]) {
                Ok(_) => {
                    let mut m = PhpArray::new();
                    m.insert(Key::from("l_onoff"), Value::Int(i32_at(&buf, 0)));
                    m.insert(Key::from("l_linger"), Value::Int(i32_at(&buf, 1)));
                    Ok(Value::Array(m))
                }
                Err(e) => get_fail(vm, a, e),
            }
        }
        (libc::SOL_SOCKET, libc::SO_RCVTIMEO | libc::SO_SNDTIMEO) => {
            let mut tv = libc::timeval {
                tv_sec: 0,
                tv_usec: 0,
            };
            let mut len = std::mem::size_of::<libc::timeval>() as libc::socklen_t;
            // SAFETY: a timeval.
            let r = unsafe {
                libc::getsockopt(
                    fd,
                    level,
                    opt,
                    (&mut tv as *mut libc::timeval).cast(),
                    &mut len,
                )
            };
            if r != 0 {
                return get_fail(vm, a, super::errno());
            }
            let mut m = PhpArray::new();
            m.insert(Key::from("sec"), Value::Int(tv.tv_sec));
            m.insert(Key::from("usec"), Value::Int(tv.tv_usec));
            Ok(Value::Array(m))
        }
        (libc::SOL_SOCKET, SO_MEMINFO) => match getsockopt_raw(fd, level, opt, &mut buf[..36]) {
            Ok(_) => {
                let names = [
                    "rmem_alloc",
                    "rcvbuf",
                    "wmem_alloc",
                    "sndbuf",
                    "fwd_alloc",
                    "wmem_queued",
                    "optmem",
                    "backlog",
                    "drops",
                ];
                let mut m = PhpArray::new();
                for (i, n) in names.iter().enumerate() {
                    m.insert(Key::from(*n), Value::Int(i32_at(&buf, i) as u32 as i64));
                }
                Ok(Value::Array(m))
            }
            Err(e) => get_fail(vm, a, e),
        },
        (libc::IPPROTO_TCP, libc::TCP_CONGESTION) => {
            match getsockopt_raw(fd, level, opt, &mut buf[..16]) {
                Ok(n) => {
                    let name: Vec<u8> = buf[..n].iter().copied().take_while(|&c| c != 0).collect();
                    let mut m = PhpArray::new();
                    m.insert(Key::from("name"), Value::Str(PhpStr::from(name.as_slice())));
                    Ok(Value::Array(m))
                }
                Err(e) => get_fail(vm, a, e),
            }
        }
        (libc::IPPROTO_IP, libc::IP_MULTICAST_IF) => {
            match getsockopt_raw(fd, level, opt, &mut buf[..4]) {
                Ok(_) => {
                    let ip = std::net::Ipv4Addr::new(buf[0], buf[1], buf[2], buf[3]);
                    Ok(Value::Int(index_of_ipv4(ip) as i64))
                }
                Err(e) => get_fail(vm, a, e),
            }
        }
        (libc::IPPROTO_IP, libc::IP_MULTICAST_LOOP | libc::IP_MULTICAST_TTL) => {
            match getsockopt_raw(fd, level, opt, &mut buf[..4]) {
                Ok(1) => Ok(Value::Int(buf[0] as i64)),
                Ok(_) => Ok(Value::Int(i32_at(&buf, 0))),
                Err(e) => get_fail(vm, a, e),
            }
        }
        (libc::IPPROTO_IPV6, libc::IPV6_PKTINFO) => {
            match getsockopt_raw(fd, level, opt, &mut buf[..20]) {
                Ok(_) => Ok(Value::Bool(false)),
                Err(e) => {
                    sock_error(vm, a, 0, "unable to get socket option", e)?;
                    Ok(Value::Bool(false))
                }
            }
        }
        _ => match getsockopt_raw(fd, level, opt, &mut buf[..4]) {
            Ok(_) => Ok(Value::Int(i32_at(&buf, 0))),
            Err(e) => get_fail(vm, a, e),
        },
    }
}

/// The interface index whose IPv4 address is `ip` (0 for INADDR_ANY).
fn index_of_ipv4(ip: std::net::Ipv4Addr) -> u32 {
    if ip.is_unspecified() {
        return 0;
    }
    let mut list: *mut libc::ifaddrs = std::ptr::null_mut();
    // SAFETY: getifaddrs(3) list walk.
    unsafe {
        if libc::getifaddrs(&mut list) != 0 {
            return 0;
        }
        let mut p = list;
        let mut found = 0;
        while !p.is_null() {
            let ifa = &*p;
            if !ifa.ifa_addr.is_null() && (*ifa.ifa_addr).sa_family as i32 == libc::AF_INET {
                let sin = &*ifa.ifa_addr.cast::<libc::sockaddr_in>();
                if sin.sin_addr.s_addr == u32::from_ne_bytes(ip.octets()) {
                    found = libc::if_nametoindex(ifa.ifa_name);
                    break;
                }
            }
            p = ifa.ifa_next;
        }
        libc::freeifaddrs(list);
        found
    }
}

/// The IPv4 address of interface `index` (SIOCGIFADDR), or the errno.
fn ipv4_of_index(index: u32) -> Result<[u8; 4], i32> {
    let name = addr::if_name(index).ok_or(libc::ENODEV)?;
    // SAFETY: an ifreq for SIOCGIFADDR on a throwaway socket.
    unsafe {
        let s = libc::socket(libc::AF_INET, libc::SOCK_DGRAM, 0);
        if s < 0 {
            return Err(super::errno());
        }
        let mut req: libc::ifreq = std::mem::zeroed();
        for (i, &b) in name.iter().take(libc::IFNAMSIZ - 1).enumerate() {
            req.ifr_name[i] = b as libc::c_char;
        }
        let r = libc::ioctl(s, libc::SIOCGIFADDR, &mut req);
        let e = super::errno();
        libc::close(s);
        if r < 0 {
            return Err(e);
        }
        let sa = &*(&req.ifr_ifru as *const _ as *const libc::sockaddr_in);
        Ok(sa.sin_addr.s_addr.to_ne_bytes())
    }
}

fn opt_name(level: i32, opt: i32) -> &'static str {
    match (level, opt) {
        (_, libc::MCAST_JOIN_GROUP) if level != libc::SOL_SOCKET => "MCAST_JOIN_GROUP",
        (_, libc::MCAST_LEAVE_GROUP) if level != libc::SOL_SOCKET => "MCAST_LEAVE_GROUP",
        (_, libc::MCAST_BLOCK_SOURCE) if level != libc::SOL_SOCKET => "MCAST_BLOCK_SOURCE",
        (_, libc::MCAST_UNBLOCK_SOURCE) if level != libc::SOL_SOCKET => "MCAST_UNBLOCK_SOURCE",
        (_, libc::MCAST_JOIN_SOURCE_GROUP) if level != libc::SOL_SOCKET => {
            "MCAST_JOIN_SOURCE_GROUP"
        }
        (_, libc::MCAST_LEAVE_SOURCE_GROUP) if level != libc::SOL_SOCKET => {
            "MCAST_LEAVE_SOURCE_GROUP"
        }
        (libc::SOL_SOCKET, libc::SO_LINGER) => "SO_LINGER",
        (libc::SOL_SOCKET, libc::SO_RCVTIMEO) => "SO_RCVTIMEO",
        (libc::SOL_SOCKET, libc::SO_SNDTIMEO) => "SO_SNDTIMEO",
        (libc::SOL_SOCKET, libc::SO_BINDTODEVICE) => "SO_BINDTODEVICE",
        (libc::IPPROTO_TCP, libc::TCP_CONGESTION) => "TCP_CONGESTION",
        _ => "",
    }
}

fn type_err<T>(vm: &mut Vm, a: &Args, want: &str, level: i32, opt: i32, v: &Value) -> R<T> {
    vm.fthrow(
        a,
        "TypeError",
        &format!(
            "Argument #4 ($value) must be of type {want} when argument #3 ($option) is {}, {} given",
            opt_name(level, opt),
            Vm::type_name_given(v)
        ),
    )
}

fn set_fail(vm: &mut Vm, a: &Args, e: i32) -> R<Value> {
    sock_error(vm, a, 0, "Unable to set socket option", e)?;
    Ok(Value::Bool(false))
}

fn apply(vm: &mut Vm, a: &Args, fd: RawFd, level: i32, opt: i32, data: &[u8]) -> R<Value> {
    match setsockopt_raw(fd, level, opt, data) {
        Ok(()) => Ok(Value::Bool(true)),
        Err(e) => set_fail(vm, a, e),
    }
}

/// The hash table of an array or object value (objects: none of their
/// keys count, as PHP 8.5 reports `must have key "sec"` for stdClass).
fn table_of(v: &Value) -> Option<PhpArray> {
    match v {
        Value::Array(a) => Some(a.clone()),
        Value::Object(_) => Some(PhpArray::new()),
        _ => None,
    }
}

fn need_key(vm: &mut Vm, a: &Args, t: &PhpArray, k: &str) -> R<Value> {
    match t.get_bytes(k.as_bytes()) {
        Some(v) => Ok(v.deref()),
        None => vm.fthrow(
            a,
            "ValueError",
            &format!("Argument #4 ($value) must have key \"{k}\""),
        ),
    }
}

/// An interface given as an index or a name; None (after a warning) when
/// no interface has that name.
fn interface(vm: &mut Vm, a: &Args, v: &Value) -> R<Option<u32>> {
    match v {
        Value::Int(n) => Ok(Some(*n as u32)),
        Value::Str(s) => {
            if let Some(n) = super::numeric_int(s.as_bytes()) {
                return Ok(Some(n as u32));
            }
            let idx = addr::if_index(s.as_bytes());
            if idx == 0 {
                vm.fwarn(
                    a,
                    &format!(
                        "No interface with name \"{}\" could be found",
                        s.to_string_lossy()
                    ),
                )?;
                return Ok(None);
            }
            Ok(Some(idx))
        }
        other => {
            let n = vm.to_int(other)?;
            Ok(Some(n as u32))
        }
    }
}

/// A multicast group (or source) address for the socket's family.
fn mcast_addr(vm: &mut Vm, a: &Args, family: i32, v: &Value) -> R<Option<addr::Addr>> {
    let s = vm.to_php_str(v)?;
    match family {
        libc::AF_INET | libc::AF_INET6 => {
            addr::resolve_for(vm, a, Some(0), family, s.as_bytes(), 0)
        }
        _ => vm.throw(
            "ValueError",
            "IP address used in the context of an unexpected type of socket",
        ),
    }
}

fn mcast(
    vm: &mut Vm,
    a: &Args,
    fd: RawFd,
    family: i32,
    level: i32,
    opt: i32,
    v: &Value,
) -> R<Value> {
    let Value::Array(t) = v else {
        return type_err(vm, a, "array", level, opt, v);
    };
    let Some(g) = t.get_bytes(b"group").map(|x| x.deref()) else {
        return vm.throw("ValueError", "No key \"group\" passed in optval");
    };
    let Some(group) = mcast_addr(vm, a, family, &g)? else {
        return Ok(Value::Bool(false));
    };
    let source = matches!(
        opt,
        libc::MCAST_BLOCK_SOURCE
            | libc::MCAST_UNBLOCK_SOURCE
            | libc::MCAST_JOIN_SOURCE_GROUP
            | libc::MCAST_LEAVE_SOURCE_GROUP
    );
    let src = if source {
        let Some(s) = t.get_bytes(b"source").map(|x| x.deref()) else {
            return vm.throw("ValueError", "No key \"source\" passed in optval");
        };
        match mcast_addr(vm, a, family, &s)? {
            Some(x) => Some(x),
            None => return Ok(Value::Bool(false)),
        }
    } else {
        None
    };
    let ifindex = match t.get_bytes(b"interface").map(|x| x.deref()) {
        Some(i) => match interface(vm, a, &i)? {
            Some(n) => n,
            None => return Ok(Value::Bool(false)),
        },
        None => 0,
    };
    match src {
        None => {
            let req = libc::group_req {
                gr_interface: ifindex,
                gr_group: group.0,
            };
            apply(vm, a, fd, level, opt, as_bytes(&req))
        }
        Some(s) => {
            let req = libc::group_source_req {
                gsr_interface: ifindex,
                gsr_group: group.0,
                gsr_source: s.0,
            };
            apply(vm, a, fd, level, opt, as_bytes(&req))
        }
    }
}

/// The classic BPF program SO_ATTACH_REUSEPORT_CBPF gets for an SKF_AD_*
/// constant: load that ancillary field and return it (the manual's
/// "SKF_AD_CPU" example routes by CPU).
fn cbpf(vm: &mut Vm, a: &Args, fd: RawFd, k: i64) -> R<Value> {
    if !(0..=60).contains(&k) || k % 4 != 0 {
        vm.fwarn(a, "Unsupported CBPF filter")?;
        return Ok(Value::Bool(false));
    }
    // struct sock_filter { u16 code; u8 jt; u8 jf; u32 k; }
    #[repr(C)]
    struct Ins(u16, u8, u8, u32);
    #[repr(C)]
    struct Prog(u16, *const Ins);
    const BPF_LD_W_ABS: u16 = 0x00 | 0x00 | 0x20;
    const BPF_RET_A: u16 = 0x06 | 0x10;
    let ins = [
        Ins(BPF_LD_W_ABS, 0, 0, (-4096i64 + k) as u32),
        Ins(BPF_RET_A, 0, 0, 0),
    ];
    let prog = Prog(2, ins.as_ptr());
    apply(
        vm,
        a,
        fd,
        libc::SOL_SOCKET,
        SO_ATTACH_REUSEPORT_CBPF,
        as_bytes(&prog),
    )
}

fn string_opt(vm: &mut Vm, a: &Args, fd: RawFd, level: i32, opt: i32, v: &Value) -> R<Value> {
    let Value::Str(s) = v else {
        return type_err(vm, a, "string", level, opt, v);
    };
    apply(vm, a, fd, level, opt, s.as_bytes())
}

/// error converting user data (path: in6_pktinfo...): IPV6_PKTINFO's
/// array of "addr" and "ifindex".
fn pktinfo(vm: &mut Vm, a: &Args, v: &Value) -> R<Result<Vec<u8>, ()>> {
    let warn = |vm: &mut Vm, path: &str, msg: &str| {
        vm.fwarn(
            a,
            &format!("error converting user data (path: {path}): {msg}"),
        )
    };
    let Value::Array(t) = v else {
        warn(vm, "in6_pktinfo", "expected an array here")?;
        return Ok(Err(()));
    };
    let Some(addr_v) = t.get_bytes(b"addr").map(|x| x.deref()) else {
        warn(vm, "in6_pktinfo", "The key 'addr' is required")?;
        return Ok(Err(()));
    };
    let Some(if_v) = t.get_bytes(b"ifindex").map(|x| x.deref()) else {
        warn(vm, "in6_pktinfo", "The key 'ifindex' is required")?;
        return Ok(Err(()));
    };
    let host = vm.to_php_str(&addr_v)?;
    let ip = match addr::resolve_v6(host.as_bytes()) {
        Ok((ip, _)) => ip,
        Err(code) => {
            super::lookup_error(vm, a, Some(0), code)?;
            warn(
                vm,
                "in6_pktinfo > addr",
                &format!(
                    "could not resolve address '{}' to get an AF_INET6 address",
                    host.to_string_lossy()
                ),
            )?;
            return Ok(Err(()));
        }
    };
    let idx = match &if_v {
        Value::Str(s) if super::numeric_int(s.as_bytes()).is_none() => {
            let i = addr::if_index(s.as_bytes());
            if i == 0 {
                warn(
                    vm,
                    "in6_pktinfo > ifindex",
                    &format!(
                        "no interface with name \"{}\" could be found",
                        s.to_string_lossy()
                    ),
                )?;
                return Ok(Err(()));
            }
            i
        }
        other => vm.to_int(other)? as u32,
    };
    let mut out = ip.octets().to_vec();
    out.extend_from_slice(&idx.to_ne_bytes());
    Ok(Ok(out))
}

pub fn socket_set_option(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let level = a.int(1) as i32;
    let opt = a.int(2) as i32;
    let v = a.val(3);
    let (fd, family, _) = sk_info(vm, a, 0)?;
    let is_mcast = matches!(
        opt,
        libc::MCAST_JOIN_GROUP
            | libc::MCAST_LEAVE_GROUP
            | libc::MCAST_BLOCK_SOURCE
            | libc::MCAST_UNBLOCK_SOURCE
            | libc::MCAST_JOIN_SOURCE_GROUP
            | libc::MCAST_LEAVE_SOURCE_GROUP
    );
    if is_mcast && (level == libc::IPPROTO_IP || level == libc::IPPROTO_IPV6) {
        return mcast(vm, a, fd, family, level, opt, &v);
    }
    match (level, opt) {
        (libc::IPPROTO_IP, libc::IP_MULTICAST_IF) => {
            let Some(idx) = interface(vm, a, &v)? else {
                return Ok(Value::Bool(false));
            };
            let ip = if idx == 0 {
                [0u8; 4]
            } else {
                match ipv4_of_index(idx) {
                    Ok(ip) => ip,
                    Err(e) => {
                        vm.fwarn(
                            a,
                            &format!("Failed obtaining address for interface {idx}: error {e}"),
                        )?;
                        return Ok(Value::Bool(false));
                    }
                }
            };
            apply(vm, a, fd, level, opt, &ip)
        }
        (libc::IPPROTO_IPV6, libc::IPV6_MULTICAST_IF) => {
            let Some(idx) = interface(vm, a, &v)? else {
                return Ok(Value::Bool(false));
            };
            apply(vm, a, fd, level, opt, &(idx as i32).to_ne_bytes())
        }
        (libc::IPPROTO_IP, libc::IP_MULTICAST_LOOP) => {
            let b = goro_runtime::convert::to_bool_quiet(&v) as u8;
            apply(vm, a, fd, level, opt, &[b])
        }
        (libc::IPPROTO_IPV6, libc::IPV6_MULTICAST_LOOP) => {
            let b = goro_runtime::convert::to_bool_quiet(&v) as i32;
            apply(vm, a, fd, level, opt, &b.to_ne_bytes())
        }
        (libc::IPPROTO_IP, libc::IP_MULTICAST_TTL) => {
            let n = vm.to_int(&v)?;
            if !(0..=255).contains(&n) {
                return vm.fthrow(
                    a,
                    "ValueError",
                    "Argument #4 ($value) must be between 0 and 255",
                );
            }
            apply(vm, a, fd, level, opt, &[n as u8])
        }
        (libc::IPPROTO_IPV6, libc::IPV6_MULTICAST_HOPS) => {
            let n = vm.to_int(&v)?;
            if !(-1..=255).contains(&n) {
                return vm.fthrow(
                    a,
                    "ValueError",
                    "Argument #4 ($value) must be between -1 and 255",
                );
            }
            apply(vm, a, fd, level, opt, &(n as i32).to_ne_bytes())
        }
        (libc::IPPROTO_IPV6, libc::IPV6_PKTINFO) => match pktinfo(vm, a, &v)? {
            Ok(data) => apply(vm, a, fd, level, opt, &data),
            Err(()) => Ok(Value::Bool(false)),
        },
        (libc::SOL_SOCKET, libc::SO_LINGER) => {
            let Some(t) = table_of(&v) else {
                return type_err(vm, a, "array", level, opt, &v);
            };
            let on = need_key(vm, a, &t, "l_onoff")?;
            let lin = need_key(vm, a, &t, "l_linger")?;
            let on = vm.to_int(&on)?;
            let lin = vm.to_int(&lin)?;
            if !(0..=i32::MAX as i64).contains(&on) {
                return vm.fthrow(
                    a,
                    "ValueError",
                    &format!(
                        "Argument #4 ($value) \"l_onoff\" must be between 0 and {}",
                        i32::MAX
                    ),
                );
            }
            if !(0..=65535).contains(&lin) {
                return vm.fthrow(
                    a,
                    "ValueError",
                    "Argument #4 ($value) \"l_linger\" must be between 0 and 65535",
                );
            }
            let l = libc::linger {
                l_onoff: on as i32,
                l_linger: lin as i32,
            };
            apply(vm, a, fd, level, opt, as_bytes(&l))
        }
        (libc::SOL_SOCKET, libc::SO_RCVTIMEO | libc::SO_SNDTIMEO) => {
            let Some(t) = table_of(&v) else {
                return type_err(vm, a, "array", level, opt, &v);
            };
            let sec = need_key(vm, a, &t, "sec")?;
            let usec = need_key(vm, a, &t, "usec")?;
            let sec = vm.to_int(&sec)?;
            let usec = vm.to_int(&usec)?;
            let tv = libc::timeval {
                tv_sec: sec,
                tv_usec: usec,
            };
            apply(vm, a, fd, level, opt, as_bytes(&tv))
        }
        (libc::SOL_SOCKET, libc::SO_BINDTODEVICE) | (libc::IPPROTO_TCP, libc::TCP_CONGESTION) => {
            string_opt(vm, a, fd, level, opt, &v)
        }
        (libc::SOL_SOCKET, SO_ATTACH_REUSEPORT_CBPF) => {
            if matches!(v, Value::Array(_) | Value::Null) {
                // Detaches the program (ENOENT when none is attached;
                // test socket_reuseport_cbpf.phpt).
                return apply(
                    vm,
                    a,
                    fd,
                    level,
                    SO_DETACH_REUSEPORT_BPF,
                    &0i32.to_ne_bytes(),
                );
            }
            let k = vm.to_int(&v)?;
            cbpf(vm, a, fd, k)
        }
        (SOL_UDP, UDP_SEGMENT) => {
            let n = vm.to_int(&v)?;
            if !(0..=65535).contains(&n) {
                return vm.fthrow(
                    a,
                    "ValueError",
                    "Argument #4 ($value) must be of between 0 and 65535",
                );
            }
            apply(vm, a, fd, level, opt, &(n as i32).to_ne_bytes())
        }
        _ => {
            let n = vm.to_int(&v)?;
            apply(vm, a, fd, level, opt, &(n as i32).to_ne_bytes())
        }
    }
}
