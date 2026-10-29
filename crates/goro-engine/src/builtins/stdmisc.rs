//! Smaller ext/standard functions: time_sleep_until(), nl_langinfo(),
//! getrusage(), sys_getloadavg(), ftok(), realpath_cache_get(),
//! realpath_cache_size(), stream_set_chunk_size(), get_meta_tags() (PHP
//! manual pages of these functions; observed on PHP 8.5).

use std::ffi::CString;

use goro_runtime::{Key, PhpArray, PhpStr, Value};

use super::file::open_stream;
use crate::func::Args;
use crate::registry::{ConstVal, Module};
use crate::vm::{R, Vm};

pub static MODULE: Module = Module {
    name: "standard",
    functions: &[
        (
            "function time_sleep_until(float $timestamp): bool",
            time_sleep_until,
        ),
        ("function nl_langinfo(int $item): string|false", nl_langinfo),
        ("function getrusage(int $mode = 0): array|false", getrusage),
        ("function sys_getloadavg(): array|false", sys_getloadavg),
        (
            "function ftok(string $filename, string $project_id): int",
            ftok,
        ),
        ("function realpath_cache_get(): array", realpath_cache_get),
        ("function realpath_cache_size(): int", realpath_cache_size),
        (
            "function stream_set_chunk_size($stream, int $size): int",
            stream_set_chunk_size,
        ),
        (
            "function get_meta_tags(string $filename, bool $use_include_path = false): array|false",
            get_meta_tags,
        ),
        (
            "function request_parse_body(?array $options = null): array",
            request_parse_body,
        ),
    ],
    classes: &[(
        "requestparsebodyexception",
        "<?php class RequestParseBodyException extends Exception {}",
    )],
    constants: &[
        ("ABDAY_1", ConstVal::Int(131072)),
        ("ABDAY_2", ConstVal::Int(131073)),
        ("ABDAY_3", ConstVal::Int(131074)),
        ("ABDAY_4", ConstVal::Int(131075)),
        ("ABDAY_5", ConstVal::Int(131076)),
        ("ABDAY_6", ConstVal::Int(131077)),
        ("ABDAY_7", ConstVal::Int(131078)),
        ("DAY_1", ConstVal::Int(131079)),
        ("DAY_2", ConstVal::Int(131080)),
        ("DAY_3", ConstVal::Int(131081)),
        ("DAY_4", ConstVal::Int(131082)),
        ("DAY_5", ConstVal::Int(131083)),
        ("DAY_6", ConstVal::Int(131084)),
        ("DAY_7", ConstVal::Int(131085)),
        ("ABMON_1", ConstVal::Int(131086)),
        ("ABMON_2", ConstVal::Int(131087)),
        ("ABMON_3", ConstVal::Int(131088)),
        ("ABMON_4", ConstVal::Int(131089)),
        ("ABMON_5", ConstVal::Int(131090)),
        ("ABMON_6", ConstVal::Int(131091)),
        ("ABMON_7", ConstVal::Int(131092)),
        ("ABMON_8", ConstVal::Int(131093)),
        ("ABMON_9", ConstVal::Int(131094)),
        ("ABMON_10", ConstVal::Int(131095)),
        ("ABMON_11", ConstVal::Int(131096)),
        ("ABMON_12", ConstVal::Int(131097)),
        ("MON_1", ConstVal::Int(131098)),
        ("MON_2", ConstVal::Int(131099)),
        ("MON_3", ConstVal::Int(131100)),
        ("MON_4", ConstVal::Int(131101)),
        ("MON_5", ConstVal::Int(131102)),
        ("MON_6", ConstVal::Int(131103)),
        ("MON_7", ConstVal::Int(131104)),
        ("MON_8", ConstVal::Int(131105)),
        ("MON_9", ConstVal::Int(131106)),
        ("MON_10", ConstVal::Int(131107)),
        ("MON_11", ConstVal::Int(131108)),
        ("MON_12", ConstVal::Int(131109)),
        ("AM_STR", ConstVal::Int(131110)),
        ("PM_STR", ConstVal::Int(131111)),
        ("D_T_FMT", ConstVal::Int(131112)),
        ("D_FMT", ConstVal::Int(131113)),
        ("T_FMT", ConstVal::Int(131114)),
        ("T_FMT_AMPM", ConstVal::Int(131115)),
        ("ERA", ConstVal::Int(131116)),
        ("ERA_YEAR", ConstVal::Int(131117)),
        ("ERA_D_T_FMT", ConstVal::Int(131120)),
        ("ERA_D_FMT", ConstVal::Int(131118)),
        ("ERA_T_FMT", ConstVal::Int(131121)),
        ("ALT_DIGITS", ConstVal::Int(131119)),
        ("INT_CURR_SYMBOL", ConstVal::Int(262144)),
        ("CURRENCY_SYMBOL", ConstVal::Int(262145)),
        ("CRNCYSTR", ConstVal::Int(262159)),
        ("MON_DECIMAL_POINT", ConstVal::Int(262146)),
        ("MON_THOUSANDS_SEP", ConstVal::Int(262147)),
        ("MON_GROUPING", ConstVal::Int(262148)),
        ("POSITIVE_SIGN", ConstVal::Int(262149)),
        ("NEGATIVE_SIGN", ConstVal::Int(262150)),
        ("INT_FRAC_DIGITS", ConstVal::Int(262151)),
        ("FRAC_DIGITS", ConstVal::Int(262152)),
        ("P_CS_PRECEDES", ConstVal::Int(262153)),
        ("P_SEP_BY_SPACE", ConstVal::Int(262154)),
        ("N_CS_PRECEDES", ConstVal::Int(262155)),
        ("N_SEP_BY_SPACE", ConstVal::Int(262156)),
        ("P_SIGN_POSN", ConstVal::Int(262157)),
        ("N_SIGN_POSN", ConstVal::Int(262158)),
        ("DECIMAL_POINT", ConstVal::Int(65536)),
        ("RADIXCHAR", ConstVal::Int(65536)),
        ("THOUSANDS_SEP", ConstVal::Int(65537)),
        ("THOUSEP", ConstVal::Int(65537)),
        ("GROUPING", ConstVal::Int(65538)),
        ("YESEXPR", ConstVal::Int(327680)),
        ("NOEXPR", ConstVal::Int(327681)),
        ("YESSTR", ConstVal::Int(327682)),
        ("NOSTR", ConstVal::Int(327683)),
        ("CODESET", ConstVal::Int(14)),
    ],
    listed: false,
    ..Module::EMPTY
};

fn now() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

fn time_sleep_until(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let t = a.float(0);
    // test ext/standard/tests/misc/gh14774.phpt
    if !t.is_finite() || !(0.0..=u64::MAX as f64).contains(&t) {
        return vm.fthrow(
            a,
            "ValueError",
            &format!(
                "Argument #1 ($timestamp) must be between 0 and {}",
                u64::MAX
            ),
        );
    }
    let left = t - now();
    if left < 0.0 {
        vm.fwarn(
            a,
            "Argument #1 ($timestamp) must be greater than or equal to the current time",
        )?;
        return Ok(Value::Bool(false));
    }
    std::thread::sleep(std::time::Duration::from_secs_f64(left));
    Ok(Value::Bool(true))
}

fn nl_langinfo(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let item = a.int(0);
    let known =
        matches!(item, 14 | 65536..=65538 | 131072..=131121 | 262144..=262159 | 327680..=327683);
    if !known {
        vm.fwarn(a, &format!("Item '{item}' is not valid"))?;
        return Ok(Value::Bool(false));
    }
    // SAFETY: nl_langinfo() returns a string owned by the C library.
    let p = unsafe { libc::nl_langinfo(item as libc::nl_item) };
    if p.is_null() {
        return Ok(Value::Bool(false));
    }
    let s = unsafe { std::ffi::CStr::from_ptr(p) };
    Ok(Value::Str(PhpStr::new(s.to_bytes())))
}

fn getrusage(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let who = if a.int_or(0, 0) == 1 {
        libc::RUSAGE_CHILDREN
    } else {
        libc::RUSAGE_SELF
    };
    let mut u: libc::rusage = unsafe { std::mem::zeroed() };
    if unsafe { libc::getrusage(who, &mut u) } != 0 {
        return Ok(Value::Bool(false));
    }
    let mut o = PhpArray::new();
    let mut put = |k: &str, v: i64| {
        o.insert(Key::from_bytes(k.as_bytes()), Value::Int(v));
    };
    put("ru_oublock", u.ru_oublock);
    put("ru_inblock", u.ru_inblock);
    put("ru_msgsnd", u.ru_msgsnd);
    put("ru_msgrcv", u.ru_msgrcv);
    put("ru_maxrss", u.ru_maxrss);
    put("ru_ixrss", u.ru_ixrss);
    put("ru_idrss", u.ru_idrss);
    put("ru_minflt", u.ru_minflt);
    put("ru_majflt", u.ru_majflt);
    put("ru_nsignals", u.ru_nsignals);
    put("ru_nvcsw", u.ru_nvcsw);
    put("ru_nivcsw", u.ru_nivcsw);
    put("ru_nswap", u.ru_nswap);
    put("ru_utime.tv_usec", u.ru_utime.tv_usec);
    put("ru_utime.tv_sec", u.ru_utime.tv_sec);
    put("ru_stime.tv_usec", u.ru_stime.tv_usec);
    put("ru_stime.tv_sec", u.ru_stime.tv_sec);
    Ok(Value::Array(o))
}

fn sys_getloadavg(_vm: &mut Vm, _a: &mut Args) -> R<Value> {
    let mut l = [0f64; 3];
    if unsafe { libc::getloadavg(l.as_mut_ptr(), 3) } != 3 {
        return Ok(Value::Bool(false));
    }
    Ok(Value::Array(PhpArray::from_values(
        l.iter().map(|&v| Value::Float(v)),
    )))
}

fn ftok(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let f = a.str(0);
    let p = a.str(1);
    if f.is_empty() {
        return vm.fthrow(a, "ValueError", "Argument #1 ($filename) cannot be empty");
    }
    if p.len() != 1 {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #2 ($project_id) must be a single character",
        );
    }
    use std::os::unix::ffi::OsStrExt;
    let path = vm.fs_path(&f);
    let Ok(c) = CString::new(path.as_os_str().as_bytes()) else {
        return Ok(Value::Int(-1));
    };
    let k = unsafe { libc::ftok(c.as_ptr(), p[0] as libc::c_int) };
    if k == -1 {
        vm.fwarn(a, "ftok() failed - No such file or directory")?;
    }
    Ok(Value::Int(k as i64))
}

fn realpath_cache_get(_vm: &mut Vm, _a: &mut Args) -> R<Value> {
    // paths are not cached
    Ok(Value::Array(PhpArray::new()))
}

fn realpath_cache_size(_vm: &mut Vm, _a: &mut Args) -> R<Value> {
    Ok(Value::Int(0))
}

thread_local! {
    /// stream_set_chunk_size() values, by resource.
    static CHUNKS: std::cell::RefCell<std::collections::HashMap<i64, i64>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}

fn stream_set_chunk_size(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let size = a.int(1);
    let id = match a.val(0) {
        Value::Resource(r) => r.id(),
        _ => 0,
    };
    super::file::with_stream(vm, a, 0, |_, _| Ok(()))?;
    if size <= 0 {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #2 ($size) must be greater than 0",
        );
    }
    let old = CHUNKS
        .with(|c| c.borrow_mut().insert(id, size))
        .unwrap_or(8192);
    Ok(Value::Int(old))
}

/// The `<meta name=... content=...>` tags before `</head>`.
fn meta_tags(d: &[u8]) -> Vec<(Vec<u8>, Vec<u8>)> {
    let mut out = Vec::new();
    let mut i = 0;
    let lower = d.to_ascii_lowercase();
    while let Some(p) = d[i..].iter().position(|&c| c == b'<') {
        i += p + 1;
        if lower[i..].starts_with(b"/head") {
            break;
        }
        if !lower[i..].starts_with(b"meta") {
            continue;
        }
        let mut j = i + 4;
        let (mut name, mut content) = (None, None);
        let mut ok = false;
        loop {
            while j < d.len() && d[j].is_ascii_whitespace() {
                j += 1;
            }
            match d.get(j) {
                None | Some(b'<') => break,
                Some(b'>') => {
                    ok = true;
                    break;
                }
                _ => {}
            }
            let ks = j;
            while j < d.len() && !matches!(d[j], b'=' | b'>' | b'<') && !d[j].is_ascii_whitespace()
            {
                j += 1;
            }
            let key = lower[ks..j].to_vec();
            while j < d.len() && d[j].is_ascii_whitespace() {
                j += 1;
            }
            let mut val = Vec::new();
            if d.get(j) == Some(&b'=') {
                j += 1;
                while j < d.len() && d[j].is_ascii_whitespace() {
                    j += 1;
                }
                match d.get(j) {
                    Some(&q @ (b'"' | b'\'')) => {
                        let vs = j + 1;
                        let Some(e) = d[vs..].iter().position(|&c| c == q) else {
                            j = d.len();
                            break;
                        };
                        val = d[vs..vs + e].to_vec();
                        j = vs + e + 1;
                    }
                    _ => {
                        let vs = j;
                        while j < d.len()
                            && !matches!(d[j], b'>' | b'<')
                            && !d[j].is_ascii_whitespace()
                        {
                            j += 1;
                        }
                        val = d[vs..j].to_vec();
                    }
                }
            }
            if key == b"name" {
                name = Some(val);
            } else if key == b"content" {
                content = Some(val);
            }
        }
        if ok && let (Some(n), Some(c)) = (name, content) {
            let n: Vec<u8> = n
                .iter()
                .map(|&c| {
                    let c = c.to_ascii_lowercase();
                    if c.is_ascii_alphanumeric() { c } else { b'_' }
                })
                .collect();
            out.push((n, c));
        }
        i = j.min(d.len());
    }
    out
}

fn get_meta_tags(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let f = a.str(0);
    let Some(mut s) = open_stream(vm, a, &f, b"rb", false)? else {
        return Ok(Value::Bool(false));
    };
    let data = super::file::read_rest(vm, &mut s);
    let mut o = PhpArray::new();
    for (n, c) in meta_tags(&data) {
        let k = Key::from_bytes(&n);
        if o.get(k.as_ref()).is_none() {
            o.insert(k, Value::Str(PhpStr::from(c)));
        }
    }
    Ok(Value::Array(o))
}

/// request_parse_body() (PHP manual; RFC "Expose request_parse_body()"):
/// the request body as [$_POST, $_FILES], with option overrides.
fn request_parse_body(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let mut limits = crate::cli_server::upload::Limits::from_ini(vm);
    let mut post_max = vm
        .get_ini(b"post_max_size")
        .map(|v| crate::vm::setup::parse_size(&v))
        .unwrap_or(8 << 20);
    if let Some(Value::Array(opts)) = a.get(0).map(|v| v.deref()) {
        for (k, v) in opts.iter() {
            let k = match k {
                goro_runtime::KeyRef::Str(k) => k.to_vec(),
                goro_runtime::KeyRef::Int(_) => {
                    return vm.throw("ValueError", "Invalid integer key in $options argument");
                }
            };
            let size = match &k[..] {
                b"post_max_size" | b"upload_max_filesize" => true,
                b"max_input_vars" | b"max_multipart_body_parts" | b"max_file_uploads" => false,
                _ => {
                    return vm.throw(
                        "ValueError",
                        &format!(
                            "Invalid key \"{}\" in $options argument",
                            String::from_utf8_lossy(&k)
                        ),
                    );
                }
            };
            let v = v.deref();
            if matches!(v, Value::Array(_) | Value::Object(_)) {
                return vm.throw("ValueError", "Invalid array value in $options argument");
            }
            let s = vm.to_php_str(&v)?;
            let n = if size {
                let (n, w) = crate::vm::setup::parse_quantity(&s);
                if let Some(w) = w {
                    vm.warn(&w)?;
                }
                n
            } else {
                goro_runtime::convert::str_to_int(s.trim_ascii())
            };
            // The options replace the ini settings (tests
            // ext/standard/tests/http/request_parse_body/multipart_options_*).
            match &k[..] {
                b"post_max_size" => post_max = n,
                b"upload_max_filesize" => limits.upload_max_filesize = n,
                b"max_input_vars" => limits.max_input_vars = n,
                b"max_multipart_body_parts" => limits.max_parts = n,
                _ => limits.max_file_uploads = n,
            }
        }
    }
    let server = vm.globals_array().get_bytes(b"_SERVER").map(|v| v.deref());
    let ct = match server {
        Some(Value::Array(s)) => s.get_bytes(b"CONTENT_TYPE").map(|v| v.deref()),
        _ => None,
    };
    let Some(Value::Str(ct)) = ct else {
        return vm.throw(
            "RequestParseBodyException",
            "Request does not provide a content type",
        );
    };
    let body = vm.out.sapi.input.clone();
    let mime = ct
        .split(|&c| c == b';' || c == b',')
        .next()
        .unwrap_or_default()
        .trim_ascii()
        .to_ascii_lowercase();
    let mut post = PhpArray::new();
    let mut files = PhpArray::new();
    if post_max > 0 && body.len() as i64 > post_max {
        return vm.throw(
            "RequestParseBodyException",
            &format!(
                "POST Content-Length of {} bytes exceeds the limit of {} bytes",
                body.len(),
                post_max
            ),
        );
    }
    match &mime[..] {
        b"application/x-www-form-urlencoded" => {
            post = super::string::parse_query(&body, b"&");
        }
        b"multipart/form-data" => {
            let b = match crate::cli_server::upload::boundary(&ct) {
                Ok(b) => b,
                Err(m) => return vm.throw("RequestParseBodyException", m),
            };
            let r = crate::cli_server::upload::parse(vm, &body, &b, limits, &mut post, &mut files);
            if let Err(m) = r {
                return vm.throw("RequestParseBodyException", &m);
            }
        }
        _ => {
            return vm.throw(
                "RequestParseBodyException",
                &format!(
                    "Content-Type \"{}\" is not supported",
                    String::from_utf8_lossy(&ct)
                ),
            );
        }
    }
    Ok(Value::Array(PhpArray::from_values([
        Value::Array(post),
        Value::Array(files),
    ])))
}
