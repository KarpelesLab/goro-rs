//! PGconn: connection establishment (fe-connect.c), socket I/O
//! (fe-misc.c, fe-secure.c) and the protocol 3 message parser
//! (fe-protocol3.c).

use std::collections::VecDeque;
use std::io::Write;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::time::{Duration, Instant};

use super::conninfo::{self, ConnInfo};
use super::encoding;
use super::result::{AttDesc, PgResult, build_error_message};
use super::*;

/// PGAsyncStatusType
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AsyncStatus {
    Idle,
    Busy,
    Ready,
    ReadyMore,
    CopyIn,
    CopyOut,
    CopyBoth,
}

/// PGQueryClass
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum QueryClass {
    Simple,
    Extended,
    Prepare,
    Describe,
    Close,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum HostType {
    Name,
    Address,
    UnixSocket,
}

#[derive(Clone, Debug)]
struct HostSpec {
    kind: HostType,
    host: String,
    hostaddr: String,
    port: String,
}

#[derive(Clone, Debug)]
enum Addr {
    Inet(std::net::SocketAddr),
    Unix(String),
}

/// The transport: a plain socket or TLS over it
enum Sock {
    Plain(OwnedFd),
    Tls(Box<TlsSock>),
}

struct TlsSock {
    fd: OwnedFd,
    tls: purecrypto::tls::Connection,
    plain: Vec<u8>,
    ppos: usize,
    wire_out: Vec<u8>,
}

impl Sock {
    fn fd(&self) -> RawFd {
        match self {
            Sock::Plain(f) => f.as_raw_fd(),
            Sock::Tls(t) => t.fd.as_raw_fd(),
        }
    }
}

/// The SCRAM exchange state (fe-auth-scram.c fe_scram_state)
pub(crate) struct ScramState {
    pub client_nonce: String,
    pub client_first_bare: String,
    pub server_first: String,
    pub salted_password: [u8; 32],
    pub auth_message: String,
    pub done: bool,
}

/// A notice processor: (processor, its argument)
pub type NoticeProcessor = fn(usize, &[u8]);

/// libpq's defaultNoticeProcessor
pub fn default_notice_processor(_arg: usize, msg: &[u8]) {
    let _ = std::io::stderr().write_all(msg);
}

pub struct PgConn {
    pub opts: ConnInfo,
    hosts: Vec<HostSpec>,
    whichhost: usize,
    addrs: Vec<Addr>,
    whichaddr: usize,
    try_next_host: bool,
    try_next_addr: bool,
    pub status: ConnStatus,
    pub async_status: AsyncStatus,
    pub xact_status: TransStatus,
    pub queryclass: QueryClass,
    /// the query of the command in progress (cmd_queue_head->query)
    pub last_query: Option<Vec<u8>>,
    /// whether a command is queued (cmd_queue_head != NULL)
    pub cmd_queued: bool,
    sock: Option<Sock>,
    pub nonblocking: bool,
    inbuf: Vec<u8>,
    inpos: usize,
    outbuf: Vec<u8>,
    pub write_failed: bool,
    pub be_pid: i32,
    pub be_key: i32,
    pub param_status: Vec<(Vec<u8>, Vec<u8>)>,
    pub notifies: VecDeque<Notify>,
    pub notice_processor: NoticeProcessor,
    pub notice_arg: usize,
    pub error_message: Vec<u8>,
    /// conn->errorReported: how much of error_message is in a result
    pub error_reported: usize,
    pub result: Option<PgResult>,
    pub next_result: Option<PgResult>,
    pub std_strings: bool,
    pub client_encoding: i32,
    pub sversion: i32,
    pub pversion: i32,
    pub verbosity: Verbosity,
    pub show_context: ContextVisibility,
    pub trace: Option<std::fs::File>,
    pub trace_flags: i32,
    pub copy_is_binary: bool,
    pub copy_already_done: usize,
    /// PQsetSingleRowMode (1) / PQsetChunkedRowsMode (n); 0: off
    pub max_chunk_rows: usize,
    pub partial_result_mode: bool,
    pub single_row_mode: bool,
    // SSL
    pub ssl_in_use: bool,
    allow_ssl_try: bool,
    // authentication
    pub(crate) sasl: Option<ScramState>,
    pub password_needed: bool,
    pub auth_req_received: bool,
    connect_timeout: Option<Duration>,
    /// lo_initialize's function OIDs
    pub(crate) lobj_funcs: Option<super::lo::LoFuncs>,
}

fn errno() -> i32 {
    std::io::Error::last_os_error().raw_os_error().unwrap_or(0)
}

/// SOCK_STRERROR
pub fn strerror(e: i32) -> String {
    unsafe {
        let p = libc::strerror(e);
        if p.is_null() {
            return format!("errno {}", e);
        }
        std::ffi::CStr::from_ptr(p).to_string_lossy().into_owned()
    }
}

fn set_nonblock(fd: RawFd, on: bool) {
    unsafe {
        let fl = libc::fcntl(fd, libc::F_GETFL);
        if fl >= 0 {
            let nf = if on {
                fl | libc::O_NONBLOCK
            } else {
                fl & !libc::O_NONBLOCK
            };
            libc::fcntl(fd, libc::F_SETFL, nf);
        }
    }
}

fn be32(b: &[u8]) -> i32 {
    i32::from_be_bytes([b[0], b[1], b[2], b[3]])
}

fn be16(b: &[u8]) -> i16 {
    i16::from_be_bytes([b[0], b[1]])
}

/// A reader over a message body
pub(crate) struct Cur<'a> {
    pub b: &'a [u8],
    pub p: usize,
}

impl<'a> Cur<'a> {
    pub fn new(b: &'a [u8]) -> Cur<'a> {
        Cur { b, p: 0 }
    }
    pub fn int32(&mut self) -> Option<i32> {
        if self.p + 4 > self.b.len() {
            return None;
        }
        let v = be32(&self.b[self.p..]);
        self.p += 4;
        Some(v)
    }
    pub fn int16(&mut self) -> Option<i16> {
        if self.p + 2 > self.b.len() {
            return None;
        }
        let v = be16(&self.b[self.p..]);
        self.p += 2;
        Some(v)
    }
    pub fn byte(&mut self) -> Option<u8> {
        let v = *self.b.get(self.p)?;
        self.p += 1;
        Some(v)
    }
    pub fn cstr(&mut self) -> Option<&'a [u8]> {
        let rest = &self.b[self.p..];
        let n = rest.iter().position(|&c| c == 0)?;
        self.p += n + 1;
        Some(&rest[..n])
    }
    pub fn bytes(&mut self, n: usize) -> Option<&'a [u8]> {
        if self.p + n > self.b.len() {
            return None;
        }
        let v = &self.b[self.p..self.p + n];
        self.p += n;
        Some(v)
    }
    pub fn rest(&self) -> &'a [u8] {
        &self.b[self.p..]
    }
}

impl Default for PgConn {
    fn default() -> Self {
        Self::new()
    }
}

impl PgConn {
    /// makeEmptyPGconn
    pub fn new() -> PgConn {
        PgConn {
            opts: ConnInfo::new(),
            hosts: Vec::new(),
            whichhost: 0,
            addrs: Vec::new(),
            whichaddr: 0,
            try_next_host: false,
            try_next_addr: false,
            status: ConnStatus::Bad,
            async_status: AsyncStatus::Idle,
            xact_status: TransStatus::Idle,
            queryclass: QueryClass::Simple,
            last_query: None,
            cmd_queued: false,
            sock: None,
            nonblocking: false,
            inbuf: Vec::new(),
            inpos: 0,
            outbuf: Vec::new(),
            write_failed: false,
            be_pid: 0,
            be_key: 0,
            param_status: Vec::new(),
            notifies: VecDeque::new(),
            notice_processor: default_notice_processor,
            notice_arg: 0,
            error_message: Vec::new(),
            error_reported: 0,
            result: None,
            next_result: None,
            std_strings: false,
            client_encoding: encoding::PG_SQL_ASCII,
            sversion: 0,
            pversion: 0,
            verbosity: Verbosity::Default,
            show_context: ContextVisibility::Errors,
            trace: None,
            trace_flags: 0,
            copy_is_binary: false,
            copy_already_done: 0,
            max_chunk_rows: 0,
            partial_result_mode: false,
            single_row_mode: false,
            ssl_in_use: false,
            allow_ssl_try: true,
            sasl: None,
            password_needed: false,
            auth_req_received: false,
            connect_timeout: None,
            lobj_funcs: None,
        }
    }

    // ── error reporting ────────────────────────────────────────────────

    /// libpq_append_conn_error
    pub fn append_error(&mut self, msg: &str) {
        self.error_message.extend_from_slice(msg.as_bytes());
        self.error_message.push(b'\n');
    }

    /// pqClearConnErrorState
    pub fn clear_error_state(&mut self) {
        self.error_message.clear();
        self.error_reported = 0;
    }

    /// PQerrorMessage
    pub fn error_message(&self) -> &[u8] {
        &self.error_message
    }

    // ── connection parameters ──────────────────────────────────────────

    pub fn param(&self, name: &str) -> Option<&str> {
        self.opts.get(name)
    }

    /// PQdb
    pub fn db(&self) -> &str {
        self.opts.get("dbname").unwrap_or("")
    }

    /// PQuser
    pub fn user(&self) -> &str {
        self.opts.get("user").unwrap_or("")
    }

    /// PQpass
    pub fn pass(&self) -> &str {
        self.opts.get("password").unwrap_or("")
    }

    /// PQhost
    pub fn host(&self) -> String {
        if let Some(h) = self.hosts.get(self.whichhost) {
            if !h.host.is_empty() {
                return h.host.clone();
            }
            if !h.hostaddr.is_empty() {
                return h.hostaddr.clone();
            }
        }
        conninfo::default_socket_dir().to_string()
    }

    /// PQhostaddr
    pub fn hostaddr(&self) -> String {
        self.hosts
            .get(self.whichhost)
            .map(|h| h.hostaddr.clone())
            .unwrap_or_default()
    }

    /// PQport
    pub fn port(&self) -> String {
        match self.hosts.get(self.whichhost) {
            Some(h) if !h.port.is_empty() => h.port.clone(),
            _ => "5432".to_string(),
        }
    }

    /// PQoptions
    pub fn options(&self) -> &str {
        self.opts.get("options").unwrap_or("")
    }

    /// PQservice
    pub fn service(&self) -> &str {
        self.opts.get("service").unwrap_or("")
    }

    /// PQparameterStatus
    pub fn parameter_status(&self, name: &[u8]) -> Option<&[u8]> {
        self.param_status
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_slice())
    }

    /// PQprotocolVersion
    pub fn protocol_version(&self) -> i32 {
        if self.status == ConnStatus::Bad {
            return 0;
        }
        self.pversion >> 16
    }

    /// PQserverVersion
    pub fn server_version(&self) -> i32 {
        if self.status == ConnStatus::Bad {
            return 0;
        }
        self.sversion
    }

    /// PQsocket
    pub fn socket(&self) -> RawFd {
        self.sock.as_ref().map(|s| s.fd()).unwrap_or(-1)
    }

    /// PQbackendPID
    pub fn backend_pid(&self) -> i32 {
        if self.status != ConnStatus::Ok {
            return 0;
        }
        self.be_pid
    }

    /// PQtransactionStatus
    pub fn transaction_status(&self) -> TransStatus {
        if self.sock.is_none() || self.status != ConnStatus::Ok {
            return TransStatus::Unknown;
        }
        if self.async_status != AsyncStatus::Idle {
            return TransStatus::Active;
        }
        self.xact_status
    }

    /// PQsetNoticeProcessor: returns the previous processor
    pub fn set_notice_processor(&mut self, p: NoticeProcessor, arg: usize) -> NoticeProcessor {
        let old = self.notice_processor;
        self.notice_processor = p;
        self.notice_arg = arg;
        old
    }

    /// PQsetErrorVerbosity
    pub fn set_error_verbosity(&mut self, v: Verbosity) -> Verbosity {
        std::mem::replace(&mut self.verbosity, v)
    }

    /// PQsetErrorContextVisibility
    pub fn set_error_context_visibility(&mut self, v: ContextVisibility) -> ContextVisibility {
        std::mem::replace(&mut self.show_context, v)
    }

    /// PQisnonblocking
    pub fn is_nonblocking(&self) -> bool {
        self.nonblocking
    }

    // ── connection establishment ───────────────────────────────────────

    /// PQconnectdb
    pub fn connectdb(conninfo: &[u8]) -> PgConn {
        let mut c = PgConn::connect_start(conninfo);
        if c.status != ConnStatus::Bad {
            c.connect_complete();
        }
        c
    }

    /// PQconnectStart
    pub fn connect_start(conninfo_str: &[u8]) -> PgConn {
        let mut c = PgConn::new();
        let mut err = String::new();
        match conninfo::parse(conninfo_str, &mut err) {
            Some(o) => c.opts = o,
            None => {
                c.error_message = err.into_bytes();
                return c;
            }
        }
        if !conninfo::apply_service(&mut c.opts, &mut err) {
            c.error_message = err.into_bytes();
            return c;
        }
        conninfo::add_defaults(&mut c.opts);
        if !c.connect_options2() {
            c.status = ConnStatus::Bad;
            return c;
        }
        c.connect_db_start();
        c
    }

    /// connectOptions2: host list, defaults and validation
    fn connect_options2(&mut self) -> bool {
        let split = |s: Option<&str>| -> Vec<String> {
            match s {
                Some(s) if !s.is_empty() => s.split(',').map(|x| x.to_string()).collect(),
                _ => Vec::new(),
            }
        };
        let hosts = split(self.opts.get("host"));
        let addrs = split(self.opts.get("hostaddr"));
        let n = hosts.len().max(addrs.len()).max(1);
        if !hosts.is_empty() && !addrs.is_empty() && hosts.len() != addrs.len() {
            self.append_error(&format!(
                "could not match {} host names to {} hostaddr values",
                hosts.len(),
                addrs.len()
            ));
            return false;
        }
        let ports = split(self.opts.get("port"));
        if ports.len() > 1 && ports.len() != n {
            self.append_error(&format!(
                "could not match {} port numbers to {} hosts",
                ports.len(),
                n
            ));
            return false;
        }
        self.hosts.clear();
        for i in 0..n {
            let host = hosts.get(i).cloned().unwrap_or_default();
            let hostaddr = addrs.get(i).cloned().unwrap_or_default();
            let port = if ports.len() == 1 {
                ports[0].clone()
            } else {
                ports.get(i).cloned().unwrap_or_default()
            };
            let kind = if !hostaddr.is_empty() {
                HostType::Address
            } else if !host.is_empty() && !host.starts_with('/') && !host.starts_with('@') {
                HostType::Name
            } else {
                HostType::UnixSocket
            };
            let host = if kind == HostType::UnixSocket && host.is_empty() {
                conninfo::default_socket_dir().to_string()
            } else {
                host
            };
            self.hosts.push(HostSpec {
                kind,
                host,
                hostaddr,
                port,
            });
        }
        // dbname defaults to the user name
        if self.opts.get("dbname").is_none_or(|d| d.is_empty()) {
            let u = self.opts.get("user").map(|s| s.to_string());
            self.opts.set("dbname", u);
        }
        // the password file
        if self.opts.get("password").is_none_or(|p| p.is_empty()) {
            let pf = self.opts.get("passfile").map(|s| s.to_string());
            let h = self.hosts[0].clone();
            let hostname = if h.kind == HostType::Address {
                h.hostaddr.clone()
            } else {
                h.host.clone()
            };
            let port = if h.port.is_empty() {
                "5432".to_string()
            } else {
                h.port.clone()
            };
            if let Some(p) = conninfo::password_from_file(
                pf.as_deref(),
                &hostname,
                &port,
                self.db(),
                self.user(),
            ) {
                self.opts.set("password", Some(p));
            }
        }
        // option validation
        let check = |c: &mut PgConn, name: &str, allowed: &[&str]| -> bool {
            if let Some(v) = c.opts.get(name) {
                if !allowed.contains(&v) {
                    let v = v.to_string();
                    c.append_error(&format!("invalid {} value: \"{}\"", name, v));
                    return false;
                }
            }
            true
        };
        if !check(
            self,
            "sslmode",
            &[
                "disable",
                "allow",
                "prefer",
                "require",
                "verify-ca",
                "verify-full",
            ],
        ) || !check(self, "channel_binding", &["disable", "prefer", "require"])
            || !check(
                self,
                "target_session_attrs",
                &[
                    "any",
                    "read-write",
                    "read-only",
                    "primary",
                    "standby",
                    "prefer-standby",
                ],
            )
            || !check(self, "gssencmode", &["disable", "prefer", "require"])
            || !check(self, "sslnegotiation", &["postgres", "direct"])
            || !check(self, "load_balance_hosts", &["disable", "random"])
        {
            return false;
        }
        if let Some(t) = self.opts.get("connect_timeout") {
            match t.trim().parse::<i64>() {
                Ok(v) if v > 0 => {
                    self.connect_timeout = Some(Duration::from_secs(v.max(2) as u64));
                }
                Ok(_) => {}
                Err(_) => {
                    let t = t.to_string();
                    self.append_error(&format!(
                        "invalid integer value \"{}\" for connection option \"connect_timeout\"",
                        t
                    ));
                    return false;
                }
            }
        }
        if self.opts.get("sslmode") == Some("disable") {
            self.allow_ssl_try = false;
        }
        // client_encoding=auto: the locale's codeset
        if self.opts.get("client_encoding") == Some("auto") {
            let cs = unsafe {
                let p = libc::nl_langinfo(libc::CODESET);
                if p.is_null() {
                    String::new()
                } else {
                    std::ffi::CStr::from_ptr(p).to_string_lossy().into_owned()
                }
            };
            let enc = match cs.to_ascii_uppercase().as_str() {
                "UTF-8" | "UTF8" => "UTF8",
                "ANSI_X3.4-1968" | "ASCII" | "US-ASCII" => "SQL_ASCII",
                "ISO-8859-1" => "LATIN1",
                "ISO-8859-15" => "LATIN9",
                "EUC-JP" => "EUC_JP",
                "SHIFT_JIS" | "SJIS" => "SJIS",
                _ => "SQL_ASCII",
            };
            self.opts.set("client_encoding", Some(enc.to_string()));
        }
        true
    }

    /// connectDBStart
    fn connect_db_start(&mut self) {
        self.whichhost = 0;
        self.addrs.clear();
        self.whichaddr = 0;
        self.try_next_host = true;
        self.try_next_addr = false;
        self.status = ConnStatus::Needed;
        self.inbuf.clear();
        self.inpos = 0;
        self.outbuf.clear();
        if self.connect_poll() == PollingStatus::Writing {
            return;
        }
        if self.status == ConnStatus::Bad {
            self.close_socket();
        }
    }

    /// connectDBComplete
    pub fn connect_complete(&mut self) -> bool {
        let mut flag = PollingStatus::Writing;
        let deadline = self.connect_timeout.map(|t| Instant::now() + t);
        loop {
            match flag {
                PollingStatus::Ok => return true,
                PollingStatus::Reading | PollingStatus::Writing => {
                    let r = self.wait_timed(
                        flag == PollingStatus::Reading,
                        flag == PollingStatus::Writing,
                        deadline,
                    );
                    if r == 1 {
                        // timeout: try the next address
                        self.try_next_addr = true;
                        self.status = ConnStatus::Needed;
                    } else if r < 0 {
                        self.status = ConnStatus::Bad;
                        return false;
                    }
                }
                _ => {
                    self.status = ConnStatus::Bad;
                    return false;
                }
            }
            flag = self.connect_poll();
        }
    }

    /// emitHostIdentityInfo
    fn emit_host_identity(&mut self, addr: &Addr, host_addr: &str) {
        match addr {
            Addr::Unix(path) => {
                let s = format!("connection to server on socket \"{}\" failed: ", path);
                self.error_message.extend_from_slice(s.as_bytes());
            }
            Addr::Inet(_) => {
                let h = &self.hosts[self.whichhost];
                let displayed_host = if h.kind == HostType::Address {
                    h.hostaddr.clone()
                } else {
                    h.host.clone()
                };
                let displayed_port = if h.port.is_empty() {
                    "5432".to_string()
                } else {
                    h.port.clone()
                };
                let s = if h.kind != HostType::Address
                    && !host_addr.is_empty()
                    && displayed_host != host_addr
                {
                    format!(
                        "connection to server at \"{}\" ({}), port {} failed: ",
                        displayed_host, host_addr, displayed_port
                    )
                } else {
                    format!(
                        "connection to server at \"{}\", port {} failed: ",
                        displayed_host, displayed_port
                    )
                };
                self.error_message.extend_from_slice(s.as_bytes());
            }
        }
    }

    /// connectFailureMessage
    fn connect_failure_message(&mut self, e: i32, unix: bool) {
        let s = format!("{}\n", strerror(e));
        self.error_message.extend_from_slice(s.as_bytes());
        if unix {
            self.append_error(
                "\tIs the server running locally and accepting connections on that socket?",
            );
        } else {
            self.append_error(
                "\tIs the server running on that host and accepting TCP/IP connections?",
            );
        }
    }

    fn close_socket(&mut self) {
        if let Some(s) = self.sock.take() {
            if let Sock::Tls(mut t) = s {
                let _ = t.tls.close();
                if let Ok(out) = t.tls.pop() {
                    unsafe {
                        libc::send(
                            t.fd.as_raw_fd(),
                            out.as_ptr() as *const libc::c_void,
                            out.len(),
                            libc::MSG_NOSIGNAL,
                        );
                    }
                }
            }
        }
        self.ssl_in_use = false;
        self.inbuf.clear();
        self.inpos = 0;
        self.outbuf.clear();
        self.write_failed = false;
    }

    /// pqDropConnection
    pub fn drop_connection(&mut self) {
        self.close_socket();
        self.sasl = None;
        self.lobj_funcs = None;
    }

    /// Resolve the current host's addresses.
    fn resolve_host(&mut self) -> bool {
        let h = self.hosts[self.whichhost].clone();
        let port_s = if h.port.is_empty() {
            "5432".to_string()
        } else {
            h.port.clone()
        };
        let port: u16 = match port_s.trim().parse::<i64>() {
            Ok(p) if (1..=65535).contains(&p) => p as u16,
            _ => {
                self.append_error(&format!("invalid port number: \"{}\"", port_s));
                return false;
            }
        };
        self.addrs.clear();
        match h.kind {
            HostType::UnixSocket => {
                let dir = h.host.trim_end_matches('/');
                let path = format!("{}/.s.PGSQL.{}", dir, port);
                if path.len() >= 108 {
                    self.append_error(&format!(
                        "Unix-domain socket path \"{}\" is too long (maximum {} bytes)",
                        path, 107
                    ));
                    return false;
                }
                self.addrs.push(Addr::Unix(path));
            }
            HostType::Address => match h.hostaddr.parse::<std::net::IpAddr>() {
                Ok(ip) => self.addrs.push(Addr::Inet((ip, port).into())),
                Err(_) => {
                    self.append_error(&format!(
                        "could not parse network address \"{}\": {}",
                        h.hostaddr, "Name or service not known"
                    ));
                    return false;
                }
            },
            HostType::Name => {
                match getaddrinfo(&h.host, port) {
                    Ok(list) => self.addrs = list.into_iter().map(Addr::Inet).collect(),
                    Err(msg) => {
                        self.append_error(&format!(
                            "could not translate host name \"{}\" to address: {}",
                            h.host, msg
                        ));
                        return false;
                    }
                }
                if self.addrs.is_empty() {
                    self.append_error(&format!(
                        "could not translate host name \"{}\" to address: {}",
                        h.host, "Name or service not known"
                    ));
                    return false;
                }
            }
        }
        true
    }

    /// PQconnectPoll
    pub fn connect_poll(&mut self) -> PollingStatus {
        match self.status {
            ConnStatus::Bad => return PollingStatus::Failed,
            ConnStatus::Ok => return PollingStatus::Ok,
            ConnStatus::AwaitingResponse | ConnStatus::AuthOk => {
                let n = self.read_data();
                if n < 0 {
                    return self.error_return();
                }
                if n == 0 && !self.has_complete_message() {
                    return PollingStatus::Reading;
                }
            }
            _ => {}
        }
        loop {
            match self.status {
                ConnStatus::Needed => {
                    // pick the next address to try
                    if self.try_next_addr {
                        self.whichaddr += 1;
                        if self.whichaddr >= self.addrs.len() {
                            self.try_next_host = true;
                        }
                        self.try_next_addr = false;
                    }
                    if self.try_next_host {
                        if !self.addrs.is_empty() || self.whichaddr > 0 {
                            self.whichhost += 1;
                        }
                        loop {
                            if self.whichhost >= self.hosts.len() {
                                return self.error_return();
                            }
                            if self.resolve_host() {
                                break;
                            }
                            self.whichhost += 1;
                        }
                        self.whichaddr = 0;
                        self.try_next_host = false;
                    }
                    if self.whichaddr >= self.addrs.len() {
                        self.try_next_host = true;
                        continue;
                    }
                    self.close_socket();
                    self.allow_ssl_try = self.opts.get("sslmode") != Some("disable");
                    self.sasl = None;
                    self.auth_req_received = false;
                    self.password_needed = false;
                    self.param_status.clear();
                    let addr = self.addrs[self.whichaddr].clone();
                    let host_addr = match &addr {
                        Addr::Inet(a) => a.ip().to_string(),
                        Addr::Unix(_) => String::new(),
                    };
                    self.emit_host_identity(&addr, &host_addr);
                    match connect_socket(&addr) {
                        Ok((fd, in_progress)) => {
                            self.sock = Some(Sock::Plain(fd));
                            self.status = ConnStatus::Started;
                            if in_progress {
                                return PollingStatus::Writing;
                            }
                        }
                        Err((e, phase)) => {
                            if phase == 0 {
                                self.connect_failure_message(e, matches!(addr, Addr::Unix(_)));
                            } else {
                                self.append_error(&format!(
                                    "could not create socket: {}",
                                    strerror(e)
                                ));
                            }
                            self.try_next_addr = true;
                            continue;
                        }
                    }
                }
                ConnStatus::Started => {
                    let fd = self.socket();
                    let mut err: i32 = 0;
                    let mut l = std::mem::size_of::<i32>() as libc::socklen_t;
                    let r = unsafe {
                        libc::getsockopt(
                            fd,
                            libc::SOL_SOCKET,
                            libc::SO_ERROR,
                            &mut err as *mut i32 as *mut libc::c_void,
                            &mut l,
                        )
                    };
                    if r == -1 {
                        let e = errno();
                        self.append_error(&format!(
                            "could not get socket error status: {}",
                            strerror(e)
                        ));
                        return self.error_return();
                    }
                    if err != 0 {
                        let unix = matches!(self.addrs[self.whichaddr], Addr::Unix(_));
                        self.connect_failure_message(err, unix);
                        self.close_socket();
                        self.try_next_addr = true;
                        self.status = ConnStatus::Needed;
                        continue;
                    }
                    self.status = ConnStatus::Made;
                }
                ConnStatus::Made => {
                    let is_tcp = matches!(self.addrs[self.whichaddr], Addr::Inet(_));
                    if is_tcp && self.allow_ssl_try && !self.ssl_in_use {
                        // SSLRequest
                        let mut pkt = Vec::new();
                        pkt.extend_from_slice(&8i32.to_be_bytes());
                        pkt.extend_from_slice(&80877103i32.to_be_bytes());
                        if !self.raw_send_all(&pkt) {
                            let e = errno();
                            self.append_error(&format!(
                                "could not send SSL negotiation packet: {}",
                                strerror(e)
                            ));
                            return self.error_return();
                        }
                        self.status = ConnStatus::SslStartup;
                        return PollingStatus::Reading;
                    }
                    let pkt = self.build_startup_packet();
                    if !self.raw_send_all(&pkt) {
                        let e = errno();
                        self.append_error(&format!(
                            "could not send startup packet: {}",
                            strerror(e)
                        ));
                        return self.error_return();
                    }
                    self.status = ConnStatus::AwaitingResponse;
                    return PollingStatus::Reading;
                }
                ConnStatus::SslStartup => {
                    let fd = self.socket();
                    let mut b = [0u8; 1];
                    let n = unsafe { libc::recv(fd, b.as_mut_ptr() as *mut libc::c_void, 1, 0) };
                    if n < 0 {
                        let e = errno();
                        if e == libc::EAGAIN || e == libc::EWOULDBLOCK || e == libc::EINTR {
                            return PollingStatus::Reading;
                        }
                        self.connect_failure_message(e, false);
                        self.close_socket();
                        self.try_next_addr = true;
                        self.status = ConnStatus::Needed;
                        continue;
                    }
                    if n == 0 {
                        self.append_error(
                            "server closed the connection unexpectedly\n\tThis probably means the server terminated abnormally\n\tbefore or while processing the request.",
                        );
                        return self.error_return();
                    }
                    match b[0] {
                        b'S' => {
                            if !self.start_tls() {
                                return self.error_return();
                            }
                            self.status = ConnStatus::Made;
                        }
                        b'N' => {
                            let mode = self.opts.get("sslmode").unwrap_or("prefer");
                            if mode.starts_with('r') || mode.starts_with('v') {
                                self.append_error(
                                    "server does not support SSL, but SSL was required",
                                );
                                return self.error_return();
                            }
                            self.allow_ssl_try = false;
                            self.status = ConnStatus::Made;
                        }
                        b'E' => {
                            // a pre-7.0 server or an error: read it as one
                            self.inbuf.push(b'E');
                            self.allow_ssl_try = false;
                            self.status = ConnStatus::AwaitingResponse;
                            let n = self.read_data();
                            if n < 0 {
                                return self.error_return();
                            }
                        }
                        c => {
                            self.append_error(&format!(
                                "received invalid response to SSL negotiation: {}",
                                c as char
                            ));
                            return self.error_return();
                        }
                    }
                }
                ConnStatus::AwaitingResponse => {
                    let avail = &self.inbuf[self.inpos..];
                    if avail.is_empty() {
                        return PollingStatus::Reading;
                    }
                    let beresp = avail[0];
                    if beresp != b'R' && beresp != b'E' && beresp != b'v' {
                        self.append_error(&format!(
                            "expected authentication request from server, but received {}",
                            beresp as char
                        ));
                        return self.error_return();
                    }
                    let Some((ty, body)) = self.take_message() else {
                        return PollingStatus::Reading;
                    };
                    self.trace_in(ty, &body);
                    match ty {
                        b'E' => {
                            self.get_error_notice(&body, true);
                            // CONNECTION_FAILED(): try the next host
                            self.close_socket();
                            self.try_next_host = true;
                            self.status = ConnStatus::Needed;
                            continue;
                        }
                        b'v' => {
                            // NegotiateProtocolVersion: accept the minor
                            // version the server offers
                            let mut c = Cur::new(&body);
                            if let Some(v) = c.int32() {
                                self.pversion = v;
                            }
                            continue;
                        }
                        _ => {
                            let mut c = Cur::new(&body);
                            let areq = c.int32().unwrap_or(-1);
                            let rest = c.rest().to_vec();
                            if areq == 0 {
                                if self.sasl.as_ref().is_some_and(|s| !s.done) {
                                    self.append_error(
                                        "server sent unexpected authentication success before the SASL exchange completed",
                                    );
                                    return self.error_return();
                                }
                                self.status = ConnStatus::AuthOk;
                                self.async_status = AsyncStatus::Busy;
                                continue;
                            }
                            self.auth_req_received = true;
                            if !super::auth::send_auth(self, areq, &rest) {
                                self.close_socket();
                                return self.error_return();
                            }
                            if self.flush() < 0 {
                                return self.error_return();
                            }
                            continue;
                        }
                    }
                }
                ConnStatus::AuthOk => {
                    if self.is_busy() {
                        return PollingStatus::Reading;
                    }
                    let res = self.get_result();
                    if let Some(res) = res {
                        if res.status != ExecStatus::FatalError {
                            self.append_error("unexpected message from server during startup");
                        }
                        return self.error_return();
                    }
                    self.status = ConnStatus::CheckTarget;
                }
                ConnStatus::CheckTarget => {
                    self.status = ConnStatus::Ok;
                    // the per-attempt messages are not part of a success
                    self.clear_error_state();
                    return PollingStatus::Ok;
                }
                _ => {
                    self.append_error(&format!(
                        "invalid connection state {}, probably indicative of memory corruption",
                        self.status as i32
                    ));
                    return self.error_return();
                }
            }
        }
    }

    fn error_return(&mut self) -> PollingStatus {
        self.status = ConnStatus::Bad;
        self.close_socket();
        PollingStatus::Failed
    }

    /// build_startup_packet (protocol 3.0)
    fn build_startup_packet(&mut self) -> Vec<u8> {
        self.pversion = 3 << 16;
        let mut body = Vec::new();
        body.extend_from_slice(&(3i32 << 16).to_be_bytes());
        let mut add = |k: &str, v: &str| {
            body.extend_from_slice(k.as_bytes());
            body.push(0);
            body.extend_from_slice(v.as_bytes());
            body.push(0);
        };
        if let Some(u) = self.opts.get("user") {
            if !u.is_empty() {
                add("user", u);
            }
        }
        if let Some(d) = self.opts.get("dbname") {
            if !d.is_empty() {
                add("database", d);
            }
        }
        if let Some(r) = self.opts.get("replication") {
            if !r.is_empty() {
                add("replication", r);
            }
        }
        if let Some(o) = self.opts.get("options") {
            if !o.is_empty() {
                add("options", o);
            }
        }
        let app = self
            .opts
            .get("application_name")
            .or(self.opts.get("fallback_application_name"));
        if let Some(a) = app {
            if !a.is_empty() {
                add("application_name", a);
            }
        }
        if let Some(e) = self.opts.get("client_encoding") {
            if !e.is_empty() {
                add("client_encoding", e);
            }
        }
        body.push(0);
        let mut pkt = Vec::with_capacity(body.len() + 4);
        pkt.extend_from_slice(&((body.len() + 4) as i32).to_be_bytes());
        pkt.extend_from_slice(&body);
        pkt
    }

    /// pqsecure_open_client: the TLS handshake (done synchronously)
    fn start_tls(&mut self) -> bool {
        use purecrypto::tls::{Config, Connection, HandshakeStatus, RootCertStore};
        let mode = self.opts.get("sslmode").unwrap_or("prefer").to_string();
        let verify = mode.starts_with("verify");
        let mut roots = RootCertStore::new();
        if verify {
            let rootcert = self
                .opts
                .get("sslrootcert")
                .map(|s| s.to_string())
                .or_else(|| {
                    std::env::var("HOME")
                        .ok()
                        .map(|h| format!("{}/.postgresql/root.crt", h))
                });
            match rootcert.as_deref().map(std::fs::read_to_string) {
                Some(Ok(pem)) => {
                    if roots.add_pem(&pem).is_err() {
                        self.append_error(&format!(
                            "could not read root certificate file \"{}\"",
                            rootcert.unwrap_or_default()
                        ));
                        return false;
                    }
                }
                _ => {
                    self.append_error(&format!(
                        "root certificate file \"{}\" does not exist\nEither provide the file, use the system's trusted roots with sslrootcert=system, or change sslmode to disable server certificate verification.",
                        rootcert.unwrap_or_default()
                    ));
                    return false;
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
            .verify_certificates(verify);
        let h = &self.hosts[self.whichhost];
        if h.kind == HostType::Name && self.opts.get("sslsni") != Some("0") {
            b = b.server_name(h.host.clone());
        }
        let cfg = b.build();
        let mut tls = match Connection::client(&cfg) {
            Ok(c) => c,
            Err(e) => {
                self.append_error(&format!("could not establish SSL connection: {:?}", e));
                return false;
            }
        };
        let Some(Sock::Plain(fd)) = self.sock.take() else {
            return false;
        };
        let raw = fd.as_raw_fd();
        set_nonblock(raw, false);
        let mut wire = [0u8; 16384];
        let ok = loop {
            if let Ok(out) = tls.pop() {
                if !out.is_empty() && !send_all_fd(raw, &out) {
                    break Err("SSL SYSCALL error: EOF detected".to_string());
                }
            }
            match tls.handshake() {
                Ok(HandshakeStatus::Complete) => break Ok(()),
                Ok(HandshakeStatus::WantWrite) => continue,
                Ok(HandshakeStatus::WantRead) => {
                    let n = unsafe {
                        libc::recv(raw, wire.as_mut_ptr() as *mut libc::c_void, wire.len(), 0)
                    };
                    if n <= 0 {
                        break Err("SSL SYSCALL error: EOF detected".to_string());
                    }
                    if let Err(e) = tls.feed(&wire[..n as usize]) {
                        break Err(format!("SSL error: {:?}", e));
                    }
                }
                Err(e) => break Err(format!("SSL error: {:?}", e)),
            }
        };
        set_nonblock(raw, true);
        if let Err(m) = ok {
            self.append_error(&m);
            return false;
        }
        self.sock = Some(Sock::Tls(Box::new(TlsSock {
            fd,
            tls,
            plain: Vec::new(),
            ppos: 0,
            wire_out: Vec::new(),
        })));
        self.ssl_in_use = true;
        true
    }

    /// PQfinish
    pub fn finish(&mut self) {
        if self.sock.is_some() && self.status == ConnStatus::Ok {
            // Terminate
            self.put_msg(b'X', &[]);
            let nb = self.nonblocking;
            self.nonblocking = true;
            let _ = self.flush();
            self.nonblocking = nb;
        }
        self.drop_connection();
        self.status = ConnStatus::Bad;
        self.untrace();
    }

    /// PQreset
    pub fn reset(&mut self) {
        self.close_socket();
        self.async_status = AsyncStatus::Idle;
        self.xact_status = TransStatus::Idle;
        self.result = None;
        self.next_result = None;
        self.lobj_funcs = None;
        self.notifies.clear();
        self.clear_error_state();
        self.connect_db_start();
        if self.status != ConnStatus::Bad {
            self.connect_complete();
        }
    }

    /// PQresetStart / PQresetPoll
    pub fn reset_start(&mut self) -> bool {
        self.close_socket();
        self.async_status = AsyncStatus::Idle;
        self.clear_error_state();
        self.connect_db_start();
        self.status != ConnStatus::Bad
    }

    // ── socket I/O ─────────────────────────────────────────────────────

    /// write `data` straight to the socket (startup packets)
    fn raw_send_all(&mut self, data: &[u8]) -> bool {
        let fd = self.socket();
        let start = Instant::now();
        let mut off = 0;
        while off < data.len() {
            let n = unsafe {
                libc::send(
                    fd,
                    data[off..].as_ptr() as *const libc::c_void,
                    data.len() - off,
                    libc::MSG_NOSIGNAL,
                )
            };
            if n < 0 {
                let e = errno();
                if (e == libc::EAGAIN || e == libc::EINTR) && start.elapsed().as_secs() < 30 {
                    poll_fd(fd, false, true, 100);
                    continue;
                }
                return false;
            }
            off += n as usize;
        }
        true
    }

    /// Whether the unparsed input holds a whole message
    fn has_complete_message(&self) -> bool {
        let a = &self.inbuf[self.inpos..];
        if a.len() < 5 {
            return false;
        }
        let len = be32(&a[1..]) as usize;
        a.len() > len
    }

    /// Take one whole message (type, body) from the input buffer
    pub(crate) fn take_message(&mut self) -> Option<(u8, Vec<u8>)> {
        let a = &self.inbuf[self.inpos..];
        if a.len() < 5 {
            return None;
        }
        let ty = a[0];
        let len = be32(&a[1..]);
        if len < 4 {
            return None;
        }
        let len = len as usize;
        if a.len() < len + 1 {
            return None;
        }
        let body = a[5..len + 1].to_vec();
        self.inpos += len + 1;
        if self.inpos == self.inbuf.len() {
            self.inbuf.clear();
            self.inpos = 0;
        }
        Some((ty, body))
    }

    /// Peek at the type of the next whole message
    pub(crate) fn peek_message_type(&self) -> Option<u8> {
        self.peek_message()
    }

    /// The address of the current connection, for cancel requests
    pub fn current_addr(&self) -> Option<super::exec::CancelAddr> {
        match self.addrs.get(self.whichaddr)? {
            Addr::Inet(a) => Some(super::exec::CancelAddr::Inet(*a)),
            Addr::Unix(p) => Some(super::exec::CancelAddr::Unix(p.clone())),
        }
    }

    fn peek_message(&self) -> Option<u8> {
        if self.has_complete_message() {
            Some(self.inbuf[self.inpos])
        } else {
            None
        }
    }

    /// Read whatever is available without blocking; -1 on EOF/error.
    fn sock_read(&mut self, buf: &mut [u8]) -> Result<usize, i32> {
        let Some(sock) = self.sock.as_mut() else {
            return Err(0);
        };
        match sock {
            Sock::Plain(fd) => loop {
                let n = unsafe {
                    libc::recv(
                        fd.as_raw_fd(),
                        buf.as_mut_ptr() as *mut libc::c_void,
                        buf.len(),
                        0,
                    )
                };
                if n < 0 {
                    let e = errno();
                    if e == libc::EINTR {
                        continue;
                    }
                    if e == libc::EAGAIN || e == libc::EWOULDBLOCK {
                        return Ok(0);
                    }
                    return Err(e);
                }
                if n == 0 {
                    return Err(0);
                }
                return Ok(n as usize);
            },
            Sock::Tls(t) => loop {
                if t.ppos < t.plain.len() {
                    let n = (t.plain.len() - t.ppos).min(buf.len());
                    buf[..n].copy_from_slice(&t.plain[t.ppos..t.ppos + n]);
                    t.ppos += n;
                    return Ok(n);
                }
                match t.tls.recv() {
                    Ok(p) if !p.is_empty() => {
                        t.plain = p;
                        t.ppos = 0;
                        continue;
                    }
                    Ok(_) => {}
                    Err(_) => return Err(0),
                }
                if t.tls.received_close_notify() {
                    return Err(0);
                }
                let mut wire = [0u8; 16384];
                let n = unsafe {
                    libc::recv(
                        t.fd.as_raw_fd(),
                        wire.as_mut_ptr() as *mut libc::c_void,
                        wire.len(),
                        0,
                    )
                };
                if n < 0 {
                    let e = errno();
                    if e == libc::EINTR {
                        continue;
                    }
                    if e == libc::EAGAIN || e == libc::EWOULDBLOCK {
                        return Ok(0);
                    }
                    return Err(e);
                }
                if n == 0 {
                    return Err(0);
                }
                if t.tls.feed(&wire[..n as usize]).is_err() {
                    return Err(0);
                }
                if let Ok(out) = t.tls.pop() {
                    t.wire_out.extend_from_slice(&out);
                }
                if !t.wire_out.is_empty() {
                    let w = std::mem::take(&mut t.wire_out);
                    send_all_fd(t.fd.as_raw_fd(), &w);
                }
            },
        }
    }

    /// Whether TLS holds decrypted input not yet read
    fn read_pending(&self) -> bool {
        match &self.sock {
            Some(Sock::Tls(t)) => t.ppos < t.plain.len(),
            _ => false,
        }
    }

    /// pqReadData: >0 bytes read, 0 nothing available, -1 error (the
    /// connection is then marked bad).
    pub fn read_data(&mut self) -> i32 {
        if self.sock.is_none() {
            self.append_error("connection not open");
            return -1;
        }
        // compact the buffer
        if self.inpos > 0 {
            self.inbuf.drain(..self.inpos);
            self.inpos = 0;
        }
        let mut total = 0usize;
        let mut buf = vec![0u8; 65536];
        loop {
            match self.sock_read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    self.inbuf.extend_from_slice(&buf[..n]);
                    total += n;
                    if n < buf.len() && !self.read_pending() {
                        break;
                    }
                }
                Err(e) => {
                    if total > 0 {
                        // report the data; the EOF shows up next time
                        break;
                    }
                    if e != 0 && e != libc::ECONNRESET {
                        self.append_error(&format!(
                            "could not receive data from server: {}",
                            strerror(e)
                        ));
                    } else {
                        self.append_error(
                            "server closed the connection unexpectedly\n\tThis probably means the server terminated abnormally\n\tbefore or while processing the request.",
                        );
                    }
                    self.status = ConnStatus::Bad;
                    self.close_socket();
                    return -1;
                }
            }
        }
        total as i32
    }

    fn sock_write(&mut self, data: &[u8]) -> Result<usize, i32> {
        let Some(sock) = self.sock.as_mut() else {
            return Err(0);
        };
        match sock {
            Sock::Plain(fd) => loop {
                let n = unsafe {
                    libc::send(
                        fd.as_raw_fd(),
                        data.as_ptr() as *const libc::c_void,
                        data.len(),
                        libc::MSG_NOSIGNAL,
                    )
                };
                if n < 0 {
                    let e = errno();
                    if e == libc::EINTR {
                        continue;
                    }
                    if e == libc::EAGAIN || e == libc::EWOULDBLOCK {
                        return Ok(0);
                    }
                    return Err(e);
                }
                return Ok(n as usize);
            },
            Sock::Tls(t) => {
                if t.tls.send(data).is_err() {
                    return Err(0);
                }
                if let Ok(out) = t.tls.pop() {
                    t.wire_out.extend_from_slice(&out);
                }
                let w = std::mem::take(&mut t.wire_out);
                if !send_all_fd(t.fd.as_raw_fd(), &w) {
                    return Err(errno());
                }
                Ok(data.len())
            }
        }
    }

    /// pqSendSome / pqFlush: 0 all sent, 1 data remain (non-blocking), -1
    /// failure.
    pub fn flush(&mut self) -> i32 {
        if self.sock.is_none() {
            self.outbuf.clear();
            return -1;
        }
        while !self.outbuf.is_empty() {
            if self.write_failed {
                self.outbuf.clear();
                return 0;
            }
            let data = std::mem::take(&mut self.outbuf);
            match self.sock_write(&data) {
                Ok(n) => {
                    self.outbuf = data[n..].to_vec();
                    if n == 0 || !self.outbuf.is_empty() {
                        if self.nonblocking {
                            return 1;
                        }
                        // wait for write-ready, reading input meanwhile so
                        // that the server does not block on us
                        let fd = self.socket();
                        if poll_fd(fd, true, true, -1) & 1 != 0 && self.read_data() < 0 {
                            return -1;
                        }
                    }
                }
                Err(e) => {
                    // remember the failure; the error comes with the input
                    self.write_failed = true;
                    if e != 0 && e != libc::EPIPE && e != libc::ECONNRESET {
                        self.append_error(&format!(
                            "could not send data to server: {}",
                            strerror(e)
                        ));
                    }
                    self.outbuf.clear();
                    return 0;
                }
            }
        }
        0
    }

    /// pqWait / pqWaitTimed: 0 ready, 1 timeout, -1 failure
    pub fn wait_timed(&mut self, forread: bool, forwrite: bool, deadline: Option<Instant>) -> i32 {
        if self.sock.is_none() {
            self.append_error("invalid socket");
            return -1;
        }
        if forread && self.read_pending() {
            return 0;
        }
        let fd = self.socket();
        loop {
            let ms = match deadline {
                Some(d) => {
                    let now = Instant::now();
                    if now >= d {
                        return 1;
                    }
                    (d - now).as_millis().min(i32::MAX as u128) as i32
                }
                None => -1,
            };
            let r = poll_fd(fd, forread, forwrite, ms);
            if r < 0 {
                let e = errno();
                if e == libc::EINTR {
                    continue;
                }
                self.append_error(&format!("{}() failed: {}", "poll", strerror(e)));
                return -1;
            }
            if r == 0 {
                if deadline.is_some() {
                    return 1;
                }
                continue;
            }
            return 0;
        }
    }

    pub fn wait(&mut self, forread: bool, forwrite: bool) -> i32 {
        self.wait_timed(forread, forwrite, None)
    }

    /// Queue a protocol message (pqPutMsgStart .. pqPutMsgEnd)
    pub fn put_msg(&mut self, ty: u8, body: &[u8]) {
        self.trace_out(ty, body);
        self.outbuf.push(ty);
        self.outbuf
            .extend_from_slice(&((body.len() + 4) as i32).to_be_bytes());
        self.outbuf.extend_from_slice(body);
        // pqPutMsgEnd: flush once the buffer gets big
        if self.outbuf.len() >= 8192 {
            let _ = self.flush_some();
        }
    }

    /// Try to send without waiting (pqPutMsgEnd's partial flush)
    fn flush_some(&mut self) -> i32 {
        let nb = self.nonblocking;
        self.nonblocking = true;
        let r = self.flush();
        self.nonblocking = nb;
        r
    }

    fn trace_out(&mut self, ty: u8, body: &[u8]) {
        if let Some(f) = self.trace.as_mut() {
            super::trace::trace_message(f, self.trace_flags, false, ty, body);
        }
    }

    pub(crate) fn trace_in(&mut self, ty: u8, body: &[u8]) {
        if let Some(f) = self.trace.as_mut() {
            super::trace::trace_message(f, self.trace_flags, true, ty, body);
        }
    }

    /// PQtrace
    pub fn set_trace(&mut self, f: std::fs::File) {
        self.trace = Some(f);
        self.trace_flags = 0;
    }

    /// PQuntrace
    pub fn untrace(&mut self) {
        if let Some(mut f) = self.trace.take() {
            let _ = f.flush();
        }
        self.trace_flags = 0;
    }

    // ── the protocol 3 parser ──────────────────────────────────────────

    /// pqGetErrorNotice3: an ErrorResponse / NoticeResponse
    pub(crate) fn get_error_notice(&mut self, body: &[u8], is_error: bool) {
        let mut res = PgResult::new(if is_error {
            ExecStatus::FatalError
        } else {
            ExecStatus::NonfatalError
        });
        res.client_encoding = self.client_encoding;
        let mut c = Cur::new(body);
        let mut have_position = false;
        while let Some(code) = c.byte() {
            if code == 0 {
                break;
            }
            let Some(v) = c.cstr() else {
                break;
            };
            if code == PG_DIAG_STATEMENT_POSITION {
                have_position = true;
            }
            res.err_fields.insert(0, (code, v.to_vec()));
        }
        if is_error && have_position && self.cmd_queued {
            res.err_query = self.last_query.clone();
        }
        let msg = build_error_message(&res, self.verbosity, self.show_context);
        res.err_msg = Some(msg.clone());
        if is_error {
            self.result = Some(res);
            self.next_result = None;
            self.error_message.extend_from_slice(&msg);
        } else {
            // the notice processor gets the formatted message
            (self.notice_processor)(self.notice_arg, &msg);
        }
    }

    /// The parameter status message
    fn get_parameter_status(&mut self, body: &[u8]) {
        let mut c = Cur::new(body);
        let (Some(k), Some(v)) = (c.cstr(), c.cstr()) else {
            return;
        };
        let (k, v) = (k.to_vec(), v.to_vec());
        // pqSaveParameterStatus
        self.param_status.retain(|(a, _)| *a != k);
        if k == b"client_encoding" {
            self.client_encoding = encoding::char_to_encoding(&String::from_utf8_lossy(&v));
            let enc = self.client_encoding;
            super::escape::STATIC.with(|s| s.set((enc, s.get().1)));
        } else if k == b"standard_conforming_strings" {
            self.std_strings = v == b"on";
            let std = self.std_strings;
            super::escape::STATIC.with(|s| s.set((s.get().0, std)));
        } else if k == b"server_version" {
            self.sversion = parse_server_version(&v);
        }
        self.param_status.insert(0, (k, v));
    }

    fn get_notify(&mut self, body: &[u8]) {
        let mut c = Cur::new(body);
        let pid = c.int32().unwrap_or(0);
        let relname = c.cstr().unwrap_or(b"").to_vec();
        let extra = c.cstr().unwrap_or(b"").to_vec();
        self.notifies.push_back(Notify {
            relname,
            be_pid: pid,
            extra,
        });
    }

    fn make_result(&self, status: ExecStatus) -> PgResult {
        let mut r = PgResult::new(status);
        r.client_encoding = self.client_encoding;
        match status {
            ExecStatus::EmptyQuery
            | ExecStatus::CommandOk
            | ExecStatus::TuplesOk
            | ExecStatus::TuplesChunk
            | ExecStatus::CopyOut
            | ExecStatus::CopyIn
            | ExecStatus::CopyBoth
            | ExecStatus::SingleTuple
            | ExecStatus::PipelineSync => {}
            _ => {
                // pqSetResultError from the connection's message
                let m = self.error_message[self.error_reported.min(self.error_message.len())..]
                    .to_vec();
                r.err_msg = Some(m);
            }
        }
        r
    }

    /// PQmakeEmptyPGresult with the connection
    pub fn make_empty_result(&self, status: ExecStatus) -> PgResult {
        self.make_result(status)
    }

    /// getRowDescriptions
    fn get_row_descriptions(&mut self, body: &[u8]) {
        let mut c = Cur::new(body);
        let n = c.int16().unwrap_or(0).max(0) as usize;
        let mut attrs = Vec::with_capacity(n);
        let mut binary = n > 0;
        for _ in 0..n {
            let name = c.cstr().unwrap_or(b"").to_vec();
            let tableid = c.int32().unwrap_or(0) as u32;
            let columnid = c.int16().unwrap_or(0) as i32;
            let typid = c.int32().unwrap_or(0) as u32;
            let typlen = c.int16().unwrap_or(0) as i32;
            let atttypmod = c.int32().unwrap_or(0);
            let format = c.int16().unwrap_or(0) as i32;
            if format != 1 {
                binary = false;
            }
            attrs.push(AttDesc {
                name,
                tableid,
                columnid,
                format,
                typid,
                typlen,
                atttypmod,
            });
        }
        if self.queryclass == QueryClass::Describe {
            let mut r = self
                .result
                .take()
                .unwrap_or_else(|| self.make_result(ExecStatus::CommandOk));
            r.attrs = attrs;
            r.binary = binary;
            self.result = Some(r);
            self.async_status = AsyncStatus::Ready;
            return;
        }
        let mut r = self.make_result(ExecStatus::TuplesOk);
        r.memory_size += attrs.iter().map(|a| 40 + a.name.len() + 1).sum::<usize>();
        r.attrs = attrs;
        r.binary = binary;
        self.result = Some(r);
    }

    /// getAnotherTuple
    fn get_another_tuple(&mut self, body: &[u8]) {
        if self.result.is_none() {
            self.append_error("unexpected field count in \"D\" message");
            let r = self.make_result(ExecStatus::FatalError);
            self.result = Some(r);
            self.async_status = AsyncStatus::Ready;
            return;
        }
        let mut c = Cur::new(body);
        let n = c.int16().unwrap_or(0).max(0) as usize;
        let mut row = Vec::with_capacity(n);
        let mut size = n * 16;
        for _ in 0..n {
            let len = c.int32().unwrap_or(-1);
            if len < 0 {
                row.push(None);
            } else {
                let v = c.bytes(len as usize).unwrap_or(b"").to_vec();
                size += v.len() + 1;
                row.push(Some(v));
            }
        }
        let max = self.max_chunk_rows;
        let single = self.single_row_mode;
        let r = self.result.as_mut().unwrap();
        r.memory_size += size + 8;
        r.tuples.push(row);
        if self.partial_result_mode && max > 0 && r.tuples.len() >= max {
            let mut chunk = r.clone();
            chunk.status = if single {
                ExecStatus::SingleTuple
            } else {
                ExecStatus::TuplesChunk
            };
            r.tuples.clear();
            r.memory_size = 208;
            let rest = self.result.take();
            self.next_result = rest;
            self.result = Some(chunk);
            self.async_status = AsyncStatus::ReadyMore;
        }
    }

    /// getParamDescriptions
    fn get_param_descriptions(&mut self, body: &[u8]) {
        let mut c = Cur::new(body);
        let n = c.int16().unwrap_or(0).max(0) as usize;
        let mut params = Vec::with_capacity(n);
        for _ in 0..n {
            params.push(c.int32().unwrap_or(0) as u32);
        }
        let mut r = self.make_result(ExecStatus::CommandOk);
        r.params = params;
        self.result = Some(r);
    }

    /// getCopyStart
    fn get_copy_start(&mut self, body: &[u8], status: ExecStatus) {
        let mut r = self.make_result(status);
        let mut c = Cur::new(body);
        let fmt = c.byte().unwrap_or(0);
        r.binary = fmt != 0;
        let n = c.int16().unwrap_or(0).max(0) as usize;
        for _ in 0..n {
            let f = c.int16().unwrap_or(0) as i32;
            r.attrs.push(AttDesc {
                format: f,
                ..AttDesc::default()
            });
        }
        self.copy_is_binary = r.binary;
        self.result = Some(r);
    }

    /// pqParseInput3
    pub fn parse_input(&mut self) {
        loop {
            let Some(id) = self.peek_message() else {
                return;
            };
            // Asynchronous messages are processed in any state
            if id == b'A' || id == b'N' {
                let (ty, body) = self.take_message().unwrap();
                self.trace_in(ty, &body);
                if ty == b'A' {
                    self.get_notify(&body);
                } else {
                    self.get_error_notice(&body, false);
                }
                continue;
            }
            if self.async_status != AsyncStatus::Busy {
                if self.async_status != AsyncStatus::Idle {
                    // Ready / copy states: leave the message for later
                    return;
                }
                let (ty, body) = self.take_message().unwrap();
                self.trace_in(ty, &body);
                match ty {
                    b'E' => {
                        // a FATAL error while idle (e.g. the server shut down)
                        self.get_error_notice(&body, true);
                        if let Some(r) = self.result.take() {
                            // nobody is waiting for it: pass it to the notice
                            // processor
                            (self.notice_processor)(self.notice_arg, r.error_message());
                        }
                    }
                    b'S' => self.get_parameter_status(&body),
                    _ => {
                        let m = format!("message type 0x{:02x} arrived from server while idle", ty);
                        let mut msg = m.into_bytes();
                        msg.push(b'\n');
                        (self.notice_processor)(self.notice_arg, &msg);
                    }
                }
                continue;
            }
            let (ty, body) = self.take_message().unwrap();
            self.trace_in(ty, &body);
            match ty {
                b'C' => {
                    // CommandComplete
                    if self.result.is_none() {
                        let r = self.make_result(ExecStatus::CommandOk);
                        self.result = Some(r);
                    }
                    let tag = Cur::new(&body).cstr().unwrap_or(b"").to_vec();
                    let r = self.result.as_mut().unwrap();
                    r.cmd_status = tag;
                    self.async_status = AsyncStatus::Ready;
                }
                b'E' => {
                    self.get_error_notice(&body, true);
                    self.async_status = AsyncStatus::Ready;
                }
                b'Z' => {
                    let st = body.first().copied().unwrap_or(b'I');
                    self.xact_status = match st {
                        b'I' => TransStatus::Idle,
                        b'T' => TransStatus::InTrans,
                        b'E' => TransStatus::InError,
                        _ => TransStatus::Unknown,
                    };
                    self.cmd_queued = false;
                    self.async_status = AsyncStatus::Idle;
                }
                b'I' => {
                    // EmptyQueryResponse
                    if self.result.is_none() {
                        let r = self.make_result(ExecStatus::EmptyQuery);
                        self.result = Some(r);
                    }
                    self.async_status = AsyncStatus::Ready;
                }
                b'1' => {
                    // ParseComplete
                    if self.queryclass == QueryClass::Prepare && self.result.is_none() {
                        let r = self.make_result(ExecStatus::CommandOk);
                        self.result = Some(r);
                        self.async_status = AsyncStatus::Ready;
                    }
                }
                b'2' => {}
                b'3' => {
                    // CloseComplete
                    if self.queryclass == QueryClass::Close && self.result.is_none() {
                        let r = self.make_result(ExecStatus::CommandOk);
                        self.result = Some(r);
                        self.async_status = AsyncStatus::Ready;
                    }
                }
                b'S' => self.get_parameter_status(&body),
                b'K' => {
                    let mut c = Cur::new(&body);
                    self.be_pid = c.int32().unwrap_or(0);
                    self.be_key = c.int32().unwrap_or(0);
                }
                b'T' => {
                    if self
                        .result
                        .as_ref()
                        .is_some_and(|r| r.status == ExecStatus::FatalError)
                    {
                        // ignore after an error
                    } else {
                        self.get_row_descriptions(&body);
                    }
                }
                b'n' => {
                    // NoData
                    if self.queryclass == QueryClass::Describe {
                        if self.result.is_none() {
                            let r = self.make_result(ExecStatus::CommandOk);
                            self.result = Some(r);
                        }
                        self.async_status = AsyncStatus::Ready;
                    }
                }
                b't' => self.get_param_descriptions(&body),
                b'D' => {
                    if self.result.as_ref().is_some_and(|r| {
                        r.status == ExecStatus::TuplesOk || r.status == ExecStatus::FatalError
                    }) {
                        if self.result.as_ref().unwrap().status == ExecStatus::TuplesOk {
                            self.get_another_tuple(&body);
                        }
                    } else {
                        self.append_error(
                            "server sent data (\"D\" message) without prior row description (\"T\" message)",
                        );
                        let r = self.make_result(ExecStatus::FatalError);
                        self.result = Some(r);
                        self.async_status = AsyncStatus::Ready;
                    }
                }
                b'G' => {
                    self.get_copy_start(&body, ExecStatus::CopyIn);
                    self.async_status = AsyncStatus::CopyIn;
                }
                b'H' => {
                    self.get_copy_start(&body, ExecStatus::CopyOut);
                    self.async_status = AsyncStatus::CopyOut;
                    self.copy_already_done = 0;
                }
                b'W' => {
                    self.get_copy_start(&body, ExecStatus::CopyBoth);
                    self.async_status = AsyncStatus::CopyBoth;
                    self.copy_already_done = 0;
                }
                b'd' | b'c' => {
                    // copy data / done outside a COPY: ignore
                }
                b's' => {}
                _ => {
                    self.append_error(&format!(
                        "unexpected response from server; first received character was \"{}\"",
                        ty as char
                    ));
                    let r = self.make_result(ExecStatus::FatalError);
                    self.result = Some(r);
                    self.async_status = AsyncStatus::Ready;
                }
            }
        }
    }

    /// pqPrepareAsyncResult
    fn prepare_async_result(&mut self) -> PgResult {
        let res = match self.result.take() {
            Some(r) => {
                if r.status == ExecStatus::FatalError {
                    self.error_reported = self.error_message.len();
                }
                r
            }
            None => {
                let r = self.make_result(ExecStatus::FatalError);
                self.error_reported = self.error_message.len();
                r
            }
        };
        self.result = self.next_result.take();
        res
    }

    /// PQisBusy
    pub fn is_busy(&mut self) -> bool {
        self.parse_input();
        self.async_status == AsyncStatus::Busy || self.write_failed
    }

    /// PQgetResult
    pub fn get_result(&mut self) -> Option<PgResult> {
        self.parse_input();
        while self.async_status == AsyncStatus::Busy {
            // flush and wait for input
            let mut flush_result;
            loop {
                flush_result = self.flush();
                if flush_result <= 0 {
                    break;
                }
                if self.wait(false, true) != 0 {
                    flush_result = -1;
                    break;
                }
            }
            if flush_result != 0 || self.wait(true, false) != 0 || self.read_data() < 0 {
                // pqSaveErrorResult
                self.result = None;
                self.next_result = None;
                self.async_status = AsyncStatus::Idle;
                self.cmd_queued = false;
                let r = self.make_result(ExecStatus::FatalError);
                self.error_reported = self.error_message.len();
                return Some(r);
            }
            self.parse_input();
            if self.write_failed && self.async_status == AsyncStatus::Busy && self.sock.is_none() {
                break;
            }
        }
        match self.async_status {
            AsyncStatus::Idle => None,
            AsyncStatus::Ready => {
                let r = self.prepare_async_result();
                self.async_status = AsyncStatus::Busy;
                Some(r)
            }
            AsyncStatus::ReadyMore => {
                let r = self.prepare_async_result();
                self.async_status = AsyncStatus::Busy;
                Some(r)
            }
            AsyncStatus::CopyIn => Some(self.copy_result(ExecStatus::CopyIn)),
            AsyncStatus::CopyOut => Some(self.copy_result(ExecStatus::CopyOut)),
            AsyncStatus::CopyBoth => Some(self.copy_result(ExecStatus::CopyBoth)),
            AsyncStatus::Busy => {
                let r = self.make_result(ExecStatus::FatalError);
                self.async_status = AsyncStatus::Idle;
                Some(r)
            }
        }
    }

    /// getCopyResult
    fn copy_result(&mut self, status: ExecStatus) -> PgResult {
        if self.status != ConnStatus::Ok {
            let r = self.make_result(ExecStatus::FatalError);
            self.async_status = AsyncStatus::Idle;
            return r;
        }
        match self.result.as_ref() {
            Some(r) if r.status == status => self.prepare_async_result(),
            _ => self.make_result(status),
        }
    }
}

impl Drop for PgConn {
    fn drop(&mut self) {
        if self.sock.is_some() {
            self.finish();
        }
    }
}

/// "17.11" / "9.6.24" / "18beta1" → 170011 / 90624 / 180000
pub fn parse_server_version(v: &[u8]) -> i32 {
    let s = String::from_utf8_lossy(v);
    let mut nums = Vec::new();
    let mut cur = String::new();
    for c in s.chars() {
        if c.is_ascii_digit() {
            cur.push(c);
        } else if c == '.' {
            nums.push(cur.parse::<i32>().unwrap_or(0));
            cur.clear();
        } else {
            break;
        }
    }
    if !cur.is_empty() {
        nums.push(cur.parse::<i32>().unwrap_or(0));
    }
    match nums.as_slice() {
        [] => 0,
        [a] => a * 10000,
        [a, b] if *a >= 10 => a * 10000 + b,
        [a, b] => (a * 100 + b) * 100,
        [a, b, c, ..] if *a >= 10 => a * 10000 + b.max(c),
        [a, b, c, ..] => (a * 100 + b) * 100 + c,
    }
}

/// poll(2) on one descriptor: bit 0 readable, bit 1 writable; 0 timeout,
/// -1 error
pub fn poll_fd(fd: RawFd, read: bool, write: bool, ms: i32) -> i32 {
    let mut ev: libc::c_short = 0;
    if read {
        ev |= libc::POLLIN;
    }
    if write {
        ev |= libc::POLLOUT;
    }
    let mut p = libc::pollfd {
        fd,
        events: ev | libc::POLLERR,
        revents: 0,
    };
    let r = unsafe { libc::poll(&mut p, 1, ms) };
    if r <= 0 {
        return r;
    }
    let mut out = 0;
    if p.revents & (libc::POLLIN | libc::POLLHUP | libc::POLLERR) != 0 {
        out |= 1;
    }
    if p.revents & (libc::POLLOUT | libc::POLLERR | libc::POLLHUP) != 0 {
        out |= 2;
    }
    if out == 0 { 1 } else { out }
}

fn send_all_fd(fd: RawFd, data: &[u8]) -> bool {
    let mut off = 0;
    while off < data.len() {
        let n = unsafe {
            libc::send(
                fd,
                data[off..].as_ptr() as *const libc::c_void,
                data.len() - off,
                libc::MSG_NOSIGNAL,
            )
        };
        if n < 0 {
            let e = errno();
            if e == libc::EINTR {
                continue;
            }
            if e == libc::EAGAIN || e == libc::EWOULDBLOCK {
                poll_fd(fd, false, true, 1000);
                continue;
            }
            return false;
        }
        off += n as usize;
    }
    true
}

/// Create a non-blocking socket and start connecting it: Ok((fd, in
/// progress)) or Err((errno, 0 connect / 1 socket))
fn connect_socket(addr: &Addr) -> Result<(OwnedFd, bool), (i32, u8)> {
    unsafe {
        let (domain, sa, len): (i32, libc::sockaddr_storage, libc::socklen_t) = match addr {
            Addr::Unix(path) => {
                let mut sun: libc::sockaddr_un = std::mem::zeroed();
                sun.sun_family = libc::AF_UNIX as libc::sa_family_t;
                for (i, b) in path.bytes().enumerate().take(sun.sun_path.len() - 1) {
                    sun.sun_path[i] = b as libc::c_char;
                }
                let mut ss: libc::sockaddr_storage = std::mem::zeroed();
                std::ptr::copy_nonoverlapping(
                    &sun as *const _ as *const u8,
                    &mut ss as *mut _ as *mut u8,
                    std::mem::size_of::<libc::sockaddr_un>(),
                );
                (
                    libc::AF_UNIX,
                    ss,
                    std::mem::size_of::<libc::sockaddr_un>() as libc::socklen_t,
                )
            }
            Addr::Inet(std::net::SocketAddr::V4(a)) => {
                let mut sin: libc::sockaddr_in = std::mem::zeroed();
                sin.sin_family = libc::AF_INET as libc::sa_family_t;
                sin.sin_port = a.port().to_be();
                sin.sin_addr.s_addr = u32::from_ne_bytes(a.ip().octets());
                let mut ss: libc::sockaddr_storage = std::mem::zeroed();
                std::ptr::copy_nonoverlapping(
                    &sin as *const _ as *const u8,
                    &mut ss as *mut _ as *mut u8,
                    std::mem::size_of::<libc::sockaddr_in>(),
                );
                (
                    libc::AF_INET,
                    ss,
                    std::mem::size_of::<libc::sockaddr_in>() as libc::socklen_t,
                )
            }
            Addr::Inet(std::net::SocketAddr::V6(a)) => {
                let mut sin6: libc::sockaddr_in6 = std::mem::zeroed();
                sin6.sin6_family = libc::AF_INET6 as libc::sa_family_t;
                sin6.sin6_port = a.port().to_be();
                sin6.sin6_addr.s6_addr = a.ip().octets();
                sin6.sin6_scope_id = a.scope_id();
                let mut ss: libc::sockaddr_storage = std::mem::zeroed();
                std::ptr::copy_nonoverlapping(
                    &sin6 as *const _ as *const u8,
                    &mut ss as *mut _ as *mut u8,
                    std::mem::size_of::<libc::sockaddr_in6>(),
                );
                (
                    libc::AF_INET6,
                    ss,
                    std::mem::size_of::<libc::sockaddr_in6>() as libc::socklen_t,
                )
            }
        };
        let fd = libc::socket(domain, libc::SOCK_STREAM | libc::SOCK_CLOEXEC, 0);
        if fd < 0 {
            return Err((errno(), 1));
        }
        let owned = OwnedFd::from_raw_fd(fd);
        if domain != libc::AF_UNIX {
            let one: i32 = 1;
            libc::setsockopt(
                fd,
                libc::IPPROTO_TCP,
                libc::TCP_NODELAY,
                &one as *const i32 as *const libc::c_void,
                4,
            );
            libc::setsockopt(
                fd,
                libc::SOL_SOCKET,
                libc::SO_KEEPALIVE,
                &one as *const i32 as *const libc::c_void,
                4,
            );
        }
        set_nonblock(fd, true);
        let r = libc::connect(fd, &sa as *const _ as *const libc::sockaddr, len);
        if r == 0 {
            return Ok((owned, false));
        }
        let e = errno();
        if e == libc::EINPROGRESS || e == libc::EINTR || e == libc::EWOULDBLOCK {
            return Ok((owned, true));
        }
        Err((e, 0))
    }
}

/// getaddrinfo(3), with gai_strerror() messages
fn getaddrinfo(host: &str, port: u16) -> Result<Vec<std::net::SocketAddr>, String> {
    use std::ffi::{CStr, CString};
    let Ok(h) = CString::new(host) else {
        return Err("Name or service not known".into());
    };
    let p = CString::new(port.to_string()).unwrap();
    unsafe {
        let mut hints: libc::addrinfo = std::mem::zeroed();
        hints.ai_family = libc::AF_UNSPEC;
        hints.ai_socktype = libc::SOCK_STREAM;
        let mut res: *mut libc::addrinfo = std::ptr::null_mut();
        let r = libc::getaddrinfo(h.as_ptr(), p.as_ptr(), &hints, &mut res);
        if r != 0 {
            let m = libc::gai_strerror(r);
            let s = if m.is_null() {
                "unknown error".to_string()
            } else {
                CStr::from_ptr(m).to_string_lossy().into_owned()
            };
            return Err(s);
        }
        let mut out = Vec::new();
        let mut cur = res;
        while !cur.is_null() {
            let ai = &*cur;
            if ai.ai_family == libc::AF_INET {
                let sin = &*(ai.ai_addr as *const libc::sockaddr_in);
                let ip = std::net::Ipv4Addr::from(u32::from_be(sin.sin_addr.s_addr));
                out.push(std::net::SocketAddr::from((ip, port)));
            } else if ai.ai_family == libc::AF_INET6 {
                let sin6 = &*(ai.ai_addr as *const libc::sockaddr_in6);
                let ip = std::net::Ipv6Addr::from(sin6.sin6_addr.s6_addr);
                out.push(std::net::SocketAddr::V6(std::net::SocketAddrV6::new(
                    ip,
                    port,
                    0,
                    sin6.sin6_scope_id,
                )));
            }
            cur = ai.ai_next;
        }
        libc::freeaddrinfo(res);
        Ok(out)
    }
}
