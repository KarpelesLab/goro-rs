//! The request variables of the web SAPIs (the built-in server and CGI):
//! $_GET, $_POST, $_COOKIE, $_FILES, $_REQUEST and $_ENV from a request
//! (PHP manual: Predefined Variables, variables_order, request_order,
//! post_max_size, enable_post_data_reading, Handling file uploads).

use std::os::unix::ffi::OsStrExt;

use goro_runtime::{Key, PhpArray, PhpStr, Value};

use crate::Vm;
use crate::builtins::string::{parse_query, register_variable};

/// What a request brings, whatever the SAPI.
pub struct RequestData<'a> {
    pub method: &'a [u8],
    /// The query string, if the request has one.
    pub query: Option<&'a [u8]>,
    pub content_type: Option<&'a [u8]>,
    /// The declared body length (Content-Length), for post_max_size.
    pub content_length: Option<i64>,
    pub body: &'a [u8],
    /// The Cookie header.
    pub cookie: Option<&'a [u8]>,
}

fn ini_on(vm: &Vm, k: &[u8]) -> bool {
    vm.get_ini(k).is_none_or(|v| {
        !matches!(
            &v.to_ascii_lowercase()[..],
            b"0" | b"off" | b"" | b"false" | b"no"
        )
    })
}

/// The top-level key a variable name registers under, and whether an
/// index follows: up to the first `[` that a `]` closes, without leading
/// spaces, spaces and dots (and an unclosed `[`) made `_` (PHP manual:
/// Variables From External Sources; tests tests/basic/023.phpt,
/// ext/standard/tests/ghsa-wpj3-hf5j-x4v4.phpt).
fn base_name(n: &[u8]) -> (Vec<u8>, bool) {
    let n = n.trim_ascii_start();
    let open = n
        .iter()
        .position(|&c| c == b'[')
        .filter(|&p| n[p..].contains(&b']'));
    let end = open.unwrap_or(n.len());
    let base = n[..end]
        .iter()
        .map(|&c| {
            if matches!(c, b' ' | b'.' | b'[') {
                b'_'
            } else {
                c
            }
        })
        .collect();
    (base, open.is_some())
}

/// The Cookie header as $_COOKIE: `;`-separated pairs, the first cookie
/// of a name wins.
fn parse_cookies(ck: &[u8], max_depth: i64) -> PhpArray {
    let mut cookie = PhpArray::new();
    // Only the space before a name goes (test tests/basic/022.phpt keeps
    // "val1  ").
    for part in ck.split(|&b| b == b';') {
        let part = part.trim_ascii_start();
        if part.is_empty() {
            continue;
        }
        let (n, v) = match part.iter().position(|&b| b == b'=') {
            Some(p) => (&part[..p], &part[p + 1..]),
            None => (part, &b""[..]),
        };
        // Names stay as sent (test tests/basic/bug79699.phpt: "__%48ost-evil"
        // is not "__Host-evil"); values are percent-decoded.
        let n = n.to_vec();
        // A name that only becomes "__Host-" or "__Secure-" by the "." and
        // " " to "_" change is dropped (test
        // ext/standard/tests/bug81727.phpt).
        let (b, indexed) = base_name(&n);
        let lead = n.trim_ascii_start();
        if (b.starts_with(b"__Host-") && !lead.starts_with(b"__Host-"))
            || (b.starts_with(b"__Secure-") && !lead.starts_with(b"__Secure-"))
        {
            continue;
        }
        // A plain name already set is kept; indexed ones write into it.
        if !indexed && cookie.get(Key::from_bytes(&b).as_ref()).is_some() {
            continue;
        }
        // "+" stays (test tests/basic/bug78929.phpt).
        let v = goro_ext_standard::url::rawurldecode(v);
        register_variable(&mut cookie, &n, Value::Str(PhpStr::from(v)), max_depth);
    }
    cookie
}

/// A form-encoded string cut to max_input_vars variables, with a warning
/// when it had more (PHP manual: max_input_vars); with
/// mbstring.encoding_translation nothing is kept (tests
/// ext/mbstring/tests/bug63447_*.phpt).
fn limit_vars(vm: &mut Vm, data: &[u8], seps: &[u8]) -> Vec<u8> {
    let max = vm
        .get_ini(b"max_input_vars")
        .map(|v| goro_runtime::convert::str_to_int(v.trim_ascii()))
        .unwrap_or(1000);
    let pairs: Vec<&[u8]> = data
        .split(|c| seps.contains(c))
        .filter(|p| !p.is_empty())
        .collect();
    if max < 0 || pairs.len() as i64 <= max {
        return data.to_vec();
    }
    startup_warning(
        vm,
        &format!(
            "Input variables exceeded {}. To increase the limit change max_input_vars in php.ini.",
            max
        ),
    );
    if vm
        .get_ini(b"mbstring.encoding_translation")
        .is_some_and(|v| ini_on(vm, b"mbstring.encoding_translation") && !v.is_empty())
    {
        return Vec::new();
    }
    let sep = seps.first().copied().unwrap_or(b'&');
    pairs[..max as usize].join(&sep)
}

/// A "PHP Request Startup" warning.
pub(crate) fn startup_warning(vm: &mut Vm, msg: &str) {
    let _ = vm.raise_at(
        crate::vm::errors::E_WARNING,
        &format!("PHP Request Startup: {msg}"),
        PhpStr::from("Unknown"),
        0,
    );
}

/// Sets the superglobals of a request; `server` is `$_SERVER`.
pub fn set_globals(vm: &mut Vm, r: &RequestData, server: PhpArray) {
    let seps = vm
        .get_ini(b"arg_separator.input")
        .map(|v| v.to_vec())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| b"&".to_vec());
    let vorder = vm
        .get_ini(b"variables_order")
        .map(|v| v.to_ascii_uppercase())
        .unwrap_or_else(|| b"EGPCS".to_vec());
    let depth = crate::builtins::string::nesting_limit(vm);
    let get = match r.query {
        Some(q) if vorder.contains(&b'G') => parse_query(&limit_vars(vm, q, &seps), &seps, depth),
        _ => PhpArray::new(),
    };
    let mut post = PhpArray::new();
    let mut files = PhpArray::new();
    let mut upload_log = None;
    if r.method == b"POST" && ini_on(vm, b"enable_post_data_reading") {
        let ct = r.content_type.unwrap_or(b"").to_vec();
        let base = ct
            .split(|&b| b == b';' || b == b',')
            .next()
            .unwrap_or(b"")
            .trim_ascii()
            .to_ascii_lowercase();
        let max = vm
            .get_ini(b"post_max_size")
            .map(|v| crate::vm::setup::parse_size(&v))
            .unwrap_or(8 << 20);
        let len = r.content_length.unwrap_or(r.body.len() as i64);
        if max > 0 && len > max {
            startup_warning(
                vm,
                &format!(
                    "POST Content-Length of {} bytes exceeds the limit of {} bytes",
                    len, max
                ),
            );
        } else if !vorder.contains(&b'P') {
        } else if base == b"application/x-www-form-urlencoded" {
            post = parse_query(&limit_vars(vm, r.body, &seps), &seps, depth);
        } else if base == b"multipart/form-data" {
            // Problems are request startup warnings; what was parsed
            // stays (tests tests/basic/rfc1867_*.phpt).
            let mut limits = crate::cli_server::upload::Limits::from_ini(vm);
            limits.mb_lead = crate::builtins::mbstring::translation_lead(vm);
            let progress = crate::builtins::session::upload_progress_name(vm);
            vm.ext::<crate::cli_server::upload::UploadLog>().armed = progress;
            let res = crate::cli_server::upload::boundary(&ct)
                .map_err(String::from)
                .and_then(|b| {
                    crate::cli_server::upload::parse(vm, r.body, &b, limits, &mut post, &mut files)
                });
            if let Err(m) = &res {
                startup_warning(vm, m);
            }
            let log = std::mem::take(vm.ext::<crate::cli_server::upload::UploadLog>());
            // a body that could not be read records nothing (test
            // ext/session/tests/bug79221.phpt)
            if log.progress.is_some() && res.is_ok() {
                upload_log = Some((log, len));
            }
        }
    }
    let mut cookie = match r.cookie {
        Some(ck) if vorder.contains(&b'C') => parse_cookies(ck, depth),
        _ => PhpArray::new(),
    };
    let mut get = get;
    // mbstring.encoding_translation (PHP manual: mbstring, HTTP input
    // character encoding translation)
    if crate::builtins::mbstring::translation_on(vm) {
        get = crate::builtins::mbstring::translate_input(vm, 0, get);
        post = crate::builtins::mbstring::translate_input(vm, 1, post);
        cookie = crate::builtins::mbstring::translate_input(vm, 2, cookie);
    }
    crate::builtins::filter::register_request(vm, [&mut get, &mut post, &mut cookie]);
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
    if vorder.contains(&b'E') {
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
    if let Some((log, len)) = upload_log {
        crate::builtins::session::record_upload_progress(vm, &log, len);
    }
}
