//! The functions of sockets.c

use std::cell::RefCell;
use std::rc::Rc;

use goro_core::array::{ArrayKey, PhpArray};
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::Vm;

use crate::zpp::{self, Arg};
use crate::*;

const USHRT_MAX: i64 = 65535;
const PHP_NORMAL_READ: i64 = 1;

pub fn vstr(s: impl AsRef<[u8]>) -> Value {
    Value::String(PhpString::from_bytes(s.as_ref()))
}

pub fn vbool(b: bool) -> Value {
    if b { Value::True } else { Value::False }
}

/// ZEND_TRY_ASSIGN_REF_*()
pub fn assign_ref(target: &Value, v: Value) {
    if let Value::Reference(r) = target {
        *r.borrow_mut() = v;
    }
}

fn sa_len<T>() -> libc::socklen_t {
    std::mem::size_of::<T>() as libc::socklen_t
}

/// Parse the leading Socket argument and check it is open.
fn sock_arg(vm: &mut Vm, f: &str, a: &Arg) -> Option<Rc<Sock>> {
    let s = sock_of(&a.obj()?);
    if !ensure_valid(vm, f, &s) {
        return None;
    }
    Some(s)
}

/// php_sock_array_to_fd_set()
fn array_to_fd_set(
    vm: &mut Vm,
    f: &str,
    argn: usize,
    arr: &PhpArray,
    set: &mut libc::fd_set,
    max_fd: &mut i32,
) -> Result<bool, ()> {
    let mut num = 0;
    for (_, v) in arr.iter() {
        let el = v.deref();
        let s = match &el {
            Value::Object(o) if o.borrow().class_name.eq_ignore_ascii_case(b"Socket") => sock_of(o),
            other => {
                zpp::arg_type_error(
                    vm,
                    f,
                    argn,
                    &format!(
                        "must only have elements of type Socket, {} given",
                        Vm::value_type_name(other)
                    ),
                );
                return Err(());
            }
        };
        if s.is_invalid() {
            zpp::arg_type_error(vm, f, argn, "contains a closed socket");
            return Err(());
        }
        let fd = s.fd.get();
        if (fd as usize) < libc::FD_SETSIZE {
            unsafe { libc::FD_SET(fd, set) };
        }
        if fd > *max_fd {
            *max_fd = fd;
        }
        num += 1;
    }
    Ok(num > 0)
}

/// php_sock_array_from_fd_set()
fn array_from_fd_set(target: &Value, arr: &PhpArray, set: &libc::fd_set) {
    let mut out = PhpArray::new();
    for (k, v) in arr.iter() {
        let el = v.deref();
        let Some(s) = value_socket(&el) else { continue };
        let fd = s.fd.get();
        if fd >= 0 && (fd as usize) < libc::FD_SETSIZE && unsafe { libc::FD_ISSET(fd, set) } {
            out.set(k.clone(), el);
        }
    }
    assign_ref(target, Value::Array(Rc::new(RefCell::new(out))));
}

/// The array behind a `?array &$x` parameter (Z_PARAM_ARRAY_EX2(x, 1, 1, 0)).
fn ref_array(
    vm: &mut Vm,
    f: &str,
    argn: usize,
    v: Option<&Value>,
) -> Result<Option<Rc<RefCell<PhpArray>>>, ()> {
    match v.map(|v| v.deref()) {
        None | Some(Value::Null) | Some(Value::Undef) => Ok(None),
        Some(Value::Array(a)) => Ok(Some(a)),
        Some(other) => {
            let m = format!(
                "{} must be of type ?array, {} given",
                zpp::arg_prefix(f, argn),
                Vm::value_type_name(&other)
            );
            zpp::throw(vm, b"TypeError", &m);
            Err(())
        }
    }
}

pub fn socket_select(vm: &mut Vm, f: &str, args: &[Value]) -> Value {
    let given = args.len();
    if !(4..=5).contains(&given) {
        zpp::count_error(vm, f, 4, 5, given);
        return Value::Null;
    }
    let mut arrays = Vec::new();
    for i in 0..3 {
        match ref_array(vm, f, i + 1, args.get(i)) {
            Ok(a) => arrays.push(a),
            Err(()) => return Value::Null,
        }
    }
    let rest = match zpp::parse(vm, f, "zzzl!|l", args) {
        Ok(a) => a,
        Err(()) => return Value::Null,
    };
    let sec = match &rest[3] {
        Arg::Long(n) => Some(*n),
        _ => None,
    };
    let usec = rest[4].long_or(0);

    let mut sets: [libc::fd_set; 3] = unsafe { std::mem::zeroed() };
    for s in sets.iter_mut() {
        unsafe { libc::FD_ZERO(s) };
    }
    let mut max_fd = 0;
    let mut nsets = 0;
    for i in 0..3 {
        if let Some(a) = &arrays[i] {
            let arr = a.borrow().clone();
            match array_to_fd_set(vm, f, i + 1, &arr, &mut sets[i], &mut max_fd) {
                Ok(true) => nsets += 1,
                Ok(false) => {}
                Err(()) => return Value::Null,
            }
        }
    }
    if nsets == 0 {
        zpp::plain_value_error(
            vm,
            "socket_select(): At least one array argument must be passed",
        );
        return Value::Null;
    }
    if max_fd as usize >= libc::FD_SETSIZE {
        vm.emit_warning(&format!(
            "{}(): You MUST recompile PHP with a larger value of FD_SETSIZE.\nIt is set to {}, but you have descriptors numbered at least as high as {}.\n --enable-fd-setsize={} is recommended, but you may want to set it\nto equal the maximum number of open files supported by your system,\nin order to avoid seeing this error again at a later date.",
            f,
            libc::FD_SETSIZE,
            max_fd,
            (max_fd as usize + 1024) & !1023
        ));
        return Value::False;
    }
    let mut tv = libc::timeval {
        tv_sec: 0,
        tv_usec: 0,
    };
    let tvp = match sec {
        Some(sec) => {
            if usec > 999_999 {
                tv.tv_sec = (sec + usec / 1_000_000) as libc::time_t;
                tv.tv_usec = (usec % 1_000_000) as libc::suseconds_t;
            } else {
                tv.tv_sec = sec as libc::time_t;
                tv.tv_usec = usec as libc::suseconds_t;
            }
            &mut tv as *mut libc::timeval
        }
        None => std::ptr::null_mut(),
    };
    vm.flush_cli_output();
    let [r, w, e] = &mut sets;
    let retval = unsafe { libc::select(max_fd + 1, r, w, e, tvp) };
    if retval == -1 {
        let err = errno();
        set_last_error(err);
        vm.emit_warning(&format!(
            "{}(): Unable to select [{}]: {}",
            f,
            err,
            sockets_strerror(err)
        ));
        return Value::False;
    }
    for i in 0..3 {
        if let Some(a) = &arrays[i] {
            let arr = a.borrow().clone();
            array_from_fd_set(&args[i], &arr, &sets[i]);
        }
    }
    Value::Long(retval as i64)
}

/// php_open_listen_sock()
fn open_listen_sock(vm: &mut Vm, f: &str, sock: &Sock, port: u16, backlog: i32) -> bool {
    let Ok(Some(addr)) = gethostbyname4(b"0.0.0.0") else {
        return false;
    };
    let la = sockaddr_in(addr, port);
    let fd = unsafe { libc::socket(libc::PF_INET, libc::SOCK_STREAM, 0) };
    sock.fd.set(fd);
    sock.blocking.set(true);
    if fd < 0 {
        sock_error(vm, f, sock, "unable to create listening socket", errno());
        return false;
    }
    sock.family.set(libc::PF_INET);
    if unsafe {
        libc::bind(
            fd,
            &la as *const _ as *const libc::sockaddr,
            sa_len::<libc::sockaddr_in>(),
        )
    } != 0
    {
        sock_error(vm, f, sock, "unable to bind to given address", errno());
        unsafe { libc::close(fd) };
        sock.fd.set(-1);
        return false;
    }
    if unsafe { libc::listen(fd, backlog) } != 0 {
        sock_error(vm, f, sock, "unable to listen on socket", errno());
        unsafe { libc::close(fd) };
        sock.fd.set(-1);
        return false;
    }
    true
}

pub fn socket_create_listen(vm: &mut Vm, f: &str, args: &[Value]) -> Value {
    let a = match zpp::parse(vm, f, "l|l", args) {
        Ok(a) => a,
        Err(()) => return Value::Null,
    };
    let port = a[0].long_or(0);
    let backlog = a[1].long_or(libc::SOMAXCONN as i64);
    if !(0..=USHRT_MAX).contains(&port) {
        zpp::value_error(vm, f, 1, "must be between 0 and 65535");
        return Value::Null;
    }
    let (obj, sock) = new_socket(vm);
    if !open_listen_sock(vm, f, &sock, port as u16, backlog as i32) {
        return Value::False;
    }
    sock.error.set(0);
    sock.blocking.set(true);
    obj
}

pub fn socket_accept(vm: &mut Vm, f: &str, args: &[Value]) -> Value {
    let a = match zpp::parse(vm, f, "O", args) {
        Ok(a) => a,
        Err(()) => return Value::Null,
    };
    let Some(sock) = sock_arg(vm, f, &a[0]) else {
        return Value::Null;
    };
    let (obj, new_sock) = new_socket(vm);
    let mut sa: libc::sockaddr_storage = unsafe { std::mem::zeroed() };
    let mut len = sa_len::<libc::sockaddr_storage>();
    let mut flags = libc::SOCK_CLOEXEC;
    if !sock.blocking.get() {
        flags |= libc::SOCK_NONBLOCK;
    }
    vm.flush_cli_output();
    let fd = unsafe {
        libc::accept4(
            sock.fd.get(),
            &mut sa as *mut _ as *mut libc::sockaddr,
            &mut len,
            flags,
        )
    };
    if fd < 0 {
        sock_error(
            vm,
            f,
            &new_sock,
            "unable to accept incoming connection",
            errno(),
        );
        return Value::False;
    }
    new_sock.fd.set(fd);
    new_sock.error.set(0);
    new_sock.blocking.set(true);
    new_sock.family.set(sa.ss_family as i32);
    obj
}

/// php_set_sock_blocking()
fn set_sock_blocking(fd: i32, block: bool) -> bool {
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 {
        return false;
    }
    let nf = if block {
        flags & !libc::O_NONBLOCK
    } else {
        flags | libc::O_NONBLOCK
    };
    unsafe { libc::fcntl(fd, libc::F_SETFL, nf) != -1 }
}

fn set_blocking_common(vm: &mut Vm, f: &str, args: &[Value], block: bool) -> Value {
    let a = match zpp::parse(vm, f, "O", args) {
        Ok(a) => a,
        Err(()) => return Value::Null,
    };
    let Some(sock) = sock_arg(vm, f, &a[0]) else {
        return Value::Null;
    };
    if let Some(sid) = sock.zstream.get() {
        if goro_ext_standard::sockets_streams::ext_set_blocking(sid, block) == Some(true) {
            sock.blocking.set(block);
            return Value::True;
        }
    }
    if set_sock_blocking(sock.fd.get(), block) {
        sock.blocking.set(block);
        Value::True
    } else {
        let msg = if block {
            "unable to set blocking mode"
        } else {
            "unable to set nonblocking mode"
        };
        sock_error(vm, f, &sock, msg, errno());
        Value::False
    }
}

pub fn socket_set_nonblock(vm: &mut Vm, f: &str, args: &[Value]) -> Value {
    set_blocking_common(vm, f, args, false)
}

pub fn socket_set_block(vm: &mut Vm, f: &str, args: &[Value]) -> Value {
    set_blocking_common(vm, f, args, true)
}

pub fn socket_listen(vm: &mut Vm, f: &str, args: &[Value]) -> Value {
    let a = match zpp::parse(vm, f, "O|l", args) {
        Ok(a) => a,
        Err(()) => return Value::Null,
    };
    let Some(sock) = sock_arg(vm, f, &a[0]) else {
        return Value::Null;
    };
    let backlog = a[1].long_or(0);
    if unsafe { libc::listen(sock.fd.get(), backlog as i32) } != 0 {
        sock_error(vm, f, &sock, "unable to listen on socket", errno());
        return Value::False;
    }
    Value::True
}

pub fn socket_close(vm: &mut Vm, f: &str, args: &[Value]) -> Value {
    let a = match zpp::parse(vm, f, "O", args) {
        Ok(a) => a,
        Err(()) => return Value::Null,
    };
    let Some(sock) = sock_arg(vm, f, &a[0]) else {
        return Value::Null;
    };
    match sock.zstream.get() {
        Some(sid) => {
            if let Some(h) = vm.socket_stream_hooks {
                (h.close_stream)(vm, sid);
            }
        }
        None => {
            if !sock.is_invalid() {
                unsafe { libc::close(sock.fd.get()) };
            }
        }
    }
    sock.zstream.set(None);
    let held = sock.zstream_ref.borrow_mut().take();
    drop(held);
    sock.fd.set(-1);
    Value::Null
}

pub fn socket_write(vm: &mut Vm, f: &str, args: &[Value]) -> Value {
    let a = match zpp::parse(vm, f, "Os|l!", args) {
        Ok(a) => a,
        Err(()) => return Value::Null,
    };
    let Some(sock) = sock_arg(vm, f, &a[0]) else {
        return Value::Null;
    };
    let data = a[1].str();
    let length = match &a[2] {
        Arg::Long(n) => *n,
        _ => data.len() as i64,
    };
    if length < 0 {
        zpp::value_error(vm, f, 3, "must be greater than or equal to 0");
        return Value::Null;
    }
    let n = (length as u64).min(data.len() as u64) as usize;
    vm.flush_cli_output();
    let r = unsafe { libc::write(sock.fd.get(), data.as_ptr() as *const libc::c_void, n) };
    if r < 0 {
        sock_error(vm, f, &sock, "unable to write to socket", errno());
        return Value::False;
    }
    Value::Long(r as i64)
}

/// php_read(): read up to `maxlen` bytes, stopping after a '\r' or '\n'.
fn php_read(sock: &Sock, buf: &mut [u8], maxlen: usize, flags: i32) -> isize {
    let fd = sock.fd.get();
    let mut m: isize = unsafe { libc::fcntl(fd, libc::F_GETFL) } as isize;
    if m < 0 {
        return m;
    }
    let nonblock = (m as i32) & libc::O_NONBLOCK != 0;
    m = 0;
    let mut n: usize = 0;
    let mut no_read = 0;
    set_errno(0);
    let mut t = 0usize;
    buf[0] = 0;
    while buf[t] != b'\n' && buf[t] != b'\r' && n < maxlen {
        if m > 0 {
            t += 1;
            n += 1;
        } else if m == 0 {
            no_read += 1;
            if nonblock && no_read >= 2 {
                return n as isize;
            }
            if no_read > 200 {
                set_errno(libc::ECONNRESET);
                return -1;
            }
        }
        if n < maxlen {
            m = unsafe { libc::recv(fd, buf[t..].as_mut_ptr() as *mut libc::c_void, 1, flags) };
        }
        let e = errno();
        if e != 0 && e != libc::ESPIPE && e != libc::EAGAIN {
            return -1;
        }
        set_errno(0);
    }
    if n < maxlen {
        n += 1;
    }
    n as isize
}

/// Buffer size for a read of `length` bytes (zend_string_alloc(length)).
fn read_buf(length: i64) -> Vec<u8> {
    vec![0u8; (length as usize).min(256 * 1024 * 1024) + 1]
}

pub fn socket_read(vm: &mut Vm, f: &str, args: &[Value]) -> Value {
    let a = match zpp::parse(vm, f, "Ol|l", args) {
        Ok(a) => a,
        Err(()) => return Value::Null,
    };
    let Some(sock) = sock_arg(vm, f, &a[0]) else {
        return Value::Null;
    };
    let length = a[1].long_or(0);
    let ty = a[2].long_or(2);
    if length <= 0 || length == i64::MAX {
        return Value::False;
    }
    let mut buf = read_buf(length);
    let len = buf.len() - 1;
    vm.flush_cli_output();
    let r = if ty == PHP_NORMAL_READ {
        php_read(&sock, &mut buf, len, 0)
    } else {
        unsafe { libc::recv(sock.fd.get(), buf.as_mut_ptr() as *mut libc::c_void, len, 0) }
    };
    if r == -1 {
        let e = errno();
        if e == libc::EAGAIN || e == libc::EWOULDBLOCK {
            sock.error.set(e);
            set_last_error(e);
        } else {
            sock_error(vm, f, &sock, "unable to read from socket", e);
        }
        return Value::False;
    } else if r == 0 {
        return vstr(b"");
    }
    buf.truncate(r as usize);
    Value::String(PhpString::from_vec(buf))
}

fn name_common(vm: &mut Vm, f: &str, args: &[Value], peer: bool) -> Value {
    let a = match zpp::parse(vm, f, "Oz|z", args) {
        Ok(a) => a,
        Err(()) => return Value::Null,
    };
    let Some(sock) = sock_arg(vm, f, &a[0]) else {
        return Value::Null;
    };
    let addr = a[1].any().cloned().unwrap_or(Value::Null);
    let port = a[2].any().cloned();
    let mut ss: libc::sockaddr_storage = unsafe { std::mem::zeroed() };
    let mut len = sa_len::<libc::sockaddr_storage>();
    let sap = &mut ss as *mut _ as *mut libc::sockaddr;
    let r = unsafe {
        if peer {
            libc::getpeername(sock.fd.get(), sap, &mut len)
        } else {
            libc::getsockname(sock.fd.get(), sap, &mut len)
        }
    };
    if r != 0 {
        let msg = if peer {
            "unable to retrieve peer name"
        } else {
            "unable to retrieve socket name"
        };
        sock_error(vm, f, &sock, msg, errno());
        return Value::False;
    }
    match ss.ss_family as i32 {
        libc::AF_INET6 => {
            let sin6 = unsafe { &*(&ss as *const _ as *const libc::sockaddr_in6) };
            assign_ref(&addr, vstr(inet_ntop6(&sin6.sin6_addr.s6_addr)));
            if let Some(p) = &port {
                assign_ref(p, Value::Long(u16::from_be(sin6.sin6_port) as i64));
            }
            Value::True
        }
        libc::AF_INET => {
            let sin = unsafe { &*(&ss as *const _ as *const libc::sockaddr_in) };
            assign_ref(&addr, vstr(inet_ntop4(&sin.sin_addr.s_addr.to_ne_bytes())));
            if let Some(p) = &port {
                assign_ref(p, Value::Long(u16::from_be(sin.sin_port) as i64));
            }
            Value::True
        }
        libc::AF_UNIX => {
            let sun = unsafe { &*(&ss as *const _ as *const libc::sockaddr_un) };
            assign_ref(&addr, vstr(sun_path_str(&sun.sun_path)));
            Value::True
        }
        libc::AF_PACKET if !peer => {
            let sll = unsafe { &*(&ss as *const _ as *const libc::sockaddr_ll) };
            let mut name = [0 as libc::c_char; libc::IF_NAMESIZE];
            if unsafe { libc::if_indextoname(sll.sll_ifindex as u32, name.as_mut_ptr()) }.is_null()
            {
                zpp::throw(vm, b"Error", "invalid interface index");
                return Value::Null;
            }
            assign_ref(&addr, vstr(sun_path_str(&name)));
            if let Some(p) = &port {
                assign_ref(p, Value::Long(sll.sll_ifindex as i64));
            }
            Value::True
        }
        _ => {
            let msg = if peer {
                "must be one of AF_UNIX, AF_INET, or AF_INET6"
            } else {
                "must be one of AF_UNIX, AF_PACKET, AF_INET, or AF_INET6"
            };
            zpp::value_error(vm, f, 1, msg);
            Value::Null
        }
    }
}

pub fn socket_getsockname(vm: &mut Vm, f: &str, args: &[Value]) -> Value {
    name_common(vm, f, args, false)
}

pub fn socket_getpeername(vm: &mut Vm, f: &str, args: &[Value]) -> Value {
    name_common(vm, f, args, true)
}

const TYPE_ERR: &str = "must be one of SOCK_STREAM, SOCK_DGRAM, SOCK_SEQPACKET, SOCK_RAW, or SOCK_RDM optionally OR'ed with SOCK_CLOEXEC, SOCK_NONBLOCK";

pub fn socket_create(vm: &mut Vm, f: &str, args: &[Value]) -> Value {
    let a = match zpp::parse(vm, f, "lll", args) {
        Ok(a) => a,
        Err(()) => return Value::Null,
    };
    let domain = a[0].long_or(0);
    let ty = a[1].long_or(0);
    let mut protocol = a[2].long_or(0);
    if domain != libc::AF_UNIX as i64
        && domain != libc::AF_INET6 as i64
        && domain != libc::AF_PACKET as i64
        && domain != libc::AF_INET as i64
    {
        zpp::value_error(
            vm,
            f,
            1,
            "must be one of AF_UNIX, AF_PACKET, AF_INET6, or AF_INET",
        );
        return Value::Null;
    }
    let checktype = ty & !((libc::SOCK_CLOEXEC | libc::SOCK_NONBLOCK) as i64);
    if checktype > 10 {
        zpp::value_error(vm, f, 2, TYPE_ERR);
        return Value::Null;
    }
    if protocol <= USHRT_MAX && domain == libc::AF_PACKET as i64 {
        protocol = (protocol as u16).to_be() as i64;
    }
    let (obj, sock) = new_socket(vm);
    let fd = unsafe { libc::socket(domain as i32, ty as i32, protocol as i32) };
    sock.fd.set(fd);
    sock.family.set(domain as i32);
    if fd < 0 {
        let e = errno();
        set_last_error(e);
        vm.emit_warning(&format!(
            "{}(): Unable to create socket [{}]: {}",
            f,
            e,
            sockets_strerror(e)
        ));
        return Value::False;
    }
    sock.error.set(0);
    sock.blocking.set(true);
    obj
}

/// A sockaddr_un with `path` copied in (memcpy of the whole string).
fn sockaddr_un_raw(path: &[u8]) -> libc::sockaddr_un {
    let mut sun: libc::sockaddr_un = unsafe { std::mem::zeroed() };
    sun.sun_family = libc::AF_UNIX as libc::sa_family_t;
    for (i, &c) in path.iter().enumerate().take(sun.sun_path.len()) {
        sun.sun_path[i] = c as libc::c_char;
    }
    sun
}

const SUN_PATH_LEN: usize = 108;
const SUN_PATH_OFFSET: usize = 2;

pub fn socket_connect(vm: &mut Vm, f: &str, args: &[Value]) -> Value {
    let a = match zpp::parse(vm, f, "Os|l!", args) {
        Ok(a) => a,
        Err(()) => return Value::Null,
    };
    let Some(sock) = sock_arg(vm, f, &a[0]) else {
        return Value::Null;
    };
    let addr = a[1].str().to_vec();
    let port = match &a[2] {
        Arg::Long(n) => Some(*n),
        _ => None,
    };
    let fd = sock.fd.get();
    vm.flush_cli_output();
    let retval = match sock.family.get() {
        libc::AF_INET6 => {
            let Some(port) = port else {
                zpp::value_error(vm, f, 3, "cannot be null when the socket type is AF_INET6");
                return Value::Null;
            };
            let Some((a6, scope)) = set_inet6_addr(vm, f, &addr, &sock) else {
                return Value::False;
            };
            let sin6 = sockaddr_in6(a6, scope, port as u16);
            unsafe {
                libc::connect(
                    fd,
                    &sin6 as *const _ as *const libc::sockaddr,
                    sa_len::<libc::sockaddr_in6>(),
                )
            }
        }
        libc::AF_INET => {
            let Some(port) = port else {
                zpp::value_error(vm, f, 3, "cannot be null when the socket type is AF_INET");
                return Value::Null;
            };
            let Some(a4) = set_inet_addr(vm, f, &addr, &sock) else {
                return Value::False;
            };
            let sin = sockaddr_in(a4, port as u16);
            unsafe {
                libc::connect(
                    fd,
                    &sin as *const _ as *const libc::sockaddr,
                    sa_len::<libc::sockaddr_in>(),
                )
            }
        }
        libc::AF_UNIX => {
            if addr.len() >= SUN_PATH_LEN {
                zpp::value_error(vm, f, 2, "must be less than 108");
                return Value::Null;
            }
            let sun = sockaddr_un_raw(&addr);
            unsafe {
                libc::connect(
                    fd,
                    &sun as *const _ as *const libc::sockaddr,
                    (SUN_PATH_OFFSET + addr.len()) as libc::socklen_t,
                )
            }
        }
        _ => {
            zpp::value_error(vm, f, 1, "must be one of AF_UNIX, AF_INET, or AF_INET6");
            return Value::Null;
        }
    };
    if retval != 0 {
        sock_error(vm, f, &sock, "unable to connect", errno());
        return Value::False;
    }
    Value::True
}

pub fn socket_strerror(vm: &mut Vm, f: &str, args: &[Value]) -> Value {
    let a = match zpp::parse(vm, f, "l", args) {
        Ok(a) => a,
        Err(()) => return Value::Null,
    };
    let e = a[0].long_or(0);
    if e < i32::MIN as i64 || e > i32::MAX as i64 {
        zpp::value_error(vm, f, 1, "must be between -2147483648 and 2147483647");
        return Value::Null;
    }
    vstr(sockets_strerror(e as i32))
}

pub fn socket_bind(vm: &mut Vm, f: &str, args: &[Value]) -> Value {
    let a = match zpp::parse(vm, f, "Os|l", args) {
        Ok(a) => a,
        Err(()) => return Value::Null,
    };
    let Some(sock) = sock_arg(vm, f, &a[0]) else {
        return Value::Null;
    };
    let addr = a[1].str().to_vec();
    let port = a[2].long_or(0);
    if !(0..=USHRT_MAX).contains(&port) {
        zpp::value_error(vm, f, 3, "must be between 0 and 65535");
        return Value::Null;
    }
    let fd = sock.fd.get();
    let retval = match sock.family.get() {
        libc::AF_UNIX => {
            if addr.len() >= SUN_PATH_LEN {
                zpp::value_error(vm, f, 2, "must be less than 108");
                return Value::Null;
            }
            let sun = sockaddr_un_raw(&addr);
            unsafe {
                libc::bind(
                    fd,
                    &sun as *const _ as *const libc::sockaddr,
                    (SUN_PATH_OFFSET + addr.len()) as libc::socklen_t,
                )
            }
        }
        libc::AF_INET => {
            let Some(a4) = set_inet_addr(vm, f, &addr, &sock) else {
                return Value::False;
            };
            let sin = sockaddr_in(a4, port as u16);
            unsafe {
                libc::bind(
                    fd,
                    &sin as *const _ as *const libc::sockaddr,
                    sa_len::<libc::sockaddr_in>(),
                )
            }
        }
        libc::AF_INET6 => {
            let Some((a6, scope)) = set_inet6_addr(vm, f, &addr, &sock) else {
                return Value::False;
            };
            let sin6 = sockaddr_in6(a6, scope, port as u16);
            unsafe {
                libc::bind(
                    fd,
                    &sin6 as *const _ as *const libc::sockaddr,
                    sa_len::<libc::sockaddr_in6>(),
                )
            }
        }
        libc::AF_PACKET => {
            let mut ss: libc::sockaddr_storage = unsafe { std::mem::zeroed() };
            // sizeof(sa): the size of a pointer
            let mut len = std::mem::size_of::<*const u8>() as libc::socklen_t;
            if unsafe { libc::getsockname(fd, &mut ss as *mut _ as *mut libc::sockaddr, &mut len) }
                < 0
            {
                zpp::plain_value_error(vm, "invalid AF_PACKET socket");
                return Value::Null;
            }
            let sll = unsafe { &mut *(&mut ss as *mut _ as *mut libc::sockaddr_ll) };
            let c = cstring(&addr);
            sll.sll_ifindex = unsafe { libc::if_nametoindex(c.as_ptr()) } as i32;
            unsafe {
                libc::bind(
                    fd,
                    &ss as *const _ as *const libc::sockaddr,
                    sa_len::<libc::sockaddr_ll>(),
                )
            }
        }
        _ => {
            zpp::value_error(
                vm,
                f,
                1,
                "must be one of AF_UNIX, AF_PACKET, AF_INET, or AF_INET6",
            );
            return Value::Null;
        }
    };
    if retval != 0 {
        sock_error(vm, f, &sock, "Unable to bind address", errno());
        return Value::False;
    }
    Value::True
}

pub fn socket_recv(vm: &mut Vm, f: &str, args: &[Value]) -> Value {
    let a = match zpp::parse(vm, f, "Ozll", args) {
        Ok(a) => a,
        Err(()) => return Value::Null,
    };
    let Some(sock) = sock_arg(vm, f, &a[0]) else {
        return Value::Null;
    };
    let target = a[1].any().cloned().unwrap_or(Value::Null);
    let len = a[2].long_or(0);
    let flags = a[3].long_or(0);
    if len <= 0 || len == i64::MAX {
        return Value::False;
    }
    let mut buf = read_buf(len);
    let n = buf.len() - 1;
    vm.flush_cli_output();
    let r = unsafe {
        libc::recv(
            sock.fd.get(),
            buf.as_mut_ptr() as *mut libc::c_void,
            n,
            flags as i32,
        )
    };
    let e = errno();
    if r < 1 {
        assign_ref(&target, Value::Null);
    } else {
        buf.truncate(r as usize);
        assign_ref(&target, Value::String(PhpString::from_vec(buf)));
    }
    if r == -1 {
        sock_error(vm, f, &sock, "Unable to read from socket", e);
        return Value::False;
    }
    Value::Long(r as i64)
}

pub fn socket_send(vm: &mut Vm, f: &str, args: &[Value]) -> Value {
    let a = match zpp::parse(vm, f, "Osll", args) {
        Ok(a) => a,
        Err(()) => return Value::Null,
    };
    let Some(sock) = sock_arg(vm, f, &a[0]) else {
        return Value::Null;
    };
    let data = a[1].str();
    let len = a[2].long_or(0);
    let flags = a[3].long_or(0);
    if len < 0 {
        zpp::value_error(vm, f, 3, "must be greater than or equal to 0");
        return Value::Null;
    }
    let n = (len as u64).min(data.len() as u64) as usize;
    vm.flush_cli_output();
    let r = unsafe {
        libc::send(
            sock.fd.get(),
            data.as_ptr() as *const libc::c_void,
            n,
            flags as i32,
        )
    };
    if r == -1 {
        sock_error(vm, f, &sock, "Unable to write to socket", errno());
        return Value::False;
    }
    Value::Long(r as i64)
}

pub fn socket_recvfrom(vm: &mut Vm, f: &str, args: &[Value]) -> Value {
    let a = match zpp::parse(vm, f, "Ozllz|z", args) {
        Ok(a) => a,
        Err(()) => return Value::Null,
    };
    let Some(sock) = sock_arg(vm, f, &a[0]) else {
        return Value::Null;
    };
    let data_ref = a[1].any().cloned().unwrap_or(Value::Null);
    let len = a[2].long_or(0);
    let flags = a[3].long_or(0) as i32;
    let addr_ref = a[4].any().cloned().unwrap_or(Value::Null);
    let port_ref = a[5].any().cloned();
    if len <= 0 || len > i64::MAX - 1 {
        return Value::False;
    }
    let mut buf = read_buf(len);
    let n = buf.len() - 1;
    let fd = sock.fd.get();
    vm.flush_cli_output();
    match sock.family.get() {
        libc::AF_UNIX => {
            let mut sun: libc::sockaddr_un = unsafe { std::mem::zeroed() };
            sun.sun_family = libc::AF_UNIX as libc::sa_family_t;
            let mut sl = sa_len::<libc::sockaddr_un>();
            let r = unsafe {
                libc::recvfrom(
                    fd,
                    buf.as_mut_ptr() as *mut libc::c_void,
                    n,
                    flags,
                    &mut sun as *mut _ as *mut libc::sockaddr,
                    &mut sl,
                )
            };
            if r < 0 {
                sock_error(vm, f, &sock, "Unable to recvfrom", errno());
                return Value::False;
            }
            buf.truncate(r as usize);
            assign_ref(&data_ref, Value::String(PhpString::from_vec(buf)));
            assign_ref(&addr_ref, vstr(sun_path_str(&sun.sun_path)));
            Value::Long(r as i64)
        }
        libc::AF_INET => {
            let Some(port_ref) = port_ref else {
                wrong_param_count(vm, f);
                return Value::Null;
            };
            let mut sin: libc::sockaddr_in = unsafe { std::mem::zeroed() };
            sin.sin_family = libc::AF_INET as libc::sa_family_t;
            let mut sl = sa_len::<libc::sockaddr_in>();
            let r = unsafe {
                libc::recvfrom(
                    fd,
                    buf.as_mut_ptr() as *mut libc::c_void,
                    n,
                    flags,
                    &mut sin as *mut _ as *mut libc::sockaddr,
                    &mut sl,
                )
            };
            if r < 0 {
                sock_error(vm, f, &sock, "Unable to recvfrom", errno());
                return Value::False;
            }
            buf.truncate(r as usize);
            assign_ref(&data_ref, Value::String(PhpString::from_vec(buf)));
            assign_ref(
                &addr_ref,
                vstr(inet_ntop4(&sin.sin_addr.s_addr.to_ne_bytes())),
            );
            assign_ref(&port_ref, Value::Long(u16::from_be(sin.sin_port) as i64));
            Value::Long(r as i64)
        }
        libc::AF_INET6 => {
            let Some(port_ref) = port_ref else {
                wrong_param_count(vm, f);
                return Value::Null;
            };
            let mut sin6: libc::sockaddr_in6 = unsafe { std::mem::zeroed() };
            sin6.sin6_family = libc::AF_INET6 as libc::sa_family_t;
            let mut sl = sa_len::<libc::sockaddr_in6>();
            let r = unsafe {
                libc::recvfrom(
                    fd,
                    buf.as_mut_ptr() as *mut libc::c_void,
                    n,
                    flags,
                    &mut sin6 as *mut _ as *mut libc::sockaddr,
                    &mut sl,
                )
            };
            if r < 0 {
                sock_error(vm, f, &sock, "unable to recvfrom", errno());
                return Value::False;
            }
            buf.truncate(r as usize);
            let a = inet_ntop6(&sin6.sin6_addr.s6_addr);
            assign_ref(&data_ref, Value::String(PhpString::from_vec(buf)));
            assign_ref(
                &addr_ref,
                vstr(if a.is_empty() { "::".to_string() } else { a }),
            );
            assign_ref(&port_ref, Value::Long(u16::from_be(sin6.sin6_port) as i64));
            Value::Long(r as i64)
        }
        _ => {
            zpp::value_error(vm, f, 1, "must be one of AF_UNIX, AF_INET, or AF_INET6");
            Value::Null
        }
    }
}

/// WRONG_PARAM_COUNT
fn wrong_param_count(vm: &mut Vm, f: &str) {
    zpp::throw(
        vm,
        b"ArgumentCountError",
        &format!("Wrong parameter count for {}()", f),
    );
}

pub fn socket_sendto(vm: &mut Vm, f: &str, args: &[Value]) -> Value {
    let a = match zpp::parse(vm, f, "Oslls|l!", args) {
        Ok(a) => a,
        Err(()) => return Value::Null,
    };
    let Some(sock) = sock_arg(vm, f, &a[0]) else {
        return Value::Null;
    };
    let data = a[1].str();
    let len = a[2].long_or(0);
    let flags = a[3].long_or(0) as i32;
    let addr = a[4].str().to_vec();
    let port = match &a[5] {
        Arg::Long(n) => Some(*n),
        _ => None,
    };
    let pv = port.unwrap_or(0);
    if !(0..=USHRT_MAX).contains(&pv) {
        zpp::value_error(vm, f, 6, "must be between 0 and 65535");
        return Value::Null;
    }
    if len < 0 {
        zpp::value_error(vm, f, 3, "must be greater than or equal to 0");
        return Value::Null;
    }
    let n = (len as u64).min(data.len() as u64) as usize;
    let fd = sock.fd.get();
    let dp = data.as_ptr() as *const libc::c_void;
    vm.flush_cli_output();
    let retval = match sock.family.get() {
        libc::AF_UNIX => {
            if addr.len() >= SUN_PATH_LEN {
                zpp::value_error(vm, f, 5, "must be less than 108");
                return Value::Null;
            }
            let path = c_bytes(&addr);
            let sun = sockaddr_un_raw(path);
            unsafe {
                libc::sendto(
                    fd,
                    dp,
                    n,
                    flags,
                    &sun as *const _ as *const libc::sockaddr,
                    (SUN_PATH_OFFSET + path.len()) as libc::socklen_t,
                )
            }
        }
        libc::AF_INET => {
            if port.is_none() {
                zpp::value_error(vm, f, 6, "cannot be null when the socket type is AF_INET");
                return Value::Null;
            }
            let Some(a4) = set_inet_addr(vm, f, &addr, &sock) else {
                return Value::False;
            };
            let sin = sockaddr_in(a4, pv as u16);
            unsafe {
                libc::sendto(
                    fd,
                    dp,
                    n,
                    flags,
                    &sin as *const _ as *const libc::sockaddr,
                    sa_len::<libc::sockaddr_in>(),
                )
            }
        }
        libc::AF_INET6 => {
            if port.is_none() {
                zpp::value_error(vm, f, 6, "cannot be null when the socket type is AF_INET6");
                return Value::Null;
            }
            let Some((a6, scope)) = set_inet6_addr(vm, f, &addr, &sock) else {
                return Value::False;
            };
            let sin6 = sockaddr_in6(a6, scope, pv as u16);
            unsafe {
                libc::sendto(
                    fd,
                    dp,
                    n,
                    flags,
                    &sin6 as *const _ as *const libc::sockaddr,
                    sa_len::<libc::sockaddr_in6>(),
                )
            }
        }
        _ => {
            zpp::value_error(vm, f, 1, "must be one of AF_UNIX, AF_INET, or AF_INET6");
            return Value::Null;
        }
    };
    if retval == -1 {
        sock_error(vm, f, &sock, "Unable to write to socket", errno());
        return Value::False;
    }
    Value::Long(retval as i64)
}

pub fn socket_create_pair(vm: &mut Vm, f: &str, args: &[Value]) -> Value {
    let a = match zpp::parse(vm, f, "lllz", args) {
        Ok(a) => a,
        Err(()) => return Value::Null,
    };
    let domain = a[0].long_or(0);
    let ty = a[1].long_or(0);
    let protocol = a[2].long_or(0);
    let target = a[3].any().cloned().unwrap_or(Value::Null);
    if domain != libc::AF_INET as i64
        && domain != libc::AF_INET6 as i64
        && domain != libc::AF_UNIX as i64
    {
        zpp::value_error(vm, f, 1, "must be one of AF_UNIX, AF_INET6, or AF_INET");
        return Value::Null;
    }
    let checktype = ty & !((libc::SOCK_CLOEXEC | libc::SOCK_NONBLOCK) as i64);
    if checktype > 10 {
        zpp::value_error(vm, f, 2, TYPE_ERR);
        return Value::Null;
    }
    let mut fds = [0i32; 2];
    if unsafe { libc::socketpair(domain as i32, ty as i32, protocol as i32, fds.as_mut_ptr()) } != 0
    {
        let e = errno();
        set_last_error(e);
        vm.emit_warning(&format!(
            "{}(): Unable to create socket pair [{}]: {}",
            f,
            e,
            sockets_strerror(e)
        ));
        return Value::False;
    }
    let mut arr = PhpArray::new();
    for (i, fd) in fds.iter().enumerate() {
        let (obj, s) = new_socket(vm);
        s.fd.set(*fd);
        s.family.set(domain as i32);
        arr.set(ArrayKey::Int(i as i64), obj);
    }
    assign_ref(&target, Value::Array(Rc::new(RefCell::new(arr))));
    Value::True
}

pub fn socket_shutdown(vm: &mut Vm, f: &str, args: &[Value]) -> Value {
    let a = match zpp::parse(vm, f, "O|l", args) {
        Ok(a) => a,
        Err(()) => return Value::Null,
    };
    let Some(sock) = sock_arg(vm, f, &a[0]) else {
        return Value::Null;
    };
    let how = a[1].long_or(2);
    if !(libc::SHUT_RD as i64..=libc::SHUT_RDWR as i64).contains(&how) {
        zpp::value_error(vm, f, 2, "must be one of SHUT_RD, SHUT_WR or SHUT_RDWR");
        return Value::Null;
    }
    if unsafe { libc::shutdown(sock.fd.get(), how as i32) } != 0 {
        sock_error(vm, f, &sock, "Unable to shutdown socket", errno());
        return Value::False;
    }
    Value::True
}

pub fn socket_atmark(vm: &mut Vm, f: &str, args: &[Value]) -> Value {
    let a = match zpp::parse(vm, f, "O", args) {
        Ok(a) => a,
        Err(()) => return Value::Null,
    };
    let Some(sock) = sock_arg(vm, f, &a[0]) else {
        return Value::Null;
    };
    let r = sock_atmark(sock.fd.get());
    if r < 0 {
        sock_error(vm, f, &sock, "Unable to apply sockmark", errno());
        Value::False
    } else {
        vbool(r != 0)
    }
}

pub fn socket_last_error(vm: &mut Vm, f: &str, args: &[Value]) -> Value {
    let a = match zpp::parse(vm, f, "|O!", args) {
        Ok(a) => a,
        Err(()) => return Value::Null,
    };
    if a[0].is_given() {
        let Some(sock) = sock_arg(vm, f, &a[0]) else {
            return Value::Null;
        };
        Value::Long(sock.error.get() as i64)
    } else {
        Value::Long(last_error() as i64)
    }
}

pub fn socket_clear_error(vm: &mut Vm, f: &str, args: &[Value]) -> Value {
    let a = match zpp::parse(vm, f, "|O!", args) {
        Ok(a) => a,
        Err(()) => return Value::Null,
    };
    if a[0].is_given() {
        let Some(sock) = sock_arg(vm, f, &a[0]) else {
            return Value::Null;
        };
        sock.error.set(0);
    } else {
        set_last_error(0);
    }
    Value::Null
}

/// socket_import_file_descriptor()
pub fn import_fd(vm: &mut Vm, f: &str, fd: i32, sock: &Sock) -> bool {
    sock.fd.set(fd);
    let mut ty: i32 = 0;
    let mut tl = sa_len::<i32>();
    if unsafe {
        libc::getsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_DOMAIN,
            &mut ty as *mut i32 as *mut libc::c_void,
            &mut tl,
        )
    } == 0
    {
        sock.family.set(ty);
    } else {
        let mut ss: libc::sockaddr_storage = unsafe { std::mem::zeroed() };
        let mut sl = sa_len::<libc::sockaddr_storage>();
        if unsafe { libc::getsockname(fd, &mut ss as *mut _ as *mut libc::sockaddr, &mut sl) } == 0
        {
            sock.family.set(ss.ss_family as i32);
        } else {
            sock_error(vm, f, sock, "Unable to obtain socket family", errno());
            return false;
        }
    }
    let t = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if t == -1 {
        sock_error(vm, f, sock, "Unable to obtain blocking state", errno());
        return false;
    }
    sock.blocking.set(t & libc::O_NONBLOCK == 0);
    true
}

pub fn socket_import_stream(vm: &mut Vm, f: &str, args: &[Value]) -> Value {
    let a = match zpp::parse(vm, f, "r", args) {
        Ok(a) => a,
        Err(()) => return Value::Null,
    };
    let Arg::Res(sid) = a[0] else {
        return Value::Null;
    };
    let Some(hooks) = vm.socket_stream_hooks else {
        return Value::False;
    };
    let fd = match (hooks.stream_socket_fd)(vm, &Value::resource(sid), f) {
        Ok(Some(fd)) => fd,
        Ok(None) => return Value::False,
        Err(_) => return Value::Null,
    };
    let (obj, sock) = new_socket(vm);
    if !import_fd(vm, f, fd, &sock) {
        // the stream keeps its descriptor
        sock.fd.set(-1);
        return Value::False;
    }
    sock.zstream.set(Some(sid));
    *sock.zstream_ref.borrow_mut() = Some(Value::resource(sid));
    obj
}

pub fn socket_export_stream(vm: &mut Vm, f: &str, args: &[Value]) -> Value {
    let a = match zpp::parse(vm, f, "O", args) {
        Ok(a) => a,
        Err(()) => return Value::Null,
    };
    let Some(sock) = sock_arg(vm, f, &a[0]) else {
        return Value::Null;
    };
    if let Some(sid) = sock.zstream.get() {
        return Value::resource(sid);
    }
    let Some(hooks) = vm.socket_stream_hooks else {
        vm.emit_warning(&format!("{}(): Failed to create stream", f));
        return Value::False;
    };
    let sid = (hooks.stream_from_socket)(vm, sock.fd.get());
    goro_ext_standard::sockets_streams::ext_set_blocking(sid, sock.blocking.get());
    sock.zstream.set(Some(sid));
    let v = Value::resource(sid);
    *sock.zstream_ref.borrow_mut() = Some(v.clone());
    v
}

const HINTS_KEYS_ERR: &str =
    "must only contain array keys \"ai_flags\", \"ai_socktype\", \"ai_protocol\", or \"ai_family\"";

pub fn socket_addrinfo_lookup(vm: &mut Vm, f: &str, args: &[Value]) -> Value {
    let a = match zpp::parse(vm, f, "s|s!a", args) {
        Ok(a) => a,
        Err(()) => return Value::Null,
    };
    let host = a[0].str().to_vec();
    let service = match &a[1] {
        Arg::Str(s) => Some(s.clone()),
        _ => None,
    };
    let mut hints: libc::addrinfo = unsafe { std::mem::zeroed() };
    if let Arg::Arr(h) = &a[2] {
        let h = h.borrow().clone();
        let packed = !h.is_empty() && h.keys().all(|k| matches!(k, ArrayKey::Int(_)));
        if packed {
            zpp::value_error(vm, f, 3, HINTS_KEYS_ERR);
            return Value::Null;
        }
        for (k, v) in h.iter() {
            let key = match k {
                ArrayKey::String(s) => s.as_bytes().to_vec(),
                _ => {
                    zpp::value_error(vm, f, 3, HINTS_KEYS_ERR);
                    return Value::Null;
                }
            };
            let name = match key.as_slice() {
                b"ai_flags" | b"ai_socktype" | b"ai_protocol" | b"ai_family" => {
                    String::from_utf8_lossy(&key).into_owned()
                }
                _ => {
                    zpp::value_error(vm, f, 3, HINTS_KEYS_ERR);
                    return Value::Null;
                }
            };
            let Some(val) = zpp::try_get_long(v) else {
                zpp::arg_type_error(
                    vm,
                    f,
                    3,
                    &format!(
                        "\"{}\" key must be of type int, {} given",
                        name,
                        Vm::value_type_name(&v.deref())
                    ),
                );
                return Value::Null;
            };
            if name == "ai_family" {
                if val != libc::AF_INET as i64 && val != libc::AF_INET6 as i64 {
                    zpp::value_error(vm, f, 3, "\"ai_family\" key must be AF_INET or AF_INET6");
                    return Value::Null;
                }
                hints.ai_family = val as i32;
                continue;
            }
            if val < 0 || val > i32::MAX as i64 {
                zpp::value_error(
                    vm,
                    f,
                    3,
                    &format!("\"{}\" key must be between 0 and 2147483647", name),
                );
                return Value::Null;
            }
            match name.as_str() {
                "ai_flags" => hints.ai_flags = val as i32,
                "ai_socktype" => hints.ai_socktype = val as i32,
                _ => hints.ai_protocol = val as i32,
            }
        }
    }
    let hc = cstring(&host);
    let sc = service.as_ref().map(|s| cstring(s));
    let mut res: *mut libc::addrinfo = std::ptr::null_mut();
    let r = unsafe {
        libc::getaddrinfo(
            hc.as_ptr(),
            sc.as_ref().map(|c| c.as_ptr()).unwrap_or(std::ptr::null()),
            &hints,
            &mut res,
        )
    };
    if r != 0 {
        return Value::False;
    }
    let mut out = PhpArray::new();
    let mut rp = res;
    while !rp.is_null() {
        let ai = unsafe { &*rp };
        if ai.ai_family != libc::AF_UNSPEC {
            let addr = unsafe {
                std::slice::from_raw_parts(ai.ai_addr as *const u8, ai.ai_addrlen as usize)
            }
            .to_vec();
            let canon = if ai.ai_canonname.is_null() {
                None
            } else {
                Some(
                    unsafe { CStr::from_ptr(ai.ai_canonname) }
                        .to_bytes()
                        .to_vec(),
                )
            };
            let v = new_addrinfo(
                vm,
                AddrInfo {
                    flags: ai.ai_flags,
                    family: ai.ai_family,
                    socktype: ai.ai_socktype,
                    protocol: ai.ai_protocol,
                    addr,
                    canonname: canon,
                },
            );
            out.push(v);
        }
        rp = ai.ai_next;
    }
    unsafe { libc::freeaddrinfo(res) };
    Value::Array(Rc::new(RefCell::new(out)))
}

fn addrinfo_socket(vm: &mut Vm, f: &str, args: &[Value], bind: bool) -> Value {
    let a = match zpp::parse(vm, f, "A", args) {
        Ok(a) => a,
        Err(()) => return Value::Null,
    };
    let Some(ai) = a[0].obj().and_then(|o| addrinfo_of(&o)) else {
        return Value::False;
    };
    let mut protocol = ai.protocol;
    if protocol as i64 <= USHRT_MAX && ai.family == libc::AF_PACKET {
        protocol = (protocol as u16).to_be() as i32;
    }
    let (obj, sock) = new_socket(vm);
    let fd = unsafe { libc::socket(ai.family, ai.socktype, protocol) };
    sock.fd.set(fd);
    sock.family.set(ai.family);
    if fd < 0 {
        let e = errno();
        set_last_error(e);
        vm.emit_warning(&format!(
            "{}(): Unable to create socket [{}]: {}",
            f,
            e,
            sockets_strerror(e)
        ));
        return Value::False;
    }
    sock.error.set(0);
    sock.blocking.set(true);
    let sap = ai.addr.as_ptr() as *const libc::sockaddr;
    let sl = ai.addr.len() as libc::socklen_t;
    vm.flush_cli_output();
    let r = unsafe {
        if bind {
            libc::bind(fd, sap, sl)
        } else {
            libc::connect(fd, sap, sl)
        }
    };
    if r != 0 {
        let msg = if bind {
            "Unable to bind address"
        } else {
            "Unable to connect address"
        };
        sock_error(vm, f, &sock, msg, errno());
        unsafe { libc::close(fd) };
        sock.fd.set(-1);
        return Value::False;
    }
    obj
}

pub fn socket_addrinfo_bind(vm: &mut Vm, f: &str, args: &[Value]) -> Value {
    addrinfo_socket(vm, f, args, true)
}

pub fn socket_addrinfo_connect(vm: &mut Vm, f: &str, args: &[Value]) -> Value {
    addrinfo_socket(vm, f, args, false)
}

fn skey(s: &str) -> ArrayKey {
    ArrayKey::String(PhpString::from_bytes(s.as_bytes()))
}

pub fn socket_addrinfo_explain(vm: &mut Vm, f: &str, args: &[Value]) -> Value {
    let a = match zpp::parse(vm, f, "A", args) {
        Ok(a) => a,
        Err(()) => return Value::Null,
    };
    let Some(ai) = a[0].obj().and_then(|o| addrinfo_of(&o)) else {
        return Value::False;
    };
    let mut out = PhpArray::new();
    out.set(skey("ai_flags"), Value::Long(ai.flags as i64));
    out.set(skey("ai_family"), Value::Long(ai.family as i64));
    out.set(skey("ai_socktype"), Value::Long(ai.socktype as i64));
    out.set(skey("ai_protocol"), Value::Long(ai.protocol as i64));
    if let Some(c) = &ai.canonname {
        out.set(skey("ai_canonname"), vstr(c));
    }
    let mut sa = PhpArray::new();
    match ai.family {
        libc::AF_INET if ai.addr.len() >= std::mem::size_of::<libc::sockaddr_in>() => {
            let sin = unsafe { &*(ai.addr.as_ptr() as *const libc::sockaddr_in) };
            sa.set(
                skey("sin_port"),
                Value::Long(u16::from_be(sin.sin_port) as i64),
            );
            sa.set(
                skey("sin_addr"),
                vstr(inet_ntop4(&sin.sin_addr.s_addr.to_ne_bytes())),
            );
        }
        libc::AF_INET6 if ai.addr.len() >= std::mem::size_of::<libc::sockaddr_in6>() => {
            let sin6 = unsafe { &*(ai.addr.as_ptr() as *const libc::sockaddr_in6) };
            sa.set(
                skey("sin6_port"),
                Value::Long(u16::from_be(sin6.sin6_port) as i64),
            );
            sa.set(skey("sin6_addr"), vstr(inet_ntop6(&sin6.sin6_addr.s6_addr)));
        }
        _ => {}
    }
    out.set(skey("ai_addr"), Value::Array(Rc::new(RefCell::new(sa))));
    Value::Array(Rc::new(RefCell::new(out)))
}

use std::ffi::CStr;
