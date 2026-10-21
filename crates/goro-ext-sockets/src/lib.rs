use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::os::unix::io::FromRawFd;
use std::rc::Rc;

use goro_core::array::{ArrayKey, PhpArray};
use goro_core::object::{ClassEntry, PhpObject};
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};

/// Socket handle variants
enum SocketHandle {
    TcpClient(TcpStream),
    TcpServer(TcpListener),
    Udp(UdpSocket),
    Closed,
}

/// AddrInfo handle for socket_addrinfo_*
#[derive(Clone, Debug)]
struct AddrInfoEntry {
    family: i32,
    socktype: i32,
    protocol: i32,
    flags: i32,
    addr: SocketAddr,
}

thread_local! {
    static SOCKETS: RefCell<HashMap<i64, SocketHandle>> = RefCell::new(HashMap::new());
    static LAST_ERROR: Cell<i64> = const { Cell::new(0) };
    static ADDRINFOS: RefCell<HashMap<i64, AddrInfoEntry>> = RefCell::new(HashMap::new());
}

fn set_last_error(code: i64) {
    LAST_ERROR.with(|c| c.set(code));
}

fn get_last_error() -> i64 {
    LAST_ERROR.with(|c| c.get())
}

// Socket domain/type/protocol constants
const AF_UNIX: i64 = 1;
const AF_INET: i64 = 2;
const AF_INET6: i64 = 10;
const SOCK_STREAM: i64 = 1;
const SOCK_DGRAM: i64 = 2;
const SOCK_RAW: i64 = 3;
const SOCK_SEQPACKET: i64 = 5;
const SOCK_RDM: i64 = 4;

// Socket option constants (used in set/get socket option implementations)
const SOL_SOCKET: i64 = 1;
const SO_RCVTIMEO: i64 = 20;
const SO_SNDTIMEO: i64 = 21;
const SO_LINGER: i64 = 13;

// Read mode constants
const PHP_BINARY_READ: i64 = 2;

/// Make a Socket object whose object_id is `id`. The handle must already be
/// registered in SOCKETS under `id`.
fn make_socket_object(id: i64) -> Value {
    let obj = PhpObject::new(b"Socket".to_vec(), id as u64);
    Value::Object(Rc::new(RefCell::new(obj)))
}

/// Make an AddressInfo object whose object_id is `id`. The entry must already be
/// registered in ADDRINFOS under `id`.
fn make_addrinfo_object(id: i64) -> Value {
    let obj = PhpObject::new(b"AddressInfo".to_vec(), id as u64);
    Value::Object(Rc::new(RefCell::new(obj)))
}

/// Allocate the next socket id by using the vm's object id allocator.
fn alloc_socket_id_via_vm(vm: &mut Vm) -> i64 {
    vm.next_object_id() as i64
}

/// Helper: extract socket id (object_id) from arg.
/// Accepts Socket object or raw Long for backward-compat.
fn arg_socket_id(arg: Option<&Value>) -> Option<i64> {
    match arg {
        Some(Value::Object(obj)) => {
            let b = obj.borrow();
            if b.class_name == b"Socket" {
                Some(b.object_id as i64)
            } else {
                None
            }
        }
        Some(Value::Long(n)) => Some(*n),
        Some(Value::Reference(r)) => arg_socket_id(Some(&r.borrow().clone())),
        _ => None,
    }
}

/// Helper: extract addrinfo id from arg
fn arg_addrinfo_id(arg: Option<&Value>) -> Option<i64> {
    match arg {
        Some(Value::Object(obj)) => {
            let b = obj.borrow();
            if b.class_name == b"AddressInfo" {
                Some(b.object_id as i64)
            } else {
                None
            }
        }
        Some(Value::Long(n)) => Some(*n),
        Some(Value::Reference(r)) => arg_addrinfo_id(Some(&r.borrow().clone())),
        _ => None,
    }
}

/// Throw a ValueError exception
fn throw_value_error(vm: &mut Vm, msg: &str) -> VmError {
    let exc = vm.create_exception(b"ValueError", msg, vm.current_line);
    vm.current_exception = Some(exc);
    VmError {
        message: msg.to_string(),
        line: vm.current_line,
    }
}

/// Throw a TypeError exception
fn throw_type_error(vm: &mut Vm, msg: &str) -> VmError {
    let exc = vm.create_exception(b"TypeError", msg, vm.current_line);
    vm.current_exception = Some(exc);
    VmError {
        message: msg.to_string(),
        line: vm.current_line,
    }
}

/// Register all socket extension functions and constants
pub fn register(vm: &mut Vm) {
    vm.register_extension(b"sockets");

    // Register Socket and AddressInfo classes
    let socket_class = ClassEntry::new(b"Socket".to_vec());
    vm.classes.insert(b"socket".to_vec(), socket_class);
    let addrinfo_class = ClassEntry::new(b"AddressInfo".to_vec());
    vm.classes.insert(b"addressinfo".to_vec(), addrinfo_class);

    // Functions
    vm.register_function(b"socket_create", socket_create);
    vm.register_function(b"socket_connect", socket_connect);
    vm.register_function(b"socket_bind", socket_bind);
    vm.register_function(b"socket_listen", socket_listen);
    vm.register_function(b"socket_accept", socket_accept);
    vm.register_function(b"socket_read", socket_read);
    vm.register_function(b"socket_write", socket_write);
    vm.register_function(b"socket_close", socket_close);
    vm.register_function(b"socket_send", socket_send);
    vm.register_function(b"socket_recv", socket_recv);
    vm.register_function(b"socket_set_option", socket_set_option);
    vm.register_function(b"socket_get_option", socket_get_option);
    vm.register_function(b"socket_setopt", socket_set_option);
    vm.register_function(b"socket_getopt", socket_get_option);
    vm.register_function(b"socket_last_error", socket_last_error);
    vm.register_function(b"socket_strerror", socket_strerror);
    vm.register_function(b"socket_set_nonblock", socket_set_nonblock);
    vm.register_function(b"socket_set_block", socket_set_block);
    vm.register_function(b"socket_getpeername", socket_getpeername);
    vm.register_function(b"socket_getsockname", socket_getsockname);
    vm.register_function(b"socket_shutdown", socket_shutdown);
    vm.register_function(b"socket_select", socket_select);
    vm.register_function(b"socket_clear_error", socket_clear_error);
    vm.register_function(b"socket_create_listen", socket_create_listen_fn);
    vm.register_function(b"socket_create_pair", socket_create_pair_fn);
    vm.register_function(b"socket_import_stream", socket_import_stream_fn);
    vm.register_function(b"socket_export_stream", socket_export_stream_fn);
    vm.register_function(b"socket_addrinfo_lookup", socket_addrinfo_lookup_fn);
    vm.register_function(b"socket_addrinfo_connect", socket_addrinfo_connect_fn);
    vm.register_function(b"socket_addrinfo_bind", socket_addrinfo_bind_fn);
    vm.register_function(b"socket_addrinfo_explain", socket_addrinfo_explain_fn);
    vm.register_function(b"socket_sendto", socket_sendto_fn);
    vm.register_function(b"socket_recvfrom", socket_recvfrom_fn);
    vm.register_function(b"socket_cmsg_space", socket_cmsg_space_fn);
    vm.register_function(b"socket_sendmsg", socket_sendmsg_fn);
    vm.register_function(b"socket_recvmsg", socket_recvmsg_fn);
    vm.register_function(b"socket_wsaprotocol_info_export", socket_stub_false);
    vm.register_function(b"socket_wsaprotocol_info_import", socket_stub_false);
    vm.register_function(b"socket_wsaprotocol_info_release", socket_stub_false);
    vm.register_function(b"socket_atomark", socket_atomark_fn);

    // Constants
    vm.constants.insert(b"AF_INET".to_vec(), Value::Long(2));
    vm.constants.insert(b"AF_INET6".to_vec(), Value::Long(10));
    vm.constants.insert(b"AF_UNIX".to_vec(), Value::Long(1));
    vm.constants.insert(b"SOCK_STREAM".to_vec(), Value::Long(1));
    vm.constants.insert(b"SOCK_DGRAM".to_vec(), Value::Long(2));
    vm.constants.insert(b"SOCK_RAW".to_vec(), Value::Long(3));
    vm.constants
        .insert(b"SOCK_SEQPACKET".to_vec(), Value::Long(5));
    vm.constants.insert(b"SOCK_RDM".to_vec(), Value::Long(4));
    vm.constants.insert(b"SOL_SOCKET".to_vec(), Value::Long(1));
    vm.constants.insert(b"SOL_TCP".to_vec(), Value::Long(6));
    vm.constants.insert(b"SOL_UDP".to_vec(), Value::Long(17));
    vm.constants
        .insert(b"SO_REUSEADDR".to_vec(), Value::Long(2));
    vm.constants
        .insert(b"SO_KEEPALIVE".to_vec(), Value::Long(9));
    vm.constants
        .insert(b"SO_BROADCAST".to_vec(), Value::Long(6));
    vm.constants
        .insert(b"SO_RCVTIMEO".to_vec(), Value::Long(20));
    vm.constants
        .insert(b"SO_SNDTIMEO".to_vec(), Value::Long(21));
    vm.constants.insert(b"SO_RCVBUF".to_vec(), Value::Long(8));
    vm.constants.insert(b"SO_SNDBUF".to_vec(), Value::Long(7));
    vm.constants.insert(b"TCP_NODELAY".to_vec(), Value::Long(1));
    vm.constants
        .insert(b"MSG_DONTWAIT".to_vec(), Value::Long(64));
    vm.constants.insert(b"MSG_PEEK".to_vec(), Value::Long(2));
    vm.constants
        .insert(b"PHP_NORMAL_READ".to_vec(), Value::Long(1));
    vm.constants
        .insert(b"PHP_BINARY_READ".to_vec(), Value::Long(2));
    vm.constants
        .insert(b"SOCKET_EINTR".to_vec(), Value::Long(4));
    vm.constants
        .insert(b"SOCKET_EACCES".to_vec(), Value::Long(13));
    vm.constants
        .insert(b"SOCKET_EAGAIN".to_vec(), Value::Long(11));
    vm.constants
        .insert(b"SOCKET_EWOULDBLOCK".to_vec(), Value::Long(11));
    vm.constants
        .insert(b"SOCKET_ECONNREFUSED".to_vec(), Value::Long(111));
    vm.constants
        .insert(b"SOCKET_ETIMEDOUT".to_vec(), Value::Long(110));
    vm.constants
        .insert(b"SOCKET_ECONNRESET".to_vec(), Value::Long(104));
    vm.constants
        .insert(b"SOCKET_EINVAL".to_vec(), Value::Long(22));
    vm.constants
        .insert(b"SOCKET_ENOTCONN".to_vec(), Value::Long(107));
    vm.constants
        .insert(b"SOCKET_EPIPE".to_vec(), Value::Long(32));
    vm.constants
        .insert(b"SOCKET_EBADF".to_vec(), Value::Long(9));
    vm.constants
        .insert(b"SOCKET_EMFILE".to_vec(), Value::Long(24));
    vm.constants
        .insert(b"SOCKET_ENOBUFS".to_vec(), Value::Long(105));
    vm.constants
        .insert(b"SOCKET_EADDRINUSE".to_vec(), Value::Long(98));
    vm.constants
        .insert(b"SOCKET_EADDRNOTAVAIL".to_vec(), Value::Long(99));
    vm.constants
        .insert(b"SOCKET_EAFNOSUPPORT".to_vec(), Value::Long(97));
    vm.constants
        .insert(b"SOCKET_EPROTONOSUPPORT".to_vec(), Value::Long(93));

    // Shutdown constants
    vm.constants.insert(b"SHUT_RD".to_vec(), Value::Long(0));
    vm.constants.insert(b"SHUT_WR".to_vec(), Value::Long(1));
    vm.constants.insert(b"SHUT_RDWR".to_vec(), Value::Long(2));

    // Missing socket constants
    vm.constants.insert(b"IPPROTO_IP".to_vec(), Value::Long(0));
    vm.constants
        .insert(b"IPPROTO_IPV6".to_vec(), Value::Long(41));
    vm.constants.insert(b"MSG_OOB".to_vec(), Value::Long(1));
    vm.constants
        .insert(b"MSG_WAITALL".to_vec(), Value::Long(256));
    vm.constants.insert(b"MSG_EOF".to_vec(), Value::Long(512));
    vm.constants
        .insert(b"MSG_DONTROUTE".to_vec(), Value::Long(4));
    vm.constants.insert(b"SO_TYPE".to_vec(), Value::Long(3));
    vm.constants.insert(b"SO_LINGER".to_vec(), Value::Long(13));
    vm.constants.insert(b"SO_ERROR".to_vec(), Value::Long(4));
    vm.constants
        .insert(b"SO_REUSEPORT".to_vec(), Value::Long(15));
    vm.constants
        .insert(b"SO_OOBINLINE".to_vec(), Value::Long(10));
    vm.constants
        .insert(b"IP_MULTICAST_LOOP".to_vec(), Value::Long(34));
    vm.constants
        .insert(b"IP_MULTICAST_TTL".to_vec(), Value::Long(33));
    vm.constants
        .insert(b"IP_MULTICAST_IF".to_vec(), Value::Long(32));
    vm.constants.insert(b"IP_TOS".to_vec(), Value::Long(1));
    vm.constants.insert(b"IP_TTL".to_vec(), Value::Long(2));
    vm.constants
        .insert(b"IPV6_MULTICAST_LOOP".to_vec(), Value::Long(19));
    vm.constants
        .insert(b"IPV6_MULTICAST_HOPS".to_vec(), Value::Long(18));
    vm.constants
        .insert(b"IPV6_MULTICAST_IF".to_vec(), Value::Long(17));
    vm.constants
        .insert(b"IPV6_V6ONLY".to_vec(), Value::Long(26));
    vm.constants
        .insert(b"MCAST_JOIN_GROUP".to_vec(), Value::Long(42));
    vm.constants
        .insert(b"MCAST_LEAVE_GROUP".to_vec(), Value::Long(45));
    vm.constants
        .insert(b"MCAST_BLOCK_SOURCE".to_vec(), Value::Long(43));
    vm.constants
        .insert(b"MCAST_UNBLOCK_SOURCE".to_vec(), Value::Long(44));
    vm.constants
        .insert(b"MCAST_JOIN_SOURCE_GROUP".to_vec(), Value::Long(46));
    vm.constants
        .insert(b"MCAST_LEAVE_SOURCE_GROUP".to_vec(), Value::Long(47));
    vm.constants
        .insert(b"STREAM_IPPROTO_IP".to_vec(), Value::Long(0));
    vm.constants
        .insert(b"STREAM_IPPROTO_TCP".to_vec(), Value::Long(6));
    vm.constants
        .insert(b"STREAM_IPPROTO_UDP".to_vec(), Value::Long(17));
    vm.constants
        .insert(b"STREAM_IPPROTO_ICMP".to_vec(), Value::Long(1));
    vm.constants
        .insert(b"STREAM_IPPROTO_RAW".to_vec(), Value::Long(255));

    // AI_* flags for addrinfo
    vm.constants.insert(b"AI_PASSIVE".to_vec(), Value::Long(1));
    vm.constants
        .insert(b"AI_CANONNAME".to_vec(), Value::Long(2));
    vm.constants
        .insert(b"AI_NUMERICHOST".to_vec(), Value::Long(4));
    vm.constants.insert(b"AI_V4MAPPED".to_vec(), Value::Long(8));
    vm.constants.insert(b"AI_ALL".to_vec(), Value::Long(16));
    vm.constants
        .insert(b"AI_ADDRCONFIG".to_vec(), Value::Long(32));
    vm.constants
        .insert(b"AI_NUMERICSERV".to_vec(), Value::Long(1024));
}

/// socket_create(int $domain, int $type, int $protocol): Socket|false
fn socket_create(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let domain = args.first().map(|v| v.to_long()).unwrap_or(0);
    let sock_type = args.get(1).map(|v| v.to_long()).unwrap_or(0);
    let protocol = args.get(2).map(|v| v.to_long()).unwrap_or(0);

    if domain != AF_INET && domain != AF_INET6 && domain != AF_UNIX {
        return Err(throw_value_error(
            vm,
            "socket_create(): Argument #1 ($domain) must be one of AF_UNIX, AF_INET6, or AF_INET",
        ));
    }

    if sock_type != SOCK_STREAM
        && sock_type != SOCK_DGRAM
        && sock_type != SOCK_RAW
        && sock_type != SOCK_SEQPACKET
        && sock_type != SOCK_RDM
    {
        return Err(throw_value_error(
            vm,
            "socket_create(): Argument #2 ($type) must be one of SOCK_STREAM, SOCK_DGRAM, SOCK_SEQPACKET, SOCK_RAW, or SOCK_RDM",
        ));
    }

    // Use libc socket() to create raw fd
    let fd = unsafe { libc::socket(domain as i32, sock_type as i32, protocol as i32) };
    if fd < 0 {
        let e = std::io::Error::last_os_error();
        set_last_error(map_io_error(&e));
        vm.emit_warning(&format!(
            "socket_create(): Unable to create socket [{}]: {}",
            map_io_error(&e),
            errno_to_string(map_io_error(&e))
        ));
        return Ok(Value::False);
    }

    // For TCP streams, wrap as a TcpStream-like placeholder via Closed for now -
    // we'll wrap the fd into appropriate type lazily. Actually, we can wrap as
    // TcpClient using FromRawFd, but for UDP we want UdpSocket. For unconnected
    // streams we keep them in Closed-ish state but with fd known.
    // We'll store the fd; conversion happens on connect/bind/listen.
    let id = alloc_socket_id_via_vm(vm);
    // Store as TcpClient by default for STREAM, Udp for DGRAM
    // Use FromRawFd to keep ownership properly.
    let handle = match (sock_type, domain) {
        (SOCK_STREAM, _) => {
            let stream = unsafe { TcpStream::from_raw_fd(fd) };
            SocketHandle::TcpClient(stream)
        }
        (SOCK_DGRAM, _) => {
            let sock = unsafe { UdpSocket::from_raw_fd(fd) };
            SocketHandle::Udp(sock)
        }
        _ => {
            // For SEQPACKET/RAW/RDM we just store as TcpClient placeholder
            let stream = unsafe { TcpStream::from_raw_fd(fd) };
            SocketHandle::TcpClient(stream)
        }
    };

    SOCKETS.with(|s| {
        s.borrow_mut().insert(id, handle);
    });
    set_last_error(0);
    Ok(make_socket_object(id))
}

/// socket_connect(Socket $socket, string $address, ?int $port = null): bool
fn socket_connect(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if args.len() < 2 {
        let msg = format!(
            "socket_connect() expects at least 2 arguments, {} given",
            args.len()
        );
        let exc = vm.create_exception(b"ArgumentCountError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: msg,
            line: vm.current_line,
        });
    }
    let socket_id = match arg_socket_id(args.first()) {
        Some(s) => s,
        None => return Ok(Value::False),
    };
    let address = args
        .get(1)
        .map(|v| v.to_php_string().to_string_lossy())
        .unwrap_or_default();
    let port_arg = args.get(2);

    // Determine if socket is bound. If TCP/UDP, port is required.
    // Check socket type to determine if unix
    let is_inet = SOCKETS.with(|s| {
        matches!(
            s.borrow().get(&socket_id),
            Some(SocketHandle::TcpClient(_))
                | Some(SocketHandle::TcpServer(_))
                | Some(SocketHandle::Udp(_))
        )
    });

    let port = match port_arg {
        None => {
            if is_inet {
                return Err(throw_value_error(
                    vm,
                    "socket_connect(): Argument #3 ($port) cannot be null when the socket type is AF_INET",
                ));
            }
            return unix_connect(vm, socket_id, &address);
        }
        Some(v) => {
            let v = v.deref();
            if matches!(v, Value::Null | Value::Undef) {
                if is_inet {
                    return Err(throw_value_error(
                        vm,
                        "socket_connect(): Argument #3 ($port) cannot be null when the socket type is AF_INET",
                    ));
                }
                return unix_connect(vm, socket_id, &address);
            }
            let p = v.to_long();
            if !(0..=65535).contains(&p) {
                return Err(throw_value_error(
                    vm,
                    "socket_connect(): Argument #3 ($port) must be between 0 and 65535",
                ));
            }
            p as u16
        }
    };

    let addr_str = format!("{}:{}", address, port);
    match addr_str.parse::<SocketAddr>() {
        Ok(addr) => match TcpStream::connect(addr) {
            Ok(stream) => {
                SOCKETS.with(|s| {
                    s.borrow_mut()
                        .insert(socket_id, SocketHandle::TcpClient(stream));
                });
                set_last_error(0);
                Ok(Value::True)
            }
            Err(e) => {
                set_last_error(map_io_error(&e));
                vm.emit_warning(&format!(
                    "socket_connect(): unable to connect [{}]: {}",
                    map_io_error(&e),
                    errno_to_string(map_io_error(&e))
                ));
                Ok(Value::False)
            }
        },
        Err(_) => {
            // Try DNS resolution
            match std::net::ToSocketAddrs::to_socket_addrs(&(&*address, port)) {
                Ok(mut addrs) => {
                    if let Some(addr) = addrs.next() {
                        match TcpStream::connect(addr) {
                            Ok(stream) => {
                                SOCKETS.with(|s| {
                                    s.borrow_mut()
                                        .insert(socket_id, SocketHandle::TcpClient(stream));
                                });
                                set_last_error(0);
                                Ok(Value::True)
                            }
                            Err(e) => {
                                set_last_error(map_io_error(&e));
                                vm.emit_warning(&format!(
                                    "socket_connect(): unable to connect [{}]: {}",
                                    map_io_error(&e),
                                    errno_to_string(map_io_error(&e))
                                ));
                                Ok(Value::False)
                            }
                        }
                    } else {
                        set_last_error(111);
                        vm.emit_warning("socket_connect(): unable to connect: No addresses found");
                        Ok(Value::False)
                    }
                }
                Err(e) => {
                    set_last_error(map_io_error(&e));
                    vm.emit_warning(&format!(
                        "socket_connect(): unable to connect [{}]: {}",
                        map_io_error(&e),
                        errno_to_string(map_io_error(&e))
                    ));
                    Ok(Value::False)
                }
            }
        }
    }
}

fn unix_connect(vm: &mut Vm, _socket_id: i64, _path: &str) -> Result<Value, VmError> {
    // Unix sockets - we don't fully support them; emit warning
    set_last_error(22);
    vm.emit_warning("socket_connect(): unable to connect: Unix socket connect not fully supported");
    Ok(Value::False)
}

/// socket_bind(Socket $socket, string $address, int $port = 0): bool
fn socket_bind(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let socket_id = match arg_socket_id(args.first()) {
        Some(s) => s,
        None => return Ok(Value::False),
    };
    let address = args
        .get(1)
        .map(|v| v.to_php_string().to_string_lossy())
        .unwrap_or_default();
    let port = args.get(2).map(|v| v.to_long()).unwrap_or(0);

    if !(0..=65535).contains(&port) {
        return Err(throw_value_error(
            vm,
            "socket_bind(): Argument #3 ($port) must be between 0 and 65535",
        ));
    }

    let port_u16 = port as u16;
    let addr_str = format!("{}:{}", address, port_u16);

    // Determine if socket is UDP or TCP-stream
    let is_udp = SOCKETS.with(|s| matches!(s.borrow().get(&socket_id), Some(SocketHandle::Udp(_))));

    if is_udp {
        match UdpSocket::bind(&addr_str) {
            Ok(sock) => {
                SOCKETS.with(|s| {
                    s.borrow_mut().insert(socket_id, SocketHandle::Udp(sock));
                });
                set_last_error(0);
                Ok(Value::True)
            }
            Err(e) => {
                set_last_error(map_io_error(&e));
                vm.emit_warning(&format!(
                    "socket_bind(): unable to bind address [{}]: {}",
                    map_io_error(&e),
                    errno_to_string(map_io_error(&e))
                ));
                Ok(Value::False)
            }
        }
    } else {
        match TcpListener::bind(&addr_str) {
            Ok(listener) => {
                SOCKETS.with(|s| {
                    s.borrow_mut()
                        .insert(socket_id, SocketHandle::TcpServer(listener));
                });
                set_last_error(0);
                Ok(Value::True)
            }
            Err(e) => {
                set_last_error(map_io_error(&e));
                vm.emit_warning(&format!(
                    "socket_bind(): unable to bind address [{}]: {}",
                    map_io_error(&e),
                    errno_to_string(map_io_error(&e))
                ));
                Ok(Value::False)
            }
        }
    }
}

/// socket_listen(Socket $socket, int $backlog = 0): bool
fn socket_listen(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let socket_id = match arg_socket_id(args.first()) {
        Some(s) => s,
        None => return Ok(Value::False),
    };

    let exists =
        SOCKETS.with(|s| matches!(s.borrow().get(&socket_id), Some(SocketHandle::TcpServer(_))));

    if exists {
        set_last_error(0);
        Ok(Value::True)
    } else {
        set_last_error(95);
        vm.emit_warning(&format!(
            "socket_listen(): unable to listen on socket [{}]: Operation not supported",
            95
        ));
        Ok(Value::False)
    }
}

/// socket_accept(Socket $socket): Socket|false
fn socket_accept(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let socket_id = match arg_socket_id(args.first()) {
        Some(s) => s,
        None => return Ok(Value::False),
    };

    let result = SOCKETS.with(|s| {
        let sockets = s.borrow();
        match sockets.get(&socket_id) {
            Some(SocketHandle::TcpServer(listener)) => match listener.accept() {
                Ok((stream, _addr)) => Ok(stream),
                Err(e) => Err(e),
            },
            _ => Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Not a server socket",
            )),
        }
    });

    match result {
        Ok(stream) => {
            let id = alloc_socket_id_via_vm(vm);
            SOCKETS.with(|s| {
                s.borrow_mut().insert(id, SocketHandle::TcpClient(stream));
            });
            set_last_error(0);
            Ok(make_socket_object(id))
        }
        Err(e) => {
            set_last_error(map_io_error(&e));
            vm.emit_warning(&format!(
                "socket_accept(): unable to accept incoming connection [{}]: {}",
                map_io_error(&e),
                errno_to_string(map_io_error(&e))
            ));
            Ok(Value::False)
        }
    }
}

/// socket_read(Socket $socket, int $length, int $mode = PHP_BINARY_READ): string|false
fn socket_read(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let socket_id = match arg_socket_id(args.first()) {
        Some(s) => s,
        None => return Ok(Value::False),
    };
    let length_l = args.get(1).map(|v| v.to_long()).unwrap_or(1024);
    if length_l < 0 || length_l > i32::MAX as i64 {
        return Ok(Value::False);
    }
    let length = length_l as usize;
    let mode = args.get(2).map(|v| v.to_long()).unwrap_or(PHP_BINARY_READ);

    let result = SOCKETS.with(|s| {
        let mut sockets = s.borrow_mut();
        match sockets.get_mut(&socket_id) {
            Some(SocketHandle::TcpClient(stream)) => {
                let mut buf = vec![0u8; length];
                if mode == PHP_BINARY_READ {
                    match stream.read(&mut buf) {
                        Ok(n) => {
                            buf.truncate(n);
                            Ok(buf)
                        }
                        Err(e) => Err(e),
                    }
                } else {
                    let mut result = Vec::with_capacity(length);
                    let mut one = [0u8; 1];
                    loop {
                        if result.len() >= length {
                            break;
                        }
                        match stream.read(&mut one) {
                            Ok(0) => break,
                            Ok(_) => {
                                if one[0] == b'\n' || one[0] == 0 {
                                    result.push(one[0]);
                                    break;
                                }
                                result.push(one[0]);
                            }
                            Err(e) => {
                                if result.is_empty() {
                                    return Err(e);
                                }
                                break;
                            }
                        }
                    }
                    Ok(result)
                }
            }
            Some(SocketHandle::Udp(sock)) => {
                let mut buf = vec![0u8; length];
                match sock.recv(&mut buf) {
                    Ok(n) => {
                        buf.truncate(n);
                        Ok(buf)
                    }
                    Err(e) => Err(e),
                }
            }
            _ => Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Invalid socket",
            )),
        }
    });

    match result {
        Ok(data) => {
            set_last_error(0);
            Ok(Value::String(PhpString::from_vec(data)))
        }
        Err(e) => {
            set_last_error(map_io_error(&e));
            vm.emit_warning(&format!(
                "socket_read(): unable to read from socket [{}]: {}",
                map_io_error(&e),
                errno_to_string(map_io_error(&e))
            ));
            Ok(Value::False)
        }
    }
}

/// socket_write(Socket $socket, string $data, ?int $length = null): int|false
fn socket_write(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let socket_id = match arg_socket_id(args.first()) {
        Some(s) => s,
        None => return Ok(Value::False),
    };
    let data = args
        .get(1)
        .map(|v| v.to_php_string())
        .unwrap_or_else(PhpString::empty);
    let max_len = args.get(2).and_then(|v| match v {
        Value::Null | Value::Undef => None,
        _ => Some(v.to_long() as usize),
    });

    let bytes = data.as_bytes();
    let write_bytes = match max_len {
        Some(len) if len < bytes.len() => &bytes[..len],
        _ => bytes,
    };

    let result = SOCKETS.with(|s| {
        let mut sockets = s.borrow_mut();
        match sockets.get_mut(&socket_id) {
            Some(SocketHandle::TcpClient(stream)) => stream.write(write_bytes),
            Some(SocketHandle::Udp(sock)) => sock.send(write_bytes),
            _ => Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Invalid socket",
            )),
        }
    });

    match result {
        Ok(n) => {
            set_last_error(0);
            Ok(Value::Long(n as i64))
        }
        Err(e) => {
            set_last_error(map_io_error(&e));
            vm.emit_warning(&format!(
                "socket_write(): unable to write to socket [{}]: {}",
                map_io_error(&e),
                errno_to_string(map_io_error(&e))
            ));
            Ok(Value::False)
        }
    }
}

/// socket_close(Socket $socket): void
fn socket_close(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if let Some(socket_id) = arg_socket_id(args.first()) {
        SOCKETS.with(|s| {
            // Replace with Closed to mark it closed but keep id valid
            s.borrow_mut().insert(socket_id, SocketHandle::Closed);
        });
    }
    Ok(Value::Null)
}

/// socket_send(Socket $socket, string $data, int $length, int $flags): int|false
fn socket_send(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let socket_id = match arg_socket_id(args.first()) {
        Some(s) => s,
        None => return Ok(Value::False),
    };
    let data = args
        .get(1)
        .map(|v| v.to_php_string())
        .unwrap_or_else(PhpString::empty);
    let length = args.get(2).map(|v| v.to_long() as usize).unwrap_or(0);
    let _flags = args.get(3).map(|v| v.to_long()).unwrap_or(0);

    let bytes = data.as_bytes();
    let send_bytes = if length > 0 && length < bytes.len() {
        &bytes[..length]
    } else {
        bytes
    };

    let result = SOCKETS.with(|s| {
        let mut sockets = s.borrow_mut();
        match sockets.get_mut(&socket_id) {
            Some(SocketHandle::TcpClient(stream)) => stream.write(send_bytes),
            Some(SocketHandle::Udp(sock)) => sock.send(send_bytes),
            _ => Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Invalid socket",
            )),
        }
    });

    match result {
        Ok(n) => {
            set_last_error(0);
            Ok(Value::Long(n as i64))
        }
        Err(e) => {
            set_last_error(map_io_error(&e));
            vm.emit_warning(&format!(
                "socket_send(): unable to send data [{}]: {}",
                map_io_error(&e),
                errno_to_string(map_io_error(&e))
            ));
            Ok(Value::False)
        }
    }
}

/// socket_recv(Socket $socket, string &$data, int $length, int $flags): int|false
fn socket_recv(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let socket_id = match arg_socket_id(args.first()) {
        Some(s) => s,
        None => return Ok(Value::False),
    };
    let data_ref = args.get(1);
    let length_l = args.get(2).map(|v| v.to_long()).unwrap_or(1024);
    if length_l < 0 || length_l > i32::MAX as i64 {
        return Ok(Value::False);
    }
    let length = length_l as usize;
    let _flags = args.get(3).map(|v| v.to_long()).unwrap_or(0);

    let result = SOCKETS.with(|s| {
        let mut sockets = s.borrow_mut();
        match sockets.get_mut(&socket_id) {
            Some(SocketHandle::TcpClient(stream)) => {
                let mut buf = vec![0u8; length];
                match stream.read(&mut buf) {
                    Ok(n) => {
                        buf.truncate(n);
                        Ok((buf, n))
                    }
                    Err(e) => Err(e),
                }
            }
            Some(SocketHandle::Udp(sock)) => {
                let mut buf = vec![0u8; length];
                match sock.recv(&mut buf) {
                    Ok(n) => {
                        buf.truncate(n);
                        Ok((buf, n))
                    }
                    Err(e) => Err(e),
                }
            }
            _ => Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Invalid socket",
            )),
        }
    });

    match result {
        Ok((data, n)) => {
            if let Some(r) = data_ref {
                let val = Value::String(PhpString::from_vec(data));
                if let Value::Reference(rc) = r {
                    *rc.borrow_mut() = val;
                }
            }
            set_last_error(0);
            Ok(Value::Long(n as i64))
        }
        Err(e) => {
            set_last_error(map_io_error(&e));
            vm.emit_warning(&format!(
                "socket_recv(): unable to receive data [{}]: {}",
                map_io_error(&e),
                errno_to_string(map_io_error(&e))
            ));
            Ok(Value::False)
        }
    }
}

/// socket_set_option(Socket $socket, int $level, int $optname, mixed $optval): bool
fn socket_set_option(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let socket_id = match arg_socket_id(args.first()) {
        Some(s) => s,
        None => return Ok(Value::False),
    };
    let level = args.get(1).map(|v| v.to_long()).unwrap_or(0);
    let optname = args.get(2).map(|v| v.to_long()).unwrap_or(0);
    let optval = args.get(3).cloned().unwrap_or(Value::Null);

    // Validation for SO_RCVTIMEO / SO_SNDTIMEO / SO_LINGER
    if level == SOL_SOCKET && (optname == SO_RCVTIMEO || optname == SO_SNDTIMEO) {
        match &optval {
            Value::Array(_) => {}
            Value::Object(_) => {
                // Object is treated like an empty array - missing sec key
                return Err(throw_value_error(
                    vm,
                    "socket_set_option(): Argument #4 ($value) must have key \"sec\"",
                ));
            }
            _ => {
                let type_name = match &optval {
                    Value::String(_) => "string",
                    Value::Long(_) => "int",
                    Value::Double(_) => "float",
                    Value::True | Value::False => "bool",
                    Value::Null => "null",
                    _ => "mixed",
                };
                let msg = if optname == SO_RCVTIMEO {
                    format!(
                        "socket_set_option(): Argument #4 ($value) must be of type array when argument #3 ($option) is SO_RCVTIMEO, {} given",
                        type_name
                    )
                } else {
                    format!(
                        "socket_set_option(): Argument #4 ($value) must be of type array when argument #3 ($option) is SO_SNDTIMEO, {} given",
                        type_name
                    )
                };
                return Err(throw_type_error(vm, &msg));
            }
        }
        // Validate "sec" key exists
        if let Value::Array(arr) = &optval {
            let arr_ref = arr.borrow();
            let sec_val = arr_ref
                .get(&ArrayKey::String(PhpString::from_bytes(b"sec")))
                .cloned();
            if sec_val.is_none() {
                return Err(throw_value_error(
                    vm,
                    "socket_set_option(): Argument #4 ($value) must have key \"sec\"",
                ));
            }
            // Emit warning if sec is object
            if let Some(Value::Object(o)) = &sec_val {
                let class_name = String::from_utf8_lossy(&o.borrow().class_name).into_owned();
                drop(arr_ref);
                vm.emit_warning(&format!(
                    "Object of class {} could not be converted to int",
                    class_name
                ));
            }
        }
    } else if level == SOL_SOCKET && optname == SO_LINGER {
        match &optval {
            Value::Array(_) => {}
            _ => {
                let type_name = match &optval {
                    Value::String(_) => "string",
                    Value::Long(_) => "int",
                    Value::Double(_) => "float",
                    Value::True | Value::False => "bool",
                    Value::Null => "null",
                    Value::Object(_) => "stdClass",
                    _ => "mixed",
                };
                let msg = format!(
                    "socket_set_option(): Argument #4 ($value) must be of type array when argument #3 ($option) is SO_LINGER, {} given",
                    type_name
                );
                return Err(throw_type_error(vm, &msg));
            }
        }
        if let Value::Array(arr) = &optval {
            let arr_ref = arr.borrow();
            let has_l_onoff = arr_ref
                .get(&ArrayKey::String(PhpString::from_bytes(b"l_onoff")))
                .is_some();
            if !has_l_onoff {
                return Err(throw_value_error(
                    vm,
                    "socket_set_option(): Argument #4 ($value) must have key \"l_onoff\"",
                ));
            }
            let has_l_linger = arr_ref
                .get(&ArrayKey::String(PhpString::from_bytes(b"l_linger")))
                .is_some();
            if !has_l_linger {
                return Err(throw_value_error(
                    vm,
                    "socket_set_option(): Argument #4 ($value) must have key \"l_linger\"",
                ));
            }
            // Check l_onoff is in range [0, USHRT_MAX]
            let l_onoff = arr_ref
                .get(&ArrayKey::String(PhpString::from_bytes(b"l_onoff")))
                .map(|v| v.to_long())
                .unwrap_or(0);
            if !(0..=65535).contains(&l_onoff) {
                return Err(throw_value_error(
                    vm,
                    "socket_set_option(): Argument #4 ($value) \"l_onoff\" must be between 0 and 65535",
                ));
            }
        }
    }

    let result = SOCKETS.with(|s| {
        let sockets = s.borrow();
        match sockets.get(&socket_id) {
            Some(handle) => set_socket_option(handle, level, optname, &optval),
            None => Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Invalid socket",
            )),
        }
    });

    match result {
        Ok(()) => {
            set_last_error(0);
            Ok(Value::True)
        }
        Err(e) => {
            set_last_error(map_io_error(&e));
            vm.emit_warning(&format!(
                "socket_set_option(): Unable to set socket option [{}]: {}",
                map_io_error(&e),
                errno_to_string(map_io_error(&e))
            ));
            Ok(Value::False)
        }
    }
}

#[cfg(unix)]
fn set_socket_option(
    handle: &SocketHandle,
    level: i64,
    optname: i64,
    optval: &Value,
) -> std::io::Result<()> {
    use std::os::unix::io::AsRawFd;

    let fd = match handle {
        SocketHandle::TcpClient(s) => s.as_raw_fd(),
        SocketHandle::TcpServer(s) => s.as_raw_fd(),
        SocketHandle::Udp(s) => s.as_raw_fd(),
        SocketHandle::Closed => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "closed",
            ));
        }
    };

    if level == SOL_SOCKET && (optname == SO_RCVTIMEO || optname == SO_SNDTIMEO) {
        let (sec, usec) = match optval {
            Value::Array(arr) => {
                let arr = arr.borrow();
                let sec = arr
                    .get(&ArrayKey::String(PhpString::from_bytes(b"sec")))
                    .map(|v| v.to_long())
                    .unwrap_or(0);
                let usec = arr
                    .get(&ArrayKey::String(PhpString::from_bytes(b"usec")))
                    .map(|v| v.to_long())
                    .unwrap_or(0);
                (sec, usec)
            }
            _ => (optval.to_long(), 0),
        };

        let timeval = libc::timeval {
            tv_sec: sec as libc::time_t,
            tv_usec: usec as libc::suseconds_t,
        };

        let ret = unsafe {
            libc::setsockopt(
                fd,
                level as i32,
                optname as i32,
                &timeval as *const libc::timeval as *const libc::c_void,
                std::mem::size_of::<libc::timeval>() as libc::socklen_t,
            )
        };

        if ret == 0 {
            Ok(())
        } else {
            Err(std::io::Error::last_os_error())
        }
    } else if level == SOL_SOCKET && optname == SO_LINGER {
        let (onoff, linger) = match optval {
            Value::Array(arr) => {
                let arr = arr.borrow();
                let o = arr
                    .get(&ArrayKey::String(PhpString::from_bytes(b"l_onoff")))
                    .map(|v| v.to_long())
                    .unwrap_or(0);
                let l = arr
                    .get(&ArrayKey::String(PhpString::from_bytes(b"l_linger")))
                    .map(|v| v.to_long())
                    .unwrap_or(0);
                (o, l)
            }
            _ => (0, 0),
        };

        let linger_val = libc::linger {
            l_onoff: onoff as i32,
            l_linger: linger as i32,
        };

        let ret = unsafe {
            libc::setsockopt(
                fd,
                level as i32,
                optname as i32,
                &linger_val as *const libc::linger as *const libc::c_void,
                std::mem::size_of::<libc::linger>() as libc::socklen_t,
            )
        };

        if ret == 0 {
            Ok(())
        } else {
            Err(std::io::Error::last_os_error())
        }
    } else {
        let int_val = optval.to_long() as i32;

        let ret = unsafe {
            libc::setsockopt(
                fd,
                level as i32,
                optname as i32,
                &int_val as *const i32 as *const libc::c_void,
                std::mem::size_of::<i32>() as libc::socklen_t,
            )
        };

        if ret == 0 {
            Ok(())
        } else {
            Err(std::io::Error::last_os_error())
        }
    }
}

#[cfg(not(unix))]
fn set_socket_option(
    _handle: &SocketHandle,
    _level: i64,
    _optname: i64,
    _optval: &Value,
) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "socket_set_option not supported on this platform",
    ))
}

/// socket_get_option(Socket $socket, int $level, int $optname): array|int|false
fn socket_get_option(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let socket_id = match arg_socket_id(args.first()) {
        Some(s) => s,
        None => return Ok(Value::False),
    };
    let level = args.get(1).map(|v| v.to_long()).unwrap_or(0);
    let optname = args.get(2).map(|v| v.to_long()).unwrap_or(0);

    let result = SOCKETS.with(|s| {
        let sockets = s.borrow();
        match sockets.get(&socket_id) {
            Some(handle) => get_socket_option(handle, level, optname),
            None => Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Invalid socket",
            )),
        }
    });

    match result {
        Ok(val) => {
            set_last_error(0);
            Ok(val)
        }
        Err(e) => {
            set_last_error(map_io_error(&e));
            vm.emit_warning(&format!(
                "socket_get_option(): Unable to get socket option [{}]: {}",
                map_io_error(&e),
                errno_to_string(map_io_error(&e))
            ));
            Ok(Value::False)
        }
    }
}

#[cfg(unix)]
fn get_socket_option(handle: &SocketHandle, level: i64, optname: i64) -> std::io::Result<Value> {
    use std::os::unix::io::AsRawFd;

    let fd = match handle {
        SocketHandle::TcpClient(s) => s.as_raw_fd(),
        SocketHandle::TcpServer(s) => s.as_raw_fd(),
        SocketHandle::Udp(s) => s.as_raw_fd(),
        SocketHandle::Closed => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "closed",
            ));
        }
    };

    if level == SOL_SOCKET && (optname == SO_RCVTIMEO || optname == SO_SNDTIMEO) {
        let mut timeval = libc::timeval {
            tv_sec: 0,
            tv_usec: 0,
        };
        let mut len = std::mem::size_of::<libc::timeval>() as libc::socklen_t;

        let ret = unsafe {
            libc::getsockopt(
                fd,
                level as i32,
                optname as i32,
                &mut timeval as *mut libc::timeval as *mut libc::c_void,
                &mut len,
            )
        };

        if ret == 0 {
            let mut arr = PhpArray::new();
            arr.set(
                ArrayKey::String(PhpString::from_bytes(b"sec")),
                Value::Long(timeval.tv_sec),
            );
            arr.set(
                ArrayKey::String(PhpString::from_bytes(b"usec")),
                Value::Long(timeval.tv_usec),
            );
            Ok(Value::Array(Rc::new(RefCell::new(arr))))
        } else {
            Err(std::io::Error::last_os_error())
        }
    } else if level == SOL_SOCKET && optname == SO_LINGER {
        let mut linger_val = libc::linger {
            l_onoff: 0,
            l_linger: 0,
        };
        let mut len = std::mem::size_of::<libc::linger>() as libc::socklen_t;

        let ret = unsafe {
            libc::getsockopt(
                fd,
                level as i32,
                optname as i32,
                &mut linger_val as *mut libc::linger as *mut libc::c_void,
                &mut len,
            )
        };

        if ret == 0 {
            let mut arr = PhpArray::new();
            arr.set(
                ArrayKey::String(PhpString::from_bytes(b"l_onoff")),
                Value::Long(linger_val.l_onoff as i64),
            );
            arr.set(
                ArrayKey::String(PhpString::from_bytes(b"l_linger")),
                Value::Long(linger_val.l_linger as i64),
            );
            Ok(Value::Array(Rc::new(RefCell::new(arr))))
        } else {
            Err(std::io::Error::last_os_error())
        }
    } else {
        let mut int_val: i32 = 0;
        let mut len = std::mem::size_of::<i32>() as libc::socklen_t;

        let ret = unsafe {
            libc::getsockopt(
                fd,
                level as i32,
                optname as i32,
                &mut int_val as *mut i32 as *mut libc::c_void,
                &mut len,
            )
        };

        if ret == 0 {
            Ok(Value::Long(int_val as i64))
        } else {
            Err(std::io::Error::last_os_error())
        }
    }
}

#[cfg(not(unix))]
fn get_socket_option(_handle: &SocketHandle, _level: i64, _optname: i64) -> std::io::Result<Value> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "socket_get_option not supported on this platform",
    ))
}

/// socket_last_error(?Socket $socket = null): int
fn socket_last_error(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Long(get_last_error()))
}

/// socket_clear_error(?Socket $socket = null): void
fn socket_clear_error(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    set_last_error(0);
    Ok(Value::Null)
}

/// socket_strerror(int $error_code): string
fn socket_strerror(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let errno = args.first().map(|v| v.to_long()).unwrap_or(0);
    // Validate range: must fit in i32
    if !((i32::MIN as i64)..=(i32::MAX as i64)).contains(&errno) {
        return Err(throw_value_error(
            vm,
            &format!(
                "socket_strerror(): Argument #1 ($error_code) must be between {} and {}",
                i32::MIN,
                i32::MAX
            ),
        ));
    }
    let msg = errno_to_string(errno);
    Ok(Value::String(PhpString::from_bytes(msg.as_bytes())))
}

/// socket_set_nonblock(Socket $socket): bool
fn socket_set_nonblock(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let socket_id = match arg_socket_id(args.first()) {
        Some(s) => s,
        None => return Ok(Value::False),
    };

    // Check if closed
    let is_closed = SOCKETS.with(|s| {
        matches!(s.borrow().get(&socket_id), Some(SocketHandle::Closed))
            || !s.borrow().contains_key(&socket_id)
    });
    if is_closed {
        let exc = vm.create_exception(
            b"Error",
            "socket_set_nonblock(): Argument #1 ($socket) has already been closed",
            vm.current_line,
        );
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: "socket already closed".to_string(),
            line: vm.current_line,
        });
    }

    let result = SOCKETS.with(|s| {
        let sockets = s.borrow();
        match sockets.get(&socket_id) {
            Some(SocketHandle::TcpClient(stream)) => stream.set_nonblocking(true),
            Some(SocketHandle::TcpServer(listener)) => listener.set_nonblocking(true),
            Some(SocketHandle::Udp(sock)) => sock.set_nonblocking(true),
            _ => Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Invalid socket",
            )),
        }
    });

    match result {
        Ok(()) => {
            set_last_error(0);
            Ok(Value::True)
        }
        Err(e) => {
            set_last_error(map_io_error(&e));
            vm.emit_warning(&format!(
                "socket_set_nonblock(): unable to set non-blocking mode [{}]: {}",
                map_io_error(&e),
                errno_to_string(map_io_error(&e))
            ));
            Ok(Value::False)
        }
    }
}

/// socket_set_block(Socket $socket): bool
fn socket_set_block(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let socket_id = match arg_socket_id(args.first()) {
        Some(s) => s,
        None => return Ok(Value::False),
    };

    let is_closed = SOCKETS.with(|s| {
        matches!(s.borrow().get(&socket_id), Some(SocketHandle::Closed))
            || !s.borrow().contains_key(&socket_id)
    });
    if is_closed {
        let exc = vm.create_exception(
            b"Error",
            "socket_set_block(): Argument #1 ($socket) has already been closed",
            vm.current_line,
        );
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: "socket already closed".to_string(),
            line: vm.current_line,
        });
    }

    let result = SOCKETS.with(|s| {
        let sockets = s.borrow();
        match sockets.get(&socket_id) {
            Some(SocketHandle::TcpClient(stream)) => stream.set_nonblocking(false),
            Some(SocketHandle::TcpServer(listener)) => listener.set_nonblocking(false),
            Some(SocketHandle::Udp(sock)) => sock.set_nonblocking(false),
            _ => Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Invalid socket",
            )),
        }
    });

    match result {
        Ok(()) => {
            set_last_error(0);
            Ok(Value::True)
        }
        Err(e) => {
            set_last_error(map_io_error(&e));
            vm.emit_warning(&format!(
                "socket_set_block(): unable to set blocking mode [{}]: {}",
                map_io_error(&e),
                errno_to_string(map_io_error(&e))
            ));
            Ok(Value::False)
        }
    }
}

/// socket_getpeername(Socket $socket, string &$address, int &$port = null): bool
fn socket_getpeername(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let socket_id = match arg_socket_id(args.first()) {
        Some(s) => s,
        None => return Ok(Value::False),
    };
    let addr_ref = args.get(1);
    let port_ref = args.get(2);

    let result = SOCKETS.with(|s| {
        let sockets = s.borrow();
        match sockets.get(&socket_id) {
            Some(SocketHandle::TcpClient(stream)) => stream.peer_addr(),
            _ => Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Invalid socket or not connected",
            )),
        }
    });

    match result {
        Ok(addr) => {
            if let Some(r) = addr_ref {
                let ip_str = addr.ip().to_string();
                let val = Value::String(PhpString::from_bytes(ip_str.as_bytes()));
                if let Value::Reference(rc) = r {
                    *rc.borrow_mut() = val;
                }
            }
            if let Some(r) = port_ref {
                let val = Value::Long(addr.port() as i64);
                if let Value::Reference(rc) = r {
                    *rc.borrow_mut() = val;
                }
            }
            set_last_error(0);
            Ok(Value::True)
        }
        Err(e) => {
            set_last_error(map_io_error(&e));
            vm.emit_warning(&format!(
                "socket_getpeername(): unable to retrieve peer name [{}]: {}",
                map_io_error(&e),
                errno_to_string(map_io_error(&e))
            ));
            Ok(Value::False)
        }
    }
}

/// socket_getsockname(Socket $socket, string &$address, int &$port = null): bool
fn socket_getsockname(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let socket_id = match arg_socket_id(args.first()) {
        Some(s) => s,
        None => return Ok(Value::False),
    };
    let addr_ref = args.get(1);
    let port_ref = args.get(2);

    let result = SOCKETS.with(|s| {
        let sockets = s.borrow();
        match sockets.get(&socket_id) {
            Some(SocketHandle::TcpClient(stream)) => stream.local_addr(),
            Some(SocketHandle::TcpServer(listener)) => listener.local_addr(),
            Some(SocketHandle::Udp(sock)) => sock.local_addr(),
            _ => Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Invalid socket",
            )),
        }
    });

    match result {
        Ok(addr) => {
            if let Some(r) = addr_ref {
                let ip_str = addr.ip().to_string();
                let val = Value::String(PhpString::from_bytes(ip_str.as_bytes()));
                if let Value::Reference(rc) = r {
                    *rc.borrow_mut() = val;
                }
            }
            if let Some(r) = port_ref {
                let val = Value::Long(addr.port() as i64);
                if let Value::Reference(rc) = r {
                    *rc.borrow_mut() = val;
                }
            }
            set_last_error(0);
            Ok(Value::True)
        }
        Err(e) => {
            set_last_error(map_io_error(&e));
            vm.emit_warning(&format!(
                "socket_getsockname(): unable to retrieve socket name [{}]: {}",
                map_io_error(&e),
                errno_to_string(map_io_error(&e))
            ));
            Ok(Value::False)
        }
    }
}

/// socket_shutdown(Socket $socket, int $mode = 2): bool
fn socket_shutdown(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let socket_id = match arg_socket_id(args.first()) {
        Some(s) => s,
        None => return Ok(Value::False),
    };
    let how = args.get(1).map(|v| v.to_long()).unwrap_or(2);

    if !(0..=2).contains(&how) {
        return Err(throw_value_error(
            vm,
            "must be one of SHUT_RD, SHUT_WR or SHUT_RDWR",
        ));
    }

    let shutdown_type = match how {
        0 => Shutdown::Read,
        1 => Shutdown::Write,
        _ => Shutdown::Both,
    };

    let result = SOCKETS.with(|s| {
        let sockets = s.borrow();
        match sockets.get(&socket_id) {
            Some(SocketHandle::TcpClient(stream)) => stream.shutdown(shutdown_type),
            _ => Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Invalid socket or not a TCP client",
            )),
        }
    });

    match result {
        Ok(()) => {
            set_last_error(0);
            Ok(Value::True)
        }
        Err(e) => {
            set_last_error(map_io_error(&e));
            vm.emit_warning(&format!(
                "socket_shutdown(): Unable to shutdown socket [{}]: {}",
                map_io_error(&e),
                errno_to_string(map_io_error(&e))
            ));
            Ok(Value::False)
        }
    }
}

/// socket_select(array &$read, array &$write, array &$except, ?int $seconds, int $microseconds = 0): int|false
fn socket_select(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let read_arg = args.first();
    let write_arg = args.get(1);
    let except_arg = args.get(2);

    // Validate at least one array
    let has_array = |arg: Option<&Value>| -> bool {
        if let Some(v) = arg {
            let v = v.deref();
            matches!(v, Value::Array(_))
        } else {
            false
        }
    };
    if !has_array(read_arg) && !has_array(write_arg) && !has_array(except_arg) {
        return Err(throw_value_error(
            vm,
            "socket_select(): At least one array argument must be passed",
        ));
    }

    let seconds_arg = args.get(3);
    let seconds = match seconds_arg {
        None => None,
        Some(v) => {
            let dv = v.deref();
            if matches!(dv, Value::Null) {
                None
            } else {
                Some(dv.to_long())
            }
        }
    };
    let microseconds = args.get(4).map(|v| v.to_long()).unwrap_or(0);

    #[cfg(unix)]
    {
        use std::os::unix::io::AsRawFd;

        let mut read_fds: Vec<(i64, i32)> = Vec::new();
        let mut write_fds: Vec<(i64, i32)> = Vec::new();
        let mut except_fds: Vec<(i64, i32)> = Vec::new();

        // Type-check the read array
        let collect_fds = |arg: Option<&Value>,
                           label: &str,
                           fds: &mut Vec<(i64, i32)>,
                           vm: &mut Vm|
         -> Result<(), VmError> {
            if let Some(arg_val) = arg {
                let val = arg_val.deref();
                if let Value::Array(arr) = &val {
                    let arr_ref = arr.borrow();
                    for (_key, val) in arr_ref.iter() {
                        let val = val.deref();
                        let sid_opt = match &val {
                            Value::Object(obj) => {
                                let b = obj.borrow();
                                if b.class_name == b"Socket" {
                                    Some(b.object_id as i64)
                                } else {
                                    None
                                }
                            }
                            Value::Long(n) => Some(*n),
                            _ => None,
                        };
                        if sid_opt.is_none() {
                            let type_name = match &val {
                                Value::String(_) => "string",
                                Value::Long(_) => "int",
                                Value::Double(_) => "float",
                                Value::True | Value::False => "bool",
                                Value::Null => "null",
                                Value::Array(_) => "array",
                                Value::Object(o) => {
                                    let b = o.borrow();
                                    return Err(throw_type_error(
                                        vm,
                                        &format!(
                                            "socket_select(): Argument #{} ({}) must only have elements of type Socket, {} given",
                                            match label {
                                                "read" => 1,
                                                "write" => 2,
                                                _ => 3,
                                            },
                                            label,
                                            String::from_utf8_lossy(&b.class_name)
                                        ),
                                    ));
                                }
                                _ => "mixed",
                            };
                            return Err(throw_type_error(
                                vm,
                                &format!(
                                    "socket_select(): Argument #{} (${}) must only have elements of type Socket, {} given",
                                    match label {
                                        "read" => 1,
                                        "write" => 2,
                                        _ => 3,
                                    },
                                    label,
                                    type_name
                                ),
                            ));
                        }
                        let sid = sid_opt.unwrap();
                        let exists = SOCKETS.with(|s| {
                            let sockets = s.borrow();
                            if let Some(handle) = sockets.get(&sid) {
                                match handle {
                                    SocketHandle::TcpClient(s) => Some(s.as_raw_fd()),
                                    SocketHandle::TcpServer(s) => Some(s.as_raw_fd()),
                                    SocketHandle::Udp(s) => Some(s.as_raw_fd()),
                                    SocketHandle::Closed => None,
                                }
                            } else {
                                None
                            }
                        });
                        if let Some(fd) = exists {
                            fds.push((sid, fd));
                        }
                    }
                }
            }
            Ok(())
        };

        collect_fds(read_arg, "read", &mut read_fds, vm)?;
        collect_fds(write_arg, "write", &mut write_fds, vm)?;
        collect_fds(except_arg, "except", &mut except_fds, vm)?;

        // Validate timeout
        if let Some(s) = seconds {
            if s < 0 {
                let err = std::io::Error::new(std::io::ErrorKind::InvalidInput, "Invalid argument");
                set_last_error(map_io_error(&err));
                vm.emit_warning(&format!(
                    "socket_select(): Unable to select [{}]: Invalid argument",
                    map_io_error(&err)
                ));
                return Ok(Value::False);
            }
        }

        let mut max_fd: i32 = 0;
        unsafe {
            let mut read_set: libc::fd_set = std::mem::zeroed();
            let mut write_set: libc::fd_set = std::mem::zeroed();
            let mut except_set: libc::fd_set = std::mem::zeroed();
            libc::FD_ZERO(&mut read_set);
            libc::FD_ZERO(&mut write_set);
            libc::FD_ZERO(&mut except_set);

            for &(_, fd) in &read_fds {
                if fd >= 0 && fd < libc::FD_SETSIZE as i32 {
                    libc::FD_SET(fd, &mut read_set);
                    if fd > max_fd {
                        max_fd = fd;
                    }
                }
            }
            for &(_, fd) in &write_fds {
                if fd >= 0 && fd < libc::FD_SETSIZE as i32 {
                    libc::FD_SET(fd, &mut write_set);
                    if fd > max_fd {
                        max_fd = fd;
                    }
                }
            }
            for &(_, fd) in &except_fds {
                if fd >= 0 && fd < libc::FD_SETSIZE as i32 {
                    libc::FD_SET(fd, &mut except_set);
                    if fd > max_fd {
                        max_fd = fd;
                    }
                }
            }

            let mut tv;
            let timeout_ptr = if let Some(secs) = seconds {
                tv = libc::timeval {
                    tv_sec: secs as libc::time_t,
                    tv_usec: microseconds as libc::suseconds_t,
                };
                &mut tv as *mut libc::timeval
            } else {
                std::ptr::null_mut()
            };

            let ret = libc::select(
                max_fd + 1,
                if read_fds.is_empty() {
                    std::ptr::null_mut()
                } else {
                    &mut read_set
                },
                if write_fds.is_empty() {
                    std::ptr::null_mut()
                } else {
                    &mut write_set
                },
                if except_fds.is_empty() {
                    std::ptr::null_mut()
                } else {
                    &mut except_set
                },
                timeout_ptr,
            );

            if ret < 0 {
                let err = std::io::Error::last_os_error();
                set_last_error(map_io_error(&err));
                vm.emit_warning(&format!(
                    "socket_select(): Unable to select [{}]: {}",
                    map_io_error(&err),
                    err
                ));
                return Ok(Value::False);
            }

            // Update arrays
            let write_into = |arg: Option<&Value>,
                              fds: &[(i64, i32)],
                              set: &libc::fd_set,
                              _vm: &mut Vm| {
                if let Some(val) = arg {
                    if let Value::Reference(rc) = val {
                        let mut arr = PhpArray::new();
                        for &(sid, fd) in fds {
                            if fd >= 0 && fd < libc::FD_SETSIZE as i32 && libc::FD_ISSET(fd, set) {
                                arr.push(make_socket_object(sid));
                            }
                        }
                        *rc.borrow_mut() = Value::Array(Rc::new(RefCell::new(arr)));
                    }
                }
            };

            write_into(read_arg, &read_fds, &read_set, vm);
            write_into(write_arg, &write_fds, &write_set, vm);
            write_into(except_arg, &except_fds, &except_set, vm);

            set_last_error(0);
            Ok(Value::Long(ret as i64))
        }
    }

    #[cfg(not(unix))]
    {
        let _ = (seconds, microseconds);
        vm.emit_warning("socket_select(): Not supported on this platform");
        Ok(Value::False)
    }
}

/// Map an I/O error to a Linux errno value
fn map_io_error(err: &std::io::Error) -> i64 {
    if let Some(code) = err.raw_os_error() {
        return code as i64;
    }
    match err.kind() {
        std::io::ErrorKind::ConnectionRefused => 111,
        std::io::ErrorKind::ConnectionReset => 104,
        std::io::ErrorKind::ConnectionAborted => 103,
        std::io::ErrorKind::NotConnected => 107,
        std::io::ErrorKind::AddrInUse => 98,
        std::io::ErrorKind::AddrNotAvailable => 99,
        std::io::ErrorKind::BrokenPipe => 32,
        std::io::ErrorKind::AlreadyExists => 98,
        std::io::ErrorKind::WouldBlock => 11,
        std::io::ErrorKind::TimedOut => 110,
        std::io::ErrorKind::Interrupted => 4,
        std::io::ErrorKind::PermissionDenied => 13,
        std::io::ErrorKind::InvalidInput => 22,
        _ => 5,
    }
}

fn errno_to_string(errno: i64) -> String {
    match errno {
        0 => "Success".to_string(),
        1 => "Operation not permitted".to_string(),
        2 => "No such file or directory".to_string(),
        4 => "Interrupted system call".to_string(),
        5 => "Input/output error".to_string(),
        9 => "Bad file descriptor".to_string(),
        11 => "Resource temporarily unavailable".to_string(),
        13 => "Permission denied".to_string(),
        22 => "Invalid argument".to_string(),
        32 => "Broken pipe".to_string(),
        88 => "Socket operation on non-socket".to_string(),
        93 => "Protocol not supported".to_string(),
        94 => "Socket type not supported".to_string(),
        95 => "Operation not supported".to_string(),
        97 => "Address family not supported by protocol".to_string(),
        98 => "Address already in use".to_string(),
        99 => "Cannot assign requested address".to_string(),
        100 => "Network is down".to_string(),
        101 => "Network is unreachable".to_string(),
        103 => "Software caused connection abort".to_string(),
        104 => "Connection reset by peer".to_string(),
        106 => "Transport endpoint is already connected".to_string(),
        107 => "Transport endpoint is not connected".to_string(),
        110 => "Connection timed out".to_string(),
        111 => "Connection refused".to_string(),
        _ => format!("Unknown error {}", errno),
    }
}

/// socket_create_listen(int $port, int $backlog = 128): Socket|false
fn socket_create_listen_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let port = args.first().map(|v| v.to_long()).unwrap_or(0);
    let _backlog = args.get(1).map(|v| v.to_long()).unwrap_or(128);

    if !(0..=65535).contains(&port) {
        return Err(throw_value_error(
            vm,
            "socket_create_listen(): Argument #1 ($port) must be between 0 and 65535",
        ));
    }

    let addr_str = format!("0.0.0.0:{}", port);
    match TcpListener::bind(&addr_str) {
        Ok(listener) => {
            let id = alloc_socket_id_via_vm(vm);
            SOCKETS.with(|s| {
                s.borrow_mut().insert(id, SocketHandle::TcpServer(listener));
            });
            set_last_error(0);
            Ok(make_socket_object(id))
        }
        Err(e) => {
            set_last_error(map_io_error(&e));
            vm.emit_warning(&format!(
                "socket_create_listen(): unable to bind to given address [{}]: {}",
                map_io_error(&e),
                errno_to_string(map_io_error(&e))
            ));
            Ok(Value::False)
        }
    }
}

/// socket_create_pair(int $domain, int $type, int $protocol, array &$pair): bool
fn socket_create_pair_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let domain = args.first().map(|v| v.to_long()).unwrap_or(0);
    let sock_type = args.get(1).map(|v| v.to_long()).unwrap_or(0);
    let protocol = args.get(2).map(|v| v.to_long()).unwrap_or(0);
    let pair_arg = args.get(3);

    if domain != AF_UNIX && domain != AF_INET && domain != AF_INET6 {
        return Err(throw_value_error(
            vm,
            "socket_create_pair(): Argument #1 ($domain) must be one of AF_UNIX, AF_INET6, or AF_INET",
        ));
    }

    // PHP only validates type against a fixed list when it's clearly out of range.
    // The value 0 is accepted (socketpair() will fail with EPROTOTYPE at runtime).
    // Reject only large invalid values (e.g. 31337).
    if !(-1..=10).contains(&sock_type) {
        return Err(throw_value_error(
            vm,
            "socket_create_pair(): Argument #2 ($type) must be one of SOCK_STREAM, SOCK_DGRAM, SOCK_SEQPACKET, SOCK_RAW, or SOCK_RDM",
        ));
    }

    let mut fds: [i32; 2] = [0, 0];
    let result = unsafe {
        libc::socketpair(
            domain as i32,
            sock_type as i32,
            protocol as i32,
            fds.as_mut_ptr(),
        )
    };
    if result != 0 {
        let e = std::io::Error::last_os_error();
        set_last_error(map_io_error(&e));
        vm.emit_warning(&format!(
            "socket_create_pair(): Unable to create socket pair [{}]: {}",
            map_io_error(&e),
            errno_to_string(map_io_error(&e))
        ));
        return Ok(Value::False);
    }

    // Wrap fds as Sockets
    let id1 = alloc_socket_id_via_vm(vm);
    let id2 = alloc_socket_id_via_vm(vm);

    let h1 = match sock_type {
        SOCK_STREAM => unsafe { SocketHandle::TcpClient(TcpStream::from_raw_fd(fds[0])) },
        SOCK_DGRAM => unsafe { SocketHandle::Udp(UdpSocket::from_raw_fd(fds[0])) },
        _ => unsafe { SocketHandle::TcpClient(TcpStream::from_raw_fd(fds[0])) },
    };
    let h2 = match sock_type {
        SOCK_STREAM => unsafe { SocketHandle::TcpClient(TcpStream::from_raw_fd(fds[1])) },
        SOCK_DGRAM => unsafe { SocketHandle::Udp(UdpSocket::from_raw_fd(fds[1])) },
        _ => unsafe { SocketHandle::TcpClient(TcpStream::from_raw_fd(fds[1])) },
    };

    SOCKETS.with(|s| {
        let mut ss = s.borrow_mut();
        ss.insert(id1, h1);
        ss.insert(id2, h2);
    });

    let obj1 = make_socket_object(id1);
    let obj2 = make_socket_object(id2);

    // Write into pair array reference
    if let Some(Value::Reference(rc)) = pair_arg {
        let mut arr = PhpArray::new();
        arr.set(ArrayKey::Int(0), obj1);
        arr.set(ArrayKey::Int(1), obj2);
        *rc.borrow_mut() = Value::Array(Rc::new(RefCell::new(arr)));
    }

    set_last_error(0);
    Ok(Value::True)
}

fn socket_import_stream_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::False)
}

fn socket_export_stream_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::False)
}

/// socket_addrinfo_lookup(string $host, ?string $service = null, array $hints = []): array|false
fn socket_addrinfo_lookup_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let host = args
        .first()
        .map(|v| v.to_php_string().to_string_lossy())
        .unwrap_or_default();
    let service = args
        .get(1)
        .map(|v| {
            let v = v.deref();
            if matches!(v, Value::Null | Value::Undef) {
                None
            } else {
                Some(v.to_php_string().to_string_lossy())
            }
        })
        .unwrap_or(None);

    let mut family: i32 = 0;
    let mut socktype: i32 = 0;
    let mut protocol: i32 = 0;
    let mut flags: i32 = 0;

    if let Some(hints_val) = args.get(2) {
        let hints_val = hints_val.deref();
        if let Value::Array(arr) = &hints_val {
            let arr_ref = arr.borrow();
            // Validate keys
            for (key, val) in arr_ref.iter() {
                let k = match key {
                    ArrayKey::String(s) => s.to_string_lossy(),
                    ArrayKey::Int(_) => {
                        return Err(throw_value_error(
                            vm,
                            "socket_addrinfo_lookup(): Argument #3 ($hints) must only contain array keys \"ai_flags\", \"ai_socktype\", \"ai_protocol\", or \"ai_family\"",
                        ));
                    }
                };
                if k != "ai_flags" && k != "ai_socktype" && k != "ai_protocol" && k != "ai_family" {
                    return Err(throw_value_error(
                        vm,
                        "socket_addrinfo_lookup(): Argument #3 ($hints) must only contain array keys \"ai_flags\", \"ai_socktype\", \"ai_protocol\", or \"ai_family\"",
                    ));
                }
                // Type check
                if let Value::Object(obj) = val {
                    let cn = String::from_utf8_lossy(&obj.borrow().class_name).into_owned();
                    return Err(throw_type_error(
                        vm,
                        &format!(
                            "socket_addrinfo_lookup(): Argument #3 ($hints) \"{}\" key must be of type int, {} given",
                            k, cn
                        ),
                    ));
                }
                let n = val.to_long();
                match k.as_str() {
                    "ai_family" => {
                        if n != AF_INET && n != AF_INET6 && n != 0 {
                            return Err(throw_value_error(
                                vm,
                                "socket_addrinfo_lookup(): Argument #3 ($hints) \"ai_family\" key must be AF_INET or AF_INET6",
                            ));
                        }
                        family = n as i32;
                    }
                    "ai_socktype" => {
                        if n < 0 || n > i32::MAX as i64 {
                            return Err(throw_value_error(
                                vm,
                                &format!(
                                    "socket_addrinfo_lookup(): Argument #3 ($hints) \"ai_socktype\" key must be between 0 and {}",
                                    i32::MAX
                                ),
                            ));
                        }
                        socktype = n as i32;
                    }
                    "ai_protocol" => {
                        if n < 0 || n > i32::MAX as i64 {
                            return Err(throw_value_error(
                                vm,
                                &format!(
                                    "socket_addrinfo_lookup(): Argument #3 ($hints) \"ai_protocol\" key must be between 0 and {}",
                                    i32::MAX
                                ),
                            ));
                        }
                        protocol = n as i32;
                    }
                    "ai_flags" => {
                        if n < 0 || n > i32::MAX as i64 {
                            return Err(throw_value_error(
                                vm,
                                &format!(
                                    "socket_addrinfo_lookup(): Argument #3 ($hints) \"ai_flags\" key must be between 0 and {}",
                                    i32::MAX
                                ),
                            ));
                        }
                        flags = n as i32;
                    }
                    _ => {}
                }
            }
        }
    }

    // Default service to 0 if not given
    let port: u16 = service
        .as_deref()
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(0);

    // Resolve via std::net
    let addr_iter = match std::net::ToSocketAddrs::to_socket_addrs(&(host.as_str(), port)) {
        Ok(it) => it,
        Err(_) => return Ok(Value::False),
    };

    let mut arr = PhpArray::new();
    let mut idx = 0i64;
    for addr in addr_iter {
        let af = match addr {
            SocketAddr::V4(_) => AF_INET as i32,
            SocketAddr::V6(_) => AF_INET6 as i32,
        };
        if family != 0 && family != af {
            continue;
        }
        let entry = AddrInfoEntry {
            family: af,
            socktype,
            protocol,
            flags,
            addr,
        };
        let aid = alloc_socket_id_via_vm(vm);
        ADDRINFOS.with(|m| {
            m.borrow_mut().insert(aid, entry);
        });
        arr.set(ArrayKey::Int(idx), make_addrinfo_object(aid));
        idx += 1;
    }

    Ok(Value::Array(Rc::new(RefCell::new(arr))))
}

/// socket_addrinfo_connect(AddressInfo $address): Socket|false
fn socket_addrinfo_connect_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let aid = match arg_addrinfo_id(args.first()) {
        Some(a) => a,
        None => return Ok(Value::False),
    };
    let entry = ADDRINFOS.with(|m| m.borrow().get(&aid).cloned());
    let entry = match entry {
        Some(e) => e,
        None => return Ok(Value::False),
    };

    // Create socket
    let fd = unsafe { libc::socket(entry.family, entry.socktype, entry.protocol) };
    if fd < 0 {
        let e = std::io::Error::last_os_error();
        set_last_error(map_io_error(&e));
        vm.emit_warning(&format!(
            "socket_addrinfo_connect(): Unable to connect [{}]: {}",
            map_io_error(&e),
            errno_to_string(map_io_error(&e))
        ));
        return Ok(Value::False);
    }

    // Connect via std::net
    let result = match entry.socktype as i64 {
        SOCK_STREAM => {
            let stream = unsafe { TcpStream::from_raw_fd(fd) };
            // Already connected if connect succeeds. Use connect via underlying API.
            // Actually we should connect manually via libc
            match TcpStream::connect(entry.addr) {
                Ok(s) => {
                    // Drop the original fd by replacing the placeholder
                    drop(stream);
                    Ok(SocketHandle::TcpClient(s))
                }
                Err(e) => {
                    drop(stream);
                    Err(e)
                }
            }
        }
        SOCK_DGRAM => {
            let sock = unsafe { UdpSocket::from_raw_fd(fd) };
            match sock.connect(entry.addr) {
                Ok(_) => Ok(SocketHandle::Udp(sock)),
                Err(e) => Err(e),
            }
        }
        _ => {
            unsafe { libc::close(fd) };
            Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Unsupported socktype",
            ))
        }
    };

    match result {
        Ok(handle) => {
            let sid = alloc_socket_id_via_vm(vm);
            SOCKETS.with(|s| {
                s.borrow_mut().insert(sid, handle);
            });
            set_last_error(0);
            Ok(make_socket_object(sid))
        }
        Err(e) => {
            set_last_error(map_io_error(&e));
            vm.emit_warning(&format!(
                "socket_addrinfo_connect(): Unable to connect [{}]: {}",
                map_io_error(&e),
                errno_to_string(map_io_error(&e))
            ));
            Ok(Value::False)
        }
    }
}

/// socket_addrinfo_bind(AddressInfo $address): Socket|false
fn socket_addrinfo_bind_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let aid = match arg_addrinfo_id(args.first()) {
        Some(a) => a,
        None => return Ok(Value::False),
    };
    let entry = ADDRINFOS.with(|m| m.borrow().get(&aid).cloned());
    let entry = match entry {
        Some(e) => e,
        None => return Ok(Value::False),
    };

    let result = match entry.socktype as i64 {
        SOCK_STREAM => match TcpListener::bind(entry.addr) {
            Ok(l) => Ok(SocketHandle::TcpServer(l)),
            Err(e) => Err(e),
        },
        SOCK_DGRAM => match UdpSocket::bind(entry.addr) {
            Ok(s) => Ok(SocketHandle::Udp(s)),
            Err(e) => Err(e),
        },
        _ => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "Unsupported socktype",
        )),
    };

    match result {
        Ok(handle) => {
            let sid = alloc_socket_id_via_vm(vm);
            SOCKETS.with(|s| {
                s.borrow_mut().insert(sid, handle);
            });
            set_last_error(0);
            Ok(make_socket_object(sid))
        }
        Err(e) => {
            set_last_error(map_io_error(&e));
            vm.emit_warning(&format!(
                "socket_addrinfo_bind(): Unable to bind [{}]: {}",
                map_io_error(&e),
                errno_to_string(map_io_error(&e))
            ));
            Ok(Value::False)
        }
    }
}

/// socket_addrinfo_explain(AddressInfo $address): array
fn socket_addrinfo_explain_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let aid = match arg_addrinfo_id(args.first()) {
        Some(a) => a,
        None => {
            let mut result = PhpArray::new();
            result.set(
                ArrayKey::String(PhpString::from_bytes(b"ai_flags")),
                Value::Long(0),
            );
            result.set(
                ArrayKey::String(PhpString::from_bytes(b"ai_family")),
                Value::Long(0),
            );
            result.set(
                ArrayKey::String(PhpString::from_bytes(b"ai_socktype")),
                Value::Long(0),
            );
            result.set(
                ArrayKey::String(PhpString::from_bytes(b"ai_protocol")),
                Value::Long(0),
            );
            return Ok(Value::Array(Rc::new(RefCell::new(result))));
        }
    };

    let entry = ADDRINFOS.with(|m| m.borrow().get(&aid).cloned());
    let entry = match entry {
        Some(e) => e,
        None => return Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new())))),
    };

    let mut result = PhpArray::new();
    result.set(
        ArrayKey::String(PhpString::from_bytes(b"ai_flags")),
        Value::Long(entry.flags as i64),
    );
    result.set(
        ArrayKey::String(PhpString::from_bytes(b"ai_family")),
        Value::Long(entry.family as i64),
    );
    result.set(
        ArrayKey::String(PhpString::from_bytes(b"ai_socktype")),
        Value::Long(entry.socktype as i64),
    );
    result.set(
        ArrayKey::String(PhpString::from_bytes(b"ai_protocol")),
        Value::Long(entry.protocol as i64),
    );

    // ai_addr
    let mut addr_arr = PhpArray::new();
    addr_arr.set(
        ArrayKey::String(PhpString::from_bytes(b"sin_port")),
        Value::Long(entry.addr.port() as i64),
    );
    addr_arr.set(
        ArrayKey::String(PhpString::from_bytes(b"sin_addr")),
        Value::String(PhpString::from_string(entry.addr.ip().to_string())),
    );
    result.set(
        ArrayKey::String(PhpString::from_bytes(b"ai_addr")),
        Value::Array(Rc::new(RefCell::new(addr_arr))),
    );

    Ok(Value::Array(Rc::new(RefCell::new(result))))
}

/// socket_sendto(Socket $socket, string $data, int $length, int $flags, string $address, ?int $port = null): int|false
#[allow(clippy::unnecessary_unwrap)] // Null/Undef share the is_none branch
fn socket_sendto_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let socket_id = match arg_socket_id(args.first()) {
        Some(s) => s,
        None => return Ok(Value::False),
    };
    let data = args
        .get(1)
        .map(|v| v.to_php_string())
        .unwrap_or_else(PhpString::empty);
    let length = args.get(2).map(|v| v.to_long() as usize).unwrap_or(0);
    let _flags = args.get(3).map(|v| v.to_long()).unwrap_or(0);
    let address = args
        .get(4)
        .map(|v| v.to_php_string().to_string_lossy())
        .unwrap_or_default();
    let port_arg = args.get(5);

    // Determine if AF_UNIX vs AF_INET to validate
    let is_udp = SOCKETS.with(|s| matches!(s.borrow().get(&socket_id), Some(SocketHandle::Udp(_))));

    // Unix socket path validation
    if port_arg.is_none() || matches!(port_arg, Some(Value::Null) | Some(Value::Undef)) {
        // No port = unix socket; check sun_path length (108 on Linux)
        if address.len() >= 108 {
            return Err(throw_value_error(
                vm,
                &format!(
                    "socket_sendto(): Argument #5 ($address) must be less than {}",
                    108
                ),
            ));
        }
    } else {
        let p = port_arg.unwrap().to_long();
        if !(0..=65535).contains(&p) {
            return Err(throw_value_error(
                vm,
                "socket_sendto(): Argument #6 ($port) must be between 0 and 65535",
            ));
        }
        let port = p as u16;
        let addr_str = format!("{}:{}", address, port);
        if let Ok(addr) = addr_str.parse::<SocketAddr>() {
            let bytes = data.as_bytes();
            let send_bytes = if length > 0 && length < bytes.len() {
                &bytes[..length]
            } else {
                bytes
            };
            if is_udp {
                let result = SOCKETS.with(|s| {
                    let sockets = s.borrow();
                    if let Some(SocketHandle::Udp(sock)) = sockets.get(&socket_id) {
                        sock.send_to(send_bytes, addr)
                    } else {
                        Err(std::io::Error::new(
                            std::io::ErrorKind::InvalidInput,
                            "Not a UDP socket",
                        ))
                    }
                });
                match result {
                    Ok(n) => {
                        set_last_error(0);
                        return Ok(Value::Long(n as i64));
                    }
                    Err(e) => {
                        set_last_error(map_io_error(&e));
                        vm.emit_warning(&format!(
                            "socket_sendto(): Unable to send data [{}]: {}",
                            map_io_error(&e),
                            errno_to_string(map_io_error(&e))
                        ));
                        return Ok(Value::False);
                    }
                }
            }
        }
    }

    Ok(Value::False)
}

/// socket_recvfrom(Socket $socket, string &$buf, int $length, int $flags, string &$name, int &$port = null): int|false
fn socket_recvfrom_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let socket_id = match arg_socket_id(args.first()) {
        Some(s) => s,
        None => return Ok(Value::False),
    };
    let buf_ref = args.get(1);
    let length_l = args.get(2).map(|v| v.to_long()).unwrap_or(0);
    if length_l < 0 || length_l > i32::MAX as i64 {
        return Ok(Value::False);
    }
    let length = length_l as usize;
    let _flags = args.get(3).map(|v| v.to_long()).unwrap_or(0);
    let name_ref = args.get(4);
    let port_ref = args.get(5);

    let result = SOCKETS.with(|s| {
        let sockets = s.borrow();
        match sockets.get(&socket_id) {
            Some(SocketHandle::Udp(sock)) => {
                let mut buf = vec![0u8; length];
                match sock.recv_from(&mut buf) {
                    Ok((n, addr)) => {
                        buf.truncate(n);
                        Ok((buf, n, Some(addr)))
                    }
                    Err(e) => Err(e),
                }
            }
            _ => Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Invalid socket",
            )),
        }
    });

    match result {
        Ok((data, n, addr)) => {
            if let Some(r) = buf_ref {
                if let Value::Reference(rc) = r {
                    *rc.borrow_mut() = Value::String(PhpString::from_vec(data));
                }
            }
            if let Some(addr) = addr {
                if let Some(r) = name_ref {
                    if let Value::Reference(rc) = r {
                        *rc.borrow_mut() =
                            Value::String(PhpString::from_string(addr.ip().to_string()));
                    }
                }
                if let Some(r) = port_ref {
                    if let Value::Reference(rc) = r {
                        *rc.borrow_mut() = Value::Long(addr.port() as i64);
                    }
                }
            }
            set_last_error(0);
            Ok(Value::Long(n as i64))
        }
        Err(e) => {
            set_last_error(map_io_error(&e));
            vm.emit_warning(&format!(
                "socket_recvfrom(): unable to receive data [{}]: {}",
                map_io_error(&e),
                errno_to_string(map_io_error(&e))
            ));
            Ok(Value::False)
        }
    }
}

fn socket_cmsg_space_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let _level = if !args.is_empty() {
        args[0].to_long()
    } else {
        0
    };
    let _type_ = if args.len() > 1 { args[1].to_long() } else { 0 };
    Ok(Value::Long(0))
}

fn socket_sendmsg_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::False)
}

fn socket_recvmsg_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::False)
}

fn socket_stub_false(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::False)
}

fn socket_atomark_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::False)
}
