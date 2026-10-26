//! Socket transports: main/streams/xp_socket.c, transports.c and the
//! socket parts of main/network.c, plus the socket-related stream
//! functions of ext/standard/streamsfuncs.c and fsock.c.
//!
//! Socket streams are ordinary `FILE_HANDLES` entries whose `file` wraps
//! the socket descriptor; `FileHandle::sock` holds php_netstream_data_t.

use super::*;
use std::os::unix::io::{AsRawFd, FromRawFd};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum SockKind {
    /// php_stream_generic_socket_ops (stream_socket_pair()).
    Generic,
    Tcp,
    Udp,
    Unix,
    Udg,
}

impl SockKind {
    /// php_stream_ops.label
    pub(crate) fn label(self) -> &'static str {
        match self {
            // The openssl extension registers its own "tcp" factory.
            SockKind::Tcp => "tcp_socket/ssl",
            SockKind::Generic => "generic_socket",
            SockKind::Udp => "udp_socket",
            SockKind::Unix => "unix_socket",
            SockKind::Udg => "udg_socket",
        }
    }

    fn is_unix(self) -> bool {
        matches!(self, SockKind::Unix | SockKind::Udg)
    }

    fn socktype(self) -> i32 {
        match self {
            SockKind::Udp | SockKind::Udg => libc::SOCK_DGRAM,
            _ => libc::SOCK_STREAM,
        }
    }
}

/// php_netstream_data_t
#[derive(Clone, Debug)]
pub(crate) struct SockData {
    pub kind: SockKind,
    pub is_blocked: bool,
    /// struct timeval; tv_sec == -1 means "no timeout".
    pub tv_sec: i64,
    pub tv_usec: i64,
    pub timeout_event: bool,
    /// PHP_STREAM_FLAG_NO_IO (bound/listening server sockets).
    pub no_io: bool,
    /// ssl:// & co: sslsock->enable_on_connect and sslsock->method.
    pub enable_on_connect: bool,
    pub crypto_method: i64,
    /// The crypto layer was set up for a client (STREAM_CRYPTO_IS_CLIENT).
    pub crypto_client: bool,
    /// sslsock->url_name: the host of the address the stream was opened for.
    pub url_name: Option<Vec<u8>>,
    /// sslsock->connect_timeout in milliseconds (-1: none).
    pub connect_timeout_ms: i32,
    /// The stream context resource the stream was created with.
    pub ctx_src: Option<i64>,
}

impl SockData {
    fn new(kind: SockKind, default_timeout: i64) -> Self {
        SockData {
            kind,
            is_blocked: true,
            tv_sec: default_timeout,
            tv_usec: 0,
            timeout_event: false,
            no_io: false,
            enable_on_connect: false,
            crypto_method: 0,
            crypto_client: false,
            url_name: None,
            connect_timeout_ms: 0,
            ctx_src: None,
        }
    }

    fn timeout_ms(&self) -> i32 {
        if self.tv_sec == -1 {
            -1
        } else {
            tv_to_ms(self.tv_sec, self.tv_usec)
        }
    }
}

const PHP_STREAM_CLIENT_PERSISTENT: i64 = 1;
const PHP_STREAM_CLIENT_ASYNC_CONNECT: i64 = 2;
const PHP_STREAM_CLIENT_CONNECT: i64 = 4;
const STREAM_XPORT_BIND: i64 = 4;
const STREAM_XPORT_LISTEN: i64 = 8;
const STREAM_OOB: i64 = 1;
const STREAM_PEEK: i64 = 2;
const PHP_TIMEOUT_ULL_MAX: f64 = u64::MAX as f64;

/// php_tvtoto()
fn tv_to_ms(sec: i64, usec: i64) -> i32 {
    (sec.wrapping_mul(1000).wrapping_add(usec / 1000)) as i32
}

fn errno() -> i32 {
    std::io::Error::last_os_error().raw_os_error().unwrap_or(0)
}

/// php_socket_strerror() / php_socket_error_str()
pub(crate) fn strerror(err: i32) -> String {
    unsafe {
        let p = libc::strerror(err);
        if p.is_null() {
            return format!("Unknown error {}", err);
        }
        std::ffi::CStr::from_ptr(p).to_string_lossy().into_owned()
    }
}

/// php_pollfd_for(): poll() a single descriptor; returns revents (> 0),
/// 0 on timeout or -1 on error.
fn pollfd_for(fd: i32, events: i16, timeout_ms: i32) -> i32 {
    let mut p = libc::pollfd {
        fd,
        events,
        revents: 0,
    };
    let n = unsafe { libc::poll(&mut p, 1, timeout_ms) };
    if n > 0 { p.revents as i32 } else { n }
}

const POLLREADABLE: i16 = libc::POLLIN | libc::POLLERR | libc::POLLHUP;

fn throw(vm: &mut Vm, class: &[u8], msg: &str) -> VmError {
    let exc = if class == b"TypeError" {
        vm.throw_type_error(msg.to_string())
    } else {
        vm.create_exception(class, msg, vm.current_line)
    };
    vm.current_exception = Some(exc);
    VmError {
        message: msg.to_string(),
        line: vm.current_line,
    }
}

/// zend_throw_exception() while an exception may be pending: the new one
/// takes it as its previous exception.
fn throw_chained(vm: &mut Vm, class: &[u8], msg: &str) {
    let exc = vm.create_exception(class, msg, vm.current_line);
    vm.chain_pending_exception(&exc);
    vm.current_exception = Some(exc);
}

fn type_error(vm: &mut Vm, func: &str, n: usize, name: &str, ty: &str, v: &Value) -> VmError {
    let msg = format!(
        "{}(): Argument #{} (${}) must be of type {}, {} given",
        func,
        n,
        name,
        ty,
        Vm::value_type_name(v)
    );
    throw(vm, b"TypeError", &msg)
}

fn null_deprecation(vm: &mut Vm, func: &str, n: usize, name: &str, ty: &str) {
    vm.emit_deprecated(&format!(
        "{}(): Passing null to parameter #{} (${}) of type {} is deprecated",
        func, n, name, ty
    ));
}

/// Z_PARAM_LONG (weak mode) for argument `n` (1-based).
fn zpp_long(
    vm: &mut Vm,
    func: &str,
    n: usize,
    name: &str,
    v: &Value,
    nullable: bool,
) -> Result<Option<i64>, VmError> {
    let v = v.deref();
    match &v {
        Value::Long(l) => Ok(Some(*l)),
        Value::Null | Value::Undef => {
            if nullable {
                Ok(None)
            } else {
                null_deprecation(vm, func, n, name, "int");
                Ok(Some(0))
            }
        }
        Value::True => Ok(Some(1)),
        Value::False => Ok(Some(0)),
        Value::Double(d) => double_to_long(vm, func, n, name, *d, &v, nullable),
        Value::String(s) => match goro_core::value::parse_numeric_str(s.as_bytes(), false) {
            Some((goro_core::value::NumStr::Long(l), _)) => Ok(Some(l)),
            Some((goro_core::value::NumStr::Double(d), _)) => {
                if !d.is_finite() || d.fract() != 0.0 {
                    if (-9.223_372_036_854_776e18..9.223_372_036_854_776e18).contains(&d) {
                        vm.emit_deprecated(&format!(
                            "Implicit conversion from float-string \"{}\" to int loses precision",
                            s.to_msg_string()
                        ));
                        return Ok(Some(d as i64));
                    }
                    return Err(type_error(vm, func, n, name, ty_int(nullable), &v));
                }
                if d < -9.223_372_036_854_776e18 || d >= 9.223_372_036_854_776e18 {
                    return Err(type_error(vm, func, n, name, ty_int(nullable), &v));
                }
                Ok(Some(d as i64))
            }
            None => Err(type_error(vm, func, n, name, ty_int(nullable), &v)),
        },
        _ => Err(type_error(vm, func, n, name, ty_int(nullable), &v)),
    }
}

fn ty_int(nullable: bool) -> &'static str {
    if nullable { "?int" } else { "int" }
}

fn double_to_long(
    vm: &mut Vm,
    func: &str,
    n: usize,
    name: &str,
    d: f64,
    v: &Value,
    nullable: bool,
) -> Result<Option<i64>, VmError> {
    if !d.is_finite() || !(-9.223_372_036_854_776e18..9.223_372_036_854_776e18).contains(&d) {
        return Err(type_error(vm, func, n, name, ty_int(nullable), v));
    }
    if d.fract() != 0.0 {
        vm.emit_deprecated(&format!(
            "Implicit conversion from float {} to int loses precision",
            goro_core::value::format_php_float_repr(d)
        ));
        if vm.current_exception.is_some() {
            return Err(VmError {
                message: String::new(),
                line: vm.current_line,
            });
        }
    }
    Ok(Some(d as i64))
}

/// Z_PARAM_DOUBLE(_OR_NULL) (weak mode).
fn zpp_double(
    vm: &mut Vm,
    func: &str,
    n: usize,
    name: &str,
    v: &Value,
    nullable: bool,
) -> Result<Option<f64>, VmError> {
    let v = v.deref();
    let ty = if nullable { "?float" } else { "float" };
    match &v {
        Value::Double(d) => Ok(Some(*d)),
        Value::Long(l) => Ok(Some(*l as f64)),
        Value::Null | Value::Undef => {
            if nullable {
                Ok(None)
            } else {
                null_deprecation(vm, func, n, name, "float");
                Ok(Some(0.0))
            }
        }
        Value::True => Ok(Some(1.0)),
        Value::False => Ok(Some(0.0)),
        Value::String(s) => match goro_core::value::parse_numeric_str(s.as_bytes(), false) {
            Some((goro_core::value::NumStr::Long(l), _)) => Ok(Some(l as f64)),
            Some((goro_core::value::NumStr::Double(d), _)) => Ok(Some(d)),
            None => Err(type_error(vm, func, n, name, ty, &v)),
        },
        _ => Err(type_error(vm, func, n, name, ty, &v)),
    }
}

/// Z_PARAM_STRING (weak mode).
fn zpp_string(
    vm: &mut Vm,
    func: &str,
    n: usize,
    name: &str,
    v: &Value,
) -> Result<Vec<u8>, VmError> {
    let v = v.deref();
    match &v {
        Value::String(s) => Ok(s.as_bytes().to_vec()),
        Value::Long(_) | Value::Double(_) | Value::True | Value::False => {
            Ok(v.to_php_string().as_bytes().to_vec())
        }
        Value::Null | Value::Undef => {
            null_deprecation(vm, func, n, name, "string");
            Ok(Vec::new())
        }
        Value::Object(_) => match vm.value_to_string_checked(&v) {
            Ok(s) => Ok(s.as_bytes().to_vec()),
            Err(_) if vm.current_exception.is_some() => Err(VmError {
                message: String::new(),
                line: vm.current_line,
            }),
            Err(_) => Err(type_error(vm, func, n, name, "string", &v)),
        },
        _ => Err(type_error(vm, func, n, name, "string", &v)),
    }
}

/// Z_PARAM_BOOL (weak mode).
fn zpp_bool(vm: &mut Vm, func: &str, n: usize, name: &str, v: &Value) -> Result<bool, VmError> {
    let v = v.deref();
    match &v {
        Value::Null | Value::Undef => {
            null_deprecation(vm, func, n, name, "bool");
            Ok(false)
        }
        Value::Array(_) | Value::Object(_) | Value::Resource(_) => {
            Err(type_error(vm, func, n, name, "bool", &v))
        }
        _ => Ok(v.is_truthy()),
    }
}

/// ZEND_PARSE_PARAMETERS_START(min, max) argument-count check.
fn check_arity(
    vm: &mut Vm,
    func: &str,
    args: &[Value],
    min: usize,
    max: usize,
) -> Result<(), VmError> {
    let n = args.len();
    if n >= min && n <= max {
        return Ok(());
    }
    let (q, c) = if min == max {
        ("exactly", min)
    } else if n < min {
        ("at least", min)
    } else {
        ("at most", max)
    };
    let msg = format!(
        "{}() expects {} {} argument{}, {} given",
        func,
        q,
        c,
        if c == 1 { "" } else { "s" },
        n
    );
    Err(throw(vm, b"ArgumentCountError", &msg))
}

fn set_ref(v: Option<&Value>, val: Value) {
    if let Some(Value::Reference(r)) = v {
        *r.borrow_mut() = val;
    }
}

fn str_val(s: &str) -> Value {
    Value::String(PhpString::from_bytes(s.as_bytes()))
}

fn default_socket_timeout(vm: &Vm) -> i64 {
    vm.constants
        .get(b"default_socket_timeout".as_ref())
        .map(|v| v.to_long())
        .unwrap_or(60)
}

// ---------------------------------------------------------------------------
// Socket descriptor helpers
// ---------------------------------------------------------------------------

/// Returns the socket data and descriptor of a stream, if it is a socket.
fn sock_of(fid: i64) -> Option<(SockData, i32)> {
    FILE_HANDLES.with(|h| {
        h.borrow()
            .get(&fid)
            .and_then(|fh| fh.sock.clone().map(|s| (s, fh.file.as_raw_fd())))
    })
}

fn with_sock<R>(fid: i64, f: impl FnOnce(&mut FileHandle) -> R) -> Option<R> {
    FILE_HANDLES.with(|h| h.borrow_mut().get_mut(&fid).map(f))
}

/// php_set_sock_blocking()
fn set_fd_blocking(fd: i32, block: bool) -> bool {
    unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFL, 0);
        if flags < 0 {
            return false;
        }
        let nf = if block {
            flags & !libc::O_NONBLOCK
        } else {
            flags | libc::O_NONBLOCK
        };
        libc::fcntl(fd, libc::F_SETFL, nf) != -1
    }
}

/// php_network_populate_name_from_sockaddr(): text form of an address.
fn sockaddr_to_text(sa: &libc::sockaddr_storage, sl: libc::socklen_t) -> Option<Vec<u8>> {
    match sa.ss_family as i32 {
        libc::AF_INET => {
            let sin = unsafe { &*(sa as *const _ as *const libc::sockaddr_in) };
            let ip = std::net::Ipv4Addr::from(u32::from_be(sin.sin_addr.s_addr));
            Some(format!("{}:{}", ip, u16::from_be(sin.sin_port)).into_bytes())
        }
        libc::AF_INET6 => {
            let sin6 = unsafe { &*(sa as *const _ as *const libc::sockaddr_in6) };
            let ip = std::net::Ipv6Addr::from(sin6.sin6_addr.s6_addr);
            Some(format!("[{}]:{}", ip, u16::from_be(sin6.sin6_port)).into_bytes())
        }
        libc::AF_UNIX => {
            let sun = unsafe { &*(sa as *const _ as *const libc::sockaddr_un) };
            let path: Vec<u8> = sun.sun_path.iter().map(|&c| c as u8).collect();
            if path.first() == Some(&0) {
                // Abstract name: everything after the family.
                let len = (sl as usize).saturating_sub(std::mem::size_of::<libc::sa_family_t>());
                Some(path[..len.min(path.len())].to_vec())
            } else {
                let len = path.iter().position(|&c| c == 0).unwrap_or(path.len());
                Some(path[..len].to_vec())
            }
        }
        _ => None,
    }
}

type SockAddr = (libc::sockaddr_storage, libc::socklen_t);

fn ipv6_works() -> bool {
    thread_local! {
        static IPV6: std::cell::Cell<i8> = const { std::cell::Cell::new(-1) };
    }
    IPV6.with(|c| {
        if c.get() == -1 {
            let s = unsafe { libc::socket(libc::PF_INET6, libc::SOCK_DGRAM, 0) };
            if s < 0 {
                c.set(0);
            } else {
                unsafe { libc::close(s) };
                c.set(1);
            }
        }
        c.get() == 1
    })
}

/// php_network_getaddresses(): resolve `host`; on failure emits the
/// warning (under `func`) and returns the error text.
fn getaddresses(
    vm: &mut Vm,
    func: &str,
    host: &[u8],
    socktype: i32,
) -> Result<Vec<SockAddr>, String> {
    let chost = match std::ffi::CString::new(host.to_vec()) {
        Ok(c) => c,
        Err(_) => return Err(String::new()),
    };
    let mut hints: libc::addrinfo = unsafe { std::mem::zeroed() };
    hints.ai_family = if ipv6_works() {
        libc::AF_UNSPEC
    } else {
        libc::AF_INET
    };
    hints.ai_socktype = socktype;
    let mut res: *mut libc::addrinfo = std::ptr::null_mut();
    let n = unsafe { libc::getaddrinfo(chost.as_ptr(), std::ptr::null(), &hints, &mut res) };
    if n != 0 {
        let gai = unsafe { std::ffi::CStr::from_ptr(libc::gai_strerror(n)) }
            .to_string_lossy()
            .into_owned();
        let msg = format!(
            "php_network_getaddresses: getaddrinfo for {} failed: {}",
            goro_core::string::msg_str(host),
            gai
        );
        vm.emit_warning(&format!("{}(): {}", func, msg));
        return Err(msg);
    }
    let mut out = Vec::new();
    let mut p = res;
    while !p.is_null() {
        let ai = unsafe { &*p };
        let mut ss: libc::sockaddr_storage = unsafe { std::mem::zeroed() };
        let len = (ai.ai_addrlen as usize).min(std::mem::size_of::<libc::sockaddr_storage>());
        unsafe {
            std::ptr::copy_nonoverlapping(
                ai.ai_addr as *const u8,
                &mut ss as *mut _ as *mut u8,
                len,
            );
        }
        out.push((ss, ai.ai_addrlen));
        p = ai.ai_next;
    }
    unsafe { libc::freeaddrinfo(res) };
    Ok(out)
}

fn set_port(sa: &mut libc::sockaddr_storage, port: u16) -> Option<libc::socklen_t> {
    match sa.ss_family as i32 {
        libc::AF_INET => {
            let sin = unsafe { &mut *(sa as *mut _ as *mut libc::sockaddr_in) };
            sin.sin_port = port.to_be();
            Some(std::mem::size_of::<libc::sockaddr_in>() as libc::socklen_t)
        }
        libc::AF_INET6 => {
            let sin6 = unsafe { &mut *(sa as *mut _ as *mut libc::sockaddr_in6) };
            sin6.sin6_port = port.to_be();
            Some(std::mem::size_of::<libc::sockaddr_in6>() as libc::socklen_t)
        }
        _ => None,
    }
}

fn setsockopt_int(fd: i32, level: i32, name: i32, val: i32) {
    unsafe {
        libc::setsockopt(
            fd,
            level,
            name,
            &val as *const i32 as *const libc::c_void,
            std::mem::size_of::<i32>() as libc::socklen_t,
        );
    }
}

/// C atoi(): leading whitespace, optional sign, digits; wraps like int.
fn atoi(s: &[u8]) -> i32 {
    let mut i = 0;
    while i < s.len() && matches!(s[i], b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c) {
        i += 1;
    }
    let mut neg = false;
    if i < s.len() && (s[i] == b'+' || s[i] == b'-') {
        neg = s[i] == b'-';
        i += 1;
    }
    let mut n: i64 = 0;
    while i < s.len() && s[i].is_ascii_digit() {
        n = n.wrapping_mul(10).wrapping_add((s[i] - b'0') as i64);
        i += 1;
    }
    (if neg { -n } else { n }) as i32
}

/// parse_ip_address_ex()
fn parse_ip_address(s: &[u8]) -> Result<(Vec<u8>, i32), String> {
    if s.contains(&0) {
        return Err("The hostname must not contain null bytes".to_string());
    }
    if s.first() == Some(&b'[') && s.len() > 1 {
        let inner = &s[1..s.len() - 1];
        if let Some(p) = inner.iter().position(|&c| c == b']') {
            let close = p + 1;
            if s.get(close + 1) == Some(&b':') {
                return Ok((s[1..close].to_vec(), atoi(&s[close + 2..])));
            }
        }
        return Err(format!(
            "Failed to parse IPv6 address \"{}\"",
            goro_core::string::msg_str(s)
        ));
    }
    let colon = if s.is_empty() {
        None
    } else {
        s[..s.len() - 1].iter().position(|&c| c == b':')
    };
    match colon {
        Some(c) => Ok((s[..c].to_vec(), atoi(&s[c + 1..]))),
        None => Err(format!(
            "Failed to parse address \"{}\"",
            goro_core::string::msg_str(s)
        )),
    }
}

/// parse_unix_address(): build a sockaddr_un; returns it and the address
/// length to pass to bind()/connect().
fn parse_unix_address(
    vm: &mut Vm,
    func: &str,
    name: &[u8],
) -> (libc::sockaddr_un, libc::socklen_t) {
    let mut sun: libc::sockaddr_un = unsafe { std::mem::zeroed() };
    sun.sun_family = libc::AF_UNIX as libc::sa_family_t;
    let is_abstract = name.first() == Some(&0);
    let cap = sun.sun_path.len();
    let max_len = if is_abstract { cap } else { cap - 1 };
    let mut len = name.len();
    if len > max_len {
        len = max_len;
        let line = vm.current_line;
        vm.emit_notice_at(
            &format!(
                "{}(): socket path exceeded the maximum allowed length of {} bytes and was truncated",
                func, max_len
            ),
            line,
        );
    }
    for (i, &b) in name[..len].iter().enumerate() {
        sun.sun_path[i] = b as libc::c_char;
    }
    let off = std::mem::size_of::<libc::sa_family_t>();
    (sun, (off + len) as libc::socklen_t)
}

/// php_network_connect_socket(): returns Ok(()) on success (or a pending
/// asynchronous connect), Err(errno) otherwise.
fn connect_socket(
    fd: i32,
    sa: *const libc::sockaddr,
    sl: libc::socklen_t,
    asynchronous: bool,
    timeout_ms: Option<i32>,
) -> Result<bool, i32> {
    let orig = unsafe { libc::fcntl(fd, libc::F_GETFL, 0) };
    unsafe { libc::fcntl(fd, libc::F_SETFL, orig | libc::O_NONBLOCK) };
    let n = unsafe { libc::connect(fd, sa, sl) };
    let mut error = 0;
    if n != 0 {
        error = errno();
        if error != libc::EINPROGRESS {
            return Err(error);
        }
        if asynchronous {
            return Ok(true);
        }
        let deadline = timeout_ms.map(|ms| {
            std::time::Instant::now() + std::time::Duration::from_millis(ms.max(0) as u64)
        });
        let mut remaining = timeout_ms.unwrap_or(-1);
        loop {
            let r = pollfd_for(fd, POLLREADABLE | libc::POLLOUT, remaining);
            if r < 0 {
                if errno() == libc::EINTR {
                    if let Some(d) = deadline {
                        let now = std::time::Instant::now();
                        if now >= d {
                            error = libc::ETIMEDOUT;
                            break;
                        }
                        remaining = (d - now).as_millis() as i32;
                    }
                    continue;
                }
                error = errno();
            } else if r == 0 {
                error = libc::ETIMEDOUT;
            } else {
                let mut e: i32 = 0;
                let mut len = std::mem::size_of::<i32>() as libc::socklen_t;
                if unsafe {
                    libc::getsockopt(
                        fd,
                        libc::SOL_SOCKET,
                        libc::SO_ERROR,
                        &mut e as *mut i32 as *mut libc::c_void,
                        &mut len,
                    )
                } != 0
                {
                    error = errno();
                } else {
                    error = e;
                }
            }
            break;
        }
    }
    if !asynchronous {
        unsafe { libc::fcntl(fd, libc::F_SETFL, orig) };
    }
    if error != 0 { Err(error) } else { Ok(false) }
}

/// Lookup of a socket context option ("socket" wrapper).
fn ctx_socket_opt(ctx: Option<i64>, name: &[u8]) -> Option<Value> {
    let id = ctx_key(ctx?);
    STREAM_CONTEXTS.with(|c| {
        c.borrow()
            .get(&id)
            .and_then(|ctx| ctx.options.get(b"socket".as_ref()))
            .and_then(|m| m.get(name).cloned())
    })
}

/// php_stream_context_from_zval(): the explicit context, or the default
/// context unless `no_default`.
fn context_from(ctx: Option<&Value>, no_default: bool) -> Option<i64> {
    match ctx.map(|v| v.deref()) {
        Some(Value::Null) | Some(Value::Undef) | None => {
            if no_default {
                None
            } else {
                DEFAULT_STREAM_CTX.with(|d| d.get())
            }
        }
        Some(v) => resolve_context_id(&v),
    }
}

// ---------------------------------------------------------------------------
// Transport creation (_php_stream_xport_create)
// ---------------------------------------------------------------------------

struct XportErr {
    text: Option<String>,
    code: i32,
}

/// The transport's stream was created before its connect / bind failed
/// (and is freed): its resource id is used up.
fn xport_failed(e: XportErr) -> XportErr {
    goro_core::value::alloc_resource_id();
    e
}

fn alloc_socket_stream(fd: i32, sd: SockData, uri: Option<&[u8]>, ctx: Option<i64>) -> i64 {
    let file = unsafe { std::fs::File::from_raw_fd(fd) };
    let fid = alloc_file_handle(file, "r+");
    FILE_HANDLES.with(|h| {
        if let Some(fh) = h.borrow_mut().get_mut(&fid) {
            fh.wrapper_type = String::new();
            fh.stream_type = sd.kind.label().to_string();
            fh.seekable = false;
            // stream->position counts what is read and written.
            fh.pipe_pos = Some(0);
            fh.uri = uri
                .map(|u| String::from_utf8_lossy(u).into_owned())
                .unwrap_or_default();
            fh.sock = Some(sd);
        }
    });
    if let Some(cid) = ctx {
        let is_ctx_resource = goro_core::value::resource_type_of(cid).as_deref()
            == Some(b"stream-context".as_ref());
        if is_ctx_resource {
            // php_stream_context_set(): the stream holds a reference to its
            // context resource and shares it (options set through the
            // stream are seen by the context and every other stream on it).
            STREAM_CTX_REFS.with(|m| m.borrow_mut().insert(fid, Value::resource(cid)));
        } else if let Some(c) = STREAM_CONTEXTS.with(|c| c.borrow().get(&cid).cloned()) {
            // the private context of another stream (an accepted socket)
            STREAM_CONTEXTS.with(|m| m.borrow_mut().insert(fid, c));
        }
    }
    goro_core::value::register_resource(fid, b"stream");
    fid
}

thread_local! {
    /// The context resource each socket stream holds (stream->ctx).
    static STREAM_CTX_REFS: RefCell<StdHashMap<i64, Value>> = RefCell::new(StdHashMap::new());
}

/// The key of the context a stream uses in STREAM_CONTEXTS: the context
/// resource it was created with, else its own (lazily created) one.
pub(crate) fn ctx_key(id: i64) -> i64 {
    STREAM_CTX_REFS.with(|m| m.borrow().get(&id).map(|v| v.to_long()).unwrap_or(id))
}

/// The stream `fid` is freed: it releases its context.
pub(crate) fn release_stream_ctx(fid: i64) {
    let held = STREAM_CTX_REFS.with(|m| m.borrow_mut().remove(&fid));
    drop(held);
}

/// A stream that was neither connected nor bound has no socket
/// (sock->socket == -1); a /dev/null descriptor stands in for it so that
/// socket operations on it fail.
fn unconnected_fd() -> i32 {
    unsafe { libc::open(c"/dev/null".as_ptr(), libc::O_RDWR | libc::O_CLOEXEC) }
}

fn split_transport(name: &[u8]) -> (Vec<u8>, &[u8]) {
    let n = name
        .iter()
        .take_while(|&&c| c.is_ascii_alphanumeric() || c == b'+' || c == b'-' || c == b'.')
        .count();
    if n > 1 && name[n..].starts_with(b"://") {
        (name[..n].to_vec(), &name[n + 3..])
    } else {
        (b"tcp".to_vec(), name)
    }
}

enum XportOp {
    Connect { asynchronous: bool },
    Server { flags: i64 },
    None,
}

#[allow(clippy::too_many_arguments)]
fn xport_create(
    vm: &mut Vm,
    func: &str,
    orig_name: &[u8],
    op: XportOp,
    timeout_ms: Option<i32>,
    ctx: Option<i64>,
) -> Result<i64, XportErr> {
    let (proto, name) = split_transport(orig_name);
    let mut crypto: Option<i64> = None;
    let ssl_loaded = vm.crypto_hooks.is_some();
    let kind = match proto.as_slice() {
        b"tcp" => SockKind::Tcp,
        // php_openssl_ssl_socket_factory()
        b"ssl" | b"tls" if ssl_loaded => {
            crypto = Some(get_crypto_method(ctx, STREAM_CRYPTO_METHOD_TLS_ANY_CLIENT));
            SockKind::Tcp
        }
        b"tlsv1.0" if ssl_loaded => {
            crypto = Some(STREAM_CRYPTO_METHOD_TLSV1_0_CLIENT);
            SockKind::Tcp
        }
        b"tlsv1.1" if ssl_loaded => {
            crypto = Some(STREAM_CRYPTO_METHOD_TLSV1_1_CLIENT);
            SockKind::Tcp
        }
        b"tlsv1.2" if ssl_loaded => {
            crypto = Some(STREAM_CRYPTO_METHOD_TLSV1_2_CLIENT);
            SockKind::Tcp
        }
        b"tlsv1.3" if ssl_loaded => {
            crypto = Some(STREAM_CRYPTO_METHOD_TLSV1_3_CLIENT);
            SockKind::Tcp
        }
        b"sslv2" if ssl_loaded => {
            vm.emit_warning(&format!(
                "{}(): SSLv2 unavailable in this PHP version",
                func
            ));
            return Err(XportErr {
                text: None,
                code: 0,
            });
        }
        b"sslv3" if ssl_loaded => {
            vm.emit_warning(&format!(
                "{}(): SSLv3 support is not compiled into the OpenSSL library against which PHP is linked",
                func
            ));
            return Err(XportErr {
                text: None,
                code: 0,
            });
        }
        b"udp" => SockKind::Udp,
        b"unix" => SockKind::Unix,
        b"udg" => SockKind::Udg,
        _ => {
            let mut w = proto.clone();
            w.truncate(31);
            return Err(XportErr {
                text: Some(format!(
                    "Unable to find the socket transport \"{}\" - did you forget to enable it when you configured PHP?",
                    String::from_utf8_lossy(&w)
                )),
                code: 0,
            });
        }
    };
    let mut sd = SockData::new(kind, default_socket_timeout(vm));
    if let Some(m) = crypto {
        sd.enable_on_connect = true;
        sd.crypto_method = m;
    }
    if kind == SockKind::Tcp && ssl_loaded {
        sd.url_name = url_name(name);
        sd.connect_timeout_ms = timeout_ms.unwrap_or(-1);
    }
    sd.ctx_src = ctx;
    let connecting = matches!(op, XportOp::Connect { .. });
    let fd = match op {
        XportOp::Connect { asynchronous } => {
            let r = if kind.is_unix() {
                connect_unix(vm, func, kind, name, asynchronous, timeout_ms)
            } else {
                connect_ip(vm, func, kind, name, asynchronous, timeout_ms, ctx)
            };
            match r {
                Ok((fd, pending)) => {
                    if asynchronous && pending {
                        sd.is_blocked = false;
                    }
                    fd
                }
                Err(e) => return Err(xport_failed(e)),
            }
        }
        XportOp::Server { flags } if flags & STREAM_XPORT_BIND == 0 => unconnected_fd(),
        XportOp::Server { flags } => {
            let fd = if kind.is_unix() {
                bind_unix(vm, func, kind, name)
            } else {
                bind_ip(vm, func, kind, name, ctx)
            }
            .map_err(xport_failed)?;
            if flags & STREAM_XPORT_LISTEN != 0 {
                let backlog = ctx_socket_opt(ctx, b"backlog")
                    .map(|v| v.to_long() as i32)
                    .unwrap_or(32);
                if unsafe { libc::listen(fd, backlog) } != 0 {
                    unsafe { libc::close(fd) };
                    return Err(xport_failed(XportErr {
                        text: None,
                        code: 0,
                    }));
                }
            }
            sd.no_io = true;
            fd
        }
        XportOp::None => unconnected_fd(),
    };
    let enable = connecting && sd.enable_on_connect;
    let method = sd.crypto_method;
    if enable && !sd.is_blocked {
        // An asynchronous connect leaves the stream flagged blocking in
        // PHP; the handshake then waits for the connection to complete and
        // the descriptor ends up blocking again.
        sd.is_blocked = true;
        set_fd_blocking(fd, true);
    }
    let fid = alloc_socket_stream(fd, sd, Some(orig_name), ctx);
    if enable
        && (crypto_setup(vm, func, fid, method, None) < 0 || crypto_enable(vm, func, fid, true) < 0)
    {
        vm.emit_warning(&format!("{}(): Failed to enable crypto", func));
        FILE_HANDLES.with(|h| h.borrow_mut().remove(&fid));
        STREAM_CONTEXTS.with(|c| c.borrow_mut().remove(&fid));
        goro_core::value::close_resource(fid);
        return Err(XportErr {
            text: None,
            code: 0,
        });
    }
    Ok(fid)
}

fn connect_unix(
    vm: &mut Vm,
    func: &str,
    kind: SockKind,
    name: &[u8],
    asynchronous: bool,
    timeout_ms: Option<i32>,
) -> Result<(i32, bool), XportErr> {
    let fd = unsafe { libc::socket(libc::PF_UNIX, kind.socktype(), 0) };
    if fd < 0 {
        return Err(XportErr {
            text: Some("Failed to create unix socket".to_string()),
            code: 0,
        });
    }
    let (sun, sl) = parse_unix_address(vm, func, name);
    match connect_socket(
        fd,
        &sun as *const _ as *const libc::sockaddr,
        sl,
        asynchronous,
        timeout_ms,
    ) {
        Ok(p) => Ok((fd, p)),
        Err(e) => {
            unsafe { libc::close(fd) };
            Err(XportErr {
                text: Some(strerror(e)),
                code: e,
            })
        }
    }
}

fn connect_ip(
    vm: &mut Vm,
    func: &str,
    kind: SockKind,
    name: &[u8],
    asynchronous: bool,
    timeout_ms: Option<i32>,
    ctx: Option<i64>,
) -> Result<(i32, bool), XportErr> {
    let (host, port) = parse_ip_address(name).map_err(|t| XportErr {
        text: Some(t),
        code: 0,
    })?;
    let mut bindto: Option<(Vec<u8>, i32)> = None;
    if let Some(v) = ctx_socket_opt(ctx, b"bindto") {
        match v.deref() {
            // A bindto address that does not parse is ignored.
            Value::String(s) => bindto = parse_ip_address(s.as_bytes()).ok(),
            _ => {
                return Err(XportErr {
                    text: Some("local_addr context option is not a string.".to_string()),
                    code: 0,
                });
            }
        }
    }
    let broadcast = kind == SockKind::Udp
        && ctx_socket_opt(ctx, b"so_broadcast")
            .map(|v| v.is_truthy())
            .unwrap_or(false);
    let nodelay = kind == SockKind::Tcp
        && ctx_socket_opt(ctx, b"tcp_nodelay")
            .map(|v| v.is_truthy())
            .unwrap_or(false);
    // php_network_connect_socket_to_host()
    let addrs = getaddresses(vm, func, &host, kind.socktype()).map_err(|t| XportErr {
        text: if t.is_empty() { None } else { Some(t) },
        code: 0,
    })?;
    let deadline = timeout_ms
        .filter(|&ms| ms >= 0)
        .map(|ms| std::time::Instant::now() + std::time::Duration::from_millis(ms as u64));
    let mut working = timeout_ms;
    let mut last = XportErr {
        text: None,
        code: 0,
    };
    for (mut sa, _) in addrs {
        let family = sa.ss_family as i32;
        if family == libc::AF_INET6 {
            if let Some((b, _)) = &bindto {
                if !b.contains(&b':') {
                    continue;
                }
            }
        } else if family == libc::AF_INET {
            if let Some((b, _)) = &bindto {
                if b.contains(&b':') || b.as_slice() == b"0" {
                    bindto = None;
                }
            }
        }
        let Some(sl) = set_port(&mut sa, port as u16) else {
            continue;
        };
        let fd = unsafe { libc::socket(family, kind.socktype(), 0) };
        if fd < 0 {
            continue;
        }
        if let Some((b, bport)) = &bindto {
            let ok = bind_local(fd, family, b, *bport as u16);
            match ok {
                None => vm.emit_warning(&format!(
                    "{}(): Invalid IP Address: {}",
                    func,
                    goro_core::string::msg_str(b)
                )),
                Some(Err(e)) => vm.emit_warning(&format!(
                    "{}(): Failed to bind to '{}:{}', system said: {}",
                    func,
                    goro_core::string::msg_str(b),
                    bport,
                    strerror(e)
                )),
                Some(Ok(())) => {}
            }
        }
        if broadcast {
            setsockopt_int(fd, libc::SOL_SOCKET, libc::SO_BROADCAST, 1);
        }
        if nodelay {
            setsockopt_int(fd, libc::IPPROTO_TCP, libc::TCP_NODELAY, 1);
        }
        match connect_socket(
            fd,
            &sa as *const _ as *const libc::sockaddr,
            sl,
            asynchronous,
            working,
        ) {
            Ok(p) => return Ok((fd, p)),
            Err(e) => {
                last = XportErr {
                    text: Some(strerror(e)),
                    code: e,
                };
                unsafe { libc::close(fd) };
                if let Some(d) = deadline {
                    let now = std::time::Instant::now();
                    if now >= d {
                        break;
                    }
                    working = Some((d - now).as_millis() as i32);
                }
            }
        }
    }
    Err(last)
}

/// Bind a client socket to the "bindto" address; None for an invalid IP.
fn bind_local(fd: i32, family: i32, addr: &[u8], port: u16) -> Option<Result<(), i32>> {
    let s = std::str::from_utf8(addr).ok()?;
    let mut ss: libc::sockaddr_storage = unsafe { std::mem::zeroed() };
    let len = if family == libc::AF_INET {
        let ip: std::net::Ipv4Addr = s.parse().ok()?;
        let sin = unsafe { &mut *(&mut ss as *mut _ as *mut libc::sockaddr_in) };
        sin.sin_family = libc::AF_INET as libc::sa_family_t;
        sin.sin_port = port.to_be();
        sin.sin_addr.s_addr = u32::from(ip).to_be();
        std::mem::size_of::<libc::sockaddr_in>()
    } else {
        let ip: std::net::Ipv6Addr = s.parse().ok()?;
        let sin6 = unsafe { &mut *(&mut ss as *mut _ as *mut libc::sockaddr_in6) };
        sin6.sin6_family = libc::AF_INET6 as libc::sa_family_t;
        sin6.sin6_port = port.to_be();
        sin6.sin6_addr.s6_addr = ip.octets();
        std::mem::size_of::<libc::sockaddr_in6>()
    };
    setsockopt_int(fd, libc::SOL_IP, libc::IP_BIND_ADDRESS_NO_PORT, 1);
    let r = unsafe {
        libc::bind(
            fd,
            &ss as *const _ as *const libc::sockaddr,
            len as libc::socklen_t,
        )
    };
    Some(if r != 0 { Err(errno()) } else { Ok(()) })
}

fn bind_unix(vm: &mut Vm, func: &str, kind: SockKind, name: &[u8]) -> Result<i32, XportErr> {
    let fd = unsafe { libc::socket(libc::PF_UNIX, kind.socktype(), 0) };
    if fd < 0 {
        let e = errno();
        return Err(XportErr {
            text: Some(format!(
                "Failed to create unix{} socket {}",
                if kind == SockKind::Unix {
                    ""
                } else {
                    "datagram"
                },
                strerror(e)
            )),
            code: 0,
        });
    }
    let (sun, sl) = parse_unix_address(vm, func, name);
    if unsafe { libc::bind(fd, &sun as *const _ as *const libc::sockaddr, sl) } != 0 {
        unsafe { libc::close(fd) };
        // php_tcp_sockop_bind() reports no error text for unix sockets.
        return Err(XportErr {
            text: None,
            code: 0,
        });
    }
    Ok(fd)
}

fn bind_ip(
    vm: &mut Vm,
    func: &str,
    kind: SockKind,
    name: &[u8],
    ctx: Option<i64>,
) -> Result<i32, XportErr> {
    let (host, port) = parse_ip_address(name).map_err(|t| XportErr {
        text: Some(t),
        code: 0,
    })?;
    let v6only = ctx_socket_opt(ctx, b"ipv6_v6only")
        .filter(|v| !matches!(v.deref(), Value::Null))
        .map(|v| v.is_truthy());
    let reuseport = ctx_socket_opt(ctx, b"so_reuseport")
        .map(|v| v.is_truthy())
        .unwrap_or(false);
    let broadcast = kind == SockKind::Udp
        && ctx_socket_opt(ctx, b"so_broadcast")
            .map(|v| v.is_truthy())
            .unwrap_or(false);
    let addrs = getaddresses(vm, func, &host, kind.socktype()).map_err(|t| XportErr {
        text: if t.is_empty() { None } else { Some(t) },
        code: 0,
    })?;
    let mut err = 0;
    for (mut sa, _) in addrs {
        let Some(sl) = set_port(&mut sa, port as u16) else {
            continue;
        };
        let family = sa.ss_family as i32;
        let fd = unsafe { libc::socket(family, kind.socktype(), 0) };
        if fd < 0 {
            continue;
        }
        setsockopt_int(fd, libc::SOL_SOCKET, libc::SO_REUSEADDR, 1);
        if let Some(on) = v6only {
            setsockopt_int(fd, libc::IPPROTO_IPV6, libc::IPV6_V6ONLY, on as i32);
        }
        if reuseport {
            setsockopt_int(fd, libc::SOL_SOCKET, libc::SO_REUSEPORT, 1);
        }
        if broadcast {
            setsockopt_int(fd, libc::SOL_SOCKET, libc::SO_BROADCAST, 1);
        }
        if unsafe { libc::bind(fd, &sa as *const _ as *const libc::sockaddr, sl) } == 0 {
            return Ok(fd);
        }
        err = errno();
        unsafe { libc::close(fd) };
    }
    Err(XportErr {
        text: Some(strerror(err)),
        code: 0,
    })
}

// ---------------------------------------------------------------------------
// Stream operations (php_sockop_*)
// ---------------------------------------------------------------------------

/// php_sockop_read(). Returns the bytes read; Ok(0) at EOF;
/// Err(WouldBlock) for "no data right now" and Err(TimedOut) when a
/// blocking read timed out (the stream read fails).
pub(super) fn sock_read(
    fh: &mut FileHandle,
    buf: &mut [u8],
    has_buffered: bool,
) -> std::io::Result<usize> {
    if let Some(r) = crypto_read(fh, buf, has_buffered) {
        return r;
    }
    let fd = fh.file.as_raw_fd();
    let Some(sd) = fh.sock.as_mut() else {
        return fh.file.read(buf);
    };
    let mut flags = 0;
    if sd.is_blocked {
        let dont_wait = has_buffered || (sd.tv_sec == 0 && sd.tv_usec == 0);
        if dont_wait || sd.tv_sec != -1 {
            flags = libc::MSG_DONTWAIT;
        }
        if !dont_wait {
            // php_sock_stream_wait_for_data()
            sd.timeout_event = false;
            let ms = sd.timeout_ms();
            loop {
                let r = pollfd_for(fd, POLLREADABLE, ms);
                if r == 0 {
                    sd.timeout_event = true;
                }
                if r >= 0 || errno() != libc::EINTR {
                    break;
                }
            }
            if sd.timeout_event {
                return Err(std::io::ErrorKind::TimedOut.into());
            }
        }
    }
    let n = unsafe { libc::recv(fd, buf.as_mut_ptr() as *mut libc::c_void, buf.len(), flags) };
    if n < 0 {
        let e = errno();
        if e == libc::EAGAIN || e == libc::EWOULDBLOCK {
            return Err(std::io::ErrorKind::WouldBlock.into());
        }
        fh.eof = true;
        return Err(std::io::Error::from_raw_os_error(e));
    }
    if n == 0 {
        fh.eof = true;
    }
    Ok(n as usize)
}

/// php_sockop_write() for one chunk; returns the bytes written, 0 when
/// a non-blocking socket would block, or -1 (with a notice) on failure.
fn sock_write_once(vm: &mut Vm, func: &str, fid: i64, data: &[u8]) -> isize {
    if let Some(n) = crypto_write(vm, func, fid, data) {
        return n;
    }
    let Some((sd, fd)) = sock_of(fid) else {
        return -1;
    };
    let has_timeout = sd.tv_sec != -1;
    let flags = libc::MSG_NOSIGNAL
        | if sd.is_blocked && has_timeout {
            libc::MSG_DONTWAIT
        } else {
            0
        };
    loop {
        let n = unsafe { libc::send(fd, data.as_ptr() as *const libc::c_void, data.len(), flags) };
        if n > 0 {
            return n;
        }
        let mut err = errno();
        if err == libc::EAGAIN || err == libc::EWOULDBLOCK {
            if sd.is_blocked {
                with_sock(fid, |fh| {
                    if let Some(s) = fh.sock.as_mut() {
                        s.timeout_event = false;
                    }
                });
                let ms = sd.timeout_ms();
                let mut retry = false;
                loop {
                    let r = pollfd_for(fd, libc::POLLOUT, ms);
                    if r == 0 {
                        with_sock(fid, |fh| {
                            if let Some(s) = fh.sock.as_mut() {
                                s.timeout_event = true;
                            }
                        });
                        break;
                    }
                    if r > 0 {
                        retry = true;
                        break;
                    }
                    err = errno();
                    if err != libc::EINTR {
                        break;
                    }
                }
                if retry {
                    continue;
                }
            } else {
                return 0;
            }
        }
        let line = vm.current_line;
        vm.emit_notice_at(
            &format!(
                "{}(): Send of {} bytes failed with errno={} {}",
                func,
                data.len(),
                err,
                strerror(err)
            ),
            line,
        );
        return n;
    }
}

/// _php_stream_write_buffer() on a socket stream.
pub(crate) fn sock_write(vm: &mut Vm, func: &str, fid: i64, data: &[u8]) -> isize {
    let mut done = 0usize;
    while done < data.len() {
        let n = sock_write_once(vm, func, fid, &data[done..]);
        if n <= 0 {
            return if done == 0 { n } else { done as isize };
        }
        // php_sockop_write(): a progress notification; the position
        // advances.
        if let Some(ctx) = stream_ctx_src(fid) {
            notify_progress_increment(Some(ctx), n as i64);
        }
        with_sock(fid, |fh| {
            if let Some(p) = fh.pipe_pos.as_mut() {
                *p += n as i64;
            }
        });
        done += n as usize;
    }
    done as isize
}

pub(crate) fn is_socket(fid: i64) -> bool {
    FILE_HANDLES.with(|h| {
        h.borrow()
            .get(&fid)
            .map(|fh| fh.sock.is_some())
            .unwrap_or(false)
    })
}

/// PHP_STREAM_OPTION_CHECK_LIVENESS with value 0 (_php_stream_eof()).
pub(super) fn check_liveness(fh: &mut FileHandle) {
    let fd = fh.file.as_raw_fd();
    if let Some(c) = fh.crypto.as_mut().filter(|c| c.is_active()) {
        if !c.alive(fd) {
            fh.eof = true;
        }
        queue_crypto_output(c, fd);
        return;
    }
    let Some(sd) = fh.sock.as_ref() else {
        return;
    };
    let mut alive = true;
    let ready = if !sd.no_io {
        true
    } else {
        pollfd_for(fd, POLLREADABLE | libc::POLLPRI, 0) > 0
    };
    if ready {
        let mut b = 0u8;
        let r = unsafe {
            libc::recv(
                fd,
                &mut b as *mut u8 as *mut libc::c_void,
                1,
                libc::MSG_PEEK | libc::MSG_DONTWAIT,
            )
        };
        if r == 0 {
            alive = false;
        } else if r < 0 {
            let e = errno();
            if e != libc::EWOULDBLOCK && e != libc::EMSGSIZE && e != libc::EAGAIN {
                alive = false;
            }
        }
    }
    if !alive {
        fh.eof = true;
    }
}

/// Metadata of a socket stream for stream_get_meta_data():
/// (timed_out, blocked).
pub(super) fn meta(fh: &FileHandle) -> Option<(bool, bool)> {
    fh.sock.as_ref().map(|s| (s.timeout_event, s.is_blocked))
}

/// PHP_STREAM_OPTION_BLOCKING on a descriptor-backed stream; returns
/// false when the stream does not support it (php_stream_set_option() == -1).
pub(crate) fn set_blocking(fid: i64, block: bool) -> bool {
    if let Some(r) = with_sock(fid, |fh| {
        let fd = fh.file.as_raw_fd();
        match fh.sock.as_mut() {
            Some(sd) => {
                if set_fd_blocking(fd, block) {
                    sd.is_blocked = block;
                    Some(true)
                } else {
                    Some(false)
                }
            }
            None => match fh.special {
                SpecialStream::Memory | SpecialStream::Temp => Some(true),
                _ => Some(set_fd_blocking(fd, block)),
            },
        }
    }) {
        return r.unwrap_or(true);
    }
    match fid {
        1..=3 => set_fd_blocking((fid - 1) as i32, block),
        _ => true,
    }
}

// ---------------------------------------------------------------------------
// PHP functions
// ---------------------------------------------------------------------------

fn finish_open_error(
    vm: &mut Vm,
    errno_ref: Option<&Value>,
    errstr_ref: Option<&Value>,
    e: &XportErr,
) {
    set_ref(errno_ref, Value::Long(e.code as i64));
    if let Some(t) = &e.text {
        set_ref(errstr_ref, str_val(t));
    }
    let _ = vm;
}

/// timeout (seconds, float) -> poll milliseconds, None meaning "no limit".
fn timeout_from_double(t: f64) -> Option<i32> {
    if t < 0.0 || t >= PHP_TIMEOUT_ULL_MAX / 1_000_000.0 {
        None
    } else {
        let conv = (t * 1_000_000.0) as u64;
        Some(tv_to_ms(
            (conv / 1_000_000) as i64,
            (conv % 1_000_000) as i64,
        ))
    }
}

thread_local! {
    /// Persistent sockets (pfsockopen(), STREAM_CLIENT_PERSISTENT): hashkey -> stream id.
    static PERSISTENT: RefCell<StdHashMap<Vec<u8>, i64>> = RefCell::new(StdHashMap::new());
}

fn persistent_lookup(key: &[u8]) -> Option<i64> {
    let fid = PERSISTENT.with(|p| p.borrow().get(key).copied())?;
    let alive = with_sock(fid, |fh| {
        check_liveness(fh);
        !fh.eof
    });
    PERSISTENT.with(|p| p.borrow_mut().remove(key));
    match alive {
        Some(true) => {
            PERSISTENT.with(|p| p.borrow_mut().insert(key.to_vec(), fid));
            Some(fid)
        }
        Some(false) => {
            // "dead - kill it"
            FILE_HANDLES.with(|h| h.borrow_mut().remove(&fid));
            goro_core::value::close_resource(fid);
            None
        }
        None => None,
    }
}

fn persistent_store(key: Vec<u8>, fid: i64) {
    goro_core::value::register_resource(fid, b"persistent stream");
    PERSISTENT.with(|p| p.borrow_mut().insert(key, fid));
}

/// PHP_FUNCTION(stream_socket_client)
pub(crate) fn stream_socket_client(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    const F: &str = "stream_socket_client";
    check_arity(vm, F, args, 1, 6)?;
    let host = zpp_string(vm, F, 1, "address", &args[0])?;
    let timeout = match args.get(3) {
        Some(v) => zpp_double(vm, F, 4, "timeout", v, true)?,
        None => None,
    };
    let flags = match args.get(4) {
        Some(v) => zpp_long(vm, F, 5, "flags", v, false)?.unwrap_or(0),
        None => PHP_STREAM_CLIENT_CONNECT,
    };
    let ctx_arg = args.get(5);
    if let Some(c) = ctx_arg {
        if !matches!(c.deref(), Value::Null | Value::Resource(_)) {
            return Err(type_error(vm, F, 6, "context", "resource or null", c));
        }
    }
    let timeout = match timeout {
        None => default_socket_timeout(vm) as f64,
        Some(t) if !t.is_finite() => {
            return Err(throw(
                vm,
                b"ValueError",
                "stream_socket_client(): Argument #4 ($timeout) must be a finite value",
            ));
        }
        Some(t) => t,
    };
    let ctx = context_from(ctx_arg, false);
    set_ref(args.get(1), Value::Long(0));
    set_ref(args.get(2), str_val(""));
    let hashkey = if flags & PHP_STREAM_CLIENT_PERSISTENT != 0 {
        let mut k = b"stream_socket_client__".to_vec();
        k.extend_from_slice(&host);
        Some(k)
    } else {
        None
    };
    if let Some(k) = &hashkey {
        if let Some(fid) = persistent_lookup(k) {
            return Ok(Value::resource(fid));
        }
    }
    let op = if flags & (PHP_STREAM_CLIENT_CONNECT | PHP_STREAM_CLIENT_ASYNC_CONNECT) != 0 {
        XportOp::Connect {
            asynchronous: flags & PHP_STREAM_CLIENT_ASYNC_CONNECT != 0,
        }
    } else {
        XportOp::None
    };
    match xport_create(vm, F, &host, op, timeout_from_double(timeout), ctx) {
        Ok(fid) => {
            if let Some(k) = hashkey {
                persistent_store(k, fid);
            }
            Ok(Value::resource(fid))
        }
        Err(e) => {
            let quoted = php_addslashes(&host);
            vm.emit_warning(&format!(
                "{}(): Unable to connect to {} ({})",
                F,
                goro_core::string::msg_str(&quoted),
                e.text.as_deref().unwrap_or("Unknown error")
            ));
            finish_open_error(vm, args.get(1), args.get(2), &e);
            Ok(Value::False)
        }
    }
}

/// php_addslashes()
fn php_addslashes(s: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    for &c in s {
        match c {
            0 => out.extend_from_slice(b"\\0"),
            b'\'' | b'"' | b'\\' => {
                out.push(b'\\');
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    out
}

/// PHP_FUNCTION(stream_socket_server)
pub(crate) fn stream_socket_server(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    const F: &str = "stream_socket_server";
    check_arity(vm, F, args, 1, 5)?;
    let host = zpp_string(vm, F, 1, "address", &args[0])?;
    let flags = match args.get(3) {
        Some(v) => zpp_long(vm, F, 4, "flags", v, false)?.unwrap_or(0),
        None => STREAM_XPORT_BIND | STREAM_XPORT_LISTEN,
    };
    let ctx_arg = args.get(4);
    if let Some(c) = ctx_arg {
        if !matches!(c.deref(), Value::Null | Value::Resource(_)) {
            return Err(type_error(vm, F, 5, "context", "resource or null", c));
        }
    }
    let ctx = context_from(ctx_arg, false);
    set_ref(args.get(1), Value::Long(0));
    set_ref(args.get(2), str_val(""));
    match xport_create(vm, F, &host, XportOp::Server { flags }, None, ctx) {
        Ok(fid) => Ok(Value::resource(fid)),
        Err(e) => {
            // The host is printed as a C string.
            let shown: Vec<u8> = host.iter().take_while(|&&c| c != 0).copied().collect();
            vm.emit_warning(&format!(
                "{}(): Unable to connect to {} ({})",
                F,
                goro_core::string::msg_str(&shown),
                e.text.as_deref().unwrap_or("Unknown error")
            ));
            finish_open_error(vm, args.get(1), args.get(2), &e);
            Ok(Value::False)
        }
    }
}

/// PHP_FUNCTION(stream_socket_accept)
pub(crate) fn stream_socket_accept(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    const F: &str = "stream_socket_accept";
    check_arity(vm, F, args, 1, 3)?;
    let fid = require_open_stream_arg(vm, &args[0], F, 1, "socket")?;
    let timeout = match args.get(1) {
        Some(v) => zpp_double(vm, F, 2, "timeout", v, true)?,
        None => None,
    };
    let timeout = match timeout {
        None => default_socket_timeout(vm) as f64,
        Some(t) if !t.is_finite() => {
            return Err(throw(
                vm,
                b"ValueError",
                "stream_socket_accept(): Argument #2 ($timeout) must be a finite value",
            ));
        }
        Some(t) => t,
    };
    let Some((sd, srv)) = sock_of(fid) else {
        vm.emit_warning(&format!("{}(): Accept failed: Unknown error", F));
        return Ok(Value::False);
    };
    let ms = timeout_from_double(timeout).unwrap_or(-1);
    super::flush_cli_output(vm);
    let n = pollfd_for(srv, POLLREADABLE, ms);
    let error;
    let mut cli = -1;
    let mut peer = None;
    if n == 0 {
        error = libc::ETIMEDOUT;
    } else if n < 0 {
        error = errno();
    } else {
        let mut ss: libc::sockaddr_storage = unsafe { std::mem::zeroed() };
        let mut sl = std::mem::size_of::<libc::sockaddr_storage>() as libc::socklen_t;
        cli = unsafe { libc::accept(srv, &mut ss as *mut _ as *mut libc::sockaddr, &mut sl) };
        if cli >= 0 {
            peer = sockaddr_to_text(&ss, sl);
            error = 0;
            let ctx = resolve_context_id(&args[0]);
            if ctx_socket_opt(ctx, b"tcp_nodelay")
                .map(|v| v.is_truthy())
                .unwrap_or(false)
            {
                setsockopt_int(cli, libc::IPPROTO_TCP, libc::TCP_NODELAY, 1);
            }
        } else {
            error = errno();
        }
    }
    if cli < 0 {
        vm.emit_warning(&format!("{}(): Accept failed: {}", F, strerror(error)));
        return Ok(Value::False);
    }
    let mut csd = sd.clone();
    csd.is_blocked = true;
    csd.no_io = false;
    csd.timeout_event = false;
    // php_tcp_sockop_accept(): the client stream gets the server's context
    let key = ctx_key(fid);
    let ctx = STREAM_CONTEXTS
        .with(|c| c.borrow().contains_key(&key))
        .then_some(key);
    let enable = csd.enable_on_connect;
    let method = csd.crypto_method & !STREAM_CRYPTO_IS_CLIENT;
    csd.enable_on_connect = false;
    csd.url_name = None;
    csd.connect_timeout_ms = 0;
    let cfid = alloc_socket_stream(cli, csd, None, ctx);
    if enable {
        if crypto_setup(vm, F, cfid, method, None) < 0 || crypto_enable(vm, F, cfid, true) < 0 {
            vm.emit_warning(&format!("{}(): Failed to enable crypto", F));
            FILE_HANDLES.with(|h| h.borrow_mut().remove(&cfid));
            STREAM_CONTEXTS.with(|c| c.borrow_mut().remove(&cfid));
            goro_core::value::close_resource(cfid);
            vm.emit_warning(&format!("{}(): Accept failed: Cannot enable crypto", F));
            return Ok(Value::False);
        }
    }
    if let (Some(p), Some(r)) = (peer, args.get(2)) {
        set_ref(Some(r), Value::String(PhpString::from_vec(p)));
    }
    Ok(Value::resource(cfid))
}

/// PHP_FUNCTION(stream_socket_get_name)
pub(crate) fn stream_socket_get_name(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    const F: &str = "stream_socket_get_name";
    check_arity(vm, F, args, 2, 2)?;
    if crate::misc::is_dir_handle(&args[0]) {
        zpp_bool(vm, F, 2, "remote", &args[1])?;
        return Ok(Value::False);
    }
    let fid = require_open_stream_arg(vm, &args[0], F, 1, "socket")?;
    let want_peer = zpp_bool(vm, F, 2, "remote", &args[1])?;
    let Some((_, fd)) = sock_of(fid) else {
        return Ok(Value::False);
    };
    let mut ss: libc::sockaddr_storage = unsafe { std::mem::zeroed() };
    let mut sl = std::mem::size_of::<libc::sockaddr_storage>() as libc::socklen_t;
    let r = unsafe {
        if want_peer {
            libc::getpeername(fd, &mut ss as *mut _ as *mut libc::sockaddr, &mut sl)
        } else {
            libc::getsockname(fd, &mut ss as *mut _ as *mut libc::sockaddr, &mut sl)
        }
    };
    if r != 0 {
        return Ok(Value::False);
    }
    match sockaddr_to_text(&ss, sl) {
        Some(t) if !t.is_empty() && t[0] != 0 => Ok(Value::String(PhpString::from_vec(t))),
        _ => Ok(Value::False),
    }
}

/// PHP_FUNCTION(stream_socket_pair)
pub(crate) fn stream_socket_pair(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    const F: &str = "stream_socket_pair";
    check_arity(vm, F, args, 3, 3)?;
    let domain = zpp_long(vm, F, 1, "domain", &args[0], false)?.unwrap_or(0);
    let ty = zpp_long(vm, F, 2, "type", &args[1], false)?.unwrap_or(0);
    let proto = zpp_long(vm, F, 3, "protocol", &args[2], false)?.unwrap_or(0);
    let mut fds = [0i32; 2];
    if unsafe { libc::socketpair(domain as i32, ty as i32, proto as i32, fds.as_mut_ptr()) } != 0 {
        let e = errno();
        vm.emit_warning(&format!(
            "{}(): Failed to create sockets: [{}]: {}",
            F,
            e,
            strerror(e)
        ));
        return Ok(Value::False);
    }
    let a = stream_from_socket(vm, fds[0]);
    let b = stream_from_socket(vm, fds[1]);
    let mut arr = PhpArray::new();
    arr.push(Value::resource(a));
    arr.push(Value::resource(b));
    Ok(Value::Array(Rc::new(RefCell::new(arr))))
}

/// php_stream_sock_open_from_socket(): a generic socket stream.
pub(crate) fn stream_from_socket(vm: &Vm, fd: i32) -> i64 {
    let t = default_socket_timeout(vm);
    alloc_socket_stream(fd, SockData::new(SockKind::Generic, t), None, None)
}

/// php_stream_url_wrap_php() for php://stdin, php://stdout or php://stderr
/// whose descriptor is a socket: php_stream_sock_open_from_socket() with
/// php_stream_socket_ops.
pub(crate) fn stdio_stream_from_socket(vm: &Vm, fd: i32, uri: &str) -> i64 {
    let fid = stream_from_socket(vm, fd);
    FILE_HANDLES.with(|h| {
        if let Some(fh) = h.borrow_mut().get_mut(&fid) {
            fh.wrapper_type = "PHP".to_string();
            fh.stream_type = "tcp_socket".to_string();
            fh.uri = uri.to_string();
        }
    });
    fid
}

/// php_network_parse_network_address_with_port()
fn parse_network_address(vm: &mut Vm, func: &str, addr: &[u8]) -> Option<SockAddr> {
    let (host, port) = if addr.first() == Some(&b'[') {
        let close = addr[1..].iter().position(|&c| c == b']')? + 1;
        if addr.get(close + 1) != Some(&b':') {
            return None;
        }
        (&addr[1..close], atoi(&addr[close + 2..]) as i16 as u16)
    } else {
        let colon = addr.iter().position(|&c| c == b':')?;
        (&addr[..colon], atoi(&addr[colon + 1..]) as i16 as u16)
    };
    let hs = String::from_utf8_lossy(host).into_owned();
    let mut ss: libc::sockaddr_storage = unsafe { std::mem::zeroed() };
    if let Ok(ip) = hs.parse::<std::net::Ipv6Addr>() {
        let sin6 = unsafe { &mut *(&mut ss as *mut _ as *mut libc::sockaddr_in6) };
        sin6.sin6_family = libc::AF_INET6 as libc::sa_family_t;
        sin6.sin6_port = port.to_be();
        sin6.sin6_addr.s6_addr = ip.octets();
        return Some((
            ss,
            std::mem::size_of::<libc::sockaddr_in6>() as libc::socklen_t,
        ));
    }
    if let Ok(ip) = hs.parse::<std::net::Ipv4Addr>() {
        let sin = unsafe { &mut *(&mut ss as *mut _ as *mut libc::sockaddr_in) };
        sin.sin_family = libc::AF_INET as libc::sa_family_t;
        sin.sin_port = port.to_be();
        sin.sin_addr.s_addr = u32::from(ip).to_be();
        return Some((
            ss,
            std::mem::size_of::<libc::sockaddr_in>() as libc::socklen_t,
        ));
    }
    match getaddresses(vm, func, host, libc::SOCK_DGRAM) {
        Ok(list) => {
            let (mut sa, _) = list.into_iter().next()?;
            let sl = set_port(&mut sa, port)?;
            Some((sa, sl))
        }
        Err(e) => {
            if !e.is_empty() {
                vm.emit_warning(&format!("{}(): Failed to resolve `{}': {}", func, hs, e));
            }
            None
        }
    }
}

/// PHP_FUNCTION(stream_socket_sendto)
pub(crate) fn stream_socket_sendto(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    const F: &str = "stream_socket_sendto";
    check_arity(vm, F, args, 2, 4)?;
    let fid = require_open_stream_arg(vm, &args[0], F, 1, "socket")?;
    let data = zpp_string(vm, F, 2, "data", &args[1])?;
    let flags = match args.get(2) {
        Some(v) => zpp_long(vm, F, 3, "flags", v, false)?.unwrap_or(0),
        None => 0,
    };
    let target = match args.get(3) {
        Some(v) => zpp_string(vm, F, 4, "address", v)?,
        None => Vec::new(),
    };
    let mut addr = None;
    if !target.is_empty() {
        match parse_network_address(vm, F, &target) {
            Some(a) => addr = Some(a),
            None => {
                vm.emit_warning(&format!(
                    "{}(): Failed to parse `{}' into a valid network address",
                    F,
                    goro_core::string::msg_str(
                        target.split(|&c| c == 0).next().unwrap_or_default()
                    )
                ));
                return Ok(Value::False);
            }
        }
    }
    let oob = flags & STREAM_OOB != 0;
    if (oob || addr.is_some())
        && crate::stream_filters::chain_has_direction(
            fid,
            crate::stream_filters::STREAM_FILTER_WRITE,
        )
    {
        vm.emit_warning(&format!(
            "{}(): Cannot write OOB data, or data to a targeted address on a filtered stream",
            F
        ));
        return Ok(Value::Long(-1));
    }
    let Some((_, fd)) = sock_of(fid) else {
        return Ok(Value::Long(-1));
    };
    let sflags = libc::MSG_NOSIGNAL | if oob { libc::MSG_OOB } else { 0 };
    let r = unsafe {
        match &addr {
            Some((ss, sl)) => libc::sendto(
                fd,
                data.as_ptr() as *const libc::c_void,
                data.len(),
                sflags,
                ss as *const _ as *const libc::sockaddr,
                *sl,
            ),
            None => libc::send(fd, data.as_ptr() as *const libc::c_void, data.len(), sflags),
        }
    };
    if r < 0 {
        let e = errno();
        vm.emit_warning(&format!("{}(): {}\n", F, strerror(e)));
        return Ok(Value::Long(-1));
    }
    Ok(Value::Long(r as i64))
}

/// PHP_FUNCTION(stream_socket_recvfrom)
pub(crate) fn stream_socket_recvfrom(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    const F: &str = "stream_socket_recvfrom";
    check_arity(vm, F, args, 2, 4)?;
    let fid = require_open_stream_arg(vm, &args[0], F, 1, "socket")?;
    let len = zpp_long(vm, F, 2, "length", &args[1], false)?.unwrap_or(0);
    let flags = match args.get(2) {
        Some(v) => zpp_long(vm, F, 3, "flags", v, false)?.unwrap_or(0),
        None => 0,
    };
    let want_addr = args.len() > 3;
    if want_addr {
        set_ref(args.get(3), Value::Null);
    }
    if len <= 0 {
        return Err(throw(
            vm,
            b"ValueError",
            "stream_socket_recvfrom(): Argument #2 ($length) must be greater than 0",
        ));
    }
    let Some((_, fd)) = sock_of(fid) else {
        return Ok(Value::False);
    };
    let mut rflags = 0;
    if flags & STREAM_OOB != 0 {
        rflags |= libc::MSG_OOB;
    }
    if flags & STREAM_PEEK != 0 {
        rflags |= libc::MSG_PEEK;
    }
    let mut buf = vec![0u8; len as usize];
    let mut ss: libc::sockaddr_storage = unsafe { std::mem::zeroed() };
    let mut sl = std::mem::size_of::<libc::sockaddr_storage>() as libc::socklen_t;
    let r = unsafe {
        if want_addr {
            libc::recvfrom(
                fd,
                buf.as_mut_ptr() as *mut libc::c_void,
                buf.len(),
                rflags,
                &mut ss as *mut _ as *mut libc::sockaddr,
                &mut sl,
            )
        } else {
            libc::recv(fd, buf.as_mut_ptr() as *mut libc::c_void, buf.len(), rflags)
        }
    };
    if r < 0 {
        return Ok(Value::False);
    }
    if want_addr {
        let text = if sl > 0 {
            sockaddr_to_text(&ss, sl).unwrap_or_default()
        } else {
            Vec::new()
        };
        // An unconnected peer (stream sockets) still yields its address.
        if sl > 0 && sockaddr_to_text(&ss, sl).is_some() || sl == 0 {
            set_ref(args.get(3), Value::String(PhpString::from_vec(text)));
        }
    }
    buf.truncate(r as usize);
    Ok(Value::String(PhpString::from_vec(buf)))
}

/// PHP_FUNCTION(stream_socket_shutdown)
pub(crate) fn stream_socket_shutdown(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    const F: &str = "stream_socket_shutdown";
    check_arity(vm, F, args, 2, 2)?;
    if crate::misc::is_dir_handle(&args[0]) {
        return Ok(Value::False);
    }
    let fid = require_open_stream(vm, &args[0], F)?;
    let how = zpp_long(vm, F, 2, "mode", &args[1], false)?.unwrap_or(0);
    if !(0..=2).contains(&how) {
        return Err(throw(
            vm,
            b"ValueError",
            "stream_socket_shutdown(): Argument #2 ($mode) must be one of STREAM_SHUT_RD, STREAM_SHUT_WR, or STREAM_SHUT_RDWR",
        ));
    }
    let Some((_, fd)) = sock_of(fid) else {
        return Ok(Value::False);
    };
    let how = [libc::SHUT_RD, libc::SHUT_WR, libc::SHUT_RDWR][how as usize];
    Ok(if unsafe { libc::shutdown(fd, how) } == 0 {
        Value::True
    } else {
        Value::False
    })
}

/// php_fsockopen_stream()
pub(crate) fn fsockopen(vm: &mut Vm, args: &[Value], persistent: bool) -> Result<Value, VmError> {
    let f = if persistent {
        "pfsockopen"
    } else {
        "fsockopen"
    };
    check_arity(vm, f, args, 1, 5)?;
    let host = zpp_string(vm, f, 1, "hostname", &args[0])?;
    let port = match args.get(1) {
        Some(v) => zpp_long(vm, f, 2, "port", v, false)?.unwrap_or(0),
        None => -1,
    };
    let timeout = match args.get(4) {
        Some(v) => zpp_double(vm, f, 5, "timeout", v, true)?,
        None => None,
    };
    let timeout = timeout.unwrap_or(default_socket_timeout(vm) as f64);
    let mut hashkey = None;
    if persistent {
        let mut k = b"pfsockopen__".to_vec();
        k.extend_from_slice(&host);
        k.extend_from_slice(format!(":{}", port).as_bytes());
        hashkey = Some(k);
    }
    let hostname = if port > 0 {
        let mut h = host.clone();
        h.extend_from_slice(format!(":{}", port).as_bytes());
        h
    } else {
        host.clone()
    };
    let max = PHP_TIMEOUT_ULL_MAX / 1_000_000.0;
    if timeout != -1.0 && !(timeout >= 0.0 && timeout <= max) {
        return Err(throw(
            vm,
            b"ValueError",
            &format!(
                "{}(): Argument #6 must be -1 or between 0 and {}",
                f, max as u64
            ),
        ));
    }
    let conv = (timeout * 1_000_000.0) as i64;
    let ms = tv_to_ms(conv / 1_000_000, conv % 1_000_000);
    if let Some(k) = &hashkey {
        if let Some(fid) = persistent_lookup(k) {
            set_ref(args.get(2), Value::Long(0));
            set_ref(args.get(3), str_val(""));
            return Ok(Value::resource(fid));
        }
    }
    match xport_create(
        vm,
        f,
        &hostname,
        XportOp::Connect {
            asynchronous: false,
        },
        Some(ms),
        None,
    ) {
        Ok(fid) => {
            set_ref(args.get(2), Value::Long(0));
            set_ref(args.get(3), str_val(""));
            if let Some(k) = hashkey {
                persistent_store(k, fid);
            }
            Ok(Value::resource(fid))
        }
        Err(e) => {
            let shown: Vec<u8> = host.iter().take_while(|&&c| c != 0).copied().collect();
            vm.emit_warning(&format!(
                "{}(): Unable to connect to {}:{} ({})",
                f,
                goro_core::string::msg_str(&shown),
                port,
                e.text.as_deref().unwrap_or("Unknown error")
            ));
            set_ref(args.get(2), Value::Long(e.code as i64));
            set_ref(args.get(3), str_val(e.text.as_deref().unwrap_or("")));
            Ok(Value::False)
        }
    }
}

/// PHP_FUNCTION(stream_set_blocking)
pub(crate) fn stream_set_blocking(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    const F: &str = "stream_set_blocking";
    check_arity(vm, F, args, 2, 2)?;
    let raw = args[0].deref();
    if crate::misc::is_dir_handle(&raw) {
        zpp_bool(vm, F, 2, "enable", &args[1])?;
        return crate::misc::dir_stream_op(vm, F, args).unwrap_or(Ok(Value::True));
    }
    let fid = match raw {
        Value::Resource(ref r) if (1..=3).contains(&r.id()) => raw.to_long(),
        _ => require_open_stream(vm, &args[0], F)?,
    };
    let block = zpp_bool(vm, F, 2, "enable", &args[1])?;
    Ok(if set_blocking(fid, block) {
        Value::True
    } else {
        Value::False
    })
}

/// PHP_FUNCTION(stream_set_timeout)
pub(crate) fn stream_set_timeout(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    stream_set_timeout_as(vm, "stream_set_timeout", args)
}

pub(crate) const SOCKET_SET_TIMEOUT_DEPRECATION: &str =
    "Function socket_set_timeout() is deprecated since 8.5, use stream_set_timeout() instead";

/// socket_set_timeout(), the deprecated alias of stream_set_timeout()
pub(crate) fn socket_set_timeout(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    vm.emit_function_deprecation(SOCKET_SET_TIMEOUT_DEPRECATION)?;
    stream_set_timeout_as(vm, "socket_set_timeout", args)
}

fn stream_set_timeout_as(vm: &mut Vm, f: &str, args: &[Value]) -> Result<Value, VmError> {
    check_arity(vm, f, args, 2, 3)?;
    if crate::misc::is_dir_handle(&args[0]) {
        return Ok(Value::False);
    }
    let fid = require_open_stream(vm, &args[0], f)?;
    let mut sec = zpp_long(vm, f, 2, "seconds", &args[1], false)?.unwrap_or(0);
    let mut usec = 0;
    if let Some(v) = args.get(2) {
        let u = zpp_long(vm, f, 3, "microseconds", v, false)?.unwrap_or(0);
        usec = u % 1_000_000;
        sec += u / 1_000_000;
    }
    let ok = with_sock(fid, |fh| match fh.sock.as_mut() {
        Some(sd) => {
            sd.tv_sec = sec;
            sd.tv_usec = usec;
            sd.timeout_event = false;
            true
        }
        None => false,
    })
    .unwrap_or(false);
    Ok(if ok { Value::True } else { Value::False })
}

// ---------------------------------------------------------------------------
// stream_select()
// ---------------------------------------------------------------------------

/// php_stream_cast(PHP_STREAM_AS_FD_FOR_SELECT) of a stream the caller
/// serves (user-space streams): None when it is not one of its streams,
/// Some(None) when it has no descriptor.
pub(crate) type FdResolver<'a> =
    dyn FnMut(&mut Vm, i64) -> Result<Option<Option<i32>>, VmError> + 'a;

fn element_stream_id(v: &Value) -> Option<i64> {
    match v.deref() {
        Value::Resource(id) => Some(id.id()),
        _ => None,
    }
}

fn select_array_arg(
    vm: &mut Vm,
    args: &[Value],
    i: usize,
    name: &str,
) -> Result<Option<Rc<RefCell<PhpArray>>>, VmError> {
    match args.get(i).map(|v| v.deref()) {
        None | Some(Value::Null) | Some(Value::Undef) => Ok(None),
        Some(Value::Array(a)) => Ok(Some(a)),
        Some(v) => Err(type_error(vm, "stream_select", i + 1, name, "?array", &v)),
    }
}

/// PHP_FUNCTION(stream_select), with `resolve` mapping a stream id that has
/// no descriptor of its own (e.g. a user-space stream) to one.
pub(crate) fn stream_select_impl(
    vm: &mut Vm,
    args: &[Value],
    resolve: &mut FdResolver,
    buffered: &dyn Fn(i64) -> bool,
) -> Result<Value, VmError> {
    const F: &str = "stream_select";
    check_arity(vm, F, args, 4, 5)?;
    let arrays = [
        select_array_arg(vm, args, 0, "read")?,
        select_array_arg(vm, args, 1, "write")?,
        select_array_arg(vm, args, 2, "except")?,
    ];
    let sec = zpp_long(vm, F, 4, "seconds", &args[3], true)?;
    let usec = match args.get(4) {
        Some(v) => zpp_long(vm, F, 5, "microseconds", v, true)?,
        None => None,
    };
    // stream_array_to_fd_set()
    let mut sets: Vec<Vec<(i32, i64)>> = vec![Vec::new(), Vec::new(), Vec::new()];
    let mut total = 0;
    for (i, a) in arrays.iter().enumerate() {
        let Some(a) = a else { continue };
        let vals: Vec<Value> = a.borrow().values().cloned().collect();
        for v in vals {
            // php_stream_from_zval_no_verify(): a TypeError for what is
            // not a stream (thrown on, chained to any pending one).
            let Some(id) = element_stream_id(&v) else {
                throw_chained(
                    vm,
                    b"TypeError",
                    "stream_select(): supplied argument is not a valid stream resource",
                );
                continue;
            };
            let is_stream = (1..=3).contains(&id)
                || FILE_HANDLES.with(|h| h.borrow().contains_key(&id))
                || DIR_HANDLES.with(|h| h.borrow().contains_key(&id));
            if !goro_core::value::is_open_resource(id) || !is_stream {
                throw_chained(
                    vm,
                    b"TypeError",
                    "stream_select(): supplied resource is not a valid stream resource",
                );
                continue;
            }
            let fd = match resolve(vm, id)? {
                Some(fd) => fd,
                None => stream_fd_for_select(vm, id)?,
            };
            if let Some(fd) = fd {
                sets[i].push((fd, id));
            }
        }
        if !sets[i].is_empty() {
            total += 1;
        }
    }
    if total == 0 {
        throw_chained(vm, b"ValueError", "No stream arrays were passed");
        return Err(VmError {
            message: "No stream arrays were passed".to_string(),
            line: vm.current_line,
        });
    }

    if sec.is_none() && usec.is_some() && usec != Some(0) {
        return Err(throw(
            vm,
            b"ValueError",
            "stream_select(): Argument #5 ($microseconds) must be null when argument #4 ($seconds) is null",
        ));
    }
    let mut timeout_ms = -1;
    if let Some(s) = sec {
        if s < 0 {
            return Err(throw(
                vm,
                b"ValueError",
                "stream_select(): Argument #4 ($seconds) must be greater than or equal to 0",
            ));
        }
        let u = usec.unwrap_or(0);
        if u < 0 {
            return Err(throw(
                vm,
                b"ValueError",
                "stream_select(): Argument #5 ($microseconds) must be greater than or equal to 0",
            ));
        }
        let total_s = s.saturating_add(u / 1_000_000);
        timeout_ms = total_s
            .saturating_mul(1000)
            .saturating_add((u % 1_000_000 + 999) / 1000)
            .min(i32::MAX as i64) as i32;
    }
    // stream_array_emulate_read_fd_set(): streams with buffered data are
    // readable without select().
    if let Some(r) = &arrays[0] {
        let mut kept = PhpArray::new();
        let mut cnt = 0;
        for (k, v) in r.borrow().iter() {
            if let Some(id) = element_stream_id(v) {
                if buffered(id) {
                    kept.set(k.clone(), v.clone());
                    cnt += 1;
                }
            }
        }
        if cnt > 0 {
            replace_array(args, 0, kept);
            for (i, a) in arrays.iter().enumerate().skip(1) {
                if a.is_some() {
                    replace_array(args, i, PhpArray::new());
                }
            }
            return Ok(Value::Long(cnt));
        }
    }
    let mut pfds: Vec<libc::pollfd> = Vec::new();
    let mut index: Vec<(usize, usize)> = Vec::new();
    for (i, set) in sets.iter().enumerate() {
        let events = match i {
            0 => libc::POLLIN,
            1 => libc::POLLOUT,
            _ => libc::POLLPRI,
        };
        for (j, (fd, _)) in set.iter().enumerate() {
            pfds.push(libc::pollfd {
                fd: *fd,
                events,
                revents: 0,
            });
            index.push((i, j));
        }
    }
    if timeout_ms != 0 {
        super::flush_cli_output(vm);
    }
    let n = unsafe { libc::poll(pfds.as_mut_ptr(), pfds.len() as libc::nfds_t, timeout_ms) };
    if n < 0 {
        let e = errno();
        let max_fd = pfds.iter().map(|p| p.fd).max().unwrap_or(0);
        vm.emit_warning(&format!(
            "{}(): Unable to select [{}]: {} (max_fd={})",
            F,
            e,
            strerror(e),
            max_fd
        ));
        return Ok(Value::False);
    }
    // Ready descriptors per set (select() semantics).
    let mut ready: Vec<std::collections::HashSet<i32>> = vec![Default::default(); 3];
    let mut count = 0i64;
    for (p, (i, _)) in pfds.iter().zip(index.iter()) {
        let rv = p.revents;
        let hit = match i {
            0 => rv & (libc::POLLIN | libc::POLLHUP | libc::POLLERR | libc::POLLNVAL) != 0,
            1 => rv & (libc::POLLOUT | libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0,
            _ => rv & libc::POLLPRI != 0,
        };
        if hit && ready[*i].insert(p.fd) {
            count += 1;
        }
    }
    for i in 0..3 {
        let Some(a) = &arrays[i] else { continue };
        let mut out = PhpArray::new();
        let entries: Vec<(ArrayKey, Value)> = a
            .borrow()
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        for (k, v) in entries {
            let Some(id) = element_stream_id(&v) else {
                throw_chained(
                    vm,
                    b"TypeError",
                    "stream_select(): supplied argument is not a valid stream resource",
                );
                continue;
            };
            // stream_array_from_fd_set() casts each stream again.
            let fd = match resolve(vm, id)? {
                Some(Some(fd)) => fd,
                Some(None) => continue,
                None => match sets[i].iter().find(|(_, sid)| *sid == id) {
                    Some(&(fd, _)) => fd,
                    None => {
                        let _ = stream_fd_for_select(vm, id)?;
                        continue;
                    }
                },
            };
            if ready[i].contains(&fd) {
                out.set(k, v.deref());
            }
        }
        replace_array(args, i, out);
    }
    if vm.current_exception.is_some() {
        return Err(VmError {
            message: String::new(),
            line: vm.current_line,
        });
    }
    Ok(Value::Long(count))
}

fn replace_array(args: &[Value], i: usize, arr: PhpArray) {
    if let Some(Value::Reference(r)) = args.get(i) {
        *r.borrow_mut() = Value::Array(Rc::new(RefCell::new(arr)));
    } else if let Some(Value::Array(a)) = args.get(i) {
        *a.borrow_mut() = arr;
    }
}

/// php_stream_cast(PHP_STREAM_AS_FD_FOR_SELECT) for built-in streams.
fn stream_fd_for_select(vm: &mut Vm, id: i64) -> Result<Option<i32>, VmError> {
    if (1..=3).contains(&id) {
        return Ok(Some((id - 1) as i32));
    }
    // php_openssl_sockop_cast(PHP_STREAM_AS_FD_FOR_SELECT): data the TLS
    // layer already decrypted moves to the read buffer, so it counts.
    FILE_HANDLES.with(|h| {
        if let Some(fh) = h.borrow_mut().get_mut(&id) {
            if !fh.filter_read_ready.is_empty() {
                return;
            }
            let fd = fh.file.as_raw_fd();
            if let Some(c) = fh.crypto.as_mut().filter(|c| c.is_active()) {
                let n = c.pending().min(STREAM_CHUNK_SIZE);
                if n > 0 {
                    let mut buf = vec![0u8; n];
                    if let Ok(k) = c.read(fd, &mut buf, false, -1) {
                        fh.filter_read_ready.extend_from_slice(&buf[..k]);
                    }
                }
            }
        }
    });
    let info = FILE_HANDLES.with(|h| {
        h.borrow()
            .get(&id)
            .map(|fh| (fh.special, fh.file.as_raw_fd(), fh.user_wrapper.is_some()))
    });
    match info {
        Some((SpecialStream::Memory, _, _)) => {
            vm.emit_warning(
                "stream_select(): Cannot represent a stream of type MEMORY as a select()able descriptor",
            );
            Ok(None)
        }
        Some((SpecialStream::Stdin, _, _)) => Ok(Some(0)),
        Some((SpecialStream::Stdout, _, _)) | Some((SpecialStream::Output, _, _)) => Ok(Some(1)),
        Some((SpecialStream::Stderr, _, _)) => Ok(Some(2)),
        Some((_, _, true)) => Ok(None),
        Some((_, fd, false)) => Ok(Some(fd)),
        None => Ok(None),
    }
}

/// stream_select() for built-in streams.
pub(crate) fn stream_select(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    stream_select_impl(vm, args, &mut |_, _| Ok(None), &|id| {
        FILE_HANDLES.with(|h| {
            h.borrow()
                .get(&id)
                .map(|fh| !fh.filter_read_ready.is_empty())
                .unwrap_or(false)
        })
    })
}

// ---------------------------------------------------------------------------
// ext/sockets services (socket_import_stream() / socket_export_stream())
// ---------------------------------------------------------------------------

/// php_stream_cast(PHP_STREAM_AS_SOCKETD) for `func`.
fn hook_stream_socket_fd(vm: &mut Vm, v: &Value, func: &str) -> Result<Option<i32>, VmError> {
    let v = v.deref();
    let id = match v {
        Value::Resource(id) => id.id(),
        _ => {
            return Err(throw(
                vm,
                b"TypeError",
                &format!(
                    "{}(): Argument #1 ($stream) must be of type resource, {} given",
                    func,
                    Vm::value_type_name(&v)
                ),
            ));
        }
    };
    let info = FILE_HANDLES.with(|h| {
        h.borrow().get(&id).map(|fh| {
            (
                fh.sock.is_some() && !fh.crypto.as_ref().is_some_and(|c| c.is_active()),
                fh.file.as_raw_fd(),
                fh.stream_type.clone(),
            )
        })
    });
    match info {
        Some((true, fd, _)) => Ok(Some(fd)),
        Some((false, _, ty)) => {
            vm.emit_warning(&format!(
                "{}(): Cannot represent a stream of type {} as a Socket Descriptor",
                func, ty
            ));
            Ok(None)
        }
        None => Err(throw(
            vm,
            b"TypeError",
            &format!(
                "{}(): supplied resource is not a valid stream resource",
                func
            ),
        )),
    }
}

/// socket_export_stream(): a stream with the sockops of the socket's
/// protocol (or a generic socket stream).
fn hook_stream_from_socket(vm: &mut Vm, fd: i32) -> i64 {
    let mut ss: libc::sockaddr_storage = unsafe { std::mem::zeroed() };
    let mut sl = std::mem::size_of::<libc::sockaddr_storage>() as libc::socklen_t;
    let fam = if unsafe { libc::getsockname(fd, &mut ss as *mut _ as *mut libc::sockaddr, &mut sl) }
        == 0
    {
        ss.ss_family as i32
    } else {
        -1
    };
    let getopt = |opt: i32| -> i32 {
        let mut v: i32 = 0;
        let mut l = std::mem::size_of::<i32>() as libc::socklen_t;
        unsafe {
            libc::getsockopt(
                fd,
                libc::SOL_SOCKET,
                opt,
                &mut v as *mut i32 as *mut libc::c_void,
                &mut l,
            );
        }
        v
    };
    let ty = getopt(libc::SO_TYPE);
    let (kind, uri) = match fam {
        libc::AF_INET | libc::AF_INET6
            if ty == libc::SOCK_STREAM && getopt(libc::SO_PROTOCOL) == libc::IPPROTO_TCP =>
        {
            (SockKind::Tcp, Some(&b"tcp://"[..]))
        }
        libc::AF_INET | libc::AF_INET6 if ty == libc::SOCK_DGRAM => {
            (SockKind::Udp, Some(&b"udp://"[..]))
        }
        libc::AF_UNIX if ty == libc::SOCK_STREAM => (SockKind::Unix, Some(&b"unix://"[..])),
        libc::AF_UNIX if ty == libc::SOCK_DGRAM => (SockKind::Udg, Some(&b"udg://"[..])),
        _ => (SockKind::Generic, None),
    };
    let t = default_socket_timeout(vm);
    alloc_socket_stream(fd, SockData::new(kind, t), uri, None)
}

fn hook_close_stream(vm: &mut Vm, id: i64) {
    if FILE_HANDLES.with(|h| h.borrow().contains_key(&id)) {
        let _ = super::fclose_fn(vm, &[Value::resource(id)]);
    }
}

pub(crate) fn install_hooks(vm: &mut Vm) {
    vm.socket_stream_hooks = Some(goro_core::vm::SocketStreamHooks {
        stream_socket_fd: hook_stream_socket_fd,
        stream_from_socket: hook_stream_from_socket,
        close_stream: hook_close_stream,
    });
}

// ---------------------------------------------------------------------------
// TLS (the socket side of ext/openssl/xp_ssl.c)
// ---------------------------------------------------------------------------

const STREAM_CRYPTO_IS_CLIENT: i64 = 1;
const STREAM_CRYPTO_METHOD_TLS_ANY_CLIENT: i64 = 121;
const STREAM_CRYPTO_METHOD_TLSV1_0_CLIENT: i64 = 9;
const STREAM_CRYPTO_METHOD_TLSV1_1_CLIENT: i64 = 17;
const STREAM_CRYPTO_METHOD_TLSV1_2_CLIENT: i64 = 33;
const STREAM_CRYPTO_METHOD_TLSV1_3_CLIENT: i64 = 65;

thread_local! {
    /// Warnings of TLS reads, emitted by the PHP function that read.
    static CRYPTO_WARNINGS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    /// Callbacks TLS reads made due, with the descriptor of their stream.
    static CRYPTO_CALLBACKS: RefCell<Vec<(i32, Value)>> = const { RefCell::new(Vec::new()) };
}

/// Queue what a TLS read left for the VM: its warnings and callbacks.
fn queue_crypto_output(c: &mut Box<dyn goro_core::crypto::CryptoStream>, fd: i32) {
    let w = c.take_warnings();
    if !w.is_empty() {
        CRYPTO_WARNINGS.with(|q| q.borrow_mut().extend(w));
    }
    let cbs = c.take_callbacks();
    if !cbs.is_empty() {
        CRYPTO_CALLBACKS.with(|q| q.borrow_mut().extend(cbs.into_iter().map(|cb| (fd, cb))));
    }
}

/// Emit the warnings TLS reads queued (they run without the VM), and call
/// the callbacks they made due (php_openssl_limit_handshake_reneg()).
pub(crate) fn flush_crypto_warnings(vm: &mut Vm, func: &str) {
    let w = CRYPTO_WARNINGS.with(|w| std::mem::take(&mut *w.borrow_mut()));
    for m in w {
        vm.emit_warning(&format!("{}(): {}", func, m));
    }
    let cbs = CRYPTO_CALLBACKS.with(|q| std::mem::take(&mut *q.borrow_mut()));
    for (fd, cb) in cbs {
        let fid = FILE_HANDLES.with(|h| {
            h.borrow()
                .iter()
                .find(|(_, fh)| fh.sock.is_some() && fh.file.as_raw_fd() == fd)
                .map(|(id, _)| *id)
        });
        let Some(fid) = fid else { continue };
        if vm.call_callback(&cb, &[Value::resource(fid)]).is_err() && vm.current_exception.is_none()
        {
            vm.emit_warning("SSL: failed invoking reneg limit notification callback");
        }
    }
}

/// The "ssl" options of a stream's (or context's) context.
pub(crate) fn ssl_options(id: i64) -> Option<PhpArray> {
    let id = ctx_key(id);
    STREAM_CONTEXTS.with(|c| {
        let c = c.borrow();
        let ctx = c.get(&id)?;
        let m = ctx.options.get(b"ssl".as_ref())?;
        let mut a = PhpArray::new();
        for (k, v) in m {
            a.set(ArrayKey::String(PhpString::from_bytes(k)), v.clone());
        }
        Some(a)
    })
}

fn set_ssl_option(id: i64, name: &[u8], v: Value) {
    let id = ctx_key(id);
    STREAM_CONTEXTS.with(|c| {
        if let Some(ctx) = c.borrow_mut().get_mut(&id) {
            ctx.options
                .entry(b"ssl".to_vec())
                .or_default()
                .insert(name.to_vec(), v);
        }
    });
}

fn ctx_crypto_method(ctx: Option<i64>) -> Option<Value> {
    let c = ctx_key(ctx?);
    STREAM_CONTEXTS.with(|m| {
        m.borrow()
            .get(&c)
            .and_then(|x| x.options.get(b"ssl".as_ref()))
            .and_then(|o| o.get(b"crypto_method".as_ref()).cloned())
    })
}

/// php_openssl_get_crypto_method()
fn get_crypto_method(ctx: Option<i64>, default: i64) -> i64 {
    match ctx_crypto_method(ctx) {
        Some(v) => v.to_long() | STREAM_CRYPTO_IS_CLIENT,
        None => default,
    }
}

/// php_openssl_get_url_name(): the host of "host:port" without trailing dots.
fn url_name(name: &[u8]) -> Option<Vec<u8>> {
    let host: &[u8] = if name.first() == Some(&b'[') {
        match name.iter().position(|&c| c == b']') {
            Some(e) => &name[..=e],
            None => name,
        }
    } else {
        match name.iter().rposition(|&c| c == b':') {
            Some(p) => &name[..p],
            None => name,
        }
    };
    let host: &[u8] = match host.iter().position(|&c| c == b'/') {
        Some(p) => &host[..p],
        None => host,
    };
    let mut h = host.to_vec();
    while h.last() == Some(&b'.') {
        h.pop();
    }
    if h.is_empty() { None } else { Some(h) }
}

/// php_stream_xport_crypto_setup(): < 0 on failure.
pub(crate) fn crypto_setup(
    vm: &mut Vm,
    func: &str,
    fid: i64,
    method: i64,
    session: Option<i64>,
) -> i32 {
    let info = FILE_HANDLES.with(|h| {
        h.borrow()
            .get(&fid)
            .map(|fh| (fh.sock.clone(), fh.crypto.is_some()))
    });
    let Some((Some(sd), has)) = info else {
        vm.emit_warning(&format!(
            "{}(): This stream does not support SSL/crypto",
            func
        ));
        return -2;
    };
    let hooks = match vm.crypto_hooks {
        Some(h) if sd.kind == SockKind::Tcp => h,
        _ => {
            vm.emit_warning(&format!(
                "{}(): This stream does not support SSL/crypto",
                func
            ));
            return -2;
        }
    };
    if has {
        if sd.is_blocked {
            vm.emit_warning(&format!(
                "{}(): SSL/TLS already set-up for this stream",
                func
            ));
            return -1;
        }
        return 0;
    }
    if let Some(sess) = session {
        let ok = FILE_HANDLES.with(|h| {
            h.borrow()
                .get(&sess)
                .map(|fh| (fh.sock.is_some(), fh.crypto.is_some()))
        });
        match ok {
            Some((true, true)) => {}
            Some((true, false)) => vm.emit_warning(&format!(
                "{}(): Supplied SSL session stream is not initialized",
                func
            )),
            _ => vm.emit_warning(&format!(
                "{}(): Supplied session stream must be an SSL enabled stream",
                func
            )),
        }
    }
    let opts = ssl_options(fid);
    let is_client = method & STREAM_CRYPTO_IS_CLIENT != 0;
    let setup = goro_core::crypto::CryptoSetup {
        is_client,
        method: method & !STREAM_CRYPTO_IS_CLIENT,
        url_name: sd.url_name.as_deref(),
        opts: opts.as_ref(),
        blocked: sd.is_blocked,
        func,
    };
    match (hooks.setup)(vm, &setup) {
        Some(c) => {
            FILE_HANDLES.with(|h| {
                if let Some(fh) = h.borrow_mut().get_mut(&fid) {
                    fh.crypto = Some(c);
                    if let Some(s) = fh.sock.as_mut() {
                        s.crypto_client = is_client;
                    }
                }
            });
            0
        }
        None => -1,
    }
}

/// php_stream_xport_crypto_enable(): -1 failure, 0 would block, 1 done.
pub(crate) fn crypto_enable(vm: &mut Vm, func: &str, fid: i64, activate: bool) -> i32 {
    let taken = FILE_HANDLES.with(|h| {
        h.borrow_mut()
            .get_mut(&fid)
            .map(|fh| (fh.crypto.take(), fh.sock.clone(), fh.file.as_raw_fd()))
    });
    let Some((crypto, Some(sd), fd)) = taken else {
        vm.emit_warning(&format!(
            "{}(): This stream does not support SSL/crypto",
            func
        ));
        return -2;
    };
    let Some(mut c) = crypto else {
        if sd.kind == SockKind::Tcp && vm.crypto_hooks.is_some() {
            return -1;
        }
        vm.emit_warning(&format!(
            "{}(): This stream does not support SSL/crypto",
            func
        ));
        return -2;
    };
    let mut result = -1;
    if activate && !c.is_active() {
        let timeout = if sd.crypto_client {
            sd.connect_timeout_ms
        } else {
            sd.timeout_ms()
        };
        let opts = ssl_options(fid);
        vm.flush_cli_output();
        let r = c.enable(vm, func, fd, sd.is_blocked, timeout, opts.as_ref());
        for (k, v) in r.captured {
            set_ssl_option(fid, &k, v.clone());
            if let Some(src) = sd.ctx_src {
                if ctx_key(src) != ctx_key(fid) {
                    set_ssl_option(src, &k, v);
                }
            }
        }
        result = r.result;
    } else if !activate && c.is_active() {
        c.disable(fd);
    }
    FILE_HANDLES.with(|h| {
        if let Some(fh) = h.borrow_mut().get_mut(&fid) {
            fh.crypto = Some(c);
        }
    });
    result
}

/// PHP_FUNCTION(stream_socket_enable_crypto)
pub(crate) fn stream_socket_enable_crypto(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    const F: &str = "stream_socket_enable_crypto";
    check_arity(vm, F, args, 2, 4)?;
    let fid = require_open_stream(vm, &args[0], F)?;
    let enable = zpp_bool(vm, F, 2, "enable", &args[1])?;
    let kind = match args.get(2) {
        Some(v) => zpp_long(vm, F, 3, "crypto_method", v, true)?,
        None => None,
    };
    let session = match args.get(3).map(|v| v.deref()) {
        None | Some(Value::Null) => None,
        Some(Value::Resource(id)) => Some(id.id()),
        Some(v) => {
            return Err(type_error(
                vm,
                F,
                4,
                "session_stream",
                "resource or null",
                &v,
            ));
        }
    };
    if enable {
        let kind = match kind {
            Some(k) => k,
            None => match ctx_crypto_method(resolve_context_id(&args[0])) {
                Some(v) => v.to_long(),
                None => {
                    return Err(throw(
                        vm,
                        b"ValueError",
                        "stream_socket_enable_crypto(): Argument #3 ($crypto_method) must be specified when enabling encryption",
                    ));
                }
            },
        };
        if crypto_setup(vm, F, fid, kind, session) < 0 {
            return Ok(Value::False);
        }
    }
    Ok(match crypto_enable(vm, F, fid, enable) {
        r if r < 0 => Value::False,
        0 => Value::Long(0),
        _ => Value::True,
    })
}

/// php_openssl_sockop_io(read) for an active TLS stream.
fn crypto_read(
    fh: &mut FileHandle,
    buf: &mut [u8],
    has_buffered: bool,
) -> Option<std::io::Result<usize>> {
    let fd = fh.file.as_raw_fd();
    if !fh.crypto.as_ref().map(|c| c.is_active()).unwrap_or(false) {
        return None;
    }
    let sd = fh.sock.as_mut()?;
    let blocked = sd.is_blocked && !has_buffered;
    let timeout = if sd.tv_sec == -1 { -1 } else { sd.timeout_ms() };
    let c = fh.crypto.as_mut()?;
    let r = c.read(fd, buf, blocked, timeout);
    queue_crypto_output(c, fd);
    match &r {
        Ok(0) if !buf.is_empty() => fh.eof = true,
        Err(e) if e.kind() == std::io::ErrorKind::TimedOut => sd.timeout_event = true,
        _ => {}
    }
    Some(r)
}

/// php_openssl_sockop_io(write) for an active TLS stream: bytes written,
/// 0 when nothing could be written, -1 on timeout.
fn crypto_write(vm: &mut Vm, func: &str, fid: i64, data: &[u8]) -> Option<isize> {
    let taken = FILE_HANDLES.with(|h| {
        let mut h = h.borrow_mut();
        let fh = h.get_mut(&fid)?;
        if !fh.crypto.as_ref().map(|c| c.is_active()).unwrap_or(false) {
            return None;
        }
        Some((fh.crypto.take()?, fh.sock.clone()?, fh.file.as_raw_fd()))
    });
    let (mut c, sd, fd) = taken?;
    let timeout = if sd.is_blocked && sd.tv_sec != -1 {
        sd.timeout_ms()
    } else {
        -1
    };
    let r = c.write(fd, data, sd.is_blocked, timeout);
    let w = c.take_warnings();
    FILE_HANDLES.with(|h| {
        if let Some(fh) = h.borrow_mut().get_mut(&fid) {
            fh.crypto = Some(c);
            if let (Err(e), Some(s)) = (&r, fh.sock.as_mut()) {
                if e.kind() == std::io::ErrorKind::TimedOut {
                    s.timeout_event = true;
                }
            }
        }
    });
    for m in w {
        vm.emit_warning(&format!("{}(): {}", func, m));
    }
    Some(match r {
        Ok(n) => n as isize,
        Err(e) if e.kind() == std::io::ErrorKind::TimedOut => -1,
        Err(_) => 0,
    })
}

// ---------------------------------------------------------------------------
// More stream services for ext/sockets (see crate::sockets_streams)
// ---------------------------------------------------------------------------

/// php_stream_set_option(PHP_STREAM_OPTION_BLOCKING) on stream `fid`: None
/// when it is not an open stream, Some(false) when the option failed.
pub fn ext_set_blocking(fid: i64, block: bool) -> Option<bool> {
    if !FILE_HANDLES.with(|h| h.borrow().contains_key(&fid)) {
        return None;
    }
    Some(set_blocking(fid, block))
}

/// php_stream_cast(PHP_STREAM_AS_FD, REPORT_ERRORS) of a stream resource
/// (None after a warning, or when it is not a stream).
pub fn ext_stream_fd(vm: &mut Vm, id: i64, func: &str) -> Option<i32> {
    if (1..=3).contains(&id) && !FILE_HANDLES.with(|h| h.borrow().contains_key(&id)) {
        return Some((id - 1) as i32);
    }
    let info = FILE_HANDLES.with(|h| {
        h.borrow().get(&id).map(|fh| {
            (
                fh.special,
                fh.file.as_raw_fd(),
                fh.user_wrapper.is_some(),
                fh.stream_type.clone(),
            )
        })
    })?;
    match info {
        (SpecialStream::Stdin, _, _, _) => Some(0),
        (SpecialStream::Stdout, _, _, _) | (SpecialStream::Output, _, _, _) => Some(1),
        (SpecialStream::Stderr, _, _, _) => Some(2),
        (SpecialStream::Memory | SpecialStream::Temp, _, _, ty) | (_, _, true, ty) => {
            vm.emit_warning(&format!(
                "{}(): Cannot represent a stream of type {} as a File Descriptor",
                func, ty
            ));
            None
        }
        (_, fd, false, _) => Some(fd),
    }
}

/// php_stream_cast(PHP_STREAM_AS_FD | PHP_STREAM_CAST_INTERNAL) without
/// reporting errors: None when `id` is not an open stream, Err(the stream
/// type) when it has no descriptor.
pub fn ext_stream_cast_fd(id: i64) -> Option<Result<i32, String>> {
    if (1..=3).contains(&id) && !FILE_HANDLES.with(|h| h.borrow().contains_key(&id)) {
        return Some(Ok((id - 1) as i32));
    }
    FILE_HANDLES.with(|h| {
        h.borrow().get(&id).map(|fh| match fh.special {
            SpecialStream::Stdin => Ok(0),
            SpecialStream::Stdout | SpecialStream::Output => Ok(1),
            SpecialStream::Stderr => Ok(2),
            SpecialStream::Memory | SpecialStream::Temp => Err(fh.stream_type.clone()),
            SpecialStream::None if fh.user_wrapper.is_some() => Err(fh.stream_type.clone()),
            SpecialStream::None => Ok(fh.file.as_raw_fd()),
        })
    })
}

/// Whether `id` is an open stream resource.
pub fn ext_is_stream(id: i64) -> bool {
    (1..=3).contains(&id) || FILE_HANDLES.with(|h| h.borrow().contains_key(&id))
}

/// php_stream_fopen_from_fd(fd, mode, NULL): a plain-file stream owning
/// descriptor `fd`.
pub fn ext_stream_from_fd(fd: i32, mode: &str) -> i64 {
    let file = unsafe { std::fs::File::from_raw_fd(fd) };
    let fid = alloc_file_handle(file, mode);
    goro_core::value::register_resource(fid, b"stream");
    fid
}

// ---------------------------------------------------------------------------
// Transport services for the ftp:// wrapper
// ---------------------------------------------------------------------------

/// php_stream_xport_create(`name`, REPORT_ERRORS, CLIENT | CONNECT,
/// context): the stream, or Err(error text). With `want_error` the text is
/// handed back (error_string != NULL); otherwise it is reported as
/// "connect() failed: ..." like ERR_RETURN does.
pub(crate) fn xport_connect(
    vm: &mut Vm,
    func: &str,
    name: &[u8],
    ctx: Option<i64>,
    want_error: bool,
) -> Result<i64, String> {
    let timeout = default_socket_timeout(vm);
    match xport_create(
        vm,
        func,
        name,
        XportOp::Connect {
            asynchronous: false,
        },
        timeout_from_double(timeout as f64),
        ctx,
    ) {
        Ok(fid) => Ok(fid),
        Err(e) => {
            let text = e.text.unwrap_or_default();
            if !want_error && !text.is_empty() {
                vm.emit_warning(&format!("{}(): connect() failed: {}", func, text));
            }
            Err(text)
        }
    }
}

/// php_stream_xport_crypto_setup(SSLv23_CLIENT) + crypto_enable(1): false
/// on failure.
pub(crate) fn enable_client_crypto(vm: &mut Vm, func: &str, fid: i64) -> bool {
    const STREAM_CRYPTO_METHOD_SSLV23_CLIENT: i64 = 57;
    let ok = crypto_setup(vm, func, fid, STREAM_CRYPTO_METHOD_SSLV23_CLIENT, None) >= 0
        && crypto_enable(vm, func, fid, true) >= 0;
    flush_crypto_warnings(vm, func);
    ok
}

// ---------------------------------------------------------------------------
// Transport services for the http:// wrapper
// ---------------------------------------------------------------------------

/// php_stream_xport_create(`name`, options, CLIENT | CONNECT, timeout,
/// context) followed by PHP_STREAM_OPTION_READ_TIMEOUT: the stream, or
/// Err(the error text, if any).
pub(crate) fn http_connect(
    vm: &mut Vm,
    func: &str,
    name: &[u8],
    timeout: f64,
    ctx: Option<i64>,
) -> Result<i64, Option<String>> {
    let fid = xport_create(
        vm,
        func,
        name,
        XportOp::Connect {
            asynchronous: false,
        },
        timeout_from_double(timeout),
        ctx,
    )
    .map_err(|e| e.text)?;
    // (a negative timeout waits forever)
    let (sec, usec) = if timeout < 0.0 {
        (-1, 0)
    } else {
        let sec = timeout.trunc();
        (sec as i64, ((timeout - sec) * 1_000_000.0) as i64)
    };
    with_sock(fid, |fh| {
        if let Some(sd) = fh.sock.as_mut() {
            sd.tv_sec = sec;
            sd.tv_usec = usec;
        }
    });
    Ok(fid)
}

/// The CONNECT tunnel of an https:// request through a proxy, then the TLS
/// layer: false ("Cannot connect to HTTPS server through proxy").
pub(crate) fn http_proxy_tunnel(
    vm: &mut Vm,
    func: &str,
    fid: i64,
    connect: &[u8],
    peer_name: Option<&[u8]>,
) -> bool {
    if sock_write(vm, func, fid, connect) != connect.len() as isize {
        return false;
    }
    // php_stream_gets(stream, header_line, HTTP_HEADER_BLOCK_SIZE - 1)
    // until an empty line.
    while let Some(line) = super::stream_get_line_raw(fid, 1022) {
        if matches!(line.first(), None | Some(b'\n' | b'\r' | 0)) {
            break;
        }
    }
    // Set peer_name or name verification would try to use the proxy
    // server name.
    let key = ctx_key(fid);
    if let Some(p) = peer_name {
        STREAM_CONTEXTS.with(|c| {
            let mut c = c.borrow_mut();
            let ctx = c.entry(key).or_default();
            ctx.options
                .entry(b"ssl".to_vec())
                .or_default()
                .entry(b"peer_name".to_vec())
                .or_insert_with(|| Value::String(PhpString::from_bytes(p)));
        });
    }
    let ok = enable_client_crypto(vm, func, fid);
    if peer_name.is_some() {
        STREAM_CONTEXTS.with(|c| {
            if let Some(o) = c
                .borrow_mut()
                .get_mut(&key)
                .and_then(|ctx| ctx.options.get_mut(b"ssl".as_slice()))
            {
                o.shift_remove(b"peer_name".as_slice());
            }
        });
    }
    ok
}

/// Close a stream the http:// wrapper opened (php_stream_close()).
pub(crate) fn http_close(fid: i64) {
    FILE_HANDLES.with(|h| h.borrow_mut().remove(&fid));
    STREAM_CONTEXTS.with(|c| c.borrow_mut().remove(&fid));
    release_stream_ctx(fid);
    goro_core::value::close_resource(fid);
}

thread_local! {
    /// Notifications of socket I/O waiting to be delivered (the stream layer
    /// cannot call user code while a stream is borrowed): (context, code,
    /// bytes so far, max).
    static PENDING_NOTIFY: RefCell<Vec<(i64, i64, i64, i64)>> = const { RefCell::new(Vec::new()) };
}

const NOTIFY_PROGRESS: i64 = 7;
const NOTIFY_COMPLETED: i64 = 8;

/// php_stream_notify_progress_increment(context, delta, 0)
pub(crate) fn notify_progress_increment(ctx: Option<i64>, delta: i64) {
    let Some(id) = ctx else {
        return;
    };
    let ev = STREAM_CONTEXTS.with(|c| {
        let mut c = c.borrow_mut();
        let ctx = c.get_mut(&id)?;
        if ctx.notification.is_none() || !ctx.progress_mask {
            return None;
        }
        ctx.progress += delta;
        Some((id, NOTIFY_PROGRESS, ctx.progress, ctx.progress_max))
    });
    if let Some(e) = ev {
        PENDING_NOTIFY.with(|p| p.borrow_mut().push(e));
    }
}

/// php_stream_notify_completed(context)
pub(crate) fn notify_completed(ctx: Option<i64>) {
    let Some(id) = ctx else {
        return;
    };
    let ev = STREAM_CONTEXTS.with(|c| {
        let c = c.borrow();
        let ctx = c.get(&id)?;
        ctx.notification.as_ref()?;
        Some((id, NOTIFY_COMPLETED, ctx.progress, ctx.progress_max))
    });
    if let Some(e) = ev {
        PENDING_NOTIFY.with(|p| p.borrow_mut().push(e));
    }
}

/// php_stream_notify_progress_init(context, 0, max)
pub(crate) fn notify_progress_init(ctx: Option<i64>, max: i64) {
    let Some(id) = ctx else {
        return;
    };
    let ev = STREAM_CONTEXTS.with(|c| {
        let mut c = c.borrow_mut();
        let ctx = c.get_mut(&id)?;
        ctx.notification.as_ref()?;
        ctx.progress = 0;
        ctx.progress_max = max;
        ctx.progress_mask = true;
        Some((id, NOTIFY_PROGRESS, 0, max))
    });
    if let Some(e) = ev {
        PENDING_NOTIFY.with(|p| p.borrow_mut().push(e));
    }
}

/// The context a socket stream was created with (stream->ctx).
pub(crate) fn stream_ctx_src(fid: i64) -> Option<i64> {
    FILE_HANDLES.with(|h| {
        h.borrow()
            .get(&fid)
            .and_then(|fh| fh.sock.as_ref())
            .and_then(|sd| sd.ctx_src)
    })
}

/// Deliver the queued notifications (php_stream_notification_notify()).
pub(crate) fn flush_notifications(vm: &mut Vm) {
    while let Some((id, code, sofar, max)) = PENDING_NOTIFY.with(|p| {
        let mut p = p.borrow_mut();
        if p.is_empty() {
            None
        } else {
            Some(p.remove(0))
        }
    }) {
        if vm.current_exception.is_some() {
            PENDING_NOTIFY.with(|p| p.borrow_mut().clear());
            return;
        }
        let Some(cb) =
            STREAM_CONTEXTS.with(|c| c.borrow().get(&id).and_then(|c| c.notification.clone()))
        else {
            continue;
        };
        let args = [
            cb,
            Value::Long(code),
            Value::Long(0),
            Value::Null,
            Value::Long(0),
            Value::Long(sofar),
            Value::Long(max),
        ];
        let _ = super::call_user_func(vm, &args);
    }
}

/// Whether notifications are waiting (a cheap check for the stream layer).
pub(crate) fn notifications_pending() -> bool {
    PENDING_NOTIFY.with(|p| !p.borrow().is_empty())
}
