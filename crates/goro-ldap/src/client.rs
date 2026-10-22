//! The LDAP client: an LDAP* session (ldap_initialize + the operation
//! functions libldap exposes to ext/ldap), speaking LDAPv3 over a plain or
//! TLS socket.

use std::net::ToSocketAddrs;
use std::os::fd::{AsRawFd, FromRawFd, IntoRawFd, OwnedFd};
use std::time::Duration;

use crate::ber::{self, Reader, TAG_OCTETSTRING, TAG_SET, Writer};
use crate::consts::*;
use crate::message::{Control, Entry, LdapMessage, Reference};
use crate::tls::{TlsClientConfig, TlsStream};
use crate::url::LdapUrl;

/// The transport socket.
enum Sock {
    Plain(OwnedFd),
    Tls(Box<TlsStream>),
}

impl Sock {
    fn write_all(&mut self, data: &[u8]) -> std::io::Result<()> {
        match self {
            Sock::Plain(f) => {
                let fd = f.as_raw_fd();
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
                    if n < 0 {
                        let e = std::io::Error::last_os_error();
                        if e.kind() == std::io::ErrorKind::Interrupted {
                            continue;
                        }
                        return Err(e);
                    }
                    off += n as usize;
                }
                Ok(())
            }
            Sock::Tls(t) => t.write_all(data),
        }
    }
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            Sock::Plain(f) => {
                let fd = f.as_raw_fd();
                loop {
                    let n = unsafe {
                        libc::recv(fd, buf.as_mut_ptr() as *mut libc::c_void, buf.len(), 0)
                    };
                    if n < 0 {
                        let e = std::io::Error::last_os_error();
                        if e.kind() == std::io::ErrorKind::Interrupted {
                            continue;
                        }
                        return Err(e);
                    }
                    return Ok(n as usize);
                }
            }
            Sock::Tls(t) => t.read(buf),
        }
    }
}

/// Per-connection options (a subset of struct ldapoptions).
#[derive(Clone)]
pub struct Options {
    pub version: i32,
    pub deref: i32,
    pub sizelimit: i32,
    pub timelimit: i32,
    pub referrals: bool,
    pub restart: bool,
    pub network_timeout: i64, // seconds; -1 = unset
    pub timeout: i64,         // seconds; -1 = unset
    pub debug_level: i32,
    pub keepalive_idle: i32,
    pub keepalive_probes: i32,
    pub keepalive_interval: i32,
    pub server_controls: Vec<Control>,
    pub client_controls: Vec<Control>,
    // string options
    pub matched_dn: Option<Vec<u8>>,
    pub diagnostic: Option<Vec<u8>>,
    pub host_name: Option<Vec<u8>>,
    pub sasl_mech: Option<Vec<u8>>,
    pub sasl_realm: Option<Vec<u8>>,
    pub sasl_authcid: Option<Vec<u8>>,
    pub sasl_authzid: Option<Vec<u8>>,
    pub sasl_nocanon: bool,
    // TLS options (inherited from the global set at connect)
    pub tls_require_cert: i32,
    pub tls_crlcheck: i32,
    pub tls_protocol_min: i32,
    pub tls_protocol_max: i32,
    pub tls_cacertfile: Option<Vec<u8>>,
    pub tls_cacertdir: Option<Vec<u8>>,
    pub tls_certfile: Option<Vec<u8>>,
    pub tls_keyfile: Option<Vec<u8>>,
    pub tls_cipher_suite: Option<Vec<u8>>,
    pub tls_dhfile: Option<Vec<u8>>,
    pub tls_crlfile: Option<Vec<u8>>,
    pub tls_randfile: Option<Vec<u8>>,
}

impl Default for Options {
    fn default() -> Options {
        Options {
            version: LDAP_VERSION2,
            deref: LDAP_DEREF_NEVER,
            sizelimit: LDAP_NO_LIMIT,
            timelimit: LDAP_NO_LIMIT,
            referrals: true,
            restart: false,
            network_timeout: -1,
            timeout: -1,
            debug_level: 0,
            keepalive_idle: 0,
            keepalive_probes: 0,
            keepalive_interval: 0,
            server_controls: Vec::new(),
            client_controls: Vec::new(),
            matched_dn: None,
            diagnostic: None,
            host_name: None,
            sasl_mech: None,
            sasl_realm: None,
            sasl_authcid: None,
            sasl_authzid: None,
            sasl_nocanon: false,
            tls_require_cert: LDAP_OPT_X_TLS_DEMAND,
            tls_crlcheck: LDAP_OPT_X_TLS_CRL_NONE,
            tls_protocol_min: 0,
            tls_protocol_max: 0,
            tls_cacertfile: None,
            tls_cacertdir: None,
            tls_certfile: None,
            tls_keyfile: None,
            tls_cipher_suite: None,
            tls_dhfile: None,
            tls_crlfile: None,
            tls_randfile: None,
        }
    }
}

/// An LDAP session.
pub struct Conn {
    pub urls: Vec<LdapUrl>,
    pub opts: Options,
    pub errno: i32,
    pub error: Vec<u8>,
    pub matched: Vec<u8>,
    sock: Option<Sock>,
    msgid: i32,
    inbuf: Vec<u8>,
    inpos: usize,
}

/// Simple-bind credential choice.
pub enum Cred<'a> {
    Simple(&'a [u8]),
}

impl Conn {
    pub fn new(urls: Vec<LdapUrl>, opts: Options) -> Conn {
        Conn {
            urls,
            opts,
            errno: LDAP_SUCCESS,
            error: Vec::new(),
            matched: Vec::new(),
            sock: None,
            msgid: 0,
            inbuf: Vec::new(),
            inpos: 0,
        }
    }

    fn next_msgid(&mut self) -> i64 {
        self.msgid += 1;
        self.msgid as i64
    }

    pub fn is_tls(&self) -> bool {
        matches!(self.sock, Some(Sock::Tls(_)))
    }

    /// Open the TCP (and, for ldaps, TLS) connection if not already open.
    fn ensure_open(&mut self) -> i32 {
        if self.sock.is_some() {
            return LDAP_SUCCESS;
        }
        let url = match self.urls.first() {
            Some(u) => u.clone(),
            None => {
                self.errno = LDAP_SERVER_DOWN;
                return LDAP_SERVER_DOWN;
            }
        };
        let host = url.host.clone().unwrap_or_else(|| "localhost".to_string());
        let port = if url.port != 0 {
            url.port as u16
        } else if url.scheme == "ldaps" {
            636
        } else {
            389
        };
        let fd = match self.tcp_connect(&host, port) {
            Some(fd) => fd,
            None => {
                self.errno = LDAP_SERVER_DOWN;
                return LDAP_SERVER_DOWN;
            }
        };
        if url.scheme == "ldaps" {
            let cfg = self.tls_client_config(&host);
            match TlsStream::connect(fd, &cfg) {
                Ok(t) => self.sock = Some(Sock::Tls(Box::new(t))),
                Err(_) => {
                    self.errno = LDAP_SERVER_DOWN;
                    return LDAP_SERVER_DOWN;
                }
            }
        } else {
            self.sock = Some(Sock::Plain(fd));
        }
        LDAP_SUCCESS
    }

    fn tls_client_config(&self, host: &str) -> TlsClientConfig {
        TlsClientConfig {
            require_cert: self.opts.tls_require_cert,
            cacertfile: self
                .opts
                .tls_cacertfile
                .as_ref()
                .map(|b| String::from_utf8_lossy(b).into_owned()),
            protocol_min: self.opts.tls_protocol_min,
            protocol_max: self.opts.tls_protocol_max,
            server_name: Some(host.to_string()),
        }
    }

    fn tcp_connect(&self, host: &str, port: u16) -> Option<OwnedFd> {
        let timeout = if self.opts.network_timeout > 0 {
            Some(Duration::from_secs(self.opts.network_timeout as u64))
        } else {
            None
        };
        let addrs: Vec<std::net::SocketAddr> = (host, port).to_socket_addrs().ok()?.collect();
        for addr in addrs {
            let res = match timeout {
                Some(t) => std::net::TcpStream::connect_timeout(&addr, t),
                None => std::net::TcpStream::connect(addr),
            };
            if let Ok(stream) = res {
                let _ = stream.set_nodelay(true);
                let raw = stream.into_raw_fd();
                return Some(unsafe { OwnedFd::from_raw_fd(raw) });
            }
        }
        None
    }

    /// StartTLS extended operation (RFC 4511), then the handshake.
    pub fn start_tls(&mut self) -> i32 {
        if self.ensure_open() != LDAP_SUCCESS {
            return self.errno;
        }
        if self.is_tls() {
            self.errno = LDAP_OPERATIONS_ERROR;
            return self.errno;
        }
        // extended request: StartTLS OID, no value
        let mut op = Writer::new();
        op.start(LDAP_REQ_EXTENDED);
        op.raw(LDAP_TAG_EXOP_REQ_OID, LDAP_EXOP_START_TLS.as_bytes());
        op.end();
        let body = op.finish();
        let msgid = self.next_msgid();
        if self.write_message(msgid, &body, &[]).is_err() {
            self.errno = LDAP_SERVER_DOWN;
            return self.errno;
        }
        let msg = match self.read_result(LDAP_RES_EXTENDED) {
            Ok(m) => m,
            Err(e) => {
                self.errno = e;
                return e;
            }
        };
        self.errno = msg.rescode;
        if msg.rescode != LDAP_SUCCESS {
            return msg.rescode;
        }
        // handshake over the existing socket
        let host = self
            .urls
            .first()
            .and_then(|u| u.host.clone())
            .unwrap_or_else(|| "localhost".to_string());
        let cfg = self.tls_client_config(&host);
        let sock = self.sock.take();
        if let Some(Sock::Plain(fd)) = sock {
            match TlsStream::connect(fd, &cfg) {
                Ok(t) => {
                    self.sock = Some(Sock::Tls(Box::new(t)));
                    self.errno = LDAP_SUCCESS;
                    LDAP_SUCCESS
                }
                Err(_) => {
                    // connection is unusable now
                    self.errno = LDAP_CONNECT_ERROR;
                    LDAP_CONNECT_ERROR
                }
            }
        } else {
            self.sock = sock;
            self.errno = LDAP_OPERATIONS_ERROR;
            LDAP_OPERATIONS_ERROR
        }
    }

    // ---- message framing ----

    fn write_message(
        &mut self,
        msgid: i64,
        op: &[u8],
        controls: &[Control],
    ) -> std::io::Result<()> {
        let mut w = Writer::new();
        w.start_seq();
        w.int(msgid);
        w.append_encoded(op);
        if !controls.is_empty() {
            w.start(LDAP_TAG_CONTROLS);
            for c in controls {
                w.start_seq();
                w.octets(&c.oid);
                if c.criticality {
                    w.boolean(true);
                }
                if let Some(v) = &c.value {
                    w.octets(v);
                }
                w.end();
            }
            w.end();
        }
        let bytes = w.finish();
        let Some(sock) = self.sock.as_mut() else {
            return Err(std::io::Error::other("closed"));
        };
        sock.write_all(&bytes)
    }

    /// Read one full LDAPMessage SEQUENCE from the socket.
    fn read_raw_message(&mut self) -> Result<Vec<u8>, i32> {
        loop {
            // is a full element buffered?
            let avail = &self.inbuf[self.inpos..];
            match ber::element_len(avail) {
                Some(Ok(total)) => {
                    let msg = avail[..total].to_vec();
                    self.inpos += total;
                    if self.inpos > 4096 && self.inpos * 2 > self.inbuf.len() {
                        self.inbuf.drain(..self.inpos);
                        self.inpos = 0;
                    }
                    return Ok(msg);
                }
                Some(Err(_)) => return Err(LDAP_DECODING_ERROR),
                None => {}
            }
            let mut buf = [0u8; 16384];
            let Some(sock) = self.sock.as_mut() else {
                return Err(LDAP_SERVER_DOWN);
            };
            match sock.read(&mut buf) {
                Ok(0) => return Err(LDAP_SERVER_DOWN),
                Ok(n) => self.inbuf.extend_from_slice(&buf[..n]),
                Err(_) => return Err(LDAP_SERVER_DOWN),
            }
        }
    }

    /// Read messages until the result of the expected type; collect any
    /// search entries/references on the way.
    fn read_result(&mut self, _want: u32) -> Result<LdapMessage, i32> {
        self.read_result_collecting()
    }

    fn read_result_collecting(&mut self) -> Result<LdapMessage, i32> {
        let mut out = LdapMessage::default();
        loop {
            let raw = self.read_raw_message()?;
            let mut r = Reader::new(&raw);
            let mut seq = r.seq().map_err(|_| LDAP_DECODING_ERROR)?;
            let _msgid = seq.int().map_err(|_| LDAP_DECODING_ERROR)?;
            let op_tag = seq.peek_tag().ok_or(LDAP_DECODING_ERROR)?;
            match op_tag {
                LDAP_RES_SEARCH_ENTRY => {
                    let e = parse_entry(&mut seq)?;
                    out.entries.push(e);
                }
                LDAP_RES_SEARCH_REFERENCE => {
                    let refc = seq
                        .enter(LDAP_RES_SEARCH_REFERENCE)
                        .map_err(|_| LDAP_DECODING_ERROR)?;
                    let mut refs = Vec::new();
                    let mut rr = refc;
                    while !rr.is_empty() {
                        refs.push(rr.any().map_err(|_| LDAP_DECODING_ERROR)?.to_vec());
                    }
                    out.references.push(Reference { referrals: refs });
                }
                LDAP_RES_INTERMEDIATE => {
                    // skip intermediate responses
                    let _ = seq.read_raw();
                }
                _ => {
                    // a final response: {resultCode, matchedDN, errorMessage,
                    // [referral]...} plus op-specific trailing fields
                    parse_result_op(op_tag, &mut seq, &mut out)?;
                    // controls on the message
                    if let Ok(Some(cbytes)) = seq.optional(LDAP_TAG_CONTROLS) {
                        out.ctrls = parse_controls(cbytes);
                    }
                    return Ok(out);
                }
            }
            // controls on entry/reference messages are ignored
            let _ = seq.optional(LDAP_TAG_CONTROLS);
        }
    }

    // ---- operations ----

    /// ldap_sasl_bind_s with LDAP_SASL_SIMPLE (ldap_bind).
    pub fn simple_bind(&mut self, dn: &[u8], pw: &[u8]) -> i32 {
        match self.bind_op(dn, pw, &[]) {
            Ok(m) => {
                self.errno = m.rescode;
                m.rescode
            }
            Err(e) => {
                self.errno = e;
                e
            }
        }
    }

    /// ldap_bind_ext (async bind → result object).
    pub fn bind_ext(
        &mut self,
        dn: &[u8],
        pw: &[u8],
        controls: &[Control],
    ) -> Result<LdapMessage, i32> {
        self.bind_op(dn, pw, controls)
    }

    fn bind_op(&mut self, dn: &[u8], pw: &[u8], controls: &[Control]) -> Result<LdapMessage, i32> {
        let rc = self.ensure_open();
        if rc != LDAP_SUCCESS {
            return Err(rc);
        }
        let version = self.opts.version;
        let mut op = Writer::new();
        op.start(LDAP_REQ_BIND);
        op.int(version as i64);
        op.octets(dn);
        op.raw(LDAP_AUTH_SIMPLE, pw); // [0] simple
        op.end();
        let body = op.finish();
        let msgid = self.next_msgid();
        let all: Vec<Control> = self.merge_server_controls(controls);
        if self.write_message(msgid, &body, &all).is_err() {
            return Err(LDAP_SERVER_DOWN);
        }
        let m = self.read_result(LDAP_RES_BIND)?;
        self.set_diag(&m);
        Ok(m)
    }

    /// A generic operation producing a single result (add/modify/delete/
    /// moddn/compare).
    fn op_sync(
        &mut self,
        op_body: Vec<u8>,
        res_tag: u32,
        controls: &[Control],
    ) -> Result<LdapMessage, i32> {
        let rc = self.ensure_open();
        if rc != LDAP_SUCCESS {
            return Err(rc);
        }
        let msgid = self.next_msgid();
        let all = self.merge_server_controls(controls);
        if self.write_message(msgid, &op_body, &all).is_err() {
            return Err(LDAP_SERVER_DOWN);
        }
        let m = self.read_result(res_tag)?;
        self.set_diag(&m);
        self.errno = m.rescode;
        Ok(m)
    }

    fn merge_server_controls(&self, extra: &[Control]) -> Vec<Control> {
        // libldap merges the option's server controls with the per-op ones.
        let mut v = self.opts.server_controls.clone();
        v.extend_from_slice(extra);
        v
    }

    fn set_diag(&mut self, m: &LdapMessage) {
        self.matched = m.matched_dn.clone();
        self.error = m.errmsg.clone();
    }

    /// php_set_opts search with its own sizelimit/timelimit/deref overrides.
    #[allow(clippy::too_many_arguments)]
    pub fn search(
        &mut self,
        base: &[u8],
        scope: i32,
        filter: &[u8],
        attrs: &[Vec<u8>],
        attrsonly: bool,
        sizelimit: i32,
        timelimit: i32,
        deref: i32,
        controls: &[Control],
    ) -> Result<LdapMessage, i32> {
        let rc = self.ensure_open();
        if rc != LDAP_SUCCESS {
            return Err(rc);
        }
        let Some(fbytes) = crate::filter::encode(filter) else {
            self.errno = LDAP_FILTER_ERROR;
            return Err(LDAP_FILTER_ERROR);
        };
        let eff_deref = if deref >= 0 { deref } else { self.opts.deref };
        let eff_size = if sizelimit >= 0 {
            sizelimit
        } else {
            self.opts.sizelimit
        };
        let eff_time = if timelimit >= 0 {
            timelimit
        } else {
            self.opts.timelimit
        };
        let mut op = Writer::new();
        op.start(LDAP_REQ_SEARCH);
        op.octets(base);
        op.enumerated(scope as i64);
        op.enumerated(eff_deref as i64);
        op.int(eff_size.max(0) as i64);
        op.int(eff_time.max(0) as i64);
        op.boolean(attrsonly);
        op.append_encoded(&fbytes);
        op.start_seq();
        for a in attrs {
            op.octets(a);
        }
        op.end();
        op.end();
        let body = op.finish();
        let msgid = self.next_msgid();
        let all = self.merge_server_controls(controls);
        if self.write_message(msgid, &body, &all).is_err() {
            return Err(LDAP_SERVER_DOWN);
        }
        let m = self.read_result(LDAP_RES_SEARCH_RESULT)?;
        self.set_diag(&m);
        self.errno = m.rescode;
        Ok(m)
    }

    /// Add/modify (LDAPMod list). `ops`: (op_type, attr, Option<values>) —
    /// None values means delete whole attribute (modify only).
    pub fn modify(
        &mut self,
        dn: &[u8],
        mods: &[(i32, Vec<u8>, Option<Vec<Vec<u8>>>)],
        is_add: bool,
        controls: &[Control],
    ) -> Result<LdapMessage, i32> {
        let mut op = Writer::new();
        if is_add {
            op.start(LDAP_REQ_ADD);
            op.octets(dn);
            op.start_seq(); // AttributeList
            for (_t, attr, vals) in mods {
                op.start_seq();
                op.octets(attr);
                op.start_set();
                if let Some(vals) = vals {
                    for v in vals {
                        op.octets(v);
                    }
                }
                op.end();
                op.end();
            }
            op.end();
            op.end();
            self.op_sync(op.finish(), LDAP_RES_ADD, controls)
        } else {
            op.start(LDAP_REQ_MODIFY);
            op.octets(dn);
            op.start_seq(); // modifications
            for (t, attr, vals) in mods {
                op.start_seq();
                op.enumerated(*t as i64); // operation ENUMERATED
                op.start_seq(); // PartialAttribute
                op.octets(attr);
                op.start_set();
                if let Some(vals) = vals {
                    for v in vals {
                        op.octets(v);
                    }
                }
                op.end();
                op.end();
                op.end();
            }
            op.end();
            op.end();
            self.op_sync(op.finish(), LDAP_RES_MODIFY, controls)
        }
    }

    pub fn delete(&mut self, dn: &[u8], controls: &[Control]) -> Result<LdapMessage, i32> {
        let mut op = Writer::new();
        op.raw(LDAP_REQ_DELETE, dn); // [APPLICATION 10] LDAPDN (primitive)
        self.op_sync(op.finish(), LDAP_RES_DELETE, controls)
    }

    pub fn compare(
        &mut self,
        dn: &[u8],
        attr: &[u8],
        value: &[u8],
        controls: &[Control],
    ) -> Result<LdapMessage, i32> {
        let mut op = Writer::new();
        op.start(LDAP_REQ_COMPARE);
        op.octets(dn);
        op.start_seq(); // AttributeValueAssertion
        op.octets(attr);
        op.octets(value);
        op.end();
        op.end();
        self.op_sync(op.finish(), LDAP_RES_COMPARE, controls)
    }

    pub fn rename(
        &mut self,
        dn: &[u8],
        newrdn: &[u8],
        newparent: Option<&[u8]>,
        delete_old: bool,
        controls: &[Control],
    ) -> Result<LdapMessage, i32> {
        let mut op = Writer::new();
        op.start(LDAP_REQ_MODDN);
        op.octets(dn);
        op.octets(newrdn);
        op.boolean(delete_old);
        if let Some(np) = newparent {
            op.raw(LDAP_TAG_NEWSUPERIOR, np);
        }
        op.end();
        self.op_sync(op.finish(), LDAP_RES_MODDN, controls)
    }

    /// Generic extended operation (ldap_extended_operation_s).
    pub fn extended(
        &mut self,
        oid: &[u8],
        data: Option<&[u8]>,
        controls: &[Control],
    ) -> Result<LdapMessage, i32> {
        let mut op = Writer::new();
        op.start(LDAP_REQ_EXTENDED);
        op.raw(LDAP_TAG_EXOP_REQ_OID, oid);
        if let Some(d) = data {
            op.raw(LDAP_TAG_EXOP_REQ_VALUE, d);
        }
        op.end();
        self.op_sync(op.finish(), LDAP_RES_EXTENDED, controls)
    }
}

// ---- response parsing helpers ----

fn parse_entry(seq: &mut Reader) -> Result<Entry, i32> {
    let mut e = seq
        .enter(LDAP_RES_SEARCH_ENTRY)
        .map_err(|_| LDAP_DECODING_ERROR)?;
    let dn = e.octets().map_err(|_| LDAP_DECODING_ERROR)?.to_vec();
    let mut attrs = Vec::new();
    let mut alist = e.seq().map_err(|_| LDAP_DECODING_ERROR)?;
    while !alist.is_empty() {
        let mut a = alist.seq().map_err(|_| LDAP_DECODING_ERROR)?;
        let name = a.octets().map_err(|_| LDAP_DECODING_ERROR)?.to_vec();
        let mut set = a.enter(TAG_SET).map_err(|_| LDAP_DECODING_ERROR)?;
        let mut vals = Vec::new();
        while !set.is_empty() {
            vals.push(set.any().map_err(|_| LDAP_DECODING_ERROR)?.to_vec());
        }
        attrs.push((name, vals));
    }
    Ok(Entry { dn, attrs })
}

/// Parse the LDAPResult fields + op-specific trailers.
fn parse_result_op(op_tag: u32, seq: &mut Reader, out: &mut LdapMessage) -> Result<(), i32> {
    let mut body = seq.enter(op_tag).map_err(|_| LDAP_DECODING_ERROR)?;
    if op_tag == LDAP_RES_EXTENDED {
        // {resultCode, matchedDN, errorMessage, [referral], [responseName
        // [10]], [responseValue [11]]}
        read_ldapresult(&mut body, out)?;
        while !body.is_empty() {
            match body.peek_tag() {
                Some(t) if t == LDAP_TAG_EXOP_RES_OID => {
                    out.exop_oid = Some(body.any().map_err(|_| LDAP_DECODING_ERROR)?.to_vec());
                }
                Some(t) if t == LDAP_TAG_EXOP_RES_VALUE => {
                    out.exop_data = Some(body.any().map_err(|_| LDAP_DECODING_ERROR)?.to_vec());
                }
                Some(_) => {
                    let _ = body.read_raw();
                }
                None => break,
            }
        }
    } else if op_tag == LDAP_RES_BIND {
        read_ldapresult(&mut body, out)?;
        if let Ok(Some(c)) = body.optional(LDAP_TAG_SASL_RES_CREDS) {
            out.sasl_creds = Some(c.to_vec());
        }
    } else {
        read_ldapresult(&mut body, out)?;
    }
    Ok(())
}

fn read_ldapresult(body: &mut Reader, out: &mut LdapMessage) -> Result<(), i32> {
    out.rescode = body.enumerated().map_err(|_| LDAP_DECODING_ERROR)? as i32;
    out.matched_dn = body.octets().map_err(|_| LDAP_DECODING_ERROR)?.to_vec();
    out.errmsg = body.octets().map_err(|_| LDAP_DECODING_ERROR)?.to_vec();
    // optional referral [3]
    if let Ok(Some(refbytes)) = body.optional(LDAP_TAG_REFERRAL) {
        let mut rr = Reader::new(refbytes);
        while !rr.is_empty() {
            if let Ok(u) = rr.any() {
                out.referrals.push(u.to_vec());
            } else {
                break;
            }
        }
    }
    Ok(())
}

/// Parse a Controls element's content into Control structs.
pub fn parse_controls(bytes: &[u8]) -> Vec<Control> {
    let mut out = Vec::new();
    let mut r = Reader::new(bytes);
    while !r.is_empty() {
        let Ok(mut c) = r.seq() else { break };
        let Ok(oid) = c.octets() else { break };
        let mut criticality = false;
        let mut value = None;
        // optional BOOLEAN criticality, optional OCTET STRING value
        while !c.is_empty() {
            match c.peek_tag() {
                Some(ber::TAG_BOOLEAN) => {
                    criticality = c.boolean().unwrap_or(false);
                }
                Some(TAG_OCTETSTRING) => {
                    value = c.octets().ok().map(|v| v.to_vec());
                }
                _ => {
                    let _ = c.read_raw();
                }
            }
        }
        out.push(Control {
            oid: oid.to_vec(),
            criticality,
            value,
        });
    }
    out
}
