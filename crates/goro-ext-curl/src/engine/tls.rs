//! TLS client (placeholder until the purecrypto layer lands)

use std::path::PathBuf;

#[derive(Clone, Debug, Default)]
pub struct TlsConfig {
    pub verify_peer: bool,
    pub verify_host: bool,
    pub verify_status: bool,
    pub cainfo: Option<PathBuf>,
    pub capath: Option<PathBuf>,
    pub ca_blob: Option<Vec<u8>>,
    pub cert: Option<Vec<u8>>,
    pub cert_type: Option<String>,
    pub cert_blob: Option<Vec<u8>>,
    pub key: Option<PathBuf>,
    pub key_type: Option<String>,
    pub key_blob: Option<Vec<u8>>,
    pub key_passwd: Option<Vec<u8>>,
    pub issuercert: Option<PathBuf>,
    pub issuercert_blob: Option<Vec<u8>>,
    pub crlfile: Option<PathBuf>,
    pub version_min: i64,
    pub version_max: i64,
    pub cipher_list: Option<String>,
    pub tls13_ciphers: Option<String>,
    pub curves: Option<String>,
    pub signature_algorithms: Option<String>,
    pub pinned_pubkey: Option<String>,
    pub ssl_options: i64,
    pub certinfo: bool,
    pub alpn: Vec<String>,
    pub sessionid_cache: bool,
}

pub struct TlsSession {}

impl TlsSession {
    pub fn new(_cfg: &TlsConfig, _hostname: &str, _port: u16) -> Result<TlsSession, (u32, String)> {
        Err((35, "TLS not available".into()))
    }
    pub fn feed(&mut self, _wire: &[u8]) -> Result<(), (u32, String)> {
        Ok(())
    }
    pub fn take_outgoing(&mut self) -> Vec<u8> {
        Vec::new()
    }
    pub fn handshake(&mut self) -> Result<bool, (u32, String)> {
        Ok(true)
    }
    pub fn take_info_lines(&mut self) -> Vec<String> {
        Vec::new()
    }
    pub fn send(&mut self, _plain: &[u8]) -> Result<(), (u32, String)> {
        Ok(())
    }
    pub fn recv(&mut self) -> Result<Vec<u8>, (u32, String)> {
        Ok(Vec::new())
    }
    pub fn peer_closed(&self) -> bool {
        false
    }
    pub fn close(&mut self) {}
    pub fn verify_result(&self) -> i64 {
        0
    }
    pub fn certinfo(&self) -> Vec<Vec<Vec<u8>>> {
        Vec::new()
    }
    pub fn negotiated_alpn(&self) -> Option<String> {
        None
    }
}

pub fn default_cainfo() -> Option<String> {
    Some("/etc/ssl/certs/ca-certificates.crt".into())
}

pub fn default_capath() -> Option<String> {
    Some("/etc/ssl/certs".into())
}

pub fn backend_version() -> String {
    "purecrypto/0.10.1".into()
}
