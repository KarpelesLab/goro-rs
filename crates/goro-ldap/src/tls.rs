//! TLS transport over a socket, via purecrypto (mirrors libldap's tls_o.c
//! behaviour as PHP uses it: a synchronous handshake with the require-cert
//! policy deciding verification).

use std::os::fd::{AsRawFd, OwnedFd, RawFd};

use purecrypto::tls::{Config, Connection, HandshakeStatus, ProtocolVersion, RootCertStore};

use crate::consts::*;

/// How the client verifies the server certificate (LDAP_OPT_X_TLS_*).
pub struct TlsClientConfig {
    pub require_cert: i32,
    pub cacertfile: Option<String>,
    pub protocol_min: i32,
    pub protocol_max: i32,
    pub server_name: Option<String>,
}

fn map_proto(p: i32) -> Option<ProtocolVersion> {
    // purecrypto only offers TLS 1.2 and 1.3; clamp lower requests up.
    match p {
        x if x == LDAP_OPT_X_TLS_PROTOCOL_TLS1_0 => Some(ProtocolVersion::TLSv1_2),
        x if x == LDAP_OPT_X_TLS_PROTOCOL_TLS1_1 => Some(ProtocolVersion::TLSv1_2),
        x if x == LDAP_OPT_X_TLS_PROTOCOL_TLS1_2 => Some(ProtocolVersion::TLSv1_2),
        x if x == LDAP_OPT_X_TLS_PROTOCOL_TLS1_3 => Some(ProtocolVersion::TLSv1_3),
        _ => None,
    }
}

/// Build a server Config from a PEM cert chain and a PKCS#8 DER private key.
pub fn server_config(cert_chain_der: Vec<Vec<u8>>, key_pkcs8_der: &[u8]) -> Result<Config, String> {
    use purecrypto::tls::SigningKey;
    use purecrypto::x509::{AnyPrivateKey, Pkcs8ReadOptions};
    let key = match AnyPrivateKey::from_pkcs8_der(key_pkcs8_der, Pkcs8ReadOptions::new())
        .map_err(|e| format!("{:?}", e))?
    {
        AnyPrivateKey::Rsa(r) => SigningKey::Rsa(r),
        AnyPrivateKey::Ecdsa(e) => SigningKey::Ecdsa(e),
        AnyPrivateKey::Ed25519(e) => SigningKey::Ed25519(e),
        AnyPrivateKey::Ed448(e) => SigningKey::Ed448(e),
        _ => return Err("unsupported key type".to_string()),
    };
    let cfg = Config::builder()
        .rng(std::sync::Arc::new(purecrypto::rng::OsRng))
        .tls_only()
        .identity(cert_chain_der, key)
        .build();
    Ok(cfg)
}

/// An established TLS session wrapping a socket fd.
pub struct TlsStream {
    pub fd: OwnedFd,
    pub tls: Connection,
    plain: Vec<u8>,
    ppos: usize,
}

impl TlsStream {
    pub fn as_raw_fd(&self) -> RawFd {
        self.fd.as_raw_fd()
    }

    /// Do a blocking client handshake on `fd`.
    pub fn connect(fd: OwnedFd, cfg: &TlsClientConfig) -> Result<TlsStream, String> {
        let verify = !matches!(
            cfg.require_cert,
            LDAP_OPT_X_TLS_NEVER | LDAP_OPT_X_TLS_ALLOW
        );
        let mut roots = RootCertStore::new();
        let mut have_roots = false;
        if let Some(f) = &cfg.cacertfile {
            if let Ok(pem) = std::fs::read_to_string(f) {
                if roots.add_pem(&pem).is_ok() {
                    have_roots = true;
                }
            }
        }
        let suites: [u16; 9] = [
            0x1302, 0x1303, 0x1301, 0xC02C, 0xC030, 0xCCA9, 0xCCA8, 0xC02B, 0xC02F,
        ];
        let mut b = Config::builder()
            .rng(std::sync::Arc::new(purecrypto::rng::OsRng))
            .tls_only()
            .roots(roots)
            .cipher_suites(&suites)
            .verify_certificates(verify && have_roots);
        if let Some(min) = map_proto(cfg.protocol_min) {
            b = b.min_version(min);
        }
        if let Some(max) = map_proto(cfg.protocol_max) {
            b = b.max_version(max);
        }
        if verify {
            if let Some(name) = &cfg.server_name {
                b = b.server_name(name.clone());
            }
        } else if let Some(name) = &cfg.server_name {
            b = b.server_name(name.clone());
        }
        let config = b.build();
        // require-cert HARD/DEMAND/TRY with no usable roots must fail
        if verify && !have_roots {
            return Err("Can't contact LDAP server".to_string());
        }
        let tls = Connection::client(&config).map_err(|e| format!("{:?}", e))?;
        Self::drive(fd, tls)
    }

    /// Do a blocking server handshake on `fd`.
    pub fn accept(fd: OwnedFd, config: &Config) -> Result<TlsStream, String> {
        let tls = Connection::server(config).map_err(|e| format!("{:?}", e))?;
        Self::drive(fd, tls)
    }

    fn drive(fd: OwnedFd, mut tls: Connection) -> Result<TlsStream, String> {
        let raw = fd.as_raw_fd();
        set_nonblock(raw, false);
        let mut wire = [0u8; 16384];
        loop {
            if let Ok(out) = tls.pop() {
                if !out.is_empty() && !send_all(raw, &out) {
                    return Err("SSL connection failed".to_string());
                }
            }
            match tls.handshake() {
                Ok(HandshakeStatus::Complete) => break,
                Ok(HandshakeStatus::WantWrite) => continue,
                Ok(HandshakeStatus::WantRead) => {
                    let n = unsafe {
                        libc::recv(raw, wire.as_mut_ptr() as *mut libc::c_void, wire.len(), 0)
                    };
                    if n <= 0 {
                        return Err("SSL handshake EOF".to_string());
                    }
                    if tls.feed(&wire[..n as usize]).is_err() {
                        return Err("SSL handshake error".to_string());
                    }
                }
                Err(e) => return Err(format!("{:?}", e)),
            }
        }
        // flush any final handshake output
        if let Ok(out) = tls.pop() {
            if !out.is_empty() {
                send_all(raw, &out);
            }
        }
        Ok(TlsStream {
            fd,
            tls,
            plain: Vec::new(),
            ppos: 0,
        })
    }

    /// Blocking read of plaintext.
    pub fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let raw = self.fd.as_raw_fd();
        loop {
            if self.ppos < self.plain.len() {
                let n = (self.plain.len() - self.ppos).min(buf.len());
                buf[..n].copy_from_slice(&self.plain[self.ppos..self.ppos + n]);
                self.ppos += n;
                if self.ppos == self.plain.len() {
                    self.plain.clear();
                    self.ppos = 0;
                }
                return Ok(n);
            }
            if let Ok(data) = self.tls.recv() {
                if !data.is_empty() {
                    self.plain = data;
                    self.ppos = 0;
                    continue;
                }
            }
            if self.tls.received_close_notify() {
                return Ok(0);
            }
            let mut wire = [0u8; 16384];
            let n =
                unsafe { libc::recv(raw, wire.as_mut_ptr() as *mut libc::c_void, wire.len(), 0) };
            if n < 0 {
                return Err(std::io::Error::last_os_error());
            }
            if n == 0 {
                return Ok(0);
            }
            if self.tls.feed(&wire[..n as usize]).is_err() {
                return Err(std::io::Error::other("tls"));
            }
            if let Ok(out) = self.tls.pop() {
                if !out.is_empty() {
                    send_all(raw, &out);
                }
            }
        }
    }

    /// Blocking write of plaintext.
    pub fn write_all(&mut self, data: &[u8]) -> std::io::Result<()> {
        let raw = self.fd.as_raw_fd();
        if self.tls.send(data).is_err() {
            return Err(std::io::Error::other("tls"));
        }
        while let Ok(out) = self.tls.pop() {
            if out.is_empty() {
                break;
            }
            if !send_all(raw, &out) {
                return Err(std::io::Error::other("send"));
            }
        }
        Ok(())
    }

    pub fn close(&mut self) {
        let _ = self.tls.close();
        if let Ok(out) = self.tls.pop() {
            if !out.is_empty() {
                send_all(self.fd.as_raw_fd(), &out);
            }
        }
    }
}

pub fn set_nonblock(fd: RawFd, nb: bool) {
    unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFL);
        if flags < 0 {
            return;
        }
        let new = if nb {
            flags | libc::O_NONBLOCK
        } else {
            flags & !libc::O_NONBLOCK
        };
        libc::fcntl(fd, libc::F_SETFL, new);
    }
}

fn send_all(fd: RawFd, data: &[u8]) -> bool {
    let mut off = 0;
    while off < data.len() {
        let n = unsafe {
            libc::send(
                fd,
                data[off..].as_ptr() as *const libc::c_void,
                data.len() - off,
                crate::msg_nosignal(),
            )
        };
        if n <= 0 {
            let e = std::io::Error::last_os_error();
            if e.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return false;
        }
        off += n as usize;
    }
    true
}
