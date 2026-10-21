//! Network I/O (mysqlnd_vio.c) and the packet frame codec
//! (mysqlnd_protocol_frame_codec.c): packet framing, sequence numbers and
//! the compressed protocol.

use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::os::unix::net::UnixStream;
use std::time::Duration;

use super::stats::*;
use super::*;

pub enum Stream {
    Tcp(TcpStream),
    Unix(UnixStream),
    Tls(Box<TlsStream>),
}

/// A TCP stream after STREAM_CRYPTO_METHOD_TLS_CLIENT was enabled on it
pub struct TlsStream {
    sock: TcpStream,
    tls: purecrypto::tls::Connection,
    plain: Vec<u8>,
    ppos: usize,
}

fn tls_io_error(e: purecrypto::tls::Error) -> std::io::Error {
    std::io::Error::other(format!("{:?}", e))
}

impl TlsStream {
    fn flush_wire(&mut self) -> std::io::Result<()> {
        let out = self.tls.pop().map_err(tls_io_error)?;
        if !out.is_empty() {
            self.sock.write_all(&out)?;
        }
        Ok(())
    }

    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        loop {
            if self.ppos < self.plain.len() {
                let n = (self.plain.len() - self.ppos).min(buf.len());
                buf[..n].copy_from_slice(&self.plain[self.ppos..self.ppos + n]);
                self.ppos += n;
                return Ok(n);
            }
            self.plain = self.tls.recv().map_err(tls_io_error)?;
            self.ppos = 0;
            if !self.plain.is_empty() {
                continue;
            }
            if self.tls.received_close_notify() {
                return Ok(0);
            }
            let mut wire = [0u8; 16384];
            let n = self.sock.read(&mut wire)?;
            if n == 0 {
                return Ok(0);
            }
            self.tls.feed(&wire[..n]).map_err(tls_io_error)?;
            // post-handshake messages may need an answer
            self.flush_wire()?;
        }
    }

    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.tls.send(buf).map_err(tls_io_error)?;
        self.flush_wire()?;
        Ok(buf.len())
    }
}

impl Stream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            Stream::Tcp(s) => s.read(buf),
            Stream::Unix(s) => s.read(buf),
            Stream::Tls(s) => s.read(buf),
        }
    }
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            Stream::Tcp(s) => s.write(buf),
            Stream::Unix(s) => s.write(buf),
            Stream::Tls(s) => s.write(buf),
        }
    }
    fn set_read_timeout(&self, d: Option<Duration>) {
        let _ = match self {
            Stream::Tcp(s) => s.set_read_timeout(d),
            Stream::Unix(s) => s.set_read_timeout(d),
            Stream::Tls(s) => s.sock.set_read_timeout(d),
        };
    }
    fn set_write_timeout(&self, d: Option<Duration>) {
        let _ = match self {
            Stream::Tcp(s) => s.set_write_timeout(d),
            Stream::Unix(s) => s.set_write_timeout(d),
            Stream::Tls(s) => s.sock.set_write_timeout(d),
        };
    }
    pub fn raw_fd(&self) -> i32 {
        use std::os::unix::io::AsRawFd;
        match self {
            Stream::Tcp(s) => s.as_raw_fd(),
            Stream::Unix(s) => s.as_raw_fd(),
            Stream::Tls(s) => s.sock.as_raw_fd(),
        }
    }
}

/// MYSQLND_SSL_PEER
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SslPeer {
    #[default]
    Default,
    Verify,
    DontVerify,
}

/// MYSQLND_VIO
pub struct Vio {
    pub stream: Option<Stream>,
    pub is_unix: bool,
    rbuf: Vec<u8>,
    rpos: usize,
    pub timeout_connect: u32,
    pub timeout_read: u32,
    pub net_read_buffer_size: usize,
    pub ssl_key: Option<String>,
    pub ssl_cert: Option<String>,
    pub ssl_ca: Option<String>,
    pub ssl_capath: Option<String>,
    pub ssl_cipher: Option<String>,
    pub ssl_passphrase: Option<String>,
    pub ssl_verify_peer: SslPeer,
    pub ssl: bool,
    /// the host of the tcp:// scheme (peer name for TLS)
    pub host: String,
}

impl Vio {
    /// mysqlnd_vio::init
    pub fn new() -> Vio {
        let g = globals();
        Vio {
            stream: None,
            is_unix: false,
            rbuf: Vec::new(),
            rpos: 0,
            timeout_connect: 0,
            timeout_read: g.net_read_timeout.clamp(0, u32::MAX as i64) as u32,
            net_read_buffer_size: g.net_read_buffer_size,
            ssl_key: None,
            ssl_cert: None,
            ssl_ca: None,
            ssl_capath: None,
            ssl_cipher: None,
            ssl_passphrase: None,
            ssl_verify_peer: SslPeer::Default,
            ssl: false,
            host: String::new(),
        }
    }

    pub fn has_stream(&self) -> bool {
        self.stream.is_some()
    }

    /// Bytes received but not consumed yet.
    pub fn buffered(&self) -> usize {
        self.rbuf.len() - self.rpos
    }

    pub fn raw_fd(&self) -> Option<i32> {
        self.stream.as_ref().map(|s| s.raw_fd())
    }

    /// mysqlnd_vio::close_stream
    pub fn close_stream(&mut self) {
        if let Some(s) = self.stream.take() {
            match &s {
                Stream::Tcp(t) => {
                    let _ = t.shutdown(std::net::Shutdown::Both);
                }
                Stream::Tls(t) => {
                    let _ = t.sock.shutdown(std::net::Shutdown::Both);
                }
                Stream::Unix(_) => {}
            }
            drop(s);
        }
        self.rbuf.clear();
        self.rpos = 0;
        self.ssl = false;
    }

    /// mysqlnd_vio::connect (open_tcp_or_unix + post_connect_set_opt)
    pub fn connect(&mut self, scheme: &str, error_info: &mut ErrorInfo) -> bool {
        self.close_stream();
        let connect_timeout = if self.timeout_connect > 0 {
            Duration::from_secs(self.timeout_connect as u64)
        } else {
            let t = globals().default_socket_timeout;
            if t > 0.0 {
                Duration::from_secs_f64(t)
            } else {
                Duration::from_secs(60)
            }
        };
        let stream = if let Some(path) = scheme.strip_prefix("unix://") {
            match UnixStream::connect(path) {
                Ok(s) => {
                    self.is_unix = true;
                    Stream::Unix(s)
                }
                Err(e) => {
                    error_info.set_str(CR_CONNECTION_ERROR, UNKNOWN_SQLSTATE, &strerror(&e));
                    return false;
                }
            }
        } else if let Some(hostport) = scheme.strip_prefix("tcp://") {
            self.host = match hostport.rfind(':') {
                Some(i) => hostport[..i]
                    .trim_matches(|c| c == '[' || c == ']')
                    .to_string(),
                None => hostport.to_string(),
            };
            match connect_tcp(hostport, connect_timeout) {
                Ok(s) => {
                    self.is_unix = false;
                    let _ = s.set_nodelay(true);
                    set_keepalive(&s);
                    Stream::Tcp(s)
                }
                Err(msg) => {
                    error_info.set_str(CR_CONNECTION_ERROR, UNKNOWN_SQLSTATE, &msg);
                    return false;
                }
            }
        } else {
            error_info.set_str(
                CR_CONNECTION_ERROR,
                UNKNOWN_SQLSTATE,
                "No handler for this scheme",
            );
            return false;
        };
        // php streams: default_socket_timeout, unless timeout_read is set
        let read_to = if self.timeout_read > 0 {
            Some(Duration::from_secs(self.timeout_read as u64))
        } else {
            let t = globals().default_socket_timeout;
            if t > 0.0 {
                Some(Duration::from_secs_f64(t))
            } else {
                None
            }
        };
        stream.set_read_timeout(read_to);
        let t = globals().default_socket_timeout;
        stream.set_write_timeout(if t > 0.0 {
            Some(Duration::from_secs_f64(t))
        } else {
            None
        });
        self.stream = Some(stream);
        true
    }

    /// mysqlnd_vio::enable_ssl: the ssl stream context options, then
    /// php_stream_xport_crypto_setup/enable(STREAM_CRYPTO_METHOD_TLS_CLIENT)
    pub fn enable_ssl(&mut self) -> bool {
        use purecrypto::tls::{Config, Connection, HandshakeStatus, RootCertStore};
        let any_flag = self.ssl_key.is_some()
            || self.ssl_cert.is_some()
            || self.ssl_ca.is_some()
            || self.ssl_capath.is_some()
            || self.ssl_passphrase.is_some()
            || self.ssl_cipher.is_some();
        if self.ssl_verify_peer == SslPeer::Default {
            self.ssl_verify_peer = if any_flag {
                SslPeer::Verify
            } else {
                SslPeer::DontVerify
            };
        }
        let verify = self.ssl_verify_peer == SslPeer::Verify;
        // php_openssl_enable_crypto: cafile / capath
        let mut roots = RootCertStore::new();
        if let Some(ca) = &self.ssl_ca {
            match std::fs::read_to_string(ca) {
                Ok(pem) if roots.add_pem(&pem).is_ok() => {}
                _ => {
                    php_error(E_WARNING, format!("failed loading cafile stream: `{}'", ca));
                    return false;
                }
            }
        }
        if let Some(dir) = &self.ssl_capath {
            if let Ok(entries) = std::fs::read_dir(dir) {
                for e in entries.flatten() {
                    if let Ok(pem) = std::fs::read_to_string(e.path()) {
                        let _ = roots.add_pem(&pem);
                    }
                }
            }
        }
        let stream = match self.stream.take() {
            Some(Stream::Tcp(s)) => s,
            other => {
                self.stream = other;
                return false;
            }
        };
        // TLS 1.3 suites first, AES-256 preferred like OpenSSL's defaults
        let suites: [u16; 9] = [
            0x1302, 0x1303, 0x1301, 0xC02C, 0xC030, 0xCCA9, 0xCCA8, 0xC02B, 0xC02F,
        ];
        let mut b = Config::builder()
            .rng(std::sync::Arc::new(purecrypto::rng::OsRng))
            .tls_only()
            .roots(roots)
            .cipher_suites(&suites)
            .verify_certificates(verify);
        if self.host.parse::<std::net::IpAddr>().is_err() && !self.host.is_empty() {
            b = b.server_name(self.host.clone());
        }
        let cfg = b.build();
        let mut tls = match Connection::client(&cfg) {
            Ok(c) => c,
            Err(_) => return false,
        };
        let mut sock = stream;
        let mut wire = [0u8; 16384];
        let ok = loop {
            if let Ok(out) = tls.pop() {
                if !out.is_empty() && sock.write_all(&out).is_err() {
                    break false;
                }
            }
            match tls.handshake() {
                Ok(HandshakeStatus::Complete) => break true,
                Ok(HandshakeStatus::WantWrite) => continue,
                Ok(HandshakeStatus::WantRead) => match sock.read(&mut wire) {
                    Ok(0) | Err(_) => break false,
                    Ok(n) => {
                        if tls.feed(&wire[..n]).is_err() {
                            break false;
                        }
                    }
                },
                Err(_) => break false,
            }
        };
        if !ok {
            let _ = sock.shutdown(std::net::Shutdown::Both);
            return false;
        }
        self.stream = Some(Stream::Tls(Box::new(TlsStream {
            sock,
            tls,
            plain: Vec::new(),
            ppos: 0,
        })));
        self.ssl = true;
        true
    }

    /// mysqlnd_vio::network_read: reads exactly `buf.len()` bytes.
    pub fn network_read(&mut self, buf: &mut [u8], stats: &mut Stats) -> bool {
        let mut done = 0;
        let mut ok = true;
        while done < buf.len() {
            if self.rpos < self.rbuf.len() {
                let n = (self.rbuf.len() - self.rpos).min(buf.len() - done);
                buf[done..done + n].copy_from_slice(&self.rbuf[self.rpos..self.rpos + n]);
                self.rpos += n;
                done += n;
                continue;
            }
            let stream = match self.stream.as_mut() {
                Some(s) => s,
                None => {
                    ok = false;
                    break;
                }
            };
            let chunk = self.net_read_buffer_size.max(8192);
            self.rbuf.resize(chunk, 0);
            self.rpos = 0;
            match stream.read(&mut self.rbuf) {
                Ok(0) | Err(_) => {
                    self.rbuf.clear();
                    ok = false;
                    break;
                }
                Ok(n) => {
                    self.rbuf.truncate(n);
                }
            }
        }
        if self.rpos >= self.rbuf.len() {
            self.rbuf.clear();
            self.rpos = 0;
        }
        stats.inc_v(STAT_BYTES_RECEIVED, done as u64);
        ok
    }

    /// mysqlnd_vio::network_write
    pub fn network_write(&mut self, data: &[u8]) -> isize {
        let stream = match self.stream.as_mut() {
            Some(s) => s,
            None => return 0,
        };
        let mut off = 0;
        while off < data.len() {
            match stream.write(&data[off..]) {
                Ok(0) => break,
                Ok(n) => off += n,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => {
                    return if off == 0 { -1 } else { off as isize };
                }
            }
        }
        off as isize
    }
}

impl Default for Vio {
    fn default() -> Self {
        Vio::new()
    }
}

fn set_keepalive(s: &TcpStream) {
    use std::os::unix::io::AsRawFd;
    let flag: libc::c_int = 1;
    // SAFETY: setsockopt on a valid, owned socket with a correctly sized value.
    unsafe {
        libc::setsockopt(
            s.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_KEEPALIVE,
            &flag as *const libc::c_int as *const libc::c_void,
            std::mem::size_of::<libc::c_int>() as libc::socklen_t,
        );
    }
}

/// strerror() text of an I/O error ("Connection refused", ...).
pub fn strerror(e: &std::io::Error) -> String {
    let s = e.to_string();
    // std formats "Connection refused (os error 111)"
    match s.rfind(" (os error ") {
        Some(p) => s[..p].to_string(),
        None => s,
    }
}

/// php_network_connect_socket_to_host: resolve and try each address.
fn connect_tcp(hostport: &str, timeout: Duration) -> Result<TcpStream, String> {
    // "[::1]:3306" / "host:port"
    let (host, port) = match hostport.rfind(':') {
        Some(p) => (&hostport[..p], &hostport[p + 1..]),
        None => (hostport, "3306"),
    };
    let host = host.trim_start_matches('[').trim_end_matches(']');
    let port: u16 = port.parse().unwrap_or(0);
    let addrs: Vec<std::net::SocketAddr> = match (host, port).to_socket_addrs() {
        Ok(a) => a.collect(),
        Err(e) => {
            let s = e.to_string();
            let reason = s
                .strip_prefix("failed to lookup address information: ")
                .unwrap_or(&s)
                .to_string();
            let msg = format!(
                "php_network_getaddresses: getaddrinfo for {} failed: {}",
                host, reason
            );
            php_error_docref(E_WARNING, msg.clone());
            return Err(msg);
        }
    };
    if addrs.is_empty() {
        let msg = format!(
            "php_network_getaddresses: getaddrinfo for {} failed: Name or service not known",
            host
        );
        php_error_docref(E_WARNING, msg.clone());
        return Err(msg);
    }
    let mut last = String::from("Unknown error while connecting");
    for a in addrs {
        match TcpStream::connect_timeout(&a, timeout) {
            Ok(s) => return Ok(s),
            Err(e) => {
                last = if e.kind() == std::io::ErrorKind::TimedOut {
                    "Connection timed out".to_string()
                } else {
                    strerror(&e)
                };
            }
        }
    }
    Err(last)
}

// ── Protocol frame codec ──────────────────────────────────────────────

pub const MYSQLND_PROTOCOL_FLAG_USE_COMPRESSION: u32 = 1;

/// MYSQLND_PFC
pub struct Pfc {
    pub packet_no: u8,
    pub compressed_envelope_packet_no: u8,
    pub compressed: bool,
    pub ssl: bool,
    pub flags: u32,
    pub cmd_buffer_length: usize,
    pub sha256_server_public_key: Option<String>,
    uncompressed: Vec<u8>,
    upos: usize,
}

impl Pfc {
    /// mysqlnd_pfc::init
    pub fn new() -> Pfc {
        Pfc {
            packet_no: 0,
            compressed_envelope_packet_no: 0,
            compressed: false,
            ssl: false,
            flags: 0,
            cmd_buffer_length: globals()
                .net_cmd_buffer_size
                .max(MYSQLND_NET_CMD_BUFFER_MIN_SIZE),
            sha256_server_public_key: None,
            uncompressed: Vec::new(),
            upos: 0,
        }
    }

    /// mysqlnd_pfc::reset
    pub fn reset(&mut self) {
        self.packet_no = 0;
        self.compressed_envelope_packet_no = 0;
    }

    /// mysqlnd_pfc::free_contents
    pub fn free_contents(&mut self) {
        self.uncompressed.clear();
        self.upos = 0;
        self.sha256_server_public_key = None;
    }

    fn write_compressed_packet(&mut self, vio: &mut Vio, uncompressed_payload: &[u8]) -> isize {
        let mut out = Vec::with_capacity(uncompressed_payload.len() + 7);
        let compressed = zlib_compress(uncompressed_payload);
        // compress() into a buffer of the input's size fails when the data
        // does not shrink: the packet is then sent uncompressed.
        let (payload, orig_len) = match compressed {
            Some(c) if c.len() <= uncompressed_payload.len() => {
                (c, uncompressed_payload.len() as u32)
            }
            _ => (uncompressed_payload.to_vec(), 0),
        };
        int3store(&mut out, payload.len() as u32);
        out.push(self.compressed_envelope_packet_no);
        int3store(&mut out, orig_len);
        out.extend_from_slice(&payload);
        let sent = vio.network_write(&out);
        self.compressed_envelope_packet_no = self.compressed_envelope_packet_no.wrapping_add(1);
        sent
    }

    /// mysqlnd_pfc::send: `payload` is the packet body (no header).
    /// Returns the number of bytes sent by the last write, 0 on error.
    pub fn send(
        &mut self,
        vio: &mut Vio,
        payload: &[u8],
        stats: &mut Stats,
        error_info: &mut ErrorInfo,
    ) -> usize {
        let count = payload.len();
        let mut packets_sent: u64 = 1;
        let mut left = count;
        let mut off = 0;
        let mut bytes_sent: isize;
        loop {
            let to_be_sent = left.min(MYSQLND_MAX_PACKET_SIZE);
            let mut packet = Vec::with_capacity(to_be_sent + MYSQLND_HEADER_SIZE);
            int3store(&mut packet, to_be_sent as u32);
            packet.push(self.packet_no);
            packet.extend_from_slice(&payload[off..off + to_be_sent]);
            if self.compressed {
                if to_be_sent <= MYSQLND_MAX_PACKET_SIZE - MYSQLND_HEADER_SIZE {
                    bytes_sent = self.write_compressed_packet(vio, &packet);
                } else {
                    let split = 8192;
                    self.write_compressed_packet(vio, &packet[..split]);
                    bytes_sent = self.write_compressed_packet(vio, &packet[split..]);
                }
            } else {
                bytes_sent = vio.network_write(&packet);
                self.compressed_envelope_packet_no =
                    self.compressed_envelope_packet_no.wrapping_add(1);
            }
            self.packet_no = self.packet_no.wrapping_add(1);
            off += to_be_sent;
            left -= to_be_sent;
            packets_sent += 1;
            if !(bytes_sent > 0 && (left > 0 || to_be_sent == MYSQLND_MAX_PACKET_SIZE)) {
                break;
            }
        }
        let hs = MYSQLND_HEADER_SIZE as u64;
        stats.inc3(
            STAT_BYTES_SENT,
            count as u64 + packets_sent * hs,
            STAT_PROTOCOL_OVERHEAD_OUT,
            packets_sent * hs,
            STAT_PACKETS_SENT,
            packets_sent,
        );
        if bytes_sent <= 0 {
            error_info.set_str(CR_SERVER_GONE_ERROR, UNKNOWN_SQLSTATE, MYSQLND_SERVER_GONE);
            return 0;
        }
        bytes_sent as usize
    }

    /// mysqlnd_pfc::receive: reads exactly `buf.len()` payload bytes.
    pub fn receive(&mut self, vio: &mut Vio, buf: &mut [u8], stats: &mut Stats) -> bool {
        if !self.compressed {
            return vio.network_read(buf, stats);
        }
        let mut done = 0;
        while done < buf.len() {
            let avail = self.uncompressed.len() - self.upos;
            if avail > 0 {
                let n = avail.min(buf.len() - done);
                buf[done..done + n].copy_from_slice(&self.uncompressed[self.upos..self.upos + n]);
                self.upos += n;
                done += n;
                if self.upos >= self.uncompressed.len() {
                    self.uncompressed.clear();
                    self.upos = 0;
                }
                continue;
            }
            let mut net_header = [0u8; MYSQLND_HEADER_SIZE];
            if !vio.network_read(&mut net_header, stats) {
                return false;
            }
            let net_payload_size = uint3korr(&net_header) as usize;
            let packet_no = net_header[3];
            if self.compressed_envelope_packet_no != packet_no {
                php_error(
                    E_WARNING,
                    format!(
                        "Packets out of order. Expected {} received {}. Packet size={}",
                        self.compressed_envelope_packet_no, packet_no, net_payload_size
                    ),
                );
                return false;
            }
            self.compressed_envelope_packet_no = self.compressed_envelope_packet_no.wrapping_add(1);
            let mut comp_header = [0u8; COMPRESSED_HEADER_SIZE];
            if !vio.network_read(&mut comp_header, stats) {
                return false;
            }
            let decompressed_size = uint3korr(&comp_header) as usize;
            let mut data = vec![0u8; net_payload_size];
            if !vio.network_read(&mut data, stats) {
                return false;
            }
            if decompressed_size > 0 {
                match zlib_uncompress(&data, decompressed_size) {
                    Some(d) => self.uncompressed = d,
                    None => return false,
                }
            } else {
                self.uncompressed = data;
            }
            self.upos = 0;
            if self.uncompressed.is_empty() {
                // nothing more to read from this envelope
                continue;
            }
        }
        true
    }
}

impl Default for Pfc {
    fn default() -> Self {
        Pfc::new()
    }
}

fn zlib_compress(data: &[u8]) -> Option<Vec<u8>> {
    use flate2::Compression;
    use flate2::write::ZlibEncoder;
    let mut e = ZlibEncoder::new(Vec::new(), Compression::default());
    e.write_all(data).ok()?;
    e.finish().ok()
}

fn zlib_uncompress(data: &[u8], size: usize) -> Option<Vec<u8>> {
    use flate2::read::ZlibDecoder;
    let mut d = ZlibDecoder::new(data);
    let mut out = Vec::with_capacity(size);
    d.read_to_end(&mut out).ok()?;
    if out.len() != size {
        return None;
    }
    Some(out)
}
