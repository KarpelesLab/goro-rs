//! TLS sessions for PHP's `ssl://`/`tls://` streams and
//! stream_socket_enable_crypto() (PHP manual: SSL context options), on
//! purecrypto's sans-I/O TLS engine (RFC 8446 TLS 1.3, RFC 5246 TLS 1.2, and
//! the legacy TLS 1.0/1.1 versions OpenSSL still allows at security level 0).
//!
//! The engine owns the socket: a [`Session`] reads and writes through an
//! [`Io`] the caller supplies, which may be blocking (with a timeout) or
//! non-blocking (`WouldBlock` errors make the operation resumable).
//!
//! Peer verification with PHP's semantics (verify_peer, allow_self_signed,
//! verify_depth, peer_name, peer_fingerprint) is done by the caller on
//! [`Session::peer_certificates`] once the handshake completes; the engine
//! is configured not to verify by itself.

use std::io;

use purecrypto::tls::{
    ClientAuth, Config, Connection, ProtocolVersion, RootCertStore, SigningKey, Step,
};

use crate::key::{Key, KeyType, LoadError};
use crate::x509::{self, Cert};

/// The transport under a session.
pub trait Io {
    /// Reads available bytes; `Ok(0)` at the end of the stream.
    fn recv(&mut self, buf: &mut [u8]) -> io::Result<usize>;
    /// Writes some of `data`, returning how much was taken.
    fn send(&mut self, data: &[u8]) -> io::Result<usize>;
}

/// A certificate chain and its private key.
#[derive(Clone)]
pub struct Identity {
    /// DER certificates, leaf first.
    pub chain: Vec<Vec<u8>>,
    key: SigningKey,
}

/// Why loading an identity failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdentityError {
    /// No certificate in the data.
    NoCertificate,
    /// No usable private key (missing, undecodable, or a wrong passphrase).
    BadKey(LoadErrorKind),
    /// The key does not belong to the certificate.
    Mismatch,
    /// A key type TLS cannot sign with (DSA, DH, X25519...).
    UnsupportedKey,
}

/// A copyable form of the key loading error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadErrorKind {
    NoStartLine,
    NoPassphrase,
    BadDecrypt,
    Decode,
}

fn load_kind(e: &LoadError) -> LoadErrorKind {
    match e {
        LoadError::NoStartLine => LoadErrorKind::NoStartLine,
        LoadError::NoPassphrase => LoadErrorKind::NoPassphrase,
        LoadError::BadDecrypt => LoadErrorKind::BadDecrypt,
        _ => LoadErrorKind::Decode,
    }
}

fn signing_key(k: &Key) -> Result<SigningKey, IdentityError> {
    let der = k.to_pkcs8_der().ok_or(IdentityError::UnsupportedKey)?;
    fn bad<E>(_: E) -> IdentityError {
        IdentityError::UnsupportedKey
    }
    Ok(match k.key_type() {
        KeyType::Rsa => {
            SigningKey::Rsa(purecrypto::rsa::BoxedRsaPrivateKey::from_pkcs8_der(&der).map_err(bad)?)
        }
        KeyType::Ec => SigningKey::Ecdsa(
            purecrypto::ec::BoxedEcdsaPrivateKey::from_pkcs8_der(&der).map_err(bad)?,
        ),
        KeyType::Ed25519 => SigningKey::Ed25519(
            purecrypto::ec::Ed25519PrivateKey::from_pkcs8_der(&der).map_err(bad)?,
        ),
        KeyType::Ed448 => {
            SigningKey::Ed448(purecrypto::ec::Ed448PrivateKey::from_pkcs8_der(&der).map_err(bad)?)
        }
        _ => return Err(IdentityError::UnsupportedKey),
    })
}

impl Identity {
    /// Certificates from `cert_pem` (leaf first, then the chain) and the
    /// key from `key_pem` (or from `cert_pem` itself when none).
    pub fn from_pem(
        cert_pem: &[u8],
        key_pem: Option<&[u8]>,
        pass: Option<&[u8]>,
    ) -> Result<Identity, IdentityError> {
        let certs = x509::certs_from_bytes(cert_pem);
        if certs.is_empty() {
            return Err(IdentityError::NoCertificate);
        }
        let k = crate::key::load_private_pem(key_pem.unwrap_or(cert_pem), pass)
            .map_err(|e| IdentityError::BadKey(load_kind(&e)))?;
        let leaf_key = certs[0]
            .public_key()
            .map_err(|_| IdentityError::NoCertificate)?;
        if !leaf_key.public_eq(&k) {
            return Err(IdentityError::Mismatch);
        }
        Ok(Identity {
            chain: certs.into_iter().map(|c| c.der).collect(),
            key: signing_key(&k)?,
        })
    }
}

/// A protocol version range (PHP's crypto_method / min_proto_version /
/// max_proto_version, as STREAM_CRYPTO_PROTO_* bits).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Versions {
    pub min: ProtocolVersion,
    pub max: ProtocolVersion,
}

/// STREAM_CRYPTO_PROTO_* bits (PHP manual: predefined stream constants).
pub mod proto {
    pub const SSLV3: i64 = 4;
    pub const TLS1_0: i64 = 8;
    pub const TLS1_1: i64 = 16;
    pub const TLS1_2: i64 = 32;
    pub const TLS1_3: i64 = 64;
}

impl Versions {
    /// The range spanning the protocol bits set in `mask` (OpenSSL takes
    /// the lowest and highest version enabled; observed on PHP 8.5).
    /// `None` when no version purecrypto supports is set.
    pub fn from_mask(mask: i64) -> Option<Versions> {
        let order = [
            (proto::TLS1_0, ProtocolVersion::TLSv1_0),
            (proto::TLS1_1, ProtocolVersion::TLSv1_1),
            (proto::TLS1_2, ProtocolVersion::TLSv1_2),
            (proto::TLS1_3, ProtocolVersion::TLSv1_3),
        ];
        let set: Vec<ProtocolVersion> = order
            .iter()
            .filter(|(b, _)| mask & b != 0)
            .map(|(_, v)| *v)
            .collect();
        Some(Versions {
            min: *set.first()?,
            max: *set.last()?,
        })
    }
}

/// Settings of a client session.
pub struct ClientConfig {
    /// SNI host name (none for IP addresses).
    pub sni: Option<String>,
    pub versions: Versions,
    pub alpn: Vec<Vec<u8>>,
    /// A client certificate (local_cert / local_pk).
    pub identity: Option<Identity>,
}

/// Settings of a server session.
pub struct ServerConfig {
    /// The default certificate (local_cert), if any.
    pub identity: Option<Identity>,
    /// Per-host certificates (SNI_server_certs), matched case-insensitively
    /// and with `*.` wildcards.
    pub sni_identities: Vec<(String, Identity)>,
    pub versions: Versions,
    pub alpn: Vec<Vec<u8>>,
    /// verify_peer on a server: request a client certificate, verified
    /// against these trust anchors (DER).
    pub client_roots: Option<Vec<Vec<u8>>>,
}

/// A handshake failure.
#[derive(Debug)]
pub enum TlsError {
    /// The transport failed (or timed out).
    Io(io::Error),
    /// The peer closed the connection during the handshake.
    Eof,
    /// A TLS-level failure: purecrypto's description.
    Protocol(String),
    /// A server without any certificate for the requested name.
    NoCertificate,
}

impl std::fmt::Display for TlsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TlsError::Io(e) => write!(f, "{e}"),
            TlsError::Eof => f.write_str("unexpected eof while reading"),
            TlsError::Protocol(s) => f.write_str(s),
            TlsError::NoCertificate => f.write_str("no certificate assigned"),
        }
    }
}

fn perr(e: purecrypto::tls::Error) -> TlsError {
    TlsError::Protocol(format!("{e:?}"))
}

fn builder(v: Versions) -> purecrypto::tls::ConfigBuilder {
    let mut b = Config::builder()
        .versions(v.min, v.max)
        .verify_certificates(false)
        .rng(std::sync::Arc::new(purecrypto::rng::OsRng))
        // Key sizes are policed by the caller (security_level).
        .signature_policy(
            purecrypto::signature_registry::SignaturePolicy::modern().with_min_rsa_bits(512),
        );
    if v.min.as_u16() < ProtocolVersion::TLSv1_2.as_u16() {
        // The legacy versions predate RFC 7627; OpenSSL negotiates them with
        // peers that do not offer the extended master secret.
        b = b.require_extended_master_secret(false);
    }
    b
}

enum State {
    /// A server waiting for the ClientHello to pick its certificate.
    Peeking(Box<ServerConfig>, Vec<u8>),
    Active(Box<Connection>),
    Failed,
}

/// A TLS session over an [`Io`].
pub struct Session {
    state: State,
    /// Wire bytes produced but not yet written.
    out: Vec<u8>,
    /// Decrypted bytes not yet returned.
    plain: Vec<u8>,
    plain_pos: usize,
    peer_eof: bool,
    done: bool,
    /// The SNI the client asked for (server side).
    sni: Option<String>,
    /// Peer bytes the caller read before the session started.
    early_in: Vec<u8>,
}

fn host_matches(pattern: &str, host: &str) -> bool {
    let p = pattern.to_ascii_lowercase();
    let h = host.to_ascii_lowercase();
    if let Some(rest) = p.strip_prefix("*.") {
        return h
            .split_once('.')
            .is_some_and(|(first, tail)| !first.is_empty() && tail == rest);
    }
    p == h
}

impl Session {
    pub fn client(c: ClientConfig) -> Result<Session, TlsError> {
        let mut b = builder(c.versions);
        if let Some(s) = c.sni {
            b = b.server_name(s);
        }
        if !c.alpn.is_empty() {
            b = b.alpn(c.alpn);
        }
        if let Some(id) = c.identity {
            b = b.identity(id.chain, id.key);
        }
        let cfg = b.build();
        let conn = Connection::client(&cfg).map_err(perr)?;
        Ok(Session::with(State::Active(Box::new(conn))))
    }

    pub fn server(c: ServerConfig) -> Session {
        Session::with(State::Peeking(Box::new(c), Vec::new()))
    }

    fn with(state: State) -> Session {
        Session {
            state,
            out: Vec::new(),
            plain: Vec::new(),
            plain_pos: 0,
            peer_eof: false,
            done: false,
            sni: None,
            early_in: Vec::new(),
        }
    }

    fn server_conn(c: &ServerConfig, sni: Option<&str>) -> Result<Connection, TlsError> {
        let id = sni
            .and_then(|h| {
                c.sni_identities
                    .iter()
                    .find(|(p, _)| host_matches(p, h))
                    .map(|(_, i)| i)
            })
            .or(c.identity.as_ref())
            .ok_or(TlsError::NoCertificate)?;
        let mut b = builder(c.versions).identity(id.chain.clone(), id.key.clone());
        if !c.alpn.is_empty() {
            b = b.alpn(c.alpn.clone());
        }
        if let Some(roots) = &c.client_roots {
            let mut store = RootCertStore::new();
            for r in roots {
                let _ = store.add_der(r.clone());
            }
            b = b.client_auth(ClientAuth::new(store, false));
        }
        let cfg = b.build();
        Connection::server(&cfg).map_err(perr)
    }

    /// Writes out pending wire bytes. `WouldBlock` leaves the rest queued.
    fn flush_out(&mut self, io: &mut dyn Io) -> io::Result<()> {
        while !self.out.is_empty() {
            let n = io.send(&self.out)?;
            if n == 0 {
                return Err(io::Error::from(io::ErrorKind::WriteZero));
            }
            self.out.drain(..n);
        }
        Ok(())
    }

    fn conn(&mut self) -> Option<&mut Connection> {
        match &mut self.state {
            State::Active(c) => Some(c),
            _ => None,
        }
    }

    /// Advances the handshake: `Ok(true)` once complete, `Ok(false)` when
    /// the non-blocking transport would block.
    pub fn handshake(&mut self, io: &mut dyn Io) -> Result<bool, TlsError> {
        if self.done {
            return Ok(true);
        }
        let mut buf = vec![0u8; 16 * 1024];
        loop {
            if let State::Peeking(cfg, pre) = &mut self.state {
                match purecrypto::tls::peek_client_hello(pre) {
                    Ok(Some(info)) => {
                        let sni = info.server_name.clone();
                        let pre = std::mem::take(pre);
                        let conn = match Session::server_conn(cfg, sni.as_deref()) {
                            Ok(c) => c,
                            Err(e) => {
                                self.state = State::Failed;
                                return Err(e);
                            }
                        };
                        self.sni = sni;
                        let mut conn = Box::new(conn);
                        if let Err(e) = conn.feed(&pre) {
                            self.state = State::Failed;
                            let _ = self.drain(io);
                            return Err(perr(e));
                        }
                        self.state = State::Active(conn);
                        continue;
                    }
                    Ok(None) => {}
                    Err(e) => {
                        self.state = State::Failed;
                        return Err(perr(e));
                    }
                }
                match io.recv(&mut buf) {
                    Ok(0) => return Err(TlsError::Eof),
                    Ok(n) => {
                        pre.extend_from_slice(&buf[..n]);
                        continue;
                    }
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => return Ok(false),
                    Err(e) => return Err(TlsError::Io(e)),
                }
            }
            let State::Active(conn) = &mut self.state else {
                return Err(TlsError::Protocol("InappropriateState".into()));
            };
            let step = match conn.drive() {
                Ok(s) => s,
                Err(e) => {
                    // Send the alert the engine queued.
                    let _ = self.drain(io);
                    self.state = State::Failed;
                    return Err(perr(e));
                }
            };
            match step {
                Step::WantWrite => {
                    let bytes = conn.pop().map_err(perr)?;
                    self.out.extend_from_slice(&bytes);
                    match self.flush_out(io) {
                        Ok(()) => {}
                        Err(e) if e.kind() == io::ErrorKind::WouldBlock => return Ok(false),
                        Err(e) => return Err(TlsError::Io(e)),
                    }
                }
                Step::WantRead => {
                    match self.flush_out(io) {
                        Ok(()) => {}
                        Err(e) if e.kind() == io::ErrorKind::WouldBlock => return Ok(false),
                        Err(e) => return Err(TlsError::Io(e)),
                    }
                    let early = std::mem::take(&mut self.early_in);
                    let got = if early.is_empty() {
                        io.recv(&mut buf).map(|n| buf[..n].to_vec())
                    } else {
                        Ok(early)
                    };
                    match got {
                        Ok(d) if d.is_empty() => return Err(TlsError::Eof),
                        Ok(d) => {
                            let State::Active(conn) = &mut self.state else {
                                unreachable!()
                            };
                            if let Err(e) = conn.feed(&d) {
                                let _ = self.drain(io);
                                self.state = State::Failed;
                                return Err(perr(e));
                            }
                        }
                        Err(e) if e.kind() == io::ErrorKind::WouldBlock => return Ok(false),
                        Err(e) => return Err(TlsError::Io(e)),
                    }
                }
                Step::Complete => {
                    match self.flush_out(io) {
                        Ok(()) => {}
                        Err(e) if e.kind() == io::ErrorKind::WouldBlock => return Ok(false),
                        Err(e) => return Err(TlsError::Io(e)),
                    }
                    self.done = true;
                    self.collect();
                    return Ok(true);
                }
                _ => return Err(TlsError::Protocol("external signer".into())),
            }
        }
    }

    /// Queues and tries to write whatever the engine has to send.
    fn drain(&mut self, io: &mut dyn Io) -> io::Result<()> {
        if let State::Active(c) = &mut self.state
            && let Ok(b) = c.pop()
        {
            self.out.extend_from_slice(&b);
        }
        self.flush_out(io)
    }

    /// Moves decrypted bytes out of the engine.
    fn collect(&mut self) {
        if let State::Active(c) = &mut self.state {
            if let Ok(v) = c.recv()
                && !v.is_empty()
            {
                if self.plain_pos == self.plain.len() {
                    self.plain.clear();
                    self.plain_pos = 0;
                }
                self.plain.extend_from_slice(&v);
            }
            if c.received_close_notify() {
                self.peer_eof = true;
            }
        }
    }

    /// Peer bytes already read from the transport before the session
    /// started (the start of the handshake).
    pub fn push_input(&mut self, data: &[u8]) {
        match &mut self.state {
            State::Peeking(_, pre) => pre.extend_from_slice(data),
            _ => self.early_in.extend_from_slice(data),
        }
    }

    /// Whether decrypted bytes are waiting.
    pub fn buffered(&self) -> bool {
        self.plain_pos < self.plain.len()
    }

    /// Whether the peer closed the session (close_notify or end of stream).
    pub fn peer_closed(&self) -> bool {
        self.peer_eof
    }

    /// Reads decrypted bytes; `Ok(0)` when the peer closed.
    pub fn read(&mut self, io: &mut dyn Io, out: &mut [u8]) -> io::Result<usize> {
        let mut buf = vec![0u8; 16 * 1024 + 512];
        loop {
            if self.buffered() {
                let n = out.len().min(self.plain.len() - self.plain_pos);
                out[..n].copy_from_slice(&self.plain[self.plain_pos..self.plain_pos + n]);
                self.plain_pos += n;
                return Ok(n);
            }
            if self.peer_eof || self.conn().is_none() {
                return Ok(0);
            }
            // Post-handshake messages (TLS 1.3 tickets, key updates) may
            // need answers.
            let _ = self.flush_out(io);
            let n = io.recv(&mut buf)?;
            if n == 0 {
                self.peer_eof = true;
                return Ok(0);
            }
            let r = self.conn().map(|c| c.feed(&buf[..n]));
            if let Some(Err(e)) = r {
                self.peer_eof = true;
                return Err(io::Error::other(format!("{e:?}")));
            }
            self.collect();
            let _ = self.drain(io);
        }
    }

    /// Encrypts and writes `data`. On a non-blocking transport the records
    /// that could not be written stay queued (and are written by later
    /// calls); the data counts as written.
    pub fn write(&mut self, io: &mut dyn Io, data: &[u8]) -> io::Result<usize> {
        if !self.out.is_empty() {
            self.flush_out(io)?;
        }
        let Some(c) = self.conn() else {
            return Err(io::Error::from(io::ErrorKind::BrokenPipe));
        };
        for chunk in data.chunks(16 * 1024) {
            c.send(chunk)
                .map_err(|e| io::Error::other(format!("{e:?}")))?;
        }
        let bytes = c.pop().map_err(|e| io::Error::other(format!("{e:?}")))?;
        self.out.extend_from_slice(&bytes);
        match self.flush_out(io) {
            Ok(()) => Ok(data.len()),
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => Ok(data.len()),
            Err(e) => Err(e),
        }
    }

    /// Writes queued records.
    pub fn flush(&mut self, io: &mut dyn Io) -> io::Result<()> {
        self.flush_out(io)
    }

    /// Whether records are still queued for writing.
    pub fn pending_output(&self) -> bool {
        !self.out.is_empty()
    }

    /// Sends close_notify.
    pub fn close(&mut self, io: &mut dyn Io) {
        if let State::Active(c) = &mut self.state
            && self.done
            && c.close().is_ok()
        {
            let _ = self.drain(io);
        }
    }

    /// The peer's certificate chain (DER, leaf first).
    pub fn peer_certificates(&self) -> Vec<Vec<u8>> {
        match &self.state {
            State::Active(c) => c.peer_certificates().to_vec(),
            _ => Vec::new(),
        }
    }

    /// The SNI the client sent (server side).
    pub fn requested_server_name(&self) -> Option<&str> {
        self.sni.as_deref()
    }

    /// The negotiated ALPN protocol.
    pub fn alpn(&self) -> Option<Vec<u8>> {
        match &self.state {
            State::Active(c) => c.alpn_selected().map(|a| a.to_vec()),
            _ => None,
        }
    }

    /// The negotiated version as OpenSSL names it ("TLSv1.3", "TLSv1").
    pub fn protocol(&self) -> &'static str {
        let v = match &self.state {
            State::Active(c) => c.negotiated_version(),
            _ => None,
        };
        match v {
            Some(ProtocolVersion::TLSv1_3) => "TLSv1.3",
            Some(ProtocolVersion::TLSv1_2) => "TLSv1.2",
            Some(ProtocolVersion::TLSv1_1) => "TLSv1.1",
            Some(ProtocolVersion::TLSv1_0) => "TLSv1",
            Some(ProtocolVersion::SSLv3) => "SSLv3",
            _ => "unknown",
        }
    }

    /// The negotiated cipher suite: OpenSSL's name, its key bits and the
    /// protocol version OpenSSL reports for it.
    pub fn cipher(&self) -> (&'static str, i64, &'static str) {
        let id = match &self.state {
            State::Active(c) => c.negotiated_cipher_suite(),
            _ => None,
        };
        suite_info(id.unwrap_or(0))
    }
}

/// OpenSSL names of the IANA cipher suites (OpenSSL documentation:
/// openssl-ciphers, "Cipher suite names"), with their secret key bits and
/// the version OpenSSL's SSL_CIPHER_get_version reports.
pub fn suite_info(id: u16) -> (&'static str, i64, &'static str) {
    match id {
        0x1301 => ("TLS_AES_128_GCM_SHA256", 128, "TLSv1.3"),
        0x1302 => ("TLS_AES_256_GCM_SHA384", 256, "TLSv1.3"),
        0x1303 => ("TLS_CHACHA20_POLY1305_SHA256", 256, "TLSv1.3"),
        0xC02F => ("ECDHE-RSA-AES128-GCM-SHA256", 128, "TLSv1.2"),
        0xC030 => ("ECDHE-RSA-AES256-GCM-SHA384", 256, "TLSv1.2"),
        0xC02B => ("ECDHE-ECDSA-AES128-GCM-SHA256", 128, "TLSv1.2"),
        0xC02C => ("ECDHE-ECDSA-AES256-GCM-SHA384", 256, "TLSv1.2"),
        0xCCA8 => ("ECDHE-RSA-CHACHA20-POLY1305", 256, "TLSv1.2"),
        0xCCA9 => ("ECDHE-ECDSA-CHACHA20-POLY1305", 256, "TLSv1.2"),
        0x009C => ("AES128-GCM-SHA256", 128, "TLSv1.2"),
        0x009D => ("AES256-GCM-SHA384", 256, "TLSv1.2"),
        0x003C => ("AES128-SHA256", 128, "TLSv1.2"),
        0x003D => ("AES256-SHA256", 256, "TLSv1.2"),
        0xC027 => ("ECDHE-RSA-AES128-SHA256", 128, "TLSv1.2"),
        0xC028 => ("ECDHE-RSA-AES256-SHA384", 256, "TLSv1.2"),
        0x002F => ("AES128-SHA", 128, "SSLv3"),
        0x0035 => ("AES256-SHA", 256, "SSLv3"),
        0xC013 => ("ECDHE-RSA-AES128-SHA", 128, "TLSv1"),
        0xC014 => ("ECDHE-RSA-AES256-SHA", 256, "TLSv1"),
        0xC009 => ("ECDHE-ECDSA-AES128-SHA", 128, "TLSv1"),
        0xC00A => ("ECDHE-ECDSA-AES256-SHA", 256, "TLSv1"),
        0x000A => ("DES-CBC3-SHA", 112, "SSLv3"),
        0xC012 => ("ECDHE-RSA-DES-CBC3-SHA", 112, "TLSv1"),
        _ => ("(NONE)", 0, "(NONE)"),
    }
}

/// Whether a version range makes sense (min not above max).
pub fn range_ok(v: Versions) -> bool {
    v.min.as_u16() <= v.max.as_u16()
}

/// The certificates of PEM or DER data (for cafile / capath).
pub fn certs_of(data: &[u8]) -> Vec<Cert> {
    let v = x509::certs_from_bytes(data);
    if !v.is_empty() {
        return v;
    }
    Cert::from_der(data).map(|c| vec![c]).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use purecrypto::ec::Ed25519PrivateKey;
    use purecrypto::x509::{CertSigner, Certificate, DistinguishedName, Time, Validity};
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::rc::Rc;

    /// One direction of an in-memory, non-blocking transport.
    type Pipe = Rc<RefCell<VecDeque<u8>>>;

    struct Mem {
        rx: Pipe,
        tx: Pipe,
    }

    impl Io for Mem {
        fn recv(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            let mut q = self.rx.borrow_mut();
            if q.is_empty() {
                return Err(io::Error::from(io::ErrorKind::WouldBlock));
            }
            let n = buf.len().min(q.len());
            for (i, b) in q.drain(..n).enumerate() {
                buf[i] = b;
            }
            Ok(n)
        }
        fn send(&mut self, data: &[u8]) -> io::Result<usize> {
            self.tx.borrow_mut().extend(data);
            Ok(data.len())
        }
    }

    fn pair() -> (Mem, Mem) {
        let a: Pipe = Rc::default();
        let b: Pipe = Rc::default();
        (
            Mem {
                rx: a.clone(),
                tx: b.clone(),
            },
            Mem { rx: b, tx: a },
        )
    }

    fn identity(name: &str, seed: u8) -> Identity {
        let key = Ed25519PrivateKey::from_bytes([seed; 32]);
        let validity = Validity::new(
            Time::utc(2024, 1, 1, 0, 0, 0),
            Time::utc(2044, 1, 1, 0, 0, 0),
        );
        let cert = Certificate::self_signed_general(
            &CertSigner::Ed25519(&key),
            &DistinguishedName::common_name(name),
            &validity,
            1,
            false,
            &[name],
        )
        .unwrap();
        Identity {
            chain: vec![cert.to_der().to_vec()],
            key: SigningKey::Ed25519(key),
        }
    }

    fn all() -> Versions {
        Versions::from_mask(proto::TLS1_2 | proto::TLS1_3).unwrap()
    }

    /// Drives both handshakes to completion (or the first error).
    fn connect(
        c: &mut Session,
        s: &mut Session,
        ci: &mut Mem,
        si: &mut Mem,
    ) -> Result<(), TlsError> {
        let (mut cd, mut sd) = (false, false);
        for _ in 0..50 {
            if !cd {
                cd = c.handshake(ci)?;
            }
            if !sd {
                sd = s.handshake(si)?;
            }
            if cd && sd {
                return Ok(());
            }
        }
        panic!("handshake did not complete");
    }

    #[test]
    fn loopback_with_sni_selection() {
        for (mask, want) in [(proto::TLS1_3, "TLSv1.3"), (proto::TLS1_2, "TLSv1.2")] {
            let (mut ci, mut si) = pair();
            let v = Versions::from_mask(mask).unwrap();
            let mut c = Session::client(ClientConfig {
                sni: Some("b.example".into()),
                versions: v,
                alpn: vec![b"h2".to_vec()],
                identity: None,
            })
            .unwrap();
            let mut s = Session::server(ServerConfig {
                identity: Some(identity("default.example", 1)),
                sni_identities: vec![
                    ("a.example".into(), identity("a.example", 2)),
                    ("*.example".into(), identity("wild.example", 3)),
                ],
                versions: all(),
                alpn: vec![b"h2".to_vec()],
                client_roots: None,
            });
            connect(&mut c, &mut s, &mut ci, &mut si).unwrap();
            assert_eq!(c.protocol(), want);
            assert_eq!(s.requested_server_name(), Some("b.example"));
            assert_eq!(c.alpn().as_deref(), Some(&b"h2"[..]));
            // The wildcard entry served "b.example".
            assert_eq!(
                c.peer_certificates()[0],
                identity("wild.example", 3).chain[0]
            );
            // Application data both ways, larger than one record.
            let big = vec![0x5au8; 40_000];
            assert_eq!(c.write(&mut ci, &big).unwrap(), big.len());
            let mut got = Vec::new();
            let mut buf = [0u8; 4096];
            while got.len() < big.len() {
                match s.read(&mut si, &mut buf) {
                    Ok(n) => got.extend_from_slice(&buf[..n]),
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => {}
                    Err(e) => panic!("{e}"),
                }
            }
            assert_eq!(got, big);
            s.write(&mut si, b"pong").unwrap();
            let n = loop {
                match c.read(&mut ci, &mut buf) {
                    Ok(n) => break n,
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => {}
                    Err(e) => panic!("{e}"),
                }
            };
            assert_eq!(&buf[..n], b"pong");
            c.close(&mut ci);
            assert_eq!(s.read(&mut si, &mut buf).unwrap(), 0);
            assert!(s.peer_closed());
        }
    }

    #[test]
    fn version_mismatch_fails() {
        let (mut ci, mut si) = pair();
        let mut c = Session::client(ClientConfig {
            sni: None,
            versions: Versions::from_mask(proto::TLS1_3).unwrap(),
            alpn: Vec::new(),
            identity: None,
        })
        .unwrap();
        let mut s = Session::server(ServerConfig {
            identity: Some(identity("x.example", 4)),
            sni_identities: Vec::new(),
            versions: Versions::from_mask(proto::TLS1_2).unwrap(),
            alpn: Vec::new(),
            client_roots: None,
        });
        assert!(connect(&mut c, &mut s, &mut ci, &mut si).is_err());
    }

    #[test]
    fn version_masks() {
        let v = Versions::from_mask(proto::TLS1_0 | proto::TLS1_2).unwrap();
        assert_eq!(v.min, ProtocolVersion::TLSv1_0);
        assert_eq!(v.max, ProtocolVersion::TLSv1_2);
        assert!(Versions::from_mask(proto::SSLV3).is_none());
        assert_eq!(
            suite_info(0x1302),
            ("TLS_AES_256_GCM_SHA384", 256, "TLSv1.3")
        );
        assert!(host_matches("*.php.net", "uk.php.net"));
        assert!(!host_matches("*.php.net", "php.net"));
        assert!(host_matches("CS.php.net", "cs.PHP.net"));
    }
}
