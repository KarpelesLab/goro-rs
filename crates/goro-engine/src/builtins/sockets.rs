//! Socket streams (PHP manual: Streams, "Socket Transports"; Stream
//! Functions: stream_socket_client(), stream_socket_server(),
//! stream_socket_accept(), fsockopen()...), the stream contexts
//! (stream_context_create() and friends), stream_set_blocking() and
//! stream_set_timeout().
//!
//! A socket stream is a [`Sock`] behind `Backing::Native`; its TLS layer
//! (ssl://, tls://, stream_socket_enable_crypto()) lives in
//! builtins/openssl/tls.rs. Messages, error numbers and metadata were
//! observed on PHP 8.5 and checked on ext/openssl/tests and
//! ext/standard/tests/network.

use std::cell::RefCell;
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::time::{Duration, Instant};

use goro_runtime::{Key, PhpArray, PhpStr, ResRef, Value};

use super::file::{Backing, Stream, new_stream_res, with_stream};
use super::streams::NativeStream;
use crate::func::Args;
use crate::registry::{ConstVal, Module};
use crate::vm::{R, Vm};

pub static MODULE: Module = Module {
    name: "standard",
    functions: &[
        (
            "function stream_socket_client(string $address, &$error_code = null, &$error_message = null, ?float $timeout = null, int $flags = STREAM_CLIENT_CONNECT, $context = null)",
            stream_socket_client,
        ),
        (
            "function stream_socket_server(string $address, &$error_code = null, &$error_message = null, int $flags = 12, $context = null)",
            stream_socket_server,
        ),
        (
            "function stream_socket_accept($socket, ?float $timeout = null, &$peer_name = null)",
            stream_socket_accept,
        ),
        (
            "function stream_socket_get_name($socket, bool $remote): string|false",
            stream_socket_get_name,
        ),
        (
            "function stream_socket_shutdown($stream, int $mode): bool",
            stream_socket_shutdown,
        ),
        (
            "function stream_socket_enable_crypto($stream, bool $enable, ?int $crypto_method = null, $session_stream = null): int|bool",
            stream_socket_enable_crypto,
        ),
        (
            "function stream_socket_pair(int $domain, int $type, int $protocol): array|false",
            stream_socket_pair,
        ),
        (
            "function stream_socket_recvfrom($socket, int $length, int $flags = 0, &$address = null): string|false",
            stream_socket_recvfrom,
        ),
        (
            "function stream_socket_sendto($socket, string $data, int $flags = 0, string $address = \"\"): int|false",
            stream_socket_sendto,
        ),
        (
            "function fsockopen(string $hostname, int $port = -1, &$error_code = null, &$error_message = null, ?float $timeout = null)",
            fsockopen,
        ),
        (
            "function pfsockopen(string $hostname, int $port = -1, &$error_code = null, &$error_message = null, ?float $timeout = null)",
            fsockopen,
        ),
        (
            "function stream_get_transports(): array",
            stream_get_transports,
        ),
        (
            "function stream_set_blocking($stream, bool $enable): bool",
            stream_set_blocking,
        ),
        (
            "function socket_set_blocking($stream, bool $enable): bool",
            stream_set_blocking,
        ),
        (
            "function stream_set_timeout($stream, int $seconds, int $microseconds = 0): bool",
            stream_set_timeout,
        ),
        (
            "function socket_get_status($stream): array",
            super::file::stream_get_meta_data,
        ),
        (
            "function socket_set_timeout($stream, int $seconds, int $microseconds = 0): bool",
            stream_set_timeout,
        ),
        (
            "function stream_context_create(?array $options = null, ?array $params = null)",
            stream_context_create,
        ),
        (
            "function stream_context_get_default(?array $options = null)",
            stream_context_get_default,
        ),
        (
            "function stream_context_set_default(array $options)",
            stream_context_set_default,
        ),
        (
            "function stream_context_set_option($context, array|string $wrapper_or_options, ?string $option_name = null, mixed $value = UNKNOWN): bool",
            stream_context_set_option,
        ),
        (
            "function stream_context_set_options($context, array $options): true",
            stream_context_set_options,
        ),
        (
            "function stream_context_get_options($stream_or_context): array",
            stream_context_get_options,
        ),
        (
            "function stream_context_set_params($context, array $params): true",
            stream_context_set_params,
        ),
        (
            "function stream_context_get_params($context): array",
            stream_context_get_params,
        ),
    ],
    // PHP manual: Streams predefined constants (values observed on PHP 8.5).
    constants: &[
        ("STREAM_CLIENT_PERSISTENT", ConstVal::Int(1)),
        ("STREAM_CLIENT_ASYNC_CONNECT", ConstVal::Int(2)),
        ("STREAM_CLIENT_CONNECT", ConstVal::Int(4)),
        ("STREAM_CRYPTO_METHOD_ANY_CLIENT", ConstVal::Int(127)),
        ("STREAM_CRYPTO_METHOD_SSLv2_CLIENT", ConstVal::Int(3)),
        ("STREAM_CRYPTO_METHOD_SSLv3_CLIENT", ConstVal::Int(5)),
        ("STREAM_CRYPTO_METHOD_SSLv23_CLIENT", ConstVal::Int(57)),
        ("STREAM_CRYPTO_METHOD_TLS_CLIENT", ConstVal::Int(121)),
        ("STREAM_CRYPTO_METHOD_TLSv1_0_CLIENT", ConstVal::Int(9)),
        ("STREAM_CRYPTO_METHOD_TLSv1_1_CLIENT", ConstVal::Int(17)),
        ("STREAM_CRYPTO_METHOD_TLSv1_2_CLIENT", ConstVal::Int(33)),
        ("STREAM_CRYPTO_METHOD_TLSv1_3_CLIENT", ConstVal::Int(65)),
        ("STREAM_CRYPTO_METHOD_ANY_SERVER", ConstVal::Int(126)),
        ("STREAM_CRYPTO_METHOD_SSLv2_SERVER", ConstVal::Int(2)),
        ("STREAM_CRYPTO_METHOD_SSLv3_SERVER", ConstVal::Int(4)),
        ("STREAM_CRYPTO_METHOD_SSLv23_SERVER", ConstVal::Int(120)),
        ("STREAM_CRYPTO_METHOD_TLS_SERVER", ConstVal::Int(120)),
        ("STREAM_CRYPTO_METHOD_TLSv1_0_SERVER", ConstVal::Int(8)),
        ("STREAM_CRYPTO_METHOD_TLSv1_1_SERVER", ConstVal::Int(16)),
        ("STREAM_CRYPTO_METHOD_TLSv1_2_SERVER", ConstVal::Int(32)),
        ("STREAM_CRYPTO_METHOD_TLSv1_3_SERVER", ConstVal::Int(64)),
        ("STREAM_CRYPTO_PROTO_SSLv3", ConstVal::Int(4)),
        ("STREAM_CRYPTO_PROTO_TLSv1_0", ConstVal::Int(8)),
        ("STREAM_CRYPTO_PROTO_TLSv1_1", ConstVal::Int(16)),
        ("STREAM_CRYPTO_PROTO_TLSv1_2", ConstVal::Int(32)),
        ("STREAM_CRYPTO_PROTO_TLSv1_3", ConstVal::Int(64)),
        ("STREAM_SHUT_RD", ConstVal::Int(0)),
        ("STREAM_SHUT_WR", ConstVal::Int(1)),
        ("STREAM_SHUT_RDWR", ConstVal::Int(2)),
        ("STREAM_PF_INET", ConstVal::Int(2)),
        ("STREAM_PF_INET6", ConstVal::Int(10)),
        ("STREAM_PF_UNIX", ConstVal::Int(1)),
        ("STREAM_IPPROTO_IP", ConstVal::Int(0)),
        ("STREAM_IPPROTO_TCP", ConstVal::Int(6)),
        ("STREAM_IPPROTO_UDP", ConstVal::Int(17)),
        ("STREAM_IPPROTO_ICMP", ConstVal::Int(1)),
        ("STREAM_IPPROTO_RAW", ConstVal::Int(255)),
        ("STREAM_SOCK_STREAM", ConstVal::Int(1)),
        ("STREAM_SOCK_DGRAM", ConstVal::Int(2)),
        ("STREAM_SOCK_RAW", ConstVal::Int(3)),
        ("STREAM_SOCK_SEQPACKET", ConstVal::Int(5)),
        ("STREAM_SOCK_RDM", ConstVal::Int(4)),
        ("STREAM_PEEK", ConstVal::Int(2)),
        ("STREAM_OOB", ConstVal::Int(1)),
        ("STREAM_SERVER_BIND", ConstVal::Int(4)),
        ("STREAM_SERVER_LISTEN", ConstVal::Int(8)),
    ],
    listed: false,
    ..Module::EMPTY
};

// ------------------------------------------------------------ contexts

/// A stream context (PHP manual: Stream Contexts): options by wrapper and
/// parameters, shared by every holder of the resource.
pub struct StreamContext {
    pub options: RefCell<PhpArray>,
    pub params: RefCell<PhpArray>,
}

fn new_context(options: PhpArray) -> Value {
    Value::Resource(ResRef::new(
        PhpStr::intern(b"stream-context"),
        Box::new(StreamContext {
            options: RefCell::new(options),
            params: RefCell::new(PhpArray::new()),
        }),
    ))
}

/// Runs `f` on the context behind `v`, if it is one.
pub fn with_context<T>(v: &Value, f: impl FnOnce(&StreamContext) -> T) -> Option<T> {
    let Value::Resource(r) = v.deref() else {
        return None;
    };
    let data = r.res().data.borrow();
    data.as_ref()?.downcast_ref::<StreamContext>().map(f)
}

/// The option `wrapper.name` of a context.
pub fn context_option(ctx: &Value, wrapper: &str, name: &str) -> Option<Value> {
    with_context(ctx, |c| {
        let o = c.options.borrow();
        match o.get_str(&PhpStr::from(wrapper)).map(|v| v.deref()) {
            Some(Value::Array(w)) => w.get_str(&PhpStr::from(name)).map(|v| v.deref()),
            _ => None,
        }
    })
    .flatten()
}

/// Sets the option `wrapper.name` of a context.
pub fn set_context_option(ctx: &Value, wrapper: &str, name: &str, v: Value) {
    with_context(ctx, |c| {
        let mut o = c.options.borrow_mut();
        let wk = Key::from(wrapper);
        let mut w = match o.get_str(&PhpStr::from(wrapper)).map(|v| v.deref()) {
            Some(Value::Array(w)) => w,
            _ => PhpArray::new(),
        };
        w.insert(Key::from(name), v);
        o.insert(wk, Value::Array(w));
    });
}

fn check_options(vm: &mut Vm, a: &Args, opts: &PhpArray) -> R<bool> {
    for (k, v) in opts.iter() {
        // wrapper names are strings (test streams/bug71884.phpt)
        if matches!(k, goro_runtime::KeyRef::Int(_)) || !matches!(v.deref(), Value::Array(_)) {
            vm.throw(
                "ValueError",
                "Options should have the form [\"wrappername\"][\"optionname\"] = $value",
            )?;
            let _ = a;
            return Ok(false);
        }
    }
    Ok(true)
}

fn merge_options(ctx: &Value, opts: &PhpArray) {
    for (k, v) in opts.iter() {
        let w = match k {
            goro_runtime::KeyRef::Str(s) => s.to_string_lossy(),
            goro_runtime::KeyRef::Int(i) => i.to_string(),
        };
        if let Value::Array(inner) = v.deref() {
            for (ok, ov) in inner.iter() {
                let o = match ok {
                    goro_runtime::KeyRef::Str(s) => s.to_string_lossy(),
                    goro_runtime::KeyRef::Int(i) => i.to_string(),
                };
                set_context_option(ctx, &w, &o, ov.deref());
            }
        }
    }
}

/// Applies stream_context_set_params()-style parameters: "notification"
/// must be callable, "options" an options array.
fn set_params(vm: &mut Vm, a: &Args, ctx: &Value, params: &PhpArray) -> R<bool> {
    if let Some(n) = params
        .get_str(&PhpStr::from("notification"))
        .map(|v| v.deref())
    {
        if !vm.is_callable_value(&n, false) {
            let why = vm.callable_error(&n);
            vm.fthrow(
                a,
                "TypeError",
                &format!(
                    "Argument #1 ($context) must be an array with valid callbacks as values, {why}"
                ),
            )?;
            return Ok(false);
        }
        with_context(ctx, |c| {
            c.params
                .borrow_mut()
                .insert(Key::from("notification"), n.clone());
        });
    }
    if let Some(o) = params.get_str(&PhpStr::from("options")).map(|v| v.deref()) {
        let Value::Array(o) = o else {
            vm.throw("TypeError", "Invalid stream/context parameter")?;
            return Ok(false);
        };
        if !check_options(vm, a, &o)? {
            return Ok(false);
        }
        merge_options(ctx, &o);
    }
    Ok(true)
}

fn stream_context_create(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let ctx = new_context(PhpArray::new());
    if let Value::Array(o) = a.val(0) {
        if !check_options(vm, a, &o)? {
            return Ok(Value::Null);
        }
        merge_options(&ctx, &o);
    }
    if let Value::Array(p) = a.val(1)
        && !set_params(vm, a, &ctx, &p)?
    {
        return Ok(Value::Null);
    }
    Ok(ctx)
}

thread_local! {
    static DEFAULT_CONTEXT: RefCell<Option<Value>> = const { RefCell::new(None) };
}

/// The default context (stream_context_get_default()).
pub fn default_context() -> Value {
    DEFAULT_CONTEXT.with(|d| {
        d.borrow_mut()
            .get_or_insert_with(|| new_context(PhpArray::new()))
            .clone()
    })
}

fn stream_context_get_default(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let ctx = default_context();
    if let Value::Array(o) = a.val(0) {
        if !check_options(vm, a, &o)? {
            return Ok(Value::Null);
        }
        merge_options(&ctx, &o);
    }
    Ok(ctx)
}

fn stream_context_set_default(vm: &mut Vm, a: &mut Args) -> R<Value> {
    stream_context_get_default(vm, a)
}

/// The context argument: a context, or a stream (its context); anything
/// else is a TypeError (test stream_context_set_options_error.phpt).
fn ctx_arg(vm: &mut Vm, a: &Args, i: usize) -> R<Option<Value>> {
    let v = a.val(i);
    if with_context(&v, |_| ()).is_some() {
        return Ok(Some(v));
    }
    if let Value::Resource(r) = &v
        && r.res()
            .data
            .borrow()
            .as_ref()
            .is_some_and(|d| d.is::<RefCell<Stream>>())
    {
        return Ok(Some(stream_context_of(&v).unwrap_or_else(default_context)));
    }
    let msg = if matches!(v, Value::Resource(_)) {
        format!(
            "Argument #{} ($context) must be a valid stream/context",
            i + 1
        )
    } else {
        format!(
            "Argument #{} ($context) must be of type resource, {} given",
            i + 1,
            crate::class::TypeName::tname(&v)
        )
    };
    vm.fthrow(a, "TypeError", &msg)?;
    Ok(None)
}

fn stream_context_set_option(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let Some(ctx) = ctx_arg(vm, a, 0)? else {
        return Ok(Value::Null);
    };
    // PHP 8.5: every two-argument call is deprecated, before the argument
    // checks (test streams/stream_context_set_option_error.phpt).
    if !a.has(2) {
        vm.deprecated(
            "Calling stream_context_set_option() with 2 arguments is deprecated, use stream_context_set_options() instead",
        )?;
    }
    match a.val(1) {
        Value::Array(o) => {
            if a.has(2) && !a.val(2).is_null() {
                return vm.fthrow(
                    a,
                    "ValueError",
                    "Argument #3 ($option_name) must be null when argument #2 ($wrapper_or_options) is an array",
                );
            }
            if a.has(3) {
                return vm.fthrow(
                    a,
                    "ValueError",
                    "Argument #4 ($value) cannot be provided when argument #2 ($wrapper_or_options) is an array",
                );
            }
            if !check_options(vm, a, &o)? {
                return Ok(Value::Null);
            }
            merge_options(&ctx, &o);
        }
        _ => {
            if !a.has(2) || a.val(2).is_null() {
                return vm.fthrow(
                    a,
                    "ValueError",
                    "Argument #3 ($option_name) cannot be null when argument #2 ($wrapper_or_options) is a string",
                );
            }
            if !a.has(3) {
                return vm.fthrow(
                    a,
                    "ValueError",
                    "Argument #4 ($value) must be provided when argument #2 ($wrapper_or_options) is a string",
                );
            }
            let w = a.str(1).to_string_lossy();
            let n = a.str(2).to_string_lossy();
            set_context_option(&ctx, &w, &n, a.val(3));
        }
    }
    Ok(Value::Bool(true))
}

fn stream_context_set_options(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let Some(ctx) = ctx_arg(vm, a, 0)? else {
        return Ok(Value::Null);
    };
    let o = a.array(1);
    if !check_options(vm, a, &o)? {
        return Ok(Value::Null);
    }
    merge_options(&ctx, &o);
    Ok(Value::Bool(true))
}

/// The context attached to a socket stream, if any.
fn stream_context_of(v: &Value) -> Option<Value> {
    let Value::Resource(r) = v.deref() else {
        return None;
    };
    let data = r.res().data.borrow();
    let cell = data.as_ref()?.downcast_ref::<RefCell<Stream>>()?;
    let s = cell.borrow();
    match &s.backing {
        Backing::Native(n) => n.context(),
        _ => None,
    }
}

fn stream_context_get_options(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let v = a.val(0);
    let ctx = if with_context(&v, |_| ()).is_some() {
        Some(v)
    } else {
        stream_context_of(&v)
    };
    Ok(Value::Array(
        ctx.and_then(|c| with_context(&c, |c| c.options.borrow().clone()))
            .unwrap_or_default(),
    ))
}

fn stream_context_set_params(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let Some(ctx) = ctx_arg(vm, a, 0)? else {
        return Ok(Value::Null);
    };
    let p = a.array(1);
    if !set_params(vm, a, &ctx, &p)? {
        return Ok(Value::Null);
    }
    Ok(Value::Bool(true))
}

fn stream_context_get_params(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let Some(ctx) = ctx_arg(vm, a, 0)? else {
        return Ok(Value::Null);
    };
    let mut out = with_context(&ctx, |c| c.params.borrow().clone()).unwrap_or_default();
    let opts = with_context(&ctx, |c| c.options.borrow().clone()).unwrap_or_default();
    out.insert(Key::from("options"), Value::Array(opts));
    Ok(Value::Array(out))
}

// ------------------------------------------------------------ sockets

/// The kind of socket.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Tcp,
    Udp,
    Unix,
    Udg,
    /// stream_socket_pair().
    Pair,
}

fn errno() -> i32 {
    io::Error::last_os_error().raw_os_error().unwrap_or(0)
}

fn strerror(e: i32) -> String {
    super::exec::strerror(e)
}

/// The descriptor I/O of a socket, honouring its blocking mode and
/// timeout (the descriptor itself is always non-blocking).
pub struct FdIo<'a> {
    pub fd: RawFd,
    pub blocking: bool,
    pub timeout: Option<Duration>,
    pub timed_out: &'a mut bool,
}

/// Waits for `events` on `fd`; false on timeout.
fn wait_fd(fd: RawFd, events: i16, timeout: Option<Duration>) -> io::Result<bool> {
    let deadline = timeout.map(|t| Instant::now() + t);
    loop {
        let ms = match deadline {
            None => -1,
            Some(d) => {
                let left = d.saturating_duration_since(Instant::now());
                left.as_millis().min(i32::MAX as u128) as i32
                    + i32::from(left.subsec_micros() % 1000 != 0)
            }
        };
        let mut p = libc::pollfd {
            fd,
            events,
            revents: 0,
        };
        // SAFETY: one valid pollfd.
        let n = unsafe { libc::poll(&mut p, 1, ms) };
        if n < 0 {
            if errno() == libc::EINTR {
                continue;
            }
            return Err(io::Error::last_os_error());
        }
        return Ok(n > 0);
    }
}

impl goro_ext_openssl::tls::Io for FdIo<'_> {
    fn recv(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        loop {
            if self.blocking && !wait_fd(self.fd, libc::POLLIN, self.timeout)? {
                *self.timed_out = true;
                return Err(io::Error::from(io::ErrorKind::TimedOut));
            }
            // SAFETY: buf is valid for writes of its length.
            let n = unsafe {
                libc::recv(
                    self.fd,
                    buf.as_mut_ptr().cast(),
                    buf.len(),
                    libc::MSG_DONTWAIT,
                )
            };
            if n >= 0 {
                return Ok(n as usize);
            }
            match errno() {
                libc::EINTR => continue,
                libc::EAGAIN if self.blocking => continue,
                libc::EAGAIN => return Err(io::Error::from(io::ErrorKind::WouldBlock)),
                e => return Err(io::Error::from_raw_os_error(e)),
            }
        }
    }

    fn send(&mut self, data: &[u8]) -> io::Result<usize> {
        loop {
            if self.blocking && !wait_fd(self.fd, libc::POLLOUT, self.timeout)? {
                *self.timed_out = true;
                return Err(io::Error::from(io::ErrorKind::TimedOut));
            }
            // SAFETY: data is valid for reads of its length.
            let n = unsafe {
                libc::send(
                    self.fd,
                    data.as_ptr().cast(),
                    data.len(),
                    libc::MSG_DONTWAIT | libc::MSG_NOSIGNAL,
                )
            };
            if n >= 0 {
                return Ok(n as usize);
            }
            match errno() {
                libc::EINTR => continue,
                libc::EAGAIN if self.blocking => continue,
                libc::EAGAIN => return Err(io::Error::from(io::ErrorKind::WouldBlock)),
                e => return Err(io::Error::from_raw_os_error(e)),
            }
        }
    }
}

/// The read-ahead size of plain stream sockets.
const READ_CHUNK: usize = 8192;

/// A socket stream.
pub struct Sock {
    pub fd: OwnedFd,
    pub kind: Kind,
    pub listening: bool,
    pub blocking: bool,
    pub timeout: Option<Duration>,
    pub timed_out: bool,
    pub eof: bool,
    pub ctx: Option<Value>,
    /// The TLS session once crypto is enabled (or being enabled).
    pub tls: Option<Box<goro_ext_openssl::tls::Session>>,
    /// Whether the TLS handshake completed.
    pub crypto_on: bool,
    /// The crypto method an ssl://-like transport implies (0 for plain
    /// transports); accepted sockets of a server inherit it.
    pub transport_method: i64,
    /// The host the client connected to (peer name and SNI default).
    pub host: Vec<u8>,
    /// The address the stream was opened with (metadata "uri").
    pub uri: Option<PhpStr>,
    /// Bytes read ahead by small reads, and how far they were consumed.
    pub rbuf: Vec<u8>,
    pub rpos: usize,
}

impl Sock {
    pub fn new(fd: OwnedFd, kind: Kind) -> Sock {
        Sock {
            fd,
            kind,
            listening: false,
            blocking: true,
            timeout: None,
            timed_out: false,
            eof: false,
            ctx: None,
            tls: None,
            crypto_on: false,
            transport_method: 0,
            host: Vec::new(),
            uri: None,
            rbuf: Vec::new(),
            rpos: 0,
        }
    }

    /// Takes the bytes read ahead and not consumed (for a TLS session
    /// started on a socket that was read in plain).
    pub fn take_read_ahead(&mut self) -> Vec<u8> {
        let v = self.rbuf.split_off(self.rpos.min(self.rbuf.len()));
        self.rbuf.clear();
        self.rpos = 0;
        v
    }

    /// The descriptor I/O and the TLS session, borrowed together.
    pub fn parts(&mut self) -> (FdIo<'_>, &mut Option<Box<goro_ext_openssl::tls::Session>>) {
        (
            FdIo {
                fd: self.fd.as_raw_fd(),
                blocking: self.blocking,
                timeout: self.timeout,
                timed_out: &mut self.timed_out,
            },
            &mut self.tls,
        )
    }
}

impl NativeStream for Sock {
    fn read(&mut self, vm: &mut Vm, buf: &mut [u8]) -> io::Result<usize> {
        // Output written so far reaches the peer process first (the CLI
        // writes unbuffered).
        vm.out.flush();
        // Only blocking reads reset the flag (observed on PHP 8.5).
        if self.blocking {
            self.timed_out = false;
        }
        if self.rpos < self.rbuf.len() {
            let n = buf.len().min(self.rbuf.len() - self.rpos);
            buf[..n].copy_from_slice(&self.rbuf[self.rpos..self.rpos + n]);
            self.rpos += n;
            return Ok(n);
        }
        let crypto = self.crypto_on;
        // Small reads of a plain stream socket (fgets() asks for one byte
        // at a time) go through an 8 KiB buffer, as PHP's stream buffer
        // does; datagrams keep their boundaries.
        let buffer = !crypto
            && buf.len() < READ_CHUNK
            && matches!(self.kind, Kind::Tcp | Kind::Unix | Kind::Pair);
        let mut chunk = std::mem::take(&mut self.rbuf);
        let (mut io, tls) = self.parts();
        let r = match tls {
            Some(t) if crypto => t.read(&mut io, buf),
            _ if buffer => {
                chunk.resize(READ_CHUNK, 0);
                goro_ext_openssl::tls::Io::recv(&mut io, &mut chunk).map(|n| {
                    chunk.truncate(n);
                    let k = n.min(buf.len());
                    buf[..k].copy_from_slice(&chunk[..k]);
                    k
                })
            }
            _ => goro_ext_openssl::tls::Io::recv(&mut io, buf),
        };
        if buffer && let Ok(k) = r {
            self.rpos = k;
            self.rbuf = chunk;
        } else {
            chunk.clear();
            self.rbuf = chunk;
            self.rpos = 0;
        }
        if let Ok(0) = r
            && !buf.is_empty()
        {
            self.eof = true;
        }
        r
    }

    fn write(&mut self, _vm: &mut Vm, data: &[u8]) -> io::Result<usize> {
        self.timed_out = false;
        let crypto = self.crypto_on;
        let blocking = self.blocking;
        let (mut io, tls) = self.parts();
        if let Some(t) = tls
            && crypto
        {
            return t.write(&mut io, data);
        }
        let mut done = 0;
        while done < data.len() {
            match goro_ext_openssl::tls::Io::send(&mut io, &data[done..]) {
                Ok(n) => done += n,
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(e) if done == 0 => return Err(e),
                Err(_) => break,
            }
            if !blocking {
                break;
            }
        }
        Ok(done)
    }

    fn eof(&self) -> bool {
        if self.rpos < self.rbuf.len() {
            return false;
        }
        if self.eof {
            return true;
        }
        if let Some(t) = &self.tls
            && self.crypto_on
        {
            if t.buffered() {
                return false;
            }
            if t.peer_closed() {
                return true;
            }
        }
        if self.listening || matches!(self.kind, Kind::Udp | Kind::Udg) {
            return false;
        }
        // A closed peer reads as the end: peek without blocking.
        let mut b = [0u8; 1];
        // SAFETY: one-byte buffer.
        let n = unsafe {
            libc::recv(
                self.fd.as_raw_fd(),
                b.as_mut_ptr().cast(),
                1,
                libc::MSG_PEEK | libc::MSG_DONTWAIT,
            )
        };
        n == 0 || (n < 0 && !matches!(errno(), libc::EAGAIN | libc::EINTR))
    }

    fn close(&mut self, _vm: &mut Vm) -> R<()> {
        if self.crypto_on {
            let fd = self.fd.as_raw_fd();
            let mut to = false;
            let mut io = FdIo {
                fd,
                blocking: false,
                timeout: None,
                timed_out: &mut to,
            };
            if let Some(t) = &mut self.tls {
                // Queued records first (non-blocking writes may have left
                // some), then close_notify.
                if t.pending_output() {
                    let mut bio = FdIo {
                        fd,
                        blocking: true,
                        timeout: Some(Duration::from_secs(5)),
                        timed_out: &mut false,
                    };
                    let _ = t.flush(&mut bio);
                }
                t.close(&mut io);
            }
        }
        Ok(())
    }

    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }

    fn raw_fd(&self) -> Option<i32> {
        Some(self.fd.as_raw_fd())
    }

    fn buffered(&self) -> bool {
        self.rpos < self.rbuf.len()
            || self.crypto_on && self.tls.as_ref().is_some_and(|t| t.buffered())
    }

    fn is_socket(&self) -> bool {
        true
    }

    fn set_blocking(&mut self, blocking: bool) -> bool {
        self.blocking = blocking;
        true
    }

    fn set_timeout(&mut self, secs: f64) -> bool {
        self.timeout = timeout_of(secs);
        self.timed_out = false;
        true
    }

    fn meta(&self, m: &mut PhpArray) {
        m.insert(Key::from("timed_out"), Value::Bool(self.timed_out));
        m.insert(Key::from("blocked"), Value::Bool(self.blocking));
        m.insert(Key::from("eof"), Value::Bool(NativeStream::eof(self)));
        m.insert(
            Key::from("unread_bytes"),
            Value::Int((self.rbuf.len() - self.rpos.min(self.rbuf.len())) as i64),
        );
        if let Some(t) = &self.tls
            && self.crypto_on
        {
            let mut c = PhpArray::new();
            let (name, bits, ver) = t.cipher();
            c.insert(Key::from("protocol"), Value::from(t.protocol()));
            c.insert(Key::from("cipher_name"), Value::from(name));
            c.insert(Key::from("cipher_bits"), Value::Int(bits));
            c.insert(Key::from("cipher_version"), Value::from(ver));
            if let Some(alpn) = t.alpn() {
                c.insert(Key::from("alpn_protocol"), Value::Str(PhpStr::from(alpn)));
            }
            m.insert(Key::from("crypto"), Value::Array(c));
        }
        if let Some(u) = &self.uri {
            m.insert(Key::from("uri"), Value::Str(u.clone()));
        }
    }

    fn context(&self) -> Option<Value> {
        self.ctx.clone()
    }
}

/// A timeout in seconds as a duration; negative means none (blocking
/// forever, as default_socket_timeout=-1 does; test bug62890.phpt).
pub fn timeout_of(secs: f64) -> Option<Duration> {
    if secs < 0.0 || !secs.is_finite() {
        None
    } else {
        Some(Duration::from_secs_f64(secs.min(1e9)))
    }
}

fn default_timeout(vm: &Vm) -> f64 {
    vm.get_ini(b"default_socket_timeout")
        .map(|v| goro_runtime::convert::str_to_float(&v))
        .unwrap_or(60.0)
}

/// Runs `f` on the socket behind stream argument `i` (None for other
/// streams).
pub fn with_sock<T>(
    vm: &mut Vm,
    a: &Args,
    i: usize,
    f: impl FnOnce(&mut Vm, Option<&mut Sock>) -> R<T>,
) -> R<T> {
    with_stream(vm, a, i, |vm, s| match &mut s.backing {
        Backing::Native(n) => {
            let sock = n.as_any_mut().and_then(|x| x.downcast_mut::<Sock>());
            f(vm, sock)
        }
        _ => f(vm, None),
    })
}

fn sock_stream(sock: Sock, uri: &[u8]) -> Value {
    let stype = match sock.kind {
        Kind::Tcp => "tcp_socket/ssl",
        Kind::Udp => "udp_socket",
        Kind::Unix => "unix_socket",
        Kind::Udg => "udg_socket",
        Kind::Pair => "generic_socket",
    };
    Value::Resource(new_stream_res(Stream::new(
        Backing::Native(Box::new(sock)),
        b"r+",
        uri,
        "",
        stype,
    )))
}

// ------------------------------------------------------------ addresses

/// A parsed transport address.
struct Target {
    /// The transport name ("tcp", "ssl", "tlsv1.2", "unix"...).
    transport: String,
    kind: Kind,
    /// Host (inet) or path (unix).
    host: Vec<u8>,
    port: u16,
}

const TRANSPORTS: &[&str] = &[
    "tcp", "udp", "unix", "udg", "ssl", "tls", "tlsv1.0", "tlsv1.1", "tlsv1.2", "tlsv1.3",
];

enum AddrError {
    /// Unable to find the socket transport "x"...
    Transport(String),
    /// Failed to parse address "x".
    Parse,
}

fn parse_target(addr: &[u8]) -> Result<Target, AddrError> {
    let (transport, rest) = match addr.windows(3).position(|w| w == b"://") {
        Some(p) => (
            String::from_utf8_lossy(&addr[..p]).to_ascii_lowercase(),
            &addr[p + 3..],
        ),
        None => ("tcp".to_string(), addr),
    };
    if !TRANSPORTS.contains(&transport.as_str()) {
        return Err(AddrError::Transport(transport));
    }
    let kind = match transport.as_str() {
        "udp" => Kind::Udp,
        "unix" => Kind::Unix,
        "udg" => Kind::Udg,
        _ => Kind::Tcp,
    };
    if matches!(kind, Kind::Unix | Kind::Udg) {
        return Ok(Target {
            transport,
            kind,
            host: rest.to_vec(),
            port: 0,
        });
    }
    let (host, port) = if rest.first() == Some(&b'[') {
        let end = rest
            .iter()
            .position(|&c| c == b']')
            .ok_or(AddrError::Parse)?;
        let after = &rest[end + 1..];
        let port = after.strip_prefix(b":").ok_or(AddrError::Parse)?;
        (rest[1..end].to_vec(), port)
    } else {
        let c = rest
            .iter()
            .rposition(|&c| c == b':')
            .ok_or(AddrError::Parse)?;
        (rest[..c].to_vec(), &rest[c + 1..])
    };
    // A "/suffix" after the port is a persistence id (test
    // ext/standard/tests/network/socket_bug74429.phpt).
    let port = match port.iter().position(|&c| c == b'/') {
        Some(p) => &port[..p],
        None => port,
    };
    // The port wraps modulo 65536 as a C unsigned short does (test
    // streams/bug69521.phpt uses 74321).
    let port: u16 = std::str::from_utf8(port)
        .ok()
        .and_then(|p| p.parse::<u64>().ok())
        .map(|p| (p & 0xffff) as u16)
        .ok_or(AddrError::Parse)?;
    Ok(Target {
        transport,
        kind,
        host,
        port,
    })
}

/// The crypto method a transport implies (PHP manual: SSL & TLS
/// transports).
fn transport_method(t: &str, server: bool) -> i64 {
    let m = match t {
        "ssl" => 127,
        "tls" => 121,
        "tlsv1.0" => 9,
        "tlsv1.1" => 17,
        "tlsv1.2" => 33,
        "tlsv1.3" => 65,
        _ => return 0,
    };
    if server { m & !1 } else { m }
}

/// Resolves `host:port` (getaddrinfo), or the getaddrinfo message.
pub(crate) fn resolve(
    host: &[u8],
    port: u16,
    dgram: bool,
) -> Result<Vec<std::net::SocketAddr>, String> {
    use std::ffi::CString;
    let h = CString::new(host.to_vec())
        .map_err(|_| "The hostname must not contain null bytes".to_string())?;
    // SAFETY: zeroed hints are a valid addrinfo.
    let mut hints: libc::addrinfo = unsafe { std::mem::zeroed() };
    hints.ai_family = libc::AF_UNSPEC;
    hints.ai_socktype = if dgram {
        libc::SOCK_DGRAM
    } else {
        libc::SOCK_STREAM
    };
    let mut res: *mut libc::addrinfo = std::ptr::null_mut();
    // SAFETY: valid C strings and hints; res is freed below.
    let rc = unsafe { libc::getaddrinfo(h.as_ptr(), std::ptr::null(), &hints, &mut res) };
    if rc != 0 {
        // SAFETY: gai_strerror returns a static string.
        let msg = unsafe { std::ffi::CStr::from_ptr(libc::gai_strerror(rc)) };
        return Err(format!(
            "php_network_getaddresses: getaddrinfo for {} failed: {}",
            String::from_utf8_lossy(host),
            msg.to_string_lossy()
        ));
    }
    let mut out = Vec::new();
    let mut p = res;
    while !p.is_null() {
        // SAFETY: p walks the list getaddrinfo returned.
        let ai = unsafe { &*p };
        if let Some(sa) = sockaddr_to_std(ai.ai_addr, ai.ai_addrlen) {
            let sa = with_port(sa, port);
            if !out.contains(&sa) {
                out.push(sa);
            }
        }
        p = ai.ai_next;
    }
    // SAFETY: res came from getaddrinfo.
    unsafe { libc::freeaddrinfo(res) };
    Ok(out)
}

fn with_port(mut sa: std::net::SocketAddr, port: u16) -> std::net::SocketAddr {
    sa.set_port(port);
    sa
}

fn sockaddr_to_std(
    sa: *const libc::sockaddr,
    len: libc::socklen_t,
) -> Option<std::net::SocketAddr> {
    if sa.is_null() {
        return None;
    }
    // SAFETY: the family tells which structure it is.
    unsafe {
        match (*sa).sa_family as i32 {
            libc::AF_INET if len as usize >= std::mem::size_of::<libc::sockaddr_in>() => {
                let s = &*(sa as *const libc::sockaddr_in);
                Some(std::net::SocketAddr::from((
                    std::net::Ipv4Addr::from(u32::from_be(s.sin_addr.s_addr)),
                    u16::from_be(s.sin_port),
                )))
            }
            libc::AF_INET6 if len as usize >= std::mem::size_of::<libc::sockaddr_in6>() => {
                let s = &*(sa as *const libc::sockaddr_in6);
                Some(std::net::SocketAddr::V6(std::net::SocketAddrV6::new(
                    std::net::Ipv6Addr::from(s.sin6_addr.s6_addr),
                    u16::from_be(s.sin6_port),
                    s.sin6_flowinfo,
                    s.sin6_scope_id,
                )))
            }
            _ => None,
        }
    }
}

/// A sockaddr for `sa` (storage and length).
fn std_to_sockaddr(sa: &std::net::SocketAddr) -> (libc::sockaddr_storage, libc::socklen_t) {
    // SAFETY: zeroed storage is valid.
    let mut st: libc::sockaddr_storage = unsafe { std::mem::zeroed() };
    let len = match sa {
        std::net::SocketAddr::V4(a) => {
            // SAFETY: storage is large and aligned enough.
            let s = unsafe { &mut *(&mut st as *mut _ as *mut libc::sockaddr_in) };
            s.sin_family = libc::AF_INET as libc::sa_family_t;
            s.sin_port = a.port().to_be();
            s.sin_addr.s_addr = u32::from(*a.ip()).to_be();
            std::mem::size_of::<libc::sockaddr_in>()
        }
        std::net::SocketAddr::V6(a) => {
            // SAFETY: as above.
            let s = unsafe { &mut *(&mut st as *mut _ as *mut libc::sockaddr_in6) };
            s.sin6_family = libc::AF_INET6 as libc::sa_family_t;
            s.sin6_port = a.port().to_be();
            s.sin6_addr.s6_addr = a.ip().octets();
            s.sin6_flowinfo = a.flowinfo();
            s.sin6_scope_id = a.scope_id();
            std::mem::size_of::<libc::sockaddr_in6>()
        }
    };
    (st, len as libc::socklen_t)
}

fn unix_sockaddr(path: &[u8]) -> Option<(libc::sockaddr_un, libc::socklen_t)> {
    // SAFETY: zeroed sockaddr_un is valid.
    let mut s: libc::sockaddr_un = unsafe { std::mem::zeroed() };
    if path.len() >= s.sun_path.len() {
        return None;
    }
    s.sun_family = libc::AF_UNIX as libc::sa_family_t;
    for (d, b) in s.sun_path.iter_mut().zip(path) {
        *d = *b as libc::c_char;
    }
    let len = std::mem::size_of::<libc::sa_family_t>() + path.len() + 1;
    Some((s, len as libc::socklen_t))
}

/// A unix socket path longer than sun_path holds is truncated with a
/// notice (test streams/bug60106-001.phpt).
fn unix_path_limit(vm: &mut Vm, a: &Args, t: &mut Target) -> R<()> {
    // SAFETY: zeroed sockaddr_un is valid.
    let max = unsafe { std::mem::zeroed::<libc::sockaddr_un>() }
        .sun_path
        .len()
        - 1;
    if matches!(t.kind, Kind::Unix | Kind::Udg) && t.host.len() > max {
        vm.fnotice(
            a,
            &format!(
                "socket path exceeded the maximum allowed length of {max} bytes and was truncated"
            ),
        )?;
        t.host.truncate(max);
    }
    Ok(())
}

fn new_socket(family: i32, ty: i32) -> io::Result<OwnedFd> {
    // SAFETY: plain socket(2).
    let fd = unsafe { libc::socket(family, ty | libc::SOCK_CLOEXEC | libc::SOCK_NONBLOCK, 0) };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: a fresh descriptor we own.
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

/// Connects (non-blocking connect then a wait up to `timeout`).
fn connect_fd(
    fd: &OwnedFd,
    sa: *const libc::sockaddr,
    len: libc::socklen_t,
    timeout: Option<Duration>,
    wait: bool,
) -> io::Result<()> {
    // SAFETY: sa/len describe a valid address.
    let rc = unsafe { libc::connect(fd.as_raw_fd(), sa, len) };
    if rc == 0 {
        return Ok(());
    }
    let e = errno();
    if e != libc::EINPROGRESS && e != libc::EINTR {
        return Err(io::Error::from_raw_os_error(e));
    }
    if !wait {
        return Ok(());
    }
    if !wait_fd(fd.as_raw_fd(), libc::POLLOUT, timeout)? {
        return Err(io::Error::from_raw_os_error(libc::ETIMEDOUT));
    }
    let mut err: libc::c_int = 0;
    let mut l = std::mem::size_of::<libc::c_int>() as libc::socklen_t;
    // SAFETY: getsockopt into an int.
    unsafe {
        libc::getsockopt(
            fd.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_ERROR,
            (&mut err as *mut libc::c_int).cast(),
            &mut l,
        )
    };
    if err != 0 {
        return Err(io::Error::from_raw_os_error(err));
    }
    Ok(())
}

/// Opens a client socket: `Err((errno, message))` on failure.
fn open_client(
    t: &Target,
    timeout: Option<Duration>,
    async_connect: bool,
) -> Result<Sock, (i32, String)> {
    let oserr = |e: io::Error| {
        let n = e.raw_os_error().unwrap_or(0);
        (n, strerror(n))
    };
    match t.kind {
        Kind::Unix | Kind::Udg => {
            let ty = if t.kind == Kind::Unix {
                libc::SOCK_STREAM
            } else {
                libc::SOCK_DGRAM
            };
            let fd = new_socket(libc::AF_UNIX, ty).map_err(oserr)?;
            let (sa, len) = unix_sockaddr(&t.host).ok_or((0, "Path too long".to_string()))?;
            connect_fd(
                &fd,
                (&sa as *const libc::sockaddr_un).cast(),
                len,
                timeout,
                !async_connect,
            )
            .map_err(oserr)?;
            Ok(Sock::new(fd, t.kind))
        }
        _ => {
            let dgram = t.kind == Kind::Udp;
            let addrs = resolve(&t.host, t.port, dgram).map_err(|m| (0, m))?;
            let mut last = (0, "Unknown error".to_string());
            for sa in addrs {
                let fam = if sa.is_ipv4() {
                    libc::AF_INET
                } else {
                    libc::AF_INET6
                };
                let ty = if dgram {
                    libc::SOCK_DGRAM
                } else {
                    libc::SOCK_STREAM
                };
                let fd = match new_socket(fam, ty) {
                    Ok(f) => f,
                    Err(e) => {
                        last = oserr(e);
                        continue;
                    }
                };
                if !dgram {
                    let one: libc::c_int = 1;
                    // SAFETY: setsockopt with an int.
                    unsafe {
                        libc::setsockopt(
                            fd.as_raw_fd(),
                            libc::IPPROTO_TCP,
                            libc::TCP_NODELAY,
                            (&one as *const libc::c_int).cast(),
                            std::mem::size_of::<libc::c_int>() as libc::socklen_t,
                        )
                    };
                }
                let (st, len) = std_to_sockaddr(&sa);
                match connect_fd(
                    &fd,
                    (&st as *const libc::sockaddr_storage).cast(),
                    len,
                    timeout,
                    !async_connect,
                ) {
                    Ok(()) => {
                        let mut s = Sock::new(fd, t.kind);
                        s.host = t.host.clone();
                        return Ok(s);
                    }
                    Err(e) => last = oserr(e),
                }
            }
            Err(last)
        }
    }
}

fn set_reuse(fd: &OwnedFd) {
    let one: libc::c_int = 1;
    // SAFETY: setsockopt with an int.
    unsafe {
        libc::setsockopt(
            fd.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_REUSEADDR,
            (&one as *const libc::c_int).cast(),
            std::mem::size_of::<libc::c_int>() as libc::socklen_t,
        )
    };
}

/// Opens a server socket.
fn open_server(t: &Target, flags: i64, backlog: i32) -> Result<Sock, (i32, String)> {
    let oserr = |e: io::Error| {
        let n = e.raw_os_error().unwrap_or(0);
        (n, strerror(n))
    };
    let listen = flags & 8 != 0;
    let dgram = matches!(t.kind, Kind::Udp | Kind::Udg);
    if dgram && listen {
        // Observed on PHP 8.5: listening on a datagram socket fails with
        // "Unknown error".
        return Err((0, "Unknown error".to_string()));
    }
    let fd = match t.kind {
        Kind::Unix | Kind::Udg => {
            let ty = if dgram {
                libc::SOCK_DGRAM
            } else {
                libc::SOCK_STREAM
            };
            let fd = new_socket(libc::AF_UNIX, ty).map_err(oserr)?;
            let (sa, len) = unix_sockaddr(&t.host).ok_or((0, "Path too long".to_string()))?;
            if flags & 4 != 0 {
                // SAFETY: valid address.
                let rc = unsafe {
                    libc::bind(
                        fd.as_raw_fd(),
                        (&sa as *const libc::sockaddr_un).cast(),
                        len,
                    )
                };
                if rc != 0 {
                    return Err(oserr(io::Error::last_os_error()));
                }
            }
            fd
        }
        _ => {
            let addrs = resolve(&t.host, t.port, dgram).map_err(|m| (0, m))?;
            let sa = *addrs.first().ok_or((0, "Unknown error".to_string()))?;
            let fam = if sa.is_ipv4() {
                libc::AF_INET
            } else {
                libc::AF_INET6
            };
            let ty = if dgram {
                libc::SOCK_DGRAM
            } else {
                libc::SOCK_STREAM
            };
            let fd = new_socket(fam, ty).map_err(oserr)?;
            if !dgram {
                set_reuse(&fd);
            }
            if flags & 4 != 0 {
                let (st, len) = std_to_sockaddr(&sa);
                // SAFETY: valid address.
                let rc = unsafe {
                    libc::bind(
                        fd.as_raw_fd(),
                        (&st as *const libc::sockaddr_storage).cast(),
                        len,
                    )
                };
                if rc != 0 {
                    // Observed on PHP 8.5: the error number stays 0 for
                    // bind failures, only the message is set.
                    let e = errno();
                    return Err((0, strerror(e)));
                }
            }
            fd
        }
    };
    if listen {
        // SAFETY: listen(2) on our descriptor.
        if unsafe { libc::listen(fd.as_raw_fd(), backlog) } != 0 {
            return Err(oserr(io::Error::last_os_error()));
        }
    }
    let mut s = Sock::new(fd, t.kind);
    s.listening = listen;
    Ok(s)
}

/// "ip:port" ("[v6]:port") or the unix path of a socket end.
fn sock_name(fd: RawFd, remote: bool) -> Option<Vec<u8>> {
    // SAFETY: zeroed storage.
    let mut st: libc::sockaddr_storage = unsafe { std::mem::zeroed() };
    let mut len = std::mem::size_of::<libc::sockaddr_storage>() as libc::socklen_t;
    let p = (&mut st as *mut libc::sockaddr_storage).cast();
    // SAFETY: storage large enough.
    let rc = unsafe {
        if remote {
            libc::getpeername(fd, p, &mut len)
        } else {
            libc::getsockname(fd, p, &mut len)
        }
    };
    if rc != 0 {
        return None;
    }
    if st.ss_family as i32 == libc::AF_UNIX {
        // SAFETY: a sockaddr_un.
        let un = unsafe { &*(&st as *const _ as *const libc::sockaddr_un) };
        let n = (len as usize).saturating_sub(std::mem::size_of::<libc::sa_family_t>());
        let path: Vec<u8> = un.sun_path[..n.min(un.sun_path.len())]
            .iter()
            .map(|&c| c as u8)
            .take_while(|&c| c != 0)
            .collect();
        return if path.is_empty() { None } else { Some(path) };
    }
    let sa = sockaddr_to_std((&st as *const libc::sockaddr_storage).cast(), len)?;
    Some(addr_text(&sa).into_bytes())
}

fn addr_text(sa: &std::net::SocketAddr) -> String {
    match sa {
        std::net::SocketAddr::V4(a) => format!("{}:{}", a.ip(), a.port()),
        std::net::SocketAddr::V6(a) => format!("[{}]:{}", a.ip(), a.port()),
    }
}

// ------------------------------------------------------------ functions

/// A timeout argument (seconds; null for default_socket_timeout). Not a
/// finite number: ValueError (test streams/non_finite_values.phpt).
fn float_timeout(vm: &mut Vm, a: &Args, i: usize) -> R<Option<f64>> {
    let t = match a.val(i) {
        Value::Null => default_timeout(vm),
        Value::Float(f) => f,
        Value::Int(n) => n as f64,
        v => qfloat(&v),
    };
    if !t.is_finite() {
        vm.fthrow(
            a,
            "ValueError",
            &format!("Argument #{} ($timeout) must be a finite value", i + 1),
        )?;
        return Ok(None);
    }
    Ok(Some(t))
}

fn set_err(a: &Args, i: usize, code: i32, msg: &str) {
    a.set_ref(i, Value::Int(code as i64));
    a.set_ref(i + 1, Value::Str(PhpStr::from(msg)));
}

/// Opens a client stream for `addr` (shared by stream_socket_client() and
/// fsockopen()); warns and gives false on failure.
#[allow(clippy::too_many_arguments)]
fn client_common(
    vm: &mut Vm,
    a: &Args,
    addr: &[u8],
    shown: &[u8],
    timeout: f64,
    flags: i64,
    ctx: Option<Value>,
    err_idx: usize,
) -> R<Value> {
    set_err(a, err_idx, 0, "");
    let fail = |vm: &mut Vm, code: i32, msg: &str| -> R<Value> {
        set_err(a, err_idx, code, msg);
        vm.fwarn(
            a,
            &format!(
                "Unable to connect to {} ({})",
                String::from_utf8_lossy(shown),
                msg
            ),
        )?;
        Ok(Value::Bool(false))
    };
    let t = match parse_target(addr) {
        Ok(t) => t,
        Err(AddrError::Transport(tr)) => {
            return fail(
                vm,
                0,
                &format!(
                    "Unable to find the socket transport \"{tr}\" - did you forget to enable it when you configured PHP?"
                ),
            );
        }
        Err(AddrError::Parse) => {
            return fail(
                vm,
                0,
                &format!(
                    "Failed to parse address \"{}\"",
                    String::from_utf8_lossy(addr)
                ),
            );
        }
    };
    let mut t = t;
    unix_path_limit(vm, a, &mut t)?;
    let to = timeout_of(timeout);
    vm.out.flush();
    let async_connect = flags & 2 != 0;
    let mut sock = match open_client(&t, to, async_connect) {
        Ok(s) => s,
        Err((code, msg)) => {
            if msg.starts_with("php_network_getaddresses") {
                vm.fwarn(a, &msg)?;
            }
            return fail(vm, code, &msg);
        }
    };
    sock.ctx = ctx.or_else(|| Some(default_context()));
    sock.uri = Some(PhpStr::new(addr));
    let method = transport_method(&t.transport, false);
    sock.transport_method = method;
    if method != 0 {
        sock.timeout = to;
        let r = super::openssl::tls::enable(vm, a, &mut sock, None, false)?;
        if r != super::openssl::tls::Enabled::Yes {
            return fail(vm, 0, "Unknown error");
        }
    }
    // Reads time out after default_socket_timeout (PHP manual:
    // stream_set_timeout()).
    sock.timeout = timeout_of(default_timeout(vm));
    Ok(sock_stream(sock, addr))
}

/// Connects a client socket to a transport address (`tcp://host:port`)
/// for a stream wrapper (http://): the message on failure, as the
/// wrapper reports it ("Connection refused").
pub(crate) fn connect_client(addr: &[u8], timeout: Option<Duration>) -> Result<Sock, String> {
    let t = match parse_target(addr) {
        Ok(t) => t,
        Err(AddrError::Transport(tr)) => {
            return Err(format!(
                "Unable to find the socket transport \"{tr}\" - did you forget to enable it when you configured PHP?"
            ));
        }
        Err(AddrError::Parse) => {
            return Err(format!(
                "Failed to parse address \"{}\"",
                String::from_utf8_lossy(addr)
            ));
        }
    };
    open_client(&t, timeout, false).map_err(|(_, m)| m)
}

fn ctx_value(a: &Args, i: usize) -> Option<Value> {
    let v = a.val(i);
    with_context(&v, |_| ()).map(|_| v)
}

fn stream_socket_client(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let addr = a.str(0);
    let Some(timeout) = float_timeout(vm, a, 3)? else {
        return Ok(Value::Null);
    };
    let flags = a.int_or(4, 4);
    let ctx = ctx_value(a, 5);
    // NUL bytes show as \0 (test streams/ghsa-3cr5-j632-f35r.phpt).
    let mut shown = Vec::with_capacity(addr.len());
    for &b in addr.as_bytes() {
        if b == 0 {
            shown.extend_from_slice(b"\\0");
        } else {
            shown.push(b);
        }
    }
    client_common(vm, a, addr.as_bytes(), &shown, timeout, flags, ctx, 1)
}

fn fsockopen(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let host = a.str(0);
    let port = a.int_or(1, -1);
    let Some(timeout) = float_timeout(vm, a, 4)? else {
        return Ok(Value::Null);
    };
    let mut addr = host.as_bytes().to_vec();
    // The message shows the port whenever one is passed, -1 included
    // (observed on PHP 8.5: "Unable to connect to tcp://127.0.0.1:1:-1").
    // The message shows the C string (test
    // ext/standard/tests/network/ghsa-3cr5-j632-f35r.phpt).
    let mut shown = match addr.iter().position(|&b| b == 0) {
        Some(p) => addr[..p].to_vec(),
        None => addr.clone(),
    };
    if a.has(1) {
        shown.extend_from_slice(format!(":{port}").as_bytes());
    }
    if port > 0 {
        addr.extend_from_slice(format!(":{port}").as_bytes());
    }
    client_common(vm, a, &addr, &shown, timeout, 4, None, 2)
}

fn stream_socket_server(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let addr = a.str(0);
    let flags = a.int_or(3, 12);
    let ctx = ctx_value(a, 4);
    set_err(a, 1, 0, "");
    let fail = |vm: &mut Vm, code: i32, msg: &str| -> R<Value> {
        set_err(a, 1, code, msg);
        vm.fwarn(
            a,
            &format!(
                "Unable to connect to {} ({})",
                String::from_utf8_lossy(addr.as_bytes()),
                msg
            ),
        )?;
        Ok(Value::Bool(false))
    };
    let t = match parse_target(addr.as_bytes()) {
        Ok(t) => t,
        Err(AddrError::Transport(tr)) => {
            return fail(
                vm,
                0,
                &format!(
                    "Unable to find the socket transport \"{tr}\" - did you forget to enable it when you configured PHP?"
                ),
            );
        }
        Err(AddrError::Parse) => {
            return fail(
                vm,
                0,
                &format!(
                    "Failed to parse address \"{}\"",
                    String::from_utf8_lossy(addr.as_bytes())
                ),
            );
        }
    };
    let mut t = t;
    unix_path_limit(vm, a, &mut t)?;
    let backlog = ctx
        .as_ref()
        .and_then(|c| context_option(c, "socket", "backlog"))
        .map(|v| qint(&v) as i32)
        .unwrap_or(32);
    let mut sock = match open_server(&t, flags, backlog) {
        Ok(s) => s,
        Err((code, msg)) => return fail(vm, code, &msg),
    };
    sock.ctx = ctx.or_else(|| Some(default_context()));
    sock.transport_method = transport_method(&t.transport, true);
    sock.uri = Some(addr.clone());
    Ok(sock_stream(sock, addr.as_bytes()))
}

fn stream_socket_accept(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let Some(timeout) = float_timeout(vm, a, 1)? else {
        return Ok(Value::Null);
    };
    vm.out.flush();
    let r = with_sock(vm, a, 0, |vm, s| {
        let Some(s) = s else {
            vm.fwarn(a, "Accept failed: Operation not supported")?;
            return Ok(None);
        };
        let fd = s.fd.as_raw_fd();
        match wait_fd(fd, libc::POLLIN, timeout_of(timeout)) {
            Ok(true) => {}
            Ok(false) => {
                vm.fwarn(a, "Accept failed: Connection timed out")?;
                return Ok(None);
            }
            Err(e) => {
                vm.fwarn(
                    a,
                    &format!("Accept failed: {}", strerror(e.raw_os_error().unwrap_or(0))),
                )?;
                return Ok(None);
            }
        }
        // SAFETY: zeroed storage.
        let mut st: libc::sockaddr_storage = unsafe { std::mem::zeroed() };
        let mut len = std::mem::size_of::<libc::sockaddr_storage>() as libc::socklen_t;
        // SAFETY: accept4 on our listening descriptor.
        let nfd = unsafe {
            libc::accept4(
                fd,
                (&mut st as *mut libc::sockaddr_storage).cast(),
                &mut len,
                libc::SOCK_CLOEXEC | libc::SOCK_NONBLOCK,
            )
        };
        if nfd < 0 {
            vm.fwarn(a, &format!("Accept failed: {}", strerror(errno())))?;
            return Ok(None);
        }
        // SAFETY: a fresh descriptor we own.
        let ofd = unsafe { OwnedFd::from_raw_fd(nfd) };
        // The "socket" context's tcp_nodelay applies to accepted
        // connections, not the listening socket (test streams/
        // stream_context_tcp_nodelay_server.phpt).
        if s.kind == Kind::Tcp
            && let Some(c) = &s.ctx
            && context_option(c, "socket", "tcp_nodelay").is_some_and(|v| qint(&v) != 0)
        {
            let one: libc::c_int = 1;
            // SAFETY: setsockopt with an int.
            unsafe {
                libc::setsockopt(
                    nfd,
                    libc::IPPROTO_TCP,
                    libc::TCP_NODELAY,
                    (&one as *const libc::c_int).cast(),
                    std::mem::size_of::<libc::c_int>() as libc::socklen_t,
                )
            };
        }
        let peer = sockaddr_to_std((&st as *const libc::sockaddr_storage).cast(), len)
            .map(|sa| addr_text(&sa).into_bytes())
            .or_else(|| sock_name(nfd, true));
        let mut c = Sock::new(ofd, s.kind);
        c.ctx = s.ctx.clone();
        c.transport_method = s.transport_method;
        Ok(Some((c, peer)))
    })?;
    let Some((mut c, peer)) = r else {
        return Ok(Value::Bool(false));
    };
    if a.ref_at(2).is_some() {
        a.set_ref(
            2,
            match &peer {
                Some(p) => Value::Str(PhpStr::new(p)),
                None => Value::Null,
            },
        );
    }
    if c.transport_method != 0 {
        c.timeout = timeout_of(timeout);
        let r = super::openssl::tls::enable(vm, a, &mut c, None, true)?;
        if r != super::openssl::tls::Enabled::Yes {
            vm.fwarn(a, "Accept failed: Cannot enable crypto")?;
            return Ok(Value::Bool(false));
        }
    }
    c.timeout = timeout_of(default_timeout(vm));
    Ok(sock_stream(c, b""))
}

fn stream_socket_get_name(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let remote = a.bool(1);
    with_sock(vm, a, 0, |_vm, s| {
        Ok(match s.and_then(|s| sock_name(s.fd.as_raw_fd(), remote)) {
            Some(n) => Value::Str(PhpStr::from(n)),
            None => Value::Bool(false),
        })
    })
}

fn stream_socket_shutdown(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let mode = a.int(1);
    if !(0..=2).contains(&mode) {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #2 ($mode) must be one of STREAM_SHUT_RD, STREAM_SHUT_WR, or STREAM_SHUT_RDWR",
        );
    }
    with_sock(vm, a, 0, |_vm, s| {
        let Some(s) = s else {
            return Ok(Value::Bool(false));
        };
        let how = match mode {
            0 => libc::SHUT_RD,
            1 => libc::SHUT_WR,
            _ => libc::SHUT_RDWR,
        };
        // SAFETY: shutdown(2) on our descriptor.
        Ok(Value::Bool(
            unsafe { libc::shutdown(s.fd.as_raw_fd(), how) } == 0,
        ))
    })
}

fn stream_socket_enable_crypto(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let enable = a.bool(1);
    let method = a.opt_int(2);
    with_sock(vm, a, 0, |vm, s| {
        let Some(s) = s else {
            vm.fwarn(a, "This stream does not support SSL/crypto")?;
            return Ok(Value::Bool(false));
        };
        if !enable {
            if s.crypto_on {
                let (mut io, tls) = s.parts();
                if let Some(t) = tls {
                    t.close(&mut io);
                }
                s.crypto_on = false;
                s.tls = None;
            }
            return Ok(Value::Bool(true));
        }
        if s.crypto_on {
            return Ok(Value::Bool(true));
        }
        let server =
            s.listening || s.transport_method & 1 == 0 && method.is_some_and(|m| m & 1 == 0);
        Ok(
            match super::openssl::tls::enable(vm, a, s, method, server)? {
                super::openssl::tls::Enabled::Yes => Value::Bool(true),
                super::openssl::tls::Enabled::Pending => Value::Int(0),
                super::openssl::tls::Enabled::No => Value::Bool(false),
            },
        )
    })
}

fn stream_socket_pair(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let domain = a.int(0) as i32;
    let ty = a.int(1) as i32;
    let proto = a.int(2) as i32;
    let mut fds = [0i32; 2];
    // SAFETY: socketpair into a two-int array.
    let rc = unsafe {
        libc::socketpair(
            domain,
            ty | libc::SOCK_CLOEXEC | libc::SOCK_NONBLOCK,
            proto,
            fds.as_mut_ptr(),
        )
    };
    if rc != 0 {
        vm.fwarn(
            a,
            &format!(
                "Failed to create sockets: [{}]: {}",
                errno(),
                strerror(errno())
            ),
        )?;
        return Ok(Value::Bool(false));
    }
    let mut out = PhpArray::new();
    for fd in fds {
        // SAFETY: fresh descriptors we own.
        let s = Sock::new(unsafe { OwnedFd::from_raw_fd(fd) }, Kind::Pair);
        let _ = out.push(sock_stream(s, b""));
    }
    Ok(Value::Array(out))
}

fn stream_socket_recvfrom(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let len = a.int(1);
    let flags = a.int_or(2, 0) as i32;
    if len <= 0 {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #2 ($length) must be greater than 0",
        );
    }
    if a.ref_at(3).is_some() {
        a.set_ref(3, Value::Null);
    }
    let r = with_sock(vm, a, 0, |_vm, s| {
        let Some(s) = s else {
            return Ok(None);
        };
        if s.listening {
            return Ok(None);
        }
        let mut buf = vec![0u8; len as usize];
        // SAFETY: zeroed storage.
        let mut st: libc::sockaddr_storage = unsafe { std::mem::zeroed() };
        let mut alen = std::mem::size_of::<libc::sockaddr_storage>() as libc::socklen_t;
        let fd = s.fd.as_raw_fd();
        if s.blocking && !wait_fd(fd, libc::POLLIN, s.timeout).unwrap_or(false) {
            return Ok(None);
        }
        // SAFETY: valid buffers.
        let n = unsafe {
            libc::recvfrom(
                fd,
                buf.as_mut_ptr().cast(),
                buf.len(),
                flags | libc::MSG_DONTWAIT,
                (&mut st as *mut libc::sockaddr_storage).cast(),
                &mut alen,
            )
        };
        if n < 0 {
            return Ok(None);
        }
        buf.truncate(n as usize);
        let from = sockaddr_to_std((&st as *const libc::sockaddr_storage).cast(), alen)
            .map(|sa| addr_text(&sa).into_bytes());
        Ok(Some((buf, from)))
    })?;
    let Some((buf, from)) = r else {
        return Ok(Value::Bool(false));
    };
    if a.ref_at(3).is_some()
        && let Some(f) = from
    {
        a.set_ref(3, Value::Str(PhpStr::from(f)));
    }
    Ok(Value::Str(PhpStr::from(buf)))
}

fn stream_socket_sendto(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let data = a.str(1);
    let flags = a.int_or(2, 0) as i32;
    let addr = a.str(3);
    with_sock(vm, a, 0, |vm, s| {
        let Some(s) = s else {
            return Ok(Value::Bool(false));
        };
        let fd = s.fd.as_raw_fd();
        let n = if addr.is_empty() {
            // SAFETY: valid buffer.
            unsafe {
                libc::send(
                    fd,
                    data.as_ptr().cast(),
                    data.len(),
                    flags | libc::MSG_NOSIGNAL,
                )
            }
        } else {
            let t = match parse_target(addr.as_bytes()) {
                Ok(t) => t,
                Err(_) => {
                    vm.fwarn(
                        a,
                        &format!(
                            "Failed to parse `{}' into a valid network address",
                            String::from_utf8_lossy(addr.as_bytes())
                        ),
                    )?;
                    return Ok(Value::Bool(false));
                }
            };
            let Some(sa) = resolve(&t.host, t.port, true)
                .ok()
                .and_then(|v| v.first().copied())
            else {
                return Ok(Value::Bool(false));
            };
            let (st, len) = std_to_sockaddr(&sa);
            // SAFETY: valid buffer and address.
            unsafe {
                libc::sendto(
                    fd,
                    data.as_ptr().cast(),
                    data.len(),
                    flags | libc::MSG_NOSIGNAL,
                    (&st as *const libc::sockaddr_storage).cast(),
                    len,
                )
            }
        };
        Ok(if n < 0 {
            Value::Bool(false)
        } else {
            Value::Int(n as i64)
        })
    })
}

fn stream_get_transports(_vm: &mut Vm, _a: &mut Args) -> R<Value> {
    Ok(Value::Array(PhpArray::from_values(
        TRANSPORTS.iter().map(|t| Value::from(*t)),
    )))
}

fn stream_set_blocking(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let enable = a.bool(1);
    with_stream(vm, a, 0, |vm, s| {
        if let Some(r) = super::userstream::set_option(
            vm,
            s,
            "stream_set_blocking",
            1,
            enable as i64,
            Value::Null,
        )? {
            return Ok(Value::Bool(r));
        }
        Ok(Value::Bool(match &mut s.backing {
            Backing::Native(n) => n.set_blocking(enable) || !n.is_socket(),
            Backing::File(f) => {
                let fd = f.as_raw_fd();
                // SAFETY: fcntl flags on our descriptor.
                unsafe {
                    let fl = libc::fcntl(fd, libc::F_GETFL);
                    if fl >= 0 {
                        let nfl = if enable {
                            fl & !libc::O_NONBLOCK
                        } else {
                            fl | libc::O_NONBLOCK
                        };
                        libc::fcntl(fd, libc::F_SETFL, nfl);
                    }
                }
                true
            }
            _ => true,
        }))
    })
}

fn stream_set_timeout(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let secs = a.int(1) as f64 + a.int_or(2, 0) as f64 / 1e6;
    if let Value::Resource(r) = a.val(0)
        && r.is_closed()
    {
        return vm.fthrow(
            a,
            "TypeError",
            "Argument #1 ($stream) must be an open stream resource",
        );
    }
    let (sec, usec) = (a.int(1), a.int_or(2, 0));
    with_stream(vm, a, 0, |vm, s| {
        if let Some(r) =
            super::userstream::set_option(vm, s, "stream_set_timeout", 4, sec, Value::Int(usec))?
        {
            return Ok(Value::Bool(r));
        }
        Ok(Value::Bool(match &mut s.backing {
            Backing::Native(n) => n.set_timeout(secs),
            Backing::File(_) => s.is_pipe(),
            _ => false,
        }))
    })
}

// ------------------------------------------------------------ helpers

/// An option value as an integer (PHP's loose conversion, no diagnostics).
pub fn qint(v: &Value) -> i64 {
    match v.deref() {
        Value::Int(n) => n,
        Value::Float(f) => goro_runtime::convert::float_to_int_wrapping(f),
        Value::Bool(b) => b as i64,
        Value::Str(s) => goro_runtime::convert::str_to_int(&s),
        _ => 0,
    }
}

/// An option value as a float.
pub fn qfloat(v: &Value) -> f64 {
    match v.deref() {
        Value::Int(n) => n as f64,
        Value::Float(f) => f,
        Value::Bool(b) => b as i64 as f64,
        Value::Str(s) => goro_runtime::convert::str_to_float(&s),
        _ => 0.0,
    }
}

/// An option value as a string.
pub fn qstr(v: &Value) -> PhpStr {
    match v.deref() {
        Value::Str(s) => s,
        Value::Int(n) => goro_runtime::convert::int_to_string(n),
        Value::Float(f) => PhpStr::from(goro_runtime::convert::float_repr(f)),
        Value::Bool(true) => PhpStr::from("1"),
        _ => PhpStr::empty(),
    }
}
