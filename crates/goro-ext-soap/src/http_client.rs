//! The HTTP request/response exchange of make_http_soap_request()
//! (php_http.c), on top of the standard stream functions.

use std::rc::Rc;

use goro_core::array::{ArrayKey, PhpArray};
use goro_core::object::NativeData;
use goro_core::value::Value;
use goro_core::vm::Vm;

use crate::http::{Url, parse_url};
use crate::php::*;
use crate::types::*;

static URL_HANDLERS: goro_core::vm::NativeObjectHandlers =
    goro_core::vm::NativeObjectHandlers::EMPTY;

const SOAP_SSL_METHOD_TLS: i64 = 0;
const SOAP_SSL_METHOD_SSLV2: i64 = 1;
const SOAP_SSL_METHOD_SSLV3: i64 = 2;
const SOAP_SSL_METHOD_SSLV23: i64 = 3;

const STREAM_CRYPTO_METHOD_SSLV2_CLIENT: i64 = 3;
const STREAM_CRYPTO_METHOD_SSLV3_CLIENT: i64 = 5;
const STREAM_CRYPTO_METHOD_SSLV23_CLIENT: i64 = 57;
const STREAM_CRYPTO_METHOD_TLS_CLIENT: i64 = 121;

/// Call a builtin function by name (None when it does not exist or failed).
fn call(vm: &mut Vm, name: &str, args: &[Value]) -> Option<Value> {
    let f = vm.functions.get(name.as_bytes()).copied()?;
    crate::php::call_adopting(vm, f, args)
}

fn bytes_of(v: &Value) -> Option<Vec<u8>> {
    match v.deref() {
        Value::String(s) => Some(s.as_bytes().to_vec()),
        _ => None,
    }
}

/// php_stream_context_get_option()
fn ctx_option(vm: &mut Vm, ctx: &Option<Value>, wrapper: &str, name: &str) -> Option<Value> {
    let ctx = ctx.as_ref()?;
    let opts = call(vm, "stream_context_get_options", std::slice::from_ref(ctx))?;
    let Value::Array(a) = opts.deref() else {
        return None;
    };
    let a = a.borrow();
    let w = a.get_str(wrapper.as_bytes())?.deref();
    let Value::Array(w) = w else {
        return None;
    };
    let w = w.borrow();
    w.get_str(name.as_bytes()).map(|v| v.deref())
}

struct Stream(Value);

impl Stream {
    fn write(&self, vm: &mut Vm, data: &[u8]) -> bool {
        matches!(call(vm, "fwrite", &[self.0.clone(), vstr(data)]), Some(Value::Long(n)) if n as usize == data.len())
    }
    /// php_stream_gets() with an 8192 byte buffer.
    fn gets(&self, vm: &mut Vm) -> Option<Vec<u8>> {
        bytes_of(&call(vm, "fgets", &[self.0.clone(), Value::Long(8192)])?)
    }
    fn read(&self, vm: &mut Vm, n: usize) -> Option<Vec<u8>> {
        bytes_of(&call(
            vm,
            "fread",
            &[self.0.clone(), Value::Long(n as i64)],
        )?)
        .filter(|b| !b.is_empty())
    }
    fn getc(&self, vm: &mut Vm) -> Option<u8> {
        bytes_of(&call(vm, "fgetc", std::slice::from_ref(&self.0))?)
            .and_then(|b| b.first().copied())
    }
    fn eof(&self, vm: &mut Vm) -> bool {
        !matches!(
            call(vm, "feof", std::slice::from_ref(&self.0)),
            Some(Value::False)
        )
    }
    fn close(&self, vm: &mut Vm) {
        call(vm, "fclose", std::slice::from_ref(&self.0));
    }
}

/// get_http_header_value_nodup(): the value of header `ty` (which
/// includes the colon) as a (start, len) slice of `headers`.
fn header_value_at(headers: &[u8], from: usize, ty: &[u8]) -> Option<(usize, usize)> {
    let mut pos = from;
    loop {
        if headers.len() - pos >= ty.len() && headers[pos..pos + ty.len()].eq_ignore_ascii_case(ty)
        {
            let mut tmp = pos + ty.len();
            while tmp < headers.len() && (headers[tmp] == b' ' || headers[tmp] == b'\t') {
                tmp += 1;
            }
            let mut eol = match headers[tmp..].iter().position(|&c| c == b'\n') {
                None => headers.len(),
                Some(p) => {
                    let mut e = tmp + p;
                    if e > tmp {
                        if headers[e - 1] == b'\r' {
                            e -= 1;
                        }
                        while e > tmp && (headers[e - 1] == b' ' || headers[e - 1] == b'\t') {
                            e -= 1;
                        }
                    }
                    e
                }
            };
            if eol < tmp {
                eol = tmp;
            }
            return Some((tmp, eol - tmp));
        }
        pos += headers[pos..].iter().position(|&c| c == b'\n')? + 1;
    }
}

/// get_http_header_value()
fn header_value(headers: &[u8], ty: &str) -> Option<Vec<u8>> {
    header_value_at(headers, 0, ty.as_bytes()).map(|(s, l)| headers[s..s + l].to_vec())
}

fn starts_with_ci(s: &[u8], p: &str) -> bool {
    s.len() >= p.len() && s[..p.len()].eq_ignore_ascii_case(p.as_bytes())
}

fn atoi(s: &[u8]) -> i64 {
    crate::schema::atoi(&String::from_utf8_lossy(s))
}

/// get_http_headers()
fn get_http_headers(vm: &mut Vm, s: &Stream) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    while let Some(line) = s.gets(vm) {
        if line.starts_with(b"\r\n") || line.starts_with(b"\n") {
            return Some(out);
        }
        // smart_str_appends(): stops at a NUL byte.
        let end = line.iter().position(|&c| c == 0).unwrap_or(line.len());
        out.extend_from_slice(&line[..end]);
    }
    None
}

/// get_http_body()
fn get_http_body(vm: &mut Vm, s: &Stream, close: bool, headers: &[u8]) -> Option<Vec<u8>> {
    let mut header_close = close;
    if !close {
        if let Some(h) = header_value(headers, "Connection:") {
            if starts_with_ci(&h, "close") {
                header_close = true;
            }
        }
    }
    let chunked =
        header_value(headers, "Transfer-Encoding:").is_some_and(|h| starts_with_ci(&h, "chunked"));
    let mut length = 0i64;
    if let Some(h) = header_value(headers, "Content-Length:") {
        length = atoi(&h) as i32 as i64;
        if length == 0 && !chunked {
            return Some(Vec::new());
        }
    }
    let mut buf = Vec::new();
    if chunked {
        loop {
            let line = s.gets(vm).unwrap_or_default();
            let hex: Vec<u8> = line
                .iter()
                .copied()
                .skip_while(|c| c.is_ascii_whitespace())
                .take_while(|c| c.is_ascii_hexdigit())
                .collect();
            if hex.is_empty() {
                return None;
            }
            let size =
                i64::from_str_radix(&String::from_utf8_lossy(&hex), 16).unwrap_or(0) as i32 as i64;
            if size > 0 {
                let mut got = 0i64;
                let mut done = false;
                while got < size {
                    match s.read(vm, (size - got) as usize) {
                        Some(d) => {
                            got += d.len() as i64;
                            buf.extend_from_slice(&d);
                        }
                        None => {
                            done = true;
                            break;
                        }
                    }
                }
                let mut ch = s.getc(vm);
                if ch == Some(b'\r') {
                    ch = s.getc(vm);
                }
                if ch != Some(b'\n') {
                    return None;
                }
                if done {
                    break;
                }
            }
            if size == 0 {
                break;
            }
        }
        while let Some(line) = s.gets(vm) {
            if line.starts_with(b"\r\n") || line.starts_with(b"\n") {
                break;
            }
        }
    } else if length != 0 {
        if !(0..i32::MAX as i64).contains(&length) {
            return None;
        }
        while (buf.len() as i64) < length {
            match s.read(vm, length as usize - buf.len()) {
                Some(d) => buf.extend_from_slice(&d),
                None => break,
            }
        }
    } else if header_close {
        loop {
            if let Some(d) = s.read(vm, 4096) {
                buf.extend_from_slice(&d);
            }
            if s.eof(vm) {
                break;
            }
        }
    } else {
        return None;
    }
    Some(buf)
}

/// http_context_add_header()
fn context_add_header(
    s: &[u8],
    has_auth: bool,
    has_proxy_auth: bool,
    has_cookies: bool,
    out: &mut Vec<u8>,
) {
    let mut i = 0;
    while i < s.len() && s[i] != 0 {
        while i < s.len() && matches!(s[i], b' ' | b'\t' | b'\r' | b'\n') {
            i += 1;
        }
        let start = i;
        let mut p = i;
        let mut name_len: Option<usize> = None;
        while p < s.len() && s[p] != 0 {
            match s[p] {
                b':' => {
                    if name_len.is_none() {
                        name_len = Some(p - start);
                    }
                    break;
                }
                b' ' | b'\t' => {
                    if name_len.is_none() {
                        name_len = Some(p - start);
                    }
                }
                b'\r' | b'\n' => break,
                _ => {}
            }
            p += 1;
        }
        if p < s.len() && s[p] == b':' {
            while p < s.len() && s[p] != 0 && s[p] != b'\r' && s[p] != b'\n' {
                p += 1;
            }
            let name = &s[start..start + name_len.unwrap_or(0)];
            let is = |n: &str| name.eq_ignore_ascii_case(n.as_bytes());
            let skip = is("host")
                || is("connection")
                || is("user-agent")
                || is("content-length")
                || is("content-type")
                || (has_cookies && is("cookie"))
                || (has_auth && is("authorization"))
                || (has_proxy_auth && is("proxy-authorization"));
            if !skip {
                out.extend_from_slice(&s[start..p]);
                out.extend_from_slice(b"\r\n");
            }
        }
        i = if p < s.len() && s[p] != 0 {
            p + 1
        } else {
            s.len()
        };
    }
}

/// http_context_headers()
fn context_headers(
    vm: &mut Vm,
    ctx: &Option<Value>,
    has_auth: bool,
    has_proxy_auth: bool,
    has_cookies: bool,
    out: &mut Vec<u8>,
) {
    match ctx_option(vm, ctx, "http", "header") {
        Some(Value::String(s)) if !s.as_bytes().is_empty() => {
            context_add_header(s.as_bytes(), has_auth, has_proxy_auth, has_cookies, out)
        }
        Some(Value::Array(a)) => {
            let vals: Vec<Value> = a.borrow().values().map(|v| v.deref()).collect();
            for v in vals {
                if let Value::String(s) = v {
                    if !s.as_bytes().is_empty() {
                        context_add_header(
                            s.as_bytes(),
                            has_auth,
                            has_proxy_auth,
                            has_cookies,
                            out,
                        );
                    }
                }
            }
        }
        _ => {}
    }
}

fn base64(vm: &mut Vm, s: &[u8]) -> Vec<u8> {
    call(vm, "base64_encode", &[vstr(s)])
        .and_then(|v| bytes_of(&v))
        .unwrap_or_default()
}

fn md5_hex(vm: &mut Vm, s: &[u8]) -> Vec<u8> {
    call(vm, "md5", &[vstr(s)])
        .and_then(|v| bytes_of(&v))
        .unwrap_or_default()
}

fn str_prop(this: &Obj, name: &str) -> Option<Vec<u8>> {
    bytes_of(&cprop(this, name))
}

/// proxy_authentication()
fn proxy_authentication(vm: &mut Vm, this: &Obj, out: &mut Vec<u8>) -> bool {
    let Some(login) = str_prop(this, "_proxy_login") else {
        return false;
    };
    let mut auth = login;
    auth.push(b':');
    if let Some(p) = str_prop(this, "_proxy_password") {
        auth.extend_from_slice(&p);
    }
    out.extend_from_slice(b"Proxy-Authorization: Basic ");
    out.extend_from_slice(&base64(vm, &auth));
    out.extend_from_slice(b"\r\n");
    true
}

fn drop_socket(vm: &mut Vm, this: &Obj, s: Option<&Stream>) {
    set_cprop(this, "httpsocket", Value::Null);
    if let Some(s) = s {
        s.close(vm);
    }
    set_cprop(this, "httpurl", Value::Null);
    set_cprop(this, "_use_proxy", Value::Null);
}

/// http_connect()
fn http_connect(
    vm: &mut Vm,
    this: &Obj,
    uri: &Url,
    use_ssl: bool,
    ctx: &Option<Value>,
    use_proxy: &mut bool,
) -> Option<Stream> {
    let (host, port) = match (
        cprop(this, "_proxy_host").deref(),
        cprop(this, "_proxy_port").deref(),
    ) {
        (Value::String(h), Value::Long(p)) => {
            *use_proxy = true;
            (h.as_bytes().to_vec(), p)
        }
        _ => (
            uri.host.clone().unwrap_or_default().into_bytes(),
            uri.port as i64,
        ),
    };
    let timeout = match cprop(this, "_connection_timeout").deref() {
        Value::Long(n) if n > 0 => Value::Double(n as f64),
        _ => Value::Null,
    };
    let saved = vm.error_reporting;
    vm.error_reporting &= !(2 | 8 | 512 | 1024);
    let ssl_method = match cprop(this, "_ssl_method").deref() {
        Value::Long(n) => Some(n),
        _ => None,
    };
    let protocol = if use_ssl && !*use_proxy {
        match ssl_method {
            Some(SOAP_SSL_METHOD_TLS) => "tls",
            Some(SOAP_SSL_METHOD_SSLV2) => "sslv2",
            Some(SOAP_SSL_METHOD_SSLV3) => "sslv3",
            _ => "ssl",
        }
    } else {
        "tcp"
    };
    let mut name = format!("{protocol}://").into_bytes();
    name.extend_from_slice(&host);
    name.extend_from_slice(format!(":{port}").as_bytes());
    let ctx_arg = ctx.clone().unwrap_or(Value::Null);
    let r = call(
        vm,
        "stream_socket_client",
        &[
            vstr(&name),
            Value::Null,
            Value::Null,
            timeout,
            Value::Long(4),
            ctx_arg,
        ],
    );
    let mut stream = match r {
        Some(v @ Value::Resource(_)) => Some(Stream(v)),
        _ => None,
    };
    if let (Some(s), true, true) = (&stream, *use_proxy, use_ssl) {
        let s = Stream(s.0.clone());
        let uhost = uri.host.clone().unwrap_or_default();
        if ctx_option(vm, ctx, "ssl", "peer_name").is_none() {
            call(
                vm,
                "stream_context_set_option",
                &[s.0.clone(), vstr("ssl"), vstr("peer_name"), vstr(&uhost)],
            );
        }
        let mut h =
            format!("CONNECT {}:{} HTTP/1.1\r\nHost: {}", uhost, uri.port, uhost).into_bytes();
        if uri.port != 80 {
            h.extend_from_slice(format!(":{}", uri.port).as_bytes());
        }
        h.extend_from_slice(b"\r\n");
        proxy_authentication(vm, this, &mut h);
        h.extend_from_slice(b"\r\n");
        let mut ok = s.write(vm, &h) && get_http_headers(vm, &s).is_some();
        if ok {
            let method = match ssl_method {
                Some(SOAP_SSL_METHOD_TLS) => STREAM_CRYPTO_METHOD_TLS_CLIENT,
                Some(SOAP_SSL_METHOD_SSLV2) => STREAM_CRYPTO_METHOD_SSLV2_CLIENT,
                Some(SOAP_SSL_METHOD_SSLV3) => STREAM_CRYPTO_METHOD_SSLV3_CLIENT,
                Some(SOAP_SSL_METHOD_SSLV23) | None => STREAM_CRYPTO_METHOD_SSLV23_CLIENT,
                Some(_) => STREAM_CRYPTO_METHOD_TLS_CLIENT,
            };
            ok = matches!(
                call(
                    vm,
                    "stream_socket_enable_crypto",
                    &[s.0.clone(), Value::True, Value::Long(method)]
                ),
                Some(Value::True)
            );
        }
        if !ok {
            s.close(vm);
            stream = None;
        }
    }
    vm.error_reporting = saved;
    stream
}

fn in_domain(host: &[u8], domain: &[u8]) -> bool {
    if domain.first() == Some(&b'.') {
        host.len() > domain.len() && host.ends_with(domain)
    } else {
        host == domain
    }
}

fn url_of(vm: &Vm, v: &Value) -> Option<Rc<Url>> {
    let o = is_obj(v)?;
    if !instance_of(vm, &o, "Soap\\Url") {
        return None;
    }
    let b = o.borrow();
    b.native.as_ref()?.data.clone().downcast::<Url>().ok()
}

/// The Soap\Url object kept in SoapClient::$httpurl.
fn new_url_object(vm: &mut Vm, uri: &Url) -> Value {
    let o = new_object(vm, b"Soap\\Url");
    o.borrow_mut().native = Some(NativeData {
        data: Rc::new(uri.clone()),
        handlers: &URL_HANDLERS,
    });
    Value::Object(o)
}

fn digest_get(d: &PhpArray, k: &str) -> Option<Vec<u8>> {
    d.get_str(k.as_bytes()).and_then(bytes_of)
}

fn path_query(uri: &Url, with_fragment: bool) -> Vec<u8> {
    let mut out = uri.path.clone().unwrap_or_else(|| "/".into()).into_bytes();
    if let Some(q) = &uri.query {
        out.push(b'?');
        out.extend_from_slice(q.as_bytes());
    }
    if with_fragment {
        if let Some(f) = &uri.fragment {
            out.push(b'#');
            out.extend_from_slice(f.as_bytes());
        }
    }
    out
}

/// The Authorization: Digest header; None when random_bytes() threw.
fn digest_header(
    vm: &mut Vm,
    this: &Obj,
    digest: &Arr,
    login: &[u8],
    uri: &Url,
) -> Option<Vec<u8>> {
    let nonce = call(vm, "random_bytes", &[Value::Long(16)]).and_then(|v| bytes_of(&v))?;
    let cnonce = call(vm, "bin2hex", &[vstr(&nonce)]).and_then(|v| bytes_of(&v))?;
    let nc = {
        let mut d = digest.borrow_mut();
        match d.get_str(b"nc").map(|v| v.deref()) {
            Some(Value::Long(n)) => {
                d.set(
                    ArrayKey::String(goro_core::string::PhpString::from_bytes(b"nc")),
                    Value::Long(n + 1),
                );
                format!("{:08}", n + 1)
            }
            _ => {
                d.set(
                    ArrayKey::String(goro_core::string::PhpString::from_bytes(b"nc")),
                    Value::Long(1),
                );
                "00000001".to_string()
            }
        }
    };
    let nc = &nc.as_bytes()[..8.min(nc.len())];
    let d = digest.borrow().clone();
    let realm = digest_get(&d, "realm");
    let dnonce = digest_get(&d, "nonce");
    let qop = digest_get(&d, "qop");
    let opaque = digest_get(&d, "opaque");
    let algorithm = digest_get(&d, "algorithm");

    let mut a1 = login.to_vec();
    a1.push(b':');
    a1.extend_from_slice(realm.as_deref().unwrap_or_default());
    a1.push(b':');
    a1.extend_from_slice(&str_prop(this, "_password").unwrap_or_default());
    let mut ha1 = md5_hex(vm, &a1);
    if algorithm
        .as_deref()
        .is_some_and(|a| a.eq_ignore_ascii_case(b"md5-sess"))
    {
        let mut s = ha1.clone();
        s.push(b':');
        s.extend_from_slice(dnonce.as_deref().unwrap_or_default());
        s.push(b':');
        s.extend_from_slice(&cnonce[..8]);
        ha1 = md5_hex(vm, &s);
    }
    let mut a2 = b"POST:".to_vec();
    a2.extend_from_slice(&path_query(uri, false));
    let ha2 = md5_hex(vm, &a2);
    let mut r = ha1.clone();
    r.push(b':');
    r.extend_from_slice(dnonce.as_deref().unwrap_or_default());
    r.push(b':');
    if qop.is_some() {
        r.extend_from_slice(nc);
        r.push(b':');
        r.extend_from_slice(&cnonce[..8]);
        r.extend_from_slice(b":auth:");
    }
    r.extend_from_slice(&ha2);
    let response = md5_hex(vm, &r);

    let mut h = b"Authorization: Digest username=\"".to_vec();
    h.extend_from_slice(login);
    if let Some(v) = &realm {
        h.extend_from_slice(b"\", realm=\"");
        h.extend_from_slice(v);
    }
    if let Some(v) = &dnonce {
        h.extend_from_slice(b"\", nonce=\"");
        h.extend_from_slice(v);
    }
    h.extend_from_slice(b"\", uri=\"");
    h.extend_from_slice(&path_query(uri, true));
    if qop.is_some() {
        h.extend_from_slice(b"\", qop=auth, nc=");
        h.extend_from_slice(nc);
        h.extend_from_slice(b", cnonce=\"");
        h.extend_from_slice(&cnonce[..8]);
    }
    h.extend_from_slice(b"\", response=\"");
    h.extend_from_slice(&response);
    if let Some(v) = &opaque {
        h.extend_from_slice(b"\", opaque=\"");
        h.extend_from_slice(v);
    }
    if let Some(v) = &algorithm {
        h.extend_from_slice(b"\", algorithm=\"");
        h.extend_from_slice(v);
    }
    h.extend_from_slice(b"\"\r\n");
    Some(h)
}

/// Parse the Set-Cookie headers into SoapClient::$_cookies.
fn store_cookies(this: &Obj, headers: &[u8], uri: &Url) {
    let mut from = 0;
    while let Some((start, len)) = header_value_at(headers, from, b"Set-Cookie:") {
        let cookie = &headers[start..start + len];
        let eq = cookie.iter().position(|&c| c == b'=');
        let sem = cookie.iter().position(|&c| c == b';');
        if let Some(eq) = eq.filter(|&e| sem.is_none_or(|s| s > e)) {
            let vlen = match sem {
                Some(s) => s - (eq + 1),
                None => cookie.len() - eq - 1,
            };
            let mut z = PhpArray::new();
            z.set(ArrayKey::Int(0), vstr(&cookie[eq + 1..eq + 1 + vlen]));
            if sem.is_some() {
                // C bug kept: the options start at cookie + value_len + 1.
                let mut o = (vlen + 1).min(cookie.len());
                while o < cookie.len() {
                    while o < cookie.len() && cookie[o] == b' ' {
                        o += 1;
                    }
                    let rest = &cookie[o..];
                    let next = rest.iter().position(|&c| c == b';');
                    let val_end = |skip: usize| {
                        let v = &rest[skip.min(rest.len())..];
                        match next {
                            Some(n) if n >= skip => &rest[skip..n],
                            _ => v,
                        }
                    };
                    if rest.starts_with(b"path=") {
                        z.set(ArrayKey::Int(1), vstr(val_end(5)));
                    } else if rest.starts_with(b"domain=") {
                        z.set(ArrayKey::Int(2), vstr(val_end(7)));
                    } else if rest.starts_with(b"secure") {
                        z.set(ArrayKey::Int(3), Value::True);
                    }
                    match next {
                        Some(n) => o += n + 1,
                        None => break,
                    }
                }
            }
            if z.get_int(1).is_none() {
                let t = uri.path.clone().unwrap_or_else(|| "/".into());
                if let Some(c) = t.rfind('/') {
                    z.set(ArrayKey::Int(1), vstr(&t[..c]));
                }
            }
            if z.get_int(2).is_none() {
                z.set(ArrayKey::Int(2), vstr(uri.host.clone().unwrap_or_default()));
            }
            let cookies = match cprop(this, "_cookies") {
                Value::Array(c) => c,
                _ => new_arr(),
            };
            let cookies = match copy_value(&Value::Array(cookies)) {
                Value::Array(c) => c,
                _ => new_arr(),
            };
            cookies
                .borrow_mut()
                .set(symtable_key(&cookie[..eq]), varr(z));
            set_cprop(this, "_cookies", Value::Array(cookies));
        }
        from = start + len;
    }
}

/// make_http_soap_request() from the URL parse on.
#[allow(clippy::too_many_arguments)]
pub fn request(
    vm: &mut Vm,
    this: &Obj,
    buf: &[u8],
    mut uri: Option<Url>,
    soapaction: Option<&str>,
    soap_version: i64,
    uri_parser_class: Option<&str>,
    want_response: bool,
) -> SR<Option<Value>> {
    let mut request = buf.to_vec();
    let mut headers_z = Vec::new();
    if let Value::Long(c) = cprop(this, "compression").deref() {
        let level = (c & 0x0f).min(9);
        let deflate = c & SOAP_COMPRESSION_DEFLATE != 0;
        if c & SOAP_COMPRESSION_ACCEPT != 0 {
            headers_z.extend_from_slice(b"Accept-Encoding: gzip, deflate\r\n");
        }
        if level > 0 {
            let r = if deflate {
                headers_z.extend_from_slice(b"Content-Encoding: deflate\r\n");
                call(vm, "gzcompress", &[vstr(buf), Value::Long(level)])
            } else {
                headers_z.extend_from_slice(b"Content-Encoding: gzip\r\n");
                call(
                    vm,
                    "gzencode",
                    &[vstr(buf), Value::Long(level), Value::Long(0x1f)],
                )
            };
            match r.as_ref().and_then(bytes_of) {
                Some(b) => request = b,
                None => return Ok(None),
            }
        }
    }

    let mut stream: Option<Stream> = None;
    let mut use_proxy = false;
    if let v @ Value::Resource(_) = cprop(this, "httpsocket").deref() {
        stream = Some(Stream(v));
        if let Value::Long(n) = cprop(this, "_use_proxy").deref() {
            use_proxy = n != 0;
        }
    }
    let ctx = match cprop(this, "_stream_context").deref() {
        v @ Value::Resource(_) => Some(v),
        _ => None,
    };
    let mut redirect_max = 20i64;
    match ctx_option(vm, &ctx, "http", "max_redirects") {
        Some(Value::String(s)) => {
            let t = String::from_utf8_lossy(s.as_bytes()).trim().to_string();
            if let Ok(n) = t.parse::<i64>() {
                redirect_max = n;
            }
        }
        Some(Value::Long(n)) => redirect_max = n,
        _ => {}
    }
    let mut http_msg: Option<Vec<u8>> = None;

    loop {
        let Some(mut u) = uri.take().filter(|u| u.host.is_some()) else {
            add_soap_fault(vm, this, "HTTP", "Unable to parse URL");
            return Ok(None);
        };
        let use_ssl = match u.scheme.as_deref() {
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
        if u.port == 0 {
            u.port = if use_ssl { 443 } else { 80 };
        }
        // Check if the request goes to the same host.
        if let Some(s) = &stream {
            let orig = url_of(vm, &cprop(this, "httpurl"));
            let same = orig.is_some_and(|o| {
                (use_proxy && !use_ssl)
                    || (((use_ssl && o.scheme.as_deref() == Some("https"))
                        || (!use_ssl && o.scheme.as_deref() != Some("https")))
                        && o.host == u.host
                        && o.port == u.port)
            });
            if !same {
                let s = Stream(s.0.clone());
                drop_socket(vm, this, Some(&s));
                stream = None;
                use_proxy = false;
            }
        }
        // Check if the keep-alive connection is still open.
        if let Some(s) = &stream {
            if s.eof(vm) {
                let s = Stream(s.0.clone());
                drop_socket(vm, this, Some(&s));
                stream = None;
                use_proxy = false;
            }
        }
        if stream.is_none() {
            match http_connect(vm, this, &u, use_ssl, &ctx, &mut use_proxy) {
                Some(s) => {
                    set_cprop(this, "httpsocket", s.0.clone());
                    set_cprop(this, "_use_proxy", Value::Long(use_proxy as i64));
                    stream = Some(s);
                }
                None => {
                    add_soap_fault(vm, this, "HTTP", "Could not connect to host");
                    return Ok(None);
                }
            }
        }
        let s = Stream(stream.as_ref().unwrap().0.clone());
        let client_trace = matches!(cprop(this, "trace").deref(), Value::True);
        let url_obj = new_url_object(vm, &u);
        set_cprop(this, "httpurl", url_obj);

        let http_1_1 = !matches!(ctx_option(vm, &ctx, "http", "protocol_version"), Some(Value::Double(d)) if d == 1.0);
        let mut h = b"POST ".to_vec();
        if use_proxy && !use_ssl {
            h.extend_from_slice(
                format!(
                    "{}://{}:{}",
                    u.scheme.as_deref().unwrap_or(""),
                    u.host.as_deref().unwrap_or(""),
                    u.port
                )
                .as_bytes(),
            );
        }
        h.extend_from_slice(&path_query(&u, true));
        h.extend_from_slice(if http_1_1 {
            b" HTTP/1.1\r\n"
        } else {
            b" HTTP/1.0\r\n"
        });
        h.extend_from_slice(b"Host: ");
        h.extend_from_slice(u.host.as_deref().unwrap_or("").as_bytes());
        if u.port != if use_ssl { 443 } else { 80 } {
            h.extend_from_slice(format!(":{}", u.port).as_bytes());
        }
        if !http_1_1 || matches!(cprop(this, "_keep_alive").deref(), Value::False) {
            h.extend_from_slice(b"\r\nConnection: close\r\n");
        } else {
            h.extend_from_slice(b"\r\nConnection: Keep-Alive\r\n");
        }
        let ua = match cprop(this, "_user_agent").deref() {
            Value::String(s) => Some(s.as_bytes().to_vec()),
            _ => match ctx_option(vm, &ctx, "http", "user_agent") {
                Some(Value::String(s)) => Some(s.as_bytes().to_vec()),
                _ => {
                    let ini = crate::sdl::ini_str(vm, "user_agent");
                    if !ini.is_empty() {
                        Some(ini.into_bytes())
                    } else {
                        let ver = vm
                            .constants
                            .get(b"PHP_VERSION".as_slice())
                            .map(|v| v.to_php_string().as_bytes().to_vec())
                            .unwrap_or_default();
                        let mut s = b"PHP-SOAP/".to_vec();
                        s.extend_from_slice(&ver);
                        Some(s)
                    }
                }
            },
        };
        if let Some(ua) = ua.filter(|u| !u.is_empty()) {
            h.extend_from_slice(b"User-Agent: ");
            h.extend_from_slice(&ua);
            h.extend_from_slice(b"\r\n");
        }
        h.extend_from_slice(&headers_z);
        let ctype = match ctx_option(vm, &ctx, "http", "content_type") {
            Some(Value::String(s)) if !s.as_bytes().is_empty() => Some(s.as_bytes().to_vec()),
            _ => None,
        };
        if soap_version == SOAP_1_2 as i64 {
            h.extend_from_slice(b"Content-Type: ");
            h.extend_from_slice(
                ctype
                    .as_deref()
                    .unwrap_or(b"application/soap+xml; charset=utf-8"),
            );
            if let Some(a) = soapaction {
                h.extend_from_slice(b"; action=\"");
                h.extend_from_slice(a.as_bytes());
                h.push(b'"');
            }
            h.extend_from_slice(b"\r\n");
        } else {
            h.extend_from_slice(b"Content-Type: ");
            h.extend_from_slice(ctype.as_deref().unwrap_or(b"text/xml; charset=utf-8"));
            h.extend_from_slice(b"\r\n");
            if let Some(a) = soapaction {
                h.extend_from_slice(b"SOAPAction: \"");
                h.extend_from_slice(a.as_bytes());
                h.extend_from_slice(b"\"\r\n");
            }
        }
        h.extend_from_slice(format!("Content-Length: {}\r\n", request.len()).as_bytes());

        let mut has_auth = false;
        if let Some(login) = str_prop(this, "_login") {
            has_auth = true;
            match cprop(this, "_digest").deref() {
                Value::Array(d) => match digest_header(vm, this, &d, &login, &u) {
                    Some(dh) => h.extend_from_slice(&dh),
                    None => {
                        drop_socket(vm, this, Some(&s));
                        return Ok(None);
                    }
                },
                _ => {
                    let mut auth = login.clone();
                    auth.push(b':');
                    if let Some(p) = str_prop(this, "_password") {
                        auth.extend_from_slice(&p);
                    }
                    h.extend_from_slice(b"Authorization: Basic ");
                    h.extend_from_slice(&base64(vm, &auth));
                    h.extend_from_slice(b"\r\n");
                }
            }
        }
        let mut has_proxy_auth = false;
        if use_proxy && !use_ssl {
            has_proxy_auth = proxy_authentication(vm, this, &mut h);
        }
        let mut has_cookies = false;
        if let Value::Array(c) = cprop(this, "_cookies").deref() {
            let c = c.borrow();
            if !c.is_empty() && !c.looks_packed() {
                has_cookies = true;
                let mut first = true;
                h.extend_from_slice(b"Cookie: ");
                let path = u.path.clone().unwrap_or_else(|| "/".into()).into_bytes();
                let host = u.host.clone().unwrap_or_default().into_bytes();
                for (k, data) in c.iter() {
                    let ArrayKey::String(key) = k else { continue };
                    let Value::Array(data) = data.deref() else {
                        continue;
                    };
                    let data = data.borrow();
                    let Some(value) = data.get_int(0).and_then(bytes_of) else {
                        continue;
                    };
                    let path_ok = match data.get_int(1).map(|v| v.deref()) {
                        Some(Value::String(p)) => path.starts_with(p.as_bytes()),
                        _ => true,
                    };
                    let dom_ok = match data.get_int(2).map(|v| v.deref()) {
                        Some(Value::String(d)) => in_domain(&host, d.as_bytes()),
                        _ => true,
                    };
                    if path_ok && dom_ok && (use_ssl || data.get_int(3).is_none()) {
                        if !first {
                            h.extend_from_slice(b"; ");
                        }
                        first = false;
                        h.extend_from_slice(key.as_bytes());
                        h.push(b'=');
                        h.extend_from_slice(&value);
                    }
                }
                h.extend_from_slice(b"\r\n");
            }
        }
        context_headers(vm, &ctx, has_auth, has_proxy_auth, has_cookies, &mut h);
        h.extend_from_slice(b"\r\n");
        if client_trace {
            set_cprop(this, "__last_request_headers", vstr(&h));
        }
        h.extend_from_slice(&request);
        if !s.write(vm, &h) {
            drop_socket(vm, this, Some(&s));
            add_soap_fault(vm, this, "HTTP", "Failed Sending HTTP SOAP request");
            return Ok(None);
        }

        let mut http_headers = Vec::new();
        let mut http_status = 0i64;
        let mut resp_1_1 = false;
        if want_response || client_trace {
            loop {
                let Some(hd) = get_http_headers(vm, &s) else {
                    set_cprop(this, "httpsocket", Value::Null);
                    s.close(vm);
                    set_cprop(this, "_use_proxy", Value::Null);
                    add_soap_fault(vm, this, "HTTP", "Error Fetching http headers");
                    return Ok(None);
                };
                if client_trace {
                    set_cprop(this, "__last_response_headers", vstr(&hd));
                }
                resp_1_1 = false;
                http_status = 0;
                if let Some(ver) = header_value(&hd, "HTTP/") {
                    if ver.starts_with(b"1.1") {
                        resp_1_1 = true;
                    }
                    if let Some(sp) = ver.iter().position(|&c| c == b' ') {
                        let rest = &ver[sp + 1..];
                        http_status = atoi(rest);
                        if let Some(sp2) = rest.iter().position(|&c| c == b' ') {
                            http_msg = Some(rest[sp2 + 1..].to_vec());
                        }
                    }
                }
                http_headers = hd;
                if http_status != 100 {
                    break;
                }
            }
        }
        if !want_response {
            set_cprop(this, "httpsocket", Value::Null);
            s.close(vm);
            set_cprop(this, "_use_proxy", Value::Null);
            return Ok(Some(vstr("")));
        }

        store_cookies(this, &http_headers, &u);

        let conn_is = |name: &str, what: &str| {
            header_value(&http_headers, name).is_some_and(|c| starts_with_ci(&c, what))
        };
        let http_close = if resp_1_1 {
            (use_proxy && !use_ssl && conn_is("Proxy-Connection:", "close"))
                || conn_is("Connection:", "close")
        } else {
            !((use_proxy && !use_ssl && conn_is("Proxy-Connection:", "Keep-Alive"))
                || conn_is("Connection:", "Keep-Alive"))
        };

        let Some(http_body) = get_http_body(vm, &s, http_close, &http_headers) else {
            set_cprop(this, "httpsocket", Value::Null);
            s.close(vm);
            set_cprop(this, "_use_proxy", Value::Null);
            add_soap_fault(
                vm,
                this,
                "HTTP",
                "Error Fetching http body, No Content-Length, connection closed or chunked data",
            );
            return Ok(None);
        };
        if http_close {
            set_cprop(this, "httpsocket", Value::Null);
            s.close(vm);
            set_cprop(this, "_use_proxy", Value::Null);
            stream = None;
        }

        if (300..400).contains(&http_status) {
            if let Some(loc) = header_value(&http_headers, "Location:") {
                let loc = String::from_utf8_lossy(&loc).to_string();
                if let Some(mut nu) = parse_url(&loc) {
                    if nu.scheme.is_none() {
                        if let Some(p) = nu.path.clone() {
                            if !p.starts_with('/') {
                                if let Some(i) = p.rfind('/') {
                                    nu.path = Some(format!("{}{}", &p[..=i], p));
                                }
                            }
                        }
                    }
                    let _ = uri_parser_class;
                    redirect_max -= 1;
                    if redirect_max < 1 {
                        add_soap_fault(vm, this, "HTTP", "Redirection limit reached, aborting");
                        return Ok(None);
                    }
                    uri = Some(nu);
                    continue;
                }
            }
        } else if http_status == 401 {
            let auth = header_value(&http_headers, "WWW-Authenticate:");
            if let Some(auth) = auth.filter(|a| a.starts_with(b"Digest")) {
                if !matches!(cprop(this, "_digest").deref(), Value::Array(_))
                    && str_prop(this, "_login").is_some()
                    && str_prop(this, "_password").is_some()
                {
                    if let Some(d) = parse_digest(&auth[6..]) {
                        set_cprop(this, "_digest", varr(d));
                        uri = Some(u);
                        continue;
                    }
                }
            }
        }

        let mut content_type_xml = false;
        if let Some(ct) = header_value(&http_headers, "Content-Type:") {
            let cmp = &ct[..ct.iter().position(|&c| c == b';').unwrap_or(ct.len())];
            // strncmp(content_type, X, cmplen) == 0
            let pre = |x: &[u8]| x.starts_with(cmp);
            if pre(b"text/xml") || pre(b"application/soap+xml") {
                content_type_xml = true;
            }
        }
        let body = if let Some(enc) = header_value(&http_headers, "Content-Encoding:") {
            let func = if enc == b"gzip" || enc == b"x-gzip" {
                "gzdecode"
            } else if enc == b"deflate" {
                "gzuncompress"
            } else {
                ""
            };
            if func.is_empty() || !vm.functions.contains_key(func.as_bytes()) {
                add_soap_fault(vm, this, "HTTP", "Unknown Content-Encoding");
                return Ok(None);
            }
            match call(vm, func, &[vstr(&http_body)])
                .as_ref()
                .and_then(bytes_of)
            {
                Some(b) => b,
                None => {
                    add_soap_fault(vm, this, "HTTP", "Can't uncompress compressed response");
                    return Ok(None);
                }
            }
        } else {
            http_body
        };
        if http_status >= 400 {
            let error = if body.is_empty() {
                true
            } else if !content_type_xml {
                let i = body
                    .iter()
                    .position(|&c| c == 0 || (b' '..0x80).contains(&c))
                    .unwrap_or(body.len());
                !body[i..].starts_with(b"<?xml")
            } else {
                false
            };
            if error {
                let msg = http_msg.unwrap_or_default();
                add_soap_fault_ex(vm, this, "HTTP", Some(&msg), None, None, b"en");
                return Ok(None);
            }
        }
        return Ok(Some(vstr(body)));
    }
}

/// The name="value" pairs of a WWW-Authenticate: Digest header.
fn parse_digest(s: &[u8]) -> Option<PhpArray> {
    let mut d: Option<PhpArray> = None;
    let mut i = 0;
    while i < s.len() {
        while i < s.len() && s[i] == b' ' {
            i += 1;
        }
        let name_start = i;
        while i < s.len() && s[i] != b'=' {
            i += 1;
        }
        if i < s.len() && s[i] == b'=' {
            let name = &s[name_start..i];
            i += 1;
            let val = if i < s.len() && s[i] == b'"' {
                i += 1;
                let vs = i;
                while i < s.len() && s[i] != b'"' {
                    i += 1;
                }
                &s[vs..i]
            } else {
                let vs = i;
                while i < s.len() && s[i] != b' ' && s[i] != b',' {
                    i += 1;
                }
                &s[vs..i]
            };
            if i < s.len() {
                if s[i] != b',' {
                    i += 1;
                    while i < s.len() && s[i] != b',' {
                        i += 1;
                    }
                    if i < s.len() {
                        i += 1;
                    }
                } else {
                    i += 1;
                }
            }
            d.get_or_insert_with(PhpArray::new).set(
                ArrayKey::String(goro_core::string::PhpString::from_bytes(name)),
                vstr(val),
            );
        }
    }
    d
}

/// basic_authentication()
fn basic_authentication(vm: &mut Vm, this: &Obj, out: &mut Vec<u8>) -> bool {
    let Some(login) = str_prop(this, "_login") else {
        return false;
    };
    if matches!(cprop(this, "_use_digest").deref(), Value::True) {
        return false;
    }
    let mut auth = login;
    auth.push(b':');
    if let Some(p) = str_prop(this, "_password") {
        auth.extend_from_slice(&p);
    }
    out.extend_from_slice(b"Authorization: Basic ");
    out.extend_from_slice(&base64(vm, &auth));
    out.extend_from_slice(b"\r\n");
    true
}

/// The stream context get_sdl() loads a WSDL with.
pub fn wsdl_context(vm: &mut Vm, this: &Obj, uri: &str) -> Value {
    let mut context: Option<Value> = None;
    let mut headers = Vec::new();
    let mut has_auth = false;
    let mut has_proxy_auth = false;
    if instance_of(vm, this, "SoapClient") {
        if let v @ Value::Resource(_) = cprop(this, "_stream_context").deref() {
            context = Some(v);
        }
        if let Some(ua) = str_prop(this, "_user_agent").filter(|u| !u.is_empty()) {
            headers.extend_from_slice(b"User-Agent: ");
            headers.extend_from_slice(&ua);
            headers.extend_from_slice(b"\r\n");
        }
        if let (Value::String(h), Value::Long(p)) = (
            cprop(this, "_proxy_host").deref(),
            cprop(this, "_proxy_port").deref(),
        ) {
            let mut proxy = b"tcp://".to_vec();
            proxy.extend_from_slice(h.as_bytes());
            proxy.extend_from_slice(format!(":{p}").as_bytes());
            let c = context.get_or_insert_with(|| new_context(vm)).clone();
            set_ctx(vm, &c, "proxy", vstr(&proxy));
            if !(uri.len() >= 8 && uri[..8].eq_ignore_ascii_case("https://")) {
                set_ctx(vm, &c, "request_fulluri", Value::True);
            }
            has_proxy_auth = proxy_authentication(vm, this, &mut headers);
        }
        has_auth = basic_authentication(vm, this, &mut headers);
    }
    let c = match context {
        Some(c) => c,
        None => new_context(vm),
    };
    let ctx = Some(c.clone());
    if ctx_option(vm, &ctx, "http", "protocol_version").is_none() {
        set_ctx(vm, &c, "protocol_version", Value::Double(1.1));
        headers.extend_from_slice(b"Connection: close\r\n");
    }
    if !headers.is_empty() {
        context_headers(vm, &ctx, has_auth, has_proxy_auth, false, &mut headers);
        set_ctx(vm, &c, "header", vstr(&headers));
    }
    c
}

fn new_context(vm: &mut Vm) -> Value {
    call(vm, "stream_context_create", &[]).unwrap_or(Value::Null)
}

fn set_ctx(vm: &mut Vm, c: &Value, name: &str, v: Value) {
    call(
        vm,
        "stream_context_set_option",
        &[c.clone(), vstr("http"), vstr(name), v],
    );
}
