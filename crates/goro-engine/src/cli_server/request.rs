//! One request of the built-in web server, run in its own process: the
//! router script, docroot lookup (index files, PATH_INFO), the PHP script
//! or static file, and the error pages (PHP manual: Built-in web server;
//! behaviour as the sapi/cli/tests php_cli_server_*.phpt tests expect and
//! as observed on PHP 8.5).

use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use goro_runtime::{Key, PhpArray, PhpStr, Value};

use super::Config;
use super::http1::Request;
use crate::Vm;
use crate::builtins::string::{parse_query, register_variable};
use crate::vm::sapi::{Conn, Transport, http_date, now};

/// Methods php -S knows; others get 501 (tests sapi/cli/tests/bug61679,
/// bug69655, bug69953, php_cli_server_020). The list is the usual HTTP
/// and WebDAV/CalDAV method registry.
const METHODS: &[&[u8]] = &[
    b"DELETE",
    b"GET",
    b"HEAD",
    b"POST",
    b"PUT",
    b"CONNECT",
    b"OPTIONS",
    b"TRACE",
    b"COPY",
    b"LOCK",
    b"MKCOL",
    b"MOVE",
    b"PROPFIND",
    b"PROPPATCH",
    b"SEARCH",
    b"UNLOCK",
    b"BIND",
    b"REBIND",
    b"UNBIND",
    b"ACL",
    b"REPORT",
    b"MKACTIVITY",
    b"CHECKOUT",
    b"MERGE",
    b"M-SEARCH",
    b"NOTIFY",
    b"SUBSCRIBE",
    b"UNSUBSCRIBE",
    b"PATCH",
    b"PURGE",
    b"MKCALENDAR",
    b"LINK",
    b"UNLINK",
    b"QUERY",
];

/// Static file types (test sapi/cli/tests/bug61977 for the first ones);
/// text types are sent with "; charset=UTF-8". Unknown types get no
/// Content-Type (observed on PHP 8.5).
fn mime(ext: &[u8]) -> Option<&'static str> {
    Some(match &ext.to_ascii_lowercase()[..] {
        b"html" | b"htm" => "text/html; charset=UTF-8",
        b"svg" => "image/svg+xml",
        b"css" => "text/css; charset=UTF-8",
        b"js" | b"mjs" => "application/javascript",
        b"png" => "image/png",
        b"webm" => "video/webm",
        b"ogv" => "video/ogg",
        b"ogg" => "audio/ogg",
        b"txt" => "text/plain; charset=UTF-8",
        b"xml" => "application/xml",
        b"json" => "application/json",
        b"jpg" | b"jpeg" => "image/jpeg",
        b"gif" => "image/gif",
        b"ico" => "image/x-icon",
        b"webp" => "image/webp",
        b"avif" => "image/avif",
        b"bmp" => "image/bmp",
        b"pdf" => "application/pdf",
        b"wasm" => "application/wasm",
        b"zip" => "application/zip",
        b"mp3" => "audio/mpeg",
        b"mp4" => "video/mp4",
        b"wav" => "audio/wav",
        b"woff" => "font/woff",
        b"woff2" => "font/woff2",
        b"ttf" => "font/ttf",
        b"csv" => "text/csv; charset=UTF-8",
        _ => return None,
    })
}

/// What a request path names under the document root.
struct Target {
    file: PathBuf,
    /// The URL path of the file ("/d/index.php").
    script_name: Vec<u8>,
    /// What follows it in the request path ("/x"), if anything.
    path_info: Vec<u8>,
}

fn join(segs: &[Vec<u8>]) -> Vec<u8> {
    let mut out = Vec::new();
    for s in segs {
        out.push(b'/');
        out.extend_from_slice(s);
    }
    out
}

/// Looks up a request path: the longest prefix that is a file, or a
/// directory with index.php or index.html, the rest being PATH_INFO
/// (tests sapi/cli/tests/php_cli_server_009, php_cli_server_010).
fn resolve(root: &Path, path: &[u8]) -> Option<Target> {
    let mut segs: Vec<Vec<u8>> = Vec::new();
    for s in path.split(|&c| c == b'/') {
        match s {
            b"" | b"." => {}
            b".." => {
                segs.pop();
            }
            s => segs.push(s.to_vec()),
        }
    }
    let trailing = path.ends_with(b"/");
    for i in (0..=segs.len()).rev() {
        let mut p = root.to_path_buf();
        for s in &segs[..i] {
            p.push(std::ffi::OsStr::from_bytes(s));
        }
        let mut info = join(&segs[i..]);
        if trailing && !info.is_empty() {
            info.push(b'/');
        }
        if p.is_file() {
            return Some(Target {
                file: p,
                script_name: join(&segs[..i]),
                path_info: info,
            });
        }
        if p.is_dir() {
            for idx in ["index.php", "index.html"] {
                let f = p.join(idx);
                if f.is_file() {
                    let mut name = join(&segs[..i]);
                    name.push(b'/');
                    name.extend_from_slice(idx.as_bytes());
                    return Some(Target {
                        file: f,
                        script_name: name,
                        path_info: info,
                    });
                }
            }
        }
    }
    None
}

fn html_escape(s: &[u8]) -> Vec<u8> {
    let mut o = Vec::new();
    for &c in s {
        match c {
            b'<' => o.extend_from_slice(b"&lt;"),
            b'>' => o.extend_from_slice(b"&gt;"),
            b'&' => o.extend_from_slice(b"&amp;"),
            b'"' => o.extend_from_slice(b"&quot;"),
            b'\'' => o.extend_from_slice(b"&#039;"),
            c => o.push(c),
        }
    }
    o
}

/// The request being served.
pub struct Ctx<'a> {
    pub cfg: &'a Config,
    pub req: &'a Request,
    pub peer: std::net::SocketAddr,
    /// The path part of the target, percent-decoded.
    pub path: Vec<u8>,
    pub query: Option<Vec<u8>>,
}

impl Ctx<'_> {
    fn head_only(&self) -> bool {
        self.req.method == b"HEAD"
    }

    /// The server's own response (static file or error page).
    fn respond(&self, t: &mut dyn Transport, status: u16, headers: Vec<Vec<u8>>, body: &[u8]) {
        let mut line = self.req.protocol.clone();
        line.extend_from_slice(
            format!(" {} {}", status, crate::vm::sapi::reason(status as i64)).as_bytes(),
        );
        let mut h = Vec::new();
        if let Some(host) = self.req.header(b"host") {
            let mut l = b"Host: ".to_vec();
            l.extend_from_slice(host);
            h.push(l);
        }
        h.push(format!("Date: {}", http_date(now())).into_bytes());
        h.push(b"Connection: close".to_vec());
        h.extend(headers);
        t.write_head(&line, &h);
        if !self.head_only() {
            t.write_body(body);
        }
    }

    /// An error page (tests sapi/cli/tests/php_cli_server_013: the page,
    /// Content-Length, Allow for 405).
    fn error_page(&self, t: &mut dyn Transport, status: u16, msg: &[u8], expose: bool) {
        let reason = crate::vm::sapi::reason(status as i64);
        let mut body = format!(
            "<!doctype html><html><head><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"><title>{} {}</title><style>\nbody {{ margin: 0; font-family: sans-serif; }}\nh1 {{ margin: 0; padding: 0.5em 10px; font-weight: normal; background: #dde; }}\np {{ padding: 0 10px; }}\ncode.url {{ font-family: monospace; }}\n</style>\n</head><body><h1>{}</h1><p>",
            status, reason, reason
        )
        .into_bytes();
        body.extend_from_slice(msg);
        body.extend_from_slice(b"</p></body></html>");
        let mut h = Vec::new();
        if expose {
            h.push(b"X-Powered-By: PHP/8.5.4".to_vec());
        }
        h.push(b"Content-Type: text/html; charset=UTF-8".to_vec());
        h.push(format!("Content-Length: {}", body.len()).into_bytes());
        if status == 405 {
            h.push(b"Allow: GET, HEAD, POST".to_vec());
        }
        self.respond(t, status, h, &body);
    }

    fn not_found(&self, t: &mut dyn Transport, expose: bool) {
        let mut msg = b"The requested resource <code class=\"url\">".to_vec();
        msg.extend_from_slice(&html_escape(&self.path));
        msg.extend_from_slice(b"</code> was not found on this server.");
        self.error_page(t, 404, &msg, expose);
        self.log(404, true);
    }

    fn log(&self, status: u16, missing: bool) {
        let mut m = format!(
            "{} [{}]: {} {}",
            self.peer,
            status,
            String::from_utf8_lossy(&self.req.method),
            String::from_utf8_lossy(&self.req.target)
        );
        if missing {
            m.push_str(" - No such file or directory");
        }
        super::log(&m);
    }

    fn serve_static(&self, t: &mut dyn Transport, file: &Path, expose: bool) {
        if !matches!(&self.req.method[..], b"GET" | b"HEAD" | b"POST") {
            self.error_page(t, 405, b"Requested method not allowed.", expose);
            self.log(405, false);
            return;
        }
        let Ok(data) = std::fs::read(file) else {
            self.not_found(t, expose);
            return;
        };
        let ext = file
            .extension()
            .map(|e| e.as_bytes().to_vec())
            .unwrap_or_default();
        let mut h = Vec::new();
        if let Some(m) = mime(&ext) {
            h.push(format!("Content-Type: {}", m).into_bytes());
        }
        h.push(format!("Content-Length: {}", data.len()).into_bytes());
        self.respond(t, 200, h, &data);
        self.log(200, false);
    }
}

/// The value of `$_SERVER` for a script.
fn server_vars(c: &Ctx, script_name: &[u8], filename: &[u8], path_info: &[u8]) -> PhpArray {
    let s = |v: &[u8]| Value::Str(PhpStr::from(v.to_vec()));
    let mut a = PhpArray::new();
    a.insert(Key::from("DOCUMENT_ROOT"), s(&c.cfg.docroot_str));
    a.insert(
        Key::from("REMOTE_ADDR"),
        s(c.peer.ip().to_string().as_bytes()),
    );
    a.insert(
        Key::from("REMOTE_PORT"),
        s(c.peer.port().to_string().as_bytes()),
    );
    a.insert(
        Key::from("SERVER_SOFTWARE"),
        s(b"PHP/8.5.4 (Development Server)"),
    );
    a.insert(Key::from("SERVER_PROTOCOL"), s(&c.req.protocol));
    a.insert(Key::from("SERVER_NAME"), s(&c.cfg.host));
    a.insert(
        Key::from("SERVER_PORT"),
        s(c.cfg.port.to_string().as_bytes()),
    );
    a.insert(Key::from("REQUEST_URI"), s(&c.req.target));
    a.insert(Key::from("REQUEST_METHOD"), s(&c.req.method));
    a.insert(Key::from("SCRIPT_NAME"), s(script_name));
    a.insert(Key::from("SCRIPT_FILENAME"), s(filename));
    if !path_info.is_empty() {
        a.insert(Key::from("PATH_INFO"), s(path_info));
    }
    let mut self_ = script_name.to_vec();
    self_.extend_from_slice(path_info);
    a.insert(Key::from("PHP_SELF"), s(&self_));
    if let Some(q) = &c.query {
        a.insert(Key::from("QUERY_STRING"), s(q));
    }
    for (n, v) in &c.req.headers {
        let mut k: Vec<u8> = n
            .iter()
            .map(|&b| match b {
                b'-' | b' ' => b'_',
                b => b.to_ascii_uppercase(),
            })
            .collect();
        if k == b"CONTENT_TYPE" || k == b"CONTENT_LENGTH" {
            a.insert(Key::from_bytes(&k), s(v));
        }
        k.splice(0..0, b"HTTP_".iter().copied());
        a.insert(Key::from_bytes(&k), s(v));
    }
    if let Some(auth) = c.req.header(b"authorization") {
        if auth.len() > 6 && auth[..6].eq_ignore_ascii_case(b"basic ") {
            let dec = goro_ext_standard::url::base64_decode(auth[6..].trim_ascii(), false)
                .unwrap_or_default();
            if let Some(p) = dec.iter().position(|&b| b == b':') {
                a.insert(Key::from("PHP_AUTH_USER"), s(&dec[..p]));
                a.insert(Key::from("PHP_AUTH_PW"), s(&dec[p + 1..]));
            }
        } else if auth.len() > 7 && auth[..7].eq_ignore_ascii_case(b"digest ") {
            a.insert(Key::from("PHP_AUTH_DIGEST"), s(auth[7..].trim_ascii()));
        }
    }
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    a.insert(
        Key::from("REQUEST_TIME_FLOAT"),
        Value::Float(t.as_secs_f64()),
    );
    a.insert(Key::from("REQUEST_TIME"), Value::Int(t.as_secs() as i64));
    a
}

/// Sets the superglobals of the request (PHP manual: Predefined
/// Variables, variables_order, request_order).
fn set_globals(vm: &mut Vm, c: &Ctx, server: PhpArray) {
    let seps = vm
        .get_ini(b"arg_separator.input")
        .map(|v| v.to_vec())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| b"&".to_vec());
    let get = c
        .query
        .as_ref()
        .map(|q| parse_query(q, &seps))
        .unwrap_or_default();
    let mut post = PhpArray::new();
    let mut files = PhpArray::new();
    let reading = vm.get_ini(b"enable_post_data_reading").is_none_or(|v| {
        !matches!(
            &v.to_ascii_lowercase()[..],
            b"0" | b"off" | b"" | b"false" | b"no"
        )
    });
    if c.req.method == b"POST" && reading {
        let ct = c.req.header(b"content-type").unwrap_or(b"").to_vec();
        let base = ct
            .split(|&b| b == b';')
            .next()
            .unwrap_or(b"")
            .trim_ascii()
            .to_ascii_lowercase();
        let max = vm
            .get_ini(b"post_max_size")
            .map(|v| crate::vm::setup::parse_size(&v))
            .unwrap_or(8 << 20);
        if max > 0 && c.req.body.len() as i64 > max {
            let msg = format!(
                "PHP Request Startup: POST Content-Length of {} bytes exceeds the limit of {} bytes",
                c.req.body.len(),
                max
            );
            let _ = vm.raise_at(
                crate::vm::errors::E_WARNING,
                &msg,
                PhpStr::from("Unknown"),
                0,
            );
        } else if base == b"application/x-www-form-urlencoded" {
            post = parse_query(&c.req.body, &seps);
        } else if base == b"multipart/form-data"
            && let Some(b) = super::upload::boundary(&ct)
        {
            super::upload::parse(vm, &c.req.body, &b, &mut post, &mut files);
        }
    }
    let mut cookie = PhpArray::new();
    if let Some(ck) = c.req.header(b"cookie") {
        for part in ck.split(|&b| b == b';') {
            let part = part.trim_ascii();
            if part.is_empty() {
                continue;
            }
            let (n, v) = match part.iter().position(|&b| b == b'=') {
                Some(p) => (&part[..p], &part[p + 1..]),
                None => (part, &b""[..]),
            };
            let n = goro_ext_standard::url::urldecode(n);
            // The first cookie of a name wins.
            if cookie.get(Key::from_bytes(&n).as_ref()).is_some() {
                continue;
            }
            let v = goro_ext_standard::url::urldecode(v);
            register_variable(&mut cookie, &n, Value::Str(PhpStr::from(v)));
        }
    }
    let order = vm
        .get_ini(b"request_order")
        .filter(|v| !v.is_empty())
        .or_else(|| vm.get_ini(b"variables_order"))
        .map(|v| v.to_vec())
        .unwrap_or_else(|| b"GPC".to_vec());
    let mut request = PhpArray::new();
    for ch in order.to_ascii_uppercase() {
        let src = match ch {
            b'G' => &get,
            b'P' => &post,
            b'C' => &cookie,
            _ => continue,
        };
        for (k, v) in src.iter() {
            request.insert(k.to_owned(), v.clone());
        }
    }
    let mut env = PhpArray::new();
    if vm
        .get_ini(b"variables_order")
        .is_none_or(|o| o.iter().any(|c| c.eq_ignore_ascii_case(&b'E')))
    {
        for (k, v) in std::env::vars_os() {
            env.insert(
                Key::from_bytes(k.as_bytes()),
                Value::Str(PhpStr::new(v.as_bytes())),
            );
        }
    }
    let g = &mut vm.globals.dyn_vars;
    g.insert(Key::from("_GET"), Value::Array(get));
    g.insert(Key::from("_POST"), Value::Array(post));
    g.insert(Key::from("_COOKIE"), Value::Array(cookie));
    g.insert(Key::from("_FILES"), Value::Array(files));
    g.insert(Key::from("_SERVER"), Value::Array(server));
    g.insert(Key::from("_REQUEST"), Value::Array(request));
    g.insert(Key::from("_ENV"), Value::Array(env));
}

/// Updates `$_SERVER` for the script a router passed the request to.
fn update_server(vm: &mut Vm, c: &Ctx, t: &Target) {
    let fname = t.file.as_os_str().as_bytes().to_vec();
    let s = |v: &[u8]| Value::Str(PhpStr::from(v.to_vec()));
    let mut self_ = t.script_name.clone();
    self_.extend_from_slice(&t.path_info);
    let _ = c;
    if let Some(Value::Array(a)) = vm.globals.dyn_vars.get_mut(Key::from("_SERVER").as_ref()) {
        a.insert(Key::from("SCRIPT_NAME"), s(&t.script_name));
        a.insert(Key::from("SCRIPT_FILENAME"), s(&fname));
        if t.path_info.is_empty() {
            a.remove(Key::from("PATH_INFO").as_ref());
        } else {
            a.insert(Key::from("PATH_INFO"), s(&t.path_info));
        }
        a.insert(Key::from("PHP_SELF"), s(&self_));
    }
}

/// Starts the PHP response on the connection.
fn start_response(vm: &mut Vm, c: &Ctx, stream: std::net::TcpStream) {
    let mime = vm
        .get_ini(b"default_mimetype")
        .map(|v| v.to_vec())
        .unwrap_or_else(|| b"text/html".to_vec());
    let charset = vm.get_ini(b"default_charset").unwrap_or_default();
    let mut default_type = mime.clone();
    if !charset.is_empty() && mime.to_ascii_lowercase().starts_with(b"text/") {
        default_type.extend_from_slice(b"; charset=");
        default_type.extend_from_slice(&charset);
    }
    let expose = vm.get_ini(b"expose_php").is_none_or(|v| {
        !matches!(
            &v.to_ascii_lowercase()[..],
            b"0" | b"off" | b"" | b"false" | b"no"
        )
    });
    let s = &mut vm.out.sapi;
    s.server = true;
    s.status = 200;
    if expose {
        s.headers.push(b"X-Powered-By: PHP/8.5.4".to_vec());
    }
    s.input = c.req.body.clone();
    s.request_headers = c.req.headers.clone();
    s.conn = Some(Conn {
        transport: Box::new(super::http1::H1Transport(stream)),
        protocol: c.req.protocol.clone(),
        host: c.req.header(b"host").map(|h| h.to_vec()),
        head_only: c.req.method == b"HEAD",
        default_type,
        body_bytes: 0,
    });
}

/// Runs a PHP file in the request's engine.
fn run_file(vm: &mut Vm, file: &Path) -> crate::R<Value> {
    let path = file.as_os_str().as_bytes().to_vec();
    let src = match std::fs::read(file) {
        Ok(s) => s,
        Err(_) => return Ok(Value::Null),
    };
    let key = PhpStr::new(&path);
    if vm.included_set.insert(key.clone()) {
        vm.included.push(key);
    }
    vm.script_path = PhpStr::new(&path);
    if let Some(d) = file.parent() {
        vm.cwd = d.to_path_buf();
        let _ = std::env::set_current_dir(d);
    }
    vm.run_main_code_value(&src, &path)
}

/// Ends the PHP response: shutdown, then the headers if no output came.
fn finish(vm: &mut Vm) {
    vm.shutdown();
    vm.out.flush();
    vm.out.sapi.send_head();
    for f in std::mem::take(&mut vm.out.sapi.uploads) {
        let _ = std::fs::remove_file(Path::new(std::ffi::OsStr::from_bytes(&f)));
    }
}

/// Handles one request (in the request's process).
pub fn handle(
    vm: &mut Vm,
    cfg: &Config,
    req: Request,
    stream: std::net::TcpStream,
    peer: std::net::SocketAddr,
) {
    let mut target = &req.target[..];
    // An absolute-form target (RFC 9112 3.2.2): the path follows the
    // authority (observed on PHP 8.5: SCRIPT_NAME is the path).
    let lower = target.to_ascii_lowercase();
    if lower.starts_with(b"http://") || lower.starts_with(b"https://") {
        let after = &target[lower.iter().position(|&c| c == b':').unwrap() + 3..];
        target = match after.iter().position(|&c| c == b'/') {
            Some(p) => &after[p..],
            None => b"/",
        };
    }
    let (raw_path, query) = match target.iter().position(|&c| c == b'?') {
        Some(p) => (&target[..p], Some(target[p + 1..].to_vec())),
        None => (target, None),
    };
    let path = goro_ext_standard::url::rawurldecode(raw_path);
    let c = Ctx {
        cfg,
        req: &req,
        peer,
        path,
        query,
    };
    let expose = vm.get_ini(b"expose_php").is_none_or(|v| {
        !matches!(
            &v.to_ascii_lowercase()[..],
            b"0" | b"off" | b"" | b"false" | b"no"
        )
    });
    let mut t: Box<dyn Transport> = match stream.try_clone() {
        Ok(s) => Box::new(super::http1::H1Transport(s)),
        Err(_) => return,
    };
    if !METHODS.contains(&&req.method[..]) {
        c.error_page(&mut *t, 501, b"Request method not supported.", expose);
        c.log(501, true);
        return;
    }
    if let Some((_, abs)) = &cfg.router {
        // SCRIPT_FILENAME is the router (test
        // sapi/cli/tests/php_cli_server_017 expects a path to it).
        let server = server_vars(&c, &c.path, abs.as_os_str().as_bytes(), b"");
        set_globals(vm, &c, server);
        start_response(vm, &c, stream);
        crate::builtins::output::start_ini_handler(vm);
        let r = run_file(vm, abs);
        let pass = matches!(r, Ok(Value::Bool(false)));
        vm.handle_unwind(r.map(|_| ()));
        if !pass {
            finish(vm);
            return;
        }
        // The router returned false: the request is served as if there
        // was no router, in the same request (test
        // sapi/cli/tests/php_cli_server_012).
        match resolve(&cfg.docroot, &c.path) {
            Some(tg) if tg.file.extension().is_some_and(|e| e == "php") => {
                update_server(vm, &c, &tg);
                let r = run_file(vm, &tg.file);
                vm.handle_unwind(r.map(|_| ()));
                finish(vm);
                c.log(vm.out.sapi.status as u16, false);
            }
            Some(tg) => {
                vm.out.sapi.conn = None;
                c.serve_static(&mut *t, &tg.file, expose);
            }
            None => {
                vm.out.sapi.conn = None;
                c.not_found(&mut *t, expose);
            }
        }
        return;
    }
    match resolve(&cfg.docroot, &c.path) {
        Some(tg) if tg.file.extension().is_some_and(|e| e == "php") => {
            let fname = tg.file.as_os_str().as_bytes().to_vec();
            let server = server_vars(&c, &tg.script_name, &fname, &tg.path_info);
            set_globals(vm, &c, server);
            start_response(vm, &c, stream);
            crate::builtins::output::start_ini_handler(vm);
            let r = run_file(vm, &tg.file);
            vm.handle_unwind(r.map(|_| ()));
            finish(vm);
            c.log(vm.out.sapi.status as u16, false);
        }
        Some(tg) => c.serve_static(&mut *t, &tg.file, expose),
        None if !matches!(&req.method[..], b"GET" | b"HEAD" | b"POST") => {
            c.error_page(&mut *t, 405, b"Requested method not allowed.", expose);
            c.log(405, false);
        }
        None => c.not_found(&mut *t, expose),
    }
}
