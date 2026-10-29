//! ext/sockets (PHP manual: Sockets): the `Socket` and `AddressInfo`
//! classes and the socket_* functions over the BSD socket calls of
//! POSIX/Linux (socket(2), bind(2), recvmsg(2), setsockopt(2)...).
//!
//! Messages, argument checks and the error numbers they report were taken
//! from ext/sockets/tests and observed on PHP 8.5. A socket shares its
//! descriptor with the stream socket_export_stream() / socket_import_stream()
//! link it to: the stream owns it then, and closing either side closes it
//! (tests socket_export_stream-4.phpt, socket_import_stream-4.phpt).

mod addr;
mod consts;
mod msg;
mod opts;

use std::cell::Cell;
use std::os::fd::{FromRawFd, OwnedFd, RawFd};

use goro_runtime::{Key, ObjRef, PhpArray, PhpStr, ResRef, Value};

use super::file::{Backing, Stream, new_stream_res};
use crate::class::NativeHooks;
use crate::func::Args;
use crate::registry::Module;
use crate::vm::{R, Vm};

pub static MODULE: Module = Module {
    name: "sockets",
    functions: &[
        (
            "function socket_select(?array &$read, ?array &$write, ?array &$except, ?int $seconds, int $microseconds = 0): int|false",
            socket_select,
        ),
        (
            "function socket_create_listen(int $port, int $backlog = SOMAXCONN): Socket|false",
            socket_create_listen,
        ),
        (
            "function socket_accept(Socket $socket): Socket|false",
            socket_accept,
        ),
        (
            "function socket_set_nonblock(Socket $socket): bool",
            socket_set_nonblock,
        ),
        (
            "function socket_set_block(Socket $socket): bool",
            socket_set_block,
        ),
        (
            "function socket_listen(Socket $socket, int $backlog = 0): bool",
            socket_listen,
        ),
        ("function socket_close(Socket $socket): void", socket_close),
        (
            "function socket_write(Socket $socket, string $data, ?int $length = null): int|false",
            socket_write,
        ),
        (
            "function socket_read(Socket $socket, int $length, int $mode = PHP_BINARY_READ): string|false",
            socket_read,
        ),
        (
            "function socket_getsockname(Socket $socket, &$address, &$port = null): bool",
            socket_getsockname,
        ),
        (
            "function socket_getpeername(Socket $socket, &$address, &$port = null): bool",
            socket_getpeername,
        ),
        (
            "function socket_create(int $domain, int $type, int $protocol): Socket|false",
            socket_create,
        ),
        (
            "function socket_connect(Socket $socket, string $address, ?int $port = null): bool",
            socket_connect,
        ),
        (
            "function socket_strerror(int $error_code): string",
            socket_strerror,
        ),
        (
            "function socket_bind(Socket $socket, string $address, int $port = 0): bool",
            socket_bind,
        ),
        (
            "function socket_recv(Socket $socket, &$data, int $length, int $flags): int|false",
            socket_recv,
        ),
        (
            "function socket_send(Socket $socket, string $data, int $length, int $flags): int|false",
            socket_send,
        ),
        (
            "function socket_recvfrom(Socket $socket, &$data, int $length, int $flags, &$address, &$port = null): int|false",
            socket_recvfrom,
        ),
        (
            "function socket_sendto(Socket $socket, string $data, int $length, int $flags, string $address, ?int $port = null): int|false",
            socket_sendto,
        ),
        (
            "function socket_get_option(Socket $socket, int $level, int $option): array|int|false",
            opts::socket_get_option,
        ),
        (
            "function socket_getopt(Socket $socket, int $level, int $option): array|int|false",
            opts::socket_get_option,
        ),
        (
            "function socket_set_option(Socket $socket, int $level, int $option, $value): bool",
            opts::socket_set_option,
        ),
        (
            "function socket_setopt(Socket $socket, int $level, int $option, $value): bool",
            opts::socket_set_option,
        ),
        (
            "function socket_create_pair(int $domain, int $type, int $protocol, &$pair): bool",
            socket_create_pair,
        ),
        (
            "function socket_shutdown(Socket $socket, int $mode = 2): bool",
            socket_shutdown,
        ),
        (
            "function socket_atmark(Socket $socket): bool",
            socket_atmark,
        ),
        (
            "function socket_last_error(?Socket $socket = null): int",
            socket_last_error,
        ),
        (
            "function socket_clear_error(?Socket $socket = null): void",
            socket_clear_error,
        ),
        (
            "function socket_import_stream($stream): Socket|false",
            socket_import_stream,
        ),
        (
            "function socket_export_stream(Socket $socket)",
            socket_export_stream,
        ),
        (
            "function socket_sendmsg(Socket $socket, array $message, int $flags = 0): int|false",
            msg::socket_sendmsg,
        ),
        (
            "function socket_recvmsg(Socket $socket, array &$message, int $flags = 0): int|false",
            msg::socket_recvmsg,
        ),
        (
            "function socket_cmsg_space(int $level, int $type, int $num = 0): ?int",
            msg::socket_cmsg_space,
        ),
        (
            "function socket_addrinfo_lookup(string $host, ?string $service = null, array $hints = []): array|false",
            addr::socket_addrinfo_lookup,
        ),
        (
            "function socket_addrinfo_connect(AddressInfo $address): Socket|false",
            addr::socket_addrinfo_connect,
        ),
        (
            "function socket_addrinfo_bind(AddressInfo $address): Socket|false",
            addr::socket_addrinfo_bind,
        ),
        (
            "function socket_addrinfo_explain(AddressInfo $address): array",
            addr::socket_addrinfo_explain,
        ),
    ],
    classes: &[
        (
            "socket",
            "<?php #[NotSerializable] #[NotCloneable] final class Socket {
                public function __construct() { throw new Error('Cannot directly construct Socket, use socket_create() instead'); }
            }",
        ),
        (
            "addressinfo",
            "<?php #[NotSerializable] #[NotCloneable] final class AddressInfo {
                public function __construct() { throw new Error('Cannot directly construct AddressInfo, use socket_addrinfo_lookup() instead'); }
            }",
        ),
    ],
    hooks: &[("socket", hooks), ("addressinfo", hooks)],
    constants: consts::CONSTANTS,
    listed: true,
    ..Module::EMPTY
};

/// Distinct objects never compare equal (test bug80723.phpt); they can't
/// be cloned ("Trying to clone an uncloneable object", observed on PHP 8.5).
fn hooks() -> NativeHooks {
    NativeHooks {
        compare: Some(|_, x, y| match (x, y) {
            (Value::Object(a), Value::Object(b)) => {
                Some(Ok(if ObjRef::ptr_eq(a, b) { 0 } else { 1 }))
            }
            _ => None,
        }),
        ..Default::default()
    }
}

thread_local! {
    /// The error of the last failed socket call (socket_last_error()).
    static LAST_ERROR: Cell<i32> = const { Cell::new(0) };
}

pub(crate) fn errno() -> i32 {
    std::io::Error::last_os_error().raw_os_error().unwrap_or(0)
}

/// The C library's text of an error number.
pub(crate) fn strerror(e: i32) -> String {
    super::exec::strerror(e)
}

/// socket_strerror(): resolver errors are below -10000 (`-10000 -
/// h_errno`), the rest are errno values (observed on PHP 8.5).
fn error_text(e: i64) -> String {
    if e < -10000 {
        match -e - 10000 {
            1 => "Unknown host".into(),
            2 => "Host name lookup failure".into(),
            3 => "Unknown server error".into(),
            4 => "No address associated with name".into(),
            _ => "Unknown resolver error".into(),
        }
    } else {
        strerror(e as i32)
    }
}

/// A socket.
pub struct Sk {
    /// The descriptor (owned unless a stream holds it).
    fd: RawFd,
    owned: bool,
    pub family: i32,
    pub stype: i32,
    pub error: i32,
    closed: bool,
    /// The stream sharing the descriptor (imported or exported).
    stream: Option<ResRef>,
}

impl goro_runtime::NativeData for Sk {}

impl Drop for Sk {
    fn drop(&mut self) {
        if self.owned && !self.closed && self.fd >= 0 {
            // SAFETY: our own descriptor.
            unsafe { libc::close(self.fd) };
        }
    }
}

impl Sk {
    fn new(fd: RawFd, family: i32, stype: i32) -> Sk {
        Sk {
            fd,
            owned: true,
            family,
            stype,
            error: 0,
            closed: false,
            stream: None,
        }
    }

    /// The descriptor in use: -1 (EBADF on every call) once the stream
    /// holding it was closed.
    pub fn fd(&self) -> RawFd {
        match &self.stream {
            Some(r) if r.is_closed() => -1,
            _ => self.fd,
        }
    }
}

fn with_native<T: 'static, U>(o: &ObjRef, f: impl FnOnce(&mut T) -> U) -> Option<U> {
    let mut n = o.obj().native.borrow_mut();
    let any: &mut dyn std::any::Any = &mut **n.as_mut()?;
    any.downcast_mut::<T>().map(f)
}

fn new_obj(vm: &mut Vm, class: &str, data: Box<dyn goro_runtime::NativeData>) -> R<Value> {
    let Some(cls) = vm.find_class(class.as_bytes(), false)? else {
        return Err(vm.fatal(&format!("Class \"{class}\" not found")));
    };
    let o = vm.instantiate_raw(&cls)?;
    *o.obj().native.borrow_mut() = Some(data);
    Ok(Value::Object(o))
}

/// A new Socket object for a descriptor we own.
pub(crate) fn new_socket(vm: &mut Vm, fd: RawFd, family: i32, stype: i32) -> R<Value> {
    new_obj(vm, "Socket", Box::new(Sk::new(fd, family, stype)))
}

/// Runs `f` on the socket of object argument `i`; an Error for a closed
/// one ("Argument #1 ($socket) has already been closed").
pub(crate) fn with_sk<U>(vm: &mut Vm, a: &Args, i: usize, f: impl FnOnce(&mut Sk) -> U) -> R<U> {
    let o = a.obj(i);
    let r = o.as_ref().and_then(|o| {
        with_native::<Sk, Option<U>>(o, |s| if s.closed { None } else { Some(f(s)) })
    });
    match r {
        Some(Some(u)) => Ok(u),
        _ => vm.fthrow(
            a,
            "Error",
            &format!("Argument #{} ($socket) has already been closed", i + 1),
        ),
    }
}

/// The descriptor, family and type of socket argument `i`.
pub(crate) fn sk_info(vm: &mut Vm, a: &Args, i: usize) -> R<(RawFd, i32, i32)> {
    with_sk(vm, a, i, |s| (s.fd(), s.family, s.stype))
}

/// Records a failure on the socket and globally, and warns
/// `"{what} [{errno}]: {text}"`, except for EAGAIN and EINPROGRESS (which
/// only set the error; test socket_sentto_recvfrom_unix.phpt).
pub(crate) fn sock_error(vm: &mut Vm, a: &Args, i: usize, what: &str, e: i32) -> R<()> {
    if let Some(o) = a.obj(i) {
        with_native::<Sk, ()>(&o, |s| s.error = e);
    }
    LAST_ERROR.with(|c| c.set(e));
    if e != libc::EAGAIN && e != libc::EINPROGRESS {
        vm.fwarn(a, &format!("{what} [{e}]: {}", strerror(e)))?;
    }
    Ok(())
}

/// Records an error without a socket (creation failures).
pub(crate) fn global_error(vm: &mut Vm, a: &Args, what: &str, e: i32) -> R<()> {
    LAST_ERROR.with(|c| c.set(e));
    vm.fwarn(a, &format!("{what} [{e}]: {}", error_text(e as i64)))
}

/// A failed lookup's error ("Host lookup failed [-10001]: Unknown host").
pub(crate) fn lookup_error(vm: &mut Vm, a: &Args, i: Option<usize>, code: i32) -> R<()> {
    if let Some(o) = i.and_then(|i| a.obj(i)) {
        with_native::<Sk, ()>(&o, |s| s.error = code);
    }
    LAST_ERROR.with(|c| c.set(code));
    vm.fwarn(
        a,
        &format!("Host lookup failed [{code}]: {}", error_text(code as i64)),
    )
}

fn check_port(vm: &mut Vm, a: &Args, i: usize, port: i64) -> R<()> {
    if !(0..=65535).contains(&port) {
        return vm.fthrow(
            a,
            "ValueError",
            &format!("Argument #{} ($port) must be between 0 and 65535", i + 1),
        );
    }
    Ok(())
}

const DOMAIN_ERR: &str = "must be one of AF_UNIX, AF_PACKET, AF_INET6, or AF_INET";
const TYPE_ERR: &str = "must be one of SOCK_STREAM, SOCK_DGRAM, SOCK_SEQPACKET, SOCK_RAW, or SOCK_RDM optionally OR'ed with SOCK_CLOEXEC, SOCK_NONBLOCK";

fn valid_type(t: i64) -> bool {
    // Types up to SOCK_PACKET (10) reach socket(2), which refuses the ones
    // it doesn't support (observed on PHP 8.5: 0, 6, 7 and 10 give
    // "Socket type not supported").
    let base = t & !((libc::SOCK_CLOEXEC | libc::SOCK_NONBLOCK) as i64);
    (0..=10).contains(&base)
}

fn base_type(t: i32) -> i32 {
    t & !(libc::SOCK_CLOEXEC | libc::SOCK_NONBLOCK)
}

fn socket_create(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (domain, ty, proto) = (a.int(0), a.int(1), a.int(2));
    if ![
        libc::AF_UNIX,
        libc::AF_PACKET,
        libc::AF_INET6,
        libc::AF_INET,
    ]
    .iter()
    .any(|&d| d as i64 == domain)
    {
        return vm.fthrow(
            a,
            "ValueError",
            &format!("Argument #1 ($domain) {DOMAIN_ERR}"),
        );
    }
    if !valid_type(ty) {
        return vm.fthrow(a, "ValueError", &format!("Argument #2 ($type) {TYPE_ERR}"));
    }
    // SAFETY: plain socket(2).
    let fd = unsafe { libc::socket(domain as i32, ty as i32, proto as i32) };
    if fd < 0 {
        global_error(vm, a, "Unable to create socket", errno())?;
        return Ok(Value::Bool(false));
    }
    new_socket(vm, fd, domain as i32, base_type(ty as i32))
}

fn socket_create_pair(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (domain, ty, proto) = (a.int(0), a.int(1), a.int(2));
    if ![libc::AF_UNIX, libc::AF_INET6, libc::AF_INET]
        .iter()
        .any(|&d| d as i64 == domain)
    {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #1 ($domain) must be one of AF_UNIX, AF_INET6, or AF_INET",
        );
    }
    if !valid_type(ty) {
        return vm.fthrow(a, "ValueError", &format!("Argument #2 ($type) {TYPE_ERR}"));
    }
    let mut fds = [0 as libc::c_int; 2];
    // SAFETY: a two-element array for socketpair(2).
    let r = unsafe { libc::socketpair(domain as i32, ty as i32, proto as i32, fds.as_mut_ptr()) };
    if r != 0 {
        global_error(vm, a, "Unable to create socket pair", errno())?;
        return Ok(Value::Bool(false));
    }
    let s0 = new_socket(vm, fds[0], domain as i32, base_type(ty as i32))?;
    let s1 = new_socket(vm, fds[1], domain as i32, base_type(ty as i32))?;
    let mut arr = PhpArray::new();
    arr.insert(Key::Int(0), s0);
    arr.insert(Key::Int(1), s1);
    a.set_ref(3, Value::Array(arr));
    Ok(Value::Bool(true))
}

fn socket_create_listen(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let port = a.int(0);
    let backlog = a.int_or(1, libc::SOMAXCONN as i64);
    check_port(vm, a, 0, port)?;
    // SAFETY: plain socket(2).
    let fd = unsafe { libc::socket(libc::AF_INET, libc::SOCK_STREAM, 0) };
    if fd < 0 {
        global_error(vm, a, "Unable to create socket", errno())?;
        return Ok(Value::Bool(false));
    }
    let one: libc::c_int = 1;
    // SAFETY: an int option on our descriptor.
    unsafe {
        libc::setsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_REUSEADDR,
            (&one as *const libc::c_int).cast(),
            4,
        )
    };
    let (st, len) = addr::inet_sockaddr(std::net::Ipv4Addr::UNSPECIFIED, port as u16);
    // SAFETY: a valid sockaddr_in.
    if unsafe { libc::bind(fd, (&st as *const libc::sockaddr_storage).cast(), len) } != 0 {
        let e = errno();
        unsafe { libc::close(fd) };
        global_error(vm, a, "unable to bind to given address", e)?;
        return Ok(Value::Bool(false));
    }
    if unsafe { libc::listen(fd, backlog as i32) } != 0 {
        let e = errno();
        unsafe { libc::close(fd) };
        global_error(vm, a, "unable to listen on socket", e)?;
        return Ok(Value::Bool(false));
    }
    new_socket(vm, fd, libc::AF_INET, libc::SOCK_STREAM)
}

fn socket_accept(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (fd, family, stype) = sk_info(vm, a, 0)?;
    vm.out.flush();
    // SAFETY: accept(2) without the peer address.
    let n = unsafe { libc::accept(fd, std::ptr::null_mut(), std::ptr::null_mut()) };
    if n < 0 {
        sock_error(vm, a, 0, "unable to accept incoming connection", errno())?;
        return Ok(Value::Bool(false));
    }
    new_socket(vm, n, family, stype)
}

fn set_nonblocking(fd: RawFd, on: bool) -> bool {
    // SAFETY: fcntl(2) flag calls.
    unsafe {
        let fl = libc::fcntl(fd, libc::F_GETFL);
        if fl < 0 {
            return false;
        }
        let nf = if on {
            fl | libc::O_NONBLOCK
        } else {
            fl & !libc::O_NONBLOCK
        };
        nf == fl || libc::fcntl(fd, libc::F_SETFL, nf) == 0
    }
}

fn is_nonblocking(fd: RawFd) -> bool {
    // SAFETY: fcntl(2) F_GETFL.
    let fl = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    fl >= 0 && fl & libc::O_NONBLOCK != 0
}

fn set_block_mode(vm: &mut Vm, a: &Args, nonblock: bool) -> R<Value> {
    let fd = sk_info(vm, a, 0)?.0;
    if !set_nonblocking(fd, nonblock) {
        let what = if nonblock {
            "unable to set nonblocking mode"
        } else {
            "unable to set blocking mode"
        };
        sock_error(vm, a, 0, what, errno())?;
        return Ok(Value::Bool(false));
    }
    // The linked stream follows (its own reads poll the descriptor).
    let stream = with_sk(vm, a, 0, |s| s.stream.clone())?;
    if let Some(r) = stream {
        with_sock_res(&r, |s| s.blocking = !nonblock);
    }
    Ok(Value::Bool(true))
}

fn socket_set_nonblock(vm: &mut Vm, a: &mut Args) -> R<Value> {
    set_block_mode(vm, a, true)
}

fn socket_set_block(vm: &mut Vm, a: &mut Args) -> R<Value> {
    set_block_mode(vm, a, false)
}

fn socket_listen(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let backlog = a.int_or(1, 0);
    let fd = sk_info(vm, a, 0)?.0;
    // SAFETY: listen(2).
    if unsafe { libc::listen(fd, backlog as i32) } != 0 {
        sock_error(vm, a, 0, "unable to listen on socket", errno())?;
        return Ok(Value::Bool(false));
    }
    Ok(Value::Bool(true))
}

/// Closes the stream resource `r` as fclose() does.
fn close_stream_res(vm: &mut Vm, r: &ResRef) -> R<()> {
    if r.is_closed() {
        return Ok(());
    }
    let cell = {
        let data = r.res().data.borrow();
        data.as_ref()
            .and_then(|d| d.downcast_ref::<std::cell::RefCell<Stream>>())
            .map(|c| c as *const std::cell::RefCell<Stream>)
    };
    if let Some(cell) = cell {
        // SAFETY: `r` holds the resource for the call.
        if let Ok(mut s) = unsafe { (*cell).try_borrow_mut() } {
            super::streams::close_stream(vm, &mut s)?;
        }
    }
    r.close();
    Ok(())
}

fn socket_close(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (stream, fd, owned) = with_sk(vm, a, 0, |s| {
        s.closed = true;
        (s.stream.take(), s.fd, s.owned)
    })?;
    match stream {
        Some(r) => close_stream_res(vm, &r)?,
        None => {
            if owned && fd >= 0 {
                // SAFETY: our own descriptor, closed once.
                unsafe { libc::close(fd) };
            }
        }
    }
    Ok(Value::Null)
}

fn socket_write(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let data = a.str(1);
    let len = match a.opt_int(2) {
        Some(n) if n < 0 => {
            return vm.fthrow(
                a,
                "ValueError",
                "Argument #3 ($length) must be greater than or equal to 0",
            );
        }
        Some(n) => (n as u64).min(data.len() as u64) as usize,
        None => data.len(),
    };
    let fd = sk_info(vm, a, 0)?.0;
    // SAFETY: data is valid for `len` bytes.
    let n = unsafe { libc::write(fd, data.as_bytes().as_ptr().cast(), len) };
    if n < 0 {
        sock_error(vm, a, 0, "unable to write to socket", errno())?;
        return Ok(Value::Bool(false));
    }
    Ok(Value::Int(n as i64))
}

/// Reads one byte; Ok(None) at the end.
fn read_byte(fd: RawFd) -> Result<Option<u8>, i32> {
    let mut b = 0u8;
    loop {
        // SAFETY: one-byte buffer.
        let n = unsafe { libc::read(fd, (&mut b as *mut u8).cast(), 1) };
        if n == 1 {
            return Ok(Some(b));
        }
        if n == 0 {
            return Ok(None);
        }
        let e = errno();
        if e != libc::EINTR {
            return Err(e);
        }
    }
}

fn socket_read(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let len = a.int(1);
    let mode = a.int_or(2, 2);
    let fd = sk_info(vm, a, 0)?.0;
    if len <= 0 || len > i32::MAX as i64 {
        return Ok(Value::Bool(false));
    }
    vm.out.flush();
    let len = len as usize;
    if mode == 1 {
        // PHP_NORMAL_READ: up to a "\n" or "\r" (included; observed on
        // PHP 8.5), one byte at a time.
        let mut out = Vec::new();
        while out.len() < len {
            match read_byte(fd) {
                Ok(Some(b)) => {
                    out.push(b);
                    if b == b'\n' || b == b'\r' {
                        break;
                    }
                }
                Ok(None) => break,
                Err(e) => {
                    if out.is_empty() {
                        sock_error(vm, a, 0, "unable to read from socket", e)?;
                        return Ok(Value::Bool(false));
                    }
                    break;
                }
            }
        }
        return Ok(Value::Str(PhpStr::from(out.as_slice())));
    }
    let mut buf = vec![0u8; len.min(1 << 26)];
    // SAFETY: buf is valid for writes of its length.
    let n = unsafe { libc::read(fd, buf.as_mut_ptr().cast(), buf.len()) };
    if n < 0 {
        sock_error(vm, a, 0, "unable to read from socket", errno())?;
        return Ok(Value::Bool(false));
    }
    buf.truncate(n as usize);
    Ok(Value::Str(PhpStr::from(buf.as_slice())))
}

fn name_common(vm: &mut Vm, a: &mut Args, peer: bool) -> R<Value> {
    let (fd, _, _) = sk_info(vm, a, 0)?;
    // SAFETY: zeroed storage.
    let mut st: libc::sockaddr_storage = unsafe { std::mem::zeroed() };
    let mut len = std::mem::size_of::<libc::sockaddr_storage>() as libc::socklen_t;
    let p = (&mut st as *mut libc::sockaddr_storage).cast();
    // SAFETY: storage of the given length.
    let r = unsafe {
        if peer {
            libc::getpeername(fd, p, &mut len)
        } else {
            libc::getsockname(fd, p, &mut len)
        }
    };
    if r != 0 {
        let what = if peer {
            "unable to retrieve peer name"
        } else {
            "unable to retrieve socket name"
        };
        sock_error(vm, a, 0, what, errno())?;
        return Ok(Value::Bool(false));
    }
    match addr::decode(&st, len) {
        addr::Decoded::Inet(host, port) => {
            a.set_ref(1, Value::from(host.as_str()));
            if a.ref_at(2).is_some() {
                a.set_ref(2, Value::Int(port as i64));
            }
        }
        addr::Decoded::Unix(path) => a.set_ref(1, Value::Str(PhpStr::from(path.as_slice()))),
        addr::Decoded::Packet(name, index) => {
            a.set_ref(1, Value::Str(PhpStr::from(name.as_slice())));
            if a.ref_at(2).is_some() {
                a.set_ref(2, Value::Int(index));
            }
        }
        addr::Decoded::Other => return Ok(Value::Bool(false)),
    }
    Ok(Value::Bool(true))
}

fn socket_getsockname(vm: &mut Vm, a: &mut Args) -> R<Value> {
    name_common(vm, a, false)
}

fn socket_getpeername(vm: &mut Vm, a: &mut Args) -> R<Value> {
    name_common(vm, a, true)
}

fn socket_connect(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let host = a.str(1);
    let (fd, family, _) = sk_info(vm, a, 0)?;
    let port = a.opt_int(2);
    let target = match family {
        libc::AF_INET | libc::AF_INET6 => {
            let Some(port) = port else {
                let fam = if family == libc::AF_INET {
                    "AF_INET"
                } else {
                    "AF_INET6"
                };
                return vm.fthrow(
                    a,
                    "ValueError",
                    &format!("Argument #3 ($port) cannot be null when the socket type is {fam}"),
                );
            };
            match addr::resolve_for(vm, a, Some(0), family, host.as_bytes(), port as u16)? {
                Some(t) => t,
                None => return Ok(Value::Bool(false)),
            }
        }
        libc::AF_UNIX => match addr::unix_sockaddr(host.as_bytes()) {
            Some(t) => t,
            None => {
                return vm.fthrow(
                    a,
                    "ValueError",
                    &format!(
                        "Argument #2 ($address) must be less than {}",
                        addr::SUN_PATH
                    ),
                );
            }
        },
        _ => {
            sock_error(vm, a, 0, "unable to connect", libc::EAFNOSUPPORT)?;
            return Ok(Value::Bool(false));
        }
    };
    vm.out.flush();
    // SAFETY: a sockaddr of the given length.
    let r = unsafe {
        libc::connect(
            fd,
            (&target.0 as *const libc::sockaddr_storage).cast(),
            target.1,
        )
    };
    if r != 0 {
        sock_error(vm, a, 0, "unable to connect", errno())?;
        return Ok(Value::Bool(false));
    }
    Ok(Value::Bool(true))
}

fn socket_bind(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let host = a.str(1);
    let port = a.int_or(2, 0);
    let (fd, family, _) = sk_info(vm, a, 0)?;
    let target = match family {
        libc::AF_INET | libc::AF_INET6 => {
            check_port(vm, a, 2, port)?;
            match addr::resolve_for(vm, a, Some(0), family, host.as_bytes(), port as u16)? {
                Some(t) => t,
                None => return Ok(Value::Bool(false)),
            }
        }
        libc::AF_UNIX => match addr::unix_sockaddr(host.as_bytes()) {
            Some(t) => t,
            None => {
                return vm.fthrow(
                    a,
                    "ValueError",
                    &format!(
                        "Argument #2 ($address) must be less than {}",
                        addr::SUN_PATH
                    ),
                );
            }
        },
        libc::AF_PACKET => match addr::packet_sockaddr(host.as_bytes()) {
            Some(t) => t,
            None => {
                sock_error(vm, a, 0, "Unable to bind address", libc::ENODEV)?;
                return Ok(Value::Bool(false));
            }
        },
        _ => {
            sock_error(vm, a, 0, "Unable to bind address", libc::EAFNOSUPPORT)?;
            return Ok(Value::Bool(false));
        }
    };
    // SAFETY: a sockaddr of the given length.
    let r = unsafe {
        libc::bind(
            fd,
            (&target.0 as *const libc::sockaddr_storage).cast(),
            target.1,
        )
    };
    if r != 0 {
        sock_error(vm, a, 0, "Unable to bind address", errno())?;
        return Ok(Value::Bool(false));
    }
    Ok(Value::Bool(true))
}

fn socket_strerror(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let e = a.int(0);
    if e < i32::MIN as i64 || e > i32::MAX as i64 {
        return vm.fthrow(
            a,
            "ValueError",
            &format!(
                "Argument #1 ($error_code) must be between {} and {}",
                i32::MIN,
                i32::MAX
            ),
        );
    }
    Ok(Value::from(error_text(e).as_str()))
}

fn socket_recv(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let len = a.int(2);
    let flags = a.int(3) as i32;
    let fd = sk_info(vm, a, 0)?.0;
    if len < 1 || len > i32::MAX as i64 {
        return Ok(Value::Bool(false));
    }
    vm.out.flush();
    let mut buf = vec![0u8; (len as usize).min(1 << 26)];
    // SAFETY: buf is valid for writes of its length.
    let n = unsafe { libc::recv(fd, buf.as_mut_ptr().cast(), buf.len(), flags) };
    if n < 0 {
        let e = errno();
        a.set_ref(1, Value::Null);
        sock_error(vm, a, 0, "Unable to read from socket", e)?;
        return Ok(Value::Bool(false));
    }
    if n == 0 {
        // Nothing read: the variable becomes null (observed on PHP 8.5).
        a.set_ref(1, Value::Null);
    } else {
        buf.truncate(n as usize);
        a.set_ref(1, Value::Str(PhpStr::from(buf.as_slice())));
    }
    Ok(Value::Int(n as i64))
}

fn socket_send(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let data = a.str(1);
    let len = a.int(2);
    let flags = a.int(3) as i32;
    if len < 0 {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #3 ($length) must be greater than or equal to 0",
        );
    }
    let fd = sk_info(vm, a, 0)?.0;
    let n = (len as u64).min(data.len() as u64) as usize;
    // SAFETY: data is valid for `n` bytes.
    let r = unsafe { libc::send(fd, data.as_bytes().as_ptr().cast(), n, flags) };
    if r < 0 {
        sock_error(vm, a, 0, "Unable to write to socket", errno())?;
        return Ok(Value::Bool(false));
    }
    Ok(Value::Int(r as i64))
}

fn socket_recvfrom(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let len = a.int(2);
    let flags = a.int(3) as i32;
    let (fd, family, _) = sk_info(vm, a, 0)?;
    if (family == libc::AF_INET || family == libc::AF_INET6) && a.ref_at(5).is_none() {
        return vm.throw(
            "ArgumentCountError",
            &format!("Wrong parameter count for {}()", a.fname()),
        );
    }
    if len < 1 || len > i32::MAX as i64 {
        return Ok(Value::Bool(false));
    }
    vm.out.flush();
    let mut buf = vec![0u8; (len as usize).min(1 << 26)];
    // SAFETY: zeroed storage.
    let mut st: libc::sockaddr_storage = unsafe { std::mem::zeroed() };
    let mut slen = std::mem::size_of::<libc::sockaddr_storage>() as libc::socklen_t;
    // SAFETY: buffers valid for their lengths.
    let n = unsafe {
        libc::recvfrom(
            fd,
            buf.as_mut_ptr().cast(),
            buf.len(),
            flags,
            (&mut st as *mut libc::sockaddr_storage).cast(),
            &mut slen,
        )
    };
    if n < 0 {
        sock_error(vm, a, 0, "unable to recvfrom", errno())?;
        return Ok(Value::Bool(false));
    }
    buf.truncate(n as usize);
    a.set_ref(1, Value::Str(PhpStr::from(buf.as_slice())));
    match addr::decode(&st, slen) {
        addr::Decoded::Inet(host, port) => {
            a.set_ref(4, Value::from(host.as_str()));
            a.set_ref(5, Value::Int(port as i64));
        }
        addr::Decoded::Unix(p) => a.set_ref(4, Value::Str(PhpStr::from(p.as_slice()))),
        _ => a.set_ref(4, Value::from("")),
    }
    Ok(Value::Int(n as i64))
}

fn socket_sendto(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let data = a.str(1);
    let len = a.int(2);
    let flags = a.int(3) as i32;
    let host = a.str(4);
    let port = a.opt_int(5);
    if len < 0 {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #3 ($length) must be greater than or equal to 0",
        );
    }
    let (fd, family, _) = sk_info(vm, a, 0)?;
    let target = match family {
        libc::AF_INET | libc::AF_INET6 => {
            let Some(port) = port else {
                let fam = if family == libc::AF_INET {
                    "AF_INET"
                } else {
                    "AF_INET6"
                };
                return vm.fthrow(
                    a,
                    "ValueError",
                    &format!("Argument #6 ($port) cannot be null when the socket type is {fam}"),
                );
            };
            check_port(vm, a, 5, port)?;
            match addr::resolve_for(vm, a, Some(0), family, host.as_bytes(), port as u16)? {
                Some(t) => t,
                None => return Ok(Value::Bool(false)),
            }
        }
        libc::AF_UNIX => match addr::unix_sockaddr(host.as_bytes()) {
            Some(t) => t,
            None => {
                return vm.fthrow(
                    a,
                    "ValueError",
                    &format!(
                        "Argument #5 ($address) must be less than {}",
                        addr::SUN_PATH
                    ),
                );
            }
        },
        _ => {
            sock_error(vm, a, 0, "Unable to write to socket", libc::EAFNOSUPPORT)?;
            return Ok(Value::Bool(false));
        }
    };
    let n = (len as u64).min(data.len() as u64) as usize;
    // SAFETY: data valid for `n` bytes, a sockaddr of the given length.
    let r = unsafe {
        libc::sendto(
            fd,
            data.as_bytes().as_ptr().cast(),
            n,
            flags,
            (&target.0 as *const libc::sockaddr_storage).cast(),
            target.1,
        )
    };
    if r < 0 {
        sock_error(vm, a, 0, "Unable to write to socket", errno())?;
        return Ok(Value::Bool(false));
    }
    Ok(Value::Int(r as i64))
}

fn socket_shutdown(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let how = a.int_or(1, 2);
    let fd = sk_info(vm, a, 0)?.0;
    if !(0..=2).contains(&how) {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #2 ($mode) must be one of SHUT_RD, SHUT_WR or SHUT_RDWR",
        );
    }
    // SAFETY: shutdown(2).
    if unsafe { libc::shutdown(fd, how as i32) } != 0 {
        sock_error(vm, a, 0, "Unable to shutdown socket", errno())?;
        return Ok(Value::Bool(false));
    }
    Ok(Value::Bool(true))
}

/// The ioctl asking whether the out-of-band mark is reached (Linux
/// sockios: 0x8905).
const SIOCATMARK: libc::Ioctl = 0x8905;

fn socket_atmark(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let fd = sk_info(vm, a, 0)?.0;
    let mut v: libc::c_int = 0;
    // SAFETY: SIOCATMARK writes one int.
    if unsafe { libc::ioctl(fd, SIOCATMARK, &mut v) } < 0 {
        sock_error(vm, a, 0, "Unable to apply SIOCATMARK", errno())?;
        return Ok(Value::Bool(false));
    }
    Ok(Value::Bool(v != 0))
}

fn socket_last_error(vm: &mut Vm, a: &mut Args) -> R<Value> {
    if a.obj(0).is_some() {
        let e = with_sk(vm, a, 0, |s| s.error)?;
        return Ok(Value::Int(e as i64));
    }
    Ok(Value::Int(LAST_ERROR.with(|c| c.get()) as i64))
}

fn socket_clear_error(vm: &mut Vm, a: &mut Args) -> R<Value> {
    if a.obj(0).is_some() {
        with_sk(vm, a, 0, |s| s.error = 0)?;
    } else {
        LAST_ERROR.with(|c| c.set(0));
    }
    Ok(Value::Null)
}

/// Runs `f` on the socket stream behind a stream resource.
fn with_sock_res<T>(r: &ResRef, f: impl FnOnce(&mut super::sockets::Sock) -> T) -> Option<T> {
    let data = r.res().data.borrow();
    let cell = data
        .as_ref()?
        .downcast_ref::<std::cell::RefCell<Stream>>()?;
    let mut s = cell.try_borrow_mut().ok()?;
    match &mut s.backing {
        Backing::Native(n) => n
            .as_any_mut()
            .and_then(|x| x.downcast_mut::<super::sockets::Sock>())
            .map(f),
        _ => None,
    }
}

/// The descriptor of a stream resource (sockets, files, pipes and the
/// standard streams), for SCM_RIGHTS.
pub(crate) fn stream_fd(r: &ResRef) -> Option<RawFd> {
    use std::os::fd::AsRawFd;
    let data = r.res().data.borrow();
    let cell = data
        .as_ref()?
        .downcast_ref::<std::cell::RefCell<Stream>>()?;
    let s = cell.try_borrow().ok()?;
    match &s.backing {
        Backing::File(f) => Some(f.as_raw_fd()),
        Backing::Stdin => Some(0),
        Backing::Stdout => Some(1),
        Backing::Stderr => Some(2),
        Backing::Native(n) => n.raw_fd(),
        _ => None,
    }
}

fn socket_import_stream(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let v = a.val(0);
    let Value::Resource(r) = &v else {
        return vm.fthrow(
            a,
            "TypeError",
            &format!(
                "Argument #1 ($stream) must be of type resource, {} given",
                Vm::type_name_given(&v)
            ),
        );
    };
    let info = {
        let data = r.res().data.borrow();
        data.as_ref()
            .and_then(|d| d.downcast_ref::<std::cell::RefCell<Stream>>())
            .and_then(|c| c.try_borrow().ok().map(|s| s.stream_type))
    };
    let Some(stype_name) = info else {
        return vm.fthrow(
            a,
            "TypeError",
            "supplied resource is not a valid stream resource",
        );
    };
    let Some((fd, blocking)) = with_sock_res(r, |s| {
        use std::os::fd::AsRawFd;
        (s.fd.as_raw_fd(), s.blocking)
    }) else {
        vm.fwarn(
            a,
            &format!("Cannot represent a stream of type {stype_name} as a Socket Descriptor"),
        )?;
        return Ok(Value::Bool(false));
    };
    let (family, stype) = fd_family_type(fd);
    // The socket's blocking mode follows the stream's.
    set_nonblocking(fd, !blocking);
    let mut sk = Sk::new(fd, family, stype);
    sk.owned = false;
    sk.stream = Some(r.clone());
    new_obj(vm, "Socket", Box::new(sk))
}

/// The family (SO_DOMAIN) and type (SO_TYPE) of a socket descriptor.
fn fd_family_type(fd: RawFd) -> (i32, i32) {
    let get = |opt| {
        let mut v: libc::c_int = 0;
        let mut l = 4 as libc::socklen_t;
        // SAFETY: an int option.
        unsafe {
            libc::getsockopt(
                fd,
                libc::SOL_SOCKET,
                opt,
                (&mut v as *mut libc::c_int).cast(),
                &mut l,
            )
        };
        v
    };
    (get(libc::SO_DOMAIN), get(libc::SO_TYPE))
}

fn socket_export_stream(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let existing = with_sk(vm, a, 0, |s| s.stream.clone())?;
    if let Some(r) = existing {
        return Ok(Value::Resource(r));
    }
    let (fd, family, stype) = sk_info(vm, a, 0)?;
    use super::sockets::Kind;
    let (kind, type_name, uri): (Kind, &'static str, &[u8]) = match (family, stype) {
        (libc::AF_INET | libc::AF_INET6, libc::SOCK_STREAM) => {
            (Kind::Tcp, "tcp_socket/ssl", b"tcp://")
        }
        (libc::AF_INET | libc::AF_INET6, libc::SOCK_DGRAM) => (Kind::Udp, "udp_socket", b"udp://"),
        (libc::AF_UNIX, libc::SOCK_STREAM) => (Kind::Unix, "unix_socket", b"unix://"),
        (libc::AF_UNIX, libc::SOCK_DGRAM) => (Kind::Udg, "udg_socket", b"udg://"),
        _ => (Kind::Pair, "generic_socket", b""),
    };
    let blocking = !is_nonblocking(fd);
    // SAFETY: the stream takes over the descriptor; the socket keeps
    // using it without owning it.
    let ofd = unsafe { OwnedFd::from_raw_fd(fd) };
    let mut sock = super::sockets::Sock::new(ofd, kind);
    sock.blocking = blocking;
    let r = new_stream_res(Stream::new(
        Backing::Native(Box::new(sock)),
        b"r+",
        uri,
        "",
        type_name,
    ));
    with_sk(vm, a, 0, |s| {
        s.owned = false;
        s.stream = Some(r.clone());
    })?;
    Ok(Value::Resource(r))
}

/// One readiness set of socket_select(): its sockets (key, object, fd).
type SelSet = Vec<(Key, Value, RawFd)>;

fn select_set(vm: &mut Vm, a: &Args, i: usize, name: &str) -> R<Option<SelSet>> {
    let v = a.val(i);
    let Value::Array(arr) = v else {
        return Ok(None);
    };
    let mut out = Vec::new();
    for (k, e) in arr.iter() {
        let e = e.deref();
        let fd = match &e {
            Value::Object(o) => with_native::<Sk, RawFd>(o, |s| if s.closed { -1 } else { s.fd() }),
            _ => None,
        };
        let Some(fd) = fd else {
            return vm.fthrow(
                a,
                "TypeError",
                &format!(
                    "Argument #{} (${name}) must only have elements of type Socket, {} given",
                    i + 1,
                    Vm::type_name_given(&e)
                ),
            );
        };
        out.push((k.to_owned(), e, fd));
    }
    Ok(Some(out))
}

fn socket_select(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let r = select_set(vm, a, 0, "read")?;
    let w = select_set(vm, a, 1, "write")?;
    let e = select_set(vm, a, 2, "except")?;
    if r.is_none() && w.is_none() && e.is_none() {
        return vm.fthrow(
            a,
            "ValueError",
            "At least one array argument must be passed",
        );
    }
    let sec = a.opt_int(3);
    let usec = a.int_or(4, 0);
    let timeout_ms: i32 = match sec {
        None => -1,
        Some(s) => {
            if s < 0 || usec < 0 {
                LAST_ERROR.with(|c| c.set(libc::EINVAL));
                vm.fwarn(
                    a,
                    &format!(
                        "Unable to select [{}]: {}",
                        libc::EINVAL,
                        strerror(libc::EINVAL)
                    ),
                )?;
                return Ok(Value::Bool(false));
            }
            let total = (s as i128) * 1000 + (usec as i128) / 1000;
            total.min(i32::MAX as i128) as i32
        }
    };
    let mut pfds: Vec<libc::pollfd> = Vec::new();
    let mut slot = |fd: RawFd, ev: i16| -> usize {
        if let Some(p) = pfds.iter().position(|p| p.fd == fd) {
            pfds[p].events |= ev;
            p
        } else {
            pfds.push(libc::pollfd {
                fd,
                events: ev,
                revents: 0,
            });
            pfds.len() - 1
        }
    };
    let ri: Vec<usize> = r
        .iter()
        .flatten()
        .map(|x| slot(x.2, libc::POLLIN))
        .collect();
    let wi: Vec<usize> = w
        .iter()
        .flatten()
        .map(|x| slot(x.2, libc::POLLOUT))
        .collect();
    let ei: Vec<usize> = e
        .iter()
        .flatten()
        .map(|x| slot(x.2, libc::POLLPRI))
        .collect();
    vm.out.flush();
    let n = loop {
        // SAFETY: a valid pollfd array.
        let n = unsafe { libc::poll(pfds.as_mut_ptr(), pfds.len() as libc::nfds_t, timeout_ms) };
        if n < 0 && errno() == libc::EINTR {
            continue;
        }
        break n;
    };
    if n < 0 {
        let e = errno();
        LAST_ERROR.with(|c| c.set(e));
        vm.fwarn(a, &format!("Unable to select [{e}]: {}", strerror(e)))?;
        return Ok(Value::Bool(false));
    }
    let mut count = 0i64;
    let mut rebuild = |set: &Option<SelSet>, idx: &[usize], mask: i16, arg: usize| {
        let Some(set) = set else {
            return;
        };
        let mut out = PhpArray::new();
        for (j, (k, v, fd)) in set.iter().enumerate() {
            let rev = pfds[idx[j]].revents;
            if *fd >= 0 && rev & mask != 0 {
                out.insert(k.clone(), v.clone());
                count += 1;
            }
        }
        a.set_ref(arg, Value::Array(out));
    };
    let bad = libc::POLLHUP | libc::POLLERR | libc::POLLNVAL;
    rebuild(&r, &ri, libc::POLLIN | bad, 0);
    rebuild(&w, &wi, libc::POLLOUT | libc::POLLERR | libc::POLLNVAL, 1);
    rebuild(&e, &ei, libc::POLLPRI, 2);
    Ok(Value::Int(count))
}

/// A socket object's descriptor (SCM_RIGHTS data).
pub(crate) fn object_fd(v: &Value) -> Option<RawFd> {
    match v {
        Value::Object(o) => with_native::<Sk, RawFd>(o, |s| s.fd()),
        _ => None,
    }
}

/// A stream resource for a received descriptor.
pub(crate) fn fd_stream(fd: RawFd) -> Value {
    // SAFETY: a descriptor received through SCM_RIGHTS, now ours.
    let f = unsafe { std::fs::File::from_raw_fd(fd) };
    Value::Resource(new_stream_res(Stream::new(
        Backing::File(f),
        b"r+",
        b"",
        "",
        "STDIO",
    )))
}

/// A decimal integer string ("5"), as interface indexes may be given.
pub(crate) fn numeric_int(b: &[u8]) -> Option<i64> {
    std::str::from_utf8(b).ok()?.trim().parse().ok()
}
