//! curl_version() / curl_version_info() (lib/version.c)

use super::consts::feature;

pub const LIBCURL_VERSION: &str = "8.14.1";
pub const LIBCURL_VERSION_NUM: i64 = 0x080e01;

/// curl_version_info_data for CURLVERSION_NOW
#[derive(Clone, Debug)]
pub struct VersionInfo {
    pub age: i64,
    pub version: String,
    pub version_num: i64,
    pub host: String,
    pub features: i64,
    pub ssl_version: String,
    pub ssl_version_num: i64,
    pub libz_version: String,
    pub protocols: Vec<String>,
    pub ares: Option<String>,
    pub ares_num: i64,
    pub libidn: Option<String>,
    pub iconv_ver_num: i64,
    pub libssh_version: Option<String>,
    pub brotli_ver_num: i64,
    pub brotli_version: Option<String>,
}

/// The CURL_VERSION_* bits of what the engine supports
pub fn features() -> i64 {
    feature::IPV6
        | feature::SSL
        | feature::LIBZ
        | feature::NTLM
        | feature::LARGEFILE
        | feature::THREADSAFE
}

/// curl_version_info(CURLVERSION_NOW)
pub fn version_info() -> VersionInfo {
    VersionInfo {
        age: 11,
        version: LIBCURL_VERSION.to_string(),
        version_num: LIBCURL_VERSION_NUM,
        host: "x86_64-pc-linux-gnu".to_string(),
        features: features(),
        ssl_version: super::tls::backend_version(),
        ssl_version_num: 0,
        libz_version: goro_zlib::ZLIB_VERSION.to_string(),
        protocols: vec!["file".to_string(), "http".to_string(), "https".to_string()],
        ares: None,
        ares_num: 0,
        libidn: None,
        iconv_ver_num: 0,
        libssh_version: None,
        brotli_ver_num: 0,
        brotli_version: None,
    }
}

/// curl_version()
pub fn version_string() -> String {
    format!(
        "libcurl/{LIBCURL_VERSION} {} zlib/{}",
        super::tls::backend_version(),
        goro_zlib::ZLIB_VERSION
    )
}
