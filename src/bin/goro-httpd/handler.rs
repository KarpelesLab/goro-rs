//! Request routing and the PHP request: which script (or static file) a
//! URL maps to, the CGI-style `$_SERVER` variables, and the response built
//! from what the script sent.

use std::net::SocketAddr;
use std::os::unix::ffi::OsStrExt;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use goro::engine::IniSettings;
use goro::websapi::{
    PhpRun, ScriptOutput, ScriptRequest, apply_default_charset, auth_vars, header_var_name,
    mangle_var_name, status_string,
};
use httpsd::{Handler, Request, Response, StaticFiles, StatusCode, Version};

/// PHP_SAPI
const SAPI_NAME: &str = "goro-httpd";

/// The site being served.
pub struct Site {
    docroot: PathBuf,
    ini: IniSettings,
    /// URL path of the script requests for missing files go to.
    front_controller: Option<String>,
    server_name: Option<String>,
    pub max_body: usize,
    pub compress: bool,
    access_log: bool,
    statics: StaticFiles,
}

impl Site {
    pub fn new(
        docroot: PathBuf,
        ini: IniSettings,
        front_controller: Option<String>,
        server_name: Option<String>,
        max_body: usize,
        compress: bool,
        access_log: bool,
    ) -> Site {
        let statics = StaticFiles::new(docroot.clone());
        Site {
            docroot,
            ini,
            front_controller,
            server_name,
            max_body,
            compress,
            access_log,
            statics,
        }
    }
}

/// The handler of one connection's requests.
pub struct PhpHandler {
    site: Arc<Site>,
    peer: SocketAddr,
    local: SocketAddr,
    secure: bool,
}

/// What a request path maps to.
enum Target {
    /// A PHP script: its URL path, file and the PATH_INFO after it.
    Script {
        script_name: Vec<u8>,
        filename: PathBuf,
        path_info: Option<Vec<u8>>,
    },
    /// A file (or a directory with an index.html) served as is.
    Static,
    /// A directory requested without its trailing slash.
    AddSlash,
    NotFound,
    Forbidden,
    BadRequest,
}

/// %XX decoding of a URL path ('+' is not a space in a path).
fn percent_decode(s: &[u8]) -> Vec<u8> {
    let hex = |c: u8| match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    };
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        if s[i] == b'%' && i + 2 < s.len() {
            if let (Some(h), Some(l)) = (hex(s[i + 1]), hex(s[i + 2])) {
                out.push(h << 4 | l);
                i += 3;
                continue;
            }
        }
        out.push(s[i]);
        i += 1;
    }
    out
}

fn is_php(name: &[u8]) -> bool {
    name.len() > 4 && name[name.len() - 4..].eq_ignore_ascii_case(b".php")
}

impl PhpHandler {
    pub fn new(site: Arc<Site>, peer: SocketAddr, local: SocketAddr, secure: bool) -> PhpHandler {
        PhpHandler {
            site,
            peer,
            local,
            secure,
        }
    }

    /// Map a (decoded) URL path to a script or a file, like a web server
    /// configured with `try_files $uri $uri/ /index.php` and PATH_INFO
    /// splitting at the first `.php` component.
    fn resolve(&self, path: &[u8]) -> Target {
        if !path.starts_with(b"/") || path.contains(&0) {
            return Target::BadRequest;
        }
        let segs: Vec<&[u8]> = path
            .split(|&c| c == b'/')
            .filter(|s| !s.is_empty() && *s != b".")
            .collect();
        for s in &segs {
            if *s == b".." {
                return Target::BadRequest;
            }
            // Dotfiles (.htaccess, .git, the SQLite database ...) are never
            // served; /.well-known/ is.
            if s.starts_with(b".") && *s != b".well-known" {
                return Target::Forbidden;
            }
        }
        let mut fs = self.site.docroot.clone();
        let mut url: Vec<u8> = Vec::new();
        for (i, seg) in segs.iter().enumerate() {
            fs.push(std::ffi::OsStr::from_bytes(seg));
            url.push(b'/');
            url.extend_from_slice(seg);
            match std::fs::metadata(&fs) {
                Ok(m) if m.is_dir() => continue,
                Ok(m) if m.is_file() => {
                    if is_php(seg) {
                        let mut rest: Vec<u8> = Vec::new();
                        for s in &segs[i + 1..] {
                            rest.push(b'/');
                            rest.extend_from_slice(s);
                        }
                        if i + 1 < segs.len() && path.ends_with(b"/") {
                            rest.push(b'/');
                        }
                        return Target::Script {
                            script_name: url,
                            filename: fs,
                            path_info: (!rest.is_empty()).then_some(rest),
                        };
                    }
                    if i + 1 == segs.len() {
                        return Target::Static;
                    }
                    return self.front_controller();
                }
                _ => return self.front_controller(),
            }
        }
        // A directory.
        if !segs.is_empty() && !path.ends_with(b"/") {
            return Target::AddSlash;
        }
        let index = fs.join("index.php");
        if index.is_file() {
            url.extend_from_slice(b"/index.php");
            return Target::Script {
                script_name: url,
                filename: index,
                path_info: None,
            };
        }
        if fs.join("index.html").is_file() {
            return Target::Static;
        }
        self.front_controller()
    }

    fn front_controller(&self) -> Target {
        let Some(fc) = &self.site.front_controller else {
            return Target::NotFound;
        };
        let file = self.site.docroot.join(fc.trim_start_matches('/'));
        if file.is_file() {
            Target::Script {
                script_name: fc.as_bytes().to_vec(),
                filename: file,
                path_info: None,
            }
        } else {
            Target::NotFound
        }
    }

    fn dispatch(&self, req: &Request) -> Response {
        let path = percent_decode(req.path().as_bytes());
        match self.resolve(&path) {
            Target::Script {
                script_name,
                filename,
                path_info,
            } => self.run_script(req, &script_name, &filename, path_info.as_deref()),
            Target::Static => self.serve_static(req),
            Target::AddSlash => {
                let mut loc = req.path().to_string();
                loc.push('/');
                if let Some(q) = req.query() {
                    loc.push('?');
                    loc.push_str(q);
                }
                Response::redirect(StatusCode::MOVED_PERMANENTLY, loc)
            }
            Target::NotFound => Response::status(StatusCode::NOT_FOUND),
            Target::Forbidden => Response::status(StatusCode::FORBIDDEN),
            Target::BadRequest => Response::status(StatusCode::BAD_REQUEST),
        }
    }

    fn serve_static(&self, req: &Request) -> Response {
        let mut resp = self.site.statics.handle(req);
        let code = resp.status_code().code();
        if (200..300).contains(&code) || code == 304 {
            // The built-in web server's (php_cli_server) MIME types.
            let path = req.path();
            let name = path.rsplit('/').next().unwrap_or("");
            let ext = name.rsplit_once('.').map(|(_, e)| e).unwrap_or("");
            if let Some(t) = goro::mime_types::lookup(ext.as_bytes()) {
                let t = if t.starts_with("text/") || t.ends_with("javascript") {
                    format!("{}; charset=utf-8", t)
                } else {
                    t.to_string()
                };
                resp.headers_mut().set("Content-Type", t);
            }
        }
        resp
    }

    /// The `$_SERVER` variables of a request (what a web server passes to
    /// php-fpm: nginx' fastcgi_params / Apache's CGI environment).
    fn server_vars(
        &self,
        req: &Request,
        script_name: &[u8],
        filename: &[u8],
        path_info: Option<&[u8]>,
    ) -> Vec<(Vec<u8>, Vec<u8>)> {
        let mut v: Vec<(Vec<u8>, Vec<u8>)> = Vec::with_capacity(48);
        let mut add = |k: &[u8], val: &[u8]| {
            if let Some(k) = mangle_var_name(k) {
                if let Some(e) = v.iter_mut().find(|(a, _)| *a == k) {
                    e.1 = val.to_vec();
                } else {
                    v.push((k, val.to_vec()));
                }
            }
        };
        let host = req.host().unwrap_or("");
        let host_name = match host.strip_prefix('[') {
            // [::1]:8080
            Some(h) => h.split(']').next().unwrap_or(""),
            None => host.rsplit_once(':').map(|(h, _)| h).unwrap_or(host),
        };
        let server_name = match &self.site.server_name {
            Some(n) => n.clone(),
            None if !host_name.is_empty() => host_name.to_string(),
            None => self.local.ip().to_string(),
        };
        let docroot = self.site.docroot.as_os_str().as_bytes();
        let query = req.query().unwrap_or("");
        add(b"PATH", b"/usr/local/bin:/usr/bin:/bin");
        add(b"GATEWAY_INTERFACE", b"CGI/1.1");
        add(
            b"SERVER_SOFTWARE",
            format!("goro-httpd (PHP/{})", goro::PHP_VERSION).as_bytes(),
        );
        add(b"QUERY_STRING", query.as_bytes());
        add(b"REQUEST_METHOD", req.method().as_str().as_bytes());
        if let Some(ct) = req.headers().get("content-type") {
            add(b"CONTENT_TYPE", ct.as_bytes());
        }
        if !req.body().is_empty() || req.headers().contains("content-length") {
            add(b"CONTENT_LENGTH", req.body().len().to_string().as_bytes());
        }
        add(b"SCRIPT_FILENAME", filename);
        add(b"SCRIPT_NAME", script_name);
        if let Some(pi) = path_info {
            add(b"PATH_INFO", pi);
            let mut pt = docroot.to_vec();
            pt.extend_from_slice(pi);
            add(b"PATH_TRANSLATED", &pt);
        }
        add(b"REQUEST_URI", req.target().as_bytes());
        add(b"DOCUMENT_URI", script_name);
        add(b"DOCUMENT_ROOT", docroot);
        add(
            b"SERVER_PROTOCOL",
            match req.version() {
                Version::Http2 => b"HTTP/2.0",
                Version::Http3 => b"HTTP/3.0",
                Version::Http10 => b"HTTP/1.0",
                _ => b"HTTP/1.1",
            },
        );
        add(
            b"REQUEST_SCHEME",
            if self.secure { b"https" } else { b"http" },
        );
        if self.secure {
            add(b"HTTPS", b"on");
        }
        add(b"REMOTE_ADDR", self.peer.ip().to_string().as_bytes());
        add(b"REMOTE_PORT", self.peer.port().to_string().as_bytes());
        add(b"SERVER_ADDR", self.local.ip().to_string().as_bytes());
        add(b"SERVER_PORT", self.local.port().to_string().as_bytes());
        add(b"SERVER_NAME", server_name.as_bytes());
        add(b"REDIRECT_STATUS", b"200");
        let mut php_self = script_name.to_vec();
        if let Some(pi) = path_info {
            php_self.extend_from_slice(pi);
        }
        add(b"PHP_SELF", &php_self);
        // The request headers (HTTP/2 may split the cookies over several
        // fields: they are joined like one Cookie header).
        let mut seen: Vec<String> = Vec::new();
        for (name, _) in req.headers().iter() {
            let lower = name.to_ascii_lowercase();
            // (CONTENT_TYPE / CONTENT_LENGTH only: CGI has no HTTP_ copies)
            if seen.contains(&lower) || lower == "content-type" || lower == "content-length" {
                continue;
            }
            let sep = if lower == "cookie" { "; " } else { ", " };
            let value = req.headers().get_all(name).collect::<Vec<_>>().join(sep);
            let mut key = b"HTTP_".to_vec();
            key.extend_from_slice(&header_var_name(name.as_bytes()));
            add(&key, value.as_bytes());
            seen.push(lower);
        }
        if let Some(auth) = req.headers().get("authorization") {
            let mut a = Vec::new();
            auth_vars(auth.as_bytes(), &mut a);
            v.extend(a);
        }
        v
    }

    fn run_script(
        &self,
        req: &Request,
        script_name: &[u8],
        filename: &std::path::Path,
        path_info: Option<&[u8]>,
    ) -> Response {
        let filename = filename.as_os_str().as_bytes();
        let headers: Vec<(Vec<u8>, Vec<u8>)> = req
            .headers()
            .iter()
            .map(|(k, v)| (k.as_bytes().to_vec(), v.as_bytes().to_vec()))
            .collect();
        let cookie = {
            let c: Vec<&str> = req.headers().get_all("cookie").collect();
            (!c.is_empty()).then(|| c.join("; "))
        };
        let server_vars = self.server_vars(req, script_name, filename, path_info);
        let mut run = PhpRun::startup(&ScriptRequest {
            ini: &self.site.ini,
            sapi: SAPI_NAME,
            method: req.method().as_str().as_bytes(),
            query_string: req.query().unwrap_or("").as_bytes(),
            headers: &headers,
            cookie: cookie.as_deref().map(|c| c.as_bytes()),
            content_type: req.headers().get("content-type").map(|c| c.as_bytes()),
            body: req.body(),
            script_filename: filename.to_vec(),
            path_translated: Some(filename),
            server_vars,
        });
        let result = run.execute_script(filename);
        if self.site.access_log {
            if let Some(e) = run.last_error().filter(|e| e.fatal) {
                eprintln!(
                    "{} {} - {} in {} on line {}",
                    req.method().as_str(),
                    req.target(),
                    e.message,
                    e.file,
                    e.line
                );
            }
        }
        let out = run.shutdown(result);
        script_response(out)
    }
}

/// The status code of a status line ("HTTP/1.1 404 Not Found") or a CGI
/// Status header ("404 Not Found").
fn status_code_of(s: &[u8]) -> Option<u16> {
    let s = if s.len() >= 5 && s[..5].eq_ignore_ascii_case(b"HTTP/") {
        let sp = s.iter().position(|&c| c == b' ')?;
        &s[sp + 1..]
    } else {
        s
    };
    let s = s.trim_ascii_start();
    let digits: Vec<u8> = s.iter().copied().take_while(u8::is_ascii_digit).collect();
    std::str::from_utf8(&digits)
        .ok()?
        .parse::<u16>()
        .ok()
        .filter(|c| (100..1000).contains(c))
}

/// The HTTP response of a script: sapi_send_headers() with the default
/// Content-Type.
fn script_response(out: ScriptOutput) -> Response {
    let mut code = out
        .status_line
        .as_deref()
        .and_then(status_code_of)
        .unwrap_or(if out.code == 0 { 200 } else { out.code as u16 });
    let mut resp = Response::new(StatusCode::OK);
    let mut has_ct = false;
    for (k, v) in &out.headers {
        let name = String::from_utf8_lossy(k);
        if name.eq_ignore_ascii_case("status") {
            // header("Status: 404 Not Found") (CGI)
            if let Some(c) = status_code_of(v) {
                code = c;
            }
            continue;
        }
        let value = if name.eq_ignore_ascii_case("content-type") {
            has_ct = true;
            apply_default_charset(v)
        } else {
            v.clone()
        };
        resp.headers_mut().append(
            name.into_owned(),
            String::from_utf8_lossy(&value).into_owned(),
        );
    }
    if !has_ct && code != 304 && code != 204 {
        resp.headers_mut()
            .append("Content-Type", "text/html; charset=UTF-8");
    }
    resp.with_status(StatusCode::new(code, status_string(code as i32)))
        .body(out.body)
}

impl Handler for PhpHandler {
    fn handle(&self, req: &Request) -> Response {
        let start = Instant::now();
        let resp = self.dispatch(req);
        if self.site.access_log {
            eprintln!(
                "{} [{}]: {} {} {:?} ({:.1} ms)",
                self.peer,
                resp.status_code().code(),
                req.method().as_str(),
                req.target(),
                req.version(),
                start.elapsed().as_secs_f64() * 1000.0
            );
        }
        resp
    }
}
