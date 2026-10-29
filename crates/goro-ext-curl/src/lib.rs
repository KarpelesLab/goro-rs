//! ext/curl for goro: a URL transfer client written from libcurl's
//! documentation (curl_easy_setopt(3) and the option pages,
//! curl_easy_getinfo(3), libcurl-errors(3)), the HTTP RFCs (RFC 9110,
//! RFC 9112), RFC 6265 (cookies), RFC 7578 (multipart/form-data) and
//! RFC 3986 (URLs), with behaviour details observed on PHP 8.5 linked
//! with libcurl 8.18.
//!
//! The crate knows nothing about PHP values: [`transfer::perform`] runs a
//! transfer described by [`options::Options`], talking to the application
//! through the [`transfer::Host`] trait (write, header, read, progress and
//! debug callbacks), and reports [`info::Info`].
//!
//! Protocols: http, https (TLS on goro-ext-openssl) and file. HTTP and
//! SOCKS 4/4a/5 proxies, CONNECT tunnels, Basic authentication, redirects,
//! cookies, chunked uploads, Expect: 100-continue, gzip/deflate content
//! decoding.

pub mod consts;
pub mod cookie;
pub mod info;
pub mod multipart;
pub mod net;
pub mod options;
pub mod strerror;
pub mod transfer;
pub mod url;

/// What curl_version() reports.
pub mod version {
    /// The libcurl version whose behaviour this crate follows.
    pub const VERSION: &str = "8.18.0";
    /// 0xXXYYZZ of [`VERSION`].
    pub const VERSION_NUM: i64 = 0x081200;
    /// The CURLVERSION_NOW "age" of the version info structure.
    pub const AGE: i64 = 11;
    pub const HOST: &str = "x86_64-pc-linux-gnu";
    pub const SSL_VERSION: &str = "purecrypto/0.10";
    pub const LIBZ_VERSION: &str = "1.3.1";

    /// The feature names of curl_version()["feature_list"] in PHP's order,
    /// their CURL_VERSION_* bits, and whether this implementation has them.
    pub const FEATURES: &[(&str, i64, bool)] = &[
        ("AsynchDNS", 1 << 7, false),
        ("CharConv", 1 << 12, false),
        ("Debug", 1 << 6, false),
        ("GSS-Negotiate", 1 << 5, false),
        ("IDN", 1 << 10, false),
        ("IPv6", 1 << 0, true),
        ("krb4", 1 << 1, false),
        ("Largefile", 1 << 9, true),
        ("libz", 1 << 3, true),
        ("NTLM", 1 << 4, false),
        ("NTLMWB", 1 << 15, false),
        ("SPNEGO", 1 << 8, false),
        ("SSL", 1 << 2, true),
        ("SSPI", 1 << 11, false),
        ("TLS-SRP", 1 << 14, false),
        ("HTTP2", 1 << 16, false),
        ("GSSAPI", 1 << 17, false),
        ("KERBEROS5", 1 << 18, false),
        ("UNIX_SOCKETS", 1 << 19, true),
        ("PSL", 1 << 20, false),
        ("HTTPS_PROXY", 1 << 21, true),
        ("MULTI_SSL", 1 << 22, false),
        ("BROTLI", 1 << 23, false),
        ("ALTSVC", 1 << 24, false),
        ("HTTP3", 1 << 25, false),
        ("UNICODE", 1 << 27, false),
        ("ZSTD", 1 << 26, false),
        ("HSTS", 1 << 28, false),
        ("GSASL", 1 << 29, false),
    ];

    /// The protocols this implementation speaks.
    pub const PROTOCOLS: &[&str] = &["file", "http", "https"];

    /// The features bit mask.
    pub fn features() -> i64 {
        FEATURES.iter().filter(|f| f.2).fold(0, |m, f| m | f.1)
    }
}
