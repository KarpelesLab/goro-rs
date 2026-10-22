//! php_http.c: the HTTP transport of SoapClient::__doRequest().

use goro_core::value::Value;
use goro_core::vm::Vm;

use crate::php::*;
use crate::types::*;

pub struct Url {
    pub scheme: Option<String>,
    pub host: Option<String>,
    pub port: u16,
    pub path: Option<String>,
    pub query: Option<String>,
    pub fragment: Option<String>,
}

/// php_url_parse_ex2() as used by the SOAP client (raw components).
pub fn parse_url(s: &str) -> Option<Url> {
    let mut u = Url {
        scheme: None,
        host: None,
        port: 0,
        path: None,
        query: None,
        fragment: None,
    };
    let mut rest = s;
    if let Some(p) = rest.find("://") {
        let sch = &rest[..p];
        if !sch.is_empty()
            && sch
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c))
        {
            u.scheme = Some(sch.to_string());
            rest = &rest[p + 3..];
        } else {
            return None;
        }
    } else {
        return Some(u);
    }
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let mut authority = &rest[..end];
    rest = &rest[end..];
    if let Some(at) = authority.rfind('@') {
        authority = &authority[at + 1..];
    }
    let (host, port) = if authority.starts_with('[') {
        let p = authority.find(']')?;
        (&authority[..=p], authority[p + 1..].strip_prefix(':'))
    } else {
        match authority.rfind(':') {
            Some(p) => (&authority[..p], Some(&authority[p + 1..])),
            None => (authority, None),
        }
    };
    if host.is_empty() {
        return None;
    }
    u.host = Some(host.to_string());
    if let Some(p) = port {
        if !p.is_empty() {
            match p.parse::<u16>() {
                Ok(n) => u.port = n,
                Err(_) => return None,
            }
        }
    }
    let (before_frag, frag) = match rest.find('#') {
        Some(p) => (&rest[..p], Some(&rest[p + 1..])),
        None => (rest, None),
    };
    let (path, query) = match before_frag.find('?') {
        Some(p) => (&before_frag[..p], Some(&before_frag[p + 1..])),
        None => (before_frag, None),
    };
    if !path.is_empty() {
        u.path = Some(path.to_string());
    }
    u.query = query.map(|s| s.to_string());
    u.fragment = frag.map(|s| s.to_string());
    Some(u)
}

/// Decompress a request body (Content-Encoding gzip / deflate).
pub fn inflate_any(vm: &mut Vm, body: &[u8], _enc: &[u8]) -> Option<Vec<u8>> {
    for f in ["gzdecode", "gzuncompress", "gzinflate"] {
        if let Some(func) = vm.functions.get(f.as_bytes()).copied() {
            let saved = vm.error_reporting;
            vm.error_reporting = goro_core::errdisplay::ER_SILENT;
            let r = func(vm, &[vstr(body)]);
            vm.error_reporting = saved;
            if let Ok(Value::String(s)) = r {
                return Some(s.as_bytes().to_vec());
            }
        }
    }
    None
}

/// make_http_soap_request(): Ok(Some(response)) / Ok(None) on failure
/// (a fault is then set on the client).
#[allow(clippy::too_many_arguments)]
pub fn make_http_soap_request(
    vm: &mut Vm,
    this: &Obj,
    buf: &[u8],
    location: &str,
    soapaction: &str,
    soap_version: i64,
    uri_parser_class: Option<&str>,
    want_response: bool,
) -> SR<Option<Value>> {
    if let Some(c) = uri_parser_class {
        let l = c.to_ascii_lowercase();
        if !matches!(
            l.as_str(),
            "uri\\rfc3986\\uri" | "uri\\whatwg\\url" | "parse_url" | ""
        ) {
            let f = crate::zpp::Func::new("SoapClient::__doRequest");
            return Err(f.value_error(vm, 6, "must be a valid URI parser name"));
        }
    }
    let uri = if !location.is_empty() {
        parse_url(location)
    } else {
        None
    };
    let Some(uri) = uri.filter(|u| u.host.is_some()) else {
        add_soap_fault(vm, this, "HTTP", "Unable to parse URL");
        return Ok(None);
    };
    let use_ssl = match uri.scheme.as_deref() {
        Some("https") => true,
        Some("http") => false,
        _ => {
            add_soap_fault(
                vm,
                this,
                "HTTP",
                "Unknown protocol. Only http and https are allowed.",
            );
            return Ok(None);
        }
    };
    crate::http_client::request(
        vm,
        this,
        buf,
        uri,
        use_ssl,
        soapaction,
        soap_version,
        want_response,
    )
}
