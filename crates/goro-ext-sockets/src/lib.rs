//! ext/sockets: a port of php-src's ext/sockets (sockets.c, multicast.c,
//! sendrecvmsg.c, conversions.c, sockaddr_conv.c) on raw descriptors.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::ffi::{CStr, CString};
use std::rc::Rc;

use goro_core::object::{ClassEntry, NativeData, PhpObject};
use goro_core::value::Value;
use goro_core::vm::{NativeObjectHandlers, Vm, VmError};

mod consts;
mod conv;
mod funcs;
mod mcast;
mod opts;
mod zpp;

use zpp::Obj;

// ---------------------------------------------------------------------------
// Socket and AddressInfo objects
// ---------------------------------------------------------------------------

/// php_socket
pub struct Sock {
    pub fd: Cell<i32>,
    /// the socket family (php_socket->type)
    pub family: Cell<i32>,
    pub error: Cell<i32>,
    pub blocking: Cell<bool>,
    /// the stream the socket was imported from / exported to
    pub zstream: Cell<Option<i64>>,
}

impl Sock {
    fn new() -> Sock {
        Sock {
            fd: Cell::new(-1),
            family: Cell::new(libc::AF_UNSPEC),
            error: Cell::new(0),
            blocking: Cell::new(true),
            zstream: Cell::new(None),
        }
    }
    pub fn is_invalid(&self) -> bool {
        self.fd.get() < 0
    }
}

impl Drop for Sock {
    /// socket_free_obj(): the descriptor is closed unless a stream owns it.
    fn drop(&mut self) {
        if self.zstream.get().is_none() && self.fd.get() >= 0 {
            unsafe {
                libc::close(self.fd.get());
            }
        }
    }
}

/// php_addrinfo
pub struct AddrInfo {
    pub flags: i32,
    pub family: i32,
    pub socktype: i32,
    pub protocol: i32,
    /// the raw sockaddr
    pub addr: Vec<u8>,
    pub canonname: Option<Vec<u8>>,
}

/// zend_objects_not_comparable: different objects are never equal.
fn not_comparable(a: &Value, b: &Value) -> Option<i64> {
    match (a, b) {
        (Value::Object(x), Value::Object(y)) if Rc::ptr_eq(x, y) => Some(0),
        (Value::Object(_), Value::Object(_)) => Some(1),
        _ => None,
    }
}

static SOCKET_HANDLERS: NativeObjectHandlers = NativeObjectHandlers {
    compare: Some(not_comparable),
    ..NativeObjectHandlers::EMPTY
};

static ADDRINFO_HANDLERS: NativeObjectHandlers = NativeObjectHandlers {
    compare: Some(not_comparable),
    ..NativeObjectHandlers::EMPTY
};

/// object_init_ex(socket_ce)
pub fn new_socket(vm: &mut Vm) -> (Value, Rc<Sock>) {
    let id = vm.next_object_id();
    let mut obj = PhpObject::new(b"Socket".to_vec(), id);
    let s = Rc::new(Sock::new());
    obj.native = Some(NativeData {
        data: s.clone() as Rc<dyn Any>,
        handlers: &SOCKET_HANDLERS,
    });
    (Value::Object(Rc::new(RefCell::new(obj))), s)
}

pub fn new_addrinfo(vm: &mut Vm, ai: AddrInfo) -> Value {
    let id = vm.next_object_id();
    let mut obj = PhpObject::new(b"AddressInfo".to_vec(), id);
    obj.native = Some(NativeData {
        data: Rc::new(ai) as Rc<dyn Any>,
        handlers: &ADDRINFO_HANDLERS,
    });
    Value::Object(Rc::new(RefCell::new(obj)))
}

/// Z_SOCKET_P(): a Socket object without native state (never initialized
/// by this extension) behaves as a closed socket.
pub fn sock_of(o: &Obj) -> Rc<Sock> {
    let d = o.borrow().native.as_ref().map(|n| n.data.clone());
    match d.and_then(|d| d.downcast::<Sock>().ok()) {
        Some(s) => s,
        None => Rc::new(Sock::new()),
    }
}

pub fn addrinfo_of(o: &Obj) -> Option<Rc<AddrInfo>> {
    let d = o.borrow().native.as_ref().map(|n| n.data.clone())?;
    d.downcast::<AddrInfo>().ok()
}

/// A Socket object value?
pub fn value_socket(v: &Value) -> Option<Rc<Sock>> {
    match v.deref() {
        Value::Object(o) if o.borrow().class_name.eq_ignore_ascii_case(b"Socket") => {
            Some(sock_of(&o))
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

thread_local! {
    /// SOCKETS_G(last_error)
    static LAST_ERROR: Cell<i32> = const { Cell::new(0) };
}

pub fn set_last_error(e: i32) {
    LAST_ERROR.with(|c| c.set(e));
}

pub fn last_error() -> i32 {
    LAST_ERROR.with(|c| c.get())
}

pub fn errno() -> i32 {
    std::io::Error::last_os_error().raw_os_error().unwrap_or(0)
}

pub fn set_errno(e: i32) {
    unsafe {
        *libc::__errno_location() = e;
    }
}

unsafe extern "C" {
    fn __h_errno_location() -> *mut libc::c_int;
    fn gethostbyname(name: *const libc::c_char) -> *mut libc::hostent;
    fn sockatmark(fd: libc::c_int) -> libc::c_int;
    fn inet_pton(af: libc::c_int, src: *const libc::c_char, dst: *mut libc::c_void) -> libc::c_int;
    fn inet_ntop(
        af: libc::c_int,
        src: *const libc::c_void,
        dst: *mut libc::c_char,
        size: libc::socklen_t,
    ) -> *const libc::c_char;
}

pub fn h_errno() -> i32 {
    unsafe { *__h_errno_location() }
}

fn cstr_to_string(p: *const libc::c_char) -> String {
    if p.is_null() {
        return String::new();
    }
    unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
}

/// sockets_strerror()
pub fn sockets_strerror(error: i32) -> String {
    if error < -10000 {
        let e = if error == i32::MIN {
            2147473648u32 as i32
        } else {
            -error - 10000
        };
        cstr_to_string(unsafe { libc::hstrerror(e) })
    } else {
        cstr_to_string(unsafe { libc::strerror(error) })
    }
}

/// PHP_SOCKET_ERROR()
pub fn sock_error(vm: &mut Vm, f: &str, sock: &Sock, msg: &str, err: i32) {
    sock.error.set(err);
    set_last_error(err);
    if err != libc::EAGAIN && err != libc::EWOULDBLOCK && err != libc::EINPROGRESS {
        vm.emit_warning(&format!(
            "{}(): {} [{}]: {}",
            f,
            msg,
            err,
            sockets_strerror(err)
        ));
    }
}

/// ENSURE_SOCKET_VALID()
pub fn ensure_valid(vm: &mut Vm, f: &str, sock: &Sock) -> bool {
    if sock.is_invalid() {
        zpp::arg_error(vm, f, 1, "has already been closed");
        return false;
    }
    true
}

// ---------------------------------------------------------------------------
// Addresses (sockaddr_conv.c)
// ---------------------------------------------------------------------------

/// The C string view of a zend_string (up to the first NUL).
pub fn c_bytes(s: &[u8]) -> &[u8] {
    match s.iter().position(|&c| c == 0) {
        Some(p) => &s[..p],
        None => s,
    }
}

pub fn cstring(s: &[u8]) -> CString {
    CString::new(c_bytes(s)).unwrap_or_default()
}

/// php_network_gethostbyname(): the first IPv4 address of `name`.
pub(crate) fn gethostbyname4(name: &[u8]) -> Result<Option<[u8; 4]>, ()> {
    let c = cstring(name);
    let he = unsafe { gethostbyname(c.as_ptr()) };
    if he.is_null() {
        return Err(());
    }
    let he = unsafe { &*he };
    if he.h_addrtype != libc::AF_INET {
        return Ok(None);
    }
    if he.h_addr_list.is_null() {
        return Err(());
    }
    let first = unsafe { *he.h_addr_list };
    if first.is_null() {
        return Err(());
    }
    let mut a = [0u8; 4];
    unsafe {
        std::ptr::copy_nonoverlapping(first as *const u8, a.as_mut_ptr(), 4);
    }
    Ok(Some(a))
}

const MAXFQDNLEN: usize = 255;

/// php_set_inet_addr(): the IPv4 address (network order bytes) of a host
/// name or dotted address.
pub fn set_inet_addr(vm: &mut Vm, f: &str, s: &[u8], sock: &Sock) -> Option<[u8; 4]> {
    let c = cstring(s);
    let mut tmp = [0u8; 4];
    if unsafe {
        inet_pton(
            libc::AF_INET,
            c.as_ptr(),
            tmp.as_mut_ptr() as *mut libc::c_void,
        )
    } == 1
    {
        return Some(tmp);
    }
    let r = if s.len() > MAXFQDNLEN {
        Err(())
    } else {
        gethostbyname4(s)
    };
    match r {
        Ok(Some(a)) => Some(a),
        Ok(None) => {
            vm.emit_warning(&format!(
                "{}(): Host lookup failed: Non AF_INET domain returned on AF_INET socket",
                f
            ));
            None
        }
        Err(()) => {
            sock_error(vm, f, sock, "Host lookup failed", -10000 - h_errno());
            None
        }
    }
}

/// php_set_inet6_addr(): the IPv6 address and scope id.
pub fn set_inet6_addr(vm: &mut Vm, f: &str, s: &[u8], sock: &Sock) -> Option<([u8; 16], u32)> {
    let cs = c_bytes(s);
    let c = cstring(s);
    let mut addr = [0u8; 16];
    if unsafe {
        inet_pton(
            libc::AF_INET6,
            c.as_ptr(),
            addr.as_mut_ptr() as *mut libc::c_void,
        )
    } != 1
    {
        let mut hints: libc::addrinfo = unsafe { std::mem::zeroed() };
        hints.ai_family = libc::AF_INET6;
        hints.ai_flags = libc::AI_V4MAPPED | libc::AI_ADDRCONFIG;
        let mut res: *mut libc::addrinfo = std::ptr::null_mut();
        unsafe {
            libc::getaddrinfo(c.as_ptr(), std::ptr::null(), &hints, &mut res);
        }
        if res.is_null() {
            sock_error(vm, f, sock, "Host lookup failed", -10000 - h_errno());
            return None;
        }
        let ai = unsafe { &*res };
        if ai.ai_family != libc::AF_INET6
            || ai.ai_addrlen as usize != std::mem::size_of::<libc::sockaddr_in6>()
        {
            vm.emit_warning(&format!(
                "{}(): Host lookup failed: Non AF_INET6 domain returned on AF_INET6 socket",
                f
            ));
            unsafe { libc::freeaddrinfo(res) };
            return None;
        }
        let sin6 = unsafe { &*(ai.ai_addr as *const libc::sockaddr_in6) };
        addr = sin6.sin6_addr.s6_addr;
        unsafe { libc::freeaddrinfo(res) };
    }
    let mut scope_id = 0u32;
    if let Some(p) = cs.iter().position(|&ch| ch == b'%') {
        let scope = &cs[p + 1..];
        match goro_core::value::parse_numeric_str(scope, false) {
            Some((goro_core::value::NumStr::Long(l), _)) => {
                if l > 0 && l <= u32::MAX as i64 {
                    scope_id = l as u32;
                }
            }
            _ => {
                if let Some(i) = mcast::string_to_if_index(vm, f, scope) {
                    scope_id = i;
                }
            }
        }
    }
    Some((addr, scope_id))
}

pub fn sockaddr_in(addr: [u8; 4], port: u16) -> libc::sockaddr_in {
    let mut sin: libc::sockaddr_in = unsafe { std::mem::zeroed() };
    sin.sin_family = libc::AF_INET as libc::sa_family_t;
    sin.sin_port = port.to_be();
    sin.sin_addr.s_addr = u32::from_ne_bytes(addr);
    sin
}

pub fn sockaddr_in6(addr: [u8; 16], scope: u32, port: u16) -> libc::sockaddr_in6 {
    let mut sin6: libc::sockaddr_in6 = unsafe { std::mem::zeroed() };
    sin6.sin6_family = libc::AF_INET6 as libc::sa_family_t;
    sin6.sin6_port = port.to_be();
    sin6.sin6_addr.s6_addr = addr;
    sin6.sin6_scope_id = scope;
    sin6
}

/// The raw bytes of a C struct.
pub fn as_bytes<T>(v: &T) -> &[u8] {
    unsafe { std::slice::from_raw_parts(v as *const T as *const u8, std::mem::size_of::<T>()) }
}

/// php_set_inet46_addr(): a sockaddr for the socket's family (the
/// ValueError is thrown for other families: Err(true)).
pub fn set_inet46_addr(vm: &mut Vm, f: &str, s: &[u8], sock: &Sock) -> Option<Vec<u8>> {
    match sock.family.get() {
        libc::AF_INET => {
            let a = set_inet_addr(vm, f, s, sock)?;
            Some(as_bytes(&sockaddr_in(a, 0)).to_vec())
        }
        libc::AF_INET6 => {
            let (a, scope) = set_inet6_addr(vm, f, s, sock)?;
            Some(as_bytes(&sockaddr_in6(a, scope, 0)).to_vec())
        }
        _ => {
            zpp::plain_value_error(
                vm,
                "IP address used in the context of an unexpected type of socket",
            );
            None
        }
    }
}

pub fn inet_ntop4(a: &[u8; 4]) -> String {
    std::net::Ipv4Addr::from(*a).to_string()
}

/// inet_ntop(AF_INET6)
pub fn inet_ntop6(a: &[u8; 16]) -> String {
    let mut buf = [0 as libc::c_char; 64];
    let p = unsafe {
        inet_ntop(
            libc::AF_INET6,
            a.as_ptr() as *const libc::c_void,
            buf.as_mut_ptr(),
            buf.len() as libc::socklen_t,
        )
    };
    if p.is_null() {
        return String::new();
    }
    cstr_to_string(buf.as_ptr())
}

/// The bytes of a sun_path up to its first NUL.
pub fn sun_path_str(path: &[libc::c_char]) -> Vec<u8> {
    path.iter()
        .take_while(|&&c| c != 0)
        .map(|&c| c as u8)
        .collect()
}

// ---------------------------------------------------------------------------
// Registration
// ---------------------------------------------------------------------------

macro_rules! php_functions {
    ($($name:literal => $imp:path),* $(,)?) => {
        fn register_functions(vm: &mut Vm) {
            $(
                {
                    fn w(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
                        Ok($imp(vm, $name, args))
                    }
                    vm.register_function($name.as_bytes(), w);
                }
            )*
        }
    };
}

php_functions! {
    "socket_select" => funcs::socket_select,
    "socket_create_listen" => funcs::socket_create_listen,
    "socket_accept" => funcs::socket_accept,
    "socket_set_nonblock" => funcs::socket_set_nonblock,
    "socket_set_block" => funcs::socket_set_block,
    "socket_listen" => funcs::socket_listen,
    "socket_close" => funcs::socket_close,
    "socket_write" => funcs::socket_write,
    "socket_read" => funcs::socket_read,
    "socket_getsockname" => funcs::socket_getsockname,
    "socket_getpeername" => funcs::socket_getpeername,
    "socket_create" => funcs::socket_create,
    "socket_connect" => funcs::socket_connect,
    "socket_strerror" => funcs::socket_strerror,
    "socket_bind" => funcs::socket_bind,
    "socket_recv" => funcs::socket_recv,
    "socket_send" => funcs::socket_send,
    "socket_recvfrom" => funcs::socket_recvfrom,
    "socket_sendto" => funcs::socket_sendto,
    "socket_get_option" => opts::socket_get_option,
    "socket_getopt" => opts::socket_get_option,
    "socket_set_option" => opts::socket_set_option,
    "socket_setopt" => opts::socket_set_option,
    "socket_create_pair" => funcs::socket_create_pair,
    "socket_shutdown" => funcs::socket_shutdown,
    "socket_atmark" => funcs::socket_atmark,
    "socket_last_error" => funcs::socket_last_error,
    "socket_clear_error" => funcs::socket_clear_error,
    "socket_import_stream" => funcs::socket_import_stream,
    "socket_export_stream" => funcs::socket_export_stream,
    "socket_sendmsg" => conv::socket_sendmsg,
    "socket_recvmsg" => conv::socket_recvmsg,
    "socket_cmsg_space" => conv::socket_cmsg_space,
    "socket_addrinfo_lookup" => funcs::socket_addrinfo_lookup,
    "socket_addrinfo_connect" => funcs::socket_addrinfo_connect,
    "socket_addrinfo_bind" => funcs::socket_addrinfo_bind,
    "socket_addrinfo_explain" => funcs::socket_addrinfo_explain,
}

/// Register all socket extension functions, classes and constants
pub fn register(vm: &mut Vm) {
    vm.register_extension(b"sockets");

    for name in [&b"Socket"[..], &b"AddressInfo"[..]] {
        let mut ce = ClassEntry::new(name.to_vec());
        ce.is_final = true;
        vm.classes.insert(name.to_ascii_lowercase(), ce);
    }

    register_functions(vm);

    for (name, v) in consts::CONSTANTS {
        vm.constants.insert(name.to_vec(), Value::Long(*v));
    }
    // ext/standard's stream socket constants (file.c), which goro's
    // standard extension does not register.
    for (name, v) in [
        (&b"STREAM_IPPROTO_IP"[..], 0),
        (b"STREAM_IPPROTO_TCP", 6),
        (b"STREAM_IPPROTO_UDP", 17),
        (b"STREAM_IPPROTO_ICMP", 1),
        (b"STREAM_IPPROTO_RAW", 255),
    ] {
        vm.constants.entry(name.to_vec()).or_insert(Value::Long(v));
    }
}

/// sockatmark()
pub fn sock_atmark(fd: i32) -> i32 {
    unsafe { sockatmark(fd) }
}
