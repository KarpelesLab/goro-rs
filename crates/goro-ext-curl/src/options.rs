//! The transfer settings of an easy handle (the options of
//! curl_easy_setopt(3) that this client implements), with libcurl's
//! defaults (libcurl option documentation, "DEFAULT" sections).

use crate::consts as c;
use crate::multipart::Part;

/// The request body.
#[derive(Clone, Debug, Default)]
pub enum Body {
    /// No body (CURLOPT_POSTFIELDS unset).
    #[default]
    None,
    /// A string body (application/x-www-form-urlencoded unless a
    /// Content-Type header says otherwise).
    Fields(Vec<u8>),
    /// A multipart/form-data form.
    Form(Vec<Part>),
}

/// The settings a transfer reads.
#[derive(Clone, Debug)]
pub struct Options {
    pub url: Option<Vec<u8>>,
    pub default_protocol: Option<String>,
    /// CURLPROTO_* masks.
    pub protocols: i64,
    pub redir_protocols: i64,
    /// CURLOPT_PORT (0: the URL's).
    pub port: i64,
    pub verbose: bool,
    /// CURLOPT_HEADER: headers in the body output.
    pub header: bool,
    pub noprogress: bool,
    pub nobody: bool,
    pub failonerror: bool,
    pub upload: bool,
    pub post: bool,
    pub body: Body,
    pub custom_request: Option<Vec<u8>>,
    /// CURLOPT_INFILESIZE (-1: unknown).
    pub infilesize: i64,
    /// CURLOPT_FOLLOWLOCATION (0, or a CURLFOLLOW_* mode).
    pub followlocation: i64,
    /// CURLOPT_MAXREDIRS (-1: no limit).
    pub maxredirs: i64,
    pub autoreferer: bool,
    /// CURLOPT_POSTREDIR bits (CURL_REDIR_POST_301/302/303).
    pub postredir: i64,
    pub unrestricted_auth: bool,
    /// CURLOPT_HTTP_VERSION.
    pub http_version: i64,
    pub headers: Vec<Vec<u8>>,
    pub proxy_headers: Vec<Vec<u8>>,
    pub useragent: Option<Vec<u8>>,
    pub referer: Option<Vec<u8>>,
    pub cookie: Option<Vec<u8>>,
    /// CURLOPT_ACCEPT_ENCODING ("" for every supported encoding).
    pub accept_encoding: Option<Vec<u8>>,
    pub content_decoding: bool,
    pub transfer_decoding: bool,
    pub userpwd: Option<Vec<u8>>,
    pub username: Option<Vec<u8>>,
    pub password: Option<Vec<u8>>,
    /// CURLOPT_HTTPAUTH mask.
    pub httpauth: i64,
    pub proxy: Option<Vec<u8>>,
    pub proxyport: i64,
    pub proxytype: i64,
    pub proxyuserpwd: Option<Vec<u8>>,
    pub proxyusername: Option<Vec<u8>>,
    pub proxypassword: Option<Vec<u8>>,
    pub proxyauth: i64,
    pub noproxy: Option<Vec<u8>>,
    pub httpproxytunnel: bool,
    /// CURLOPT_TIMEOUT(_MS) in milliseconds (0: none).
    pub timeout_ms: i64,
    /// CURLOPT_CONNECTTIMEOUT(_MS) in milliseconds (0: the default 300 s).
    pub connect_timeout_ms: i64,
    pub expect_100_timeout_ms: i64,
    pub range: Option<Vec<u8>>,
    pub resume_from: i64,
    pub buffersize: i64,
    /// CURLOPT_RESOLVE entries ("host:port:addr[,addr]", "-host:port").
    pub resolve: Vec<Vec<u8>>,
    /// CURLOPT_CONNECT_TO entries ("host:port:connect-host:connect-port").
    pub connect_to: Vec<Vec<u8>>,
    pub interface: Option<Vec<u8>>,
    pub localport: i64,
    /// CURLOPT_IPRESOLVE (CURL_IPRESOLVE_WHATEVER/V4/V6).
    pub ipresolve: i64,
    pub unix_socket_path: Option<Vec<u8>>,
    pub request_target: Option<Vec<u8>>,
    pub ssl_verifypeer: bool,
    pub ssl_verifyhost: i64,
    pub sslversion: i64,
    pub cainfo: Option<Vec<u8>>,
    pub cainfo_blob: Option<Vec<u8>>,
    pub capath: Option<Vec<u8>>,
    pub sslcert: Option<Vec<u8>>,
    pub sslcert_blob: Option<Vec<u8>>,
    pub sslkey: Option<Vec<u8>>,
    pub sslkey_blob: Option<Vec<u8>>,
    pub keypasswd: Option<Vec<u8>>,
    pub issuercert: Option<Vec<u8>>,
    pub issuercert_blob: Option<Vec<u8>>,
    pub certinfo: bool,
    pub filetime: bool,
    /// CURLOPT_TIMECONDITION (CURL_TIMECOND_*) and CURLOPT_TIMEVALUE.
    pub timecondition: i64,
    pub timevalue: i64,
    pub maxfilesize: i64,
    pub ignore_content_length: bool,
    pub http09_allowed: bool,
    pub path_as_is: bool,
    pub keep_sending_on_error: bool,
    pub forbid_reuse: bool,
    pub fresh_connect: bool,
    /// CURLMIMEOPT_FORMESCAPE.
    pub mime_formescape: bool,
    /// Whether the request header block is kept for CURLINFO_HEADER_OUT.
    pub record_header_out: bool,
    /// CURLOPT_APPEND (file:// uploads append).
    pub append: bool,
    pub new_file_perms: i64,
    pub low_speed_limit: i64,
    pub low_speed_time: i64,
    pub transfertext: bool,
    pub crlf: bool,
}

/// CURLPROTO_* bits of the schemes this client speaks.
pub const SUPPORTED: i64 = c::CURLPROTO_HTTP | c::CURLPROTO_HTTPS | c::CURLPROTO_FILE;

impl Default for Options {
    fn default() -> Options {
        Options {
            url: None,
            default_protocol: None,
            protocols: c::CURLPROTO_ALL,
            redir_protocols: c::CURLPROTO_HTTP
                | c::CURLPROTO_HTTPS
                | c::CURLPROTO_FTP
                | c::CURLPROTO_FTPS,
            port: 0,
            verbose: false,
            header: false,
            noprogress: true,
            nobody: false,
            failonerror: false,
            upload: false,
            post: false,
            body: Body::None,
            custom_request: None,
            infilesize: -1,
            followlocation: 0,
            maxredirs: 30,
            autoreferer: false,
            postredir: 0,
            unrestricted_auth: false,
            http_version: 0,
            headers: Vec::new(),
            proxy_headers: Vec::new(),
            useragent: None,
            referer: None,
            cookie: None,
            accept_encoding: None,
            content_decoding: true,
            transfer_decoding: true,
            userpwd: None,
            username: None,
            password: None,
            httpauth: c::CURLAUTH_BASIC,
            proxy: None,
            proxyport: 0,
            proxytype: c::CURLPROXY_HTTP,
            proxyuserpwd: None,
            proxyusername: None,
            proxypassword: None,
            proxyauth: c::CURLAUTH_BASIC,
            noproxy: None,
            httpproxytunnel: false,
            timeout_ms: 0,
            connect_timeout_ms: 0,
            expect_100_timeout_ms: 1000,
            range: None,
            resume_from: 0,
            buffersize: 16384,
            resolve: Vec::new(),
            connect_to: Vec::new(),
            interface: None,
            localport: 0,
            ipresolve: 0,
            unix_socket_path: None,
            request_target: None,
            ssl_verifypeer: true,
            ssl_verifyhost: 2,
            sslversion: 0,
            cainfo: None,
            cainfo_blob: None,
            capath: None,
            sslcert: None,
            sslcert_blob: None,
            sslkey: None,
            sslkey_blob: None,
            keypasswd: None,
            issuercert: None,
            issuercert_blob: None,
            certinfo: false,
            filetime: false,
            timecondition: 0,
            timevalue: 0,
            maxfilesize: 0,
            ignore_content_length: false,
            http09_allowed: false,
            path_as_is: false,
            keep_sending_on_error: false,
            forbid_reuse: false,
            fresh_connect: false,
            mime_formescape: false,
            record_header_out: false,
            append: false,
            new_file_perms: 0o644,
            low_speed_limit: 0,
            low_speed_time: 0,
            transfertext: false,
            crlf: false,
        }
    }
}

/// The CURLPROTO_* bit of a scheme (0 for unknown schemes).
pub fn proto_bit(scheme: &str) -> i64 {
    match scheme {
        "http" => c::CURLPROTO_HTTP,
        "https" => c::CURLPROTO_HTTPS,
        "ftp" => c::CURLPROTO_FTP,
        "ftps" => c::CURLPROTO_FTPS,
        "scp" => c::CURLPROTO_SCP,
        "sftp" => c::CURLPROTO_SFTP,
        "telnet" => c::CURLPROTO_TELNET,
        "ldap" => c::CURLPROTO_LDAP,
        "ldaps" => c::CURLPROTO_LDAPS,
        "dict" => c::CURLPROTO_DICT,
        "file" => c::CURLPROTO_FILE,
        "tftp" => c::CURLPROTO_TFTP,
        "imap" => c::CURLPROTO_IMAP,
        "imaps" => c::CURLPROTO_IMAPS,
        "pop3" => c::CURLPROTO_POP3,
        "pop3s" => c::CURLPROTO_POP3S,
        "smtp" => c::CURLPROTO_SMTP,
        "smtps" => c::CURLPROTO_SMTPS,
        "rtsp" => c::CURLPROTO_RTSP,
        "rtmp" => c::CURLPROTO_RTMP,
        "rtmpt" => c::CURLPROTO_RTMPT,
        "rtmpe" => c::CURLPROTO_RTMPE,
        "rtmpte" => c::CURLPROTO_RTMPTE,
        "rtmps" => c::CURLPROTO_RTMPS,
        "rtmpts" => c::CURLPROTO_RTMPTS,
        "gopher" => c::CURLPROTO_GOPHER,
        "gophers" => 1 << 29,
        "smb" => c::CURLPROTO_SMB,
        "smbs" => c::CURLPROTO_SMBS,
        "mqtt" => c::CURLPROTO_MQTT,
        "ws" => 1 << 30,
        "wss" => 1 << 31,
        _ => 0,
    }
}

/// Parses a CURLOPT_PROTOCOLS_STR list ("http,https", "all"): the mask,
/// or None for an unknown or unsupported name (libcurl documentation of
/// CURLOPT_PROTOCOLS_STR).
pub fn parse_protocols_str(s: &[u8]) -> Option<i64> {
    let mut mask = 0i64;
    for part in s.split(|&b| b == b',') {
        let name = String::from_utf8_lossy(part).trim().to_ascii_lowercase();
        if name.is_empty() {
            continue;
        }
        if name == "all" {
            mask = c::CURLPROTO_ALL;
            continue;
        }
        let bit = proto_bit(&name);
        if bit == 0 {
            return None;
        }
        mask |= bit;
    }
    Some(mask)
}
