use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream, UdpSocket};

use goro_core::array::{ArrayKey, PhpArray};
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};
use std::rc::Rc;

/// Socket handle variants
enum SocketHandle {
    TcpClient(TcpStream),
    TcpServer(TcpListener),
    Udp(UdpSocket),
}

thread_local! {
    static SOCKETS: RefCell<HashMap<i64, SocketHandle>> = RefCell::new(HashMap::new());
    static NEXT_SOCKET_ID: Cell<i64> = const { Cell::new(1) };
    static LAST_ERROR: Cell<i64> = const { Cell::new(0) };
}

fn alloc_socket_id() -> i64 {
    NEXT_SOCKET_ID.with(|c| {
        let id = c.get();
        c.set(id + 1);
        id
    })
}

fn set_last_error(code: i64) {
    LAST_ERROR.with(|c| c.set(code));
}

fn get_last_error() -> i64 {
    LAST_ERROR.with(|c| c.get())
}

// Socket domain/type/protocol constants
const AF_INET: i64 = 2;
const AF_INET6: i64 = 10;
const SOCK_STREAM: i64 = 1;
const SOCK_DGRAM: i64 = 2;

// Socket option constants (used in set/get socket option implementations)
const SOL_SOCKET: i64 = 1;
const SO_RCVTIMEO: i64 = 20;
const SO_SNDTIMEO: i64 = 21;

// Read mode constants
const PHP_BINARY_READ: i64 = 2;

/// Register all socket extension functions and constants
pub fn register(vm: &mut Vm) {
    vm.register_extension(b"sockets");
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
    vm.register_function(b"socket_setopt", socket_set_option); // alias
    vm.register_function(b"socket_getopt", socket_get_option); // alias
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

    // Constants
    vm.constants.insert(b"AF_INET".to_vec(), Value::Long(2));
    vm.constants.insert(b"AF_INET6".to_vec(), Value::Long(10));
    vm.constants.insert(b"AF_UNIX".to_vec(), Value::Long(1));
    vm.constants.insert(b"SOCK_STREAM".to_vec(), Value::Long(1));
    vm.constants.insert(b"SOCK_DGRAM".to_vec(), Value::Long(2));
    vm.constants.insert(b"SOCK_RAW".to_vec(), Value::Long(3));
    vm.constants.insert(b"SOL_SOCKET".to_vec(), Value::Long(1));
    vm.constants.insert(b"SOL_TCP".to_vec(), Value::Long(6));
    vm.constants.insert(b"SOL_UDP".to_vec(), Value::Long(17));
    vm.constants.insert(b"SO_REUSEADDR".to_vec(), Value::Long(2));
    vm.constants.insert(b"SO_KEEPALIVE".to_vec(), Value::Long(9));
    vm.constants.insert(b"SO_BROADCAST".to_vec(), Value::Long(6));
    vm.constants.insert(b"SO_RCVTIMEO".to_vec(), Value::Long(20));
    vm.constants.insert(b"SO_SNDTIMEO".to_vec(), Value::Long(21));
    vm.constants.insert(b"SO_RCVBUF".to_vec(), Value::Long(8));
    vm.constants.insert(b"SO_SNDBUF".to_vec(), Value::Long(7));
    vm.constants.insert(b"TCP_NODELAY".to_vec(), Value::Long(1));
    vm.constants.insert(b"MSG_DONTWAIT".to_vec(), Value::Long(64));
    vm.constants.insert(b"MSG_PEEK".to_vec(), Value::Long(2));
    vm.constants.insert(b"PHP_NORMAL_READ".to_vec(), Value::Long(1));
    vm.constants.insert(b"PHP_BINARY_READ".to_vec(), Value::Long(2));
    vm.constants.insert(b"SOCKET_EINTR".to_vec(), Value::Long(4));
    vm.constants.insert(b"SOCKET_EACCES".to_vec(), Value::Long(13));
    vm.constants.insert(b"SOCKET_ECONNREFUSED".to_vec(), Value::Long(111));
    vm.constants.insert(b"SOCKET_ETIMEDOUT".to_vec(), Value::Long(110));
    vm.constants.insert(b"SOCKET_ECONNRESET".to_vec(), Value::Long(104));

    // Shutdown constants
    vm.constants.insert(b"SHUT_RD".to_vec(), Value::Long(0));
    vm.constants.insert(b"SHUT_WR".to_vec(), Value::Long(1));
    vm.constants.insert(b"SHUT_RDWR".to_vec(), Value::Long(2));

    // Missing socket constants
    vm.constants.insert(b"IPPROTO_IP".to_vec(), Value::Long(0));
    vm.constants.insert(b"IPPROTO_IPV6".to_vec(), Value::Long(41));
    vm.constants.insert(b"MSG_OOB".to_vec(), Value::Long(1));
    vm.constants.insert(b"MSG_WAITALL".to_vec(), Value::Long(256));
    vm.constants.insert(b"MSG_EOF".to_vec(), Value::Long(512));
    vm.constants.insert(b"MSG_DONTROUTE".to_vec(), Value::Long(4));
    vm.constants.insert(b"SO_TYPE".to_vec(), Value::Long(3));
    vm.constants.insert(b"SO_LINGER".to_vec(), Value::Long(13));
    vm.constants.insert(b"SO_ERROR".to_vec(), Value::Long(4));
    vm.constants.insert(b"SO_REUSEPORT".to_vec(), Value::Long(15));
    vm.constants.insert(b"SO_OOBINLINE".to_vec(), Value::Long(10));
    vm.constants.insert(b"IP_MULTICAST_LOOP".to_vec(), Value::Long(34));
    vm.constants.insert(b"IP_MULTICAST_TTL".to_vec(), Value::Long(33));
    vm.constants.insert(b"IP_MULTICAST_IF".to_vec(), Value::Long(32));
    vm.constants.insert(b"IP_TOS".to_vec(), Value::Long(1));
    vm.constants.insert(b"IP_TTL".to_vec(), Value::Long(2));
    vm.constants.insert(b"IPV6_MULTICAST_LOOP".to_vec(), Value::Long(19));
    vm.constants.insert(b"IPV6_MULTICAST_HOPS".to_vec(), Value::Long(18));
    vm.constants.insert(b"IPV6_MULTICAST_IF".to_vec(), Value::Long(17));
    vm.constants.insert(b"IPV6_V6ONLY".to_vec(), Value::Long(26));
    vm.constants.insert(b"MCAST_JOIN_GROUP".to_vec(), Value::Long(42));
    vm.constants.insert(b"MCAST_LEAVE_GROUP".to_vec(), Value::Long(45));
    vm.constants.insert(b"MCAST_BLOCK_SOURCE".to_vec(), Value::Long(43));
    vm.constants.insert(b"MCAST_UNBLOCK_SOURCE".to_vec(), Value::Long(44));
    vm.constants.insert(b"MCAST_JOIN_SOURCE_GROUP".to_vec(), Value::Long(46));
    vm.constants.insert(b"MCAST_LEAVE_SOURCE_GROUP".to_vec(), Value::Long(47));
    vm.constants.insert(b"STREAM_IPPROTO_IP".to_vec(), Value::Long(0));
    vm.constants.insert(b"STREAM_IPPROTO_TCP".to_vec(), Value::Long(6));
    vm.constants.insert(b"STREAM_IPPROTO_UDP".to_vec(), Value::Long(17));
    vm.constants.insert(b"STREAM_IPPROTO_ICMP".to_vec(), Value::Long(1));
    vm.constants.insert(b"STREAM_IPPROTO_RAW".to_vec(), Value::Long(255));
}

/// socket_create(int $domain, int $type, int $protocol): Socket|false
fn socket_create(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let domain = args.first().map(|v| v.to_long()).unwrap_or(0);
    let sock_type = args.get(1).map(|v| v.to_long()).unwrap_or(0);
    let _protocol = args.get(2).map(|v| v.to_long()).unwrap_or(0);

    if domain != AF_INET && domain != AF_INET6 {
        vm.emit_warning("socket_create(): Only AF_INET and AF_INET6 are supported");
        set_last_error(97); // EAFNOSUPPORT
        return Ok(Value::False);
    }

    match sock_type {
        SOCK_STREAM => {
            // For TCP, we just allocate an ID. The actual socket is created on connect/bind.
            // We store a placeholder that will be replaced.
            let id = alloc_socket_id();
            // Create a connected-to-nothing TCP placeholder via a dummy listener trick.
            // Actually, Rust's std::net doesn't let us create an unconnected TcpStream.
            // We store the domain info and create the real socket on connect/bind/listen.
            // Use a UDP socket as a temporary placeholder (it's cheap).
            match UdpSocket::bind("0.0.0.0:0") {
                Ok(sock) => {
                    // We mark this as a "pending TCP" by storing metadata.
                    // But our enum doesn't have that variant. Let's just store the ID
                    // and handle creation lazily.
                    // Actually, let's just create the ID and store nothing yet.
                    // We'll track "pending" sockets via a separate approach.
                    drop(sock);
                    // Socket not stored yet - will be created on connect/bind
                    // Return the socket ID as a Long (acting as a Socket resource)
                    Ok(Value::Long(id))
                }
                Err(e) => {
                    set_last_error(map_io_error(&e));
                    vm.emit_warning(&format!("socket_create(): Unable to create socket: {}", e));
                    Ok(Value::False)
                }
            }
        }
        SOCK_DGRAM => {
            let bind_addr = if domain == AF_INET6 { "[::]:0" } else { "0.0.0.0:0" };
            match UdpSocket::bind(bind_addr) {
                Ok(sock) => {
                    let id = alloc_socket_id();
                    SOCKETS.with(|s| {
                        s.borrow_mut().insert(id, SocketHandle::Udp(sock));
                    });
                    Ok(Value::Long(id))
                }
                Err(e) => {
                    set_last_error(map_io_error(&e));
                    vm.emit_warning(&format!("socket_create(): Unable to create socket: {}", e));
                    Ok(Value::False)
                }
            }
        }
        _ => {
            vm.emit_warning(&format!(
                "socket_create(): Invalid socket type {}",
                sock_type
            ));
            set_last_error(94); // ESOCKTNOSUPPORT
            Ok(Value::False)
        }
    }
}

/// socket_connect(Socket $socket, string $address, ?int $port = null): bool
fn socket_connect(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let socket_id = args.first().map(|v| v.to_long()).unwrap_or(0);
    let address = args
        .get(1)
        .map(|v| v.to_php_string().to_string_lossy())
        .unwrap_or_default();
    let port = args.get(2).map(|v| v.to_long() as u16).unwrap_or(0);

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
                    "socket_connect(): Unable to connect [{}]: {}",
                    map_io_error(&e),
                    e
                ));
                Ok(Value::False)
            }
        },
        Err(_) => {
            // Try DNS resolution via ToSocketAddrs
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
                                    "socket_connect(): Unable to connect [{}]: {}",
                                    map_io_error(&e),
                                    e
                                ));
                                Ok(Value::False)
                            }
                        }
                    } else {
                        set_last_error(111); // ECONNREFUSED
                        vm.emit_warning("socket_connect(): Unable to connect: No addresses found");
                        Ok(Value::False)
                    }
                }
                Err(e) => {
                    set_last_error(map_io_error(&e));
                    vm.emit_warning(&format!(
                        "socket_connect(): Unable to connect [{}]: {}",
                        map_io_error(&e),
                        e
                    ));
                    Ok(Value::False)
                }
            }
        }
    }
}

/// socket_bind(Socket $socket, string $address, int $port = 0): bool
fn socket_bind(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let socket_id = args.first().map(|v| v.to_long()).unwrap_or(0);
    let address = args
        .get(1)
        .map(|v| v.to_php_string().to_string_lossy())
        .unwrap_or_default();
    let port = args.get(2).map(|v| v.to_long() as u16).unwrap_or(0);

    let addr_str = format!("{}:{}", address, port);

    // Check if there is already a UDP socket with this ID
    let is_udp = SOCKETS.with(|s| {
        matches!(s.borrow().get(&socket_id), Some(SocketHandle::Udp(_)))
    });

    if is_udp {
        // For UDP, we need to rebind. Drop the old one and create a new one.
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
                    "socket_bind(): Unable to bind address [{}]: {}",
                    map_io_error(&e),
                    e
                ));
                Ok(Value::False)
            }
        }
    } else {
        // TCP server: create a TcpListener
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
                    "socket_bind(): Unable to bind address [{}]: {}",
                    map_io_error(&e),
                    e
                ));
                Ok(Value::False)
            }
        }
    }
}

/// socket_listen(Socket $socket, int $backlog = 0): bool
fn socket_listen(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let socket_id = args.first().map(|v| v.to_long()).unwrap_or(0);
    let _backlog = args.get(1).map(|v| v.to_long()).unwrap_or(128);

    // Rust's TcpListener::bind already calls listen(), so this is a no-op if already bound
    let exists = SOCKETS.with(|s| {
        matches!(s.borrow().get(&socket_id), Some(SocketHandle::TcpServer(_)))
    });

    if exists {
        set_last_error(0);
        Ok(Value::True)
    } else {
        set_last_error(95); // EOPNOTSUPP
        vm.emit_warning("socket_listen(): Unable to listen on socket: not a server socket");
        Ok(Value::False)
    }
}

/// socket_accept(Socket $socket): Socket|false
fn socket_accept(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let socket_id = args.first().map(|v| v.to_long()).unwrap_or(0);

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
            let id = alloc_socket_id();
            SOCKETS.with(|s| {
                s.borrow_mut()
                    .insert(id, SocketHandle::TcpClient(stream));
            });
            set_last_error(0);
            Ok(Value::Long(id))
        }
        Err(e) => {
            set_last_error(map_io_error(&e));
            vm.emit_warning(&format!(
                "socket_accept(): Unable to accept connection [{}]: {}",
                map_io_error(&e),
                e
            ));
            Ok(Value::False)
        }
    }
}

/// socket_read(Socket $socket, int $length, int $mode = PHP_BINARY_READ): string|false
fn socket_read(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let socket_id = args.first().map(|v| v.to_long()).unwrap_or(0);
    let length = args.get(1).map(|v| v.to_long()).unwrap_or(1024) as usize;
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
                    // PHP_NORMAL_READ: read until \n or \0 or length
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
            Ok(Value::String(goro_core::string::PhpString::from_vec(data)))
        }
        Err(e) => {
            set_last_error(map_io_error(&e));
            vm.emit_warning(&format!("socket_read(): Unable to read from socket [{}]: {}", map_io_error(&e), e));
            Ok(Value::False)
        }
    }
}

/// socket_write(Socket $socket, string $data, ?int $length = null): int|false
fn socket_write(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let socket_id = args.first().map(|v| v.to_long()).unwrap_or(0);
    let data = args
        .get(1)
        .map(|v| v.to_php_string())
        .unwrap_or_else(|| goro_core::string::PhpString::empty());
    let max_len = args.get(2).map(|v| v.to_long() as usize);

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
            vm.emit_warning(&format!("socket_write(): Unable to write to socket [{}]: {}", map_io_error(&e), e));
            Ok(Value::False)
        }
    }
}

/// socket_close(Socket $socket): void
fn socket_close(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let socket_id = args.first().map(|v| v.to_long()).unwrap_or(0);
    SOCKETS.with(|s| {
        s.borrow_mut().remove(&socket_id);
    });
    Ok(Value::Null)
}

/// socket_send(Socket $socket, string $data, int $length, int $flags): int|false
fn socket_send(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let socket_id = args.first().map(|v| v.to_long()).unwrap_or(0);
    let data = args
        .get(1)
        .map(|v| v.to_php_string())
        .unwrap_or_else(|| goro_core::string::PhpString::empty());
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
            vm.emit_warning(&format!("socket_send(): Unable to send data [{}]: {}", map_io_error(&e), e));
            Ok(Value::False)
        }
    }
}

/// socket_recv(Socket $socket, string &$data, int $length, int $flags): int|false
fn socket_recv(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let socket_id = args.first().map(|v| v.to_long()).unwrap_or(0);
    let data_ref = args.get(1);
    let length = args.get(2).map(|v| v.to_long() as usize).unwrap_or(1024);
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
            // Write data back to the reference parameter
            if let Some(r) = data_ref {
                let val = Value::String(goro_core::string::PhpString::from_vec(data));
                if let Value::Reference(rc) = r {
                    *rc.borrow_mut() = val;
                }
            }
            set_last_error(0);
            Ok(Value::Long(n as i64))
        }
        Err(e) => {
            set_last_error(map_io_error(&e));
            vm.emit_warning(&format!("socket_recv(): Unable to receive data [{}]: {}", map_io_error(&e), e));
            Ok(Value::False)
        }
    }
}

/// socket_set_option(Socket $socket, int $level, int $optname, mixed $optval): bool
fn socket_set_option(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let socket_id = args.first().map(|v| v.to_long()).unwrap_or(0);
    let level = args.get(1).map(|v| v.to_long()).unwrap_or(0);
    let optname = args.get(2).map(|v| v.to_long()).unwrap_or(0);
    let optval = args.get(3).cloned().unwrap_or(Value::Null);

    let result = SOCKETS.with(|s| {
        let sockets = s.borrow();
        match sockets.get(&socket_id) {
            Some(handle) => {
                set_socket_option(handle, level, optname, &optval)
            }
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
                e
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
    };

    // Handle SO_RCVTIMEO and SO_SNDTIMEO specially - they take a timeval struct
    if level == SOL_SOCKET && (optname == SO_RCVTIMEO || optname == SO_SNDTIMEO) {
        // optval should be an array with 'sec' and 'usec' keys, or an integer (seconds)
        let (sec, usec) = match optval {
            Value::Array(arr) => {
                let arr = arr.borrow();
                let sec = arr
                    .get(&goro_core::array::ArrayKey::String(
                        goro_core::string::PhpString::from_bytes(b"sec"),
                    ))
                    .map(|v| v.to_long())
                    .unwrap_or(0);
                let usec = arr
                    .get(&goro_core::array::ArrayKey::String(
                        goro_core::string::PhpString::from_bytes(b"usec"),
                    ))
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
    let socket_id = args.first().map(|v| v.to_long()).unwrap_or(0);
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
                e
            ));
            Ok(Value::False)
        }
    }
}

#[cfg(unix)]
fn get_socket_option(handle: &SocketHandle, level: i64, optname: i64) -> std::io::Result<Value> {
    use std::cell::RefCell as StdRefCell;
    use std::os::unix::io::AsRawFd;
    use std::rc::Rc;

    let fd = match handle {
        SocketHandle::TcpClient(s) => s.as_raw_fd(),
        SocketHandle::TcpServer(s) => s.as_raw_fd(),
        SocketHandle::Udp(s) => s.as_raw_fd(),
    };

    // SO_RCVTIMEO and SO_SNDTIMEO return an array
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
            let mut arr = goro_core::array::PhpArray::new();
            arr.set(
                goro_core::array::ArrayKey::String(goro_core::string::PhpString::from_bytes(
                    b"sec",
                )),
                Value::Long(timeval.tv_sec as i64),
            );
            arr.set(
                goro_core::array::ArrayKey::String(goro_core::string::PhpString::from_bytes(
                    b"usec",
                )),
                Value::Long(timeval.tv_usec as i64),
            );
            Ok(Value::Array(Rc::new(StdRefCell::new(arr))))
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
fn get_socket_option(
    _handle: &SocketHandle,
    _level: i64,
    _optname: i64,
) -> std::io::Result<Value> {
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
fn socket_strerror(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let errno = args.first().map(|v| v.to_long()).unwrap_or(0);
    let msg = errno_to_string(errno);
    Ok(Value::String(goro_core::string::PhpString::from_bytes(
        msg.as_bytes(),
    )))
}

/// socket_set_nonblock(Socket $socket): bool
fn socket_set_nonblock(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let socket_id = args.first().map(|v| v.to_long()).unwrap_or(0);

    let result = SOCKETS.with(|s| {
        let sockets = s.borrow();
        match sockets.get(&socket_id) {
            Some(SocketHandle::TcpClient(stream)) => stream.set_nonblocking(true),
            Some(SocketHandle::TcpServer(listener)) => listener.set_nonblocking(true),
            Some(SocketHandle::Udp(sock)) => sock.set_nonblocking(true),
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
                "socket_set_nonblock(): Unable to set non-blocking mode [{}]: {}",
                map_io_error(&e),
                e
            ));
            Ok(Value::False)
        }
    }
}

/// socket_set_block(Socket $socket): bool
fn socket_set_block(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let socket_id = args.first().map(|v| v.to_long()).unwrap_or(0);

    let result = SOCKETS.with(|s| {
        let sockets = s.borrow();
        match sockets.get(&socket_id) {
            Some(SocketHandle::TcpClient(stream)) => stream.set_nonblocking(false),
            Some(SocketHandle::TcpServer(listener)) => listener.set_nonblocking(false),
            Some(SocketHandle::Udp(sock)) => sock.set_nonblocking(false),
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
                "socket_set_block(): Unable to set blocking mode [{}]: {}",
                map_io_error(&e),
                e
            ));
            Ok(Value::False)
        }
    }
}

/// socket_getpeername(Socket $socket, string &$address, int &$port = null): bool
fn socket_getpeername(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let socket_id = args.first().map(|v| v.to_long()).unwrap_or(0);
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
                let val = Value::String(goro_core::string::PhpString::from_bytes(ip_str.as_bytes()));
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
                "socket_getpeername(): Unable to get peer name [{}]: {}",
                map_io_error(&e),
                e
            ));
            Ok(Value::False)
        }
    }
}

/// socket_getsockname(Socket $socket, string &$address, int &$port = null): bool
fn socket_getsockname(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let socket_id = args.first().map(|v| v.to_long()).unwrap_or(0);
    let addr_ref = args.get(1);
    let port_ref = args.get(2);

    let result = SOCKETS.with(|s| {
        let sockets = s.borrow();
        match sockets.get(&socket_id) {
            Some(SocketHandle::TcpClient(stream)) => stream.local_addr(),
            Some(SocketHandle::TcpServer(listener)) => listener.local_addr(),
            Some(SocketHandle::Udp(sock)) => sock.local_addr(),
            None => Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Invalid socket",
            )),
        }
    });

    match result {
        Ok(addr) => {
            if let Some(r) = addr_ref {
                let ip_str = addr.ip().to_string();
                let val = Value::String(goro_core::string::PhpString::from_bytes(ip_str.as_bytes()));
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
                "socket_getsockname(): Unable to get socket name [{}]: {}",
                map_io_error(&e),
                e
            ));
            Ok(Value::False)
        }
    }
}

/// socket_shutdown(Socket $socket, int $mode = 2): bool
fn socket_shutdown(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let socket_id = args.first().map(|v| v.to_long()).unwrap_or(0);
    let how = args.get(1).map(|v| v.to_long()).unwrap_or(2);

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
                e
            ));
            Ok(Value::False)
        }
    }
}

/// socket_select(array &$read, array &$write, array &$except, ?int $seconds, int $microseconds = 0): int|false
///
/// This is a simplified implementation using polling with timeouts.
/// Full select(2) support would require platform-specific code.
fn socket_select(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let _read_ref = args.first();
    let _write_ref = args.get(1);
    let _except_ref = args.get(2);
    let seconds = args.get(3).map(|v| {
        if matches!(v, Value::Null) {
            None
        } else {
            Some(v.to_long())
        }
    }).unwrap_or(None);
    let microseconds = args.get(4).map(|v| v.to_long()).unwrap_or(0);

    #[cfg(unix)]
    {
        use std::os::unix::io::AsRawFd;

        // Collect file descriptors from read array
        let mut read_fds: Vec<(i64, i32)> = Vec::new(); // (socket_id, fd)
        let mut write_fds: Vec<(i64, i32)> = Vec::new();

        if let Some(read_val) = _read_ref {
            let read_val = read_val.deref();
            if let Value::Array(arr) = &read_val {
                let arr = arr.borrow();
                for (_key, val) in arr.iter() {
                    let sid = val.deref().to_long();
                    SOCKETS.with(|s| {
                        let sockets = s.borrow();
                        if let Some(handle) = sockets.get(&sid) {
                            let fd = match handle {
                                SocketHandle::TcpClient(s) => s.as_raw_fd(),
                                SocketHandle::TcpServer(s) => s.as_raw_fd(),
                                SocketHandle::Udp(s) => s.as_raw_fd(),
                            };
                            read_fds.push((sid, fd));
                        }
                    });
                }
            }
        }

        if let Some(write_val) = _write_ref {
            let write_val = write_val.deref();
            if let Value::Array(arr) = &write_val {
                let arr = arr.borrow();
                for (_key, val) in arr.iter() {
                    let sid = val.deref().to_long();
                    SOCKETS.with(|s| {
                        let sockets = s.borrow();
                        if let Some(handle) = sockets.get(&sid) {
                            let fd = match handle {
                                SocketHandle::TcpClient(s) => s.as_raw_fd(),
                                SocketHandle::TcpServer(s) => s.as_raw_fd(),
                                SocketHandle::Udp(s) => s.as_raw_fd(),
                            };
                            write_fds.push((sid, fd));
                        }
                    });
                }
            }
        }

        if read_fds.is_empty() && write_fds.is_empty() {
            vm.emit_warning("socket_select(): No valid sockets to select on");
            return Ok(Value::False);
        }

        // Build fd_sets
        let mut max_fd: i32 = 0;
        unsafe {
            let mut read_set: libc::fd_set = std::mem::zeroed();
            let mut write_set: libc::fd_set = std::mem::zeroed();
            libc::FD_ZERO(&mut read_set);
            libc::FD_ZERO(&mut write_set);

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

            let timeout_ptr = if let Some(secs) = seconds {
                let mut tv = libc::timeval {
                    tv_sec: secs as libc::time_t,
                    tv_usec: microseconds as libc::suseconds_t,
                };
                &mut tv as *mut libc::timeval
            } else {
                std::ptr::null_mut()
            };

            let ret = libc::select(
                max_fd + 1,
                if read_fds.is_empty() { std::ptr::null_mut() } else { &mut read_set },
                if write_fds.is_empty() { std::ptr::null_mut() } else { &mut write_set },
                std::ptr::null_mut(), // except
                timeout_ptr,
            );

            if ret < 0 {
                let err = std::io::Error::last_os_error();
                set_last_error(map_io_error(&err));
                vm.emit_warning(&format!("socket_select(): select() failed [{}]: {}", map_io_error(&err), err));
                return Ok(Value::False);
            }

            // Update the read array to only contain ready sockets
            if let Some(read_val) = _read_ref {
                if let Value::Reference(rc) = read_val {
                    let mut arr = goro_core::array::PhpArray::new();
                    for &(sid, fd) in &read_fds {
                        if fd >= 0 && fd < libc::FD_SETSIZE as i32 && libc::FD_ISSET(fd, &read_set) {
                            arr.push(Value::Long(sid));
                        }
                    }
                    *rc.borrow_mut() = Value::Array(std::rc::Rc::new(std::cell::RefCell::new(arr)));
                }
            }

            // Update the write array to only contain ready sockets
            if let Some(write_val) = _write_ref {
                if let Value::Reference(rc) = write_val {
                    let mut arr = goro_core::array::PhpArray::new();
                    for &(sid, fd) in &write_fds {
                        if fd >= 0 && fd < libc::FD_SETSIZE as i32 && libc::FD_ISSET(fd, &write_set) {
                            arr.push(Value::Long(sid));
                        }
                    }
                    *rc.borrow_mut() = Value::Array(std::rc::Rc::new(std::cell::RefCell::new(arr)));
                }
            }

            // Clear except array
            if let Some(except_val) = _except_ref {
                if let Value::Reference(rc) = except_val {
                    *rc.borrow_mut() = Value::Array(std::rc::Rc::new(std::cell::RefCell::new(
                        goro_core::array::PhpArray::new(),
                    )));
                }
            }

            set_last_error(0);
            Ok(Value::Long(ret as i64))
        }
    }

    #[cfg(not(unix))]
    {
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
        std::io::ErrorKind::ConnectionRefused => 111,  // ECONNREFUSED
        std::io::ErrorKind::ConnectionReset => 104,    // ECONNRESET
        std::io::ErrorKind::ConnectionAborted => 103,  // ECONNABORTED
        std::io::ErrorKind::NotConnected => 107,       // ENOTCONN
        std::io::ErrorKind::AddrInUse => 98,           // EADDRINUSE
        std::io::ErrorKind::AddrNotAvailable => 99,    // EADDRNOTAVAIL
        std::io::ErrorKind::BrokenPipe => 32,          // EPIPE
        std::io::ErrorKind::AlreadyExists => 98,       // EADDRINUSE
        std::io::ErrorKind::WouldBlock => 11,          // EAGAIN
        std::io::ErrorKind::TimedOut => 110,           // ETIMEDOUT
        std::io::ErrorKind::Interrupted => 4,          // EINTR
        std::io::ErrorKind::PermissionDenied => 13,    // EACCES
        std::io::ErrorKind::InvalidInput => 22,        // EINVAL
        _ => 5,                                         // EIO
    }
}

/// Convert an errno value to a human-readable string
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

fn socket_create_listen_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    // Stub: return false (would need actual socket binding)
    Ok(Value::False)
}

fn socket_create_pair_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if args.len() < 4 {
        return Ok(Value::False);
    }
    // Create a real socketpair using libc
    let domain = args[0].to_long() as i32;
    let sock_type = args[1].to_long() as i32;
    let protocol = args[2].to_long() as i32;
    let mut fds: [i32; 2] = [0, 0];
    let result = unsafe { libc::socketpair(domain, sock_type, protocol, fds.as_mut_ptr()) };
    if result != 0 {
        return Ok(Value::False);
    }
    // Store the sockets as resource-like values in the output array
    // For now return false since we'd need proper socket resource integration
    unsafe { libc::close(fds[0]); libc::close(fds[1]); }
    Ok(Value::False)
}

fn socket_import_stream_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::False)
}

fn socket_export_stream_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::False)
}

fn socket_addrinfo_lookup_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new()))))
}

fn socket_addrinfo_connect_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::False)
}

fn socket_addrinfo_bind_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::False)
}

fn socket_addrinfo_explain_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    let mut result = PhpArray::new();
    result.set(ArrayKey::String(PhpString::from_bytes(b"ai_flags")), Value::Long(0));
    result.set(ArrayKey::String(PhpString::from_bytes(b"ai_family")), Value::Long(0));
    result.set(ArrayKey::String(PhpString::from_bytes(b"ai_socktype")), Value::Long(0));
    result.set(ArrayKey::String(PhpString::from_bytes(b"ai_protocol")), Value::Long(0));
    Ok(Value::Array(Rc::new(RefCell::new(result))))
}

fn socket_sendto_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::False)
}

fn socket_recvfrom_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::False)
}

fn socket_cmsg_space_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let _level = if !args.is_empty() { args[0].to_long() } else { 0 };
    let _type_ = if args.len() > 1 { args[1].to_long() } else { 0 };
    Ok(Value::Long(0))
}

fn socket_sendmsg_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::False)
}

fn socket_recvmsg_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::False)
}
