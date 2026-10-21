//! The user-defined settings of an easy handle (struct UserDefined) and the
//! option validation of lib/setopt.c.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use super::consts::{OPTIONS, OptKind, auth, code, httpver, opt, proto};
use super::{CurlCode, Mime};

pub(crate) const CURL_MAX_INPUT_LENGTH: usize = 8_000_000;
pub(crate) const READBUFFER_SIZE: i64 = 16384;
pub(crate) const READBUFFER_MAX: i64 = 10 * 1024 * 1024;
pub(crate) const READBUFFER_MIN: i64 = 1024;
pub(crate) const UPLOADBUFFER_DEFAULT: i64 = 65536;
pub(crate) const UPLOADBUFFER_MAX: i64 = 2 * 1024 * 1024;
pub(crate) const UPLOADBUFFER_MIN: i64 = 16384;
pub(crate) const INT_MAX: i64 = i32::MAX as i64;
pub(crate) const UINT_MAX: i64 = u32::MAX as i64;

pub(crate) const DEFAULT_CA_BUNDLE: &str = "/etc/ssl/certs/ca-certificates.crt";
pub(crate) const DEFAULT_CA_PATH: &str = "/etc/ssl/certs";

/// CURLPROTO_* of the schemes libcurl knows about (Curl_getn_scheme_handler)
pub(crate) fn scheme_protocol(name: &[u8]) -> Option<i64> {
    let n = name.to_ascii_lowercase();
    Some(match &n[..] {
        b"http" => proto::HTTP,
        b"https" => proto::HTTPS,
        b"ftp" => proto::FTP,
        b"ftps" => proto::FTPS,
        b"file" => proto::FILE,
        b"dict" => proto::DICT,
        b"telnet" => proto::TELNET,
        b"tftp" => proto::TFTP,
        b"ldap" => proto::LDAP,
        b"ldaps" => proto::LDAPS,
        b"imap" => proto::IMAP,
        b"imaps" => proto::IMAPS,
        b"pop3" => proto::POP3,
        b"pop3s" => proto::POP3S,
        b"smtp" => proto::SMTP,
        b"smtps" => proto::SMTPS,
        b"rtsp" => proto::RTSP,
        b"rtmp" => proto::RTMP,
        b"rtmpt" => proto::RTMPT,
        b"rtmpe" => proto::RTMPE,
        b"rtmpte" => proto::RTMPTE,
        b"rtmps" => proto::RTMPS,
        b"rtmpts" => proto::RTMPTS,
        b"gopher" => proto::GOPHER,
        b"gophers" => proto::GOPHERS,
        b"smb" => proto::SMB,
        b"smbs" => proto::SMBS,
        b"mqtt" => proto::MQTT,
        b"scp" => proto::SCP,
        b"sftp" => proto::SFTP,
        b"ws" => 1 << 30,
        b"wss" => 1i64 << 31,
        _ => return None,
    })
}

pub(crate) const CURLPROTO_ALL: i64 = !0;
pub(crate) const CURLPROTO_REDIR: i64 = proto::HTTP | proto::HTTPS | proto::FTP | proto::FTPS;

/// Curl_HttpReq
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HttpReq {
    Get,
    Post,
    PostForm,
    PostMime,
    Put,
    Head,
}

/// struct ssl_primary_config + the ssl_config_data bits
#[derive(Clone, Debug)]
pub(crate) struct SslSet {
    pub verifypeer: bool,
    pub verifyhost: bool,
    pub verifystatus: bool,
    pub version: i64,
    pub version_max: i64,
    pub ssl_options: i64,
    pub cache_session: bool,
}

impl Default for SslSet {
    fn default() -> Self {
        SslSet {
            verifypeer: true,
            verifyhost: true,
            verifystatus: false,
            version: 0,
            version_max: 0,
            ssl_options: 0,
            cache_session: true,
        }
    }
}

/// struct UserDefined
#[derive(Clone)]
pub(crate) struct Set {
    /// string options (STRING_*), keyed by CURLOPT number
    pub strs: HashMap<u32, Vec<u8>>,
    pub slists: HashMap<u32, Vec<Vec<u8>>>,
    pub blobs: HashMap<u32, Vec<u8>>,
    /// installed callbacks (*FUNCTION options)
    pub callbacks: HashSet<u32>,
    /// values of options without a dedicated field
    pub longs: HashMap<u32, i64>,

    pub verbose: bool,
    pub include_header: bool,
    pub hide_progress: bool,
    pub opt_no_body: bool,
    pub method: HttpReq,
    pub fail_on_error: bool,
    pub keep_sending_on_error: bool,
    pub get_filetime: bool,
    pub postfields: Option<Vec<u8>>,
    pub postfieldsize: i64,
    pub filesize: i64,
    pub maxredirs: i64,
    pub follow_mode: i64,
    pub allow_auth_to_other_hosts: bool,
    pub auto_referer: bool,
    pub keep_post: i64,
    pub httpauth: u64,
    pub proxyauth: u64,
    pub digest_ie: bool,
    pub httpwant: i64,
    pub expect_100_timeout: i64,
    pub http09_allowed: bool,
    pub tunnel_proxy: bool,
    pub proxyport: i64,
    pub proxytype: i64,
    pub ssl: SslSet,
    pub proxy_ssl: SslSet,
    pub certinfo: bool,
    pub buffer_size: i64,
    pub upload_buffer_size: i64,
    pub max_filesize: i64,
    pub ipresolve: i64,
    pub tcp_nodelay: bool,
    pub ignore_cl: bool,
    pub connect_only: i64,
    pub http_te_skip: bool,
    pub http_ce_skip: bool,
    pub allowed_protocols: i64,
    pub redir_protocols: i64,
    pub path_as_is: bool,
    pub disallow_username_in_url: bool,
    pub use_port: i64,
    pub timeout_ms: i64,
    pub connecttimeout_ms: i64,
    pub low_speed_limit: i64,
    pub low_speed_time: i64,
    pub resume_from: i64,
    pub maxconnects: i64,
    pub reuse_forbid: bool,
    pub reuse_fresh: bool,
    pub cookiesession: bool,
    pub dns_cache_timeout: i64,
    pub http_transfer_encoding: bool,
    pub sep_headers: bool,
    pub crlf: bool,
    pub timecondition: i64,
    pub timevalue: i64,
    pub max_send_speed: i64,
    pub max_recv_speed: i64,
    pub suppress_connect_headers: bool,
    pub upkeep_interval_ms: i64,
    pub maxage_conn: i64,
    pub maxlifetime_conn: i64,
    pub ssl_enable_alpn: bool,
    pub prefer_ascii: bool,
    pub mime_formescape: bool,
    pub mimepost: Option<Rc<std::cell::RefCell<Mime>>>,
    pub has_stderr: bool,
}

impl Default for Set {
    fn default() -> Self {
        // Curl_init_userdefined()
        let mut strs = HashMap::new();
        strs.insert(opt::CAINFO, DEFAULT_CA_BUNDLE.as_bytes().to_vec());
        strs.insert(opt::PROXY_CAINFO, DEFAULT_CA_BUNDLE.as_bytes().to_vec());
        strs.insert(opt::CAPATH, DEFAULT_CA_PATH.as_bytes().to_vec());
        strs.insert(opt::PROXY_CAPATH, DEFAULT_CA_PATH.as_bytes().to_vec());
        Set {
            strs,
            slists: HashMap::new(),
            blobs: HashMap::new(),
            callbacks: HashSet::new(),
            longs: HashMap::new(),
            verbose: false,
            include_header: false,
            hide_progress: true,
            opt_no_body: false,
            method: HttpReq::Get,
            fail_on_error: false,
            keep_sending_on_error: false,
            get_filetime: false,
            postfields: None,
            postfieldsize: -1,
            filesize: -1,
            maxredirs: 30,
            follow_mode: 0,
            allow_auth_to_other_hosts: false,
            auto_referer: false,
            keep_post: 0,
            httpauth: auth::BASIC,
            proxyauth: auth::BASIC,
            digest_ie: false,
            httpwant: httpver::NONE,
            expect_100_timeout: 1000,
            http09_allowed: false,
            tunnel_proxy: false,
            proxyport: 0,
            proxytype: 0,
            ssl: SslSet::default(),
            proxy_ssl: SslSet::default(),
            certinfo: false,
            buffer_size: READBUFFER_SIZE,
            upload_buffer_size: UPLOADBUFFER_DEFAULT,
            max_filesize: 0,
            ipresolve: 0,
            tcp_nodelay: true,
            ignore_cl: false,
            connect_only: 0,
            http_te_skip: false,
            http_ce_skip: false,
            allowed_protocols: CURLPROTO_ALL,
            redir_protocols: CURLPROTO_REDIR,
            path_as_is: false,
            disallow_username_in_url: false,
            use_port: 0,
            timeout_ms: 0,
            connecttimeout_ms: 0,
            low_speed_limit: 0,
            low_speed_time: 0,
            resume_from: 0,
            maxconnects: 5,
            reuse_forbid: false,
            reuse_fresh: false,
            cookiesession: false,
            dns_cache_timeout: 60,
            http_transfer_encoding: false,
            sep_headers: true,
            crlf: false,
            timecondition: 0,
            timevalue: 0,
            max_send_speed: 0,
            max_recv_speed: 0,
            suppress_connect_headers: false,
            upkeep_interval_ms: 60000,
            maxage_conn: 118,
            maxlifetime_conn: 0,
            ssl_enable_alpn: true,
            prefer_ascii: false,
            mime_formescape: false,
            mimepost: None,
            has_stderr: false,
        }
    }
}

/// The kind of a known option, None for options libcurl 8.14.1 does not know
pub(crate) fn option_kind(option: u32) -> Option<OptKind> {
    OPTIONS
        .iter()
        .find(|(n, _, _)| *n == option)
        .map(|(_, _, k)| *k)
}

/// Curl_parse_login_details() without options: "user:password"
pub(crate) fn parse_login(login: &[u8]) -> (Vec<u8>, Option<Vec<u8>>) {
    match login.iter().position(|&c| c == b':') {
        Some(i) => (login[..i].to_vec(), Some(login[i + 1..].to_vec())),
        None => (login.to_vec(), None),
    }
}

/// protocol2num() for CURLOPT_PROTOCOLS_STR / REDIR_PROTOCOLS_STR
pub(crate) fn protocol2num(s: &[u8]) -> Result<i64, CurlCode> {
    if s.eq_ignore_ascii_case(b"all") {
        return Ok(CURLPROTO_ALL);
    }
    let mut val = 0i64;
    for tok in s.split(|&c| c == b',') {
        if tok.is_empty() {
            continue;
        }
        match scheme_protocol(tok) {
            Some(p) => val |= p,
            None => return Err(code::UNSUPPORTED_PROTOCOL),
        }
    }
    if val == 0 {
        return Err(code::BAD_FUNCTION_ARGUMENT);
    }
    Ok(val)
}

/// What the http auth bits are after httpauth() (setopt.c) filtering
pub(crate) fn filter_auth(mut a: u64) -> Result<(u64, bool), CurlCode> {
    let mut iestyle = false;
    if a != auth::NONE {
        iestyle = a & auth::DIGEST_IE != 0;
        if iestyle {
            a |= auth::DIGEST;
            a &= !auth::DIGEST_IE;
        }
        // no Negotiate (no GSS-API); NTLM is supported
        a &= !auth::NEGOTIATE;
        if (0..31).all(|b| a & (1u64 << b) == 0) {
            return Err(code::NOT_BUILT_IN);
        }
    }
    Ok((a, iestyle))
}
