//! TLS on socket streams: ssl://, tls://, tlsv1.x:// and
//! stream_socket_enable_crypto() (PHP manual: SSL context options,
//! "SSL & TLS" transports), on goro-ext-openssl's TLS sessions.
//!
//! The context options, their defaults and the warnings (peer name and
//! fingerprint checks, certificate loading, "Failed to enable crypto")
//! follow PHP 8.5 as the expected output of ext/openssl/tests shows it
//! (bug54992, bug65729, bug68920, san_peer_matching, peer_verification,
//! gh9310, gh19245, stream_security_level...).

use goro_ext_openssl::tls::{
    ClientConfig, Identity, IdentityError, ServerConfig, Session, TlsError, Versions, proto,
};
use goro_ext_openssl::x509::{Cert, GeneralName};
use goro_runtime::{Key, PhpArray, PhpStr, Value};

use crate::builtins::sockets::{Sock, context_option, set_context_option};
use crate::func::Args;
use crate::vm::{R, Vm};

/// The outcome of enabling crypto.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Enabled {
    Yes,
    /// A non-blocking handshake that would block (enable_crypto() gives 0).
    Pending,
    No,
}

fn opt(s: &Sock, name: &str) -> Option<Value> {
    s.ctx.as_ref().and_then(|c| context_option(c, "ssl", name))
}

fn opt_bool(vm: &mut Vm, s: &Sock, name: &str, default: bool) -> bool {
    match opt(s, name) {
        Some(v) => vm.to_bool(&v),
        None => default,
    }
}

fn opt_str(s: &Sock, name: &str) -> Option<PhpStr> {
    match opt(s, name)? {
        Value::Str(p) => Some(p),
        Value::Null => None,
        v => Some(crate::builtins::sockets::qstr(&v)),
    }
}

fn opt_int(s: &Sock, name: &str) -> Option<i64> {
    opt(s, name).map(|v| crate::builtins::sockets::qint(&v))
}

/// Reads a whole file through the stream layer (cafile may be any
/// stream URL; test bug65538_001.phpt).
fn read_file(vm: &mut Vm, a: &Args, path: &[u8]) -> R<Option<Vec<u8>>> {
    let Some(mut st) = crate::builtins::file::open_stream(vm, a, path, b"rb", true)? else {
        return Ok(None);
    };
    let mut out = Vec::new();
    let mut buf = vec![0u8; 8192];
    loop {
        match st.read_some(vm, &mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => out.extend_from_slice(&buf[..n]),
        }
    }
    Ok(Some(out))
}

/// Trust anchors from cafile and capath.
fn trust_anchors(vm: &mut Vm, a: &Args, s: &Sock) -> R<Vec<Cert>> {
    let mut out = Vec::new();
    let cafile =
        opt_str(s, "cafile").or_else(|| vm.get_ini(b"openssl.cafile").filter(|v| !v.is_empty()));
    if let Some(f) = cafile
        && let Some(d) = read_file(vm, a, f.as_bytes())?
    {
        out.extend(goro_ext_openssl::tls::certs_of(&d));
    }
    let capath =
        opt_str(s, "capath").or_else(|| vm.get_ini(b"openssl.capath").filter(|v| !v.is_empty()));
    if let Some(p) = capath
        && let Ok(rd) = std::fs::read_dir(p.to_string_lossy())
    {
        for e in rd.flatten() {
            if let Ok(d) = std::fs::read(e.path()) {
                out.extend(goro_ext_openssl::tls::certs_of(&d));
            }
        }
    }
    Ok(out)
}

/// The protocol range for a crypto method, after the context's
/// security_level (OpenSSL 3 refuses TLS 1.0 and 1.1 above level 0) and
/// min_proto_version / max_proto_version.
fn versions(s: &Sock, method: i64) -> Option<Versions> {
    let mut mask = method & (proto::TLS1_0 | proto::TLS1_1 | proto::TLS1_2 | proto::TLS1_3);
    let level = opt_int(s, "security_level").unwrap_or(1);
    if level > 0 {
        mask &= !(proto::TLS1_0 | proto::TLS1_1);
    }
    if let Some(min) = opt_int(s, "min_proto_version") {
        mask &= !(min - 1);
    }
    if let Some(max) = opt_int(s, "max_proto_version")
        && max > 0
    {
        mask &= (max << 1) - 1;
    }
    Versions::from_mask(mask)
}

fn alpn(s: &Sock) -> Vec<Vec<u8>> {
    match opt_str(s, "alpn_protocols") {
        Some(p) => p
            .as_bytes()
            .split(|&c| c == b',')
            .filter(|x| !x.is_empty())
            .map(|x| x.to_vec())
            .collect(),
        None => Vec::new(),
    }
}

/// Checks a local_cert / local_pk path as PHP does before loading it:
/// no NUL bytes, inside open_basedir, resolvable. `what` is "certificate
/// file" or "private key file".
fn local_path(vm: &mut Vm, a: &Args, opt_name: &str, path: &[u8], what: &str) -> R<Option<String>> {
    let (path, nul) = match path.iter().position(|&b| b == 0) {
        Some(p) => (&path[..p], true),
        None => (path, false),
    };
    let shown = String::from_utf8_lossy(path).into_owned();
    if nul {
        vm.fwarn(
            a,
            &format!(
                "Path for {opt_name} in ssl stream context option must not contain any null bytes"
            ),
        )?;
    } else if !vm.basedir_allows(path) {
        basedir_warning(vm, a, &shown)?;
    } else if let Ok(p) = std::fs::canonicalize(&shown) {
        return Ok(Some(p.to_string_lossy().into_owned()));
    }
    vm.fwarn(a, &format!("Unable to get real path of {what} `{shown}'"))?;
    Ok(None)
}

fn basedir_warning(vm: &mut Vm, a: &Args, shown: &str) -> R<()> {
    let bd = vm
        .get_ini(b"open_basedir")
        .map(|v| v.to_string_lossy())
        .unwrap_or_default();
    vm.fwarn(
        a,
        &format!(
            "open_basedir restriction in effect. File({shown}) is not within the allowed path(s): ({bd})"
        ),
    )
}

fn load_identity(
    vm: &mut Vm,
    a: &Args,
    cert: &[u8],
    key: Option<&[u8]>,
    pass: Option<&[u8]>,
    cert_shown: &str,
) -> R<Option<Identity>> {
    match Identity::from_pem(cert, key, pass) {
        Ok(i) => Ok(Some(i)),
        Err(IdentityError::NoCertificate) => {
            vm.fwarn(
                a,
                &format!("Unable to set local cert chain file `{cert_shown}'; Check that your cafile/capath settings include details of your certificate and its issuer"),
            )?;
            Ok(None)
        }
        Err(_) => {
            vm.fwarn(a, &format!("Unable to set private key file `{cert_shown}'"))?;
            Ok(None)
        }
    }
}

/// local_cert / local_pk / passphrase of the context.
fn main_identity(vm: &mut Vm, a: &Args, s: &Sock) -> R<Result<Option<Identity>, ()>> {
    let Some(cert) = opt_str(s, "local_cert") else {
        return Ok(Ok(None));
    };
    let Some(cpath) = local_path(vm, a, "local_cert", cert.as_bytes(), "certificate file")? else {
        return Ok(Err(()));
    };
    let key = match opt_str(s, "local_pk") {
        Some(k) => match local_path(vm, a, "local_pk", k.as_bytes(), "private key file")? {
            Some(p) => Some(p),
            None => return Ok(Err(())),
        },
        None => None,
    };
    let pass = opt_str(s, "passphrase");
    let cdata = std::fs::read(&cpath).unwrap_or_default();
    let kdata = key.as_ref().map(|k| std::fs::read(k).unwrap_or_default());
    Ok(
        match load_identity(
            vm,
            a,
            &cdata,
            kdata.as_deref(),
            pass.as_ref().map(|p| p.as_bytes()),
            &cpath,
        )? {
            Some(i) => Ok(Some(i)),
            None => Err(()),
        },
    )
}

/// SNI_server_certs: host => combined PEM path, or host => [local_cert,
/// local_pk] (PHP manual: SSL context options).
fn sni_identities(vm: &mut Vm, a: &Args, s: &Sock) -> R<Result<Vec<(String, Identity)>, ()>> {
    let mut out = Vec::new();
    if !opt_bool(vm, s, "SNI_enabled", true) {
        return Ok(Ok(out));
    }
    let Some(v) = opt(s, "SNI_server_certs") else {
        return Ok(Ok(out));
    };
    let Value::Array(arr) = v else {
        vm.fwarn(
            a,
            "SNI_server_certs requires an array mapping host names to cert paths",
        )?;
        return Ok(Err(()));
    };
    let pass = opt_str(s, "passphrase");
    for (k, v) in arr.iter() {
        let host = match k {
            goro_runtime::KeyRef::Str(h) => h.to_string_lossy(),
            goro_runtime::KeyRef::Int(_) => {
                vm.fwarn(a, "SNI_server_certs array requires string host name keys")?;
                return Ok(Err(()));
            }
        };
        let (cert, key) = match v.deref() {
            Value::Str(p) => (p, None),
            Value::Array(m) => {
                let c = m.get_str(&PhpStr::from("local_cert")).map(|v| v.deref());
                let k = m.get_str(&PhpStr::from("local_pk")).map(|v| v.deref());
                match (c, k) {
                    (Some(Value::Str(c)), Some(Value::Str(k))) => (c, Some(k)),
                    _ => {
                        vm.fwarn(
                            a,
                            "local_cert and local_pk must be set for each SNI_server_certs entry",
                        )?;
                        return Ok(Err(()));
                    }
                }
            }
            _ => {
                vm.fwarn(
                    a,
                    "SNI_server_certs host cert must be a string or an array with local_cert and local_pk",
                )?;
                return Ok(Err(()));
            }
        };
        let cshown = cert.to_string_lossy();
        if !vm.basedir_allows(cert.as_bytes()) {
            basedir_warning(vm, a, &cshown)?;
            vm.fwarn(
                a,
                &if key.is_none() {
                    format!("Failed setting local cert chain file `{cshown}'; file not found")
                } else {
                    format!("Failed setting local cert chain file `{cshown}'; could not open file")
                },
            )?;
            return Ok(Err(()));
        }
        let Ok(cdata) = std::fs::read(cert.to_string_lossy()) else {
            vm.fwarn(
                a,
                &format!("Failed setting local cert chain file `{cshown}'; file not found"),
            )?;
            return Ok(Err(()));
        };
        let kdata = match &key {
            Some(k) => {
                let kshown = k.to_string_lossy();
                if !vm.basedir_allows(k.as_bytes()) {
                    basedir_warning(vm, a, &kshown)?;
                    vm.fwarn(
                        a,
                        &format!(
                            "Failed setting local private key file `{kshown}';  could not open file"
                        ),
                    )?;
                    return Ok(Err(()));
                }
                match std::fs::read(&kshown) {
                    Ok(d) => Some(d),
                    Err(_) => {
                        vm.fwarn(
                            a,
                            &format!(
                                "Failed setting local private key file `{kshown}';  could not open file"
                            ),
                        )?;
                        return Ok(Err(()));
                    }
                }
            }
            None => None,
        };
        match load_identity(
            vm,
            a,
            &cdata,
            kdata.as_deref(),
            pass.as_ref().map(|p| p.as_bytes()),
            &cshown,
        )? {
            Some(i) => out.push((host, i)),
            None => return Ok(Err(())),
        }
    }
    Ok(Ok(out))
}

fn is_ip(h: &[u8]) -> bool {
    std::str::from_utf8(h).is_ok_and(|s| {
        s.trim_start_matches('[')
            .trim_end_matches(']')
            .parse::<std::net::IpAddr>()
            .is_ok()
    })
}

/// Enables crypto on a socket (a pending non-blocking handshake
/// continues).
pub(crate) fn enable(
    vm: &mut Vm,
    a: &Args,
    s: &mut Sock,
    method: Option<i64>,
    server: bool,
) -> R<Enabled> {
    vm.out.flush();
    if s.tls.is_none() {
        let m = opt_int(s, "crypto_method")
            .or(method)
            .unwrap_or(if s.transport_method != 0 {
                s.transport_method
            } else if server {
                120
            } else {
                121
            });
        let Some(v) = versions(s, m) else {
            vm.fwarn(a, "Failed to enable crypto")?;
            return Ok(Enabled::No);
        };
        let session = if server {
            let sni = match sni_identities(vm, a, s)? {
                Ok(x) => x,
                Err(()) => {
                    vm.fwarn(a, "Failed to enable crypto")?;
                    return Ok(Enabled::No);
                }
            };
            let id = match main_identity(vm, a, s)? {
                Ok(x) => x,
                Err(()) => {
                    vm.fwarn(a, "Failed to enable crypto")?;
                    return Ok(Enabled::No);
                }
            };
            let client_roots = if opt_bool(vm, s, "verify_peer", false) {
                Some(
                    trust_anchors(vm, a, s)?
                        .into_iter()
                        .map(|c| c.der)
                        .collect(),
                )
            } else {
                None
            };
            Session::server(ServerConfig {
                identity: id,
                sni_identities: sni,
                versions: v,
                alpn: alpn(s),
                client_roots,
            })
        } else {
            let peer = opt_str(s, "peer_name")
                .map(|p| p.as_bytes().to_vec())
                .unwrap_or_else(|| s.host.clone());
            let sni = if opt_bool(vm, s, "SNI_enabled", true) && !peer.is_empty() && !is_ip(&peer) {
                Some(String::from_utf8_lossy(&peer).into_owned())
            } else {
                None
            };
            let identity = match main_identity(vm, a, s)? {
                Ok(x) => x,
                Err(()) => {
                    vm.fwarn(a, "Failed to enable crypto")?;
                    return Ok(Enabled::No);
                }
            };
            match Session::client(ClientConfig {
                sni,
                versions: v,
                alpn: alpn(s),
                identity,
            }) {
                Ok(x) => x,
                Err(e) => {
                    ssl_failure(vm, a, &e)?;
                    vm.fwarn(a, "Failed to enable crypto")?;
                    return Ok(Enabled::No);
                }
            }
        };
        let mut session = session;
        let ahead = s.take_read_ahead();
        if !ahead.is_empty() {
            session.push_input(&ahead);
        }
        s.tls = Some(Box::new(session));
    }
    let r = {
        let (mut io, tls) = s.parts();
        tls.as_mut().expect("session").handshake(&mut io)
    };
    match r {
        Ok(true) => {}
        Ok(false) => return Ok(Enabled::Pending),
        Err(e) => {
            s.tls = None;
            ssl_failure(vm, a, &e)?;
            vm.fwarn(a, "Failed to enable crypto")?;
            return Ok(Enabled::No);
        }
    }
    let ok = verify(vm, a, s, server)?;
    if !ok {
        let (mut io, tls) = s.parts();
        if let Some(t) = tls {
            t.close(&mut io);
        }
        s.tls = None;
        vm.fwarn(a, "Failed to enable crypto")?;
        return Ok(Enabled::No);
    }
    s.crypto_on = true;
    Ok(Enabled::Yes)
}

/// The warning for a failed handshake (OpenSSL's error queue as PHP
/// prints it). Timeouts and closed connections have their own texts.
fn ssl_failure(vm: &mut Vm, a: &Args, e: &TlsError) -> R<()> {
    match e {
        TlsError::Io(err) if err.kind() == std::io::ErrorKind::TimedOut => Ok(()),
        TlsError::Io(_) | TlsError::Eof => vm.fwarn(
            a,
            "SSL operation failed with code 1. OpenSSL Error messages:\nerror:0A000126:SSL routines::unexpected eof while reading",
        ),
        TlsError::NoCertificate => vm.fwarn(
            a,
            "SSL operation failed with code 1. OpenSSL Error messages:\nerror:0A0000C1:SSL routines::no shared cipher",
        ),
        TlsError::Protocol(p) => {
            let reason = if p.contains("UnsupportedVersion") || p.contains("ProtocolVersion") {
                "0A000102:SSL routines::unsupported protocol"
            } else if p.contains("BadCertificate") || p.contains("CertificateRequired") {
                "0A000412:SSL routines::ssl/tls alert bad certificate"
            } else {
                "0A000410:SSL routines::ssl/tls alert handshake failure"
            };
            vm.fwarn(
                a,
                &format!("SSL operation failed with code 1. OpenSSL Error messages:\nerror:{reason}"),
            )
        }
    }
}

const VERIFY_FAILED: &str = "SSL operation failed with code 1. OpenSSL Error messages:\nerror:0A000086:SSL routines::certificate verify failed";

/// Peer checks after the handshake (PHP manual: SSL context options
/// verify_peer, verify_peer_name, peer_name, allow_self_signed,
/// verify_depth, peer_fingerprint, capture_peer_cert[_chain]).
fn verify(vm: &mut Vm, a: &Args, s: &mut Sock, server: bool) -> R<bool> {
    let chain_der = s
        .tls
        .as_ref()
        .map(|t| t.peer_certificates())
        .unwrap_or_default();
    let chain: Vec<Cert> = chain_der
        .iter()
        .filter_map(|d| Cert::from_der(d).ok())
        .collect();
    // Captured before the checks (test capture_peer_cert_001.phpt).
    if let Some(ctx) = s.ctx.clone() {
        if opt_bool(vm, s, "capture_peer_cert", false)
            && let Some(leaf) = chain_der.first()
        {
            let v = super::cert_value(vm, leaf.clone())?;
            set_context_option(&ctx, "ssl", "peer_certificate", v);
        }
        if opt_bool(vm, s, "capture_peer_cert_chain", false) && !chain_der.is_empty() {
            let mut arr = PhpArray::new();
            for d in &chain_der {
                let _ = arr.push(super::cert_value(vm, d.clone())?);
            }
            set_context_option(&ctx, "ssl", "peer_certificate_chain", Value::Array(arr));
        }
        if opt_bool(vm, s, "capture_session_meta", false) {
            let mut m = PhpArray::new();
            if let Some(t) = &s.tls {
                let (name, bits, ver) = t.cipher();
                m.insert(Key::from("protocol"), Value::from(t.protocol()));
                m.insert(Key::from("cipher_name"), Value::from(name));
                m.insert(Key::from("cipher_bits"), Value::Int(bits));
                m.insert(Key::from("cipher_version"), Value::from(ver));
            }
            set_context_option(&ctx, "ssl", "session_meta", Value::Array(m));
        }
    }
    if server {
        // Client certificates were verified by the TLS engine (verify_peer
        // on a server requests them).
        return Ok(true);
    }
    let verify_peer = opt_bool(vm, s, "verify_peer", true);
    if verify_peer {
        let Some(leaf) = chain.first() else {
            vm.fwarn(a, VERIFY_FAILED)?;
            return Ok(false);
        };
        let anchors = trust_anchors(vm, a, s)?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let r = goro_ext_openssl::verify::verify_chain(leaf, &anchors, &chain[1..], None, now);
        let allow_self = opt_bool(vm, s, "allow_self_signed", false);
        let ok = match r {
            Ok(path) => match opt_int(s, "verify_depth") {
                Some(d) => (path.len() as i64 - 1) <= d,
                None => true,
            },
            Err(goro_ext_openssl::verify::VerifyError::DepthZeroSelfSigned) => allow_self,
            Err(goro_ext_openssl::verify::VerifyError::SelfSignedInChain) => allow_self,
            Err(_) => false,
        };
        // security_level key floors (OpenSSL documentation:
        // SSL_CTX_set_security_level: levels 1, 2, 3, 4, 5 require RSA, DSA
        // and DH keys of 1024, 2048, 3072, 7680 and 15360 bits).
        let level = opt_int(s, "security_level").unwrap_or(1);
        let floor = match level {
            i64::MIN..=0 => 0,
            1 => 1024,
            2 => 2048,
            3 => 3072,
            4 => 7680,
            _ => 15360,
        };
        let small = chain.iter().any(|c| {
            c.public_key().is_ok_and(|k| {
                matches!(
                    k.key_type(),
                    goro_ext_openssl::key::KeyType::Rsa
                        | goro_ext_openssl::key::KeyType::Dsa
                        | goro_ext_openssl::key::KeyType::Dh
                ) && k.bits() < floor
            })
        });
        if !ok || small {
            vm.fwarn(a, VERIFY_FAILED)?;
            return Ok(false);
        }
    }
    if let Some(fp) = opt(s, "peer_fingerprint")
        && !fingerprint_ok(vm, a, &fp, chain.first())?
    {
        return Ok(false);
    }
    // The peer name is checked even without verify_peer (test
    // san_peer_matching.phpt).
    if opt_bool(vm, s, "verify_peer_name", true) {
        let name = opt_str(s, "peer_name")
            .map(|p| p.as_bytes().to_vec())
            .unwrap_or_else(|| s.host.clone());
        if let Some(leaf) = chain.first()
            && !name_ok(vm, a, leaf, &name)?
        {
            return Ok(false);
        }
    }
    Ok(true)
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// peer_fingerprint: a string (MD5 by its length 32, SHA-1 by 40) or an
/// [algo => fingerprint] array.
fn fingerprint_ok(vm: &mut Vm, a: &Args, fp: &Value, leaf: Option<&Cert>) -> R<bool> {
    use goro_ext_openssl::digest::Md;
    let check = |algo: &str, want: &[u8]| -> Option<bool> {
        let md = Md::by_name(algo)?;
        let have = hex(&leaf?.fingerprint(md));
        Some(have.eq_ignore_ascii_case(&String::from_utf8_lossy(want)))
    };
    let ok = match fp {
        Value::Str(f) => {
            let algo = match f.len() {
                32 => Some("md5"),
                40 => Some("sha1"),
                _ => None,
            };
            algo.and_then(|al| check(al, f.as_bytes())).unwrap_or(false)
        }
        Value::Array(arr) => {
            let mut all = !arr.is_empty();
            let mut invalid = arr.is_empty();
            for (k, v) in arr.iter() {
                let (goro_runtime::KeyRef::Str(algo), Value::Str(f)) = (k, v.deref()) else {
                    invalid = true;
                    all = false;
                    break;
                };
                match check(&algo.to_string_lossy(), f.as_bytes()) {
                    Some(true) => {}
                    Some(false) => all = false,
                    None => {
                        invalid = true;
                        all = false;
                        break;
                    }
                }
            }
            if invalid {
                vm.fwarn(
                    a,
                    "Invalid peer_fingerprint array; [algo => fingerprint] form required",
                )?;
            }
            all
        }
        _ => {
            vm.fwarn(a, "Expected peer fingerprint must be a string or an array")?;
            return Ok(false);
        }
    };
    if !ok {
        vm.fwarn(a, "peer_fingerprint match failure")?;
    }
    Ok(ok)
}

/// RFC 6125-style matching with OpenSSL's wildcard rule: `*` stands for
/// one whole leftmost label. A trailing dot of the name is ignored (test
/// bug68265.phpt).
fn wildcard_match(pattern: &[u8], name: &[u8]) -> bool {
    let mut p = pattern.to_ascii_lowercase();
    if p.last() == Some(&b'.') {
        p.pop();
    }
    let mut n = name.to_ascii_lowercase();
    if n.last() == Some(&b'.') {
        n.pop();
    }
    if p == n {
        return true;
    }
    if let Some(rest) = p.strip_prefix(b"*.")
        && let Some(dot) = n.iter().position(|&c| c == b'.')
    {
        return dot > 0 && &n[dot + 1..] == rest;
    }
    false
}

/// The peer name check: subjectAltName DNS and IP entries, then the
/// subject CN (PHP's messages).
fn name_ok(vm: &mut Vm, a: &Args, leaf: &Cert, name: &[u8]) -> R<bool> {
    let ip: Option<std::net::IpAddr> = std::str::from_utf8(name)
        .ok()
        .and_then(|s| s.trim_start_matches('[').trim_end_matches(']').parse().ok());
    if let Some(ext) = leaf.ext("2.5.29.17")
        && let Ok(t) = goro_ext_openssl::der::parse(&ext.value)
        && let Ok(names) = goro_ext_openssl::x509::parse_general_names(&t)
    {
        for g in &names {
            match g {
                GeneralName::Dns(d) if ip.is_none() && wildcard_match(d, name) => {
                    return Ok(true);
                }
                GeneralName::Ip(b) => {
                    let matched = match (ip, b.len()) {
                        (Some(std::net::IpAddr::V4(v4)), 4) => v4.octets() == b[..],
                        (Some(std::net::IpAddr::V6(v6)), 16) => v6.octets() == b[..],
                        _ => false,
                    };
                    if matched {
                        return Ok(true);
                    }
                }
                _ => {}
            }
        }
    }
    let cn = leaf
        .subject
        .entries()
        .filter(|e| e.oid == "2.5.4.3")
        .last()
        .map(|e| e.utf8());
    let Some(cn) = cn else {
        vm.fwarn(a, "Unable to locate peer certificate CN")?;
        return Ok(false);
    };
    if wildcard_match(&cn, name) {
        return Ok(true);
    }
    vm.fwarn(
        a,
        &format!(
            "Peer certificate CN=`{}' did not match expected CN=`{}'",
            String::from_utf8_lossy(&cn),
            String::from_utf8_lossy(name)
        ),
    )?;
    Ok(false)
}
