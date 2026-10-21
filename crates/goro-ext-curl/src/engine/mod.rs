//! A pure-Rust emulation of the libcurl 8.14.1 C API (easy + share
//! interfaces) as observable by php-src's ext/curl.
//!
//! The binding layer (a port of ext/curl/interface.c) drives this module
//! exactly like it would drive libcurl: `Easy::setopt()` with the CURLOPT_*
//! numbers, `Easy::perform()` with the PHP callbacks, `Easy::getinfo()`.
//! Protocols implemented: http, https (TLS via purecrypto) and file.
#![allow(dead_code)]

pub mod consts;
mod strerror;

mod content;
mod cookie;
mod easy;
mod file;
mod getinfo;
mod http;
mod httpauth;
mod mime;
mod net;
mod options;
mod proxy;
mod share;
mod tls;
mod trace;
mod transfer;
mod urlapi;
mod version;

use std::cell::RefCell;
use std::rc::Rc;

pub use consts::*;
pub use easy::Easy;
pub use share::Share;
pub use strerror::{easy_strerror, multi_strerror, share_strerror};
pub use version::{VersionInfo, version_info, version_string};

/// CURLE_* numbers from curl.h (CURLE_OK = 0 ...)
pub type CurlCode = u32;
/// CURLM_*
pub type CurlMCode = i32;
/// CURLSHE_*
pub type CurlSHcode = u32;

/// Values passed to Easy::setopt(), by option kind (CURLOPTTYPE_LONG/VALUES
/// -> Long, OFF_T -> OffT, STRINGPOINT -> Str, SLISTPOINT -> Slist, BLOB ->
/// Blob, function pointers -> Callback).
pub enum OptVal<'a> {
    Long(i64),
    OffT(i64),
    /// None = NULL (resets to default)
    Str(Option<&'a [u8]>),
    /// None = NULL
    Slist(Option<Vec<Vec<u8>>>),
    Blob(&'a [u8]),
    /// For *FUNCTION options: whether a callback is installed (the engine
    /// then calls the matching Callbacks method; when not installed the
    /// engine uses libcurl's default behaviour)
    Callback(bool),
    /// CURLOPT_MIMEPOST
    Mime(Option<Mime>),
    /// CURLOPT_SHARE
    Share(Option<Rc<RefCell<Share>>>),
}

/// curl_easy_getinfo() results
#[derive(Clone, Debug, PartialEq)]
pub enum InfoVal {
    Long(i64),
    OffT(i64),
    Double(f64),
    Str(Option<Vec<u8>>),
    Slist(Vec<Vec<u8>>),
    CertInfo(Vec<Vec<Vec<u8>>>),
}

/// What the read callback returned
pub enum ReadResult {
    /// upload data; empty = EOF
    Data(Vec<u8>),
    /// CURL_READFUNC_ABORT
    Abort,
    /// CURL_READFUNC_PAUSE
    Pause,
}

/// CURL_WRITEFUNC_PAUSE
pub const WRITEFUNC_PAUSE: usize = 0x1000_0001;
/// CURL_WRITEFUNC_ERROR
pub const WRITEFUNC_ERROR: usize = 0xFFFF_FFFF;
/// CURL_PROGRESSFUNC_CONTINUE
pub const PROGRESSFUNC_CONTINUE: i64 = 0x1000_0001;

/// The callbacks of a transfer. The PHP layer always installs write, header
/// and read (like PHP does: curl_write/curl_write_header/curl_read are
/// always set), the others only when the corresponding option was set with
/// OptVal::Callback(true).
pub trait Callbacks {
    /// CURLOPT_WRITEFUNCTION: body data. Return the number of bytes taken;
    /// anything else than data.len() aborts with CURLE_WRITE_ERROR,
    /// WRITEFUNC_PAUSE pauses.
    fn write(&mut self, data: &[u8]) -> usize;
    /// CURLOPT_HEADERFUNCTION: one complete header line at a time, CRLF
    /// included (status line, each header, the empty line).
    fn header(&mut self, data: &[u8]) -> usize;
    /// CURLOPT_READFUNCTION: upload data, at most `max` bytes.
    fn read(&mut self, max: usize) -> ReadResult;
    /// CURLOPT_SEEKFUNCTION; return CURL_SEEKFUNC_* (0 ok, 1 fail, 2 cantseek)
    fn seek(&mut self, _offset: i64, _origin: i32) -> i32 {
        2
    }
    /// CURLOPT_XFERINFOFUNCTION / CURLOPT_PROGRESSFUNCTION (only when
    /// NOPROGRESS is 0 and installed): non-zero aborts, PROGRESSFUNC_CONTINUE
    /// means "continue".
    fn progress(&mut self, _dltotal: i64, _dlnow: i64, _ultotal: i64, _ulnow: i64) -> i64 {
        0
    }
    /// CURLOPT_DEBUGFUNCTION (when VERBOSE=1 and installed)
    fn debug(&mut self, _kind: u32, _data: &[u8]) {}
    /// The default verbose trace when no debug callback is installed
    /// (CURLOPT_STDERR, default stderr).
    fn stderr(&mut self, data: &[u8]) {
        use std::io::Write;
        let _ = std::io::stderr().write_all(data);
    }
    /// CURLOPT_PREREQFUNCTION: 0 = ok, anything else aborts
    fn prereq(
        &mut self,
        _primary_ip: &str,
        _local_ip: &str,
        _primary_port: i32,
        _local_port: i32,
    ) -> i32 {
        0
    }
    /// CURLOPT_FNMATCH_FUNCTION
    fn fnmatch(&mut self, _pattern: &[u8], _string: &[u8]) -> i32 {
        1
    }
    /// CURLOPT_SSH_HOSTKEYFUNCTION
    fn ssh_hostkey(&mut self, _keytype: i32, _key: &[u8]) -> i32 {
        0
    }
}

/// curl_mime (built by the PHP layer from CURLOPT_POSTFIELDS arrays and
/// CURLFile / CURLStringFile objects)
#[derive(Default)]
pub struct Mime {
    pub parts: Vec<MimePart>,
}

/// curl_mimepart
pub struct MimePart {
    pub name: Vec<u8>,
    pub data: MimeData,
    /// remote file name
    pub filename: Option<Vec<u8>>,
    pub content_type: Option<Vec<u8>>,
}

/// The data source of a mime part
pub enum MimeData {
    Bytes(Vec<u8>),
    /// read at perform time; missing file -> CURLE_READ_ERROR
    File(std::path::PathBuf),
    Callback {
        size: i64,
        reader: Box<dyn FnMut(usize) -> Vec<u8>>,
    },
}

/// A connection cache, the part of a libcurl multi handle that decides
/// connection reuse and CURLINFO_CONN_ID / CURLINFO_XFER_ID numbering.
/// Every Easy has a private one (like libcurl's per-easy multi); the
/// binding attaches a multi handle's pool with `Easy::set_conn_pool()`.
#[derive(Clone, Default)]
pub struct ConnPool(pub(crate) Rc<RefCell<net::Pool>>);

impl ConnPool {
    pub fn new() -> ConnPool {
        ConnPool::default()
    }
}

/// curl_easy_escape()
pub fn easy_escape(s: &[u8]) -> Vec<u8> {
    urlapi::easy_escape(s)
}

/// curl_easy_unescape()
pub fn easy_unescape(s: &[u8]) -> Vec<u8> {
    urlapi::easy_unescape(s)
}

/// Callbacks that swallow everything (for setopt-time traces)
pub(crate) struct NullCallbacks;

impl Callbacks for NullCallbacks {
    fn write(&mut self, data: &[u8]) -> usize {
        data.len()
    }
    fn header(&mut self, data: &[u8]) -> usize {
        data.len()
    }
    fn read(&mut self, _max: usize) -> ReadResult {
        ReadResult::Data(Vec::new())
    }
    fn stderr(&mut self, _data: &[u8]) {}
}

#[cfg(test)]
mod tests;
