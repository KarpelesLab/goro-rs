//! The easy handle: curl_easy_init/duphandle/reset/setopt/pause/upkeep
//! (lib/easy.c, lib/setopt.c)

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Instant;

use super::consts::{OptKind, code, httpver, lock, opt, sslver};
use super::cookie::CookieInfo;
use super::http::AuthState;
use super::options::*;
use super::share::Share;
use super::trace::Tracer;
use super::{Callbacks, ConnPool, CurlCode, OptVal};

/// struct PureInfo + the timing part of struct Progress
#[derive(Clone, Default)]
pub(crate) struct Info {
    pub httpcode: i64,
    pub httpproxycode: i64,
    pub httpversion: u8,
    pub filetime: i64,
    pub timecond: bool,
    pub header_size: i64,
    pub request_size: i64,
    pub proxyauthavail: u64,
    pub httpauthavail: u64,
    pub proxyauthpicked: u64,
    pub httpauthpicked: u64,
    pub numconnects: i64,
    pub contenttype: Option<Vec<u8>>,
    pub wouldredirect: Option<Vec<u8>>,
    pub primary_ip: String,
    pub primary_port: i64,
    pub local_ip: String,
    pub local_port: i64,
    pub retry_after: i64,
    pub conn_scheme: Option<String>,
    pub conn_protocol: i64,
    pub conn_remote_port: i64,
    pub used_proxy: bool,
    pub ssl_verifyresult: i64,
    pub proxy_ssl_verifyresult: i64,
    pub certinfo: Vec<Vec<Vec<u8>>>,
    pub os_errno: i64,
}

impl Info {
    /// Curl_initinfo()
    pub fn init(&mut self) {
        let certinfo = std::mem::take(&mut self.certinfo);
        drop(certinfo);
        *self = Info {
            filetime: -1,
            primary_port: -1,
            local_port: -1,
            ssl_verifyresult: self.ssl_verifyresult,
            proxy_ssl_verifyresult: self.proxy_ssl_verifyresult,
            os_errno: self.os_errno,
            used_proxy: self.used_proxy,
            ..Default::default()
        };
    }
}

/// struct Progress
#[derive(Clone)]
pub(crate) struct Progress {
    pub start: Instant,
    pub t_startsingle: Instant,
    pub t_startop: Instant,
    pub t_startqueue: Instant,
    pub t_nslookup: i64,
    pub t_connect: i64,
    pub t_appconnect: i64,
    pub t_pretransfer: i64,
    pub t_posttransfer: i64,
    pub t_starttransfer: i64,
    pub t_redirect: i64,
    pub t_postqueue: i64,
    pub timespent: i64,
    pub is_t_startransfer_set: bool,
    pub dl_total: i64,
    pub dl_cur: i64,
    pub ul_total: i64,
    pub ul_cur: i64,
    pub dl_size_known: bool,
    pub ul_size_known: bool,
    pub dl_speed: i64,
    pub ul_speed: i64,
    pub last_cb: Option<Instant>,
}

impl Default for Progress {
    fn default() -> Self {
        let now = Instant::now();
        Progress {
            start: now,
            t_startsingle: now,
            t_startop: now,
            t_startqueue: now,
            t_nslookup: 0,
            t_connect: 0,
            t_appconnect: 0,
            t_pretransfer: 0,
            t_posttransfer: 0,
            t_starttransfer: 0,
            t_redirect: 0,
            t_postqueue: 0,
            timespent: 0,
            is_t_startransfer_set: false,
            dl_total: 0,
            dl_cur: 0,
            ul_total: 0,
            ul_cur: 0,
            dl_size_known: false,
            ul_size_known: false,
            dl_speed: 0,
            ul_speed: 0,
            last_cb: None,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Timer {
    StartOp,
    StartSingle,
    PostQueue,
    NameLookup,
    Connect,
    AppConnect,
    PreTransfer,
    StartTransfer,
    PostTransfer,
    Redirect,
}

impl Progress {
    fn us_since(a: Instant, b: Instant) -> i64 {
        b.saturating_duration_since(a).as_micros() as i64
    }

    /// Curl_pgrsTime()
    pub fn time(&mut self, timer: Timer) -> Instant {
        let now = Instant::now();
        let delta = match timer {
            Timer::StartOp => {
                self.t_startop = now;
                self.t_startqueue = now;
                self.t_postqueue = 0;
                None
            }
            Timer::StartSingle => {
                self.t_startsingle = now;
                self.is_t_startransfer_set = false;
                None
            }
            Timer::PostQueue => {
                self.t_postqueue += Self::us_since(self.t_startqueue, now);
                None
            }
            Timer::NameLookup => Some(&mut self.t_nslookup),
            Timer::Connect => Some(&mut self.t_connect),
            Timer::AppConnect => Some(&mut self.t_appconnect),
            Timer::PreTransfer => Some(&mut self.t_pretransfer),
            Timer::StartTransfer => {
                if self.is_t_startransfer_set {
                    return now;
                }
                self.is_t_startransfer_set = true;
                Some(&mut self.t_starttransfer)
            }
            Timer::PostTransfer => Some(&mut self.t_posttransfer),
            Timer::Redirect => {
                self.t_redirect = Self::us_since(self.start, now);
                self.t_startqueue = now;
                None
            }
        };
        if let Some(d) = delta {
            let us = Self::us_since(self.t_startsingle, now).max(1);
            *d += us;
        }
        now
    }

    /// Curl_pgrsStartNow()
    pub fn start_now(&mut self) {
        self.start = Instant::now();
        self.is_t_startransfer_set = false;
        self.dl_cur = 0;
        self.ul_cur = 0;
        self.dl_size_known = false;
        self.ul_size_known = false;
    }

    pub fn set_download_size(&mut self, size: i64) {
        if size >= 0 {
            self.dl_total = size;
            self.dl_size_known = true;
        } else {
            self.dl_total = 0;
            self.dl_size_known = false;
        }
    }

    pub fn set_upload_size(&mut self, size: i64) {
        if size >= 0 {
            self.ul_total = size;
            self.ul_size_known = true;
        } else {
            self.ul_total = 0;
            self.ul_size_known = false;
        }
    }

    /// progress_calc(): time spent and average speeds
    pub fn calc(&mut self) {
        let now = Instant::now();
        self.timespent = Self::us_since(self.start, now);
        let secs = self.timespent as f64 / 1_000_000.0;
        let us = self.timespent.max(1);
        self.dl_speed = ((self.dl_cur as f64) / (us as f64 / 1_000_000.0)) as i64;
        self.ul_speed = ((self.ul_cur as f64) / (us as f64 / 1_000_000.0)) as i64;
        let _ = secs;
    }
}

/// The per-handle state that survives between requests (struct UrlState)
#[derive(Clone)]
pub(crate) struct State {
    /// data->state.url (the effective URL)
    pub url: Option<Vec<u8>>,
    pub referer: Option<Vec<u8>>,
    pub httpreq: HttpReq,
    pub followlocation: i64,
    pub requests: i64,
    pub this_is_a_follow: bool,
    pub allow_port: bool,
    pub first_host: Option<String>,
    pub first_remote_port: u16,
    pub first_remote_protocol: i64,
    pub authhost: AuthState,
    pub authproxy: AuthState,
    pub authproblem: bool,
    pub http_ignorecustom: bool,
    pub disableexpect: bool,
    pub infilesize: i64,
    pub user: Option<Vec<u8>>,
    pub passwd: Option<Vec<u8>>,
    pub proxyuser: Option<Vec<u8>>,
    pub proxypasswd: Option<Vec<u8>>,
    pub creds_from_url: bool,
    pub uagent: Option<Vec<u8>>,
    pub cookiehost: Option<String>,
    pub retrycount: i32,
    pub rcvd_min: u8,
    pub resume_from: i64,
    pub digest: super::http::DigestData,
    pub proxydigest: super::http::DigestData,
    pub recent_conn_id: i64,
    pub lastconnect_id: i64,
    pub upload: bool,
    pub rewind_read: bool,
}

impl Default for State {
    fn default() -> Self {
        State {
            url: None,
            referer: None,
            httpreq: HttpReq::Get,
            followlocation: 0,
            requests: 0,
            this_is_a_follow: false,
            allow_port: true,
            first_host: None,
            first_remote_port: 0,
            first_remote_protocol: 0,
            authhost: AuthState::default(),
            authproxy: AuthState::default(),
            authproblem: false,
            http_ignorecustom: false,
            disableexpect: false,
            infilesize: 0,
            user: None,
            passwd: None,
            proxyuser: None,
            proxypasswd: None,
            creds_from_url: false,
            uagent: None,
            cookiehost: None,
            retrycount: 0,
            rcvd_min: 0,
            resume_from: 0,
            digest: Default::default(),
            proxydigest: Default::default(),
            recent_conn_id: -1,
            lastconnect_id: -1,
            upload: false,
            rewind_read: false,
        }
    }
}

/// struct Curl_easy
pub struct Easy {
    pub(crate) set: Set,
    pub(crate) st: State,
    pub(crate) info: Info,
    pub(crate) progress: Progress,
    pub(crate) share: Option<Rc<RefCell<Share>>>,
    pub(crate) own_pool: ConnPool,
    pub(crate) ext_pool: Option<ConnPool>,
    /// data->cookies
    pub(crate) cookies: Option<Rc<RefCell<CookieInfo>>>,
    /// data->state.cookie_engine
    pub(crate) cookie_engine: bool,
    /// data->state.cookielist: cookie files to load
    pub(crate) cookie_files: Vec<Vec<u8>>,
    pub(crate) errbuf: Vec<u8>,
    pub(crate) errbuf_set: bool,
    /// data->id (CURLINFO_XFER_ID)
    pub(crate) id: i64,
    pub(crate) in_perform: bool,
    pub(crate) pause_state: i64,
    /// the Content-Type header line of a mime post being sent
    pub(crate) mime_ct: Option<Vec<u8>>,
    /// NTLM challenges seen in the current response (proxy?, value)
    pub(crate) pending_ntlm: Vec<(bool, Vec<u8>)>,
    pub(crate) low_speed_since: Option<Instant>,
    /// data->req.setcookies
    pub(crate) setcookies: u8,
    /// trace events of setopt() calls, replayed by the next perform()
    pub(crate) backlog: Rc<RefCell<Vec<super::TraceEvent>>>,
}

impl Default for Easy {
    fn default() -> Self {
        Easy::new()
    }
}

impl Drop for Easy {
    fn drop(&mut self) {
        if let Some(sh) = self.share.take() {
            let mut s = sh.borrow_mut();
            s.dirty = s.dirty.saturating_sub(1);
        }
    }
}

impl Easy {
    /// curl_easy_init()
    pub fn new() -> Easy {
        let mut info = Info::default();
        info.init();
        Easy {
            set: Set::default(),
            st: State::default(),
            info,
            progress: Progress::default(),
            share: None,
            own_pool: ConnPool::new(),
            ext_pool: None,
            cookies: None,
            cookie_engine: false,
            cookie_files: Vec::new(),
            errbuf: Vec::new(),
            errbuf_set: false,
            id: -1,
            in_perform: false,
            pause_state: 0,
            mime_ct: None,
            pending_ntlm: Vec::new(),
            low_speed_since: None,
            setcookies: 0,
            backlog: Rc::default(),
        }
    }

    /// curl_easy_duphandle(): options copied, no transfer state
    pub fn duphandle(&self) -> Easy {
        let mut e = Easy::new();
        e.set = self.set.clone();
        if let Some(sh) = &self.share {
            sh.borrow_mut().dirty += 1;
            e.share = Some(sh.clone());
            if let Some(c) = &sh.borrow().cookies {
                e.cookies = Some(c.clone());
            }
        }
        if self.cookies.is_some() && self.cookie_engine {
            let mut sink = self.backlog_sink();
            e.cookie_init(&mut sink, self.set.cookiesession);
        }
        e.cookie_files = self.cookie_files.clone();
        e.st.url = self.st.url.clone();
        e.st.referer = self.st.referer.clone();
        e
    }

    /// curl_easy_reset()
    pub fn reset(&mut self) {
        let share = self.share.take();
        if let Some(sh) = &share {
            let mut s = sh.borrow_mut();
            s.dirty = s.dirty.saturating_sub(1);
        }
        let pool = self.own_pool.clone();
        let ext = self.ext_pool.take();
        let cookies = self.cookies.take();
        let cookie_engine = self.cookie_engine;
        let id = self.id;
        let recent = self.st.recent_conn_id;
        let last = self.st.lastconnect_id;
        *self = Easy::new();
        self.own_pool = pool;
        self.ext_pool = ext;
        // the cookie jar is not part of the user settings
        self.cookies = cookies;
        self.cookie_engine = cookie_engine;
        self.id = id;
        self.st.recent_conn_id = recent;
        self.st.lastconnect_id = last;
    }

    /// Attach a multi handle's connection pool (curl_multi_add_handle), or
    /// go back to the private one with None (curl_multi_remove_handle).
    pub fn set_conn_pool(&mut self, pool: Option<ConnPool>) {
        self.ext_pool = pool;
    }

    /// The connection pool transfers use (share CONNECT > multi > own)
    pub(crate) fn conn_pool(&self) -> ConnPool {
        if let Some(sh) = &self.share {
            let s = sh.borrow();
            if s.shares(lock::CONNECT)
                && let Some(p) = &s.cpool
            {
                return p.clone();
            }
        }
        self.multi_pool()
    }

    /// The "multi" of this handle (for transfer ids and the DNS cache)
    pub(crate) fn multi_pool(&self) -> ConnPool {
        match &self.ext_pool {
            Some(p) => p.clone(),
            None => self.own_pool.clone(),
        }
    }

    pub(crate) fn backlog_sink(&self) -> super::NullCallbacks {
        super::NullCallbacks(self.backlog.clone())
    }

    /// Hand the trace of setopt() calls to the transfer callbacks
    pub(crate) fn replay_backlog(&self, cb: &mut dyn Callbacks) {
        let ev = std::mem::take(&mut *self.backlog.borrow_mut());
        for e in ev {
            match e {
                super::TraceEvent::Debug(k, d) => cb.debug(k, &d),
                super::TraceEvent::Stderr(d) => cb.stderr(&d),
            }
        }
    }

    pub(crate) fn tracer(&self) -> Tracer {
        Tracer {
            verbose: self.set.verbose,
            debug_cb: self.set.callbacks.contains(&opt::DEBUGFUNCTION),
        }
    }

    /// failf(): first error of a transfer goes to the error buffer, all of
    /// them to the verbose trace
    pub(crate) fn failf(&mut self, cb: &mut dyn Callbacks, msg: &str) {
        let mut m = msg.as_bytes().to_vec();
        if m.len() > 255 {
            m.truncate(255);
        }
        if !self.errbuf_set {
            self.errbuf = m.clone();
            self.errbuf_set = true;
        }
        let tr = self.tracer();
        if tr.verbose {
            m.push(b'\n');
            tr.debug(cb, super::consts::info::TEXT, &m);
        }
    }

    /// infof()
    pub(crate) fn infof(&self, cb: &mut dyn Callbacks, msg: &str) {
        self.tracer().infof(cb, msg);
    }

    /// What libcurl wrote into CURLOPT_ERRORBUFFER ("" when none)
    pub fn error_buffer(&self) -> &[u8] {
        &self.errbuf
    }

    /// curl_easy_pause()
    pub fn pause(&mut self, bitmask: i64) -> CurlCode {
        if !self.in_perform {
            return code::BAD_FUNCTION_ARGUMENT;
        }
        self.pause_state = bitmask;
        code::OK
    }

    /// Curl_getconnectinfo(): the socket of the last connection if it is
    /// still alive in the pool
    pub(crate) fn last_socket(&self) -> i64 {
        let id = self.st.lastconnect_id;
        if id < 0 {
            return -1;
        }
        let pool = self.conn_pool();
        let p = pool.0.borrow();
        match p.idle.iter().find(|c| c.id == id) {
            Some(c) => c.fd() as i64,
            None => -1,
        }
    }

    /// curl_easy_upkeep()
    pub fn upkeep(&mut self) -> CurlCode {
        code::OK
    }

    /// Write the cookie jar (CURLOPT_COOKIEJAR), like curl_easy_cleanup()
    /// does (Curl_flush_cookies(data, TRUE)).
    pub fn flush_cookies(&mut self) {
        let mut sink = self.backlog_sink();
        self.flush_cookies_cb(&mut sink, true);
    }

    pub(crate) fn flush_cookies_cb(&mut self, cb: &mut dyn Callbacks, _cleanup: bool) {
        let Some(jar) = self.cookies.clone() else {
            return;
        };
        let file = self.set.strs.get(&opt::COOKIEJAR).cloned();
        let mut log = Vec::new();
        super::cookie::flush_cookies(file.as_deref(), Some(&mut jar.borrow_mut()), &mut log);
        let tr = self.tracer();
        for l in log {
            tr.text(cb, &l);
        }
    }

    /// A cookie jar for this handle (Curl_cookie_init(data, NULL, data->cookies, ns))
    pub(crate) fn cookie_init(&mut self, cb: &mut dyn Callbacks, newsession: bool) {
        let jar = self
            .cookies
            .get_or_insert_with(|| Rc::new(RefCell::new(CookieInfo::new())))
            .clone();
        let mut log = Vec::new();
        jar.borrow_mut()
            .init(None, newsession, &mut self.setcookies, &mut log);
        self.cookie_engine = true;
        let tr = self.tracer();
        for l in log {
            tr.text(cb, &l);
        }
    }

    /// Curl_cookie_loadfiles()
    pub(crate) fn load_cookie_files(&mut self, cb: &mut dyn Callbacks) {
        if self.cookie_files.is_empty() {
            return;
        }
        let files = self.cookie_files.clone();
        let mut log = Vec::new();
        let jar = self
            .cookies
            .get_or_insert_with(|| Rc::new(RefCell::new(CookieInfo::new())))
            .clone();
        for f in &files {
            jar.borrow_mut().init(
                Some(f),
                self.set.cookiesession,
                &mut self.setcookies,
                &mut log,
            );
        }
        self.cookie_engine = true;
        let tr = self.tracer();
        for l in log {
            tr.text(cb, &l);
        }
    }

    /// curl_easy_setopt()
    pub fn setopt(&mut self, option: u32, value: OptVal) -> CurlCode {
        let r = self.setopt_inner(option, value);
        if r == code::BAD_FUNCTION_ARGUMENT {
            let mut sink = self.backlog_sink();
            self.failf(&mut sink, &format!("setopt 0x{option:x} got bad argument"));
        }
        r
    }

    fn setopt_inner(&mut self, option: u32, value: OptVal) -> CurlCode {
        let Some(kind) = option_kind(option) else {
            return code::UNKNOWN_OPTION;
        };
        if NOT_IN_BUILD.contains(&option) {
            // SSH, GSS-API, c-ares and ECH options of a build without them
            return code::UNKNOWN_OPTION;
        }
        match (kind, value) {
            (OptKind::Long | OptKind::Values, OptVal::Long(v)) => self.setopt_long(option, v),
            (OptKind::OffT, OptVal::OffT(v)) | (OptKind::OffT, OptVal::Long(v)) => {
                self.setopt_offt(option, v)
            }
            (OptKind::SlistPoint, OptVal::Slist(l)) => self.setopt_slist(option, l),
            (OptKind::StringPoint | OptKind::ObjectPoint | OptKind::CbPoint, OptVal::Str(s)) => {
                self.setopt_str(option, s)
            }
            (OptKind::FunctionPoint, OptVal::Callback(on)) => {
                if option == opt::SSL_CTX_FUNCTION {
                    // supported by the OpenSSL backend; nothing to call here
                }
                if on {
                    self.set.callbacks.insert(option);
                } else {
                    self.set.callbacks.remove(&option);
                }
                code::OK
            }
            (OptKind::Blob, OptVal::Blob(b)) => {
                if b.len() > CURL_MAX_INPUT_LENGTH {
                    return code::BAD_FUNCTION_ARGUMENT;
                }
                self.set.blobs.insert(option, b.to_vec());
                code::OK
            }
            (OptKind::ObjectPoint, OptVal::Mime(m)) if option == opt::MIMEPOST => {
                self.set.mimepost = m.map(|m| Rc::new(RefCell::new(m)));
                self.set.method = HttpReq::PostMime;
                self.set.opt_no_body = false;
                code::OK
            }
            (OptKind::ObjectPoint, OptVal::Share(s)) if option == opt::SHARE => {
                self.set_share(s);
                code::OK
            }
            (_, OptVal::Str(None)) | (_, OptVal::Slist(None)) => {
                // NULL pointer for some other pointer kind
                self.set.strs.remove(&option);
                self.set.slists.remove(&option);
                code::OK
            }
            _ => code::BAD_FUNCTION_ARGUMENT,
        }
    }

    fn set_share(&mut self, s: Option<Rc<RefCell<Share>>>) {
        if let Some(old) = self.share.take() {
            let mut o = old.borrow_mut();
            if let (Some(a), Some(b)) = (&o.cookies, &self.cookies)
                && Rc::ptr_eq(a, b)
            {
                self.cookies = None;
            }
            o.dirty = o.dirty.saturating_sub(1);
        }
        if let Some(new) = s {
            {
                let mut n = new.borrow_mut();
                n.dirty += 1;
                if let Some(c) = &n.cookies {
                    self.cookies = Some(c.clone());
                }
            }
            self.share = Some(new);
        }
    }

    fn store_long(&mut self, option: u32, v: i64) -> CurlCode {
        self.set.longs.insert(option, v);
        code::OK
    }

    fn setopt_long(&mut self, option: u32, arg: i64) -> CurlCode {
        let enabled = arg != 0;
        let uarg = arg as u64;
        let s = &mut self.set;
        match option {
            opt::DNS_CACHE_TIMEOUT => {
                if arg < -1 {
                    return code::BAD_FUNCTION_ARGUMENT;
                }
                s.dns_cache_timeout = arg.min(INT_MAX);
            }
            opt::CA_CACHE_TIMEOUT => {
                if arg < -1 {
                    return code::BAD_FUNCTION_ARGUMENT;
                }
                return self.store_long(option, arg.min(INT_MAX));
            }
            opt::MAXCONNECTS => {
                if uarg > UINT_MAX as u64 {
                    return code::BAD_FUNCTION_ARGUMENT;
                }
                s.maxconnects = arg;
            }
            opt::FORBID_REUSE => s.reuse_forbid = enabled,
            opt::FRESH_CONNECT => s.reuse_fresh = enabled,
            opt::VERBOSE => s.verbose = enabled,
            opt::HEADER => s.include_header = enabled,
            opt::NOPROGRESS => s.hide_progress = enabled,
            opt::NOBODY => {
                s.opt_no_body = enabled;
                if enabled {
                    s.method = HttpReq::Head;
                } else if s.method == HttpReq::Head {
                    s.method = HttpReq::Get;
                }
            }
            opt::FAILONERROR => s.fail_on_error = enabled,
            opt::KEEP_SENDING_ON_ERROR => s.keep_sending_on_error = enabled,
            opt::UPLOAD | opt::PUT => {
                if enabled {
                    s.method = HttpReq::Put;
                    s.opt_no_body = false;
                } else {
                    s.method = HttpReq::Get;
                }
            }
            opt::FILETIME => s.get_filetime = enabled,
            opt::SERVER_RESPONSE_TIMEOUT => {
                if !(0..=INT_MAX / 1000).contains(&arg) {
                    return code::BAD_FUNCTION_ARGUMENT;
                }
                return self.store_long(option, arg);
            }
            opt::SERVER_RESPONSE_TIMEOUT_MS => {
                if !(0..=INT_MAX).contains(&arg) {
                    return code::BAD_FUNCTION_ARGUMENT;
                }
                return self.store_long(option, arg);
            }
            opt::TFTP_BLKSIZE => {
                let v = if arg < 8 { 512 } else { arg.min(65464) };
                return self.store_long(option, v);
            }
            opt::NETRC => {
                if !(0..3).contains(&arg) {
                    return code::BAD_FUNCTION_ARGUMENT;
                }
                return self.store_long(option, arg);
            }
            opt::TRANSFERTEXT => s.prefer_ascii = enabled,
            opt::TIMECONDITION => {
                if !(0..4).contains(&arg) {
                    return code::BAD_FUNCTION_ARGUMENT;
                }
                s.timecondition = arg;
            }
            opt::TIMEVALUE => s.timevalue = arg,
            opt::SSLVERSION | opt::PROXY_SSLVERSION => {
                let version = arg & 0xffff;
                let version_max = ((arg as u64) & 0xffff_0000) as i64;
                if version < sslver::DEFAULT
                    || version == sslver::SSLv2
                    || version == sslver::SSLv3
                    || version >= sslver::LAST
                    || !(sslver::MAX_NONE..sslver::MAX_LAST).contains(&version_max)
                {
                    return code::BAD_FUNCTION_ARGUMENT;
                }
                let p = if option == opt::SSLVERSION {
                    &mut s.ssl
                } else {
                    &mut s.proxy_ssl
                };
                p.version = version;
                p.version_max = version_max;
            }
            opt::POSTFIELDSIZE => {
                if arg < -1 {
                    return code::BAD_FUNCTION_ARGUMENT;
                }
                if s.postfieldsize < arg && s.postfields.is_some() {
                    s.postfields = None;
                }
                s.postfieldsize = arg;
            }
            opt::COOKIESESSION => s.cookiesession = enabled,
            opt::AUTOREFERER => s.auto_referer = enabled,
            opt::TRANSFER_ENCODING => s.http_transfer_encoding = enabled,
            opt::FOLLOWLOCATION => {
                if uarg > 3 {
                    return code::BAD_FUNCTION_ARGUMENT;
                }
                s.follow_mode = arg;
            }
            opt::UNRESTRICTED_AUTH => s.allow_auth_to_other_hosts = enabled,
            opt::MAXREDIRS => {
                if arg < -1 {
                    return code::BAD_FUNCTION_ARGUMENT;
                }
                s.maxredirs = arg;
            }
            opt::POSTREDIR => {
                if arg < 0 {
                    return code::BAD_FUNCTION_ARGUMENT;
                }
                s.keep_post = arg & 7;
            }
            opt::POST => {
                if enabled {
                    s.method = HttpReq::Post;
                    s.opt_no_body = false;
                } else {
                    s.method = HttpReq::Get;
                }
            }
            opt::HEADEROPT => s.sep_headers = arg & 1 != 0,
            opt::HTTPAUTH | opt::PROXYAUTH => match filter_auth(uarg) {
                Ok((a, ie)) => {
                    if option == opt::HTTPAUTH {
                        s.httpauth = a;
                        s.digest_ie = ie;
                    } else {
                        s.proxyauth = a;
                    }
                }
                Err(e) => return e,
            },
            opt::HTTPGET => {
                if enabled {
                    s.method = HttpReq::Get;
                    s.opt_no_body = false;
                }
            }
            opt::HTTP_VERSION => match arg {
                httpver::NONE | httpver::_1_0 | httpver::_1_1 => s.httpwant = arg,
                _ => {
                    if arg < httpver::NONE {
                        return code::BAD_FUNCTION_ARGUMENT;
                    }
                    return code::UNSUPPORTED_PROTOCOL;
                }
            },
            opt::EXPECT_100_TIMEOUT_MS => {
                if arg < 0 {
                    return code::BAD_FUNCTION_ARGUMENT;
                }
                s.expect_100_timeout = arg;
            }
            opt::HTTP09_ALLOWED => s.http09_allowed = enabled,
            opt::MIME_OPTIONS => s.mime_formescape = arg & 1 != 0,
            opt::HTTPPROXYTUNNEL => s.tunnel_proxy = enabled,
            opt::PROXYPORT => {
                if !(0..=65535).contains(&arg) {
                    return code::BAD_FUNCTION_ARGUMENT;
                }
                s.proxyport = arg;
            }
            opt::PROXYTYPE => {
                if !(0..=7).contains(&arg) {
                    return code::BAD_FUNCTION_ARGUMENT;
                }
                s.proxytype = arg;
            }
            opt::PROXY_TRANSFER_MODE => {
                if uarg > 1 {
                    return code::BAD_FUNCTION_ARGUMENT;
                }
                return self.store_long(option, arg);
            }
            opt::SOCKS5_AUTH => {
                if uarg & !(1 | 4) != 0 {
                    return code::NOT_BUILT_IN;
                }
                return self.store_long(option, arg);
            }
            opt::PROXY_SSL_VERIFYPEER => s.proxy_ssl.verifypeer = enabled,
            opt::PROXY_SSL_VERIFYHOST => s.proxy_ssl.verifyhost = enabled,
            opt::FTP_FILEMETHOD => {
                if !(0..4).contains(&arg) {
                    return code::BAD_FUNCTION_ARGUMENT;
                }
                return self.store_long(option, arg);
            }
            opt::FTP_SSL_CCC => {
                if !(0..3).contains(&arg) {
                    return code::BAD_FUNCTION_ARGUMENT;
                }
                return self.store_long(option, arg);
            }
            opt::FTPSSLAUTH => {
                if !(0..3).contains(&arg) {
                    return code::BAD_FUNCTION_ARGUMENT;
                }
                return self.store_long(option, arg);
            }
            opt::ACCEPTTIMEOUT_MS => {
                return self.store_long(option, (uarg.min(UINT_MAX as u64)) as i64);
            }
            opt::FTP_CREATE_MISSING_DIRS => {
                if !(0..=2).contains(&arg) {
                    return code::BAD_FUNCTION_ARGUMENT;
                }
                return self.store_long(option, arg);
            }
            opt::INFILESIZE => {
                if arg < -1 {
                    return code::BAD_FUNCTION_ARGUMENT;
                }
                s.filesize = arg;
            }
            opt::LOW_SPEED_LIMIT => {
                if arg < 0 {
                    return code::BAD_FUNCTION_ARGUMENT;
                }
                s.low_speed_limit = arg;
            }
            opt::LOW_SPEED_TIME => {
                if arg < 0 {
                    return code::BAD_FUNCTION_ARGUMENT;
                }
                s.low_speed_time = arg;
            }
            opt::PORT => {
                if !(0..=65535).contains(&arg) {
                    return code::BAD_FUNCTION_ARGUMENT;
                }
                s.use_port = arg;
            }
            opt::TIMEOUT => {
                if !(0..=INT_MAX / 1000).contains(&arg) {
                    return code::BAD_FUNCTION_ARGUMENT;
                }
                s.timeout_ms = arg * 1000;
            }
            opt::TIMEOUT_MS => s.timeout_ms = uarg.min(UINT_MAX as u64) as i64,
            opt::CONNECTTIMEOUT => {
                if !(0..=INT_MAX / 1000).contains(&arg) {
                    return code::BAD_FUNCTION_ARGUMENT;
                }
                s.connecttimeout_ms = arg * 1000;
            }
            opt::CONNECTTIMEOUT_MS => s.connecttimeout_ms = uarg.min(UINT_MAX as u64) as i64,
            opt::RESUME_FROM => {
                if arg < -1 {
                    return code::BAD_FUNCTION_ARGUMENT;
                }
                s.resume_from = arg;
            }
            opt::CRLF => s.crlf = enabled,
            opt::LOCALPORT | opt::LOCALPORTRANGE => {
                if !(0..=65535).contains(&arg) {
                    return code::BAD_FUNCTION_ARGUMENT;
                }
                return self.store_long(option, arg);
            }
            opt::SSL_VERIFYPEER => s.ssl.verifypeer = enabled,
            opt::DOH_SSL_VERIFYPEER | opt::DOH_SSL_VERIFYHOST | opt::DOH_SSL_VERIFYSTATUS => {
                return self.store_long(option, arg);
            }
            opt::SSL_VERIFYHOST => s.ssl.verifyhost = enabled,
            opt::SSL_VERIFYSTATUS => s.ssl.verifystatus = enabled,
            opt::SSL_FALSESTART => return code::NOT_BUILT_IN,
            opt::CERTINFO => s.certinfo = enabled,
            opt::BUFFERSIZE => {
                s.buffer_size = if arg > READBUFFER_MAX {
                    READBUFFER_MAX
                } else if arg < 1 {
                    READBUFFER_SIZE
                } else if arg < READBUFFER_MIN {
                    READBUFFER_MIN
                } else {
                    arg
                };
            }
            opt::UPLOAD_BUFFERSIZE => {
                s.upload_buffer_size = arg.clamp(UPLOADBUFFER_MIN, UPLOADBUFFER_MAX);
            }
            opt::NOSIGNAL => return self.store_long(option, arg),
            opt::MAXFILESIZE => {
                if arg < 0 {
                    return code::BAD_FUNCTION_ARGUMENT;
                }
                s.max_filesize = arg;
            }
            opt::USE_SSL => {
                if !(0..4).contains(&arg) {
                    return code::BAD_FUNCTION_ARGUMENT;
                }
                return self.store_long(option, arg);
            }
            opt::SSL_OPTIONS => s.ssl.ssl_options = arg & 0xff,
            opt::PROXY_SSL_OPTIONS => s.proxy_ssl.ssl_options = arg & 0xff,
            opt::IPRESOLVE => {
                if !(0..=2).contains(&arg) {
                    return code::BAD_FUNCTION_ARGUMENT;
                }
                s.ipresolve = arg;
            }
            opt::TCP_NODELAY => s.tcp_nodelay = enabled,
            opt::IGNORE_CONTENT_LENGTH => s.ignore_cl = enabled,
            opt::CONNECT_ONLY => {
                if arg > 2 {
                    return code::BAD_FUNCTION_ARGUMENT;
                }
                s.connect_only = arg;
            }
            opt::SSL_SESSIONID_CACHE => {
                s.ssl.cache_session = enabled;
                s.proxy_ssl.cache_session = enabled;
            }
            opt::HTTP_TRANSFER_DECODING => s.http_te_skip = !enabled,
            opt::HTTP_CONTENT_DECODING => s.http_ce_skip = !enabled,
            opt::NEW_FILE_PERMS | opt::NEW_DIRECTORY_PERMS => {
                if !(0..=0o777).contains(&arg) {
                    return code::BAD_FUNCTION_ARGUMENT;
                }
                return self.store_long(option, arg);
            }
            opt::ADDRESS_SCOPE => {
                if uarg > UINT_MAX as u64 {
                    return code::BAD_FUNCTION_ARGUMENT;
                }
                return self.store_long(option, arg);
            }
            opt::PROTOCOLS => s.allowed_protocols = arg,
            opt::REDIR_PROTOCOLS => s.redir_protocols = arg,
            opt::RTSP_REQUEST => {
                if !(1..=11).contains(&arg) {
                    return code::BAD_FUNCTION_ARGUMENT;
                }
                return self.store_long(option, arg);
            }
            opt::TCP_KEEPIDLE | opt::TCP_KEEPINTVL | opt::TCP_KEEPCNT => {
                if arg < 0 {
                    return code::BAD_FUNCTION_ARGUMENT;
                }
                return self.store_long(option, arg.min(INT_MAX));
            }
            opt::STREAM_WEIGHT => return code::NOT_BUILT_IN,
            opt::SSL_ENABLE_ALPN => s.ssl_enable_alpn = enabled,
            opt::PATH_AS_IS => s.path_as_is = enabled,
            opt::SUPPRESS_CONNECT_HEADERS => s.suppress_connect_headers = enabled,
            opt::HAPPY_EYEBALLS_TIMEOUT_MS => {
                return self.store_long(option, (uarg.min(UINT_MAX as u64)) as i64);
            }
            opt::DISALLOW_USERNAME_IN_URL => s.disallow_username_in_url = enabled,
            opt::UPKEEP_INTERVAL_MS => {
                if arg < 0 {
                    return code::BAD_FUNCTION_ARGUMENT;
                }
                s.upkeep_interval_ms = arg;
            }
            opt::MAXAGE_CONN => {
                if arg < 0 {
                    return code::BAD_FUNCTION_ARGUMENT;
                }
                s.maxage_conn = arg;
            }
            opt::MAXLIFETIME_CONN => {
                if arg < 0 {
                    return code::BAD_FUNCTION_ARGUMENT;
                }
                s.maxlifetime_conn = arg;
            }
            opt::ALTSVC_CTRL => {
                if arg == 0 {
                    return code::BAD_FUNCTION_ARGUMENT;
                }
                return self.store_long(option, arg);
            }
            opt::SSLENGINE_DEFAULT => {
                s.strs.remove(&opt::SSLENGINE);
                return code::OK;
            }
            _ => {
                // every other long option of the table: accepted as is
                return self.store_long(option, arg);
            }
        }
        code::OK
    }

    fn setopt_offt(&mut self, option: u32, arg: i64) -> CurlCode {
        let s = &mut self.set;
        match option {
            opt::TIMEVALUE_LARGE => s.timevalue = arg,
            opt::POSTFIELDSIZE_LARGE => {
                if arg < -1 {
                    return code::BAD_FUNCTION_ARGUMENT;
                }
                if s.postfieldsize < arg && s.postfields.is_some() {
                    s.postfields = None;
                }
                s.postfieldsize = arg;
            }
            opt::INFILESIZE_LARGE => {
                if arg < -1 {
                    return code::BAD_FUNCTION_ARGUMENT;
                }
                s.filesize = arg;
            }
            opt::MAX_SEND_SPEED_LARGE => {
                if arg < 0 {
                    return code::BAD_FUNCTION_ARGUMENT;
                }
                s.max_send_speed = arg;
            }
            opt::MAX_RECV_SPEED_LARGE => {
                if arg < 0 {
                    return code::BAD_FUNCTION_ARGUMENT;
                }
                s.max_recv_speed = arg;
            }
            opt::RESUME_FROM_LARGE => {
                if arg < -1 {
                    return code::BAD_FUNCTION_ARGUMENT;
                }
                s.resume_from = arg;
            }
            opt::MAXFILESIZE_LARGE => {
                if arg < 0 {
                    return code::BAD_FUNCTION_ARGUMENT;
                }
                s.max_filesize = arg;
            }
            _ => return code::UNKNOWN_OPTION,
        }
        code::OK
    }

    fn setopt_slist(&mut self, option: u32, l: Option<Vec<Vec<u8>>>) -> CurlCode {
        match l {
            Some(v) => {
                self.set.slists.insert(option, v);
            }
            None => {
                self.set.slists.remove(&option);
            }
        }
        code::OK
    }

    fn setstr(&mut self, option: u32, v: Option<&[u8]>) -> CurlCode {
        match v {
            Some(v) => {
                let v = cstr(v);
                if v.len() > CURL_MAX_INPUT_LENGTH {
                    self.set.strs.remove(&option);
                    return code::BAD_FUNCTION_ARGUMENT;
                }
                self.set.strs.insert(option, v.to_vec());
            }
            None => {
                self.set.strs.remove(&option);
            }
        }
        code::OK
    }

    fn setopt_str(&mut self, option: u32, v: Option<&[u8]>) -> CurlCode {
        match option {
            opt::RANDOM_FILE | opt::EGDSOCKET => code::OK,
            opt::COPYPOSTFIELDS | opt::POSTFIELDS => {
                let s = &mut self.set;
                match v {
                    None => s.postfields = None,
                    Some(p) => {
                        if s.postfieldsize == -1 {
                            let p = cstr(p);
                            if p.len() > CURL_MAX_INPUT_LENGTH {
                                return code::BAD_FUNCTION_ARGUMENT;
                            }
                            s.postfields = Some(p.to_vec());
                        } else {
                            let n = (s.postfieldsize as usize).min(p.len());
                            let mut d = p[..n].to_vec();
                            d.resize(s.postfieldsize as usize, 0);
                            s.postfields = Some(d);
                        }
                    }
                }
                s.method = HttpReq::Post;
                code::OK
            }
            opt::ACCEPT_ENCODING => match v {
                Some(p) if cstr(p).is_empty() => self.setstr(option, Some(b"deflate, gzip")),
                _ => self.setstr(option, v),
            },
            opt::AWS_SIGV4 => {
                let r = self.setstr(option, v);
                if self.set.strs.contains_key(&opt::AWS_SIGV4) {
                    self.set.httpauth = super::consts::auth::AWS_SIGV4;
                }
                r
            }
            opt::REFERER => {
                let r = self.setstr(option, v);
                self.st.referer = self.set.strs.get(&opt::REFERER).cloned();
                r
            }
            opt::COOKIEFILE => match v {
                Some(p) => {
                    let p = cstr(p);
                    if p.len() > CURL_MAX_INPUT_LENGTH {
                        return code::BAD_FUNCTION_ARGUMENT;
                    }
                    self.cookie_files.push(p.to_vec());
                    code::OK
                }
                None => {
                    self.cookie_files.clear();
                    let shared = self
                        .share
                        .as_ref()
                        .is_some_and(|s| s.borrow().cookies.is_some());
                    if !shared && let Some(c) = &self.cookies {
                        c.borrow_mut().clearall();
                    }
                    self.cookies = None;
                    code::OK
                }
            },
            opt::COOKIEJAR => {
                let r = self.setstr(option, v);
                if r == code::OK {
                    let mut sink = self.backlog_sink();
                    self.cookie_init(&mut sink, self.set.cookiesession);
                }
                r
            }
            opt::COOKIELIST => {
                let Some(p) = v else {
                    return code::OK;
                };
                let p = cstr(p).to_vec();
                if p.eq_ignore_ascii_case(b"ALL") {
                    if let Some(c) = &self.cookies {
                        c.borrow_mut().clearall();
                    }
                } else if p.eq_ignore_ascii_case(b"SESS") {
                    if let Some(c) = &self.cookies {
                        c.borrow_mut().clearsess();
                    }
                } else if p.eq_ignore_ascii_case(b"FLUSH") {
                    let mut sink = self.backlog_sink();
                    self.flush_cookies_cb(&mut sink, false);
                } else if p.eq_ignore_ascii_case(b"RELOAD") {
                    let mut sink = self.backlog_sink();
                    self.load_cookie_files(&mut sink);
                } else {
                    if self.cookies.is_none() {
                        self.cookies =
                            Some(Rc::new(RefCell::new(CookieInfo::new_initialized(true))));
                        self.cookie_engine = true;
                    }
                    if p.len() > CURL_MAX_INPUT_LENGTH {
                        return code::BAD_FUNCTION_ARGUMENT;
                    }
                    let jar = self.cookies.clone().unwrap();
                    let mut log = Vec::new();
                    jar.borrow_mut()
                        .add_cookielist_line(&mut self.setcookies, &p, &mut log);
                    let mut sink = self.backlog_sink();
                    let tr = self.tracer();
                    for l in log {
                        tr.text(&mut sink, &l);
                    }
                }
                code::OK
            }
            opt::URL => {
                let r = self.setstr(option, v);
                self.st.url = self.set.strs.get(&opt::URL).cloned();
                r
            }
            opt::USERPWD => match v {
                Some(p) => {
                    let p = cstr(p);
                    if p.len() > CURL_MAX_INPUT_LENGTH {
                        return code::BAD_FUNCTION_ARGUMENT;
                    }
                    let (u, pw) = parse_login(p);
                    self.set.strs.insert(opt::USERNAME, u);
                    match pw {
                        Some(pw) => self.set.strs.insert(opt::PASSWORD, pw),
                        None => self.set.strs.remove(&opt::PASSWORD),
                    };
                    code::OK
                }
                None => {
                    self.set.strs.remove(&opt::USERNAME);
                    self.set.strs.remove(&opt::PASSWORD);
                    code::OK
                }
            },
            opt::PROXYUSERPWD => {
                if let Some(p) = v {
                    let p = cstr(p);
                    if p.len() > CURL_MAX_INPUT_LENGTH {
                        return code::BAD_FUNCTION_ARGUMENT;
                    }
                    let (u, pw) = parse_login(p);
                    self.set
                        .strs
                        .insert(opt::PROXYUSERNAME, super::urlapi::easy_unescape(&u));
                    if let Some(pw) = pw {
                        self.set
                            .strs
                            .insert(opt::PROXYPASSWORD, super::urlapi::easy_unescape(&pw));
                    }
                }
                code::OK
            }
            opt::SSLENGINE => {
                if let Some(p) = v
                    && !cstr(p).is_empty()
                {
                    // there are no crypto engines: Curl_ssl_set_engine fails
                    let _ = self.setstr(option, Some(p));
                    let e = String::from_utf8_lossy(cstr(p)).into_owned();
                    let mut sink = self.backlog_sink();
                    self.failf(&mut sink, &format!("SSL Engine '{e}' not found"));
                    return code::SSL_ENGINE_NOTFOUND;
                }
                code::OK
            }
            opt::INTERFACE => {
                if let Some(p) = v {
                    let p = cstr(p);
                    let bad = p.len() > 512
                        || p.is_empty()
                        || p == b"if!"
                        || p == b"host!"
                        || (p.starts_with(b"ifhost!") && {
                            let rest = &p[7..];
                            match rest.iter().position(|&c| c == b'!') {
                                Some(i) => i + 1 >= rest.len(),
                                None => true,
                            }
                        });
                    if bad {
                        return code::BAD_FUNCTION_ARGUMENT;
                    }
                }
                self.setstr(option, v)
            }
            opt::HAPROXY_CLIENT_IP => {
                let r = self.setstr(option, v);
                self.set.longs.insert(opt::HAPROXYPROTOCOL, 1);
                r
            }
            opt::PROTOCOLS_STR => match v {
                Some(p) => match protocol2num(cstr(p)) {
                    Ok(n) => {
                        self.set.allowed_protocols = n;
                        code::OK
                    }
                    Err(e) => {
                        self.set.allowed_protocols = 0;
                        e
                    }
                },
                None => {
                    self.set.allowed_protocols = CURLPROTO_ALL;
                    code::OK
                }
            },
            opt::REDIR_PROTOCOLS_STR => match v {
                Some(p) => match protocol2num(cstr(p)) {
                    Ok(n) => {
                        self.set.redir_protocols = n;
                        code::OK
                    }
                    Err(e) => {
                        self.set.redir_protocols = 0;
                        e
                    }
                },
                None => {
                    self.set.redir_protocols = CURLPROTO_REDIR;
                    code::OK
                }
            },
            opt::TLSAUTH_TYPE | opt::PROXY_TLSAUTH_TYPE => match v {
                Some(p) if !cstr(p).eq_ignore_ascii_case(b"SRP") => code::BAD_FUNCTION_ARGUMENT,
                _ => code::OK,
            },
            opt::FTPPORT => {
                let r = self.setstr(option, v);
                let on = self.set.strs.contains_key(&opt::FTPPORT) as i64;
                self.set.longs.insert(0x7fff_0001, on);
                r
            }
            opt::CURLU => {
                // a CURLU pointer cannot cross this API
                code::OK
            }
            opt::ECH => match v {
                None => code::OK,
                Some(p) => {
                    if cstr(p).len() > CURL_MAX_INPUT_LENGTH {
                        return code::BAD_FUNCTION_ARGUMENT;
                    }
                    self.setstr(option, Some(p))
                }
            },
            opt::ERRORBUFFER
            | opt::WRITEDATA
            | opt::READDATA
            | opt::HEADERDATA
            | opt::DEBUGDATA
            | opt::XFERINFODATA
            | opt::SEEKDATA
            | opt::IOCTLDATA
            | opt::SSL_CTX_DATA
            | opt::SOCKOPTDATA
            | opt::OPENSOCKETDATA
            | opt::RESOLVER_START_DATA
            | opt::CLOSESOCKETDATA
            | opt::TRAILERDATA
            | opt::PREREQDATA
            | opt::PRIVATE
            | opt::SSH_KEYDATA
            | opt::SSH_HOSTKEYDATA
            | opt::INTERLEAVEDATA
            | opt::CHUNK_DATA
            | opt::FNMATCH_DATA
            | opt::HSTSREADDATA
            | opt::HSTSWRITEDATA => {
                // opaque pointers: the binding keeps them
                code::OK
            }
            opt::STDERR => {
                self.set.has_stderr = v.is_some();
                code::OK
            }
            _ => self.setstr(option, v),
        }
    }
}

/// Options libcurl 8.14.1 only knows when built with libssh2, GSS-API,
/// c-ares or ECH support: setopt() says CURLE_UNKNOWN_OPTION for them
const NOT_IN_BUILD: &[u32] = &[
    opt::SSH_AUTH_TYPES,
    opt::NEW_DIRECTORY_PERMS,
    opt::SOCKS5_GSSAPI_NEC,
    opt::GSSAPI_DELEGATION,
    opt::SSH_COMPRESSION,
    opt::SSH_PUBLIC_KEYFILE,
    opt::SSH_PRIVATE_KEYFILE,
    opt::SSH_HOST_PUBLIC_KEY_MD5,
    opt::SSH_KNOWNHOSTS,
    opt::SSH_KEYFUNCTION,
    opt::SSH_KEYDATA,
    opt::SSH_HOST_PUBLIC_KEY_SHA256,
    opt::SSH_HOSTKEYFUNCTION,
    opt::SSH_HOSTKEYDATA,
    opt::DNS_SERVERS,
    opt::DNS_INTERFACE,
    opt::DNS_LOCAL_IP4,
    opt::DNS_LOCAL_IP6,
    opt::ECH,
    opt::KRBLEVEL,
];

/// strdup() semantics: a C string stops at the first NUL
pub(crate) fn cstr(v: &[u8]) -> &[u8] {
    match v.iter().position(|&c| c == 0) {
        Some(i) => &v[..i],
        None => v,
    }
}
