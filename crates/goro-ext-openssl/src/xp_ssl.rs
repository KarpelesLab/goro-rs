//! TLS for socket streams: ext/openssl/xp_ssl.c on top of purecrypto's
//! sans-I/O TLS engine. The socket layer (goro-ext-standard) owns the
//! descriptor and calls in through `goro_core::crypto::CryptoStream`.
//!
//! Peer certificate verification follows OpenSSL + xp_ssl.c: the client
//! verifies the server chain itself (verify_callback semantics:
//! allow_self_signed, verify_depth, security_level) as soon as the
//! Certificate message has been processed, before its Finished leaves,
//! and then applies php_openssl_apply_peer_verification_policy() (peer
//! fingerprint, peer name) once the handshake completed. A server asks
//! for a client certificate whenever verify_peer is not disabled and the
//! engine checks it against the cafile.

use crate::asn1;
use crate::cert::Cert;
use crate::digest::Md;
use crate::pkey::{self, PKey};
use crate::x509::{self, Store};
use goro_core::array::{ArrayKey, PhpArray};
use goro_core::crypto::{CryptoEnable, CryptoHooks, CryptoSetup, CryptoStream};
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::Vm;
use purecrypto::tls::{
    ClientAuth, Config, ConfigBuilder, Connection, ProtocolVersion, RootCertStore, SigningKey, Step,
};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

const METHOD_SSLV3: i64 = 1 << 2;
const METHOD_TLSV1_0: i64 = 1 << 3;
const METHOD_TLSV1_1: i64 = 1 << 4;
const METHOD_TLSV1_2: i64 = 1 << 5;
const METHOD_TLSV1_3: i64 = 1 << 6;
const MIN_PROTO: i64 = METHOD_TLSV1_0;
const MAX_PROTO: i64 = METHOD_TLSV1_3;

/// OPENSSL_DEFAULT_STREAM_VERIFY_DEPTH
const DEFAULT_VERIFY_DEPTH: i64 = 9;
/// The default OpenSSL security level (OPENSSL_TLS_SECURITY_LEVEL).
const DEFAULT_SECURITY_LEVEL: i64 = 1;

const ERR_VERIFY_FAILED: &str = "error:0A000086:SSL routines::certificate verify failed";

pub fn install(vm: &mut Vm) {
    vm.crypto_hooks = Some(CryptoHooks { setup });
}

// ---------------------------------------------------------------------------
// Context option helpers (GET_VER_OPT & co)
// ---------------------------------------------------------------------------

fn opt<'a>(opts: Option<&'a PhpArray>, name: &str) -> Option<&'a Value> {
    opts?.get(&ArrayKey::String(PhpString::from_bytes(name.as_bytes())))
}

fn opt_true(opts: Option<&PhpArray>, name: &str) -> Option<bool> {
    opt(opts, name).map(|v| v.deref().is_truthy())
}

fn opt_long(opts: Option<&PhpArray>, name: &str) -> Option<i64> {
    opt(opts, name).map(|v| v.deref().to_long())
}

fn opt_bytes(opts: Option<&PhpArray>, name: &str) -> Option<Vec<u8>> {
    opt(opts, name).map(|v| v.deref().to_php_string().as_bytes().to_vec())
}

/// A C string view (the option as char*).
fn cstr(b: &[u8]) -> String {
    let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    String::from_utf8_lossy(&b[..end]).into_owned()
}

fn sval(s: &str) -> Value {
    Value::String(PhpString::from_bytes(s.as_bytes()))
}

fn akey(s: &str) -> ArrayKey {
    ArrayKey::String(PhpString::from_bytes(s.as_bytes()))
}

// ---------------------------------------------------------------------------
// Protocol versions
// ---------------------------------------------------------------------------

/// php_openssl_get_proto_version_flags()
fn proto_version_flags(flags: i64, min: i64, max: i64) -> i64 {
    let mut flags = flags;
    if min != 0 || max != 0 {
        let min = if min == 0 { MIN_PROTO } else { min };
        let max = if max == 0 { MAX_PROTO } else { max };
        flags = 0;
        let mut ver = min;
        while ver <= max {
            flags |= ver;
            ver <<= 1;
        }
    }
    flags
}

fn min_proto_flag(flags: i64) -> i64 {
    let mut ver = METHOD_SSLV3;
    while ver <= MAX_PROTO {
        if flags & ver != 0 {
            return ver;
        }
        ver <<= 1;
    }
    MAX_PROTO
}

fn max_proto_flag(flags: i64) -> i64 {
    let mut ver = MAX_PROTO;
    while ver >= METHOD_SSLV3 {
        if flags & ver != 0 {
            return ver;
        }
        ver >>= 1;
    }
    METHOD_TLSV1_3
}

fn map_proto(flag: i64) -> ProtocolVersion {
    match flag {
        METHOD_TLSV1_3 => ProtocolVersion::TLSv1_3,
        METHOD_TLSV1_2 => ProtocolVersion::TLSv1_2,
        METHOD_TLSV1_1 => ProtocolVersion::TLSv1_1,
        METHOD_TLSV1_0 => ProtocolVersion::TLSv1_0,
        METHOD_SSLV3 => ProtocolVersion::SSLv3,
        _ => ProtocolVersion::TLSv1_2,
    }
}

fn version_rank(v: ProtocolVersion) -> u8 {
    match v {
        ProtocolVersion::SSLv3 => 1,
        ProtocolVersion::TLSv1_0 => 2,
        ProtocolVersion::TLSv1_1 => 3,
        ProtocolVersion::TLSv1_2 => 4,
        ProtocolVersion::TLSv1_3 => 5,
        _ => 0,
    }
}

// ---------------------------------------------------------------------------
// Certificates and keys
// ---------------------------------------------------------------------------

/// Strip a file:// prefix (php_openssl_check_path_ex()).
fn strip_file_scheme(p: &[u8]) -> &[u8] {
    if p.len() >= 7 && p[..7].eq_ignore_ascii_case(b"file://") {
        &p[7..]
    } else {
        p
    }
}

/// php_openssl_check_path_ex() for a context option.
fn check_path(vm: &mut Vm, func: &str, path: &[u8], what: &str) -> Option<String> {
    let p = strip_file_scheme(path);
    if p.is_empty() {
        return None;
    }
    x509::check_plain_path(vm, func, 0, p, false, Some(what))
        .ok()
        .flatten()
}

/// The certificates of a PEM file, leaf first (SSL_CTX_use_certificate_chain_file()).
fn read_cert_chain(path: &str) -> Option<Vec<Rc<Cert>>> {
    let certs = x509::load_pem_certs(path)?;
    if certs.is_empty() { None } else { Some(certs) }
}

/// SSL_CTX_use_PrivateKey_file(PEM) with the passphrase callback.
fn read_private_key(path: &str, pass: &[u8]) -> Option<PKey> {
    let data = std::fs::read(path).ok()?;
    pkey::read_private_pem(&data, pass).ok()
}

fn signing_key(k: &PKey) -> Option<SigningKey> {
    use purecrypto::x509::{AnyPrivateKey, Pkcs8ReadOptions};
    if !k.has_private() {
        return None;
    }
    let der = k.pkcs8_der();
    match AnyPrivateKey::from_pkcs8_der(&der, Pkcs8ReadOptions::new()).ok()? {
        AnyPrivateKey::Rsa(r) => Some(SigningKey::Rsa(r)),
        AnyPrivateKey::Ecdsa(e) => Some(SigningKey::Ecdsa(e)),
        AnyPrivateKey::Ed25519(e) => Some(SigningKey::Ed25519(e)),
        AnyPrivateKey::Ed448(e) => Some(SigningKey::Ed448(e)),
        _ => None,
    }
}

/// The security strength of a public key in bits (EVP_PKEY_security_bits()).
fn security_bits(k: &PKey) -> i64 {
    match k {
        PKey::Rsa(_) | PKey::Dsa(_) | PKey::Dh(_) => {
            let b = k.bits();
            match b {
                b if b >= 15360 => 256,
                b if b >= 7680 => 192,
                b if b >= 3072 => 128,
                b if b >= 2048 => 112,
                b if b >= 1024 => 80,
                _ => 0,
            }
        }
        PKey::Ec(_) => k.bits() / 2,
        PKey::Ecx(_) => {
            if k.bits() >= 448 {
                224
            } else {
                128
            }
        }
    }
}

/// The minimum security bits of a security level.
fn level_bits(level: i64) -> i64 {
    match level {
        i64::MIN..=0 => 0,
        1 => 80,
        2 => 112,
        3 => 128,
        4 => 192,
        _ => 256,
    }
}

// ---------------------------------------------------------------------------
// Peer name matching (php_openssl_matches_*)
// ---------------------------------------------------------------------------

fn eq_ci(a: &[u8], b: &[u8]) -> bool {
    a.eq_ignore_ascii_case(b)
}

/// php_openssl_matches_wildcard_name()
fn matches_wildcard_name(subject: &[u8], certname: &[u8]) -> bool {
    if eq_ci(subject, certname) {
        return true;
    }
    let Some(w) = certname.iter().position(|&c| c == b'*') else {
        return false;
    };
    if certname[..w].contains(&b'.') {
        return false;
    }
    let prefix_len = w;
    if prefix_len > 0
        && (subject.len() < prefix_len || !eq_ci(&subject[..prefix_len], &certname[..prefix_len]))
    {
        return false;
    }
    let suffix = &certname[w + 1..];
    let subject_len = subject.len();
    if suffix.len() <= subject_len {
        if subject_len < suffix.len() + prefix_len {
            return false;
        }
        return eq_ci(suffix, &subject[subject_len - suffix.len()..])
            && !subject[prefix_len..subject_len - suffix.len()].contains(&b'.');
    }
    false
}

fn expand_ipv6(b: &[u8]) -> String {
    let mut parts = Vec::new();
    for i in 0..8 {
        parts.push(format!(
            "{:x}",
            ((b[2 * i] as u16) << 8) | b[2 * i + 1] as u16
        ));
    }
    parts.join(":")
}

/// php_openssl_matches_san_list()
fn matches_san_list(peer: &Cert, subject: &[u8]) -> bool {
    let Some(ext) = peer.ext("2.5.29.17") else {
        return false;
    };
    let Some(names) = asn1::parse_one(&ext.value).and_then(|t| t.children()) else {
        return false;
    };
    let subject_v6 = std::str::from_utf8(subject)
        .ok()
        .and_then(|s| s.parse::<std::net::Ipv6Addr>().ok())
        .map(|a| expand_ipv6(&a.octets()));
    for g in names {
        match g.tag & 0x1f {
            2 => {
                let mut name = g.content.to_vec();
                if name.contains(&0) {
                    continue;
                }
                if name.last() == Some(&b'.') {
                    name.pop();
                }
                if matches_wildcard_name(subject, &name) {
                    return true;
                }
            }
            7 => {
                if g.content.len() == 4 {
                    let ip = format!(
                        "{}.{}.{}.{}",
                        g.content[0], g.content[1], g.content[2], g.content[3]
                    );
                    if eq_ci(subject, ip.as_bytes()) {
                        return true;
                    }
                } else if g.content.len() == 16 {
                    if let Some(sv6) = &subject_v6 {
                        if sv6.eq_ignore_ascii_case(&expand_ipv6(g.content)) {
                            return true;
                        }
                    }
                }
            }
            _ => {}
        }
    }
    false
}

/// php_openssl_matches_common_name()
fn matches_common_name(vm: &mut Vm, f: &str, peer: &Cert, subject: &[u8]) -> bool {
    let cn = peer
        .subject
        .entries()
        .find(|a| a.oid == "2.5.4.3")
        .map(|a| a.value.clone());
    match cn {
        None => {
            vm.emit_warning(&format!("{}(): Unable to locate peer certificate CN", f));
            false
        }
        Some(mut buf) => {
            buf.truncate(1023);
            let shown = String::from_utf8_lossy(&buf).into_owned();
            if buf.contains(&0) {
                vm.emit_warning(&format!(
                    "{}(): Peer certificate CN=`{}' is malformed",
                    f, shown
                ));
                false
            } else if matches_wildcard_name(subject, &buf) {
                true
            } else {
                vm.emit_warning(&format!(
                    "{}(): Peer certificate CN=`{}' did not match expected CN=`{}'",
                    f,
                    shown,
                    String::from_utf8_lossy(subject)
                ));
                false
            }
        }
    }
}

fn fingerprint_cmp(peer: &Cert, method: &str, expected: &[u8]) -> bool {
    let Some(md) = Md::by_name(method) else {
        return false;
    };
    let fp = crate::hex_lower(&md.digest(&peer.der));
    eq_ci(expected, fp.as_bytes())
}

/// php_openssl_x509_fingerprint_match()
fn fingerprint_match(vm: &mut Vm, f: &str, peer: &Cert, val: &Value) -> bool {
    match val.deref() {
        Value::String(s) => {
            let method = match s.len() {
                32 => "md5",
                40 => "sha1",
                _ => return false,
            };
            fingerprint_cmp(peer, method, s.as_bytes())
        }
        Value::Array(a) => {
            let a = a.borrow();
            if a.is_empty() {
                vm.emit_warning(&format!(
                    "{}(): Invalid peer_fingerprint array; [algo => fingerprint] form required",
                    f
                ));
                return false;
            }
            for (k, v) in a.iter() {
                let (ArrayKey::String(k), Value::String(v)) = (k, v.deref()) else {
                    vm.emit_warning(&format!(
                        "{}(): Invalid peer_fingerprint array; [algo => fingerprint] form required",
                        f
                    ));
                    return false;
                };
                if !fingerprint_cmp(peer, &k.to_string_lossy(), v.as_bytes()) {
                    return false;
                }
            }
            true
        }
        _ => {
            vm.emit_warning(&format!(
                "{}(): Invalid peer_fingerprint value; fingerprint string or array of the form [algo => fingerprint] required",
                f
            ));
            false
        }
    }
}

// ---------------------------------------------------------------------------
// Raw socket I/O
// ---------------------------------------------------------------------------

enum Recv {
    Data(usize),
    Eof,
    WouldBlock,
    Err(i32),
}

fn errno() -> i32 {
    std::io::Error::last_os_error().raw_os_error().unwrap_or(0)
}

fn strerror(e: i32) -> String {
    let s = unsafe { std::ffi::CStr::from_ptr(libc::strerror(e)) };
    s.to_string_lossy().into_owned()
}

fn sock_recv(fd: i32, buf: &mut [u8]) -> Recv {
    loop {
        let n = unsafe {
            libc::recv(
                fd,
                buf.as_mut_ptr() as *mut libc::c_void,
                buf.len(),
                libc::MSG_DONTWAIT,
            )
        };
        if n > 0 {
            return Recv::Data(n as usize);
        }
        if n == 0 {
            return Recv::Eof;
        }
        let e = errno();
        if e == libc::EINTR {
            continue;
        }
        if e == libc::EAGAIN || e == libc::EWOULDBLOCK {
            return Recv::WouldBlock;
        }
        return Recv::Err(e);
    }
}

/// Some(n) bytes sent (0: would block), None on error (errno in .1).
fn sock_send(fd: i32, data: &[u8]) -> Result<usize, i32> {
    loop {
        let n = unsafe {
            libc::send(
                fd,
                data.as_ptr() as *const libc::c_void,
                data.len(),
                libc::MSG_DONTWAIT | libc::MSG_NOSIGNAL,
            )
        };
        if n >= 0 {
            return Ok(n as usize);
        }
        let e = errno();
        if e == libc::EINTR {
            continue;
        }
        if e == libc::EAGAIN || e == libc::EWOULDBLOCK {
            return Ok(0);
        }
        return Err(e);
    }
}

fn poll_fd(fd: i32, events: i16, ms: i32) -> i32 {
    let mut p = libc::pollfd {
        fd,
        events,
        revents: 0,
    };
    loop {
        let r = unsafe { libc::poll(&mut p, 1, ms) };
        if r < 0 && errno() == libc::EINTR {
            continue;
        }
        return r;
    }
}

/// Milliseconds left of `timeout_ms` since `start` (None: no limit;
/// Some(0): expired).
fn time_left(start: Instant, timeout_ms: i32) -> Option<i32> {
    if timeout_ms <= 0 {
        return None;
    }
    let el = start.elapsed().as_millis() as i64;
    Some((timeout_ms as i64 - el).max(0) as i32)
}

fn expired(start: Instant, timeout_ms: i32) -> bool {
    timeout_ms > 0 && start.elapsed().as_millis() as i64 > timeout_ms as i64
}

// ---------------------------------------------------------------------------
// Error reporting (php_openssl_handle_ssl_error)
// ---------------------------------------------------------------------------

fn alert_reason(a: &purecrypto::tls::AlertDescription) -> (u32, &'static str) {
    use purecrypto::tls::AlertDescription as A;
    match a {
        A::UnexpectedMessage => (0x0A00_03F2, "sslv3 alert unexpected message"),
        A::BadRecordMac => (0x0A00_03FC, "sslv3 alert bad record mac"),
        A::HandshakeFailure => (0x0A00_0410, "sslv3 alert handshake failure"),
        A::BadCertificate => (0x0A00_0412, "sslv3 alert bad certificate"),
        A::UnsupportedCertificate => (0x0A00_0413, "sslv3 alert unsupported certificate"),
        A::CertificateExpired => (0x0A00_0415, "sslv3 alert certificate expired"),
        A::CertificateUnknown => (0x0A00_0416, "sslv3 alert certificate unknown"),
        A::IllegalParameter => (0x0A00_042F, "sslv3 alert illegal parameter"),
        A::DecodeError => (0x0A00_0432, "tlsv1 alert decode error"),
        A::DecryptError => (0x0A00_0433, "tlsv1 alert decrypt error"),
        A::ProtocolVersion => (0x0A00_042E, "tlsv1 alert protocol version"),
        A::InternalError => (0x0A00_0438, "tlsv1 alert internal error"),
        A::UnrecognizedName => (0x0A00_0458, "tlsv1 unrecognized name"),
        A::NoApplicationProtocol => (0x0A00_0460, "tlsv1 alert no application protocol"),
        A::CertificateRequired => (0x0A00_045C, "tlsv13 alert certificate required"),
        A::Unknown(48) => (0x0A00_0418, "tlsv1 alert unknown ca"),
        _ => (0x0A00_0438, "tlsv1 alert internal error"),
    }
}

/// The OpenSSL error queue line for an engine error.
fn error_line(e: &purecrypto::tls::Error, is_client: bool) -> String {
    use purecrypto::tls::Error as E;
    let (code, reason): (u32, &str) = match e {
        E::AlertReceived(a) => alert_reason(a),
        E::UnsupportedVersion => (0x0A00_0102, "unsupported protocol"),
        E::BadCertificate | E::CertificateRevoked => (0x0A00_0086, "certificate verify failed"),
        E::CertificateRequired => (0x0A00_00C7, "peer did not return a certificate"),
        E::BadRecordMac => (0x0A00_0119, "decryption failed or bad record mac"),
        E::Decode => (0x0A00_009F, "packet length too long"),
        E::UnexpectedMessage => (0x0A00_00F4, "unexpected message"),
        E::HandshakeFailure if !is_client => (0x0A00_00C1, "no shared cipher"),
        E::HandshakeFailure => (0x0A00_0410, "sslv3 alert handshake failure"),
        E::NoUsableCipherSuites => (0x0A00_00B5, "no ciphers available"),
        E::InappropriateFallback => (0x0A00_0175, "inappropriate fallback"),
        E::NoApplicationProtocol => (0x0A00_00EB, "no application protocol"),
        _ => (0x0A00_0139, "record layer failure"),
    };
    format!("error:{:08X}:SSL routines::{}", code, reason)
}

fn ssl_failed_warning(lines: &[String]) -> String {
    if lines.is_empty() {
        "SSL operation failed with code 1. ".to_string()
    } else {
        format!(
            "SSL operation failed with code 1. OpenSSL Error messages:\n{}",
            lines.join("\n")
        )
    }
}

// ---------------------------------------------------------------------------
// Setup (php_openssl_setup_crypto)
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct SniCert {
    name: Vec<u8>,
    config: Config,
}

pub struct SslStream {
    is_client: bool,
    conn: Option<Connection>,
    /// Client: the config the connection is built from at enable time.
    /// Server: the default context (SNI may pick another).
    config: Option<Config>,
    sni: Vec<SniCert>,
    has_identity: bool,
    /// Server: bytes read before the ClientHello was complete.
    hello: Vec<u8>,
    /// Client trust store (SSL_VERIFY_PEER on the client).
    store: Option<Store>,
    verify_peer: bool,
    verify_depth: i64,
    allow_self_signed: bool,
    security_level: i64,
    verify_result: i32,
    chain_checked: bool,
    verify_failed: bool,
    url_name: Option<Vec<u8>>,
    active: bool,
    /// TLS records waiting for the socket.
    out: Vec<u8>,
    /// Decrypted application data not returned yet.
    plain: Vec<u8>,
    eof: bool,
    warnings: Vec<String>,
    /// The PHP function warnings are reported for.
    func: String,
    /// Server: renegotiation rate limiting (sslsock->reneg).
    reneg: Option<RenegLimit>,
    /// Record boundaries of what the peer sent.
    records: RecordScan,
    /// reneg_limit_callback calls due.
    callbacks: Vec<Value>,
}

/// php_openssl_handshake_bucket_t
struct RenegLimit {
    prev_handshake: i64,
    limit: i64,
    window: i64,
    tokens: i64,
    callback: Option<Value>,
}

impl RenegLimit {
    /// php_openssl_init_server_reneg_limit(): None without rate limiting.
    fn from_opts(opts: Option<&PhpArray>) -> Option<RenegLimit> {
        // OPENSSL_DEFAULT_RENEG_LIMIT / OPENSSL_DEFAULT_RENEG_WINDOW
        let limit = opt_long(opts, "reneg_limit").unwrap_or(2);
        if limit < 0 {
            return None;
        }
        let window = opt_long(opts, "reneg_window").unwrap_or(300);
        Some(RenegLimit {
            prev_handshake: 0,
            limit,
            window,
            tokens: 0,
            callback: opt(opts, "reneg_limit_callback").map(|v| v.deref()),
        })
    }

    /// php_openssl_limit_handshake_reneg() at a handshake start: true when
    /// the stream must close (after the callback, if any).
    fn handshake_start(&mut self) -> bool {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        // The initial handshake is never rate-limited.
        if self.prev_handshake == 0 {
            self.prev_handshake = now;
            return false;
        }
        let elapsed = now - self.prev_handshake;
        self.prev_handshake = now;
        let rate = if self.window != 0 {
            self.limit / self.window
        } else {
            0
        };
        self.tokens = (self.tokens - elapsed * rate).max(0) + 1;
        self.tokens > self.limit
    }
}

/// Tracks the TLS record headers in the bytes the peer sends, to see a
/// renegotiation (a handshake record after the handshake) start.
#[derive(Default)]
struct RecordScan {
    hdr: Vec<u8>,
    left: usize,
}

impl RecordScan {
    /// Whether a handshake record starts in `data`.
    fn scan(&mut self, data: &[u8]) -> bool {
        let mut saw = false;
        let mut i = 0;
        while i < data.len() {
            if self.left > 0 {
                let k = self.left.min(data.len() - i);
                self.left -= k;
                i += k;
                continue;
            }
            self.hdr.push(data[i]);
            i += 1;
            if self.hdr.len() == 5 {
                if self.hdr[0] == 22 {
                    saw = true;
                }
                self.left = u16::from_be_bytes([self.hdr[3], self.hdr[4]]) as usize;
                self.hdr.clear();
            }
        }
        saw
    }
}

fn setup(vm: &mut Vm, p: &CryptoSetup<'_>) -> Option<Box<dyn CryptoStream>> {
    let opts = p.opts;
    let f = p.func.to_string();
    let is_client = p.is_client;
    let mut method_flags = p.method;
    let min_v = opt_long(opts, "min_proto_version").unwrap_or(0);
    let max_v = opt_long(opts, "max_proto_version").unwrap_or(0);
    method_flags = proto_version_flags(method_flags, min_v, max_v);

    // verify_peer: SSL_VERIFY_PEER unless explicitly disabled
    let verify_peer = opt_true(opts, "verify_peer") != Some(false);
    let mut store = None;
    let mut client_ca: Option<RootCertStore> = None;
    if verify_peer {
        let cafile = opt_bytes(opts, "cafile")
            .map(|b| cstr(&b))
            .or_else(|| ini_string(vm, "openssl.cafile"));
        let capath = opt_bytes(opts, "capath")
            .map(|b| cstr(&b))
            .or_else(|| ini_string(vm, "openssl.capath"));
        let mut st = Store {
            certs: Vec::new(),
            dirs: Vec::new(),
        };
        if cafile.is_some() || capath.is_some() {
            if let Some(cf) = &cafile {
                match load_cafile(vm, &f, cf) {
                    Some(c) => st.certs.extend(c),
                    None => {
                        capath.as_ref()?;
                    }
                }
            }
            if let Some(cp) = capath {
                st.dirs.push(cp);
            }
        } else if is_client {
            // SSL_CTX_set_default_verify_paths()
            if let Some(c) = x509::load_pem_certs(&x509::default_cert_file()) {
                st.certs.extend(c);
            }
            st.dirs.push(x509::default_cert_dir());
        }
        if !is_client {
            let mut roots = RootCertStore::new();
            for c in &st.certs {
                let _ = roots.add_der(c.der.clone());
            }
            client_ca = Some(roots);
        }
        store = Some(st);
    }

    let passphrase = opt_bytes(opts, "passphrase").unwrap_or_default();

    let ciphers = opt_bytes(opts, "ciphers").map(|b| cstr(&b));
    let cipher_suites = match &ciphers {
        Some(list) => parse_cipher_list(list)?,
        None => None,
    };

    let mut security_level = DEFAULT_SECURITY_LEVEL;
    if let Some(l) = opt_long(opts, "security_level") {
        if !(0..=5).contains(&l) {
            vm.emit_warning(&format!("{}(): Security level must be between 0 and 5", f));
        }
        security_level = l;
    }

    let mut alpn: Vec<Vec<u8>> = Vec::new();
    if let Some(a) = opt_bytes(opts, "alpn_protocols") {
        let a = cstr(&a).into_bytes();
        let mut ok = a.len() < 65535;
        if ok {
            for part in a.split(|&c| c == b',') {
                if part.len() > 255 {
                    ok = false;
                    break;
                }
                alpn.push(part.to_vec());
            }
        }
        if !ok {
            vm.emit_warning(&format!(
                "{}(): Failed parsing comma-separated TLS ALPN protocol string",
                f
            ));
            return None;
        }
    }

    // local_cert / local_pk (php_openssl_set_local_cert)
    let mut identity: Option<(Vec<Vec<u8>>, SigningKey)> = None;
    if let Some(certfile) = opt_bytes(opts, "local_cert") {
        let Some(cert_path) = check_path(vm, &f, &certfile, "local_cert in ssl stream context")
        else {
            vm.emit_warning(&format!(
                "{}(): Unable to get real path of certificate file `{}'",
                f,
                cstr(&certfile)
            ));
            return None;
        };
        let Some(chain) = read_cert_chain(&cert_path) else {
            vm.emit_warning(&format!(
                "{}(): Unable to set local cert chain file `{}'; Check that your cafile/capath settings include details of your certificate and its issuer",
                f,
                cstr(&certfile)
            ));
            return None;
        };
        let mut key_path = cert_path.clone();
        if let Some(pk) = opt_bytes(opts, "local_pk") {
            match check_path(vm, &f, &pk, "local_pk in ssl stream context") {
                Some(p) => key_path = p,
                None => {
                    vm.emit_warning(&format!(
                        "{}(): Unable to get real path of private key file `{}'",
                        f,
                        cstr(&pk)
                    ));
                    return None;
                }
            }
        }
        let key = read_private_key(&key_path, &passphrase);
        let Some(sk) = key.as_ref().and_then(signing_key) else {
            vm.emit_warning(&format!(
                "{}(): Unable to set private key file `{}'",
                f, key_path
            ));
            return None;
        };
        let ders: Vec<Vec<u8>> = chain.iter().map(|c| c.der.clone()).collect();
        let id = purecrypto::tls::Identity::new(ders.clone(), sk.clone());
        if id.check_key_matches_leaf().is_err() {
            vm.emit_warning(&format!("{}(): Private key does not match certificate!", f));
        }
        identity = Some((ders, sk));
    }

    // versions
    let mut min = map_proto(min_proto_flag(method_flags));
    let max = map_proto(max_proto_flag(method_flags));
    if security_level >= 1 && version_rank(min) < version_rank(ProtocolVersion::TLSv1_2) {
        // OpenSSL 3 allows SSLv3/TLSv1.0/TLSv1.1 at security level 0 only.
        min = ProtocolVersion::TLSv1_2;
    }

    if !is_client {
        if opt(opts, "rsa_key_size").is_some() {
            vm.emit_warning(&format!(
                "{}(): rsa_key_size context option has been removed",
                f
            ));
        }
        if let Some(dh) = opt_bytes(opts, "dh_param") {
            // DH parameters only matter for the FFDHE suites, which the
            // engine does not implement; validate them like OpenSSL does.
            let path = cstr(&dh);
            match std::fs::read(&path) {
                Err(_) => {
                    vm.emit_warning(&format!("{}(): Invalid dh_param", f));
                    return None;
                }
                Ok(d) => {
                    if !asn1::pem_blocks(&d)
                        .iter()
                        .any(|b| b.label.contains("PARAMETERS"))
                    {
                        vm.emit_warning(&format!("{}(): Failed reading DH params", f));
                        return None;
                    }
                }
            }
        }
    }

    let base = || {
        let mut b: ConfigBuilder = Config::builder()
            .rng(Arc::new(purecrypto::rng::OsRng))
            .verify_certificates(false)
            .require_extended_master_secret(false);
        if !alpn.is_empty() {
            b = b.alpn(alpn.clone());
        }
        if let Some(cs) = &cipher_suites {
            b = b.cipher_suites(cs);
        }
        b
    };
    let build = |b: ConfigBuilder, id: Option<(Vec<Vec<u8>>, SigningKey)>| -> Config {
        let mut b = b;
        if let Some((chain, key)) = id {
            b = b.identity(chain, key);
        }
        if let Some(roots) = &client_ca {
            b = b.client_auth(ClientAuth::new(roots.clone_store(), false));
        }
        let mut c = b.build();
        c.min_version = min;
        c.max_version = max;
        c
    };

    let mut sni = Vec::new();
    let has_identity = identity.is_some();
    let config;
    if is_client {
        let mut b = base();
        // php_openssl_enable_client_sni()
        if opt_true(opts, "SNI_enabled") != Some(false) {
            let name = opt_bytes(opts, "peer_name")
                .map(|b| cstr(&b).into_bytes())
                .or_else(|| p.url_name.map(|u| u.to_vec()));
            if let Some(n) = name {
                if !n.is_empty() {
                    b = b.server_name(String::from_utf8_lossy(&n).into_owned());
                }
            }
        }
        config = build(b, identity);
    } else {
        config = build(base(), identity);
        // php_openssl_enable_server_sni()
        if opt_true(opts, "SNI_enabled") != Some(false) {
            if let Some(v) = opt(opts, "SNI_server_certs") {
                let Value::Array(arr) = v.deref() else {
                    vm.emit_warning(&format!(
                        "{}(): SNI_server_certs requires an array mapping host names to cert paths",
                        f
                    ));
                    return None;
                };
                let entries: Vec<(ArrayKey, Value)> = arr
                    .borrow()
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect();
                if entries.is_empty() {
                    vm.emit_warning(&format!(
                        "{}(): SNI_server_certs host cert array must not be empty",
                        f
                    ));
                    return None;
                }
                for (k, cur) in entries {
                    let ArrayKey::String(host) = k else {
                        vm.emit_warning(&format!(
                            "{}(): SNI_server_certs array requires string host name keys",
                            f
                        ));
                        return None;
                    };
                    let cur = cur.deref();
                    let id = match &cur {
                        Value::Array(a) => {
                            let a = a.borrow().clone();
                            let Some(lc) = opt(Some(&a), "local_cert") else {
                                vm.emit_warning(&format!(
                                    "{}(): local_cert not present in the array",
                                    f
                                ));
                                return None;
                            };
                            let lc = vm.value_to_string(&lc.deref());
                            if vm.current_exception.is_some() {
                                return None;
                            }
                            let Some(cert_path) = check_path(
                                vm,
                                &f,
                                lc.as_bytes(),
                                "SNI_server_certs local_cert in ssl stream context",
                            ) else {
                                vm.emit_warning(&format!(
                                    "{}(): Failed setting local cert chain file `{}'; could not open file",
                                    f,
                                    cstr(lc.as_bytes())
                                ));
                                return None;
                            };
                            let Some(lp) = opt(Some(&a), "local_pk") else {
                                vm.emit_warning(&format!(
                                    "{}(): local_pk not present in the array",
                                    f
                                ));
                                return None;
                            };
                            let lp = vm.value_to_string(&lp.deref());
                            if vm.current_exception.is_some() {
                                return None;
                            }
                            let Some(key_path) = check_path(
                                vm,
                                &f,
                                lp.as_bytes(),
                                "SNI_server_certs local_pk in ssl stream context",
                            ) else {
                                vm.emit_warning(&format!(
                                    "{}(): Failed setting local private key file `{}';  could not open file",
                                    f,
                                    cstr(lp.as_bytes())
                                ));
                                return None;
                            };
                            sni_identity(vm, &f, &cert_path, &key_path)
                        }
                        Value::String(s) => {
                            match check_path(
                                vm,
                                &f,
                                s.as_bytes(),
                                "SNI_server_certs in ssl stream context",
                            ) {
                                Some(path) => sni_identity(vm, &f, &path, &path),
                                None => {
                                    vm.emit_warning(&format!(
                                        "{}(): Failed setting local cert chain file `{}'; file not found",
                                        f,
                                        cstr(s.as_bytes())
                                    ));
                                    None
                                }
                            }
                        }
                        _ => {
                            vm.emit_warning(&format!(
                                "{}(): SNI_server_certs options values must be of type array|string",
                                f
                            ));
                            None
                        }
                    };
                    let id = id?;
                    sni.push(SniCert {
                        name: cstr(host.as_bytes()).into_bytes(),
                        config: build(base(), Some(id)),
                    });
                }
            }
        }
    }

    Some(Box::new(SslStream {
        is_client,
        conn: None,
        config: Some(config),
        sni,
        has_identity,
        hello: Vec::new(),
        store: if is_client { store } else { None },
        verify_peer,
        verify_depth: opt_long(opts, "verify_depth").unwrap_or(DEFAULT_VERIFY_DEPTH),
        allow_self_signed: opt_true(opts, "allow_self_signed").unwrap_or(false),
        security_level,
        verify_result: 0,
        chain_checked: false,
        verify_failed: false,
        url_name: p.url_name.map(|u| u.to_vec()),
        active: false,
        out: Vec::new(),
        plain: Vec::new(),
        eof: false,
        warnings: Vec::new(),
        func: f,
        reneg: if is_client {
            None
        } else {
            RenegLimit::from_opts(opts)
        },
        records: RecordScan::default(),
        callbacks: Vec::new(),
    }))
}

/// php_openssl_create_sni_server_ctx()
fn sni_identity(
    vm: &mut Vm,
    f: &str,
    cert_path: &str,
    key_path: &str,
) -> Option<(Vec<Vec<u8>>, SigningKey)> {
    let Some(chain) = read_cert_chain(cert_path) else {
        vm.emit_warning(&format!(
            "{}(): Failed setting local cert chain file `{}'; check that your cafile/capath settings include details of your certificate and its issuer",
            f, cert_path
        ));
        return None;
    };
    let Some(sk) = read_private_key(key_path, b"")
        .as_ref()
        .and_then(signing_key)
    else {
        vm.emit_warning(&format!(
            "{}(): Failed setting private key from file `{}'",
            f, key_path
        ));
        return None;
    };
    Some((chain.iter().map(|c| c.der.clone()).collect(), sk))
}

fn ini_string(vm: &Vm, name: &str) -> Option<String> {
    let v = vm.constants.get(name.as_bytes())?;
    let s = v.to_php_string().to_string_lossy();
    if s.is_empty() { None } else { Some(s) }
}

/// SSL_CTX_load_verify_locations(cafile) with the php_openssl_load_stream_cafile()
/// fallback; None after a warning when nothing usable was found.
fn load_cafile(vm: &mut Vm, f: &str, cafile: &str) -> Option<Vec<Rc<Cert>>> {
    if let Some(c) = x509::load_pem_certs(cafile) {
        if !c.is_empty() {
            return Some(c);
        }
    }
    // php_openssl_load_stream_cafile(): local stream wrappers only
    let data = if let Some(rest) = cafile.strip_prefix("file://") {
        std::fs::read(rest).ok()
    } else if let Some(pos) = cafile.find("://") {
        let scheme = cafile[..pos].to_ascii_lowercase();
        if matches!(scheme.as_str(), "http" | "https" | "ftp" | "ftps") {
            // stream->wrapper->is_url
            vm.emit_warning(&format!(
                "{}(): remote cafile streams are disabled for security purposes",
                f
            ));
            return None;
        }
        // another (non-url) wrapper, e.g. phar://: opened without
        // REPORT_ERRORS
        match vm.functions.get(b"file_get_contents".as_slice()).copied() {
            Some(fgc) => {
                let saved = vm.warning_capture.replace(Vec::new());
                let r = fgc(
                    vm,
                    &[Value::String(PhpString::from_bytes(cafile.as_bytes()))],
                );
                vm.warning_capture = saved;
                match r {
                    Ok(Value::String(s)) => Some(s.as_bytes().to_vec()),
                    _ => None,
                }
            }
            None => None,
        }
    } else {
        std::fs::read(cafile).ok()
    };
    let Some(data) = data else {
        vm.emit_warning(&format!(
            "{}(): failed loading cafile stream: `{}'",
            f, cafile
        ));
        return None;
    };
    let certs: Vec<Rc<Cert>> = asn1::pem_blocks(&data)
        .iter()
        .filter(|b| b.label == "CERTIFICATE")
        .filter_map(|b| Cert::parse(&b.der).map(Rc::new))
        .collect();
    if certs.is_empty() {
        vm.emit_warning(&format!(
            "{}(): no valid certs found cafile stream: `{}'",
            f, cafile
        ));
        return None;
    }
    Some(certs)
}

/// SSL_CTX_set_cipher_list(): the IANA suites an OpenSSL cipher string
/// selects. Some(None): the engine's default set; None: nothing matched.
fn parse_cipher_list(list: &str) -> Option<Option<Vec<u16>>> {
    const NAMED: &[(&str, u16)] = &[
        ("ECDHE-ECDSA-AES128-GCM-SHA256", 0xC02B),
        ("ECDHE-ECDSA-AES256-GCM-SHA384", 0xC02C),
        ("ECDHE-RSA-AES128-GCM-SHA256", 0xC02F),
        ("ECDHE-RSA-AES256-GCM-SHA384", 0xC030),
        ("ECDHE-RSA-CHACHA20-POLY1305", 0xCCA8),
        ("ECDHE-ECDSA-CHACHA20-POLY1305", 0xCCA9),
        ("DES-CBC3-SHA", 0x000A),
        ("AES128-SHA", 0x002F),
        ("AES256-SHA", 0x0035),
        ("AES128-SHA256", 0x003C),
        ("AES256-SHA256", 0x003D),
        ("ECDHE-RSA-DES-CBC3-SHA", 0xC012),
        ("ECDHE-RSA-AES128-SHA", 0xC013),
        ("ECDHE-RSA-AES256-SHA", 0xC014),
        ("ECDHE-RSA-AES128-SHA256", 0xC027),
        ("ECDHE-RSA-AES256-SHA384", 0xC028),
    ];
    let mut explicit = Vec::new();
    let mut generic = false;
    for tok in list.split([':', ',', ' ']) {
        let t = tok.trim();
        if t.is_empty() || t.starts_with('!') || t.starts_with('-') || t.starts_with('@') {
            continue;
        }
        let t = t.trim_start_matches('+');
        if let Some((_, id)) = NAMED.iter().find(|(n, _)| n.eq_ignore_ascii_case(t)) {
            explicit.push(*id);
        } else if t
            .split('+')
            .all(|w| w.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'))
            && t.chars().any(|c| c.is_ascii_uppercase())
            && !t.contains("NULL")
            && !t.contains("RC4")
            && !t.contains("MD5")
            && !t.starts_with("EXP")
        {
            // Aliases and keywords (DEFAULT, HIGH, ECDHE, AESGCM, ...)
            generic = true;
        }
    }
    if generic {
        Some(None)
    } else if explicit.is_empty() {
        None
    } else {
        // TLS 1.3 suites are configured separately (ciphersuites); keep them.
        explicit.extend_from_slice(&[0x1301, 0x1302, 0x1303]);
        Some(Some(explicit))
    }
}

/// OpenSSL name, bits, version of a cipher suite.
fn cipher_info(id: u16) -> (&'static str, i64, &'static str) {
    match id {
        0x1301 => ("TLS_AES_128_GCM_SHA256", 128, "TLSv1.3"),
        0x1302 => ("TLS_AES_256_GCM_SHA384", 256, "TLSv1.3"),
        0x1303 => ("TLS_CHACHA20_POLY1305_SHA256", 256, "TLSv1.3"),
        0xC02B => ("ECDHE-ECDSA-AES128-GCM-SHA256", 128, "TLSv1.2"),
        0xC02C => ("ECDHE-ECDSA-AES256-GCM-SHA384", 256, "TLSv1.2"),
        0xC02F => ("ECDHE-RSA-AES128-GCM-SHA256", 128, "TLSv1.2"),
        0xC030 => ("ECDHE-RSA-AES256-GCM-SHA384", 256, "TLSv1.2"),
        0xCCA8 => ("ECDHE-RSA-CHACHA20-POLY1305", 256, "TLSv1.2"),
        0xCCA9 => ("ECDHE-ECDSA-CHACHA20-POLY1305", 256, "TLSv1.2"),
        0x000A => ("DES-CBC3-SHA", 112, "SSLv3"),
        0x002F => ("AES128-SHA", 128, "SSLv3"),
        0x0035 => ("AES256-SHA", 256, "SSLv3"),
        0x003C => ("AES128-SHA256", 128, "TLSv1.2"),
        0x003D => ("AES256-SHA256", 256, "TLSv1.2"),
        0xC012 => ("ECDHE-RSA-DES-CBC3-SHA", 112, "TLSv1"),
        0xC013 => ("ECDHE-RSA-AES128-SHA", 128, "TLSv1"),
        0xC014 => ("ECDHE-RSA-AES256-SHA", 256, "TLSv1"),
        0xC027 => ("ECDHE-RSA-AES128-SHA256", 128, "TLSv1.2"),
        0xC028 => ("ECDHE-RSA-AES256-SHA384", 256, "TLSv1.2"),
        _ => ("(NONE)", 0, "(NONE)"),
    }
}

// ---------------------------------------------------------------------------
// The stream
// ---------------------------------------------------------------------------

enum Hs {
    Done,
    WantRead,
    WantWrite,
    Failed,
}

impl SslStream {
    /// Push pending records to the socket; false on a socket error.
    fn flush_out(&mut self, fd: i32) -> Result<bool, i32> {
        while !self.out.is_empty() {
            match sock_send(fd, &self.out) {
                Ok(0) => return Ok(false),
                Ok(n) => {
                    self.out.drain(..n);
                }
                Err(e) => return Err(e),
            }
        }
        Ok(true)
    }

    fn pop_out(&mut self) {
        if let Some(c) = self.conn.as_mut() {
            while let Ok(b) = c.pop() {
                if b.is_empty() {
                    break;
                }
                self.out.extend_from_slice(&b);
            }
        }
    }

    /// Send whatever the engine queued (alerts) before giving up.
    fn flush_alert(&mut self, fd: i32) {
        self.pop_out();
        let _ = self.flush_out(fd);
    }

    fn fail(&mut self, vm: &mut Vm, fd: i32, e: &purecrypto::tls::Error) -> Hs {
        self.flush_alert(fd);
        let f = self.func.clone();
        if matches!(e, purecrypto::tls::Error::HandshakeFailure) && !self.is_client {
            vm.emit_warning(&format!(
                "{}(): SSL_R_NO_SHARED_CIPHER: no suitable shared cipher could be used.  This could be because the server is missing an SSL certificate (local_cert context option)",
                f
            ));
        } else {
            vm.emit_warning(&format!(
                "{}(): {}",
                f,
                ssl_failed_warning(&[error_line(e, self.is_client)])
            ));
        }
        Hs::Failed
    }

    /// The server builds its engine once the ClientHello is in (SNI).
    fn start_server(&mut self, vm: &mut Vm, fd: i32) -> Option<Hs> {
        let info = match purecrypto::tls::peek_client_hello(&self.hello) {
            Ok(None) => return None,
            Ok(Some(i)) => Some(i),
            Err(_) => None,
        };
        let mut cfg = self.config.clone()?;
        let mut has_id = self.has_identity;
        if let Some(name) = info.as_ref().and_then(|i| i.server_name.as_ref()) {
            if let Some(s) = self
                .sni
                .iter()
                .find(|s| matches_wildcard_name(name.as_bytes(), &s.name))
            {
                cfg = s.config.clone();
                has_id = true;
            }
        }
        if !has_id {
            // OpenSSL has no certificate to offer: no shared cipher.
            let _ = sock_send(fd, &[0x15, 0x03, 0x01, 0x00, 0x02, 0x02, 0x28]);
            let f = self.func.clone();
            vm.emit_warning(&format!(
                "{}(): SSL_R_NO_SHARED_CIPHER: no suitable shared cipher could be used.  This could be because the server is missing an SSL certificate (local_cert context option)",
                f
            ));
            return Some(Hs::Failed);
        }
        // purecrypto's TLS 1.2 server engine never answers below TLS 1.2
        // when the client offers 1.2 (it ignores a max_version under 1.2),
        // so refuse such a client like a TLS 1.0/1.1-only OpenSSL server
        // whose answer the client rejects.
        if version_rank(cfg.max_version) < version_rank(ProtocolVersion::TLSv1_2)
            && self.hello.len() >= 11
            && self.hello[5] == 1
        {
            let client_max = u16::from_be_bytes([self.hello[9], self.hello[10]]);
            if client_max > cfg.max_version.as_u16() {
                let _ = sock_send(fd, &[0x15, 0x03, 0x01, 0x00, 0x02, 0x02, 0x46]);
                let f = self.func.clone();
                vm.emit_warning(&format!(
                    "{}(): {}",
                    f,
                    ssl_failed_warning(&[
                        "error:0A000102:SSL routines::unsupported protocol".to_string()
                    ])
                ));
                return Some(Hs::Failed);
            }
        }
        match Connection::server(&cfg) {
            Ok(c) => self.conn = Some(c),
            Err(e) => return Some(self.fail(vm, fd, &e)),
        }
        let hello = std::mem::take(&mut self.hello);
        self.records.scan(&hello);
        if let Some(r) = self.reneg.as_mut() {
            r.handshake_start();
        }
        if let Err(e) = self.conn.as_mut().unwrap().feed(&hello) {
            return Some(self.fail(vm, fd, &e));
        }
        None
    }

    /// verify_callback(): Ok to go on, Err(code) to fail.
    fn verify_cb(&self, err: i32, depth: usize, c: &Cert) -> Result<(), i32> {
        let mut ok = err == 0;
        if err == 18 && self.allow_self_signed {
            ok = true;
        }
        if depth as i64 > self.verify_depth {
            return Err(22);
        }
        if ok && self.security_level > 0 {
            if let Some(k) = c.public_key() {
                if security_bits(&k) < level_bits(self.security_level) {
                    return Err(if depth == 0 { 66 } else { 67 });
                }
            }
        }
        if ok { Ok(()) } else { Err(err) }
    }

    /// The client's chain check, run as soon as the server Certificate
    /// message was processed (before the client Finished is sent).
    fn check_server_chain(&mut self, vm: &mut Vm) -> bool {
        self.chain_checked = true;
        let chain: Vec<Rc<Cert>> = match self.conn.as_ref() {
            Some(c) => c
                .peer_certificates()
                .iter()
                .filter_map(|d| Cert::parse(d).map(Rc::new))
                .collect(),
            None => return true,
        };
        let Some(leaf) = chain.first().cloned() else {
            return true;
        };
        let store = self.store.take().unwrap_or(Store {
            certs: Vec::new(),
            dirs: Vec::new(),
        });
        let mut cb = |e: i32, d: usize, c: &Cert| self.verify_cb(e, d, c);
        let r = x509::verify_peer_chain(&store, &leaf, &chain[1..], &mut cb);
        self.store = Some(store);
        self.verify_result = r;
        let forgiven = r == 18 && self.allow_self_signed;
        if r != 0 && !forgiven && self.verify_peer {
            let f = self.func.clone();
            vm.emit_warning(&format!(
                "{}(): {}",
                f,
                ssl_failed_warning(&[ERR_VERIFY_FAILED.to_string()])
            ));
            // The client aborts with an alert; the engine has none for us,
            // so the peer just sees the connection go away.
            self.verify_failed = true;
            return false;
        }
        true
    }

    /// One pass of SSL_connect()/SSL_accept().
    fn handshake_step(&mut self, vm: &mut Vm, fd: i32) -> Hs {
        let mut buf = vec![0u8; 16 * 1024 + 512];
        loop {
            match self.flush_out(fd) {
                Ok(true) => {}
                Ok(false) => return Hs::WantWrite,
                Err(e) => {
                    let f = self.func.clone();
                    vm.emit_warning(&format!("{}(): SSL: {}", f, strerror(e)));
                    return Hs::Failed;
                }
            }
            if self.conn.is_none() {
                if self.is_client {
                    let Some(cfg) = self.config.as_ref() else {
                        return Hs::Failed;
                    };
                    if version_rank(cfg.min_version) > version_rank(cfg.max_version) {
                        let f = self.func.clone();
                        vm.emit_warning(&format!(
                            "{}(): {}",
                            f,
                            ssl_failed_warning(&[
                                "error:0A0000BF:SSL routines::no protocols available".to_string()
                            ])
                        ));
                        return Hs::Failed;
                    }
                    match Connection::client(cfg) {
                        Ok(c) => self.conn = Some(c),
                        Err(e) => return self.fail(vm, fd, &e),
                    }
                    continue;
                }
                match sock_recv(fd, &mut buf) {
                    Recv::Data(n) => {
                        self.hello.extend_from_slice(&buf[..n]);
                        if let Some(r) = self.start_server(vm, fd) {
                            return r;
                        }
                        continue;
                    }
                    Recv::WouldBlock => return Hs::WantRead,
                    Recv::Eof => return Hs::Failed,
                    Recv::Err(e) => {
                        let f = self.func.clone();
                        vm.emit_warning(&format!("{}(): SSL: {}", f, strerror(e)));
                        return Hs::Failed;
                    }
                }
            }
            let step = self.conn.as_mut().unwrap().drive();
            match step {
                Ok(Step::WantWrite) => {
                    self.pop_out();
                    continue;
                }
                Ok(Step::Complete) => return Hs::Done,
                Ok(Step::WantRead) => match sock_recv(fd, &mut buf) {
                    Recv::Data(n) => {
                        self.records.scan(&buf[..n]);
                        let r = self.conn.as_mut().unwrap().feed(&buf[..n]);
                        if let Err(e) = r {
                            return self.fail(vm, fd, &e);
                        }
                        if self.is_client
                            && !self.chain_checked
                            && !self.conn.as_ref().unwrap().peer_certificates().is_empty()
                            && !self.check_server_chain(vm)
                        {
                            return Hs::Failed;
                        }
                        continue;
                    }
                    Recv::WouldBlock => return Hs::WantRead,
                    Recv::Eof => return Hs::Failed,
                    Recv::Err(e) => {
                        let f = self.func.clone();
                        vm.emit_warning(&format!("{}(): SSL: {}", f, strerror(e)));
                        return Hs::Failed;
                    }
                },
                Ok(_) => continue,
                Err(e) => return self.fail(vm, fd, &e),
            }
        }
    }

    fn peer_cert(&self) -> Option<Rc<Cert>> {
        let c = self.conn.as_ref()?;
        c.peer_certificates()
            .first()
            .and_then(|d| Cert::parse(d))
            .map(Rc::new)
    }

    /// php_openssl_capture_peer_certs()
    fn capture(&self, vm: &mut Vm, opts: Option<&PhpArray>) -> Vec<(Vec<u8>, Value)> {
        let mut out = Vec::new();
        let Some(conn) = self.conn.as_ref() else {
            return out;
        };
        let ders: Vec<Vec<u8>> = conn.peer_certificates().to_vec();
        if ders.is_empty() {
            return out;
        }
        if opt_true(opts, "capture_peer_cert") == Some(true) {
            if let Some(c) = Cert::parse(&ders[0]) {
                let v = x509::make_cert_object(vm, Rc::new(c));
                out.push((b"peer_certificate".to_vec(), v));
            }
        }
        if opt_true(opts, "capture_peer_cert_chain") == Some(true) {
            let mut arr = PhpArray::new();
            for d in &ders {
                if let Some(c) = Cert::parse(d) {
                    arr.push(x509::make_cert_object(vm, Rc::new(c)));
                }
            }
            out.push((
                b"peer_certificate_chain".to_vec(),
                Value::Array(Rc::new(std::cell::RefCell::new(arr))),
            ));
        }
        out
    }

    /// php_openssl_apply_peer_verification_policy()
    fn apply_policy(&mut self, vm: &mut Vm, opts: Option<&PhpArray>) -> bool {
        let f = self.func.clone();
        let peer = self.peer_cert();
        let must_verify_peer = opt_true(opts, "verify_peer").unwrap_or(self.is_client);
        let must_verify_peer_name = opt_true(opts, "verify_peer_name").unwrap_or(self.is_client);
        let fingerprint = opt(opts, "peer_fingerprint").cloned();
        if (must_verify_peer || must_verify_peer_name || fingerprint.is_some()) && peer.is_none() {
            vm.emit_warning(&format!("{}(): Could not get peer certificate", f));
            return false;
        }
        if must_verify_peer {
            let err = self.verify_result;
            let ok = err == 0 || (err == 18 && self.allow_self_signed);
            if !ok {
                vm.emit_warning(&format!(
                    "{}(): Could not verify peer: code:{} {}",
                    f,
                    err,
                    x509::verify_error_string(err)
                ));
                return false;
            }
        }
        let peer = match peer {
            Some(p) => p,
            None => return true,
        };
        if let Some(fp) = fingerprint {
            if matches!(fp.deref(), Value::String(_) | Value::Array(_)) {
                if !fingerprint_match(vm, &f, &peer, &fp) {
                    vm.emit_warning(&format!("{}(): peer_fingerprint match failure", f));
                    return false;
                }
            } else {
                vm.emit_warning(&format!(
                    "{}(): Expected peer fingerprint must be a string or an array",
                    f
                ));
                return false;
            }
        }
        if must_verify_peer_name {
            let mut name = opt_bytes(opts, "peer_name").map(|b| cstr(&b).into_bytes());
            if name.is_none() && self.is_client {
                name = self.url_name.clone();
            }
            let Some(name) = name else {
                return false;
            };
            if matches_san_list(&peer, &name) {
                return true;
            }
            return matches_common_name(vm, &f, &peer, &name);
        }
        true
    }

    fn shutdown(&mut self, fd: i32) {
        if let Some(c) = self.conn.as_mut() {
            let _ = c.close();
        }
        self.pop_out();
        // best effort: wait briefly for the socket to take the alert
        let start = Instant::now();
        while !self.out.is_empty() {
            match self.flush_out(fd) {
                Ok(true) | Err(_) => break,
                Ok(false) => {
                    if expired(start, 100) {
                        break;
                    }
                    poll_fd(fd, libc::POLLOUT, 100);
                }
            }
        }
        self.out.clear();
    }

    /// Pull records off the socket into `plain`. Ok(true): something
    /// arrived; Ok(false): would block.
    fn fill(&mut self, fd: i32) -> std::io::Result<bool> {
        let mut buf = vec![0u8; 16 * 1024 + 512];
        match sock_recv(fd, &mut buf) {
            Recv::Data(n) => {
                // A handshake record after the handshake: the client
                // renegotiates (the info callback's SSL_CB_HANDSHAKE_START).
                let reneg = self.records.scan(&buf[..n])
                    && self.active
                    && self.conn.as_ref().and_then(|c| c.negotiated_version())
                        != Some(ProtocolVersion::TLSv1_3);
                if reneg {
                    if let Some(r) = self.reneg.as_mut() {
                        if r.handshake_start() {
                            // should_close: php_stream_xport_shutdown(SHUT_RDWR)
                            // and EOF (the engine cannot renegotiate anyway).
                            if let Some(cb) = r.callback.clone() {
                                self.callbacks.push(cb);
                            }
                            unsafe {
                                libc::shutdown(fd, libc::SHUT_RDWR);
                            }
                            self.eof = true;
                            return Ok(true);
                        }
                    }
                }
                let conn = self.conn.as_mut().unwrap();
                if let Err(e) = conn.feed(&buf[..n]) {
                    if let Ok(pt) = conn.recv() {
                        self.plain.extend_from_slice(&pt);
                    }
                    if !matches!(
                        e,
                        purecrypto::tls::Error::AlertReceived(
                            purecrypto::tls::AlertDescription::CloseNotify
                        )
                    ) {
                        self.warnings
                            .push(ssl_failed_warning(&[error_line(&e, self.is_client)]));
                    }
                    self.eof = true;
                    self.pop_out();
                    let _ = self.flush_out(fd);
                    return Ok(true);
                }
                if let Ok(pt) = conn.recv() {
                    self.plain.extend_from_slice(&pt);
                }
                if conn.received_close_notify() {
                    self.eof = true;
                }
                // key updates, session tickets acks, alerts
                self.pop_out();
                let _ = self.flush_out(fd);
                Ok(true)
            }
            Recv::Eof => {
                self.eof = true;
                Ok(true)
            }
            Recv::WouldBlock => Ok(false),
            Recv::Err(e) => {
                self.eof = true;
                self.warnings.push(format!("SSL: {}", strerror(e)));
                Ok(true)
            }
        }
    }

    fn take_plain(&mut self, buf: &mut [u8]) -> usize {
        let n = buf.len().min(self.plain.len());
        buf[..n].copy_from_slice(&self.plain[..n]);
        self.plain.drain(..n);
        n
    }
}

impl CryptoStream for SslStream {
    fn enable(
        &mut self,
        vm: &mut Vm,
        func: &str,
        fd: i32,
        blocked: bool,
        timeout_ms: i32,
        opts: Option<&PhpArray>,
    ) -> CryptoEnable {
        self.func = func.to_string();
        if self.active {
            return CryptoEnable {
                result: -1,
                captured: Vec::new(),
            };
        }
        let start = Instant::now();
        let f = self.func.clone();
        let r = loop {
            let r = self.handshake_step(vm, fd);
            if expired(start, timeout_ms) {
                vm.emit_warning(&format!("{}(): SSL: Handshake timed out", f));
                return CryptoEnable {
                    result: -1,
                    captured: Vec::new(),
                };
            }
            match r {
                Hs::Done => break 1,
                Hs::Failed => break -1,
                Hs::WantRead | Hs::WantWrite => {
                    if !blocked {
                        return CryptoEnable {
                            result: 0,
                            captured: Vec::new(),
                        };
                    }
                    let ev = if matches!(r, Hs::WantRead) {
                        libc::POLLIN | libc::POLLPRI
                    } else {
                        libc::POLLOUT
                    };
                    let left = time_left(start, timeout_ms).unwrap_or(-1);
                    poll_fd(fd, ev, left);
                }
            }
        };
        let mut captured = Vec::new();
        let mut result = r;
        if r == 1 {
            if self.is_client && !self.chain_checked && !self.check_server_chain(vm) {
                result = -1;
            }
        }
        if self.peer_cert().is_some() && opts.is_some() {
            captured = self.capture(vm, opts);
        }
        if result == 1 {
            if !self.apply_policy(vm, opts) {
                self.shutdown(fd);
                result = -1;
            } else {
                self.active = true;
            }
        }
        CryptoEnable { result, captured }
    }

    fn disable(&mut self, fd: i32) -> bool {
        if !self.active {
            return false;
        }
        self.shutdown(fd);
        self.active = false;
        true
    }

    fn is_active(&self) -> bool {
        self.active
    }

    fn read(
        &mut self,
        fd: i32,
        buf: &mut [u8],
        blocked: bool,
        timeout_ms: i32,
    ) -> std::io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let start = Instant::now();
        loop {
            if !self.plain.is_empty() {
                return Ok(self.take_plain(buf));
            }
            if self.eof {
                return Ok(0);
            }
            let _ = self.flush_out(fd);
            if self.fill(fd)? {
                continue;
            }
            if !blocked {
                return Err(std::io::ErrorKind::WouldBlock.into());
            }
            if expired(start, timeout_ms) {
                return Err(std::io::ErrorKind::TimedOut.into());
            }
            let left = time_left(start, timeout_ms).unwrap_or(-1);
            let ev = if self.out.is_empty() {
                libc::POLLIN | libc::POLLPRI
            } else {
                libc::POLLIN | libc::POLLPRI | libc::POLLOUT
            };
            poll_fd(fd, ev, left);
        }
    }

    fn write(
        &mut self,
        fd: i32,
        data: &[u8],
        blocked: bool,
        timeout_ms: i32,
    ) -> std::io::Result<usize> {
        let start = Instant::now();
        // Records from an earlier write must leave first.
        loop {
            match self.flush_out(fd) {
                Ok(true) => break,
                Ok(false) => {
                    if !blocked {
                        return Err(std::io::ErrorKind::WouldBlock.into());
                    }
                    if expired(start, timeout_ms) {
                        return Err(std::io::ErrorKind::TimedOut.into());
                    }
                    let left = time_left(start, timeout_ms).unwrap_or(-1);
                    poll_fd(fd, libc::POLLOUT, left);
                }
                Err(e) => {
                    self.warnings.push(format!("SSL: {}", strerror(e)));
                    return Err(std::io::Error::from_raw_os_error(e));
                }
            }
        }
        if data.is_empty() {
            return Ok(0);
        }
        let n = data.len().min(16384);
        let Some(conn) = self.conn.as_mut() else {
            return Err(std::io::ErrorKind::NotConnected.into());
        };
        if let Err(e) = conn.send(&data[..n]) {
            if conn.received_close_notify() {
                // OpenSSL still writes after the peer's close_notify; the
                // engine cannot, and the peer reads no more anyway.
                return Ok(n);
            }
            self.warnings
                .push(ssl_failed_warning(&[error_line(&e, self.is_client)]));
            return Err(std::io::ErrorKind::Other.into());
        }
        self.pop_out();
        loop {
            match self.flush_out(fd) {
                Ok(true) => break,
                Ok(false) => {
                    // The record is ours now; a non-blocking stream sends
                    // the rest on the next write/read/close.
                    if !blocked {
                        break;
                    }
                    if expired(start, timeout_ms) {
                        break;
                    }
                    let left = time_left(start, timeout_ms).unwrap_or(-1);
                    poll_fd(fd, libc::POLLOUT, left);
                }
                Err(e) => {
                    self.warnings.push(format!("SSL: {}", strerror(e)));
                    return Err(std::io::Error::from_raw_os_error(e));
                }
            }
        }
        Ok(n)
    }

    fn pending(&self) -> usize {
        self.plain.len()
    }

    fn meta(&self) -> Option<PhpArray> {
        if !self.active {
            return None;
        }
        let conn = self.conn.as_ref()?;
        let mut a = PhpArray::new();
        let proto = match conn.negotiated_version() {
            Some(ProtocolVersion::TLSv1_3) => "TLSv1.3",
            Some(ProtocolVersion::TLSv1_2) => "TLSv1.2",
            Some(ProtocolVersion::TLSv1_1) => "TLSv1.1",
            Some(ProtocolVersion::TLSv1_0) => "TLSv1",
            Some(ProtocolVersion::SSLv3) => "SSLv3",
            _ => "UNKNOWN",
        };
        let (name, bits, ver) = cipher_info(conn.negotiated_cipher_suite().unwrap_or(0));
        a.set(akey("protocol"), sval(proto));
        a.set(akey("cipher_name"), sval(name));
        a.set(akey("cipher_bits"), Value::Long(bits));
        a.set(akey("cipher_version"), sval(ver));
        if let Some(p) = conn.alpn_selected() {
            a.set(
                akey("alpn_protocol"),
                Value::String(PhpString::from_bytes(p)),
            );
        }
        Some(a)
    }

    fn take_warnings(&mut self) -> Vec<String> {
        std::mem::take(&mut self.warnings)
    }

    fn take_callbacks(&mut self) -> Vec<Value> {
        std::mem::take(&mut self.callbacks)
    }

    fn alive(&mut self, fd: i32) -> bool {
        if !self.plain.is_empty() {
            return true;
        }
        if self.eof {
            return false;
        }
        if poll_fd(
            fd,
            libc::POLLIN | libc::POLLPRI | libc::POLLERR | libc::POLLHUP,
            0,
        ) > 0
        {
            let _ = self.fill(fd);
            return !self.plain.is_empty() || !self.eof;
        }
        true
    }

    fn close(&mut self, fd: i32) {
        if self.active {
            self.shutdown(fd);
            self.active = false;
        }
    }
}
