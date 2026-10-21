//! Built-in HTTP server (the `php -S` mode).
//!
//! Implements a minimal single-threaded HTTP/1.1 server that handles
//! one request at a time and dispatches to PHP scripts via a fresh VM
//! instance per request.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::process;
use std::time::Duration;

use goro_core::array::{ArrayKey, PhpArray};
use goro_core::compiler::Compiler;
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::Vm;
use goro_parser::{Lexer, Parser};

pub struct CliServerOptions {
    pub addr: String,
    pub doc_root: PathBuf,
    pub router: Option<PathBuf>,
}

pub fn run(opts: CliServerOptions) -> ! {
    // Parse address
    let now_str = || {
        let secs = epoch_secs() as i64;
        let (year, month, day, hour, min, sec, _) = gm_time(secs);
        format!(
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
            year, month, day, hour, min, sec
        )
    };
    let bind = match opts.addr.to_socket_addrs() {
        Ok(mut iter) => match iter.next() {
            Some(a) => a,
            None => {
                eprintln!(
                    "[{}] Failed to listen on {} (reason: address resolution failed)",
                    now_str(),
                    opts.addr
                );
                process::exit(1);
            }
        },
        Err(e) => {
            eprintln!(
                "[{}] Failed to listen on {} (reason: {})",
                now_str(),
                opts.addr,
                e
            );
            process::exit(1);
        }
    };
    let listener = match TcpListener::bind(bind) {
        Ok(l) => l,
        Err(e) => {
            eprintln!(
                "[{}] Failed to listen on {} (reason: {})",
                now_str(),
                bind,
                e
            );
            process::exit(1);
        }
    };
    // Determine actual local addr (for port=0)
    let local = listener.local_addr().unwrap_or(bind);
    let host = if let Some(colon) = opts.addr.rfind(':') {
        // Use the host as given so e.g. "localhost" stays "localhost"
        opts.addr[..colon].to_string()
    } else {
        local.ip().to_string()
    };
    let port = local.port();

    // The startup message must match the regex used by php_cli_server.inc:
    //   PHP \S* Development Server \(https?://(.*?:\d+)\) started
    eprintln!(
        "PHP 8.5.4 Development Server (http://{}:{}) started",
        host, port
    );
    // Flush stderr (line-buffered usually but safety first)
    let _ = std::io::stderr().flush();

    // Resolve doc root
    let doc_root = if opts.doc_root.is_absolute() {
        opts.doc_root.clone()
    } else {
        std::env::current_dir()
            .map(|c| c.join(&opts.doc_root))
            .unwrap_or_else(|_| opts.doc_root.clone())
    };

    // Resolve router (relative to doc_root if not absolute)
    let router_abs = opts.router.as_ref().map(|r| {
        if r.is_absolute() {
            r.clone()
        } else {
            doc_root.join(r)
        }
    });

    // SIGTERM/SIGINT handling - we just exit when killed. For tests this is fine.
    for stream in listener.incoming() {
        match stream {
            Ok(s) => {
                let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
                let _ = s.set_write_timeout(Some(Duration::from_secs(5)));
                let _ = s.set_nodelay(true);
                let _ = handle_connection(s, &doc_root, router_abs.as_deref(), &host, port);
            }
            Err(_) => continue,
        }
    }
    process::exit(0);
}

fn handle_connection(
    mut stream: TcpStream,
    doc_root: &Path,
    router: Option<&Path>,
    host: &str,
    port: u16,
) -> std::io::Result<()> {
    // Parse request line + headers
    let peer_addr = stream
        .peer_addr()
        .map(|a| a.ip().to_string())
        .unwrap_or_else(|_| "127.0.0.1".to_string());
    let peer_port = stream.peer_addr().map(|a| a.port()).unwrap_or(0);

    // Read the full request into a buffer, then split into request line,
    // headers, and body. Avoid try_clone(), which has been observed to
    // confuse fd ownership in some forked-process scenarios.
    let mut request_data: Vec<u8> = Vec::with_capacity(1024);
    let mut headers_end: Option<usize> = None;
    let mut buf = [0u8; 1024];
    while headers_end.is_none() {
        let n = stream.read(&mut buf)?;
        if n == 0 {
            // Connection closed before request completed
            return Ok(());
        }
        request_data.extend_from_slice(&buf[..n]);
        // Find end of headers (\r\n\r\n or \n\n)
        if request_data.len() >= 4 {
            for i in 0..=request_data.len() - 4 {
                if &request_data[i..i + 4] == b"\r\n\r\n" {
                    headers_end = Some(i + 4);
                    break;
                }
            }
        }
        if headers_end.is_none() && request_data.len() >= 2 {
            for i in 0..=request_data.len() - 2 {
                if &request_data[i..i + 2] == b"\n\n" {
                    headers_end = Some(i + 2);
                    break;
                }
            }
        }
        // Safety: cap request size to avoid OOM
        if request_data.len() > 64 * 1024 {
            return send_response(
                &mut stream,
                413,
                "Payload Too Large",
                &[],
                b"Headers too large",
            );
        }
    }
    let hdr_end = headers_end.unwrap();
    let header_part = &request_data[..hdr_end];
    let header_text = String::from_utf8_lossy(header_part);
    // Normalize CRLF to LF for parsing — accept either form.
    let header_text_lf: String = header_text.replace("\r\n", "\n");
    let mut lines = header_text_lf.split('\n');
    let request_line = lines
        .next()
        .unwrap_or("")
        .trim_end_matches(['\r', '\n'])
        .to_string();
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("GET").to_string();
    let raw_uri = parts.next().unwrap_or("/").to_string();
    let protocol = parts.next().unwrap_or("HTTP/1.1").to_string();

    // Use an insertion-order vector here; tests iterate $_SERVER and expect
    // HTTP_* entries in the order they appeared in the request.
    let mut header_list: Vec<(String, String)> = Vec::new();
    let mut headers_lc: HashMap<String, String> = HashMap::new();
    for line in lines {
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            continue;
        }
        if let Some(colon) = trimmed.find(':') {
            let name = trimmed[..colon].trim().to_string();
            let value = trimmed[colon + 1..].trim().to_string();
            // Avoid duplicates in the ordered list
            let name_lc = name.to_ascii_lowercase();
            if !headers_lc.contains_key(&name_lc) {
                header_list.push((name.clone(), value.clone()));
            }
            headers_lc.insert(name_lc, value);
        }
    }
    let headers = headers_lc;

    // Body (if Content-Length present, read remaining)
    let mut body: Vec<u8> = request_data[hdr_end..].to_vec();
    if let Some(cl) = headers
        .get("content-length")
        .and_then(|s| s.parse::<usize>().ok())
    {
        while body.len() < cl {
            let need = cl - body.len();
            let mut chunk = vec![0u8; need.min(4096)];
            let n = stream.read(&mut chunk)?;
            if n == 0 {
                break;
            }
            body.extend_from_slice(&chunk[..n]);
        }
        if body.len() > cl {
            body.truncate(cl);
        }
    } else {
        body.clear();
    }

    // Split raw_uri into path and query
    let (path, query) = match raw_uri.find('?') {
        Some(q) => (raw_uri[..q].to_string(), raw_uri[q + 1..].to_string()),
        None => (raw_uri.clone(), String::new()),
    };
    let decoded_path = url_decode(&path);

    // Determine the file to run
    // Strategy:
    //  - If router is set, always run router.
    //  - Else, look for docroot+path. If file exists and is .php, run it.
    //    If file exists and is not .php, serve it as a static file (404 if not).
    //    If file doesn't exist, return 404.
    let target_file: Option<PathBuf>;
    let mut script_name = decoded_path.clone();
    let script_filename: PathBuf;
    let mut path_info: String = String::new();

    if let Some(router_path) = router {
        target_file = Some(router_path.to_path_buf());
        script_filename = router_path.to_path_buf();
        // Try to set SCRIPT_NAME based on the path
        if decoded_path != "/" {
            script_name = decoded_path.clone();
        } else {
            script_name = format!(
                "/{}",
                router_path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
            );
        }
    } else {
        // Try to resolve URI to a real file, splitting the URI into
        // script_name + path_info if a prefix of the URI resolves to a file.
        let rel_path = decoded_path.trim_start_matches('/');
        let candidate = if rel_path.is_empty() {
            doc_root.join("index.php")
        } else {
            doc_root.join(rel_path)
        };
        if candidate.is_file() {
            target_file = Some(candidate.clone());
            script_filename = candidate;
        } else if rel_path.is_empty() && doc_root.join("index.html").is_file() {
            // Static fallback
            serve_static(&mut stream, &doc_root.join("index.html"))?;
            return Ok(());
        } else {
            // Walk the path components looking for an existing file. The
            // matched portion becomes SCRIPT_NAME; the rest becomes PATH_INFO.
            let parts: Vec<&str> = decoded_path.trim_start_matches('/').split('/').collect();
            let mut script_idx: Option<usize> = None;
            let mut script_file: Option<PathBuf> = None;
            for i in (0..parts.len()).rev() {
                let candidate_path: String = parts[..=i].join("/");
                let p = doc_root.join(&candidate_path);
                if p.is_file() {
                    script_idx = Some(i);
                    script_file = Some(p);
                    break;
                }
            }
            if let Some(idx) = script_idx {
                target_file = script_file.clone();
                script_filename = script_file.unwrap();
                script_name = format!("/{}", parts[..=idx].join("/"));
                if idx + 1 < parts.len() {
                    // The remaining parts include an empty trailing element
                    // if decoded_path ended with "/", so just join them.
                    path_info = format!("/{}", parts[idx + 1..].join("/"));
                } else if decoded_path.ends_with('/') && !parts[idx].is_empty() {
                    path_info = "/".to_string();
                }
            } else {
                // Walk up the directory tree looking for an index.php / index.html.
                let mut found: Option<(PathBuf, String)> = None;
                let mut current = std::path::PathBuf::from(rel_path);
                loop {
                    let cur_str = current.to_string_lossy().to_string();
                    let prefix = if cur_str.is_empty() {
                        String::new()
                    } else {
                        format!("/{}", cur_str)
                    };
                    let idx = doc_root.join(&current).join("index.php");
                    if idx.is_file() {
                        found = Some((idx, format!("{}/index.php", prefix)));
                        break;
                    }
                    let idxh = doc_root.join(&current).join("index.html");
                    if idxh.is_file() {
                        serve_static(&mut stream, &idxh)?;
                        return Ok(());
                    }
                    if !current.pop() {
                        // Try docroot directly
                        let idx = doc_root.join("index.php");
                        if idx.is_file() {
                            found = Some((idx, "/index.php".to_string()));
                        }
                        break;
                    }
                }
                if let Some((f, found_script_name)) = found {
                    target_file = Some(f.clone());
                    script_filename = f;
                    script_name = found_script_name.clone();
                    // PATH_INFO = rest of original URI after the matched script name
                    // The decoded_path doesn't contain index.php here (since we walked up).
                    // PATH_INFO = decoded_path minus the script's directory portion.
                    let script_dir = std::path::Path::new(&found_script_name)
                        .parent()
                        .map(|p| p.to_string_lossy().to_string())
                        .unwrap_or_else(|| "/".to_string());
                    let dir = if script_dir.is_empty() {
                        "/".to_string()
                    } else {
                        script_dir
                    };
                    let dir_trim = dir.trim_end_matches('/');
                    if let Some(suffix) = decoded_path.strip_prefix(dir_trim) {
                        path_info = suffix.to_string();
                        if path_info.is_empty() {
                            path_info = "/".to_string();
                        }
                    } else {
                        path_info = decoded_path.clone();
                    }
                } else {
                    return send_response(&mut stream, 404, "Not Found", &[], b"Not Found");
                }
            }
        }
    }

    let target = match target_file {
        Some(t) => t,
        None => {
            return send_response(&mut stream, 404, "Not Found", &[], b"Not Found");
        }
    };

    // If the target is not a PHP file, serve it as static.
    if let Some(ext) = target.extension().and_then(|e| e.to_str()) {
        if !ext.eq_ignore_ascii_case("php") && router.is_none() {
            return serve_static(&mut stream, &target);
        }
    } else if router.is_none() {
        // No extension - serve as static
        return serve_static(&mut stream, &target);
    }

    // Read PHP source
    let source = match std::fs::read(&target) {
        Ok(b) => b,
        Err(_) => {
            return send_response(&mut stream, 404, "Not Found", &[], b"Not Found");
        }
    };

    // Execute and capture
    let (status, response_headers, output) = execute_php_request(
        &source,
        &target,
        host,
        port,
        &peer_addr,
        peer_port,
        &method,
        &raw_uri,
        &decoded_path,
        &query,
        &protocol,
        &script_name,
        &script_filename,
        &path_info,
        &headers,
        &header_list,
        &body,
        doc_root,
    );

    // PHP's cli-server emits HTTP/1.0 for fatal-error (500) responses.
    let resp_proto = if status == 500 {
        "HTTP/1.0"
    } else {
        protocol.as_str()
    };
    send_response_with_host_proto(
        &mut stream,
        status,
        status_text(status),
        &response_headers,
        &output,
        host,
        port,
        resp_proto,
    )?;
    Ok(())
}

fn serve_static(stream: &mut TcpStream, path: &Path) -> std::io::Result<()> {
    match std::fs::read(path) {
        Ok(content) => {
            let ct = content_type(path);
            send_response(
                stream,
                200,
                "OK",
                &[(b"Content-Type".to_vec(), ct.into_bytes())],
                &content,
            )
        }
        Err(_) => send_response(stream, 404, "Not Found", &[], b"Not Found"),
    }
}

fn content_type(path: &Path) -> String {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let ct = match ext.as_str() {
        "html" | "htm" => "text/html",
        "css" => "text/css",
        "js" => "application/javascript",
        "json" => "application/json",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "txt" => "text/plain",
        _ => "application/octet-stream",
    };
    ct.to_string()
}

fn send_response(
    stream: &mut TcpStream,
    status: u16,
    status_msg: &str,
    headers: &[(Vec<u8>, Vec<u8>)],
    body: &[u8],
) -> std::io::Result<()> {
    send_response_with_host_proto(stream, status, status_msg, headers, body, "", 0, "HTTP/1.1")
}

fn send_response_with_host_proto(
    stream: &mut TcpStream,
    status: u16,
    status_msg: &str,
    headers: &[(Vec<u8>, Vec<u8>)],
    body: &[u8],
    host: &str,
    port: u16,
    proto: &str,
) -> std::io::Result<()> {
    let mut buf = Vec::new();
    write!(buf, "{} {} {}\r\n", proto, status, status_msg)?;
    // Build a set of headers we will emit. Mimic PHP's built-in server order:
    //   Host, Date, Connection, X-Powered-By, Content-type, Content-Length
    let mut have_ct = false;
    let mut have_cl = false;
    let mut have_host = false;
    let mut have_date = false;
    let mut have_xpb = false;
    let mut have_conn = false;
    for (k, _v) in headers {
        if k.eq_ignore_ascii_case(b"Content-Type") {
            have_ct = true;
        }
        if k.eq_ignore_ascii_case(b"Content-Length") {
            have_cl = true;
        }
        if k.eq_ignore_ascii_case(b"Host") {
            have_host = true;
        }
        if k.eq_ignore_ascii_case(b"Date") {
            have_date = true;
        }
        if k.eq_ignore_ascii_case(b"X-Powered-By") {
            have_xpb = true;
        }
        if k.eq_ignore_ascii_case(b"Connection") {
            have_conn = true;
        }
    }
    if !have_host && !host.is_empty() {
        // PHP cli-server emits the request's Host header value back. Defer
        // to that if the request supplied one (passed through via host: just
        // the hostname; port appended if standard ports unused).
        write!(buf, "Host: {}:{}\r\n", host, port)?;
    }
    if !have_date {
        write!(buf, "Date: {}\r\n", http_date())?;
    }
    if !have_conn {
        buf.extend_from_slice(b"Connection: close\r\n");
    }
    if !have_xpb {
        buf.extend_from_slice(b"X-Powered-By: PHP/8.5.4\r\n");
    }
    // Emit custom headers (after the standard ones)
    for (k, v) in headers {
        buf.extend_from_slice(k);
        buf.extend_from_slice(b": ");
        buf.extend_from_slice(v);
        buf.extend_from_slice(b"\r\n");
    }
    if !have_ct {
        buf.extend_from_slice(b"Content-type: text/html; charset=UTF-8\r\n");
    }
    // Note: PHP's built-in server does not emit Content-Length; the client
    // detects EOF via Connection: close. Mirror that exactly so tests that
    // match the response headers exactly continue to work.
    let _ = have_cl;
    buf.extend_from_slice(b"\r\n");
    buf.extend_from_slice(body);
    stream.write_all(&buf)?;
    stream.flush()?;
    Ok(())
}

fn http_date() -> String {
    // Format: "Wed, 21 Oct 2015 07:28:00 GMT"
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0) as i64;
    // Simple gmtime conversion
    let (year, month, day, hour, min, sec, wday) = gm_time(secs);
    let days = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    let months = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    format!(
        "{}, {:02} {} {:04} {:02}:{:02}:{:02} GMT",
        days[wday as usize],
        day,
        months[(month - 1) as usize],
        year,
        hour,
        min,
        sec
    )
}

// Returns (year, month, day, hour, min, sec, wday). wday: 0=Sunday.
fn gm_time(secs: i64) -> (i32, i32, i32, i32, i32, i32, i32) {
    let mut t = secs;
    let sec = (t.rem_euclid(60)) as i32;
    t = t.div_euclid(60);
    let min = (t.rem_euclid(60)) as i32;
    t = t.div_euclid(60);
    let hour = (t.rem_euclid(24)) as i32;
    let mut days = t.div_euclid(24);
    // wday: 1970-01-01 was Thursday (4).
    let wday = ((days + 4).rem_euclid(7)) as i32;
    let mut year = 1970i32;
    while days >= 365 {
        let dy = if is_leap(year) { 366 } else { 365 };
        if days >= dy {
            days -= dy;
            year += 1;
        } else {
            break;
        }
    }
    let month_days = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut m = 0i32;
    while m < 12 {
        let mut dm = month_days[m as usize] as i64;
        if m == 1 && is_leap(year) {
            dm = 29;
        }
        if days < dm {
            break;
        }
        days -= dm;
        m += 1;
    }
    let month = m + 1;
    let day = (days + 1) as i32;
    (year, month, day, hour, min, sec, wday)
}

fn is_leap(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

fn status_text(code: u16) -> &'static str {
    match code {
        200 => "OK",
        201 => "Created",
        204 => "No Content",
        301 => "Moved Permanently",
        302 => "Found",
        303 => "See Other",
        304 => "Not Modified",
        307 => "Temporary Redirect",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        500 => "Internal Server Error",
        501 => "Not Implemented",
        503 => "Service Unavailable",
        _ => "OK",
    }
}

fn url_decode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let h1 = (bytes[i + 1] as char).to_digit(16);
            let h2 = (bytes[i + 2] as char).to_digit(16);
            if let (Some(a), Some(b)) = (h1, h2) {
                out.push(((a << 4) | b) as u8 as char);
                i += 3;
                continue;
            }
        }
        if bytes[i] == b'+' {
            out.push(' ');
            i += 1;
            continue;
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn execute_php_request(
    source: &[u8],
    target_path: &Path,
    host: &str,
    port: u16,
    remote_addr: &str,
    remote_port: u16,
    method: &str,
    request_uri: &str,
    _path: &str,
    query: &str,
    protocol: &str,
    script_name: &str,
    script_filename: &Path,
    path_info: &str,
    headers: &HashMap<String, String>,
    header_list: &[(String, String)],
    body: &[u8],
    doc_root: &Path,
) -> (u16, Vec<(Vec<u8>, Vec<u8>)>, Vec<u8>) {
    // Strip BOM and PHP open tags handled by parser already.
    let filename = target_path.to_string_lossy().to_string();
    let mut lexer = Lexer::new(source);
    let tokens = lexer.tokenize();
    let mut parser = Parser::new(tokens).with_source(source);
    let program = match parser.parse() {
        Ok(p) => p,
        Err(e) => {
            let msg = format!("Parse error: {}", e);
            return (500, Vec::new(), msg.into_bytes());
        }
    };
    let mut compiler = Compiler::new();
    compiler.source_file = filename.as_bytes().to_vec();
    let (op_array, compiled_classes, _warnings) = match compiler.compile(&program) {
        Ok(r) => r,
        Err(e) => {
            let msg = format!("Compile error: {} at line {}", e.message, e.line);
            return (500, Vec::new(), msg.into_bytes());
        }
    };

    let mut vm = Vm::new();
    vm.current_file = filename.clone();
    // Register extensions
    goro_ext_standard::register_standard_functions(&mut vm);
    goro_ext_date::register(&mut vm);
    goro_ext_json::register(&mut vm);
    goro_ext_ctype::register(&mut vm);
    goro_ext_hash::register(&mut vm);
    goro_ext_openssl::register(&mut vm);
    goro_ext_zlib::register(&mut vm);
    goro_ext_gmp::register(&mut vm);
    goro_ext_bz2::register(&mut vm);
    goro_ext_curl::register(&mut vm);
    goro_ext_xml::register(&mut vm);
    goro_ext_session::register(&mut vm);
    goro_ext_mysqli::register(&mut vm);
    goro_ext_sockets::register(&mut vm);
    goro_ext_mbstring::register(&mut vm);
    goro_ext_spl::register(&mut vm);
    goro_ext_reflection::register(&mut vm);
    for class in compiled_classes {
        vm.register_class(class);
    }

    // Build $_SERVER
    let mut server = PhpArray::new();
    let host_with_port = if (port == 80) || port == 443 {
        host.to_string()
    } else {
        format!("{}:{}", host, port)
    };
    set_kv(
        &mut server,
        b"DOCUMENT_ROOT",
        doc_root.to_string_lossy().as_bytes(),
    );
    set_kv(&mut server, b"REMOTE_ADDR", remote_addr.as_bytes());
    set_kv(
        &mut server,
        b"REMOTE_PORT",
        remote_port.to_string().as_bytes(),
    );
    set_kv(
        &mut server,
        b"SERVER_SOFTWARE",
        b"PHP/8.5.4 (Development Server)",
    );
    set_kv(&mut server, b"SERVER_PROTOCOL", protocol.as_bytes());
    set_kv(&mut server, b"SERVER_NAME", host.as_bytes());
    set_kv(&mut server, b"SERVER_PORT", port.to_string().as_bytes());
    set_kv(&mut server, b"REQUEST_URI", request_uri.as_bytes());
    set_kv(&mut server, b"REQUEST_METHOD", method.as_bytes());
    set_kv(&mut server, b"SCRIPT_NAME", script_name.as_bytes());
    set_kv(
        &mut server,
        b"SCRIPT_FILENAME",
        script_filename.to_string_lossy().as_bytes(),
    );
    set_kv(&mut server, b"PATH_INFO", path_info.as_bytes());
    if !path_info.is_empty() {
        let translated = format!("{}{}", doc_root.to_string_lossy(), path_info);
        set_kv(&mut server, b"PATH_TRANSLATED", translated.as_bytes());
    }
    // PHP_SELF includes script_name + path_info
    if path_info.is_empty() {
        set_kv(&mut server, b"PHP_SELF", script_name.as_bytes());
    } else {
        let php_self = format!("{}{}", script_name, path_info);
        set_kv(&mut server, b"PHP_SELF", php_self.as_bytes());
    }
    set_kv(&mut server, b"QUERY_STRING", query.as_bytes());
    set_kv(&mut server, b"HTTP_HOST", host_with_port.as_bytes());
    set_kv(&mut server, b"GATEWAY_INTERFACE", b"CGI/1.1");
    set_kv(
        &mut server,
        b"REQUEST_TIME",
        &epoch_secs().to_string().into_bytes(),
    );
    set_kv_float(&mut server, b"REQUEST_TIME_FLOAT", epoch_secs_f64());

    // HTTP_* from headers - iterate in insertion order
    for (k, v) in header_list {
        let upper = k.to_ascii_uppercase().replace('-', "_");
        // Content-Type and Content-Length are also set without HTTP_ prefix
        if k.eq_ignore_ascii_case("content-type") {
            set_kv(&mut server, b"CONTENT_TYPE", v.as_bytes());
        }
        if k.eq_ignore_ascii_case("content-length") {
            set_kv(&mut server, b"CONTENT_LENGTH", v.as_bytes());
        }
        let key = format!("HTTP_{}", upper);
        set_kv(&mut server, key.as_bytes(), v.as_bytes());
    }
    // Parse Authorization: Basic <base64> into PHP_AUTH_USER / PHP_AUTH_PW
    if let Some(auth) = headers.get("authorization") {
        if let Some(b64) = auth
            .strip_prefix("Basic ")
            .or_else(|| auth.strip_prefix("basic "))
        {
            if let Some(decoded) = base64_decode(b64.trim()) {
                if let Ok(s) = std::str::from_utf8(&decoded) {
                    if let Some(colon) = s.find(':') {
                        set_kv(&mut server, b"PHP_AUTH_USER", &s.as_bytes()[..colon]);
                        set_kv(&mut server, b"PHP_AUTH_PW", &s.as_bytes()[colon + 1..]);
                    }
                }
            }
        } else if let Some(rest) = auth
            .strip_prefix("Digest ")
            .or_else(|| auth.strip_prefix("digest "))
        {
            set_kv(&mut server, b"PHP_AUTH_DIGEST", rest.as_bytes());
        }
    }
    vm.set_global(
        b"_SERVER".to_vec(),
        Value::Array(std::rc::Rc::new(std::cell::RefCell::new(server))),
    );

    // Build $_GET
    let get_arr = parse_query_string(query);
    vm.set_global(
        b"_GET".to_vec(),
        Value::Array(std::rc::Rc::new(std::cell::RefCell::new(get_arr.clone()))),
    );

    // Build $_POST and $_FILES
    let mut post_arr = PhpArray::new();
    let mut files_arr = PhpArray::new();
    let ct = headers.get("content-type").cloned().unwrap_or_default();
    if method == "POST" {
        if ct
            .to_ascii_lowercase()
            .starts_with("application/x-www-form-urlencoded")
        {
            let body_str = String::from_utf8_lossy(body).to_string();
            post_arr = parse_query_string(&body_str);
        } else if ct.to_ascii_lowercase().starts_with("multipart/form-data") {
            // Extract boundary
            if let Some(bidx) = ct.to_ascii_lowercase().find("boundary=") {
                let boundary_raw = &ct[bidx + "boundary=".len()..];
                let boundary = boundary_raw.trim_matches('"').trim().to_string();
                parse_multipart(body, &boundary, &mut post_arr, &mut files_arr);
            }
        }
    }
    vm.set_global(
        b"_POST".to_vec(),
        Value::Array(std::rc::Rc::new(std::cell::RefCell::new(post_arr.clone()))),
    );
    vm.set_global(
        b"_FILES".to_vec(),
        Value::Array(std::rc::Rc::new(std::cell::RefCell::new(files_arr))),
    );

    // Build $_COOKIE
    let mut cookie_arr = PhpArray::new();
    if let Some(c) = headers.get("cookie") {
        for piece in c.split(';') {
            let piece = piece.trim();
            if let Some(eq) = piece.find('=') {
                let k = url_decode(&piece[..eq]);
                let v = url_decode(&piece[eq + 1..]);
                cookie_arr.set(
                    ArrayKey::String(PhpString::from_string(k)),
                    Value::String(PhpString::from_string(v)),
                );
            }
        }
    }
    vm.set_global(
        b"_COOKIE".to_vec(),
        Value::Array(std::rc::Rc::new(std::cell::RefCell::new(
            cookie_arr.clone(),
        ))),
    );

    // Build $_REQUEST = GET + POST + COOKIE (in that order)
    let mut request_arr = PhpArray::new();
    for (k, v) in get_arr.iter() {
        request_arr.set(k.clone(), v.clone());
    }
    for (k, v) in post_arr.iter() {
        request_arr.set(k.clone(), v.clone());
    }
    for (k, v) in cookie_arr.iter() {
        request_arr.set(k.clone(), v.clone());
    }
    vm.set_global(
        b"_REQUEST".to_vec(),
        Value::Array(std::rc::Rc::new(std::cell::RefCell::new(request_arr))),
    );

    // $_ENV from env
    let mut env_arr = PhpArray::new();
    for (k, v) in std::env::vars() {
        env_arr.set(
            ArrayKey::String(PhpString::from_string(k)),
            Value::String(PhpString::from_string(v)),
        );
    }
    vm.set_global(
        b"_ENV".to_vec(),
        Value::Array(std::rc::Rc::new(std::cell::RefCell::new(env_arr))),
    );

    // Provide raw body as php://input -- not implemented yet but we can store
    // it on env so getenv("HTTP_RAW_POST_DATA") might work; we punt.

    // Execute
    // Clear any headers/code from a previous request in this process
    let _ = goro_ext_standard::misc::take_sent_headers();
    let _ = goro_ext_standard::misc::take_response_code();
    // Make the request body readable via php://input
    goro_ext_standard::misc::set_request_body(body.to_vec());
    let exec_result = vm.execute(&op_array);
    let output = vm.take_output();
    let response_code = goro_ext_standard::misc::take_response_code();
    let status: u16 = if response_code > 0 {
        response_code as u16
    } else {
        200
    };
    let response_headers: Vec<(Vec<u8>, Vec<u8>)> = goro_ext_standard::misc::take_sent_headers();

    if let Err(_e) = exec_result {
        // Uncaught fatal error => HTTP 500 per PHP cli-server behaviour.
        let mut body_out = output;
        if let Some(exc) = vm.current_exception.take() {
            if let Value::Object(obj) = &exc {
                let obj = obj.borrow();
                let class = String::from_utf8_lossy(&obj.class_name).to_string();
                let msg = obj
                    .get_property(b"message")
                    .to_php_string()
                    .to_string_lossy();
                let line = obj.get_property(b"line").to_long();
                let conn = goro_core::vm::Vm::exception_location_connector(&msg);
                let fatal = format!(
                    "\nFatal error: Uncaught {}: {}{}{}:{}\nStack trace:\n#0 {{main}}\n  thrown in {} on line {}\n",
                    class, msg, conn, filename, line, filename, line
                );
                body_out.extend_from_slice(fatal.as_bytes());
            }
        }
        return (500, response_headers, body_out);
    }
    (status, response_headers, output)
}

fn set_kv(arr: &mut PhpArray, key: &[u8], val: &[u8]) {
    arr.set(
        ArrayKey::String(PhpString::from_bytes(key)),
        Value::String(PhpString::from_bytes(val)),
    );
}
fn set_kv_float(arr: &mut PhpArray, key: &[u8], val: f64) {
    arr.set(
        ArrayKey::String(PhpString::from_bytes(key)),
        Value::Double(val),
    );
}

fn parse_query_string(q: &str) -> PhpArray {
    let mut arr = PhpArray::new();
    if q.is_empty() {
        return arr;
    }
    for pair in q.split('&') {
        if pair.is_empty() {
            continue;
        }
        let (k, v) = match pair.find('=') {
            Some(i) => (&pair[..i], &pair[i + 1..]),
            None => (pair, ""),
        };
        let key = url_decode(k);
        let value = url_decode(v);
        // PHP-style array merging for keys ending in []
        if let Some(strip) = key.strip_suffix("[]") {
            // Append to existing array under that key
            let key_str = PhpString::from_string(strip.to_string());
            let key_owned = ArrayKey::String(key_str);
            let existing = arr.get(&key_owned).cloned();
            let mut inner = match existing {
                Some(Value::Array(a)) => a.borrow().clone(),
                _ => PhpArray::new(),
            };
            inner.push(Value::String(PhpString::from_string(value)));
            arr.set(
                key_owned,
                Value::Array(std::rc::Rc::new(std::cell::RefCell::new(inner))),
            );
        } else {
            arr.set(
                ArrayKey::String(PhpString::from_string(key)),
                Value::String(PhpString::from_string(value)),
            );
        }
    }
    arr
}

fn parse_multipart(body: &[u8], boundary: &str, post: &mut PhpArray, files: &mut PhpArray) {
    let delim = format!("--{}", boundary);
    let delim_b = delim.as_bytes();
    // Split body on boundary
    let mut start = 0usize;
    let mut parts: Vec<&[u8]> = Vec::new();
    let mut i = 0usize;
    while i + delim_b.len() <= body.len() {
        if &body[i..i + delim_b.len()] == delim_b {
            if i > start {
                parts.push(&body[start..i]);
            }
            i += delim_b.len();
            // Skip trailing CRLF or LF
            if i + 1 < body.len() && &body[i..i + 2] == b"\r\n" {
                i += 2;
            } else if i < body.len() && body[i] == b'\n' {
                i += 1;
            }
            start = i;
            continue;
        }
        i += 1;
    }
    if start < body.len() {
        parts.push(&body[start..]);
    }
    for part in parts {
        // Strip a single trailing CRLF or LF (from the boundary preamble).
        let mut p = part;
        if p.ends_with(b"\r\n") {
            p = &p[..p.len() - 2];
        } else if p.ends_with(b"\n") {
            p = &p[..p.len() - 1];
        }
        // Find header/body separator (try CRLFCRLF then LFLF)
        let sep = if let Some(s) = find_subsequence(p, b"\r\n\r\n") {
            (s, 4)
        } else if let Some(s) = find_subsequence(p, b"\n\n") {
            (s, 2)
        } else {
            continue;
        };
        let (sep_idx, sep_len) = sep;
        let header_str = String::from_utf8_lossy(&p[..sep_idx]);
        let part_body = &p[sep_idx + sep_len..];
        let mut name = String::new();
        let mut filename: Option<String> = None;
        let mut ctype: Option<String> = None;
        let header_str_lf = header_str.replace("\r\n", "\n");
        for line in header_str_lf.split('\n') {
            let line_lower = line.to_ascii_lowercase();
            if let Some(idx) = line_lower.find("content-disposition:") {
                let rest = &line[idx + "content-disposition:".len()..];
                // form-data; name="x"; filename="y"
                for tok in rest.split(';') {
                    let t = tok.trim();
                    if let Some(v) = t.strip_prefix("name=") {
                        name = v.trim_matches('"').to_string();
                    } else if let Some(v) = t.strip_prefix("filename=") {
                        filename = Some(v.trim_matches('"').to_string());
                    }
                }
            } else if let Some(idx) = line_lower.find("content-type:") {
                let rest = &line[idx + "content-type:".len()..];
                ctype = Some(rest.trim().to_string());
            }
        }
        if name.is_empty() {
            continue;
        }
        if let Some(fname) = filename {
            // Write the uploaded file to a temporary file so callers can move_uploaded_file().
            let tmp_path = std::env::temp_dir().join(format!(
                "phpcli_upload_{}_{}.tmp",
                std::process::id(),
                epoch_secs()
            ));
            let tmp_str = match std::fs::write(&tmp_path, part_body) {
                Ok(_) => tmp_path.to_string_lossy().to_string(),
                Err(_) => String::new(),
            };
            // Store as a file. Mimic PHP's $_FILES structure.
            let mut entry = PhpArray::new();
            entry.set(
                ArrayKey::String(PhpString::from_bytes(b"name")),
                Value::String(PhpString::from_string(fname.clone())),
            );
            entry.set(
                ArrayKey::String(PhpString::from_bytes(b"full_path")),
                Value::String(PhpString::from_string(fname)),
            );
            entry.set(
                ArrayKey::String(PhpString::from_bytes(b"type")),
                Value::String(PhpString::from_string(
                    ctype.unwrap_or_else(|| "application/octet-stream".to_string()),
                )),
            );
            entry.set(
                ArrayKey::String(PhpString::from_bytes(b"tmp_name")),
                Value::String(PhpString::from_string(tmp_str)),
            );
            entry.set(
                ArrayKey::String(PhpString::from_bytes(b"error")),
                Value::Long(0),
            );
            entry.set(
                ArrayKey::String(PhpString::from_bytes(b"size")),
                Value::Long(part_body.len() as i64),
            );
            files.set(
                ArrayKey::String(PhpString::from_string(name)),
                Value::Array(std::rc::Rc::new(std::cell::RefCell::new(entry))),
            );
        } else {
            post.set(
                ArrayKey::String(PhpString::from_string(name)),
                Value::String(PhpString::from_bytes(part_body)),
            );
        }
    }
}

fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

fn base64_decode(s: &str) -> Option<Vec<u8>> {
    let s: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    let s_bytes = s.as_bytes();
    if s_bytes.is_empty() {
        return Some(Vec::new());
    }
    fn ch(c: u8) -> Option<u8> {
        match c {
            b'A'..=b'Z' => Some(c - b'A'),
            b'a'..=b'z' => Some(c - b'a' + 26),
            b'0'..=b'9' => Some(c - b'0' + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            b'=' => Some(64),
            _ => None,
        }
    }
    let mut out = Vec::with_capacity(s_bytes.len() * 3 / 4);
    let mut i = 0;
    while i + 4 <= s_bytes.len() {
        let a = ch(s_bytes[i])?;
        let b = ch(s_bytes[i + 1])?;
        let c = ch(s_bytes[i + 2])?;
        let d = ch(s_bytes[i + 3])?;
        if a == 64 || b == 64 {
            return None;
        }
        out.push((a << 2) | (b >> 4));
        if c != 64 {
            out.push((b << 4) | (c >> 2));
            if d != 64 {
                out.push((c << 6) | d);
            }
        }
        i += 4;
    }
    Some(out)
}

fn epoch_secs() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
fn epoch_secs_f64() -> f64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}
