mod handle;

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use goro_core::array::{ArrayKey, PhpArray};
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};

use handle::{CurlHandle, CURL_HANDLES, NEXT_CURL_ID};

// CURLOPT constants
const CURLOPT_URL: i64 = 10002;
const CURLOPT_RETURNTRANSFER: i64 = 19913;
const CURLOPT_POST: i64 = 47;
const CURLOPT_POSTFIELDS: i64 = 10015;
const CURLOPT_HTTPHEADER: i64 = 10023;
const CURLOPT_FOLLOWLOCATION: i64 = 52;
const CURLOPT_MAXREDIRS: i64 = 68;
const CURLOPT_TIMEOUT: i64 = 13;
const CURLOPT_CONNECTTIMEOUT: i64 = 78;
const CURLOPT_USERAGENT: i64 = 10018;
const CURLOPT_HEADER: i64 = 42;
const CURLOPT_NOBODY: i64 = 44;
const CURLOPT_SSL_VERIFYPEER: i64 = 64;
const CURLOPT_SSL_VERIFYHOST: i64 = 81;
const CURLOPT_CUSTOMREQUEST: i64 = 10036;
const CURLOPT_ENCODING: i64 = 10102;
const CURLOPT_HTTPGET: i64 = 80;
const CURLOPT_COOKIE: i64 = 10022;
const CURLOPT_USERPWD: i64 = 10005;
const CURLOPT_HTTPAUTH: i64 = 107;
const CURLOPT_FAILONERROR: i64 = 45;
const CURLOPT_PUT: i64 = 54;

// CURLINFO constants
const CURLINFO_HTTP_CODE: i64 = 2097154;
const CURLINFO_EFFECTIVE_URL: i64 = 1048577;
const CURLINFO_CONTENT_TYPE: i64 = 1048594;
const CURLINFO_TOTAL_TIME: i64 = 3145731;
const CURLINFO_REDIRECT_COUNT: i64 = 2097172;
const CURLINFO_HEADER_SIZE: i64 = 2097163;

/// Register all curl extension functions and constants
pub fn register(vm: &mut Vm) {
    vm.register_extension(b"curl");
    // Register functions
    vm.register_function(b"curl_init", curl_init);
    vm.register_function(b"curl_setopt", curl_setopt);
    vm.register_function(b"curl_setopt_array", curl_setopt_array);
    vm.register_function(b"curl_exec", curl_exec);
    vm.register_function(b"curl_close", curl_close);
    vm.register_function(b"curl_error", curl_error);
    vm.register_function(b"curl_errno", curl_errno);
    vm.register_function(b"curl_getinfo", curl_getinfo);
    vm.register_function(b"curl_reset", curl_reset);
    vm.register_function(b"curl_version", curl_version);

    // Register CURLOPT constants
    vm.constants.insert(b"CURLOPT_URL".to_vec(), Value::Long(10002));
    vm.constants.insert(b"CURLOPT_RETURNTRANSFER".to_vec(), Value::Long(19913));
    vm.constants.insert(b"CURLOPT_POST".to_vec(), Value::Long(47));
    vm.constants.insert(b"CURLOPT_POSTFIELDS".to_vec(), Value::Long(10015));
    vm.constants.insert(b"CURLOPT_HTTPHEADER".to_vec(), Value::Long(10023));
    vm.constants.insert(b"CURLOPT_FOLLOWLOCATION".to_vec(), Value::Long(52));
    vm.constants.insert(b"CURLOPT_MAXREDIRS".to_vec(), Value::Long(68));
    vm.constants.insert(b"CURLOPT_TIMEOUT".to_vec(), Value::Long(13));
    vm.constants.insert(b"CURLOPT_CONNECTTIMEOUT".to_vec(), Value::Long(78));
    vm.constants.insert(b"CURLOPT_USERAGENT".to_vec(), Value::Long(10018));
    vm.constants.insert(b"CURLOPT_HEADER".to_vec(), Value::Long(42));
    vm.constants.insert(b"CURLOPT_NOBODY".to_vec(), Value::Long(44));
    vm.constants.insert(b"CURLOPT_SSL_VERIFYPEER".to_vec(), Value::Long(64));
    vm.constants.insert(b"CURLOPT_SSL_VERIFYHOST".to_vec(), Value::Long(81));
    vm.constants.insert(b"CURLOPT_CUSTOMREQUEST".to_vec(), Value::Long(10036));
    vm.constants.insert(b"CURLOPT_ENCODING".to_vec(), Value::Long(10102));
    vm.constants.insert(b"CURLOPT_HTTPGET".to_vec(), Value::Long(80));
    vm.constants.insert(b"CURLOPT_COOKIE".to_vec(), Value::Long(10022));
    vm.constants.insert(b"CURLOPT_COOKIEFILE".to_vec(), Value::Long(10031));
    vm.constants.insert(b"CURLOPT_USERPWD".to_vec(), Value::Long(10005));
    vm.constants.insert(b"CURLOPT_HTTPAUTH".to_vec(), Value::Long(107));
    vm.constants.insert(b"CURLOPT_HTTP_VERSION".to_vec(), Value::Long(84));
    vm.constants.insert(b"CURLOPT_VERBOSE".to_vec(), Value::Long(41));
    vm.constants.insert(b"CURLOPT_HEADERFUNCTION".to_vec(), Value::Long(20079));
    vm.constants.insert(b"CURLOPT_WRITEFUNCTION".to_vec(), Value::Long(20011));
    vm.constants.insert(b"CURLOPT_READFUNCTION".to_vec(), Value::Long(20012));
    vm.constants.insert(b"CURLOPT_INFILESIZE".to_vec(), Value::Long(14));
    vm.constants.insert(b"CURLOPT_PUT".to_vec(), Value::Long(54));
    vm.constants.insert(b"CURLOPT_FAILONERROR".to_vec(), Value::Long(45));

    // Register CURLINFO constants
    vm.constants.insert(b"CURLINFO_HTTP_CODE".to_vec(), Value::Long(2097154));
    vm.constants.insert(b"CURLINFO_EFFECTIVE_URL".to_vec(), Value::Long(1048577));
    vm.constants.insert(b"CURLINFO_CONTENT_TYPE".to_vec(), Value::Long(1048594));
    vm.constants.insert(b"CURLINFO_TOTAL_TIME".to_vec(), Value::Long(3145731));
    vm.constants.insert(b"CURLINFO_REDIRECT_COUNT".to_vec(), Value::Long(2097172));
    vm.constants.insert(b"CURLINFO_HEADER_SIZE".to_vec(), Value::Long(2097163));
    vm.constants.insert(b"CURLINFO_RESPONSE_CODE".to_vec(), Value::Long(2097154));

    // Register CURLE error constants
    vm.constants.insert(b"CURLE_OK".to_vec(), Value::Long(0));
    vm.constants.insert(b"CURLE_UNSUPPORTED_PROTOCOL".to_vec(), Value::Long(1));
    vm.constants.insert(b"CURLE_URL_MALFORMAT".to_vec(), Value::Long(3));
    vm.constants.insert(b"CURLE_COULDNT_RESOLVE_HOST".to_vec(), Value::Long(6));
    vm.constants.insert(b"CURLE_COULDNT_CONNECT".to_vec(), Value::Long(7));
    vm.constants.insert(b"CURLE_OPERATION_TIMEDOUT".to_vec(), Value::Long(28));
    vm.constants.insert(b"CURLE_SSL_CONNECT_ERROR".to_vec(), Value::Long(35));

    // HTTP version constants
    vm.constants.insert(b"CURL_HTTP_VERSION_NONE".to_vec(), Value::Long(0));
    vm.constants.insert(b"CURL_HTTP_VERSION_1_0".to_vec(), Value::Long(1));
    vm.constants.insert(b"CURL_HTTP_VERSION_1_1".to_vec(), Value::Long(2));

    // Auth constants
    vm.constants.insert(b"CURLAUTH_BASIC".to_vec(), Value::Long(1));
    vm.constants.insert(b"CURLAUTH_DIGEST".to_vec(), Value::Long(2));
    vm.constants.insert(b"CURLAUTH_ANY".to_vec(), Value::Long(-17));
}

/// curl_init(?string $url = null): CurlHandle|false
fn curl_init(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let mut handle = CurlHandle::new();

    // Optional URL argument
    if let Some(url_val) = args.first() {
        if !matches!(url_val, Value::Null | Value::Undef) {
            let url_str = url_val.to_php_string().to_string_lossy();
            handle.url = url_str.clone();
            handle.effective_url = url_str;
        }
    }

    let id = NEXT_CURL_ID.with(|cell| {
        let id = cell.get();
        cell.set(id + 1);
        id
    });

    CURL_HANDLES.with(|handles| {
        handles.borrow_mut().insert(id, handle);
    });

    Ok(Value::Long(id))
}

/// Apply a single curl option to a handle. Returns true on success, false on failure.
fn apply_setopt(handle: &mut CurlHandle, option: i64, value: &Value) -> bool {
    match option {
        CURLOPT_URL => {
            let url_str = value.to_php_string().to_string_lossy();
            handle.url = url_str.clone();
            handle.effective_url = url_str;
        }
        CURLOPT_RETURNTRANSFER => {
            handle.return_transfer = value.to_long() != 0;
        }
        CURLOPT_POST => {
            if value.to_long() != 0 {
                handle.method = "POST".to_string();
            }
        }
        CURLOPT_POSTFIELDS => {
            match value {
                Value::String(s) => {
                    handle.post_fields = Some(s.as_bytes().to_vec());
                    handle.method = "POST".to_string();
                }
                Value::Array(arr) => {
                    // Build URL-encoded form data from array
                    let arr = arr.borrow();
                    let mut parts = Vec::new();
                    for (key, val) in arr.iter() {
                        let k = match key {
                            ArrayKey::Int(n) => n.to_string(),
                            ArrayKey::String(s) => s.to_string_lossy(),
                        };
                        let v = val.to_php_string().to_string_lossy();
                        parts.push(format!(
                            "{}={}",
                            url_encode(&k),
                            url_encode(&v)
                        ));
                    }
                    let encoded = parts.join("&");
                    handle.post_fields = Some(encoded.into_bytes());
                    handle.method = "POST".to_string();
                }
                _ => {
                    let s = value.to_php_string();
                    handle.post_fields = Some(s.as_bytes().to_vec());
                    handle.method = "POST".to_string();
                }
            }
        }
        CURLOPT_HTTPHEADER => {
            // Value should be an array of "Header: Value" strings
            if let Value::Array(arr) = value {
                handle.headers.clear();
                let arr = arr.borrow();
                for (_key, val) in arr.iter() {
                    let header_str = val.to_php_string().to_string_lossy();
                    if let Some(pos) = header_str.find(':') {
                        let name = header_str[..pos].trim().to_string();
                        let hval = header_str[pos + 1..].trim().to_string();
                        handle.headers.push((name, hval));
                    }
                }
            }
        }
        CURLOPT_FOLLOWLOCATION => {
            handle.follow_location = value.to_long() != 0;
        }
        CURLOPT_MAXREDIRS => {
            handle.max_redirects = value.to_long();
        }
        CURLOPT_TIMEOUT => {
            handle.timeout = value.to_long() as u64;
        }
        CURLOPT_CONNECTTIMEOUT => {
            handle.connect_timeout = value.to_long() as u64;
        }
        CURLOPT_USERAGENT => {
            handle.user_agent = value.to_php_string().to_string_lossy();
        }
        CURLOPT_HEADER => {
            handle.include_header = value.to_long() != 0;
        }
        CURLOPT_NOBODY => {
            handle.nobody = value.to_long() != 0;
            if handle.nobody {
                handle.method = "HEAD".to_string();
            }
        }
        CURLOPT_SSL_VERIFYPEER => {
            handle.ssl_verify_peer = value.to_long() != 0;
        }
        CURLOPT_SSL_VERIFYHOST => {
            handle.ssl_verify_host = value.to_long();
        }
        CURLOPT_CUSTOMREQUEST => {
            let method = value.to_php_string().to_string_lossy();
            handle.custom_request = Some(method);
        }
        CURLOPT_ENCODING => {
            let enc = value.to_php_string().to_string_lossy();
            handle.encoding = Some(enc);
        }
        CURLOPT_HTTPGET => {
            if value.to_long() != 0 {
                handle.method = "GET".to_string();
                handle.post_fields = None;
            }
        }
        CURLOPT_COOKIE => {
            handle.cookie = Some(value.to_php_string().to_string_lossy());
        }
        CURLOPT_USERPWD => {
            handle.userpwd = Some(value.to_php_string().to_string_lossy());
        }
        CURLOPT_HTTPAUTH => {
            handle.http_auth = value.to_long();
        }
        CURLOPT_FAILONERROR => {
            handle.fail_on_error = value.to_long() != 0;
        }
        CURLOPT_PUT => {
            if value.to_long() != 0 {
                handle.method = "PUT".to_string();
            }
        }
        _ => {
            // Silently ignore unknown options (like PHP does for most)
        }
    }
    true
}

/// curl_setopt(CurlHandle $handle, int $option, mixed $value): bool
fn curl_setopt(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let handle_id = args.first().unwrap_or(&Value::Null).to_long();
    let option = args.get(1).unwrap_or(&Value::Null).to_long();
    let value = args.get(2).unwrap_or(&Value::Null);

    let result = CURL_HANDLES.with(|handles| {
        let mut handles = handles.borrow_mut();
        if let Some(handle) = handles.get_mut(&handle_id) {
            apply_setopt(handle, option, value)
        } else {
            false
        }
    });

    Ok(if result { Value::True } else { Value::False })
}

/// curl_setopt_array(CurlHandle $handle, array $options): bool
fn curl_setopt_array(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let handle_id = args.first().unwrap_or(&Value::Null).to_long();
    let options = args.get(1).unwrap_or(&Value::Null);

    if let Value::Array(arr) = options {
        let arr = arr.borrow();
        // Collect options first to avoid borrow issues
        let opts: Vec<(i64, Value)> = arr
            .iter()
            .map(|(key, val)| {
                let opt = match key {
                    ArrayKey::Int(n) => *n,
                    ArrayKey::String(s) => s.to_string_lossy().parse::<i64>().unwrap_or(0),
                };
                (opt, val.clone())
            })
            .collect();

        let result = CURL_HANDLES.with(|handles| {
            let mut handles = handles.borrow_mut();
            if let Some(handle) = handles.get_mut(&handle_id) {
                for (option, value) in &opts {
                    if !apply_setopt(handle, *option, value) {
                        return false;
                    }
                }
                true
            } else {
                false
            }
        });

        Ok(if result { Value::True } else { Value::False })
    } else {
        Ok(Value::False)
    }
}

/// curl_exec(CurlHandle $handle): string|bool
fn curl_exec(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let handle_id = args.first().unwrap_or(&Value::Null).to_long();

    // Extract all needed data from the handle before making the HTTP request
    let request_data = CURL_HANDLES.with(|handles| {
        let handles = handles.borrow();
        if let Some(handle) = handles.get(&handle_id) {
            Some((
                handle.url.clone(),
                handle.custom_request.clone().unwrap_or_else(|| handle.method.clone()),
                handle.headers.clone(),
                handle.post_fields.clone(),
                handle.timeout,
                handle.connect_timeout,
                handle.user_agent.clone(),
                handle.follow_location,
                handle.max_redirects,
                handle.encoding.clone(),
                handle.cookie.clone(),
                handle.userpwd.clone(),
                handle.return_transfer,
                handle.include_header,
                handle.nobody,
                handle.fail_on_error,
            ))
        } else {
            None
        }
    });

    let (
        url,
        method,
        headers,
        post_fields,
        timeout,
        _connect_timeout,
        user_agent,
        follow_location,
        max_redirects,
        encoding,
        cookie,
        userpwd,
        return_transfer,
        include_header,
        nobody,
        fail_on_error,
    ) = match request_data {
        Some(data) => data,
        None => return Ok(Value::False),
    };

    if url.is_empty() {
        CURL_HANDLES.with(|handles| {
            let mut handles = handles.borrow_mut();
            if let Some(handle) = handles.get_mut(&handle_id) {
                handle.error_message = "No URL set!".to_string();
                handle.error_number = 3; // CURLE_URL_MALFORMAT
            }
        });
        return Ok(Value::False);
    }

    let start_time = Instant::now();

    // Build ureq agent with config
    let mut config_builder = ureq::Agent::config_builder();

    if timeout > 0 {
        config_builder = config_builder.timeout_global(Some(Duration::from_secs(timeout)));
    }

    // Configure redirects
    if follow_location {
        let max_redir = if max_redirects < 0 { 30 } else { max_redirects as u32 };
        config_builder = config_builder.max_redirects(max_redir);
    } else {
        config_builder = config_builder.max_redirects(0);
    }

    // Don't treat HTTP status codes as errors - we handle them ourselves
    config_builder = config_builder.http_status_as_error(false);

    let agent: ureq::Agent = config_builder.build().into();

    // Build request based on method
    let effective_method = if nobody { "HEAD" } else { &method };

    // Execute the request
    let result = execute_request(
        &agent,
        effective_method,
        &url,
        &headers,
        &post_fields,
        &user_agent,
        &encoding,
        &cookie,
        &userpwd,
    );

    let elapsed = start_time.elapsed().as_secs_f64();

    // Process result and update handle
    match result {
        Ok((status, resp_headers, body, content_type, effective_url)) => {
            // Build header text if needed
            let mut header_text = Vec::new();
            if include_header {
                header_text.extend_from_slice(
                    format!("HTTP/1.1 {} OK\r\n", status).as_bytes(),
                );
                for (name, value) in &resp_headers {
                    header_text.extend_from_slice(
                        format!("{}: {}\r\n", name, value).as_bytes(),
                    );
                }
                header_text.extend_from_slice(b"\r\n");
            }

            let header_size = header_text.len();

            CURL_HANDLES.with(|handles| {
                let mut handles = handles.borrow_mut();
                if let Some(handle) = handles.get_mut(&handle_id) {
                    handle.response_code = status as i64;
                    handle.response_headers = resp_headers;
                    handle.response_body = body.clone();
                    handle.header_size = header_size;
                    handle.content_type = content_type;
                    handle.effective_url = effective_url;
                    handle.total_time = elapsed;
                    handle.error_message.clear();
                    handle.error_number = 0;
                }
            });

            // Check fail_on_error
            if fail_on_error && status >= 400 {
                CURL_HANDLES.with(|handles| {
                    let mut handles = handles.borrow_mut();
                    if let Some(handle) = handles.get_mut(&handle_id) {
                        handle.error_message = format!(
                            "The requested URL returned error: {}",
                            status
                        );
                        handle.error_number = 22; // CURLE_HTTP_RETURNED_ERROR
                    }
                });
                return Ok(Value::False);
            }

            if return_transfer {
                let mut result_body = Vec::new();
                if include_header {
                    result_body.extend_from_slice(&header_text);
                }
                result_body.extend_from_slice(&body);
                Ok(Value::String(PhpString::from_vec(result_body)))
            } else {
                // Echo output directly
                if include_header {
                    vm.write_output(&header_text);
                }
                vm.write_output(&body);
                Ok(Value::True)
            }
        }
        Err((error_number, error_message)) => {
            CURL_HANDLES.with(|handles| {
                let mut handles = handles.borrow_mut();
                if let Some(handle) = handles.get_mut(&handle_id) {
                    handle.error_message = error_message;
                    handle.error_number = error_number;
                    handle.total_time = elapsed;
                }
            });
            Ok(Value::False)
        }
    }
}

/// Execute the HTTP request using ureq and return the results.
fn execute_request(
    agent: &ureq::Agent,
    method: &str,
    url: &str,
    headers: &[(String, String)],
    post_fields: &Option<Vec<u8>>,
    user_agent: &str,
    encoding: &Option<String>,
    cookie: &Option<String>,
    userpwd: &Option<String>,
) -> Result<(u16, Vec<(String, String)>, Vec<u8>, String, String), (i64, String)> {
    // For methods with body (POST, PUT, PATCH), we need to use the body-capable builder
    let has_body = matches!(method, "POST" | "PUT" | "PATCH") || post_fields.is_some();

    let response = if has_body {
        let mut request = match method {
            "POST" => agent.post(url),
            "PUT" => agent.put(url),
            "PATCH" => agent.patch(url),
            _ => agent.post(url), // fallback for body-capable
        };

        // Set headers
        for (name, value) in headers {
            request = request.header(name.as_str(), value.as_str());
        }
        if !user_agent.is_empty() {
            request = request.header("User-Agent", user_agent);
        }
        if let Some(enc) = encoding {
            request = request.header("Accept-Encoding", enc.as_str());
        }
        if let Some(cookie_val) = cookie {
            request = request.header("Cookie", cookie_val.as_str());
        }
        if let Some(userpwd_val) = userpwd {
            if let Some(pos) = userpwd_val.find(':') {
                let user = &userpwd_val[..pos];
                let pass = &userpwd_val[pos + 1..];
                // Build basic auth header
                let credentials = base64_encode(&format!("{}:{}", user, pass));
                request = request.header("Authorization", &format!("Basic {}", credentials));
            }
        }

        // Send with body
        if let Some(body) = post_fields {
            // Set content-type if not already set
            let has_content_type = headers.iter().any(|(n, _)| n.eq_ignore_ascii_case("content-type"));
            if !has_content_type {
                request = request.header("Content-Type", "application/x-www-form-urlencoded");
            }
            request.send(body.as_slice())
        } else {
            request.send_empty()
        }
    } else {
        let mut request = match method {
            "GET" => agent.get(url),
            "HEAD" => agent.head(url),
            "DELETE" => agent.delete(url),
            "OPTIONS" => agent.options(url),
            _ => agent.get(url),
        };

        // Set headers
        for (name, value) in headers {
            request = request.header(name.as_str(), value.as_str());
        }
        if !user_agent.is_empty() {
            request = request.header("User-Agent", user_agent);
        }
        if let Some(enc) = encoding {
            request = request.header("Accept-Encoding", enc.as_str());
        }
        if let Some(cookie_val) = cookie {
            request = request.header("Cookie", cookie_val.as_str());
        }
        if let Some(userpwd_val) = userpwd {
            if let Some(pos) = userpwd_val.find(':') {
                let user = &userpwd_val[..pos];
                let pass = &userpwd_val[pos + 1..];
                let credentials = base64_encode(&format!("{}:{}", user, pass));
                request = request.header("Authorization", &format!("Basic {}", credentials));
            }
        }

        request.call()
    };

    match response {
        Ok(mut resp) => {
            let status = resp.status().as_u16();

            // Collect response headers
            let mut resp_headers = Vec::new();
            for (name, value) in resp.headers().iter() {
                if let Ok(v) = value.to_str() {
                    resp_headers.push((name.as_str().to_string(), v.to_string()));
                }
            }

            // Get content type
            let content_type = resp
                .headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .to_string();

            // Get effective URL (after redirects)
            let effective_url = url.to_string();

            // Read body
            let body = resp
                .body_mut()
                .read_to_vec()
                .unwrap_or_default();

            Ok((status, resp_headers, body, content_type, effective_url))
        }
        Err(e) => {
            let (error_number, error_message) = classify_ureq_error(&e);
            Err((error_number, error_message))
        }
    }
}

/// Classify a ureq error into a curl error number and message
fn classify_ureq_error(e: &ureq::Error) -> (i64, String) {
    let msg = e.to_string();
    match e {
        ureq::Error::Timeout(_) => (28, msg),           // CURLE_OPERATION_TIMEDOUT
        ureq::Error::HostNotFound => (6, msg),           // CURLE_COULDNT_RESOLVE_HOST
        ureq::Error::ConnectionFailed => (7, msg),       // CURLE_COULDNT_CONNECT
        ureq::Error::Tls(_) => (35, msg),                // CURLE_SSL_CONNECT_ERROR
        ureq::Error::Rustls(_) => (35, msg),             // CURLE_SSL_CONNECT_ERROR
        ureq::Error::BadUri(_) => (3, msg),              // CURLE_URL_MALFORMAT
        ureq::Error::TooManyRedirects => (47, msg),      // CURLE_TOO_MANY_REDIRECTS
        _ => (7, msg),                                    // CURLE_COULDNT_CONNECT as fallback
    }
}

/// curl_close(CurlHandle $handle): void
fn curl_close(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let handle_id = args.first().unwrap_or(&Value::Null).to_long();
    CURL_HANDLES.with(|handles| {
        handles.borrow_mut().remove(&handle_id);
    });
    Ok(Value::Null)
}

/// curl_error(CurlHandle $handle): string
fn curl_error(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let handle_id = args.first().unwrap_or(&Value::Null).to_long();
    let msg = CURL_HANDLES.with(|handles| {
        let handles = handles.borrow();
        if let Some(handle) = handles.get(&handle_id) {
            handle.error_message.clone()
        } else {
            String::new()
        }
    });
    Ok(Value::String(PhpString::from_string(msg)))
}

/// curl_errno(CurlHandle $handle): int
fn curl_errno(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let handle_id = args.first().unwrap_or(&Value::Null).to_long();
    let errno = CURL_HANDLES.with(|handles| {
        let handles = handles.borrow();
        if let Some(handle) = handles.get(&handle_id) {
            handle.error_number
        } else {
            0
        }
    });
    Ok(Value::Long(errno))
}

/// curl_getinfo(CurlHandle $handle, ?int $option = null): mixed
fn curl_getinfo(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let handle_id = args.first().unwrap_or(&Value::Null).to_long();
    let option = args.get(1);

    CURL_HANDLES.with(|handles| {
        let handles = handles.borrow();
        if let Some(handle) = handles.get(&handle_id) {
            if let Some(opt) = option {
                if matches!(opt, Value::Null | Value::Undef) {
                    // Return full info array
                    return Ok(build_info_array(handle));
                }
                let opt_val = opt.to_long();
                match opt_val {
                    CURLINFO_HTTP_CODE => Ok(Value::Long(handle.response_code)),
                    CURLINFO_EFFECTIVE_URL => {
                        Ok(Value::String(PhpString::from_string(handle.effective_url.clone())))
                    }
                    CURLINFO_CONTENT_TYPE => {
                        if handle.content_type.is_empty() {
                            Ok(Value::Null)
                        } else {
                            Ok(Value::String(PhpString::from_string(handle.content_type.clone())))
                        }
                    }
                    CURLINFO_TOTAL_TIME => Ok(Value::Double(handle.total_time)),
                    CURLINFO_REDIRECT_COUNT => Ok(Value::Long(handle.redirect_count)),
                    CURLINFO_HEADER_SIZE => Ok(Value::Long(handle.header_size as i64)),
                    _ => Ok(Value::False),
                }
            } else {
                // No option specified - return full info array
                Ok(build_info_array(handle))
            }
        } else {
            Ok(Value::False)
        }
    })
}

/// Build the full info array for curl_getinfo with no option
fn build_info_array(handle: &CurlHandle) -> Value {
    let mut arr = PhpArray::new();

    arr.set(
        ArrayKey::String(PhpString::from_bytes(b"url")),
        Value::String(PhpString::from_string(handle.effective_url.clone())),
    );
    arr.set(
        ArrayKey::String(PhpString::from_bytes(b"content_type")),
        if handle.content_type.is_empty() {
            Value::Null
        } else {
            Value::String(PhpString::from_string(handle.content_type.clone()))
        },
    );
    arr.set(
        ArrayKey::String(PhpString::from_bytes(b"http_code")),
        Value::Long(handle.response_code),
    );
    arr.set(
        ArrayKey::String(PhpString::from_bytes(b"header_size")),
        Value::Long(handle.header_size as i64),
    );
    arr.set(
        ArrayKey::String(PhpString::from_bytes(b"redirect_count")),
        Value::Long(handle.redirect_count),
    );
    arr.set(
        ArrayKey::String(PhpString::from_bytes(b"total_time")),
        Value::Double(handle.total_time),
    );
    arr.set(
        ArrayKey::String(PhpString::from_bytes(b"namelookup_time")),
        Value::Double(0.0),
    );
    arr.set(
        ArrayKey::String(PhpString::from_bytes(b"connect_time")),
        Value::Double(0.0),
    );
    arr.set(
        ArrayKey::String(PhpString::from_bytes(b"pretransfer_time")),
        Value::Double(0.0),
    );
    arr.set(
        ArrayKey::String(PhpString::from_bytes(b"starttransfer_time")),
        Value::Double(0.0),
    );
    arr.set(
        ArrayKey::String(PhpString::from_bytes(b"redirect_time")),
        Value::Double(0.0),
    );
    arr.set(
        ArrayKey::String(PhpString::from_bytes(b"size_upload")),
        Value::Double(0.0),
    );
    arr.set(
        ArrayKey::String(PhpString::from_bytes(b"size_download")),
        Value::Double(handle.response_body.len() as f64),
    );
    arr.set(
        ArrayKey::String(PhpString::from_bytes(b"speed_download")),
        Value::Double(0.0),
    );
    arr.set(
        ArrayKey::String(PhpString::from_bytes(b"speed_upload")),
        Value::Double(0.0),
    );
    arr.set(
        ArrayKey::String(PhpString::from_bytes(b"download_content_length")),
        Value::Double(-1.0),
    );
    arr.set(
        ArrayKey::String(PhpString::from_bytes(b"upload_content_length")),
        Value::Double(-1.0),
    );
    arr.set(
        ArrayKey::String(PhpString::from_bytes(b"response_code")),
        Value::Long(handle.response_code),
    );

    Value::Array(Rc::new(RefCell::new(arr)))
}

/// curl_reset(CurlHandle $handle): void
fn curl_reset(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let handle_id = args.first().unwrap_or(&Value::Null).to_long();
    CURL_HANDLES.with(|handles| {
        let mut handles = handles.borrow_mut();
        if let Some(handle) = handles.get_mut(&handle_id) {
            handle.reset();
        }
    });
    Ok(Value::Null)
}

/// curl_version(int $age = 0): array|false
fn curl_version(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    let mut arr = PhpArray::new();

    arr.set(
        ArrayKey::String(PhpString::from_bytes(b"version_number")),
        Value::Long(0x075500), // 7.85.0 equivalent
    );
    arr.set(
        ArrayKey::String(PhpString::from_bytes(b"version")),
        Value::String(PhpString::from_bytes(b"7.85.0")),
    );
    arr.set(
        ArrayKey::String(PhpString::from_bytes(b"ssl_version_number")),
        Value::Long(0),
    );
    arr.set(
        ArrayKey::String(PhpString::from_bytes(b"ssl_version")),
        Value::String(PhpString::from_bytes(b"rustls/0.23")),
    );
    arr.set(
        ArrayKey::String(PhpString::from_bytes(b"host")),
        Value::String(PhpString::from_bytes(b"rust")),
    );
    arr.set(
        ArrayKey::String(PhpString::from_bytes(b"features")),
        Value::Long(0),
    );
    arr.set(
        ArrayKey::String(PhpString::from_bytes(b"protocols")),
        {
            let mut protos = PhpArray::new();
            protos.push(Value::String(PhpString::from_bytes(b"http")));
            protos.push(Value::String(PhpString::from_bytes(b"https")));
            Value::Array(Rc::new(RefCell::new(protos)))
        },
    );

    Ok(Value::Array(Rc::new(RefCell::new(arr))))
}

/// Simple URL encoding (percent-encoding)
fn url_encode(s: &str) -> String {
    let mut result = String::new();
    for byte in s.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                result.push(byte as char);
            }
            b' ' => {
                result.push('+');
            }
            _ => {
                result.push('%');
                result.push_str(&format!("{:02X}", byte));
            }
        }
    }
    result
}

/// Simple base64 encoding for Basic auth
fn base64_encode(input: &str) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let bytes = input.as_bytes();
    let mut result = String::new();

    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let triple = (b0 << 16) | (b1 << 8) | b2;

        result.push(CHARS[((triple >> 18) & 0x3F) as usize] as char);
        result.push(CHARS[((triple >> 12) & 0x3F) as usize] as char);

        if chunk.len() > 1 {
            result.push(CHARS[((triple >> 6) & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }

        if chunk.len() > 2 {
            result.push(CHARS[(triple & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
    }

    result
}
