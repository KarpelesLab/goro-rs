//! socket_sendmsg(), socket_recvmsg() and socket_cmsg_space() (PHP
//! manual; sendmsg(2), recvmsg(2), cmsg(3)): the message array and the
//! control messages the tests use (SCM_RIGHTS, SCM_CREDENTIALS,
//! IPV6_PKTINFO, IPV6_HOPLIMIT, IPV6_TCLASS). Conversion errors read
//! "error converting user data (path: msghdr > ...): ..." (observed on
//! PHP 8.5).

use std::os::fd::RawFd;

use goro_runtime::{Key, PhpArray, PhpStr, Value};

use super::addr;
use super::{sk_info, sock_error};
use crate::func::Args;
use crate::vm::{R, Vm};

/// The largest buffer_size recvmsg() takes.
const MAX_BUFFER: i64 = 104857600;

/// The receive buffer when the message gives no "buffer_size".
const DEFAULT_BUFFER: usize = 8192;

/// A conversion failure (already reported).
struct Failed;

type Conv<T> = Result<T, Failed>;

struct Ctx<'a, 'b> {
    a: &'a Args<'b>,
    path: Vec<String>,
}

impl Ctx<'_, '_> {
    fn fail<T>(&self, vm: &mut Vm, msg: &str) -> R<Conv<T>> {
        vm.fwarn(
            self.a,
            &format!(
                "error converting user data (path: {}): {msg}",
                self.path.join(" > ")
            ),
        )?;
        Ok(Err(Failed))
    }
}

fn data_size(level: i32, ty: i32) -> Option<usize> {
    match (level, ty) {
        (libc::SOL_SOCKET, libc::SCM_RIGHTS) => Some(0),
        (libc::SOL_SOCKET, libc::SCM_CREDENTIALS) => Some(std::mem::size_of::<libc::ucred>()),
        (libc::IPPROTO_IPV6, libc::IPV6_PKTINFO) => Some(20),
        (libc::IPPROTO_IPV6, libc::IPV6_HOPLIMIT | libc::IPV6_TCLASS) => Some(4),
        _ => None,
    }
}

pub fn socket_cmsg_space(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (level, ty, n) = (a.int(0), a.int(1), a.int_or(2, 0));
    if n < 0 {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #3 ($num) must be greater than or equal to 0",
        );
    }
    if n > i32::MAX as i64 {
        return vm.fthrow(
            a,
            "ValueError",
            &format!(
                "Argument #3 ($num) must be between {} and {}",
                i32::MIN,
                i32::MAX
            ),
        );
    }
    let Some(size) = data_size(level as i32, ty as i32) else {
        return vm.throw(
            "ValueError",
            &format!("Pair level {level} and/or type {ty} is not supported"),
        );
    };
    let total = if (level as i32, ty as i32) == (libc::SOL_SOCKET, libc::SCM_RIGHTS) {
        (n as u64) * 4
    } else {
        size as u64
    };
    if total > u32::MAX as u64 / 2 {
        return Ok(Value::Null);
    }
    // SAFETY: arithmetic macro.
    Ok(Value::Int(unsafe { libc::CMSG_SPACE(total as u32) } as i64))
}

fn get(t: &PhpArray, k: &str) -> Option<Value> {
    t.get_bytes(k.as_bytes()).map(|v| v.deref())
}

/// The destination address ("name") for the socket's family.
fn conv_name(vm: &mut Vm, c: &mut Ctx, family: i32, v: &Value) -> R<Conv<addr::Addr>> {
    let Value::Array(t) = v else {
        return c.fail(vm, "expected an array here");
    };
    match family {
        libc::AF_INET | libc::AF_INET6 => {
            let port = match get(&t, "port") {
                Some(p) => vm.to_int(&p)? as u16,
                None => 0,
            };
            let host = match get(&t, "addr") {
                Some(h) => Some(vm.to_php_str(&h)?),
                None => None,
            };
            if family == libc::AF_INET {
                let ip = match &host {
                    Some(h) => match addr::resolve_v4(h.as_bytes()) {
                        Ok(ip) => ip,
                        Err(code) => {
                            super::lookup_error(vm, c.a, Some(0), code)?;
                            c.path.push("addr".into());
                            return c.fail(
                                vm,
                                &format!(
                                    "could not resolve address '{}' to get an AF_INET address",
                                    h.to_string_lossy()
                                ),
                            );
                        }
                    },
                    None => std::net::Ipv4Addr::UNSPECIFIED,
                };
                return Ok(Ok(addr::inet_sockaddr(ip, port)));
            }
            let flow = match get(&t, "flowinfo") {
                Some(f) => vm.to_int(&f)? as u32,
                None => 0,
            };
            let mut scope = match get(&t, "scope_id") {
                Some(f) => vm.to_int(&f)? as u32,
                None => 0,
            };
            let ip = match &host {
                Some(h) => match addr::resolve_v6(h.as_bytes()) {
                    Ok((ip, sc)) => {
                        if scope == 0 {
                            scope = sc;
                        }
                        ip
                    }
                    Err(code) => {
                        super::lookup_error(vm, c.a, Some(0), code)?;
                        c.path.push("addr".into());
                        return c.fail(
                            vm,
                            &format!(
                                "could not resolve address '{}' to get an AF_INET6 address",
                                h.to_string_lossy()
                            ),
                        );
                    }
                },
                None => std::net::Ipv6Addr::UNSPECIFIED,
            };
            Ok(Ok(addr::inet6_sockaddr(ip, port, flow, scope)))
        }
        libc::AF_UNIX => {
            let Some(p) = get(&t, "path") else {
                return c.fail(vm, "The key 'path' is required");
            };
            let p = vm.to_php_str(&p)?;
            match addr::unix_sockaddr(p.as_bytes()) {
                Some(x) => Ok(Ok(x)),
                None => {
                    c.path.push("path".into());
                    c.fail(
                        vm,
                        &format!(
                            "the path is too long, the maximum permitted length is {}",
                            addr::SUN_PATH - 1
                        ),
                    )
                }
            }
        }
        _ => c.fail(vm, "the socket has an unsupported address family"),
    }
}

fn need(vm: &mut Vm, c: &Ctx, t: &PhpArray, k: &str) -> R<Conv<Value>> {
    match get(t, k) {
        Some(v) => Ok(Ok(v)),
        None => c.fail(vm, &format!("The key '{k}' is required")),
    }
}

/// One control message's payload.
fn conv_cmsg_data(vm: &mut Vm, c: &mut Ctx, level: i32, ty: i32, v: &Value) -> R<Conv<Vec<u8>>> {
    match (level, ty) {
        (libc::SOL_SOCKET, libc::SCM_RIGHTS) => {
            let Value::Array(t) = v else {
                return c.fail(vm, "expected an array here");
            };
            let mut out = Vec::new();
            for (i, (_, e)) in t.iter().enumerate() {
                let e = e.deref();
                let fd: Option<RawFd> = match &e {
                    Value::Object(_) => super::object_fd(&e),
                    Value::Resource(r) => super::stream_fd(r),
                    _ => None,
                };
                let Some(fd) = fd else {
                    c.path.push(format!("element #{}", i + 1));
                    return c.fail(vm, "expected a Socket object or a stream resource");
                };
                out.extend_from_slice(&fd.to_ne_bytes());
            }
            Ok(Ok(out))
        }
        (libc::SOL_SOCKET, libc::SCM_CREDENTIALS) => {
            let Value::Array(t) = v else {
                return c.fail(vm, "expected an array here");
            };
            let mut vals = [0i64; 3];
            for (i, k) in ["pid", "uid", "gid"].iter().enumerate() {
                let Ok(x) = need(vm, c, &t, k)? else {
                    return Ok(Err(Failed));
                };
                vals[i] = vm.to_int(&x)?;
            }
            let mut out = (vals[0] as i32).to_ne_bytes().to_vec();
            out.extend_from_slice(&(vals[1] as u32).to_ne_bytes());
            out.extend_from_slice(&(vals[2] as u32).to_ne_bytes());
            Ok(Ok(out))
        }
        (libc::IPPROTO_IPV6, libc::IPV6_PKTINFO) => {
            let Value::Array(t) = v else {
                return c.fail(vm, "expected an array here");
            };
            let Ok(addr_v) = need(vm, c, &t, "addr")? else {
                return Ok(Err(Failed));
            };
            let Ok(idx_v) = need(vm, c, &t, "ifindex")? else {
                return Ok(Err(Failed));
            };
            let h = vm.to_php_str(&addr_v)?;
            let ip = match addr::resolve_v6(h.as_bytes()) {
                Ok((ip, _)) => ip,
                Err(code) => {
                    super::lookup_error(vm, c.a, Some(0), code)?;
                    c.path.push("addr".into());
                    return c.fail(
                        vm,
                        &format!(
                            "could not resolve address '{}' to get an AF_INET6 address",
                            h.to_string_lossy()
                        ),
                    );
                }
            };
            let idx = vm.to_int(&idx_v)? as u32;
            let mut out = ip.octets().to_vec();
            out.extend_from_slice(&idx.to_ne_bytes());
            Ok(Ok(out))
        }
        (libc::IPPROTO_IPV6, libc::IPV6_HOPLIMIT | libc::IPV6_TCLASS) => {
            let n = vm.to_int(v)?;
            Ok(Ok((n as i32).to_ne_bytes().to_vec()))
        }
        _ => c.fail(
            vm,
            &format!("cmsghdr with level {level} and type {ty} not supported"),
        ),
    }
}

/// The control buffer for sendmsg().
fn conv_control(vm: &mut Vm, c: &mut Ctx, v: &Value) -> R<Conv<Vec<u8>>> {
    let Value::Array(t) = v else {
        return c.fail(vm, "expected an array here");
    };
    let mut msgs: Vec<(i32, i32, Vec<u8>)> = Vec::new();
    for (i, (_, e)) in t.iter().enumerate() {
        c.path.push(format!("element #{i}"));
        let e = e.deref();
        let Value::Array(m) = &e else {
            return c.fail(vm, "expected an array here");
        };
        let Ok(lv) = need(vm, c, m, "level")? else {
            return Ok(Err(Failed));
        };
        let level = vm.to_int(&lv)? as i32;
        let ty = match get(m, "type") {
            Some(t) => vm.to_int(&t)? as i32,
            None => 0,
        };
        if data_size(level, ty).is_none() {
            return c.fail(
                vm,
                &format!("cmsghdr with level {level} and type {ty} not supported"),
            );
        }
        let Ok(dv) = need(vm, c, m, "data")? else {
            return Ok(Err(Failed));
        };
        c.path.push("data".into());
        let Ok(data) = conv_cmsg_data(vm, c, level, ty, &dv)? else {
            return Ok(Err(Failed));
        };
        c.path.pop();
        c.path.pop();
        msgs.push((level, ty, data));
    }
    // SAFETY: CMSG_* arithmetic and writes inside the buffer they size.
    unsafe {
        let total: usize = msgs
            .iter()
            .map(|m| libc::CMSG_SPACE(m.2.len() as u32) as usize)
            .sum();
        let mut buf = vec![0u8; total];
        let mut off = 0;
        for (level, ty, data) in &msgs {
            let h = buf.as_mut_ptr().add(off).cast::<libc::cmsghdr>();
            (*h).cmsg_level = *level;
            (*h).cmsg_type = *ty;
            (*h).cmsg_len = libc::CMSG_LEN(data.len() as u32) as _;
            std::ptr::copy_nonoverlapping(data.as_ptr(), libc::CMSG_DATA(h), data.len());
            off += libc::CMSG_SPACE(data.len() as u32) as usize;
        }
        Ok(Ok(buf))
    }
}

pub fn socket_sendmsg(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let m = a.array(1);
    let flags = a.int_or(2, 0) as i32;
    let (fd, family, _) = sk_info(vm, a, 0)?;
    let mut c = Ctx {
        a,
        path: vec!["msghdr".into()],
    };
    let mut name: Option<addr::Addr> = None;
    if let Some(v) = get(&m, "name") {
        c.path.push("name".into());
        let Ok(n) = conv_name(vm, &mut c, family, &v)? else {
            return Ok(Value::Bool(false));
        };
        c.path.pop();
        name = Some(n);
    }
    let mut iov_data: Vec<Vec<u8>> = Vec::new();
    if let Some(v) = get(&m, "iov") {
        c.path.push("iov".into());
        let Value::Array(t) = &v else {
            let _ = c.fail::<()>(vm, "expected an array here")?;
            return Ok(Value::Bool(false));
        };
        for (_, e) in t.iter() {
            let s = vm.to_php_str(&e.deref())?;
            iov_data.push(s.as_bytes().to_vec());
        }
        c.path.pop();
    }
    let mut control = Vec::new();
    if let Some(v) = get(&m, "control") {
        c.path.push("control".into());
        let Ok(b) = conv_control(vm, &mut c, &v)? else {
            return Ok(Value::Bool(false));
        };
        c.path.pop();
        control = b;
    }
    let msg_flags = match get(&m, "flags") {
        Some(f) => vm.to_int(&f)? as i32,
        None => 0,
    };
    let mut iovs: Vec<libc::iovec> = iov_data
        .iter_mut()
        .map(|d| libc::iovec {
            iov_base: d.as_mut_ptr().cast(),
            iov_len: d.len(),
        })
        .collect();
    // SAFETY: zeroed msghdr filled with valid buffers.
    let mut hdr: libc::msghdr = unsafe { std::mem::zeroed() };
    if let Some(n) = &mut name {
        hdr.msg_name = (&mut n.0 as *mut libc::sockaddr_storage).cast();
        hdr.msg_namelen = n.1;
    }
    hdr.msg_iov = iovs.as_mut_ptr();
    hdr.msg_iovlen = iovs.len() as _;
    if !control.is_empty() {
        hdr.msg_control = control.as_mut_ptr().cast();
        hdr.msg_controllen = control.len() as _;
    }
    hdr.msg_flags = msg_flags;
    // SAFETY: the header points at live buffers.
    let n = unsafe { libc::sendmsg(fd, &hdr, flags) };
    if n < 0 {
        sock_error(vm, a, 0, "Error in sendmsg", super::errno())?;
        return Ok(Value::Bool(false));
    }
    Ok(Value::Int(n as i64))
}

fn name_value(st: &libc::sockaddr_storage, len: libc::socklen_t) -> Value {
    if len == 0 {
        return Value::Null;
    }
    let mut m = PhpArray::new();
    match st.ss_family as i32 {
        libc::AF_INET => {
            // SAFETY: an AF_INET address.
            let sin =
                unsafe { &*(st as *const libc::sockaddr_storage).cast::<libc::sockaddr_in>() };
            m.insert(Key::from("family"), Value::Int(libc::AF_INET as i64));
            m.insert(
                Key::from("addr"),
                Value::from(
                    std::net::Ipv4Addr::from(sin.sin_addr.s_addr.to_ne_bytes())
                        .to_string()
                        .as_str(),
                ),
            );
            m.insert(
                Key::from("port"),
                Value::Int(u16::from_be(sin.sin_port) as i64),
            );
        }
        libc::AF_INET6 => {
            // SAFETY: an AF_INET6 address.
            let sin =
                unsafe { &*(st as *const libc::sockaddr_storage).cast::<libc::sockaddr_in6>() };
            m.insert(Key::from("family"), Value::Int(libc::AF_INET6 as i64));
            m.insert(
                Key::from("addr"),
                Value::from(
                    std::net::Ipv6Addr::from(sin.sin6_addr.s6_addr)
                        .to_string()
                        .as_str(),
                ),
            );
            m.insert(
                Key::from("port"),
                Value::Int(u16::from_be(sin.sin6_port) as i64),
            );
            m.insert(
                Key::from("flowinfo"),
                Value::Int(u32::from_be(sin.sin6_flowinfo) as i64),
            );
            m.insert(Key::from("scope_id"), Value::Int(sin.sin6_scope_id as i64));
        }
        libc::AF_UNIX => {
            if len as usize <= std::mem::size_of::<libc::sa_family_t>() {
                // An unnamed peer (observed on PHP 8.5: empty).
                return Value::Null;
            }
            let path = match addr::decode(st, len) {
                addr::Decoded::Unix(p) => p,
                _ => Vec::new(),
            };
            m.insert(Key::from("family"), Value::Int(libc::AF_UNIX as i64));
            m.insert(Key::from("path"), Value::Str(PhpStr::from(path.as_slice())));
        }
        _ => return Value::Null,
    }
    Value::Array(m)
}

/// The received control messages as PHP arrays.
fn control_value(hdr: &libc::msghdr) -> PhpArray {
    let mut out = PhpArray::new();
    // SAFETY: walking the control buffer recvmsg() filled.
    unsafe {
        let mut h = libc::CMSG_FIRSTHDR(hdr);
        while !h.is_null() {
            let level = (*h).cmsg_level;
            let ty = (*h).cmsg_type;
            let base = libc::CMSG_LEN(0) as usize;
            let dlen = ((*h).cmsg_len as usize).saturating_sub(base);
            let d = std::slice::from_raw_parts(libc::CMSG_DATA(h), dlen);
            let data = match (level, ty) {
                (libc::SOL_SOCKET, libc::SCM_RIGHTS) => {
                    let mut l = PhpArray::new();
                    for ch in d.chunks_exact(4) {
                        let fd = i32::from_ne_bytes([ch[0], ch[1], ch[2], ch[3]]);
                        let _ = l.push(super::fd_stream(fd));
                    }
                    Value::Array(l)
                }
                (libc::SOL_SOCKET, libc::SCM_CREDENTIALS) if dlen >= 12 => {
                    let w = |i: usize| [d[i], d[i + 1], d[i + 2], d[i + 3]];
                    let mut l = PhpArray::new();
                    l.insert(
                        Key::from("pid"),
                        Value::Int(i32::from_ne_bytes(w(0)) as i64),
                    );
                    l.insert(
                        Key::from("uid"),
                        Value::Int(u32::from_ne_bytes(w(4)) as i64),
                    );
                    l.insert(
                        Key::from("gid"),
                        Value::Int(u32::from_ne_bytes(w(8)) as i64),
                    );
                    Value::Array(l)
                }
                (libc::IPPROTO_IPV6, libc::IPV6_PKTINFO) if dlen >= 20 => {
                    let mut o = [0u8; 16];
                    o.copy_from_slice(&d[..16]);
                    let mut l = PhpArray::new();
                    l.insert(
                        Key::from("addr"),
                        Value::from(std::net::Ipv6Addr::from(o).to_string().as_str()),
                    );
                    l.insert(
                        Key::from("ifindex"),
                        Value::Int(u32::from_ne_bytes([d[16], d[17], d[18], d[19]]) as i64),
                    );
                    Value::Array(l)
                }
                (libc::IPPROTO_IPV6, libc::IPV6_HOPLIMIT | libc::IPV6_TCLASS) if dlen >= 4 => {
                    Value::Int(i32::from_ne_bytes([d[0], d[1], d[2], d[3]]) as i64)
                }
                _ => Value::Str(PhpStr::from(d)),
            };
            let mut e = PhpArray::new();
            e.insert(Key::from("level"), Value::Int(level as i64));
            e.insert(Key::from("type"), Value::Int(ty as i64));
            e.insert(Key::from("data"), data);
            let _ = out.push(Value::Array(e));
            h = libc::CMSG_NXTHDR(hdr, h);
        }
    }
    out
}

pub fn socket_recvmsg(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let m = a.array(1);
    let flags = a.int_or(2, 0) as i32;
    let fd = sk_info(vm, a, 0)?.0;
    let mut c = Ctx {
        a,
        path: vec!["msghdr".into()],
    };
    let want_name = match get(&m, "name") {
        Some(Value::Array(_)) => true,
        Some(_) => {
            c.path.push("name".into());
            let _ = c.fail::<()>(vm, "expected an array here")?;
            return Ok(Value::Bool(false));
        }
        None => false,
    };
    let bufsize = match get(&m, "buffer_size") {
        Some(v) => {
            let n = vm.to_int(&v)?;
            if n < 0 || n > MAX_BUFFER {
                c.path.push("buffer_size".into());
                let _ = c.fail::<()>(
                    vm,
                    &format!("the buffer size must be between 1 and {MAX_BUFFER}; given {n}"),
                )?;
                return Ok(Value::Bool(false));
            }
            Some(n as usize)
        }
        None => None,
    };
    let Some(cl) = get(&m, "controllen") else {
        let _ = c.fail::<()>(vm, "The key 'controllen' is required")?;
        return Ok(Value::Bool(false));
    };
    let controllen = vm.to_int(&cl)?;
    if controllen == 0 {
        c.path.push("controllen".into());
        let _ = c.fail::<()>(vm, "controllen cannot be 0")?;
        return Ok(Value::Bool(false));
    }
    if !(0..=MAX_BUFFER).contains(&controllen) {
        c.path.push("controllen".into());
        let _ = c.fail::<()>(
            vm,
            &format!("controllen must be between 1 and {MAX_BUFFER}; given {controllen}"),
        )?;
        return Ok(Value::Bool(false));
    }
    // Without "buffer_size" a default buffer is used (observed on PHP
    // 8.5: a 3000-byte datagram arrives whole).
    let bufsize = bufsize.unwrap_or(DEFAULT_BUFFER);
    let mut buf = vec![0u8; bufsize];
    let mut control = vec![0u8; controllen as usize];
    // SAFETY: zeroed storage.
    let mut st: libc::sockaddr_storage = unsafe { std::mem::zeroed() };
    let mut iov = libc::iovec {
        iov_base: buf.as_mut_ptr().cast(),
        iov_len: buf.len(),
    };
    // SAFETY: zeroed msghdr filled with live buffers.
    let mut hdr: libc::msghdr = unsafe { std::mem::zeroed() };
    if want_name {
        hdr.msg_name = (&mut st as *mut libc::sockaddr_storage).cast();
        hdr.msg_namelen = std::mem::size_of::<libc::sockaddr_storage>() as libc::socklen_t;
    }
    if bufsize > 0 {
        hdr.msg_iov = &mut iov;
        hdr.msg_iovlen = 1;
    }
    if !control.is_empty() {
        hdr.msg_control = control.as_mut_ptr().cast();
    }
    hdr.msg_controllen = control.len() as _;
    vm.out.flush();
    // SAFETY: the header points at live buffers.
    let n = unsafe { libc::recvmsg(fd, &mut hdr, flags) };
    if n < 0 {
        sock_error(vm, a, 0, "Error in recvmsg", super::errno())?;
        return Ok(Value::Bool(false));
    }
    let mut out = PhpArray::new();
    let name = if want_name {
        name_value(&st, hdr.msg_namelen)
    } else {
        Value::Null
    };
    out.insert(Key::from("name"), name);
    out.insert(Key::from("control"), Value::Array(control_value(&hdr)));
    let mut iovs = PhpArray::new();
    if bufsize > 0 {
        let got = (n as usize).min(buf.len());
        let _ = iovs.push(Value::Str(PhpStr::from(&buf[..got])));
    }
    out.insert(Key::from("iov"), Value::Array(iovs));
    out.insert(Key::from("flags"), Value::Int(hdr.msg_flags as i64));
    a.set_ref(1, Value::Array(out));
    Ok(Value::Int(n as i64))
}
