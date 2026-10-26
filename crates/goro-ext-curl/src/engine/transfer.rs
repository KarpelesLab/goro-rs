//! One easy transfer from start to end: the multi_runsingle() state
//! machine run synchronously (lib/multi.c, lib/transfer.c, lib/url.c,
//! lib/http.c, lib/request.c, lib/sendf.c, lib/connect.c, lib/cf-socket.c).

use std::cell::RefCell;
use std::net::{IpAddr, SocketAddr};
use std::rc::Rc;
use std::time::{Duration, Instant};

use super::consts::{auth, code, httpver, info, opt, proto, proxytype};
use super::content::{ChunkOut, Chunker, ContentDecoder};
use super::easy::{Easy, Timer};
use super::http::*;
use super::mime::MimeReader;
use super::net::{self, Conn, ConnectErr};
use super::options::HttpReq;
use super::proxy::{self, ProxyInfo};
use super::tls::{TlsConfig, TlsLog, TlsSession};
use super::urlapi::{self, FollowParams, FollowType, TransferOptions, TransferUrl};
use super::{Callbacks, CurlCode, Mime, ReadResult, WRITEFUNC_ERROR, WRITEFUNC_PAUSE};

const DEFAULT_CONNECT_TIMEOUT: i64 = 300_000;
const MAX_HTTP_RESP_HEADER_SIZE: usize = 300 * 1024;
const EXPECT_100_THRESHOLD: i64 = 1024 * 1024;
const CURL_MAX_WRITE_SIZE: usize = 16384;
const MAX_COOKIE_HEADER_LEN: usize = 8190;

/// CLIENTWRITE_* flags
const CW_BODY: u32 = 1 << 0;
const CW_INFO: u32 = 1 << 1;
const CW_HEADER: u32 = 1 << 2;
const CW_STATUS: u32 = 1 << 3;
const CW_CONNECT: u32 = 1 << 4;
const CW_1XX: u32 = 1 << 5;
const CW_TRAILER: u32 = 1 << 6;
const CW_EOS: u32 = 1 << 7;

/// The request body source (client readers)
pub(crate) enum BodySrc {
    Null,
    Buf { data: Vec<u8>, pos: usize },
    Fread { total: i64, read: i64, eos: bool },
    Mime(Box<MimeReader>, Rc<RefCell<Mime>>),
}

impl BodySrc {
    /// Curl_creader_total_length()
    fn total_length(&self) -> i64 {
        match self {
            BodySrc::Null => 0,
            BodySrc::Buf { data, .. } => data.len() as i64,
            BodySrc::Fread { total, .. } => *total,
            BodySrc::Mime(r, _) => r.total,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Exp100 {
    None,
    SendingRequest,
    Awaiting(Instant),
    SendData,
    Failed,
}

/// struct SingleRequest
pub(crate) struct Req {
    pub httpreq: HttpReq,
    pub method: Vec<u8>,
    pub httpcode: i64,
    pub httpversion: u8,
    pub httpversion_sent: u8,
    pub header: bool,
    pub headerline: usize,
    pub headerb: Vec<u8>,
    pub size: i64,
    pub maxdownload: i64,
    pub bytecount: i64,
    pub writebytecount: i64,
    pub chunk: bool,
    pub ignore_cl: bool,
    pub ignorebody: bool,
    pub http_bodyless: bool,
    pub download_done: bool,
    pub no_body: bool,
    pub newurl: Option<Vec<u8>>,
    pub location: Option<Vec<u8>>,
    pub content_range: bool,
    pub offset: i64,
    pub timeofdoc: i64,
    pub headerbytecount: i64,
    pub allheadercount: i64,
    pub deductheadercount: i64,
    pub upload_chunky: bool,
    pub authneg: bool,
    pub upload_done: bool,
    pub upload_aborted: bool,
    pub eos_read: bool,
    pub eos_sent: bool,
    pub eos_written: bool,
    pub done: bool,
    pub keep_recv: bool,
    pub keep_send: bool,
    pub started_response: bool,
    pub chunker: Option<Chunker>,
    pub cdecoders: Vec<ContentDecoder>,
    exp100: Exp100,
    pub body: BodySrc,
    pub chunk_enc: Option<ChunkEnc>,
    pub sendbuf: Vec<u8>,
    pub sendbuf_hds_len: usize,
    pub out_errored: bool,
    pub resp_trailer: bool,
    pub pause_retries: u32,
    /// connclose() decided while writing
    pub close_conn: bool,
    /// the request header block (for CURLINFO_HEADER_OUT style uses)
    pub header_out: Vec<u8>,
}

pub(crate) struct ChunkEnc {
    pub read_eos: bool,
    pub eos: bool,
    pub buf: Vec<u8>,
}

impl Req {
    fn new(no_body: bool) -> Req {
        Req {
            httpreq: HttpReq::Get,
            method: b"GET".to_vec(),
            httpcode: 0,
            httpversion: 0,
            httpversion_sent: 0,
            header: true,
            headerline: 0,
            headerb: Vec::new(),
            size: -1,
            maxdownload: -1,
            bytecount: 0,
            writebytecount: 0,
            chunk: false,
            ignore_cl: false,
            ignorebody: false,
            http_bodyless: false,
            download_done: false,
            no_body,
            newurl: None,
            location: None,
            content_range: false,
            offset: 0,
            timeofdoc: 0,
            headerbytecount: 0,
            allheadercount: 0,
            deductheadercount: 0,
            upload_chunky: false,
            authneg: false,
            upload_done: false,
            upload_aborted: false,
            eos_read: false,
            eos_sent: false,
            eos_written: false,
            done: false,
            keep_recv: false,
            keep_send: false,
            started_response: false,
            chunker: None,
            cdecoders: Vec::new(),
            exp100: Exp100::None,
            body: BodySrc::Null,
            chunk_enc: None,
            sendbuf: Vec::new(),
            sendbuf_hds_len: 0,
            out_errored: false,
            resp_trailer: false,
            pause_retries: 0,
            close_conn: false,
            header_out: Vec::new(),
        }
    }
}

/// What a request of this transfer talks to
pub(crate) struct Target {
    pub tu: TransferUrl,
    pub scheme: String,
    pub protocol: i64,
    /// conn->host.name
    pub host: String,
    /// conn->remote_port
    pub port: u16,
    pub ipv6: bool,
    pub proxy: Option<ProxyInfo>,
    pub tunnel: bool,
    pub proxy_user_passwd: bool,
    /// conn->conn_to_host (CURLOPT_CONNECT_TO)
    pub conn_to_host: Option<String>,
    /// conn->conn_to_port (CURLOPT_CONNECT_TO)
    pub conn_to_port: Option<u16>,
}

/// curlx_str_number() on a decimal prefix: at least one digit, value <= max
fn dec_prefix(s: &[u8], max: u64) -> Option<u64> {
    let n = s.iter().take_while(|c| c.is_ascii_digit()).count();
    if n == 0 {
        return None;
    }
    let mut v: u64 = 0;
    for &c in &s[..n] {
        v = v.checked_mul(10)?.checked_add((c - b'0') as u64)?;
        if v > max {
            return None;
        }
    }
    Some(v)
}

/// How a request ended, for the follow logic
enum Next {
    Done,
    Follow(Vec<u8>, FollowType),
}

fn is_ip_literal(h: &str) -> bool {
    h.parse::<IpAddr>().is_ok()
}

fn lossy(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

/// base64 (Curl_base64_encode)
pub(crate) fn base64(input: &[u8]) -> Vec<u8> {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = Vec::with_capacity(input.len().div_ceil(3) * 4);
    for ch in input.chunks(3) {
        let b = [ch[0], *ch.get(1).unwrap_or(&0), *ch.get(2).unwrap_or(&0)];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(T[(n >> 18) as usize & 63]);
        out.push(T[(n >> 12) as usize & 63]);
        out.push(if ch.len() > 1 {
            T[(n >> 6) as usize & 63]
        } else {
            b'='
        });
        out.push(if ch.len() > 2 {
            T[n as usize & 63]
        } else {
            b'='
        });
    }
    out
}

impl Easy {
    /// curl_easy_perform()
    pub fn perform(&mut self, cb: &mut dyn Callbacks) -> CurlCode {
        self.in_perform = true;
        let r = self.perform_inner(cb);
        self.in_perform = false;
        r
    }

    fn perform_inner(&mut self, cb: &mut dyn Callbacks) -> CurlCode {
        self.replay_backlog(cb);
        // curl_multi_add_handle(): a transfer id
        {
            let mp = self.multi_pool();
            let mut p = mp.0.borrow_mut();
            self.id = p.next_xfer_id;
            p.next_xfer_id += 1;
        }
        // MSTATE_INIT
        if let Err(c) = self.pretransfer(cb) {
            return c;
        }
        self.progress.time(Timer::StartOp);
        let mut req_conn: Option<Conn>;
        loop {
            // MSTATE_SETUP
            self.progress.time(Timer::StartSingle);
            // MSTATE_CONNECT
            let target = match self.setup_target(cb) {
                Ok(t) => t,
                Err(c) => {
                    self.infof(cb, "closing connection #-1");
                    self.posttransfer();
                    return c;
                }
            };
            if target.protocol == proto::FILE {
                let r = self.file_transfer(cb, &target);
                return r;
            }
            let mut conn = match self.connect(cb, &target) {
                Ok(c) => c,
                Err(c) => {
                    self.posttransfer();
                    self.progress.calc();
                    return c;
                }
            };
            self.progress.time(Timer::PostQueue);
            // MSTATE_DO: pre-request callback
            if self.set.callbacks.contains(&opt::PREREQFUNCTION) {
                let rc = cb.prereq(
                    &self.info.primary_ip.clone(),
                    &self.info.local_ip.clone(),
                    self.info.primary_port as i32,
                    self.info.local_port as i32,
                );
                if rc != 0 {
                    self.failf(cb, "operation aborted by pre-request callback");
                    self.posttransfer();
                    self.done_conn(cb, conn, false, &target);
                    return code::ABORTED_BY_CALLBACK;
                }
            }
            if self.set.connect_only == 1 {
                self.done_conn(cb, conn, false, &target);
                return code::OK;
            }
            // Curl_init_do()
            if self.set.opt_no_body {
                self.st.httpreq = HttpReq::Head;
            }
            self.progress.ul_cur = 0;
            self.progress.dl_cur = 0;
            self.low_speed_since = None;
            let mut req = Req::new(self.set.opt_no_body);
            self.progress.time(Timer::PreTransfer);
            if let Err(e) = self.http_do(cb, &mut conn, &target, &mut req) {
                // state_do(): multi_done(data, result, FALSE); callback
                // errors make it premature
                self.posttransfer();
                self.progress.calc();
                let premature = matches!(
                    e,
                    code::ABORTED_BY_CALLBACK | code::READ_ERROR | code::WRITE_ERROR
                );
                self.done_conn(cb, conn, premature, &target);
                return e;
            }
            let r = self.sendrecv(cb, &mut conn, &target, &mut req);
            // retry on a dead reused connection
            let mut retry = false;
            if (r.is_ok() && req.done) || r == Err(code::RECV_ERROR) || r == Err(code::SEND_ERROR) {
                match self.retry_request(cb, &mut conn, &req) {
                    Ok(true) => retry = true,
                    Ok(false) => {}
                    Err(e) => {
                        self.posttransfer();
                        conn.close = true;
                        self.done_conn(cb, conn, true, &target);
                        return e;
                    }
                }
            }
            if !retry && let Err(e) = r {
                conn.close = true;
                self.posttransfer();
                // multi_done(premature): writers flushed, connection closed
                self.progress.calc();
                self.done_conn(cb, conn, true, &target);
                return e;
            }
            // Curl_http_done(): nothing at all received
            if !retry
                && self.set.connect_only == 0
                && req.bytecount + req.headerbytecount - req.deductheadercount <= 0
            {
                self.failf(cb, "Empty reply from server");
                conn.close = true;
                self.posttransfer();
                self.progress.calc();
                self.done_conn(cb, conn, false, &target);
                return code::GOT_NOTHING;
            }
            self.posttransfer();
            let next = if retry {
                Next::Follow(self.st.url.clone().unwrap_or_default(), FollowType::Retry)
            } else if let Some(n) = req.newurl.take() {
                Next::Follow(n, FollowType::Redir)
            } else {
                Next::Done
            };
            let location = req.location.take();
            // multi_done()
            let r = self.multi_done(cb, &mut req);
            req_conn = Some(conn);
            if let Some(c) = req_conn.take() {
                self.done_conn(cb, c, false, &target);
            }
            if let Err(e) = r {
                return e;
            }
            match next {
                Next::Done => {
                    if let Some(loc) = location {
                        // FOLLOW_FAKE: figure out CURLINFO_REDIRECT_URL
                        if let Err(e) = self.follow(cb, &loc, FollowType::Fake, &target, &req) {
                            return e;
                        }
                    }
                    return code::OK;
                }
                Next::Follow(url, kind) => {
                    if let Err(e) = self.follow(cb, &url, kind, &target, &req) {
                        return e;
                    }
                }
            }
        }
    }

    /// multi_posttransfer()
    fn posttransfer(&mut self) {}

    /// The parts of multi_done() that may fail: Curl_pgrsDone
    fn multi_done(&mut self, cb: &mut dyn Callbacks, _req: &mut Req) -> Result<(), CurlCode> {
        self.progress.calc();
        if self.pgrs_update(cb) {
            return Err(code::ABORTED_BY_CALLBACK);
        }
        Ok(())
    }

    /// Curl_pretransfer()
    fn pretransfer(&mut self, cb: &mut dyn Callbacks) -> Result<(), CurlCode> {
        self.errbuf.clear();
        self.errbuf_set = false;
        let Some(url) = self.set.strs.get(&opt::URL).cloned() else {
            self.failf(cb, "No URL set");
            return Err(code::URL_MALFORMAT);
        };
        self.st.url = Some(url);
        if self.set.postfields.is_some() && self.set.resume_from != 0 {
            self.failf(cb, "cannot mix POSTFIELDS with RESUME_FROM");
            return Err(code::BAD_FUNCTION_ARGUMENT);
        }
        self.st.httpreq = self.set.method;
        self.st.requests = 0;
        self.st.followlocation = 0;
        self.st.this_is_a_follow = false;
        self.st.rcvd_min = 0;
        self.st.authproblem = false;
        self.st.authhost.want = self.set.httpauth;
        self.st.authhost.iestyle = self.set.digest_ie;
        self.st.authproxy.want = self.set.proxyauth;
        self.info.wouldredirect = None;
        self.st.http_ignorecustom = false;
        self.st.disableexpect = false;
        self.st.infilesize = match self.st.httpreq {
            HttpReq::Put => self.set.filesize,
            HttpReq::Get | HttpReq::Head => 0,
            _ => {
                let mut s = self.set.postfieldsize;
                if s == -1
                    && let Some(p) = &self.set.postfields
                {
                    s = p.len() as i64;
                }
                s
            }
        };
        // cookie files
        self.load_cookie_files(cb);
        // CURLOPT_RESOLVE
        self.load_hostpairs(cb);
        self.st.allow_port = true;
        self.info.init();
        self.info.ssl_verifyresult = 0;
        self.info.proxy_ssl_verifyresult = 0;
        self.info.os_errno = 0;
        self.info.used_proxy = false;
        self.progress.set_download_size(-1);
        self.progress.set_upload_size(-1);
        self.progress.start_now();
        self.st.authhost.picked &= self.st.authhost.want;
        self.st.authproxy.picked &= self.st.authproxy.want;
        self.st.uagent = self.set.strs.get(&opt::USERAGENT).map(|ua| {
            let mut v = b"User-Agent: ".to_vec();
            v.extend_from_slice(ua);
            v.extend_from_slice(b"\r\n");
            v
        });
        self.st.creds_from_url = false;
        self.st.user = self.set.strs.get(&opt::USERNAME).cloned();
        self.st.passwd = self.set.strs.get(&opt::PASSWORD).cloned();
        self.st.proxyuser = self.set.strs.get(&opt::PROXYUSERNAME).cloned();
        self.st.proxypasswd = self.set.strs.get(&opt::PROXYPASSWORD).cloned();
        self.st.resume_from = self.set.resume_from;
        Ok(())
    }

    /// Curl_loadhostpairs(): CURLOPT_RESOLVE entries into the DNS cache
    fn load_hostpairs(&mut self, cb: &mut dyn Callbacks) {
        if !self.resolve_pending {
            return;
        }
        self.resolve_pending = false;
        let Some(list) = self.set.slists.get(&opt::RESOLVE).cloned() else {
            return;
        };
        let dns = self.dns_cache();
        for entry in list {
            let s = lossy(&entry);
            if let Some(rest) = s.strip_prefix('-') {
                // remove entry: "-host:port"
                if let Some((h, p)) = rest.rsplit_once(':')
                    && let Ok(port) = p.parse::<u16>()
                {
                    dns.borrow_mut()
                        .entries
                        .remove(&(h.to_ascii_lowercase(), port));
                }
                continue;
            }
            let (permanent, s) = match s.strip_prefix('+') {
                Some(r) => (false, r.to_string()),
                None => (true, s),
            };
            let mut it = s.splitn(3, ':');
            let (Some(host), Some(port), Some(addrs)) = (it.next(), it.next(), it.next()) else {
                self.infof(cb, &format!("Couldn't parse CURLOPT_RESOLVE entry '{s}'"));
                continue;
            };
            let Ok(port) = port.parse::<u16>() else {
                self.infof(cb, &format!("Couldn't parse CURLOPT_RESOLVE entry '{s}'"));
                continue;
            };
            let mut ips = Vec::new();
            for a in addrs.split(',') {
                let a = a.trim().trim_start_matches('[').trim_end_matches(']');
                if let Ok(ip) = a.parse::<IpAddr>() {
                    ips.push(ip);
                }
            }
            if ips.is_empty() {
                continue;
            }
            let joined: Vec<String> = ips.iter().map(|i| i.to_string()).collect();
            self.infof(
                cb,
                &format!("Added {host}:{port}:{} to DNS cache", joined.join(",")),
            );
            dns.borrow_mut().put(host, port, ips, permanent);
        }
    }

    /// The DNS cache in use (share DNS > multi)
    pub(crate) fn dns_cache(&self) -> Rc<RefCell<net::DnsCache>> {
        if let Some(sh) = &self.share {
            let s = sh.borrow();
            if s.shares(super::consts::lock::DNS)
                && let Some(d) = &s.dns
            {
                return d.clone();
            }
        }
        // the multi's cache lives inside its pool
        self.multi_pool().0.borrow().dns.clone()
    }

    /// parseurlandfillconn() + proxy setup: what to talk to
    fn setup_target(&mut self, cb: &mut dyn Callbacks) -> Result<Target, CurlCode> {
        let url = self.st.url.clone().unwrap_or_default();
        let defproto = self.set.strs.get(&opt::DEFAULT_PROTOCOL).cloned();
        let opts = TransferOptions {
            default_protocol: defproto.as_deref(),
            path_as_is: self.set.path_as_is,
            disallow_username_in_url: self.set.disallow_username_in_url,
            this_is_a_follow: self.st.this_is_a_follow,
            allowed_protocols: self.set.allowed_protocols as u64,
            redir_protocols: self.set.redir_protocols as u64,
            schemes: urlapi::SCHEMES,
        };
        let tu = match urlapi::parse_transfer_url(&url, &opts) {
            Ok(t) => t,
            Err(e) => {
                if !e.message.is_empty() {
                    self.failf(cb, &e.message);
                }
                return Err(e.code);
            }
        };
        self.st.url = Some(tu.url.clone());
        let scheme = lossy(&tu.scheme).to_ascii_lowercase();
        let protocol = tu.handler.protocol as i64;
        // credentials from the URL
        if tu.user.is_some() || tu.password.is_some() {
            if let Some(u) = &tu.user {
                self.st.user = Some(u.clone());
            }
            self.st.passwd = Some(tu.password.clone().unwrap_or_default());
            self.st.creds_from_url = true;
        }
        let host = lossy(&tu.host_name);
        let mut port = tu.port.unwrap_or(tu.handler.defport);
        if self.set.use_port != 0 && self.st.allow_port {
            port = self.set.use_port as u16;
        }
        let ipv6 = tu.ipv6_ip;
        let mut t = Target {
            tu,
            scheme,
            protocol,
            host,
            port,
            ipv6,
            proxy: None,
            tunnel: self.set.tunnel_proxy,
            proxy_user_passwd: false,
            conn_to_host: None,
            conn_to_port: None,
        };
        if protocol != proto::FILE {
            self.setup_proxy(cb, &mut t)?;
        }
        self.parse_connect_to_slist(cb, &mut t)?;
        self.info.conn_scheme = Some(t.tu.handler.name.to_string());
        self.info.conn_protocol = protocol;
        Ok(t)
    }

    /// parse_connect_to_slist(): the first CURLOPT_CONNECT_TO entry matching
    /// the URL's host and port picks the host and/or port to connect to
    fn parse_connect_to_slist(
        &mut self,
        cb: &mut dyn Callbacks,
        t: &mut Target,
    ) -> Result<(), CurlCode> {
        let Some(list) = self.set.slists.get(&opt::CONNECT_TO).cloned() else {
            return Ok(());
        };
        for line in &list {
            let line = &line[..line.iter().position(|&c| c == 0).unwrap_or(line.len())];
            let (host, port) = self.parse_connect_to_string(cb, t, line)?;
            match host.filter(|h| !h.is_empty()) {
                Some(h) => {
                    self.infof(cb, &format!("Connecting to hostname: {h}"));
                    t.conn_to_host = Some(h);
                }
                None => t.conn_to_host = None,
            }
            match port {
                Some(p) => {
                    self.infof(cb, &format!("Connecting to port: {p}"));
                    t.conn_to_port = Some(p);
                }
                None => t.conn_to_port = None,
            }
            if t.conn_to_host.is_some() || t.conn_to_port.is_some() {
                break;
            }
        }
        Ok(())
    }

    /// parse_connect_to_string()
    fn parse_connect_to_string(
        &mut self,
        cb: &mut dyn Callbacks,
        t: &Target,
        line: &[u8],
    ) -> Result<(Option<String>, Option<u16>), CurlCode> {
        let mut ptr = 0usize;
        let host_match;
        if line.first() == Some(&b':') {
            host_match = true;
            ptr += 1;
        } else {
            let m = if t.ipv6 {
                format!("[{}]", t.host)
            } else {
                t.host.clone()
            };
            let m = m.as_bytes();
            host_match = line.len() >= m.len()
                && line[..m.len()].eq_ignore_ascii_case(m)
                && line.get(m.len()) == Some(&b':');
            ptr = m.len() + 1;
        }
        let mut port_match = false;
        if host_match {
            let rest = line.get(ptr..).unwrap_or_default();
            if rest.first() == Some(&b':') {
                port_match = true;
                ptr += 1;
            } else if let Some(i) = rest.iter().position(|&c| c == b':') {
                port_match = dec_prefix(rest, 0xffff) == Some(t.port as u64);
                ptr += i + 1;
            }
        }
        if !(host_match && port_match) {
            return Ok((None, None));
        }
        // parse_connect_to_host_port()
        let hp = line.get(ptr..).unwrap_or_default();
        if hp.is_empty() {
            return Ok((None, None));
        }
        let (host, portpart): (&[u8], &[u8]) = if hp[0] == b'[' {
            let mut i = 1;
            while i < hp.len() && (hp[i].is_ascii_hexdigit() || hp[i] == b':' || hp[i] == b'.') {
                i += 1;
            }
            if i < hp.len() && hp[i] == b'%' {
                if !hp[i..].starts_with(b"%25") {
                    self.infof(cb, "Please URL encode % as %25, see RFC 6874.");
                }
                i += 1;
                while i < hp.len()
                    && (hp[i].is_ascii_alphanumeric() || matches!(hp[i], b'-' | b'.' | b'_' | b'~'))
                {
                    i += 1;
                }
            }
            if i < hp.len() && hp[i] == b']' {
                (&hp[1..i], &hp[i + 1..])
            } else {
                self.infof(cb, "Invalid IPv6 address format");
                (&hp[1..], &hp[i..])
            }
        } else {
            (hp, hp)
        };
        let same = std::ptr::eq(host.as_ptr(), portpart.as_ptr());
        let (host, port) = match portpart.iter().position(|&c| c == b':') {
            Some(i) => {
                let num = &portpart[i + 1..];
                let host = if same { &host[..i] } else { host };
                if num.is_empty() {
                    (host, None)
                } else {
                    match dec_prefix(num, 0xffff) {
                        Some(n) => (host, Some(n as u16)),
                        None => {
                            self.failf(
                                cb,
                                &format!(
                                    "No valid port number in connect to host string ({})",
                                    lossy(num)
                                ),
                            );
                            return Err(code::SETOPT_OPTION_SYNTAX);
                        }
                    }
                }
            }
            None => (host, None),
        };
        Ok((Some(lossy(host)), port))
    }

    /// create_conn_helper_init_proxy()
    fn setup_proxy(&mut self, cb: &mut dyn Callbacks, t: &mut Target) -> Result<(), CurlCode> {
        let mut proxy: Option<String> = self.set.strs.get(&opt::PROXY).map(|p| lossy(p));
        let noproxy_opt = self.set.strs.get(&opt::NOPROXY).map(|p| lossy(p));
        let mut env_np = None;
        if noproxy_opt.is_none()
            && let Some((name, v)) = proxy::env_noproxy()
        {
            self.infof(cb, &format!("Uses proxy env variable {name} == '{v}'"));
            env_np = Some(v);
        }
        let hostname_for_np = if t.ipv6 {
            format!("[{}]", t.host)
        } else {
            t.host.clone()
        };
        let np = noproxy_opt.as_deref().or(env_np.as_deref());
        if proxy::check_noproxy(&hostname_for_np, np) {
            proxy = None;
        } else if proxy.is_none()
            && let Some((env, p)) = proxy::detect_proxy(&t.scheme)
        {
            self.infof(cb, &format!("Uses proxy env variable {env} == '{p}'"));
            proxy = Some(p);
        }
        let Some(p) = proxy.filter(|p| !p.is_empty()) else {
            return Ok(());
        };
        // parse_proxy()
        let mut uhp = urlapi::Url::new();
        let flags = urlapi::CURLU_NON_SUPPORT_SCHEME | urlapi::CURLU_GUESS_SCHEME;
        if let Err(uc) = uhp.set(urlapi::Part::Url, Some(p.as_bytes()), flags) {
            self.failf(
                cb,
                &format!(
                    "Unsupported proxy syntax in '{p}': {}",
                    urlapi::url_strerror(uc)
                ),
            );
            return Err(code::COULDNT_RESOLVE_PROXY);
        }
        let pscheme =
            lossy(&uhp.get(urlapi::Part::Scheme, 0).unwrap_or_default()).to_ascii_lowercase();
        let mut ptype = self.set.proxytype;
        match pscheme.as_str() {
            "https" => ptype = proxytype::HTTPS,
            "socks5h" => ptype = proxytype::SOCKS5_HOSTNAME,
            "socks5" => ptype = proxytype::SOCKS5,
            "socks4a" => ptype = proxytype::SOCKS4A,
            "socks4" | "socks" => ptype = proxytype::SOCKS4,
            "http" => {}
            _ => {
                self.failf(cb, &format!("Unsupported proxy scheme for '{p}'"));
                return Err(code::COULDNT_CONNECT);
            }
        }
        if ptype == proxytype::HTTPS || ptype == proxytype::HTTPS2 {
            self.failf(
                cb,
                &format!(
                    "Unsupported proxy '{p}', libcurl is built without the HTTPS-proxy support."
                ),
            );
            return Err(code::NOT_BUILT_IN);
        }
        if ptype != proxytype::HTTP && ptype != proxytype::HTTP_1_0 {
            self.failf(cb, &format!("Unsupported proxy scheme for '{p}'"));
            return Err(code::COULDNT_CONNECT);
        }
        let mut pi = ProxyInfo {
            host: String::new(),
            port: 1080,
            proxytype: ptype,
            user: None,
            passwd: None,
            ipv6: false,
        };
        let u = uhp.get(urlapi::Part::User, urlapi::CURLU_URLDECODE).ok();
        let pw = uhp
            .get(urlapi::Part::Password, urlapi::CURLU_URLDECODE)
            .ok();
        if u.is_some() || pw.is_some() {
            self.st.proxyuser = u.clone();
            self.st.proxypasswd = Some(pw.clone().unwrap_or_default());
            pi.user = u;
            pi.passwd = Some(pw.unwrap_or_default());
            t.proxy_user_passwd = true;
        }
        if self.st.proxyuser.is_some() || self.st.proxypasswd.is_some() {
            t.proxy_user_passwd = true;
        }
        match uhp.get(urlapi::Part::Port, 0) {
            Ok(pp) => pi.port = lossy(&pp).parse().unwrap_or(1080),
            Err(_) => {
                pi.port = if self.set.proxyport != 0 {
                    self.set.proxyport as u16
                } else {
                    1080
                };
            }
        }
        let mut h = lossy(
            &uhp.get(urlapi::Part::Host, urlapi::CURLU_URLDECODE)
                .unwrap_or_default(),
        );
        if h.starts_with('[') {
            h = h.trim_start_matches('[').trim_end_matches(']').to_string();
            pi.ipv6 = true;
        }
        pi.host = h;
        if t.scheme == "https" {
            t.tunnel = true;
        }
        t.proxy = Some(pi);
        self.info.used_proxy = true;
        Ok(())
    }

    /// The TLS settings for the origin
    fn tls_config(&self, proxy: bool) -> TlsConfig {
        let s = &self.set;
        let ssl = if proxy { &s.proxy_ssl } else { &s.ssl };
        let st = |o: u32| s.strs.get(&o).cloned();
        let path = |o: u32| s.strs.get(&o).map(|v| std::path::PathBuf::from(lossy(v)));
        TlsConfig {
            verify_peer: ssl.verifypeer,
            verify_host: ssl.verifyhost,
            verify_status: ssl.verifystatus,
            cainfo: path(if proxy {
                opt::PROXY_CAINFO
            } else {
                opt::CAINFO
            }),
            capath: path(if proxy {
                opt::PROXY_CAPATH
            } else {
                opt::CAPATH
            }),
            ca_blob: s
                .blobs
                .get(&if proxy {
                    opt::PROXY_CAINFO_BLOB
                } else {
                    opt::CAINFO_BLOB
                })
                .cloned(),
            cert: st(if proxy {
                opt::PROXY_SSLCERT
            } else {
                opt::SSLCERT
            }),
            cert_type: st(if proxy {
                opt::PROXY_SSLCERTTYPE
            } else {
                opt::SSLCERTTYPE
            })
            .map(|v| lossy(&v)),
            cert_blob: s
                .blobs
                .get(&if proxy {
                    opt::PROXY_SSLCERT_BLOB
                } else {
                    opt::SSLCERT_BLOB
                })
                .cloned(),
            key: path(if proxy {
                opt::PROXY_SSLKEY
            } else {
                opt::SSLKEY
            }),
            key_type: st(if proxy {
                opt::PROXY_SSLKEYTYPE
            } else {
                opt::SSLKEYTYPE
            })
            .map(|v| lossy(&v)),
            key_blob: s
                .blobs
                .get(&if proxy {
                    opt::PROXY_SSLKEY_BLOB
                } else {
                    opt::SSLKEY_BLOB
                })
                .cloned(),
            key_passwd: st(if proxy {
                opt::PROXY_KEYPASSWD
            } else {
                opt::KEYPASSWD
            }),
            issuercert: path(if proxy {
                opt::PROXY_ISSUERCERT
            } else {
                opt::ISSUERCERT
            }),
            issuercert_blob: s
                .blobs
                .get(&if proxy {
                    opt::PROXY_ISSUERCERT_BLOB
                } else {
                    opt::ISSUERCERT_BLOB
                })
                .cloned(),
            crlfile: path(if proxy {
                opt::PROXY_CRLFILE
            } else {
                opt::CRLFILE
            }),
            version_min: ssl.version,
            version_max: ssl.version_max,
            cipher_list: st(if proxy {
                opt::PROXY_SSL_CIPHER_LIST
            } else {
                opt::SSL_CIPHER_LIST
            })
            .map(|v| lossy(&v)),
            tls13_ciphers: st(if proxy {
                opt::PROXY_TLS13_CIPHERS
            } else {
                opt::TLS13_CIPHERS
            })
            .map(|v| lossy(&v)),
            curves: st(opt::SSL_EC_CURVES).map(|v| lossy(&v)),
            signature_algorithms: st(opt::SSL_SIGNATURE_ALGORITHMS).map(|v| lossy(&v)),
            pinned_pubkey: st(if proxy {
                opt::PROXY_PINNEDPUBLICKEY
            } else {
                opt::PINNEDPUBLICKEY
            })
            .map(|v| lossy(&v)),
            ssl_options: ssl.ssl_options,
            certinfo: s.certinfo,
            trace: s.verbose && s.callbacks.contains(&opt::DEBUGFUNCTION),
            proxy,
            alpn: if s.ssl_enable_alpn {
                vec!["http/1.1".to_string()]
            } else {
                Vec::new()
            },
            sessionid_cache: ssl.cache_session,
        }
    }

    /// A fingerprint of the TLS options for connection reuse
    fn tls_key(&self) -> Vec<u8> {
        let s = &self.set;
        let mut k = format!(
            "{}{}{}{}{}{}",
            s.ssl.verifypeer as u8,
            s.ssl.verifyhost as u8,
            s.ssl.verifystatus as u8,
            s.ssl.version,
            s.ssl.version_max,
            s.ssl.ssl_options
        )
        .into_bytes();
        for o in [
            opt::CAINFO,
            opt::CAPATH,
            opt::SSLCERT,
            opt::SSLKEY,
            opt::SSL_CIPHER_LIST,
            opt::TLS13_CIPHERS,
            opt::PINNEDPUBLICKEY,
        ] {
            k.push(0);
            if let Some(v) = s.strs.get(&o) {
                k.extend_from_slice(v);
            }
        }
        for o in [opt::CAINFO_BLOB, opt::SSLCERT_BLOB, opt::SSLKEY_BLOB] {
            k.push(1);
            if let Some(v) = s.blobs.get(&o) {
                k.extend_from_slice(v);
            }
        }
        k
    }

    /// Curl_connect(): reuse a pooled connection or make a new one
    fn connect(&mut self, cb: &mut dyn Callbacks, t: &Target) -> Result<Conn, CurlCode> {
        // Curl_req_hard_reset()
        self.setcookies = 0;
        let pool = self.conn_pool();
        let tls_key = if t.scheme == "https" {
            self.tls_key()
        } else {
            Vec::new()
        };
        let proxy_key = t
            .proxy
            .as_ref()
            .map(|p| (p.host.to_ascii_lowercase(), p.port, t.tunnel));
        // ConnectionExists()
        if !self.set.reuse_fresh {
            let mut found = None;
            {
                let mut p = pool.0.borrow_mut();
                let mut i = 0;
                while i < p.idle.len() {
                    let c = &p.idle[i];
                    let same = c.scheme == t.scheme
                        && c.host.eq_ignore_ascii_case(&t.host)
                        && c.port == t.port
                        && c.conn_to == (t.conn_to_host.clone(), t.conn_to_port)
                        && c.proxy
                            .as_ref()
                            .map(|(h, po, tu)| (h.to_ascii_lowercase(), *po, *tu))
                            == proxy_key
                        && c.tls_key == tls_key
                        && (t.proxy.is_none() || t.tunnel || c.proxy_user == self.st.proxyuser);
                    if same {
                        found = Some(p.idle.remove(i));
                        break;
                    }
                    i += 1;
                }
            }
            if let Some(mut c) = found {
                let maxage = self.set.maxage_conn;
                let too_old = maxage > 0 && c.idle_since.elapsed().as_secs() as i64 > maxage;
                if too_old {
                    let ms = c.idle_since.elapsed().as_millis();
                    self.infof(
                        cb,
                        &format!(
                            "Too old connection ({ms} ms idle, max idle is {} ms), disconnect it",
                            maxage * 1000
                        ),
                    );
                    self.infof(cb, &format!("shutting down connection #{}", c.id));
                    c.shutdown();
                } else if c.seems_dead() {
                    self.infof(cb, &format!("Connection {} seems to be dead", c.id));
                    self.infof(cb, &format!("shutting down connection #{}", c.id));
                    c.shutdown();
                } else {
                    let what = if t.proxy.is_some() { "proxy" } else { "host" };
                    let disp = match &t.proxy {
                        Some(p) => p.host.clone(),
                        None => t.host.clone(),
                    };
                    self.infof(
                        cb,
                        &format!(
                            "Re-using existing {}: connection with {what} {disp}",
                            t.scheme
                        ),
                    );
                    self.st.recent_conn_id = c.id;
                    self.info.primary_ip = c.primary.ip().to_string();
                    self.info.primary_port = c.primary.port() as i64;
                    self.info.local_ip = c.local.ip().to_string();
                    self.info.local_port = c.local.port() as i64;
                    self.info.conn_remote_port = t.port as i64;
                    self.info.ssl_verifyresult = c.ssl_verifyresult;
                    self.info.certinfo = c.certinfo.clone();
                    self.progress.time(Timer::NameLookup);
                    self.progress.time(Timer::Connect);
                    if c.tls.is_some() {
                        self.progress.time(Timer::AppConnect);
                    }
                    c.reused = true;
                    return Ok(c);
                }
            }
        }
        // a new connection: it gets its id when added to the pool
        let id = {
            let mut p = pool.0.borrow_mut();
            let id = p.next_conn_id;
            p.next_conn_id += 1;
            id
        };
        self.info.conn_remote_port = t.port as i64;
        let (chost, cport, peertype) = match &t.proxy {
            Some(p) => (p.host.clone(), p.port, "proxy"),
            None => (
                t.conn_to_host.clone().unwrap_or_else(|| t.host.clone()),
                t.conn_to_port.unwrap_or(t.port),
                "host",
            ),
        };
        // resolve
        let mut addrs = self.resolve_host(cb, &chost, cport, peertype, id)?;
        match self.set.ipresolve {
            1 => addrs.retain(|a| a.is_ipv4()),
            2 => addrs.retain(|a| a.is_ipv6()),
            _ => {}
        }
        self.st.recent_conn_id = id;
        if self.pgrs_update(cb) {
            self.infof(cb, &format!("closing connection #{id}"));
            return Err(code::ABORTED_BY_CALLBACK);
        }
        // connect
        let (sock, primary) =
            self.connect_addrs(cb, id, &chost, cport, &addrs, t.proxy.is_some())?;
        let local = sock.local_addr().unwrap_or(primary);
        if self.set.tcp_nodelay {
            let _ = sock.set_nodelay(true);
        }
        self.info.numconnects += 1;
        let mut conn = Conn {
            id,
            sock,
            tls: None,
            rbuf: Vec::new(),
            rpos: 0,
            eof: false,
            scheme: t.scheme.clone(),
            host: t.host.clone(),
            port: t.port,
            proxy: t.proxy.as_ref().map(|p| (p.host.clone(), p.port, t.tunnel)),
            proxy_user: self.st.proxyuser.clone(),
            primary,
            local,
            close: false,
            httpversion_seen: 0,
            created: Instant::now(),
            idle_since: Instant::now(),
            ssl_verifyresult: 0,
            certinfo: Vec::new(),
            tls_key,
            ntlm: Default::default(),
            proxy_ntlm: Default::default(),
            reused: false,
            tls_err: None,
            conn_to: (t.conn_to_host.clone(), t.conn_to_port),
        };
        let disp = if t.proxy.is_some() {
            chost.clone()
        } else {
            t.conn_to_host.clone().unwrap_or_else(|| t.host.clone())
        };
        // proxy tunnel
        if t.proxy.is_some() && t.tunnel {
            if let Err(e) = self.proxy_connect(cb, &mut conn, t) {
                self.close_conn(cb, conn, true);
                return Err(e);
            }
        }
        if t.scheme == "https" {
            if let Err(e) = self.tls_handshake(cb, &mut conn, t) {
                self.close_conn(cb, conn, true);
                return Err(e);
            }
            self.progress.time(Timer::AppConnect);
        }
        self.info.primary_ip = conn.primary.ip().to_string();
        self.info.primary_port = conn.primary.port() as i64;
        self.info.local_ip = conn.local.ip().to_string();
        self.info.local_port = conn.local.port() as i64;
        self.infof(
            cb,
            &format!(
                "Connected to {disp} ({}) port {}",
                primary.ip(),
                primary.port()
            ),
        );
        self.infof(cb, "using HTTP/1.x");
        // PROTOCONNECT and DO rounds of the multi state machine
        for _ in 0..2 {
            if self.pgrs_update(cb) {
                self.close_conn(cb, conn, true);
                return Err(code::ABORTED_BY_CALLBACK);
            }
        }
        Ok(conn)
    }

    /// Curl_resolv() with the DNS cache and the verbose texts
    fn resolve_host(
        &mut self,
        cb: &mut dyn Callbacks,
        host: &str,
        port: u16,
        peertype: &str,
        id: i64,
    ) -> Result<Vec<IpAddr>, CurlCode> {
        let dns = self.dns_cache();
        let cached = dns.borrow_mut().get(host, port, self.set.dns_cache_timeout);
        let r = match cached {
            Some(e) => {
                self.infof(cb, &format!("Hostname {host} was found in DNS cache"));
                Ok(e.addrs)
            }
            None => match net::resolve(host, port, 0) {
                Some(addrs) => {
                    dns.borrow_mut().put(host, port, addrs.clone(), false);
                    if !is_ip_literal(host) {
                        if self.set.verbose {
                            self.infof(cb, &format!("Host {host}:{port} was resolved."));
                            let v6: Vec<String> = addrs
                                .iter()
                                .filter(|a| a.is_ipv6())
                                .map(|a| a.to_string())
                                .collect();
                            let v4: Vec<String> = addrs
                                .iter()
                                .filter(|a| a.is_ipv4())
                                .map(|a| a.to_string())
                                .collect();
                            self.infof(
                                cb,
                                &format!(
                                    "IPv6: {}",
                                    if v6.is_empty() {
                                        "(none)".to_string()
                                    } else {
                                        v6.join(", ")
                                    }
                                ),
                            );
                            self.infof(
                                cb,
                                &format!(
                                    "IPv4: {}",
                                    if v4.is_empty() {
                                        "(none)".to_string()
                                    } else {
                                        v4.join(", ")
                                    }
                                ),
                            );
                        }
                    }
                    Ok(addrs)
                }
                None => {
                    self.failf(cb, &format!("Could not resolve {peertype}: {host}"));
                    self.infof(cb, &format!("shutting down connection #{id}"));
                    return Err(if peertype == "proxy" {
                        code::COULDNT_RESOLVE_PROXY
                    } else {
                        code::COULDNT_RESOLVE_HOST
                    });
                }
            },
        };

        self.progress.time(Timer::NameLookup);
        r
    }

    /// Happy-eyeballs-ish connect over the resolved addresses
    fn connect_addrs(
        &mut self,
        cb: &mut dyn Callbacks,
        id: i64,
        host: &str,
        port: u16,
        addrs: &[IpAddr],
        _is_proxy: bool,
    ) -> Result<(std::net::TcpStream, SocketAddr), CurlCode> {
        // first family first, then the other one
        let first_v6 = addrs.first().is_some_and(|a| a.is_ipv6());
        let mut order: Vec<IpAddr> = addrs
            .iter()
            .filter(|a| a.is_ipv6() == first_v6)
            .cloned()
            .collect();
        let other: Vec<IpAddr> = addrs
            .iter()
            .filter(|a| a.is_ipv6() != first_v6)
            .cloned()
            .collect();
        let n_first = order.len();
        order.extend(other);
        let mut last_errno = 0;
        for (i, ip) in order.iter().enumerate() {
            let left = self.timeleft(true);
            if left < 0 {
                break;
            }
            let addr = SocketAddr::new(*ip, port);
            if ip.is_ipv6() {
                self.infof(cb, &format!("  Trying [{ip}]:{port}..."));
            } else {
                self.infof(cb, &format!("  Trying {ip}:{port}..."));
            }
            // while there is another family to try, do not wait for long
            let mut to = Duration::from_millis(left.max(1) as u64);
            if i < n_first && n_first < order.len() {
                to = to.min(Duration::from_millis(
                    self.set
                        .longs
                        .get(&opt::HAPPY_EYEBALLS_TIMEOUT_MS)
                        .copied()
                        .unwrap_or(200)
                        .max(1) as u64,
                ));
            }
            match net::tcp_connect(&addr, Some(to)) {
                Ok(s) => {
                    self.progress.time(Timer::Connect);
                    if self.pgrs_update(cb) {
                        self.infof(cb, &format!("closing connection #{id}"));
                        return Err(code::ABORTED_BY_CALLBACK);
                    }
                    return Ok((s, addr));
                }
                Err(ConnectErr::Failed(errno, local)) => {
                    last_errno = errno;
                    self.info.os_errno = errno as i64;
                    let (lip, lport) = match local {
                        Some(l) => (l.ip().to_string(), l.port() as i64),
                        None => (String::new(), -1),
                    };
                    self.infof(
                        cb,
                        &format!(
                            "connect to {ip} port {port} from {lip} port {lport} failed: {}",
                            net::os_strerror(errno)
                        ),
                    );
                }
                Err(ConnectErr::Timeout) => {
                    if self.timeleft(true) < 0 {
                        break;
                    }
                }
            }
        }
        let elapsed = self.progress.t_startsingle.elapsed().as_millis();
        if self.timeleft(true) < 0 {
            let ms = self.progress.t_startsingle.elapsed().as_millis();
            self.failf(cb, &format!("Connection timed out after {ms} milliseconds"));
            self.infof(cb, &format!("closing connection #{id}"));
            return Err(code::OPERATION_TIMEDOUT);
        }
        let _ = last_errno;
        self.failf(
            cb,
            &format!(
                "Failed to connect to {host} port {port} after {elapsed} ms: {}",
                super::easy_strerror(code::COULDNT_CONNECT)
            ),
        );
        self.infof(cb, &format!("closing connection #{id}"));
        Err(code::COULDNT_CONNECT)
    }

    /// Curl_timeleft() in ms (0 = no limit is reported as i64::MAX)
    pub(crate) fn timeleft(&self, duringconnect: bool) -> i64 {
        let timeout = self.set.timeout_ms;
        if timeout <= 0 && !duringconnect {
            return i64::MAX;
        }
        let mut timeleft = 0i64;
        if timeout > 0 {
            timeleft = timeout - self.progress.t_startop.elapsed().as_millis() as i64;
            if timeleft == 0 {
                timeleft = -1;
            }
            if !duringconnect {
                return timeleft;
            }
        }
        let ct = if self.set.connecttimeout_ms > 0 {
            self.set.connecttimeout_ms
        } else {
            DEFAULT_CONNECT_TIMEOUT
        };
        let mut cleft = ct - self.progress.t_startsingle.elapsed().as_millis() as i64;
        if cleft == 0 {
            cleft = -1;
        }
        if timeleft == 0 {
            return cleft;
        }
        cleft.min(timeleft)
    }

    /// Hand the TLS layer's verbose events to the trace
    pub(crate) fn drain_tls_log(&self, cb: &mut dyn Callbacks, sess: &mut TlsSession) {
        let tr = self.tracer();
        for e in sess.take_log() {
            match e {
                TlsLog::Info(l) | TlsLog::Trace(l) => tr.infof(cb, &l),
                TlsLog::DataIn(d) => tr.debug(cb, info::SSL_DATA_IN, &d),
                TlsLog::DataOut(d) => tr.debug(cb, info::SSL_DATA_OUT, &d),
            }
        }
    }

    /// TLS handshake on a fresh connection
    fn tls_handshake(
        &mut self,
        cb: &mut dyn Callbacks,
        conn: &mut Conn,
        t: &Target,
    ) -> Result<(), CurlCode> {
        use std::io::{Read, Write};
        let cfg = self.tls_config(false);
        self.info.ssl_verifyresult = 1;
        let pool = self.conn_pool();
        let key = (t.host.to_ascii_lowercase(), t.port);
        let resume = if self.set.ssl.cache_session {
            pool.0.borrow().tls_sessions.get(&key).cloned()
        } else {
            None
        };
        let mut sess = match TlsSession::new_with_resumption(&cfg, &t.host, t.port, resume.as_ref())
        {
            Ok(s) => s,
            Err((c, m)) => {
                if !m.is_empty() {
                    self.failf(cb, &m);
                }
                return Err(c);
            }
        };
        loop {
            let r = sess.handshake();
            let out = sess.take_outgoing();
            if !out.is_empty() && conn.sock.write_all(&out).is_err() {
                self.drain_tls_log(cb, &mut sess);
                self.failf(cb, "Send failure: Broken pipe");
                return Err(code::SEND_ERROR);
            }
            self.drain_tls_log(cb, &mut sess);
            match r {
                Ok(true) => break,
                Ok(false) => {}
                Err((c, m)) => {
                    self.info.ssl_verifyresult = sess.verify_result();
                    if !m.is_empty() {
                        self.failf(cb, &m);
                        if m.starts_with("error setting certificate") {
                            // Curl_ssl_setup_x509_store() runs twice
                            self.failf(cb, &m);
                        }
                    }
                    return Err(c);
                }
            }
            let left = self.timeleft(true);
            if left < 0
                || !net::wait_readable(conn.fd(), Some(Duration::from_millis(left.max(1) as u64)))
            {
                let ms = self.progress.t_startsingle.elapsed().as_millis();
                self.failf(cb, "SSL connection timeout");
                let _ = ms;
                return Err(code::OPERATION_TIMEDOUT);
            }
            let mut buf = [0u8; 16384];
            match conn.sock.read(&mut buf) {
                Ok(0) => sess.feed_eof(),
                Ok(n) => {
                    if let Err((c, m)) = sess.feed(&buf[..n]) {
                        self.drain_tls_log(cb, &mut sess);
                        self.failf(cb, &m);
                        return Err(c);
                    }
                }
                Err(e) => {
                    let errno = e.raw_os_error().unwrap_or(libc::ECONNRESET);
                    self.info.os_errno = errno as i64;
                    self.failf(cb, &format!("Recv failure: {}", net::os_strerror(errno)));
                    return Err(code::RECV_ERROR);
                }
            }
        }
        conn.ssl_verifyresult = sess.verify_result();
        self.info.ssl_verifyresult = conn.ssl_verifyresult;
        if self.set.certinfo {
            conn.certinfo = sess.certinfo();
            self.info.certinfo = conn.certinfo.clone();
        }
        if self.set.ssl.cache_session
            && let Some(r) = sess.take_resumption()
        {
            pool.0.borrow_mut().tls_sessions.insert(key, r);
        }
        conn.tls = Some(Box::new(sess));
        Ok(())
    }

    /// HTTP CONNECT through a proxy (lib/http_proxy.c, lib/cf-h1-proxy.c)
    fn proxy_connect(
        &mut self,
        cb: &mut dyn Callbacks,
        conn: &mut Conn,
        t: &Target,
    ) -> Result<(), CurlCode> {
        let authority = if t.ipv6 {
            format!("[{}]:{}", t.host, t.port)
        } else {
            format!("{}:{}", t.host, t.port)
        };
        let ptype = t.proxy.as_ref().map(|p| p.proxytype).unwrap_or(0);
        let minor = if ptype == proxytype::HTTP_1_0 { 0 } else { 1 };
        self.infof(cb, &format!("CONNECT tunnel: HTTP/1.{minor} negotiated"));
        self.infof(cb, "allocate connect buffer");
        let mut req = Req::new(false);
        loop {
            if self.timeleft(true) <= 0 {
                self.failf(cb, "Proxy CONNECT aborted due to timeout");
                return Err(code::OPERATION_TIMEDOUT);
            }
            // start_CONNECT()
            req.newurl = None;
            let auth = self.output_auth(
                cb,
                conn,
                t,
                &mut req,
                b"CONNECT",
                authority.as_bytes(),
                true,
            )?;
            self.infof(cb, &format!("Establish HTTP proxy tunnel to {authority}"));
            let ph = self.connect_headers().cloned();
            let mut r = format!("CONNECT {authority} HTTP/1.{minor}\r\n").into_bytes();
            if checkheaders(ph.as_ref(), b"Host").is_none() {
                r.extend_from_slice(format!("Host: {authority}\r\n").as_bytes());
            }
            r.extend_from_slice(&auth.proxy);
            if checkheaders(ph.as_ref(), b"User-Agent").is_none()
                && let Some(ua) = self.set.strs.get(&opt::USERAGENT)
                && !ua.is_empty()
            {
                r.extend_from_slice(b"User-Agent: ");
                r.extend_from_slice(ua);
                r.extend_from_slice(b"\r\n");
            }
            if checkheaders(ph.as_ref(), b"Proxy-Connection").is_none() {
                r.extend_from_slice(b"Proxy-Connection: Keep-Alive\r\n");
            }
            self.add_connect_custom_headers(&mut r, ph.as_ref());
            r.extend_from_slice(b"\r\n");
            // send_CONNECT()
            if conn.send_all(&r).is_err() {
                self.failf(cb, "Failed sending CONNECT to proxy");
                return Err(code::SEND_ERROR);
            }
            self.tracer().debug(cb, info::HEADER_OUT, &r);
            // recv_CONNECT_resp()
            req.httpcode = 0;
            let mut line: Vec<u8> = Vec::new();
            let mut headerlines = 0;
            let mut cl: i64 = 0;
            let mut chunked = false;
            let mut close_connection = false;
            let mut error = false;
            let mut ignoring = false;
            let mut ch = Chunker::new();
            loop {
                if conn.buffered() == 0 {
                    let left = self.timeleft(true);
                    match conn.fill(Some(Duration::from_millis(left.max(1) as u64))) {
                        Ok(0) | Err(_) => {
                            if self.st.authproxy.avail != 0 && !auth.proxy.is_empty() {
                                close_connection = true;
                                self.infof(cb, "Proxy CONNECT connection closed");
                            } else {
                                error = true;
                                self.failf(cb, "Proxy CONNECT aborted");
                            }
                            break;
                        }
                        Ok(_) => {}
                    }
                }
                let byte = conn.take(1)[0];
                if ignoring {
                    if cl > 0 {
                        cl -= 1;
                        if cl <= 0 {
                            break;
                        }
                    } else if chunked {
                        let _ = ch.process(&[byte], false, &mut |_| Ok(()));
                        if ch.is_done() {
                            self.infof(cb, "chunk reading DONE");
                            break;
                        }
                    }
                    continue;
                }
                line.push(byte);
                if byte != b'\n' {
                    continue;
                }
                headerlines += 1;
                let l = std::mem::take(&mut line);
                self.tracer().debug(cb, info::HEADER_IN, &l);
                let wt = CW_HEADER | CW_CONNECT | if headerlines == 1 { CW_STATUS } else { 0 };
                self.client_write(cb, &mut req, wt, &l)?;
                // Curl_bump_headersize(connect_only)
                self.info.header_size += l.len() as i64;
                if l[0] == b'\r' || l[0] == b'\n' {
                    if req.httpcode == 407 && !self.st.authproblem {
                        if cl > 0 {
                            self.infof(cb, &format!("Ignore {cl} bytes of response-body"));
                            ignoring = true;
                            continue;
                        } else if chunked {
                            self.infof(cb, "Ignore chunked response-body");
                            ignoring = true;
                            continue;
                        }
                    }
                    break;
                }
                // on_resp_header()
                let hl = |n: &[u8]| l.len() >= n.len() && l[..n.len()].eq_ignore_ascii_case(n);
                if (hl(b"WWW-Authenticate:") && req.httpcode == 401)
                    || (hl(b"Proxy-authenticate:") && req.httpcode == 407)
                {
                    let v = copy_header_value(&l);
                    self.input_auth(cb, req.httpcode == 407, &v);
                } else if hl(b"Content-Length:") {
                    if req.httpcode / 100 == 2 {
                        self.infof(
                            cb,
                            &format!(
                                "Ignoring Content-Length in CONNECT {:03} response",
                                req.httpcode
                            ),
                        );
                    } else {
                        let v = lossy(trim_crlf(&l[15..]));
                        match v.trim_start().parse::<i64>() {
                            Ok(n) if n >= 0 => cl = n,
                            _ => {
                                self.failf(cb, "Unsupported Content-Length value");
                                return Err(code::WEIRD_SERVER_REPLY);
                            }
                        }
                    }
                } else if compareheader(&l, b"Connection", b"close") {
                    close_connection = true;
                } else if hl(b"Transfer-Encoding:") {
                    if req.httpcode / 100 == 2 {
                        self.infof(
                            cb,
                            &format!(
                                "Ignoring Transfer-Encoding in CONNECT {:03} response",
                                req.httpcode
                            ),
                        );
                    } else if compareheader(&l, b"Transfer-Encoding", b"chunked") {
                        self.infof(cb, "CONNECT responded chunked");
                        chunked = true;
                        ch = Chunker::new();
                    }
                } else if compareheader(&l, b"Proxy-Connection", b"close") {
                    close_connection = true;
                } else if l.len() >= 13
                    && l.starts_with(b"HTTP/1.")
                    && (l[7] == b'0' || l[7] == b'1')
                    && l[8] == b' '
                    && l[9].is_ascii_digit()
                    && l[10].is_ascii_digit()
                    && l[11].is_ascii_digit()
                    && !l[12].is_ascii_digit()
                {
                    req.httpcode = ((l[9] - b'0') as i64) * 100
                        + ((l[10] - b'0') as i64) * 10
                        + (l[11] - b'0') as i64;
                    self.info.httpproxycode = req.httpcode;
                }
            }
            if error {
                return Err(code::RECV_ERROR);
            }
            if self.info.httpproxycode / 100 != 2 {
                self.auth_act(cb, conn, t, &mut req)?;
            }
            if self.pgrs_update(cb) {
                return Err(code::ABORTED_BY_CALLBACK);
            }
            if req.newurl.is_some() {
                if close_connection || conn.close {
                    self.infof(cb, "Connect me again please");
                    // reconnect to the proxy and try again
                    let p = t.proxy.as_ref().unwrap();
                    let addrs =
                        net::resolve(&p.host, p.port, self.set.ipresolve).unwrap_or_default();
                    let (sock, primary) =
                        self.connect_addrs(cb, conn.id, &p.host, p.port, &addrs, true)?;
                    conn.local = sock.local_addr().unwrap_or(primary);
                    conn.sock = sock;
                    conn.primary = primary;
                    conn.rbuf.clear();
                    conn.rpos = 0;
                    conn.eof = false;
                    conn.close = false;
                }
                continue;
            }
            break;
        }
        self.info.httpcode = 0;
        if self.info.httpproxycode / 100 != 2 {
            conn.close = true;
            self.failf(
                cb,
                &format!("CONNECT tunnel failed, response {}", req.httpcode),
            );
            return Err(code::RECV_ERROR);
        }
        self.infof(cb, "CONNECT phase completed");
        self.st.authproxy.done = true;
        self.st.authproxy.multipass = false;
        self.infof(
            cb,
            &format!(
                "CONNECT tunnel established, response {}",
                self.info.httpproxycode
            ),
        );
        Ok(())
    }

    /// dynhds_add_custom() for CONNECT requests
    fn add_connect_custom_headers(&mut self, r: &mut Vec<u8>, hl: Option<&Vec<Vec<u8>>>) {
        let Some(hl) = hl else { return };
        for h in hl {
            let h = super::easy::cstr(h);
            let (name, line) = if let Some(semi) = h.iter().position(|&c| c == b';')
                && semi + 1 == h.len()
                && !h[..semi].contains(&b':')
            {
                let mut l = h[..semi].to_vec();
                l.extend_from_slice(b": \r\n");
                (h[..semi].to_vec(), l)
            } else {
                let Some(colon) = h.iter().position(|&c| c == b':') else {
                    continue;
                };
                let v = &h[colon + 1..];
                if v.iter().all(|&c| c == b' ' || c == b'\t') {
                    continue;
                }
                let mut val = v;
                while let Some((&c, rest)) = val.split_first() {
                    if c == b' ' || c == b'\t' {
                        val = rest;
                    } else {
                        break;
                    }
                }
                let mut l = h[..colon].to_vec();
                l.extend_from_slice(b": ");
                l.extend_from_slice(val);
                l.extend_from_slice(b"\r\n");
                (h[..colon].to_vec(), l)
            };
            let is = |n: &[u8]| name.eq_ignore_ascii_case(n);
            if is(b"Host")
                || is(b"Content-Type")
                || is(b"Content-Length")
                || is(b"Connection")
                || is(b"Transfer-Encoding")
            {
                continue;
            }
            r.extend_from_slice(&line);
        }
    }

    /// The header list for a CONNECT request
    fn connect_headers(&self) -> Option<&Vec<Vec<u8>>> {
        if self.set.sep_headers {
            self.set.slists.get(&opt::PROXYHEADER)
        } else {
            self.set.slists.get(&opt::HTTPHEADER)
        }
    }

    /// Close (not pool) a connection
    /// Store a TLS session ticket the connection got for later resumption
    fn keep_tls_session(&mut self, conn: &mut Conn) {
        if !self.set.ssl.cache_session {
            return;
        }
        if let Some(t) = conn.tls.as_mut()
            && let Some(r) = t.take_resumption()
        {
            let key = (conn.host.to_ascii_lowercase(), conn.port);
            self.conn_pool().0.borrow_mut().tls_sessions.insert(key, r);
        }
    }

    pub(crate) fn close_conn(&mut self, cb: &mut dyn Callbacks, mut conn: Conn, aborted: bool) {
        self.keep_tls_session(&mut conn);
        self.infof(
            cb,
            &format!(
                "{} connection #{}",
                if aborted { "closing" } else { "shutting down" },
                conn.id
            ),
        );
        conn.shutdown();
    }

    /// multi_done_locked(): keep the connection in the pool or close it
    fn done_conn(&mut self, cb: &mut dyn Callbacks, mut conn: Conn, premature: bool, t: &Target) {
        self.st.recent_conn_id = conn.id;
        self.keep_tls_session(&mut conn);
        if self.set.reuse_forbid || conn.close || premature {
            self.st.lastconnect_id = -1;
            self.close_conn(cb, conn, premature);
            return;
        }
        let pool = self.conn_pool();
        // maxconnects: drop the oldest idle connection when full
        let max = if self.set.maxconnects > 0 {
            self.set.maxconnects as usize
        } else {
            5
        };
        let mut evicted = None;
        {
            let mut p = pool.0.borrow_mut();
            if p.idle.len() >= max {
                evicted = Some(p.idle.remove(0));
            }
        }
        if let Some(e) = evicted {
            self.infof(cb, "Connection pool is full, closing the oldest one");
            self.close_conn(cb, e, false);
        }
        let host = match &t.proxy {
            Some(p) => p.host.clone(),
            None => t.host.clone(),
        };
        self.st.lastconnect_id = conn.id;
        self.infof(
            cb,
            &format!("Connection #{} to host {host} left intact", conn.id),
        );
        conn.idle_since = Instant::now();
        pool.0.borrow_mut().idle.push(conn);
    }

    /// Curl_retry_request()
    fn retry_request(
        &mut self,
        cb: &mut dyn Callbacks,
        conn: &mut Conn,
        req: &Req,
    ) -> Result<bool, CurlCode> {
        if req.bytecount + req.headerbytecount == 0 && conn.reused {
            if self.st.retrycount >= 5 {
                self.failf(cb, "Connection died, tried 5 times before giving up");
                self.st.retrycount = 0;
                return Err(code::SEND_ERROR);
            }
            self.st.retrycount += 1;
            self.infof(
                cb,
                &format!(
                    "Connection died, retrying a fresh connect (retry count: {})",
                    self.st.retrycount
                ),
            );
            conn.close = true;
            self.st.rewind_read = true;
            return Ok(true);
        }
        Ok(false)
    }

    /// Curl_http_follow() and friends
    fn follow(
        &mut self,
        cb: &mut dyn Callbacks,
        newurl: &[u8],
        kind: FollowType,
        t: &Target,
        req: &Req,
    ) -> Result<(), CurlCode> {
        let mut kind = kind;
        let mut reachedmax = false;
        if kind != FollowType::Fake {
            self.st.requests += 1;
        }
        if kind == FollowType::Redir {
            if self.set.maxredirs != -1 && self.st.followlocation >= self.set.maxredirs {
                reachedmax = true;
                kind = FollowType::Fake;
            } else {
                self.st.followlocation += 1;
                if self.set.auto_referer
                    && let Some(cur) = &self.st.url
                {
                    self.st.referer = urlapi::auto_referer(cur, urlapi::SCHEMES);
                }
            }
        }
        if kind == FollowType::Retry {
            self.st.url = Some(newurl.to_vec());
            return Ok(());
        }
        let mut uh = t.tu.uh.clone();
        let fp = FollowParams {
            kind,
            path_as_is: self.set.path_as_is,
            httpcode: req.httpcode as u16,
            allow_auth_to_other_hosts: self.set.allow_auth_to_other_hosts,
            custom_port: if self.set.use_port != 0 && self.st.allow_port {
                Some(self.set.use_port as u16)
            } else {
                None
            },
            conn_remote_port: self.info.conn_remote_port as u16,
            conn_protocol: self.info.conn_protocol as u64,
        };
        let r = match urlapi::http_follow(&mut uh, newurl, &fp) {
            Ok(r) => r,
            Err(e) => {
                if !e.message.is_empty() {
                    self.failf(cb, &e.message);
                }
                return Err(e.code);
            }
        };
        if kind == FollowType::Fake {
            self.info.wouldredirect = Some(r.url);
            if reachedmax {
                self.failf(
                    cb,
                    &format!("Maximum ({}) redirects followed", self.set.maxredirs),
                );
                return Err(code::TOO_MANY_REDIRECTS);
            }
            return Ok(());
        }
        if r.clear_auth {
            let newport = match fp.custom_port {
                Some(p) => p as i64,
                None => uh
                    .get(urlapi::Part::Port, urlapi::CURLU_DEFAULT_PORT)
                    .ok()
                    .and_then(|p| lossy(&p).parse::<i64>().ok())
                    .unwrap_or(0),
            };
            if newport != self.info.conn_remote_port {
                self.infof(
                    cb,
                    &format!(
                        "Clear auth, redirects to port from {} to {newport}",
                        self.info.conn_remote_port
                    ),
                );
            } else {
                let ns = lossy(&uh.get(urlapi::Part::Scheme, 0).unwrap_or_default());
                let os = self.info.conn_scheme.clone().unwrap_or_default();
                self.infof(
                    cb,
                    &format!("Clear auth, redirects scheme from {os} to {ns}"),
                );
            }
            self.st.user = None;
            self.st.passwd = None;
        }
        if r.disallow_port {
            self.st.allow_port = false;
        }
        self.st.url = Some(r.url.clone());
        let rewind_result = self.client_rewind(cb, req);
        self.infof(
            cb,
            &format!("Issue another request to this URL: '{}'", lossy(&r.url)),
        );
        let custom = self.set.strs.contains_key(&opt::CUSTOMREQUEST);
        if self.set.follow_mode == 3 && custom && !self.st.http_ignorecustom {
            self.st.http_ignorecustom = true;
            self.infof(cb, "Drop custom request method for next request");
        }
        let code = self.info.httpcode;
        let is_post = matches!(
            self.st.httpreq,
            HttpReq::Post | HttpReq::PostForm | HttpReq::PostMime
        );
        let mut switch_to_get = false;
        match code {
            301 if is_post && self.set.keep_post & 1 == 0 => {
                self.switch_to_get(cb, 301);
                switch_to_get = true;
            }
            302 if is_post && self.set.keep_post & 2 == 0 => {
                self.switch_to_get(cb, 302);
                switch_to_get = true;
            }
            303 if self.st.httpreq != HttpReq::Get && (!is_post || self.set.keep_post & 4 == 0) => {
                self.switch_to_get(cb, 303);
                switch_to_get = true;
            }
            _ => {}
        }
        if let Err(e) = rewind_result
            && !switch_to_get
        {
            return Err(e);
        }
        self.st.this_is_a_follow = true;
        self.progress.time(Timer::Redirect);
        self.progress.set_download_size(-1);
        self.progress.set_upload_size(-1);
        Ok(())
    }

    /// Curl_client_start(): rewind the upload readers when needed
    fn client_rewind(&mut self, cb: &mut dyn Callbacks, req: &Req) -> Result<(), CurlCode> {
        if !self.st.rewind_read {
            return Ok(());
        }
        let (name, r) = match &req.body {
            BodySrc::Fread { read, eos, .. } => {
                let used = *read > 0 || *eos;
                if !used {
                    ("cr-in", Ok(()))
                } else if self.set.callbacks.contains(&opt::SEEKFUNCTION) {
                    let err = cb.seek(0, 0);
                    if err != 0 {
                        self.failf(cb, &format!("seek callback returned error {err}"));
                        ("cr-in", Err(code::SEND_FAIL_REWIND))
                    } else {
                        ("cr-in", Ok(()))
                    }
                } else {
                    self.failf(cb, "necessary data rewind was not possible");
                    ("cr-in", Err(code::SEND_FAIL_REWIND))
                }
            }
            BodySrc::Mime(_, m) => {
                let seekable = m
                    .borrow()
                    .parts
                    .iter()
                    .all(|p| !matches!(p.data, super::MimeData::Callback { .. }));
                if seekable {
                    ("cr-mime", Ok(()))
                } else {
                    self.failf(cb, "Cannot rewind mime/post data");
                    ("cr-mime", Err(code::SEND_FAIL_REWIND))
                }
            }
            _ => ("cr-buf", Ok(())),
        };
        if let Err(e) = r {
            self.failf(cb, &format!("rewind of client reader '{name}' failed: {e}"));
            return Err(e);
        }
        self.st.rewind_read = false;
        Ok(())
    }

    /// http_switch_to_get()
    fn switch_to_get(&mut self, cb: &mut dyn Callbacks, code: i64) {
        let req = self.set.strs.get(&opt::CUSTOMREQUEST).cloned();
        if (req.is_some() || self.st.httpreq != HttpReq::Get) && self.set.follow_mode == 2 {
            self.infof(cb, &format!("Switch to GET because of {code} response"));
            self.st.http_ignorecustom = true;
        } else if let Some(r) = &req
            && self.set.follow_mode != 3
        {
            self.infof(cb, &format!("Stick to {} instead of GET", lossy(r)));
        }
        self.st.httpreq = HttpReq::Get;
        self.st.rewind_read = false;
    }

    /// Curl_pgrsUpdate(): returns true when the progress callback aborted
    pub(crate) fn pgrs_update(&mut self, cb: &mut dyn Callbacks) -> bool {
        self.progress.calc();
        if self.set.hide_progress {
            return false;
        }
        let has = self.set.callbacks.contains(&opt::XFERINFOFUNCTION)
            || self.set.callbacks.contains(&opt::PROGRESSFUNCTION);
        if !has {
            return false;
        }
        let p = &self.progress;
        let r = cb.progress(p.dl_total, p.dl_cur, p.ul_total, p.ul_cur);
        if r != super::PROGRESSFUNC_CONTINUE && r != 0 {
            self.failf(cb, "Callback aborted");
            return true;
        }
        false
    }

    // ------------------------------------------------------------------
    // client writers

    /// cw_out for header data
    fn cw_out_header(
        &mut self,
        cb: &mut dyn Callbacks,
        data: &[u8],
        _connect: bool,
    ) -> Result<(), CurlCode> {
        if self.set.include_header {
            self.cw_out_write(cb, data, false)?;
        }
        self.cw_out_write(cb, data, true)
    }

    /// cw_out_ptr_flush(): call the write/header callback
    fn cw_out_write(
        &mut self,
        cb: &mut dyn Callbacks,
        data: &[u8],
        hds: bool,
    ) -> Result<(), CurlCode> {
        let mut data = data;
        while !data.is_empty() || (!hds && data.is_empty() && false) {
            let wlen = if hds {
                data.len()
            } else {
                data.len().min(CURL_MAX_WRITE_SIZE)
            };
            let chunk = &data[..wlen];
            let n = if hds {
                cb.header(chunk)
            } else {
                cb.write(chunk)
            };
            if n == WRITEFUNC_PAUSE {
                // a blocking transfer cannot pause: call again shortly
                std::thread::sleep(Duration::from_millis(1));
                continue;
            }
            if n == WRITEFUNC_ERROR {
                self.failf(
                    cb,
                    &format!("client returned ERROR on write of {wlen} bytes"),
                );
                return Err(code::WRITE_ERROR);
            }
            if n != wlen {
                self.failf(
                    cb,
                    &format!(
                        "Failure writing output to destination, passed {wlen} returned {}",
                        n as isize
                    ),
                );
                return Err(code::WRITE_ERROR);
            }
            data = &data[wlen..];
        }
        Ok(())
    }

    /// Curl_client_write() through the writer stack
    fn client_write(
        &mut self,
        cb: &mut dyn Callbacks,
        req: &mut Req,
        flags: u32,
        buf: &[u8],
    ) -> Result<(), CurlCode> {
        // cw_raw
        if flags & CW_BODY != 0 && self.set.verbose && !req.ignorebody {
            self.tracer().debug(cb, info::DATA_IN, buf);
        }
        // transfer decoding (chunked)
        if flags & CW_BODY != 0
            && let Some(mut ch) = req.chunker.take()
        {
            let te_skip = self.set.http_te_skip;
            let mut pieces: Vec<(u32, Vec<u8>)> = Vec::new();
            let r = ch.process(buf, te_skip, &mut |o| {
                match o {
                    ChunkOut::Body(b) => pieces.push((CW_BODY, b.to_vec())),
                    ChunkOut::Trailer(t) => pieces.push((CW_HEADER | CW_TRAILER, t)),
                }
                Ok(())
            });
            let mut res = Ok(());
            for (f, p) in pieces {
                if let Err(e) = self.cw_download(cb, req, f, &p) {
                    self.failf(cb, "Failed reading the chunked-encoded stream");
                    res = Err(e);
                    break;
                }
            }
            let done = ch.is_done();
            let last_err = ch.last_err;
            let passthru = ch.passthru_err;
            req.chunker = Some(ch);
            res?;
            match r {
                Err((c, msg)) => {
                    if let Some(m) = msg {
                        self.failf(cb, &m);
                    }
                    if passthru {
                        self.failf(cb, "Failed reading the chunked-encoded stream");
                    } else {
                        self.failf(cb, &format!("{last_err} in chunked-encoding"));
                    }
                    return Err(c);
                }
                Ok(consumed) => {
                    if done {
                        req.download_done = true;
                        let left = buf.len() - consumed;
                        if left > 0 {
                            self.infof(cb, &format!("Leftovers after chunking: {left} bytes"));
                        }
                    } else if flags & CW_EOS != 0 && !req.no_body {
                        self.failf(cb, "transfer closed with outstanding read data remaining");
                        return Err(code::PARTIAL_FILE);
                    }
                }
            }
            return Ok(());
        }
        self.cw_download(cb, req, flags, buf)
    }

    /// cw_download_write()
    fn cw_download(
        &mut self,
        cb: &mut dyn Callbacks,
        req: &mut Req,
        flags: u32,
        buf: &[u8],
    ) -> Result<(), CurlCode> {
        let is_connect = flags & CW_CONNECT != 0;
        if !is_connect && !req.started_response {
            self.progress.time(Timer::StartTransfer);
            req.started_response = true;
        }
        if flags & CW_BODY == 0 {
            if is_connect && self.set.suppress_connect_headers {
                return Ok(());
            }
            return self.cw_content(cb, req, flags, buf);
        }
        if req.no_body && !buf.is_empty() {
            // streamclose(conn, "ignoring body")
            req.close_conn = true;
            req.download_done = true;
            if self.info.header_size > 0 {
                return Ok(());
            }
            return Err(code::WEIRD_SERVER_REPLY);
        }
        let mut nwrite = buf.len();
        let mut excess = 0usize;
        if req.maxdownload != -1 {
            let wmax = (req.maxdownload - req.bytecount).max(0) as usize;
            if nwrite > wmax {
                excess = nwrite - wmax;
                nwrite = wmax;
            }
            if nwrite == wmax {
                req.download_done = true;
            }
            if flags & CW_EOS != 0 && !req.no_body && req.maxdownload > req.bytecount {
                self.failf(
                    cb,
                    &format!(
                        "end of response with {} bytes missing",
                        req.maxdownload - req.bytecount
                    ),
                );
                return Err(code::PARTIAL_FILE);
            }
        }
        if self.set.max_filesize > 0 && !req.ignorebody {
            let wmax = (self.set.max_filesize - req.bytecount).max(0) as usize;
            nwrite = nwrite.min(wmax);
        }
        if !req.ignorebody && (nwrite > 0 || flags & CW_EOS != 0) {
            self.cw_content(cb, req, flags, &buf[..nwrite])?;
        }
        req.bytecount += nwrite as i64;
        self.progress.dl_cur = req.bytecount;
        if excess > 0 {
            if !req.ignorebody {
                self.infof(
                    cb,
                    &format!(
                        "Excess found writing body: excess = {excess}, size = {}, maxdownload = {}, bytecount = {}",
                        req.size, req.maxdownload, req.bytecount
                    ),
                );
                req.close_conn = true;
            }
        } else if nwrite < buf.len() && !req.ignorebody {
            self.failf(
                cb,
                &format!(
                    "Exceeded the maximum allowed file size ({}) with {} bytes",
                    self.set.max_filesize, req.bytecount
                ),
            );
            return Err(code::FILESIZE_EXCEEDED);
        }
        Ok(())
    }

    /// content decoders then cw_out
    fn cw_content(
        &mut self,
        cb: &mut dyn Callbacks,
        req: &mut Req,
        flags: u32,
        buf: &[u8],
    ) -> Result<(), CurlCode> {
        if flags & CW_BODY != 0 && !req.cdecoders.is_empty() && !buf.is_empty() {
            // the last added decoder sees the data first
            let mut data = vec![buf.to_vec()];
            let mut decs = std::mem::take(&mut req.cdecoders);
            let mut err = None;
            for d in decs.iter_mut().rev() {
                let mut next = Vec::new();
                for piece in &data {
                    let r = d.write(piece, &mut |o| {
                        next.push(o.to_vec());
                        Ok(())
                    });
                    if let Err(e) = r {
                        err = Some(e);
                        break;
                    }
                }
                if err.is_some() {
                    break;
                }
                data = next;
            }
            req.cdecoders = decs;
            if let Some((c, m)) = err {
                // what was decoded before the error is lost like in libcurl
                if let Some(m) = m {
                    self.failf(cb, &m);
                }
                return Err(c);
            }
            for p in data {
                if !p.is_empty() {
                    self.cw_out(cb, req, flags, &p)?;
                }
            }
            return Ok(());
        }
        self.cw_out(cb, req, flags, buf)
    }

    /// cw_out_write()
    fn cw_out(
        &mut self,
        cb: &mut dyn Callbacks,
        req: &mut Req,
        flags: u32,
        buf: &[u8],
    ) -> Result<(), CurlCode> {
        if req.out_errored {
            return Err(code::WRITE_ERROR);
        }
        let r = (|| {
            if flags & CW_BODY != 0 || (flags & CW_HEADER != 0 && self.set.include_header) {
                self.cw_out_write(cb, buf, false)?;
            }
            if flags & (CW_HEADER | CW_INFO) != 0 {
                self.cw_out_write(cb, buf, true)?;
            }
            Ok(())
        })();
        if r.is_err() {
            req.out_errored = true;
        }
        r
    }

    // ------------------------------------------------------------------
    // the request

    /// Curl_http(): build and send the request
    fn http_do(
        &mut self,
        cb: &mut dyn Callbacks,
        conn: &mut Conn,
        t: &Target,
        req: &mut Req,
    ) -> Result<(), CurlCode> {
        let headers = self.set.slists.get(&opt::HTTPHEADER).cloned();
        let hl = headers.as_ref();
        // http_host()
        if !self.st.this_is_a_follow {
            self.st.first_host = Some(t.host.clone());
            self.st.first_remote_port = t.port;
            self.st.first_remote_protocol = t.protocol;
        }
        self.st.cookiehost = None;
        let mut host_hdr: Option<Vec<u8>> = None;
        let custom_host = checkheaders(hl, b"Host");
        if let Some(ch) = custom_host
            && (!self.st.this_is_a_follow
                || self
                    .st
                    .first_host
                    .as_deref()
                    .is_some_and(|f| f.eq_ignore_ascii_case(&t.host)))
        {
            let mut cookiehost = copy_header_value(ch);
            if !cookiehost.is_empty() {
                if cookiehost[0] == b'[' {
                    cookiehost.remove(0);
                    if let Some(e) = cookiehost.iter().position(|&c| c == b']') {
                        cookiehost.truncate(e);
                    }
                } else if let Some(c) = cookiehost.iter().position(|&c| c == b':') {
                    cookiehost.truncate(c);
                }
                self.st.cookiehost = Some(lossy(&cookiehost));
            }
            if !ch.eq_ignore_ascii_case(b"Host:") {
                let mut h = b"Host:".to_vec();
                h.extend_from_slice(&ch[5..]);
                h.extend_from_slice(b"\r\n");
                host_hdr = Some(h);
            }
        } else {
            let mut h = b"Host: ".to_vec();
            h.extend_from_slice(&t.tu.host_header(t.port));
            h.extend_from_slice(b"\r\n");
            host_hdr = Some(h);
        }
        let has_host = host_hdr.is_some();
        // http_useragent()
        if checkheaders(hl, b"User-Agent").is_some() {
            self.st.uagent = None;
        }
        // Curl_http_method()
        let mut httpreq = self.st.httpreq;
        if self.st.upload || httpreq == HttpReq::Put {
            httpreq = HttpReq::Put;
        }
        let custom = self.set.strs.get(&opt::CUSTOMREQUEST).cloned();
        let method: Vec<u8> = match (&custom, self.st.http_ignorecustom) {
            (Some(c), false) => c.clone(),
            _ => {
                if req.no_body {
                    b"HEAD".to_vec()
                } else {
                    match httpreq {
                        HttpReq::Post | HttpReq::PostForm | HttpReq::PostMime => b"POST".to_vec(),
                        HttpReq::Put => b"PUT".to_vec(),
                        HttpReq::Head => b"HEAD".to_vec(),
                        HttpReq::Get => b"GET".to_vec(),
                    }
                }
            }
        };
        req.method = method.clone();
        req.httpreq = httpreq;
        // auth
        let mut pq = t.tu.path.clone();
        if let Some(q) = &t.tu.query {
            pq.push(b'?');
            pq.extend_from_slice(q);
        }
        let authh = self.output_auth(cb, conn, t, req, &method, &pq, false)?;
        // referer
        let referer = match &self.st.referer {
            Some(r) if checkheaders(hl, b"Referer").is_none() => {
                let mut v = b"Referer: ".to_vec();
                v.extend_from_slice(r);
                v.extend_from_slice(b"\r\n");
                Some(v)
            }
            _ => None,
        };
        let accept_encoding = match self.set.strs.get(&opt::ACCEPT_ENCODING) {
            Some(e) if checkheaders(hl, b"Accept-Encoding").is_none() && !e.is_empty() => {
                let mut v = b"Accept-Encoding: ".to_vec();
                v.extend_from_slice(e);
                v.extend_from_slice(b"\r\n");
                Some(v)
            }
            _ => None,
        };
        // http_transferencode()
        let mut te_hdr: Option<Vec<u8>> = None;
        if checkheaders(hl, b"TE").is_none() && self.set.http_transfer_encoding {
            let cptr = checkheaders(hl, b"Connection").map(copy_header_value);
            let mut v = b"Connection: ".to_vec();
            if let Some(c) = &cptr {
                v.extend_from_slice(c);
                if !c.is_empty() {
                    v.extend_from_slice(b", ");
                }
            }
            v.extend_from_slice(b"TE\r\nTE: gzip\r\n");
            te_hdr = Some(v);
        }
        // http_request_version()
        let httpversion: u8 = if self.st.rcvd_min == 10
            || conn.httpversion_seen == 10
            || self.set.httpwant == httpver::_1_0
        {
            10
        } else {
            11
        };
        let httpstring = if httpversion == 11 { "1.1" } else { "1.0" };
        // http_req_set_reader()
        self.set_reader(cb, req, httpreq)?;
        let mut te = "";
        if let Some(p) = checkheaders(hl, b"Transfer-Encoding") {
            req.upload_chunky = compareheader(p, b"Transfer-Encoding", b"chunked");
        } else {
            let clen = req.body.total_length();
            if clen < 0 {
                if httpversion > 10 {
                    req.upload_chunky = true;
                } else {
                    self.failf(cb, "Chunky upload is not supported by HTTP 1.0");
                    return Err(code::UPLOAD_FAILED);
                }
            } else {
                req.upload_chunky = false;
            }
            if req.upload_chunky {
                te = "Transfer-Encoding: chunked\r\n";
            }
        }
        let p_accept = if checkheaders(hl, b"Accept").is_some() {
            ""
        } else {
            "Accept: */*\r\n"
        };
        // http_range()
        let mut rangeline: Option<Vec<u8>> = None;
        let range = self.set.strs.get(&opt::RANGE).cloned().or_else(|| {
            if self.st.resume_from > 0 {
                Some(format!("{}-", self.st.resume_from).into_bytes())
            } else {
                None
            }
        });
        if let Some(rg) = &range {
            if matches!(httpreq, HttpReq::Get | HttpReq::Head)
                && checkheaders(hl, b"Range").is_none()
            {
                let mut v = b"Range: bytes=".to_vec();
                v.extend_from_slice(rg);
                v.extend_from_slice(b"\r\n");
                rangeline = Some(v);
            } else if matches!(httpreq, HttpReq::Post | HttpReq::Put)
                && checkheaders(hl, b"Content-Range").is_none()
            {
                let clen = req.body.total_length();
                let v = if self.st.resume_from > 0 {
                    let total = if req.authneg {
                        self.st.infilesize
                    } else {
                        self.st.resume_from + clen
                    };
                    format!(
                        "Content-Range: bytes {}{}/{}\r\n",
                        lossy(rg),
                        total - 1,
                        total
                    )
                } else {
                    format!("Content-Range: bytes {}/{}\r\n", lossy(rg), clen)
                };
                rangeline = Some(v.into_bytes());
            }
        }
        // build
        let mut r: Vec<u8> = Vec::new();
        r.extend_from_slice(&method);
        r.push(b' ');
        // http_target()
        let target_override = self.set.strs.get(&opt::REQUEST_TARGET).cloned();
        if t.proxy.is_some() && !t.tunnel {
            match &target_override {
                Some(tg) => r.extend_from_slice(tg),
                None => r.extend_from_slice(&t.tu.proxy_request_url()),
            }
        } else {
            match &target_override {
                Some(tg) => r.extend_from_slice(tg),
                None => r.extend_from_slice(&t.tu.request_target()),
            }
        }
        r.extend_from_slice(format!(" HTTP/{httpstring}\r\n").as_bytes());
        if let Some(h) = &host_hdr {
            r.extend_from_slice(h);
        }
        r.extend_from_slice(&authh.proxy);
        r.extend_from_slice(&authh.host);
        if let Some(rl) = &rangeline {
            r.extend_from_slice(rl);
        }
        if self
            .set
            .strs
            .get(&opt::USERAGENT)
            .is_some_and(|u| !u.is_empty())
            && let Some(ua) = &self.st.uagent
        {
            r.extend_from_slice(ua);
        }
        r.extend_from_slice(p_accept.as_bytes());
        if let Some(v) = &te_hdr {
            r.extend_from_slice(v);
        }
        if let Some(v) = &accept_encoding {
            r.extend_from_slice(v);
        }
        if let Some(v) = &referer {
            r.extend_from_slice(v);
        }
        if t.proxy.is_some()
            && !t.tunnel
            && checkheaders(hl, b"Proxy-Connection").is_none()
            && checkheaders(self.set.slists.get(&opt::PROXYHEADER), b"Proxy-Connection").is_none()
        {
            r.extend_from_slice(b"Proxy-Connection: Keep-Alive\r\n");
        }
        r.extend_from_slice(te.as_bytes());
        // cookies
        self.http_cookies(cb, t, &mut r);
        // time condition
        self.add_timecondition(cb, hl, &mut r)?;
        // custom headers
        let state = HdrState {
            has_host,
            httpreq,
            authneg: req.authneg,
            te: te_hdr.is_some(),
        };
        self.add_custom_headers_st(&mut r, hl, &state, httpversion, false, t);
        if t.proxy.is_some() && !t.tunnel && self.set.sep_headers {
            let ph = self.set.slists.get(&opt::PROXYHEADER).cloned();
            self.add_custom_headers_st(&mut r, ph.as_ref(), &state, httpversion, false, t);
        }
        // http_req_complete()
        if req.upload_chunky {
            req.chunk_enc = Some(ChunkEnc {
                read_eos: false,
                eos: false,
                buf: Vec::new(),
            });
        }
        let req_clen = req.body.total_length();
        let mut announced_exp100 = false;
        if matches!(
            httpreq,
            HttpReq::Put | HttpReq::Post | HttpReq::PostForm | HttpReq::PostMime
        ) {
            if req_clen >= 0
                && !req.upload_chunky
                && (req.authneg || checkheaders(hl, b"Content-Length").is_none())
            {
                r.extend_from_slice(format!("Content-Length: {req_clen}\r\n").as_bytes());
            }
            if let Some(ct) = &self.mime_ct
                && matches!(httpreq, HttpReq::PostForm | HttpReq::PostMime)
            {
                r.extend_from_slice(ct);
                r.extend_from_slice(b"\r\n");
            }
            if httpreq == HttpReq::Post && checkheaders(hl, b"Content-Type").is_none() {
                r.extend_from_slice(b"Content-Type: application/x-www-form-urlencoded\r\n");
            }
            // addexpect()
            if let Some(p) = checkheaders(hl, b"Expect") {
                announced_exp100 = compareheader(p, b"Expect", b"100-continue");
            } else if !self.st.disableexpect && httpversion == 11 {
                let client_len = req_clen;
                if !(0..=EXPECT_100_THRESHOLD).contains(&client_len) {
                    r.extend_from_slice(b"Expect: 100-continue\r\n");
                    announced_exp100 = true;
                }
            }
        }
        r.extend_from_slice(b"\r\n");
        self.progress.set_upload_size(req_clen);
        if announced_exp100 {
            req.exp100 = Exp100::SendingRequest;
        }
        req.keep_recv = true;
        req.keep_send = true;
        req.header = true;
        req.httpversion_sent = httpversion;
        req.header_out = r.clone();
        // Curl_req_send()
        if req_clen == 0 {
            req.eos_read = true;
            self.xfer_send(cb, conn, req, &r, r.len())?;
        } else {
            req.sendbuf = r;
            req.sendbuf_hds_len = req.sendbuf.len();
            self.req_send_more(cb, conn, req)?;
        }
        Ok(())
    }

    /// set_reader(): the request body source
    fn set_reader(
        &mut self,
        cb: &mut dyn Callbacks,
        req: &mut Req,
        httpreq: HttpReq,
    ) -> Result<(), CurlCode> {
        self.mime_ct = None;
        let postsize = self.st.infilesize;
        if req.authneg {
            req.body = BodySrc::Null;
            return Ok(());
        }
        req.body = match httpreq {
            HttpReq::Put => {
                if postsize != 0 {
                    BodySrc::Fread {
                        total: postsize,
                        read: 0,
                        eos: false,
                    }
                } else {
                    BodySrc::Null
                }
            }
            HttpReq::PostForm | HttpReq::PostMime => match self.set.mimepost.clone() {
                Some(m) => {
                    let hl = self.set.slists.get(&opt::HTTPHEADER);
                    let cthdr: Vec<u8> = match checkheaders(hl, b"Content-Type") {
                        Some(h) => {
                            let mut v = &h[13..];
                            while v.first() == Some(&b' ') {
                                v = &v[1..];
                            }
                            v.to_vec()
                        }
                        None => b"multipart/form-data".to_vec(),
                    };
                    let (rd, ct) = MimeReader::new(&m.borrow(), &cthdr, self.set.mime_formescape);
                    self.mime_ct = Some(ct);
                    let total = rd.total;
                    self.st.infilesize = total;
                    BodySrc::Mime(Box::new(rd), m)
                }
                None => {
                    self.st.infilesize = 0;
                    BodySrc::Null
                }
            },
            HttpReq::Post => {
                if postsize == 0 {
                    BodySrc::Null
                } else if let Some(pf) = &self.set.postfields {
                    if postsize > 0 {
                        let mut d = pf.clone();
                        d.resize(postsize as usize, 0);
                        BodySrc::Buf { data: d, pos: 0 }
                    } else {
                        BodySrc::Null
                    }
                } else {
                    let hl = self.set.slists.get(&opt::HTTPHEADER);
                    let chunked = checkheaders(hl, b"Transfer-Encoding")
                        .is_some_and(|p| compareheader(p, b"Transfer-Encoding", b"chunked"));
                    BodySrc::Fread {
                        total: if chunked { -1 } else { postsize },
                        read: 0,
                        eos: false,
                    }
                }
            }
            _ => {
                self.st.infilesize = 0;
                BodySrc::Null
            }
        };
        // http_resume(): skip the first bytes of an upload
        if matches!(httpreq, HttpReq::Post | HttpReq::Put)
            && self.st.resume_from > 0
            && !req.authneg
        {
            let mut left = self.st.resume_from;
            if let BodySrc::Fread { total, .. } = &mut req.body {
                // try the seek callback first
                let mut ok = false;
                if self.set.callbacks.contains(&opt::SEEKFUNCTION) {
                    let r = cb.seek(left, 0);
                    if r == 0 {
                        ok = true;
                    } else if r == 1 {
                        self.failf(cb, "Could not seek stream");
                        return Err(code::READ_ERROR);
                    }
                }
                if !ok {
                    while left > 0 {
                        let want = left.min(16384) as usize;
                        match cb.read(want) {
                            ReadResult::Data(d) if !d.is_empty() => left -= d.len() as i64,
                            _ => {
                                self.failf(
                                    cb,
                                    &format!(
                                        "Could only read {} bytes from the input",
                                        self.st.resume_from - left
                                    ),
                                );
                                return Err(code::READ_ERROR);
                            }
                        }
                    }
                }
                if *total > 0 {
                    *total -= self.st.resume_from;
                    if *total <= 0 {
                        self.failf(cb, "File already completely uploaded");
                        return Err(code::PARTIAL_FILE);
                    }
                }
            }
        }
        Ok(())
    }

    /// Curl_add_timecondition()
    fn add_timecondition(
        &mut self,
        cb: &mut dyn Callbacks,
        hl: Option<&Vec<Vec<u8>>>,
        r: &mut Vec<u8>,
    ) -> Result<(), CurlCode> {
        let cond = self.set.timecondition;
        if cond == 0 {
            return Ok(());
        }
        let name: &[u8] = match cond {
            1 => b"If-Modified-Since",
            2 => b"If-Unmodified-Since",
            3 => b"Last-Modified",
            _ => {
                let _ = cb;
                return Err(code::BAD_FUNCTION_ARGUMENT);
            }
        };
        if checkheaders(hl, name).is_some() {
            return Ok(());
        }
        let (y, mon, d, h, mi, s, wday) = gmtime(self.set.timevalue);
        let wd = WKDAY[if wday > 0 { wday - 1 } else { 6 }];
        r.extend_from_slice(name);
        r.extend_from_slice(
            format!(
                ": {wd}, {d:02} {} {y:4} {h:02}:{mi:02}:{s:02} GMT\r\n",
                MONTH[mon]
            )
            .as_bytes(),
        );
        Ok(())
    }

    /// http_cookies()
    fn http_cookies(&mut self, cb: &mut dyn Callbacks, t: &Target, r: &mut Vec<u8>) {
        let hl = self.set.slists.get(&opt::HTTPHEADER);
        let addcookies = match self.set.strs.get(&opt::COOKIE) {
            Some(c) if checkheaders(hl, b"Cookie").is_none() => Some(c.clone()),
            _ => None,
        };
        let host = self.st.cookiehost.clone().unwrap_or_else(|| t.host.clone());
        let secure = super::cookie::secure_context(
            t.protocol & (proto::HTTPS | (1i64 << 31)) != 0,
            host.as_bytes(),
        );
        let mut log = Vec::new();
        let jar = self.cookies.clone();
        let line = match &jar {
            Some(j) => super::cookie::http_cookie_header(
                Some(&mut j.borrow_mut()),
                self.cookie_engine,
                host.as_bytes(),
                &t.tu.path,
                secure,
                addcookies.as_deref(),
                &mut log,
            ),
            None => super::cookie::http_cookie_header(
                None,
                self.cookie_engine,
                host.as_bytes(),
                &t.tu.path,
                secure,
                addcookies.as_deref(),
                &mut log,
            ),
        };
        let tr = self.tracer();
        for l in log {
            tr.text(cb, &l);
        }
        r.extend_from_slice(&line);
    }

    fn add_custom_headers(
        &mut self,
        r: &mut Vec<u8>,
        hl: Option<&Vec<Vec<u8>>>,
        req: &Req,
        httpversion: u8,
        is_connect: bool,
    ) {
        let st = HdrState {
            has_host: true,
            httpreq: req.httpreq,
            authneg: req.authneg,
            te: false,
        };
        let _ = is_connect;
        let Some(hl) = hl else { return };
        for h in hl {
            if let Some(line) = custom_header_line(h, &st, httpversion, true) {
                r.extend_from_slice(&line);
            }
        }
    }

    /// Curl_add_custom_headers()
    fn add_custom_headers_st(
        &mut self,
        r: &mut Vec<u8>,
        hl: Option<&Vec<Vec<u8>>>,
        st: &HdrState,
        httpversion: u8,
        _is_connect: bool,
        t: &Target,
    ) {
        let Some(hl) = hl else { return };
        let allowed = self.auth_allowed_to_host(t);
        for h in hl {
            if let Some(line) = custom_header_line(h, st, httpversion, allowed) {
                r.extend_from_slice(&line);
            }
        }
    }

    /// Curl_auth_allowed_to_host()
    fn auth_allowed_to_host(&self, t: &Target) -> bool {
        !self.st.this_is_a_follow
            || self.set.allow_auth_to_other_hosts
            || (self
                .st
                .first_host
                .as_deref()
                .is_some_and(|f| f.eq_ignore_ascii_case(&t.host))
                && self.st.first_remote_port == t.port
                && self.st.first_remote_protocol == t.protocol)
    }

    /// Curl_http_output_auth()
    fn output_auth(
        &mut self,
        cb: &mut dyn Callbacks,
        conn: &mut Conn,
        t: &Target,
        req: &mut Req,
        method: &[u8],
        path: &[u8],
        proxytunnel: bool,
    ) -> Result<AuthHeaders, CurlCode> {
        let mut out = AuthHeaders::default();
        let has_bearer = self.set.strs.contains_key(&opt::XOAUTH2_BEARER);
        if !((t.proxy.is_some() && t.proxy_user_passwd) || self.st.user.is_some() || has_bearer) {
            self.st.authhost.done = true;
            self.st.authproxy.done = true;
            return Ok(out);
        }
        if self.st.authhost.want != 0 && self.st.authhost.picked == 0 {
            self.st.authhost.picked = self.st.authhost.want;
        }
        if self.st.authproxy.want != 0 && self.st.authproxy.picked == 0 {
            self.st.authproxy.picked = self.st.authproxy.want;
        }
        if t.proxy.is_some() && t.tunnel == proxytunnel {
            out.proxy = self.output_auth_headers(cb, conn, t, true, method, path)?;
        } else {
            self.st.authproxy.done = true;
        }
        if !proxytunnel || t.proxy.is_none() {
            if self.auth_allowed_to_host(t) {
                out.host = self.output_auth_headers(cb, conn, t, false, method, path)?;
            } else {
                self.st.authhost.done = true;
            }
        }
        let httpreq = req.httpreq;
        req.authneg = ((self.st.authhost.multipass && !self.st.authhost.done)
            || (self.st.authproxy.multipass && !self.st.authproxy.done))
            && httpreq != HttpReq::Get
            && httpreq != HttpReq::Head;
        Ok(out)
    }

    /// output_auth_headers()
    fn output_auth_headers(
        &mut self,
        cb: &mut dyn Callbacks,
        conn: &mut Conn,
        t: &Target,
        proxy: bool,
        method: &[u8],
        path: &[u8],
    ) -> Result<Vec<u8>, CurlCode> {
        let picked = if proxy {
            self.st.authproxy.picked
        } else {
            self.st.authhost.picked
        };
        let mut name: Option<&str> = None;
        let mut hdr = Vec::new();
        let hl = self.set.slists.get(&opt::HTTPHEADER);
        let (user, pass) = if proxy {
            (self.st.proxyuser.clone(), self.st.proxypasswd.clone())
        } else {
            (self.st.user.clone(), self.st.passwd.clone())
        };
        if picked == auth::NTLM {
            name = Some("NTLM");
            let state = if proxy {
                &mut conn.proxy_ntlm
            } else {
                &mut conn.ntlm
            };
            let mut fail = Vec::new();
            let r = state.output(
                proxy,
                user.as_deref().unwrap_or(b""),
                pass.as_deref().unwrap_or(b""),
                None,
                None,
                &mut fail,
            );
            for f in fail {
                self.failf(cb, &f);
            }
            let r = r?;
            if let Some(h) = r.header {
                hdr = h;
            }
            if r.force_picked {
                if proxy {
                    self.info.proxyauthpicked = auth::NTLM;
                } else {
                    self.info.httpauthpicked = auth::NTLM;
                }
            }
            let a = if proxy {
                &mut self.st.authproxy
            } else {
                &mut self.st.authhost
            };
            a.done = r.done;
        } else if picked == auth::DIGEST {
            name = Some("Digest");
            let ie = self.st.authhost.iestyle;
            let d = if proxy {
                &mut self.st.proxydigest
            } else {
                &mut self.st.digest
            };
            let done = match d.output(
                proxy,
                user.as_deref().unwrap_or(b""),
                pass.as_deref().unwrap_or(b""),
                method,
                path,
                ie,
                None,
            ) {
                Ok(Some(h)) => {
                    hdr = h;
                    true
                }
                Ok(None) => false,
                Err(e) => return Err(e),
            };
            let a = if proxy {
                &mut self.st.authproxy
            } else {
                &mut self.st.authhost
            };
            a.done = done;
        } else if picked == auth::BASIC {
            let need = if proxy {
                t.proxy_user_passwd
                    && checkheaders(
                        self.set.slists.get(&opt::PROXYHEADER).or(hl),
                        b"Proxy-authorization",
                    )
                    .is_none()
            } else {
                self.st.user.is_some() && checkheaders(hl, b"Authorization").is_none()
            };
            if need {
                name = Some("Basic");
                let mut up = user.clone().unwrap_or_default();
                up.push(b':');
                up.extend_from_slice(pass.as_deref().unwrap_or(b""));
                if proxy {
                    hdr.extend_from_slice(b"Proxy-");
                }
                hdr.extend_from_slice(b"Authorization: Basic ");
                hdr.extend_from_slice(&base64(&up));
                hdr.extend_from_slice(b"\r\n");
            }
            let a = if proxy {
                &mut self.st.authproxy
            } else {
                &mut self.st.authhost
            };
            a.done = true;
        } else if picked == auth::BEARER {
            if !proxy
                && let Some(b) = self.set.strs.get(&opt::XOAUTH2_BEARER)
                && checkheaders(hl, b"Authorization").is_none()
            {
                name = Some("Bearer");
                hdr.extend_from_slice(b"Authorization: Bearer ");
                hdr.extend_from_slice(b);
                hdr.extend_from_slice(b"\r\n");
            }
            let a = if proxy {
                &mut self.st.authproxy
            } else {
                &mut self.st.authhost
            };
            a.done = true;
        } else if picked == auth::AWS_SIGV4 && !proxy {
            name = Some("AWS_SIGV4");
        }
        let a = if proxy {
            &mut self.st.authproxy
        } else {
            &mut self.st.authhost
        };
        if let Some(n) = name {
            a.multipass = !a.done;
            let u = lossy(user.as_deref().unwrap_or(b""));
            self.infof(
                cb,
                &format!(
                    "{} auth using {n} with user '{u}'",
                    if proxy { "Proxy" } else { "Server" }
                ),
            );
        } else {
            a.multipass = false;
        }
        Ok(hdr)
    }

    /// Curl_http_input_auth()
    pub(crate) fn input_auth(&mut self, cb: &mut dyn Callbacks, proxy: bool, value: &[u8]) {
        let mut p = value;
        loop {
            if authcmp(b"NTLM", p) {
                let a = if proxy {
                    &mut self.st.authproxy
                } else {
                    &mut self.st.authhost
                };
                a.avail |= auth::NTLM;
                if proxy {
                    self.info.proxyauthavail |= auth::NTLM;
                } else {
                    self.info.httpauthavail |= auth::NTLM;
                }
                if a.picked == auth::NTLM {
                    self.pending_ntlm.push((proxy, p.to_vec()));
                }
            }
            if authcmp(b"Digest", p) {
                let a = if proxy {
                    &mut self.st.authproxy
                } else {
                    &mut self.st.authhost
                };
                if a.avail & auth::DIGEST != 0 {
                    self.infof(cb, "Ignoring duplicate digest auth header.");
                } else {
                    a.avail |= auth::DIGEST;
                    if proxy {
                        self.info.proxyauthavail |= auth::DIGEST;
                    } else {
                        self.info.httpauthavail |= auth::DIGEST;
                    }
                    let d = if proxy {
                        &mut self.st.proxydigest
                    } else {
                        &mut self.st.digest
                    };
                    if d.input(p).is_err() {
                        self.infof(cb, "Digest authentication problem, ignoring.");
                        self.st.authproblem = true;
                    }
                }
            }
            if authcmp(b"Basic", p) {
                let a = if proxy {
                    &mut self.st.authproxy
                } else {
                    &mut self.st.authhost
                };
                a.avail |= auth::BASIC;
                if proxy {
                    self.info.proxyauthavail |= auth::BASIC;
                } else {
                    self.info.httpauthavail |= auth::BASIC;
                }
                if a.picked == auth::BASIC {
                    a.avail = auth::NONE;
                    self.infof(cb, "Basic authentication problem, ignoring.");
                    self.st.authproblem = true;
                }
            }
            if authcmp(b"Bearer", p) {
                let a = if proxy {
                    &mut self.st.authproxy
                } else {
                    &mut self.st.authhost
                };
                a.avail |= auth::BEARER;
                if proxy {
                    self.info.proxyauthavail |= auth::BEARER;
                } else {
                    self.info.httpauthavail |= auth::BEARER;
                }
                if a.picked == auth::BEARER {
                    a.avail = auth::NONE;
                    self.infof(cb, "Bearer authentication problem, ignoring.");
                    self.st.authproblem = true;
                }
            }
            match p.iter().position(|&c| c == b',') {
                Some(i) => {
                    p = &p[i + 1..];
                    while let Some((&c, rest)) = p.split_first() {
                        if c == b' ' || c == b'\t' {
                            p = rest;
                        } else {
                            break;
                        }
                    }
                }
                None => break,
            }
        }
    }

    /// Curl_http_auth_act()
    fn auth_act(
        &mut self,
        cb: &mut dyn Callbacks,
        conn: &mut Conn,
        t: &Target,
        req: &mut Req,
    ) -> Result<(), CurlCode> {
        let mut authmask = !0u64;
        if !self.set.strs.contains_key(&opt::XOAUTH2_BEARER) {
            authmask &= !auth::BEARER;
        }
        if (100..=199).contains(&req.httpcode) {
            return Ok(());
        }
        if self.st.authproblem {
            return if self.set.fail_on_error {
                Err(code::HTTP_RETURNED_ERROR)
            } else {
                Ok(())
            };
        }
        let mut pickhost = false;
        let mut pickproxy = false;
        if (self.st.user.is_some() || self.set.strs.contains_key(&opt::XOAUTH2_BEARER))
            && (req.httpcode == 401 || (req.authneg && req.httpcode < 300))
        {
            pickhost = pickoneauth(&mut self.st.authhost, authmask);
            if !pickhost {
                self.st.authproblem = true;
            } else {
                self.info.httpauthpicked = self.st.authhost.picked;
            }
        }
        if t.proxy_user_passwd && (req.httpcode == 407 || (req.authneg && req.httpcode < 300)) {
            pickproxy = pickoneauth(&mut self.st.authproxy, authmask & !auth::BEARER);
            if !pickproxy {
                self.st.authproblem = true;
            } else {
                self.info.proxyauthpicked = self.st.authproxy.picked;
            }
        }
        // NTLM input happens after picking
        for (proxy, v) in std::mem::take(&mut self.pending_ntlm) {
            let st = if proxy {
                &mut conn.proxy_ntlm
            } else {
                &mut conn.ntlm
            };
            let mut log = Vec::new();
            let r = st.input(&v, &mut log);
            for l in log {
                self.infof(cb, &l);
            }
            if r.is_err() {
                self.infof(cb, "NTLM authentication problem, ignoring.");
                self.st.authproblem = true;
            }
        }
        if pickhost || pickproxy {
            self.perhapsrewind(cb, conn, req);
            req.newurl = self.st.url.clone();
        } else if req.httpcode < 300 && !self.st.authhost.done && req.authneg {
            if self.st.httpreq != HttpReq::Get && self.st.httpreq != HttpReq::Head {
                req.newurl = self.st.url.clone();
                self.st.authhost.done = true;
            }
        }
        if self.http_should_fail(t, req.httpcode) {
            self.failf(
                cb,
                &format!("The requested URL returned error: {}", req.httpcode),
            );
            return Err(code::HTTP_RETURNED_ERROR);
        }
        Ok(())
    }

    /// http_should_fail()
    fn http_should_fail(&self, t: &Target, httpcode: i64) -> bool {
        if !self.set.fail_on_error {
            return false;
        }
        if httpcode < 400 {
            return false;
        }
        if self.st.resume_from > 0 && self.st.httpreq == HttpReq::Get && httpcode == 416 {
            return false;
        }
        if httpcode != 401 && httpcode != 407 {
            return true;
        }
        if httpcode == 401 && self.st.user.is_none() {
            return true;
        }
        if httpcode == 407 && !t.proxy_user_passwd {
            return true;
        }
        self.st.authproblem
    }

    /// http_perhapsrewind()
    fn perhapsrewind(&mut self, cb: &mut dyn Callbacks, conn: &mut Conn, req: &mut Req) {
        let bytessent = req.writebytecount;
        let expectsend = req.body.total_length();
        let upload_remain = if expectsend >= 0 {
            expectsend - bytessent
        } else {
            -1
        };
        let little = (0..2000).contains(&upload_remain);
        let needs_rewind = match &req.body {
            BodySrc::Fread { read, eos, .. } => *read > 0 || *eos,
            BodySrc::Buf { pos, .. } => *pos > 0,
            BodySrc::Mime(..) => req.writebytecount > 0 || !req.sendbuf.is_empty() || req.eos_read,
            BodySrc::Null => false,
        };
        let mut abort_upload = !req.upload_done && !little;
        if needs_rewind {
            self.infof(cb, "Need to rewind upload for next request");
            self.st.rewind_read = true;
        }
        if conn.close {
            return;
        }
        let mut ongoing = None;
        if abort_upload
            && (self.st.authproxy.picked == auth::NTLM || self.st.authhost.picked == auth::NTLM)
        {
            ongoing = Some("NTLM");
            if conn.ntlm.state() != 0 || conn.proxy_ntlm.state() != 0 {
                abort_upload = false;
            }
        }
        if abort_upload {
            let pre = match ongoing {
                Some(o) => format!("{o} send, "),
                None => String::new(),
            };
            if upload_remain >= 0 {
                self.infof(
                    cb,
                    &format!("{pre}close instead of sending {upload_remain} more bytes"),
                );
            } else {
                self.infof(
                    cb,
                    &format!("{pre}close instead of sending unknown amount of more bytes"),
                );
            }
            conn.close = true;
            req.size = 0;
        }
    }

    // ------------------------------------------------------------------
    // sending

    /// xfer_send(): write to the connection, trace HEADER_OUT/DATA_OUT
    fn xfer_send(
        &mut self,
        cb: &mut dyn Callbacks,
        conn: &mut Conn,
        req: &mut Req,
        buf: &[u8],
        hds_len: usize,
    ) -> Result<(), CurlCode> {
        if buf.is_empty() {
            if req.eos_read {
                req.eos_sent = true;
            }
            return Ok(());
        }
        if let Err(e) = conn.send_all(buf) {
            let errno = e.raw_os_error().unwrap_or(libc::EPIPE);
            self.info.os_errno = errno as i64;
            self.failf(cb, &format!("Send failure: {}", net::os_strerror(errno)));
            return Err(code::SEND_ERROR);
        }
        self.info.request_size += buf.len() as i64;
        if req.eos_read {
            req.eos_sent = true;
        }
        let tr = self.tracer();
        if hds_len > 0 {
            tr.debug(cb, info::HEADER_OUT, &buf[..hds_len.min(buf.len())]);
        }
        if buf.len() > hds_len {
            let body = &buf[hds_len..];
            tr.debug(cb, info::DATA_OUT, body);
            req.writebytecount += body.len() as i64;
            self.progress.ul_cur = req.writebytecount;
        }
        Ok(())
    }

    /// Curl_req_send_more()
    fn req_send_more(
        &mut self,
        cb: &mut dyn Callbacks,
        conn: &mut Conn,
        req: &mut Req,
    ) -> Result<(), CurlCode> {
        let cap = self.set.upload_buffer_size as usize;
        if !req.upload_aborted && !req.eos_read && req.sendbuf.len() < cap {
            let max = cap - req.sendbuf.len();
            let (data, eos) = self.client_read(cb, req, max)?;
            req.sendbuf.extend_from_slice(&data);
            if eos {
                req.eos_read = true;
            }
        }
        self.req_flush(cb, conn, req)
    }

    /// req_flush()
    fn req_flush(
        &mut self,
        cb: &mut dyn Callbacks,
        conn: &mut Conn,
        req: &mut Req,
    ) -> Result<(), CurlCode> {
        if !req.sendbuf.is_empty() {
            let buf = std::mem::take(&mut req.sendbuf);
            let hds = req.sendbuf_hds_len.min(buf.len());
            req.sendbuf_hds_len = 0;
            self.xfer_send(cb, conn, req, &buf, hds)?;
        }
        if req.eos_read && !req.eos_sent {
            req.eos_sent = true;
        }
        if !req.upload_done && req.eos_read && req.eos_sent {
            self.req_set_upload_done(cb, req);
        }
        Ok(())
    }

    /// req_set_upload_done()
    fn req_set_upload_done(&mut self, cb: &mut dyn Callbacks, req: &mut Req) {
        req.upload_done = true;
        req.keep_send = false;
        self.progress.time(Timer::PostTransfer);
        if req.upload_aborted {
            req.sendbuf.clear();
            if req.writebytecount > 0 {
                self.infof(
                    cb,
                    &format!(
                        "abort upload after having sent {} bytes",
                        req.writebytecount
                    ),
                );
            } else {
                self.infof(cb, "abort upload");
            }
        } else if req.writebytecount > 0 {
            self.infof(
                cb,
                &format!("upload completely sent off: {} bytes", req.writebytecount),
            );
        } else if !req.download_done {
            if req.body.total_length() != 0 {
                self.infof(cb, "We are completely uploaded and fine");
            } else {
                self.infof(cb, "Request completely sent off");
            }
        }
        if let Exp100::Awaiting(_) | Exp100::SendingRequest = req.exp100 {
            req.exp100 = Exp100::SendData;
        }
    }

    /// Curl_req_abort_sending()
    fn req_abort_sending(&mut self, cb: &mut dyn Callbacks, req: &mut Req) {
        if !req.upload_done {
            req.sendbuf.clear();
            req.upload_aborted = true;
            req.keep_send = false;
            if req.exp100 != Exp100::None {
                req.exp100 = Exp100::Failed;
            }
            self.req_set_upload_done(cb, req);
        }
    }

    /// Curl_client_read(): chunked encoder -> exp100 gate -> body source
    fn client_read(
        &mut self,
        cb: &mut dyn Callbacks,
        req: &mut Req,
        max: usize,
    ) -> Result<(Vec<u8>, bool), CurlCode> {
        if let Some(mut enc) = req.chunk_enc.take() {
            let r = self.chunked_read(cb, req, &mut enc, max);
            req.chunk_enc = Some(enc);
            return r;
        }
        self.exp100_read(cb, req, max)
    }

    /// cr_chunked_read()
    fn chunked_read(
        &mut self,
        cb: &mut dyn Callbacks,
        req: &mut Req,
        enc: &mut ChunkEnc,
        max: usize,
    ) -> Result<(Vec<u8>, bool), CurlCode> {
        if enc.eos {
            return Ok((Vec::new(), true));
        }
        if !enc.read_eos && enc.buf.is_empty() {
            // add_chunk()
            let blen = max.min(64 * 1024);
            let blen = if blen < 1024 { 1024 } else { blen - 12 };
            let (data, eos) = self.exp100_read(cb, req, blen)?;
            if eos {
                enc.read_eos = true;
            }
            if !data.is_empty() {
                enc.buf
                    .extend_from_slice(format!("{:x}\r\n", data.len()).as_bytes());
                enc.buf.extend_from_slice(&data);
                enc.buf.extend_from_slice(b"\r\n");
            }
            if enc.read_eos {
                // add_last_chunk(): no trailers
                enc.buf.extend_from_slice(b"0\r\n\r\n");
            }
        }
        if !enc.buf.is_empty() {
            let n = enc.buf.len().min(max);
            let out: Vec<u8> = enc.buf.drain(..n).collect();
            if enc.read_eos && enc.buf.is_empty() {
                enc.eos = true;
                return Ok((out, true));
            }
            return Ok((out, false));
        }
        Ok((Vec::new(), false))
    }

    /// cr_exp100_read()
    fn exp100_read(
        &mut self,
        cb: &mut dyn Callbacks,
        req: &mut Req,
        max: usize,
    ) -> Result<(Vec<u8>, bool), CurlCode> {
        match req.exp100 {
            Exp100::SendingRequest => {
                if !req.sendbuf.is_empty() {
                    return Ok((Vec::new(), false));
                }
                req.exp100 = Exp100::Awaiting(Instant::now());
                return Ok((Vec::new(), false));
            }
            Exp100::Failed => return Err(code::READ_ERROR),
            Exp100::Awaiting(start) => {
                if (start.elapsed().as_millis() as i64) < self.set.expect_100_timeout {
                    return Ok((Vec::new(), false));
                }
                req.exp100 = Exp100::SendData;
                self.infof(cb, "Done waiting for 100-continue");
            }
            _ => {}
        }
        self.body_read(cb, req, max)
    }

    /// The base client readers (cr_in, cr_buf, mime)
    fn body_read(
        &mut self,
        cb: &mut dyn Callbacks,
        req: &mut Req,
        max: usize,
    ) -> Result<(Vec<u8>, bool), CurlCode> {
        match &mut req.body {
            BodySrc::Null => Ok((Vec::new(), true)),
            BodySrc::Buf { data, pos } => {
                let n = (data.len() - *pos).min(max);
                let v = data[*pos..*pos + n].to_vec();
                *pos += n;
                Ok((v, *pos >= data.len()))
            }
            BodySrc::Fread { total, read, eos } => {
                if *eos {
                    return Ok((Vec::new(), true));
                }
                let mut blen = max;
                if *total >= 0 {
                    let remain = *total - *read;
                    blen = if remain <= 0 {
                        0
                    } else {
                        (remain as usize).min(blen)
                    };
                }
                let (total, readn) = (*total, *read);
                let res = if blen > 0 { Some(cb.read(blen)) } else { None };
                let BodySrc::Fread { read, eos, .. } = &mut req.body else {
                    unreachable!()
                };
                match res {
                    None => {
                        if total >= 0 && readn < total {
                            let msg = format!(
                                "client read function EOF fail, only {readn}/{total} of needed bytes read"
                            );
                            self.failf(cb, &msg);
                            return Err(code::READ_ERROR);
                        }
                        *eos = true;
                        Ok((Vec::new(), true))
                    }
                    Some(ReadResult::Abort) => {
                        self.failf(cb, "operation aborted by callback");
                        Err(code::ABORTED_BY_CALLBACK)
                    }
                    Some(ReadResult::Pause) => {
                        // cannot pause a synchronous transfer: try again soon
                        std::thread::sleep(Duration::from_millis(1));
                        Ok((Vec::new(), false))
                    }
                    Some(ReadResult::Data(d)) => {
                        if d.is_empty() {
                            if total >= 0 && readn < total {
                                let msg = format!(
                                    "client read function EOF fail, only {readn}/{total} of needed bytes read"
                                );
                                self.failf(cb, &msg);
                                return Err(code::READ_ERROR);
                            }
                            *eos = true;
                            return Ok((Vec::new(), true));
                        }
                        if d.len() > blen {
                            self.failf(cb, "read function returned funny value");
                            return Err(code::READ_ERROR);
                        }
                        *read += d.len() as i64;
                        let e = total >= 0 && *read >= total;
                        *eos = e;
                        Ok((d, e))
                    }
                }
            }
            BodySrc::Mime(rd, m) => {
                let m = m.clone();
                let r = rd.read(&mut m.borrow_mut(), max);
                match r {
                    Ok(v) => Ok(v),
                    Err((c, msg)) => {
                        self.failf(cb, &msg);
                        Err(c)
                    }
                }
            }
        }
    }

    // ------------------------------------------------------------------
    // the transfer loop

    /// Curl_sendrecv() until the request is done
    fn sendrecv(
        &mut self,
        cb: &mut dyn Callbacks,
        conn: &mut Conn,
        t: &Target,
        req: &mut Req,
    ) -> Result<(), CurlCode> {
        for _ in 0..2 {
            if self.pgrs_update(cb) {
                return Err(code::ABORTED_BY_CALLBACK);
            }
        }
        // the first round: flush the request (libcurl gets there before any
        // response byte is read on plain connections)
        if !req.upload_done {
            self.req_send_more(cb, conn, req)?;
        }
        loop {
            if req.done {
                return Ok(());
            }
            let want_send = !req.upload_done && !req.upload_aborted;
            // wait for data, bounded by the timeouts
            let left = self.timeleft(false);
            if left <= 0 && self.set.timeout_ms > 0 {
                return Err(self.timeout_error(cb, req));
            }
            if req.keep_recv {
                let mut wait = if left == i64::MAX {
                    None
                } else {
                    Some(Duration::from_millis(left.max(1) as u64))
                };
                if self.set.low_speed_time > 0 && self.set.low_speed_limit > 0 {
                    let sec = Duration::from_millis(1000);
                    wait = Some(wait.map_or(sec, |w| w.min(sec)));
                }
                if want_send {
                    // only poll when we also have something to send
                    let sendable = match req.exp100 {
                        Exp100::Awaiting(start) => {
                            let ms =
                                self.set.expect_100_timeout - start.elapsed().as_millis() as i64;
                            wait =
                                Some(wait.map_or(Duration::from_millis(ms.max(0) as u64), |w| {
                                    w.min(Duration::from_millis(ms.max(0) as u64))
                                }));
                            false
                        }
                        Exp100::SendingRequest => true,
                        _ => true,
                    };
                    if sendable {
                        wait = Some(Duration::from_millis(0));
                    }
                }
                if conn.buffered() == 0 {
                    match conn.fill(wait) {
                        Ok(0) => {
                            // EOF
                            req.keep_recv = false;
                            // Curl_req_stop_send_recv()
                            req.keep_send = false;
                            self.req_abort_sending(cb, req);
                            if !req.eos_written {
                                self.xfer_write_resp(cb, conn, t, req, &[], true)?;
                            }
                        }
                        Ok(_) => {}
                        Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {
                            if !want_send && self.set.timeout_ms > 0 && self.timeleft(false) <= 0 {
                                return Err(self.timeout_error(cb, req));
                            }
                        }
                        Err(e) => {
                            if let Some(t) = conn.tls.as_mut() {
                                self.drain_tls_log(cb, t);
                            }
                            if let Some((c, m)) = conn.tls_err.take() {
                                if !m.is_empty() {
                                    self.failf(cb, &m);
                                }
                                return Err(c);
                            }
                            let errno = e.raw_os_error().unwrap_or(libc::ECONNRESET);
                            self.info.os_errno = errno as i64;
                            self.failf(cb, &format!("Recv failure: {}", net::os_strerror(errno)));
                            return Err(code::RECV_ERROR);
                        }
                    }
                }
                if conn.buffered() > 0 {
                    // limit reads like xfer_recv_resp() when the body size is known
                    let mut max = self.set.buffer_size as usize;
                    if !req.header && req.size != -1 {
                        let totalleft = req.size - req.bytecount;
                        if totalleft <= 0 {
                            max = 0;
                        } else {
                            max = max.min(totalleft as usize);
                        }
                    }
                    if max > 0 || req.header {
                        let data = conn.take(if req.header { usize::MAX } else { max });
                        let consumed_all = self.xfer_write_resp(cb, conn, t, req, &data, false)?;
                        let _ = consumed_all;
                    }
                    if req.download_done {
                        req.keep_recv = false;
                    }
                } else if req.download_done {
                    req.keep_recv = false;
                }
                if let Some(tls) = conn.tls.as_mut() {
                    self.drain_tls_log(cb, tls);
                }
                if req.done {
                    return Ok(());
                }
            }
            if !req.upload_done && !req.upload_aborted && req.keep_send {
                self.req_send_more(cb, conn, req)?;
            } else if !req.keep_recv && !req.upload_done && !req.upload_aborted {
                // nothing to receive anymore but still sending
                self.req_send_more(cb, conn, req)?;
            }
            if !req.keep_recv && (req.upload_done || req.upload_aborted) {
                if !req.keep_recv && req.keep_send && conn.close {
                    self.infof(
                        cb,
                        "we are done reading and this is set to close, stop send",
                    );
                    self.req_abort_sending(cb, req);
                }
                // all done
                if self.pgrs_update(cb) {
                    return Err(code::ABORTED_BY_CALLBACK);
                }
                if !req.no_body
                    && req.size != -1
                    && req.bytecount != req.size
                    && req.newurl.is_none()
                {
                    self.failf(
                        cb,
                        &format!(
                            "transfer closed with {} bytes remaining to read",
                            req.size - req.bytecount
                        ),
                    );
                    return Err(code::PARTIAL_FILE);
                }
                if req.close_conn {
                    conn.close = true;
                }
                req.done = true;
                return Ok(());
            }
            if self.pgrs_update(cb) {
                return Err(code::ABORTED_BY_CALLBACK);
            }
            self.speedcheck(cb, req)?;
            if req.close_conn {
                conn.close = true;
            }
        }
    }

    fn req_abort_sending_if_needed(&mut self, _req: &mut Req) {}

    /// The "Operation timed out" failf
    fn timeout_error(&mut self, cb: &mut dyn Callbacks, req: &Req) -> CurlCode {
        let ms = self.progress.t_startop.elapsed().as_millis();
        if req.size != -1 {
            self.failf(
                cb,
                &format!(
                    "Operation timed out after {ms} milliseconds with {} out of {} bytes received",
                    req.bytecount, req.size
                ),
            );
        } else {
            self.failf(
                cb,
                &format!(
                    "Operation timed out after {ms} milliseconds with {} bytes received",
                    req.bytecount
                ),
            );
        }
        code::OPERATION_TIMEDOUT
    }

    /// Curl_speedcheck()
    fn speedcheck(&mut self, cb: &mut dyn Callbacks, req: &mut Req) -> Result<(), CurlCode> {
        if self.set.low_speed_time <= 0 || self.set.low_speed_limit <= 0 {
            return Ok(());
        }
        let now = Instant::now();
        let speed = self.progress.dl_speed.max(self.progress.ul_speed);
        match self.low_speed_since {
            Some(since) if speed < self.set.low_speed_limit => {
                let howlong = now.duration_since(since).as_millis() as i64;
                if howlong >= self.set.low_speed_time * 1000 {
                    self.failf(
                        cb,
                        &format!(
                            "Operation too slow. Less than {} bytes/sec transferred the last {} seconds",
                            self.set.low_speed_limit, self.set.low_speed_time
                        ),
                    );
                    return Err(code::OPERATION_TIMEDOUT);
                }
            }
            None if speed < self.set.low_speed_limit => self.low_speed_since = Some(now),
            _ => {
                if speed >= self.set.low_speed_limit {
                    self.low_speed_since = None;
                }
            }
        }
        let _ = req;
        Ok(())
    }

    /// Curl_http_write_resp(): headers then body
    fn xfer_write_resp(
        &mut self,
        cb: &mut dyn Callbacks,
        conn: &mut Conn,
        t: &Target,
        req: &mut Req,
        buf: &[u8],
        is_eos: bool,
    ) -> Result<bool, CurlCode> {
        let mut buf = buf;
        if req.header {
            let consumed = self.http_parse_headers(cb, conn, t, req, buf)?;
            buf = &buf[consumed..];
            if !req.header {
                // leftovers from a non-header (HTTP/0.9)
                let left = std::mem::take(&mut req.headerb);
                if !req.no_body && !left.is_empty() {
                    self.client_write(cb, req, CW_BODY, &left)?;
                }
            }
            if req.done {
                return Ok(true);
            }
        }
        if !req.header && (!buf.is_empty() || is_eos) {
            let flags = CW_BODY | if is_eos { CW_EOS } else { 0 };
            self.client_write(cb, req, flags, buf)?;
        }
        if is_eos {
            req.eos_written = true;
            req.download_done = true;
        }
        Ok(true)
    }

    /// http_parse_headers(): returns the bytes consumed
    fn http_parse_headers(
        &mut self,
        cb: &mut dyn Callbacks,
        conn: &mut Conn,
        t: &Target,
        req: &mut Req,
        buf: &[u8],
    ) -> Result<usize, CurlCode> {
        let mut consumed = 0;
        let mut buf = buf;
        while !buf.is_empty() && req.header {
            let Some(end) = buf.iter().position(|&c| c == b'\n') else {
                req.headerb.extend_from_slice(buf);
                consumed += buf.len();
                if req.headerline == 0 && checkprotoprefix(&req.headerb) == StatusLine::Bad {
                    req.header = false;
                    conn.close = true;
                    if req.httpversion >= 10 || conn.reused {
                        self.failf(cb, "Invalid status line");
                        return Err(code::WEIRD_SERVER_REPLY);
                    }
                    if !self.set.http09_allowed {
                        self.failf(cb, "Received HTTP/0.9 when not allowed");
                        return Err(code::UNSUPPORTED_PROTOCOL);
                    }
                    return Ok(consumed);
                }
                if req.headerb.len() > MAX_HTTP_RESP_HEADER_SIZE {
                    self.failf(
                        cb,
                        &format!(
                            "Too large response headers: {} > {}",
                            req.headerb.len(),
                            MAX_HTTP_RESP_HEADER_SIZE
                        ),
                    );
                    return Err(code::RECV_ERROR);
                }
                return Ok(consumed);
            };
            req.headerb.extend_from_slice(&buf[..=end]);
            buf = &buf[end + 1..];
            consumed += end + 1;
            if req.headerline == 0 && checkprotoprefix(&req.headerb) == StatusLine::Bad {
                conn.close = true;
                if req.httpversion >= 10 || conn.reused {
                    self.failf(cb, "Invalid status line");
                    return Err(code::WEIRD_SERVER_REPLY);
                }
                if !self.set.http09_allowed {
                    self.failf(cb, "Received HTTP/0.9 when not allowed");
                    return Err(code::UNSUPPORTED_PROTOCOL);
                }
                req.header = false;
                return Ok(consumed);
            }
            let line = std::mem::take(&mut req.headerb);
            self.http_rw_hd(cb, conn, t, req, &line)?;
            if req.done {
                return Ok(consumed);
            }
        }
        Ok(consumed)
    }

    /// http_rw_hd(): one complete header line
    fn http_rw_hd(
        &mut self,
        cb: &mut dyn Callbacks,
        conn: &mut Conn,
        t: &Target,
        req: &mut Req,
        hd: &[u8],
    ) -> Result<(), CurlCode> {
        if hd[0] == b'\n' || hd[0] == b'\r' {
            return self.http_on_response(cb, conn, t, req, hd);
        }
        let mut writetype = CW_HEADER;
        let first = req.headerline == 0;
        req.headerline += 1;
        if first {
            let mut fine = false;
            req.httpversion = 0;
            let mut p = hd;
            while let Some((&c, rest)) = p.split_first() {
                if c == b' ' || c == b'\t' {
                    p = rest;
                } else {
                    break;
                }
            }
            if p.starts_with(b"HTTP/") {
                let q = &p[5..];
                match q.first() {
                    Some(b'1') => {
                        if q.len() >= 4
                            && q[1] == b'.'
                            && (q[2] == b'0' || q[2] == b'1')
                            && (q[3] == b' ' || q[3] == b'\t')
                        {
                            req.httpversion = 10 + (q[2] - b'0');
                            let d = &q[4..];
                            if d.len() >= 3
                                && d[0].is_ascii_digit()
                                && d[1].is_ascii_digit()
                                && d[2].is_ascii_digit()
                            {
                                req.httpcode = ((d[0] - b'0') as i64) * 100
                                    + ((d[1] - b'0') as i64) * 10
                                    + (d[2] - b'0') as i64;
                                fine = true;
                            }
                        }
                        if !fine {
                            self.failf(cb, "Unsupported HTTP/1 subversion in response");
                            return Err(code::UNSUPPORTED_PROTOCOL);
                        }
                    }
                    Some(b'2') | Some(b'3') => {
                        if q.len() >= 2 && (q[1] == b' ' || q[1] == b'\t') {
                            req.httpversion = (q[0] - b'0') * 10;
                            let d = &q[2..];
                            if d.len() >= 4
                                && d[0].is_ascii_digit()
                                && d[1].is_ascii_digit()
                                && d[2].is_ascii_digit()
                                && (d[3] == b' ' || d[3] == b'\t')
                            {
                                req.httpcode = ((d[0] - b'0') as i64) * 100
                                    + ((d[1] - b'0') as i64) * 10
                                    + (d[2] - b'0') as i64;
                                fine = true;
                            }
                        }
                    }
                    _ => {
                        self.failf(cb, "Unsupported HTTP version in response");
                        return Err(code::UNSUPPORTED_PROTOCOL);
                    }
                }
            }
            if !fine && let Some(aliases) = self.set.slists.get(&opt::HTTP200ALIASES) {
                for a in aliases {
                    if hd.len() >= a.len() && hd[..a.len()].eq_ignore_ascii_case(a) {
                        fine = true;
                        req.httpcode = 200;
                        req.httpversion = 10;
                        break;
                    }
                }
            }
            if fine {
                self.http_statusline(cb, conn, req)?;
                writetype |= CW_STATUS;
            } else {
                req.header = false;
                return Err(code::WEIRD_SERVER_REPLY);
            }
        }
        // verify_header()
        if hd.contains(&0) {
            self.failf(cb, "Nul byte in header");
            return Err(code::WEIRD_SERVER_REPLY);
        }
        if req.headerline >= 2
            && !((hd[0] == b' ' || hd[0] == b'\t') && req.headerline > 2)
            && !hd.contains(&b':')
        {
            self.failf(cb, "Header without colon");
            return Err(code::WEIRD_SERVER_REPLY);
        }
        self.http_header(cb, conn, t, req, hd)?;
        self.tracer().debug(cb, info::HEADER_IN, hd);
        if req.httpcode / 100 == 1 {
            writetype |= CW_1XX;
        }
        self.client_write(cb, req, writetype, hd)?;
        self.bump_headersize(cb, req, hd.len())?;
        Ok(())
    }

    /// Curl_bump_headersize()
    fn bump_headersize(
        &mut self,
        cb: &mut dyn Callbacks,
        req: &mut Req,
        delta: usize,
    ) -> Result<(), CurlCode> {
        let max = MAX_HTTP_RESP_HEADER_SIZE as i64;
        self.info.header_size += delta as i64;
        req.allheadercount += delta as i64;
        req.headerbytecount += delta as i64;
        let mut bad = 0;
        let mut m = max;
        if req.allheadercount > max {
            bad = req.allheadercount;
        } else if self.info.header_size > max * 20 {
            bad = self.info.header_size;
            m = max * 20;
        }
        if bad > 0 {
            self.failf(cb, &format!("Too large response headers: {bad} > {m}"));
            return Err(code::RECV_ERROR);
        }
        Ok(())
    }

    /// http_statusline()
    fn http_statusline(
        &mut self,
        cb: &mut dyn Callbacks,
        conn: &mut Conn,
        req: &mut Req,
    ) -> Result<(), CurlCode> {
        match req.httpversion {
            10 | 11 => {
                if req.httpversion_sent != 0 && req.httpversion / 10 != req.httpversion_sent / 10 {
                    self.failf(
                        cb,
                        &format!(
                            "Version mismatch (from HTTP/{} to HTTP/{})",
                            req.httpversion_sent / 10,
                            req.httpversion / 10
                        ),
                    );
                    return Err(code::WEIRD_SERVER_REPLY);
                }
            }
            v => {
                self.failf(
                    cb,
                    &format!(
                        "Unsupported HTTP version ({}.{}) in response",
                        v / 10,
                        v % 10
                    ),
                );
                return Err(code::UNSUPPORTED_PROTOCOL);
            }
        }
        self.info.httpcode = req.httpcode;
        self.info.httpversion = req.httpversion;
        conn.httpversion_seen = req.httpversion;
        if self.st.rcvd_min == 0 || self.st.rcvd_min > req.httpversion {
            self.st.rcvd_min = req.httpversion;
        }
        if self.st.resume_from > 0 && self.st.httpreq == HttpReq::Get && req.httpcode == 416 {
            req.ignorebody = true;
        }
        if req.httpversion == 10 {
            self.infof(cb, "HTTP 1.0, assume close after body");
            conn.close = true;
        }
        req.http_bodyless = (100..200).contains(&req.httpcode);
        match req.httpcode {
            304 | 204 => {
                if req.httpcode == 304 && self.set.timecondition != 0 {
                    self.info.timecond = true;
                }
                req.size = 0;
                req.maxdownload = 0;
                req.http_bodyless = true;
            }
            _ => {}
        }
        Ok(())
    }

    /// http_header(): act on one response header
    fn http_header(
        &mut self,
        cb: &mut dyn Callbacks,
        conn: &mut Conn,
        t: &Target,
        req: &mut Req,
        hd: &[u8],
    ) -> Result<(), CurlCode> {
        let c = hd[0].to_ascii_lowercase();
        match c {
            b'c' => {
                if !req.http_bodyless
                    && !self.set.ignore_cl
                    && let Some(v) = header_val(hd, b"Content-Length")
                {
                    let s = lossy(v);
                    let s = s
                        .trim_end_matches(['\r', '\n'])
                        .trim_end_matches([' ', '\t']);
                    match s.parse::<i64>() {
                        Ok(n) if n >= 0 && s.bytes().all(|b| b.is_ascii_digit()) => {
                            req.size = n;
                            req.maxdownload = n;
                        }
                        _ => {
                            if !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) {
                                // overflow
                                if self.set.max_filesize > 0 {
                                    self.failf(cb, "Maximum file size exceeded");
                                    return Err(code::FILESIZE_EXCEEDED);
                                }
                                conn.close = true;
                                self.infof(cb, "Overflow Content-Length: value");
                            } else {
                                self.failf(cb, "Invalid Content-Length: value");
                                return Err(code::WEIRD_SERVER_REPLY);
                            }
                        }
                    }
                    return Ok(());
                }
                if !req.http_bodyless
                    && self.set.strs.contains_key(&opt::ACCEPT_ENCODING)
                    && let Some(v) = header_val(hd, b"Content-Encoding")
                {
                    return self.build_unencoding_stack(cb, req, v, false);
                }
                if header_is(hd, b"Content-Type") {
                    let ct = copy_header_value(hd);
                    if !ct.is_empty() {
                        self.info.contenttype = Some(ct);
                    }
                    return Ok(());
                }
                if hd.len() > 16 && compareheader(hd, b"Connection", b"close") {
                    conn.close = true;
                    return Ok(());
                }
                if req.httpversion == 10
                    && hd.len() > 21
                    && compareheader(hd, b"Connection", b"keep-alive")
                {
                    conn.close = false;
                    self.infof(cb, "HTTP/1.0 connection set to keep alive");
                    return Ok(());
                }
                if !req.http_bodyless
                    && let Some(v) = header_val(hd, b"Content-Range")
                {
                    let mut i = 0;
                    while i < v.len() && !v[i].is_ascii_digit() && v[i] != b'*' {
                        i += 1;
                    }
                    if i < v.len() && v[i].is_ascii_digit() {
                        let mut n: i64 = 0;
                        while i < v.len() && v[i].is_ascii_digit() {
                            n = n.saturating_mul(10).saturating_add((v[i] - b'0') as i64);
                            i += 1;
                        }
                        req.offset = n;
                        if self.st.resume_from == n {
                            req.content_range = true;
                        }
                    } else if req.httpcode < 300 {
                        self.st.resume_from = 0;
                    }
                }
            }
            b'l' => {
                if !req.http_bodyless
                    && (self.set.timecondition != 0 || self.set.get_filetime)
                    && let Some(v) = header_val(hd, b"Last-Modified")
                {
                    let now = now_secs();
                    req.timeofdoc = super::cookie::getdate_capped(trim_crlf(v), now);
                    if self.set.get_filetime {
                        self.info.filetime = req.timeofdoc;
                    }
                    return Ok(());
                }
                if (300..400).contains(&req.httpcode)
                    && header_is(hd, b"Location")
                    && req.location.is_none()
                {
                    let loc = copy_header_value(hd);
                    if !loc.is_empty() {
                        req.location = Some(loc.clone());
                        if self.set.follow_mode != 0 {
                            req.newurl = Some(loc);
                            self.perhapsrewind(cb, conn, req);
                            self.st.this_is_a_follow = true;
                        }
                    }
                }
            }
            b'p' => {
                if header_is(hd, b"Proxy-Connection") {
                    if req.httpversion == 10
                        && t.proxy.is_some()
                        && compareheader(hd, b"Proxy-Connection", b"keep-alive")
                    {
                        conn.close = false;
                        self.infof(cb, "HTTP/1.0 proxy connection set to keep alive");
                    } else if req.httpversion == 11
                        && t.proxy.is_some()
                        && compareheader(hd, b"Proxy-Connection", b"close")
                    {
                        conn.close = true;
                        self.infof(cb, "HTTP/1.1 proxy connection set close");
                    }
                    return Ok(());
                }
                if req.httpcode == 407 && header_is(hd, b"Proxy-authenticate") {
                    let v = copy_header_value(hd);
                    self.input_auth(cb, true, &v);
                    return Ok(());
                }
            }
            b'r' => {
                if let Some(v) = header_val(hd, b"Retry-After") {
                    let v = trim_crlf(v);
                    let now = now_secs();
                    let date = super::cookie::getdate_capped(v, now);
                    let mut ra: i64 = 0;
                    if date != -1 {
                        if date >= now {
                            ra = date - now;
                        }
                    } else {
                        for &b in v {
                            if b.is_ascii_digit() {
                                ra = ra.saturating_mul(10).saturating_add((b - b'0') as i64);
                            } else {
                                break;
                            }
                        }
                    }
                    self.info.retry_after = ra.min(21600);
                    return Ok(());
                }
            }
            b's' => {
                if self.cookie_engine
                    && let Some(jar) = self.cookies.clone()
                    && header_is(hd, b"Set-Cookie")
                {
                    let host = self.st.cookiehost.clone().unwrap_or_else(|| t.host.clone());
                    let secure = super::cookie::secure_context(
                        t.protocol & (proto::HTTPS | (1i64 << 31)) != 0,
                        host.as_bytes(),
                    );
                    let mut log = Vec::new();
                    jar.borrow_mut().add(
                        &mut self.setcookies,
                        true,
                        false,
                        &hd[11..],
                        Some(host.as_bytes()),
                        Some(&t.tu.path),
                        secure,
                        &mut log,
                    );
                    let tr = self.tracer();
                    for l in log {
                        tr.text(cb, &l);
                    }
                    return Ok(());
                }
            }
            b't' => {
                if !req.http_bodyless
                    && self.st.httpreq != HttpReq::Head
                    && req.httpcode != 304
                    && let Some(v) = header_val(hd, b"Transfer-Encoding")
                {
                    self.build_unencoding_stack(cb, req, v, true)?;
                    if !req.chunk && self.set.http_transfer_encoding {
                        conn.close = true;
                        req.ignore_cl = true;
                    }
                    return Ok(());
                }
                if header_is(hd, b"Trailer") {
                    req.resp_trailer = true;
                    return Ok(());
                }
            }
            b'w' => {
                if req.httpcode == 401 && header_is(hd, b"WWW-Authenticate") {
                    let v = copy_header_value(hd);
                    self.input_auth(cb, false, &v);
                    return Ok(());
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// Curl_build_unencoding_stack()
    fn build_unencoding_stack(
        &mut self,
        cb: &mut dyn Callbacks,
        req: &mut Req,
        v: &[u8],
        is_transfer: bool,
    ) -> Result<(), CurlCode> {
        let v = trim_crlf(v);
        let mut has_chunked = false;
        for raw in v.split(|&c| c == b',') {
            let mut name = raw;
            while let Some((&c, rest)) = name.split_first() {
                if c == b' ' || c == b'\t' {
                    name = rest;
                } else {
                    break;
                }
            }
            let end = name
                .iter()
                .rposition(|&c| c > b' ')
                .map(|i| i + 1)
                .unwrap_or(0);
            let name = &name[..end];
            if name.is_empty() {
                continue;
            }
            let is_chunked = is_transfer && name.eq_ignore_ascii_case(b"chunked");
            if (is_transfer && !self.set.http_transfer_encoding && !is_chunked)
                || (!is_transfer && self.set.http_ce_skip)
            {
                let is_identity = name.len() >= 8 && name[..8].eq_ignore_ascii_case(b"identity");
                if is_transfer && !self.set.http_te_skip {
                    if has_chunked {
                        self.failf(
                            cb,
                            &format!(
                                "A Transfer-Encoding ({}) was listed after chunked",
                                lossy(name)
                            ),
                        );
                    } else if is_identity {
                        continue;
                    } else {
                        self.failf(
                            cb,
                            &format!("Unsolicited Transfer-Encoding ({}) found", lossy(name)),
                        );
                    }
                    return Err(code::BAD_CONTENT_ENCODING);
                }
                return Ok(());
            }
            let count = if is_transfer {
                req.chunker.is_some() as usize
            } else {
                req.cdecoders.len()
            };
            if count + 1 >= 5 {
                self.failf(cb, "Reject response due to more than 5 content encodings");
                return Err(code::BAD_CONTENT_ENCODING);
            }
            if is_chunked {
                if req.chunker.is_some() {
                    return Ok(());
                }
                req.chunker = Some(Chunker::new());
                req.chunk = true;
                has_chunked = true;
                continue;
            }
            if is_transfer && req.chunker.is_some() {
                self.failf(
                    cb,
                    "Reject response due to 'chunked' not being the last Transfer-Encoding",
                );
                return Err(code::BAD_CONTENT_ENCODING);
            }
            // general decoders apply to the content in both phases
            req.cdecoders.push(ContentDecoder::by_name(name));
        }
        Ok(())
    }

    /// http_on_response(): the end of a header block
    fn http_on_response(
        &mut self,
        cb: &mut dyn Callbacks,
        conn: &mut Conn,
        t: &Target,
        req: &mut Req,
        last_hd: &[u8],
    ) -> Result<(), CurlCode> {
        let mut last = Some(last_hd.to_vec());
        let r = self.http_on_response_inner(cb, conn, t, req, &mut last);
        if let Some(l) = last {
            let r2 = self.http_write_header(cb, req, &l);
            r?;
            return r2;
        }
        r
    }

    fn http_write_header(
        &mut self,
        cb: &mut dyn Callbacks,
        req: &mut Req,
        hd: &[u8],
    ) -> Result<(), CurlCode> {
        self.tracer().debug(cb, info::HEADER_IN, hd);
        let wt = CW_HEADER | if req.httpcode / 100 == 1 { CW_1XX } else { 0 };
        self.client_write(cb, req, wt, hd)?;
        self.bump_headersize(cb, req, hd.len())?;
        req.deductheadercount = if (100..=199).contains(&req.httpcode) {
            req.headerbytecount
        } else {
            0
        };
        Ok(())
    }

    fn http_on_response_inner(
        &mut self,
        cb: &mut dyn Callbacks,
        conn: &mut Conn,
        t: &Target,
        req: &mut Req,
        last: &mut Option<Vec<u8>>,
    ) -> Result<(), CurlCode> {
        if req.httpcode < 200
            && let Some(l) = last.take()
        {
            self.http_write_header(cb, req, &l)?;
        }
        if req.httpcode < 100 {
            self.failf(cb, "Unsupported response code in HTTP response");
            return Err(code::UNSUPPORTED_PROTOCOL);
        } else if req.httpcode < 200 {
            req.header = true;
            req.headerline = 0;
            match req.httpcode {
                100 => {
                    if req.exp100 != Exp100::None && req.exp100 != Exp100::SendData {
                        req.exp100 = Exp100::SendData;
                        req.keep_send = true;
                    }
                }
                101 => {
                    if req.httpversion_sent != 11 {
                        self.failf(cb, "unexpected 101 response code");
                        return Err(code::WEIRD_SERVER_REPLY);
                    }
                    req.header = false;
                }
                _ => {}
            }
            return Ok(());
        }
        req.header = false;
        if req.size == -1
            && !req.chunk
            && !conn.close
            && req.httpversion == 11
            && self.st.httpreq != HttpReq::Head
        {
            self.infof(
                cb,
                "no chunk, no close, no size. Assume close to signal end",
            );
            conn.close = true;
        }
        if self.http_should_fail(t, req.httpcode) {
            self.failf(
                cb,
                &format!("The requested URL returned error: {}", req.httpcode),
            );
            return Err(code::HTTP_RETURNED_ERROR);
        }
        self.auth_act(cb, conn, t, req)?;
        if req.httpcode >= 300 {
            let will_rewind = self.st.rewind_read;
            if !req.authneg && !conn.close && !will_rewind {
                match self.st.httpreq {
                    HttpReq::Put | HttpReq::Post | HttpReq::PostForm | HttpReq::PostMime => {
                        if !(req.upload_done && req.sendbuf.is_empty()) {
                            if req.httpcode == 417 && req.exp100 != Exp100::None {
                                if req.writebytecount == 0
                                    && matches!(req.exp100, Exp100::Awaiting(_))
                                {
                                    self.infof(cb, "Got HTTP failure 417 while waiting for a 100");
                                } else {
                                    self.infof(cb, "Got HTTP failure 417 while sending data");
                                    conn.close = true;
                                    self.perhapsrewind(cb, conn, req);
                                }
                                self.st.disableexpect = true;
                                req.newurl = self.st.url.clone();
                                self.req_abort_sending(cb, req);
                            } else if self.set.keep_sending_on_error {
                                self.infof(cb, "HTTP error before end of send, keep sending");
                                if req.exp100 != Exp100::None {
                                    req.exp100 = Exp100::SendData;
                                }
                            } else {
                                self.infof(cb, "HTTP error before end of send, stop sending");
                                conn.close = true;
                                self.req_abort_sending(cb, req);
                            }
                        }
                    }
                    _ => {}
                }
            }
            if will_rewind && !(req.upload_done && req.sendbuf.is_empty()) {
                self.infof(cb, "Keep sending data to get tossed away");
                req.keep_send = true;
            }
        }
        if req.no_body {
            req.download_done = true;
        }
        if req.maxdownload == 0 {
            req.download_done = true;
        }
        // http_firstwrite()
        if req.newurl.is_some() {
            if conn.close {
                req.keep_recv = false;
                req.done = true;
                return Ok(());
            }
            req.ignorebody = true;
            self.infof(cb, "Ignoring the response-body");
        }
        if self.st.resume_from > 0
            && !req.content_range
            && self.st.httpreq == HttpReq::Get
            && !req.ignorebody
        {
            if req.size == self.st.resume_from {
                self.infof(cb, "The entire document is already downloaded");
                conn.close = true;
                req.keep_recv = false;
                req.done = true;
                return Ok(());
            }
            self.failf(
                cb,
                "HTTP server does not seem to support byte ranges. Cannot resume.",
            );
            return Err(code::RANGE_ERROR);
        }
        if self.set.timecondition != 0
            && !self.set.strs.contains_key(&opt::RANGE)
            && !self.meets_timecondition(cb, req.timeofdoc)
        {
            req.done = true;
            self.info.httpcode = 304;
            self.infof(cb, "Simulate an HTTP 304 response");
            conn.close = true;
            return Ok(());
        }
        // http_size()
        if req.ignore_cl || req.chunk {
            req.size = -1;
            req.maxdownload = -1;
        } else if req.size != -1 {
            if self.set.max_filesize > 0 && !req.ignorebody && req.size > self.set.max_filesize {
                self.failf(cb, "Maximum file size exceeded");
                return Err(code::FILESIZE_EXCEEDED);
            }
            if req.ignorebody {
                self.infof(cb, "setting size while ignoring");
            }
            self.progress.set_download_size(req.size);
            req.maxdownload = req.size;
        }
        Ok(())
    }

    /// Curl_meets_timecondition()
    pub(crate) fn meets_timecondition(&mut self, cb: &mut dyn Callbacks, timeofdoc: i64) -> bool {
        if timeofdoc == 0 || self.set.timevalue == 0 {
            return true;
        }
        match self.set.timecondition {
            2 => {
                if timeofdoc > self.set.timevalue {
                    self.infof(cb, "The requested document is not old enough");
                    self.info.timecond = true;
                    return false;
                }
            }
            _ => {
                if timeofdoc <= self.set.timevalue {
                    self.infof(cb, "The requested document is not new enough");
                    self.info.timecond = true;
                    return false;
                }
            }
        }
        true
    }
}

/// Response header state needed by Curl_add_custom_headers()
pub(crate) struct HdrState {
    has_host: bool,
    httpreq: HttpReq,
    authneg: bool,
    te: bool,
}

#[derive(Default)]
pub(crate) struct AuthHeaders {
    pub host: Vec<u8>,
    pub proxy: Vec<u8>,
}

/// One CURLOPT_HTTPHEADER entry as Curl_add_custom_headers() emits it
fn custom_header_line(
    h: &[u8],
    st: &HdrState,
    httpversion: u8,
    auth_allowed: bool,
) -> Option<Vec<u8>> {
    let h = super::easy::cstr(h);
    // "Name;" sends an empty header
    let mut blank = false;
    let name: &[u8];
    if let Some(semi) = h.iter().position(|&c| c == b';')
        && semi + 1 == h.len()
        && !h[..semi].contains(&b':')
    {
        blank = true;
        name = &h[..semi];
    } else {
        let colon = h.iter().position(|&c| c == b':')?;
        name = &h[..colon];
        let mut v = &h[colon + 1..];
        let end = v
            .iter()
            .position(|&c| c == b'\r' || c == b'\n')
            .unwrap_or(v.len());
        v = &v[..end];
        if v.iter().all(|&c| c == b' ' || c == b'\t') {
            return None;
        }
    }
    let is = |n: &[u8]| name.eq_ignore_ascii_case(n);
    if st.has_host && is(b"Host") {
        return None;
    }
    if (st.httpreq == HttpReq::PostForm || st.httpreq == HttpReq::PostMime) && is(b"Content-Type") {
        return None;
    }
    if st.authneg && is(b"Content-Length") {
        return None;
    }
    if st.te && is(b"Connection") {
        return None;
    }
    if httpversion >= 20 && is(b"Transfer-Encoding") {
        return None;
    }
    if (is(b"Authorization") || is(b"Cookie")) && !auth_allowed {
        return None;
    }
    let mut out = Vec::new();
    if blank {
        out.extend_from_slice(name);
        out.extend_from_slice(b":\r\n");
    } else {
        out.extend_from_slice(h);
        out.extend_from_slice(b"\r\n");
    }
    Some(out)
}

/// authcmp(): the auth scheme word, not followed by an alnum
fn authcmp(word: &[u8], line: &[u8]) -> bool {
    line.len() >= word.len()
        && line[..word.len()].eq_ignore_ascii_case(word)
        && !line
            .get(word.len())
            .is_some_and(|c| c.is_ascii_alphanumeric())
}

/// time(NULL)
pub(crate) fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn trim_crlf(v: &[u8]) -> &[u8] {
    let mut v = v;
    while let Some((&c, rest)) = v.split_last() {
        if c == b'\r' || c == b'\n' || c == b' ' || c == b'\t' {
            v = rest;
        } else {
            break;
        }
    }
    v
}

#[derive(PartialEq, Eq)]
enum StatusLine {
    Unknown,
    Done,
    Bad,
}

/// checkhttpprefix() for HTTP
fn checkprotoprefix(s: &[u8]) -> StatusLine {
    let prefix = b"HTTP/";
    let n = prefix.len().min(s.len());
    if s[..n].eq_ignore_ascii_case(&prefix[..n]) {
        if s.len() >= prefix.len() {
            StatusLine::Done
        } else {
            StatusLine::Unknown
        }
    } else {
        StatusLine::Bad
    }
}
