//! Easy handles: curl_init(), curl_setopt(), curl_exec(), curl_getinfo()
//! and the other functions on a `CurlHandle` (PHP manual: cURL Functions).
//! Option checks, messages and the precedence of the output options
//! (RETURNTRANSFER, FILE, WRITEFUNCTION: the last one set wins) observed on
//! PHP 8.5 and in the ext/curl tests.

use std::cell::RefCell;
use std::rc::Rc;

use goro_ext_curl::consts as c;
use goro_ext_curl::cookie::CookieJar;
use goro_ext_curl::info::Info;
use goro_ext_curl::multipart::Part;
use goro_ext_curl::options::{Body, Options, parse_protocols_str};
use goro_ext_curl::transfer::{self, Failure, Host, Pool, ReadResult};
use goro_runtime::{Key, ObjRef, PhpArray, Value};

use crate::builtins::file::Stream;
use crate::bytecode::CastTo;
use crate::class::NativeHooks;
use crate::func::Args;
use crate::vm::{R, Unwind, Vm};

/// Where body data goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum WriteMode {
    Stdout,
    Return,
    File,
    User,
}

/// A CURLOPT_POSTFIELDS array entry.
#[derive(Clone, Debug)]
pub(super) enum FormData {
    Bytes(Vec<u8>),
    /// A CURLFile: read when the transfer starts.
    File {
        path: Vec<u8>,
        mime: Vec<u8>,
        postname: Vec<u8>,
    },
    /// A CURLStringFile.
    StringFile {
        data: Vec<u8>,
        mime: Vec<u8>,
        postname: Vec<u8>,
    },
}

/// An easy handle's state.
#[derive(Clone)]
pub(super) struct Easy {
    pub opts: Options,
    pub info: Info,
    pub errno: i64,
    pub error: String,
    pub write_mode: WriteMode,
    pub file: Option<Value>,
    pub writefunc: Option<Value>,
    pub writeheader: Option<Value>,
    pub headerfunc: Option<Value>,
    pub infile: Option<Value>,
    pub readfunc: Option<Value>,
    pub stderr: Option<Value>,
    pub progressfunc: Option<Value>,
    pub xferinfofunc: Option<Value>,
    pub debugfunc: Option<Value>,
    pub prereqfunc: Option<Value>,
    pub fnmatchfunc: Option<Value>,
    pub sshhostkeyfunc: Option<Value>,
    pub private: Option<Value>,
    pub form: Option<Vec<(Vec<u8>, FormData)>>,
    pub header_out: bool,
    pub share: Option<ObjRef>,
    pub jar: Rc<RefCell<CookieJar>>,
    pub pool: Rc<RefCell<Pool>>,
    /// Cookie files to read at the next transfer, and all of them
    /// (CURLOPT_COOKIELIST "RELOAD").
    pub cookie_files: Vec<Vec<u8>>,
    pub all_cookie_files: Vec<Vec<u8>>,
    pub cookiejar: Option<Vec<u8>>,
    /// The body of the last RETURNTRANSFER transfer.
    pub content: Option<Vec<u8>>,
    pub in_multi: bool,
    pub running: bool,
}

impl Easy {
    pub(super) fn new(vm: &Vm) -> Easy {
        let mut opts = Options::default();
        if basedir_set(vm) {
            opts.protocols &= !c::CURLPROTO_FILE;
            opts.redir_protocols &= !c::CURLPROTO_FILE;
        }
        let info = Info {
            effective_url: Some(Vec::new()),
            ..Info::default()
        };
        Easy {
            opts,
            info,
            errno: 0,
            error: String::new(),
            write_mode: WriteMode::Stdout,
            file: None,
            writefunc: None,
            writeheader: None,
            headerfunc: None,
            infile: None,
            readfunc: None,
            stderr: None,
            progressfunc: None,
            xferinfofunc: None,
            debugfunc: None,
            prereqfunc: None,
            fnmatchfunc: None,
            sshhostkeyfunc: None,
            private: None,
            form: None,
            header_out: false,
            share: None,
            jar: Rc::new(RefCell::new(CookieJar::new())),
            pool: Rc::new(RefCell::new(Pool::default())),
            cookie_files: Vec::new(),
            all_cookie_files: Vec::new(),
            cookiejar: None,
            content: None,
            in_multi: false,
            running: false,
        }
    }

    /// curl_copy_handle(): the same options, callbacks and information,
    /// its own cookie store copy and connection cache.
    fn duplicate(&self) -> Easy {
        let mut e = self.clone();
        e.jar = Rc::new(RefCell::new(self.jar.borrow().clone()));
        e.pool = Rc::new(RefCell::new(Pool::default()));
        e.in_multi = false;
        e.running = false;
        e
    }

    fn values(&self) -> impl Iterator<Item = &Value> {
        [
            &self.file,
            &self.writefunc,
            &self.writeheader,
            &self.headerfunc,
            &self.infile,
            &self.readfunc,
            &self.stderr,
            &self.progressfunc,
            &self.xferinfofunc,
            &self.debugfunc,
            &self.prereqfunc,
            &self.fnmatchfunc,
            &self.sshhostkeyfunc,
            &self.private,
        ]
        .into_iter()
        .flatten()
    }
}

pub(super) struct EasyObj(pub Easy);

impl goro_runtime::NativeData for EasyObj {
    fn trace(&self, visit: &mut dyn FnMut(&Value)) {
        for v in self.0.values() {
            visit(v);
        }
        if let Some(s) = &self.0.share {
            visit(&Value::Object(s.clone()));
        }
    }
    fn clear(&mut self) {
        let e = &mut self.0;
        e.file = None;
        e.writefunc = None;
        e.writeheader = None;
        e.headerfunc = None;
        e.infile = None;
        e.readfunc = None;
        e.stderr = None;
        e.progressfunc = None;
        e.xferinfofunc = None;
        e.debugfunc = None;
        e.prereqfunc = None;
        e.fnmatchfunc = None;
        e.sshhostkeyfunc = None;
        e.private = None;
        e.share = None;
    }
}

pub(super) fn basedir_set(vm: &Vm) -> bool {
    vm.get_ini(b"open_basedir").is_some_and(|v| !v.is_empty())
}

pub(super) fn with_easy<T>(o: &ObjRef, f: impl FnOnce(&mut Easy) -> T) -> Option<T> {
    let mut n = o.obj().native.borrow_mut();
    let any: &mut dyn std::any::Any = &mut **n.as_mut()?;
    any.downcast_mut::<EasyObj>().map(|e| f(&mut e.0))
}

pub(super) fn hooks() -> NativeHooks {
    NativeHooks {
        create: Some(|vm, o| {
            let e = Easy::new(vm);
            *o.obj().native.borrow_mut() = Some(Box::new(EasyObj(e)));
        }),
        clone: Some(|_, from, to| {
            let e = with_easy(from, |e| e.duplicate());
            if let Some(e) = e {
                *to.obj().native.borrow_mut() = Some(Box::new(EasyObj(e)));
            }
        }),
        cast: Some(|_, o, to| match to {
            // (int) gives the object id (test curl_int_cast.phpt).
            CastTo::Int => Some(Ok(Value::Int(o.handle() as i64))),
            _ => None,
        }),
        destroy: Some(|vm, o| {
            let jar = with_easy(o, |e| (e.cookiejar.clone(), e.jar.clone())).flatten_jar();
            if let Some((path, jar)) = jar {
                write_jar(vm, &path, &jar);
            }
        }),
        ..Default::default()
    }
}

trait FlattenJar {
    fn flatten_jar(self) -> Option<(Vec<u8>, Rc<RefCell<CookieJar>>)>;
}

impl FlattenJar for Option<(Option<Vec<u8>>, Rc<RefCell<CookieJar>>)> {
    fn flatten_jar(self) -> Option<(Vec<u8>, Rc<RefCell<CookieJar>>)> {
        let (p, j) = self?;
        Some((p?, j))
    }
}

/// Writes the cookie jar file (CURLOPT_COOKIEJAR; "-" is standard output).
fn write_jar(vm: &mut Vm, path: &[u8], jar: &Rc<RefCell<CookieJar>>) {
    let data = jar.borrow_mut().jar_file();
    if path == b"-" {
        vm.echo(&data);
        return;
    }
    let p = vm.fs_path(path);
    let _ = std::fs::write(p, data);
}

fn handle_arg(a: &Args) -> ObjRef {
    a.obj(0).expect("CurlHandle argument")
}

/// `new CurlHandle` (observed on PHP 8.5).
pub(super) fn no_construct(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    vm.throw(
        "Error",
        "Cannot directly construct CurlHandle, use curl_init() instead",
    )
}

fn has_nul(b: &[u8]) -> bool {
    b.contains(&0)
}

pub(super) fn curl_init(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let url = a.opt_str(0);
    if let Some(u) = &url
        && has_nul(u)
    {
        return vm.fthrow(
            a,
            "ValueError",
            "cURL option must not contain any null bytes",
        );
    }
    let cls = vm
        .find_class(b"CurlHandle", false)?
        .expect("CurlHandle is declared");
    let o = vm.instantiate_raw(&cls)?;
    if let Some(u) = url {
        with_easy(&o, |e| set_url(e, Some(u.as_bytes().to_vec())));
    }
    Ok(Value::Object(o))
}

fn set_url(e: &mut Easy, u: Option<Vec<u8>>) {
    // Before a transfer the effective URL is the URL set, when it has a
    // scheme (observed on PHP 8.5: curl_init("foobar") reports none).
    e.info.effective_url = match &u {
        Some(u) if goro_ext_curl::url::scheme_of(u).is_some() => Some(u.clone()),
        _ => None,
    };
    e.opts.url = u;
}

pub(super) fn curl_close(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    vm.deprecated(
        "Function curl_close() is deprecated since 8.5, as it has no effect since PHP 8.0",
    )?;
    Ok(Value::Null)
}

pub(super) fn curl_copy_handle(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = handle_arg(a);
    let cls = vm.class_of(&o);
    let n = vm.instantiate_raw(&cls)?;
    if let Some(e) = with_easy(&o, |e| e.duplicate()) {
        *n.obj().native.borrow_mut() = Some(Box::new(EasyObj(e)));
    }
    Ok(Value::Object(n))
}

pub(super) fn curl_errno(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(Value::Int(
        with_easy(&handle_arg(a), |e| e.errno).unwrap_or(0),
    ))
}

pub(super) fn curl_error(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let s = with_easy(&handle_arg(a), |e| e.error.clone()).unwrap_or_default();
    Ok(Value::from(s.as_bytes()))
}

pub(super) fn curl_escape(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(Value::from(
        goro_ext_curl::url::escape(a.bytes(1)).as_slice(),
    ))
}

pub(super) fn curl_unescape(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(Value::from(
        goro_ext_curl::url::unescape(a.bytes(1)).as_slice(),
    ))
}

pub(super) fn curl_strerror(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(Value::from(
        goro_ext_curl::strerror::easy(a.int(0)).as_bytes(),
    ))
}

pub(super) fn curl_pause(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    // Transfers run to completion inside curl_exec()/curl_multi_exec(), so
    // there is never a transfer to pause between calls: libcurl answers
    // CURLE_BAD_FUNCTION_ARGUMENT for an idle handle (observed on PHP 8.5).
    let busy = with_easy(&handle_arg(a), |e| e.running || e.in_multi).unwrap_or(false);
    Ok(Value::Int(if busy {
        0
    } else {
        c::CURLE_BAD_FUNCTION_ARGUMENT
    }))
}

pub(super) fn curl_upkeep(_vm: &mut Vm, _a: &mut Args) -> R<Value> {
    Ok(Value::Bool(true))
}

pub(super) fn curl_reset(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = handle_arg(a);
    let fresh = Easy::new(vm);
    with_easy(&o, |e| {
        let jar = e.jar.clone();
        let pool = e.pool.clone();
        let in_multi = e.in_multi;
        let running = e.running;
        let private = e.private.take();
        *e = fresh;
        // Cookies and connections survive a reset (libcurl documentation
        // of curl_easy_reset), and so does CURLOPT_PRIVATE (observed on
        // PHP 8.5).
        e.jar = jar;
        e.pool = pool;
        e.private = private;
        e.in_multi = in_multi;
        e.running = running;
    });
    Ok(Value::Null)
}

pub(super) fn curl_file_create(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let cls = vm
        .find_class(b"CURLFile", false)?
        .expect("CURLFile is declared");
    let o = vm.instantiate_raw(&cls)?;
    let args: Vec<Value> = (0..a.len()).map(|i| a.val(i)).collect();
    vm.call_method(&o, b"__construct", args)?;
    Ok(Value::Object(o))
}

pub(super) fn curl_multi_getcontent(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let r = with_easy(&handle_arg(a), |e| {
        if e.write_mode == WriteMode::Return {
            Some(e.content.clone().unwrap_or_default())
        } else {
            None
        }
    })
    .flatten();
    Ok(match r {
        Some(b) => Value::from(b.as_slice()),
        None => Value::Null,
    })
}

// ------------------------------------------------------------ curl_version

pub(super) fn curl_version(_vm: &mut Vm, _a: &mut Args) -> R<Value> {
    use goro_ext_curl::version as v;
    let mut arr = PhpArray::new();
    let set = |arr: &mut PhpArray, k: &str, val: Value| {
        arr.insert(Key::from(k), val);
    };
    set(&mut arr, "version_number", Value::Int(v::VERSION_NUM));
    set(&mut arr, "age", Value::Int(v::AGE));
    set(&mut arr, "features", Value::Int(v::features()));
    let mut fl = PhpArray::new();
    for (name, _, on) in v::FEATURES {
        fl.insert(Key::from(*name), Value::Bool(*on));
    }
    set(&mut arr, "feature_list", Value::Array(fl));
    set(&mut arr, "ssl_version_number", Value::Int(0));
    set(&mut arr, "version", Value::from(v::VERSION.as_bytes()));
    set(&mut arr, "host", Value::from(v::HOST.as_bytes()));
    set(
        &mut arr,
        "ssl_version",
        Value::from(v::SSL_VERSION.as_bytes()),
    );
    set(
        &mut arr,
        "libz_version",
        Value::from(v::LIBZ_VERSION.as_bytes()),
    );
    let mut pr = PhpArray::new();
    for p in v::PROTOCOLS {
        let _ = pr.push(Value::from(p.as_bytes()));
    }
    set(&mut arr, "protocols", Value::Array(pr));
    set(&mut arr, "ares", Value::from(&b""[..]));
    set(&mut arr, "ares_num", Value::Int(0));
    set(&mut arr, "libidn", Value::from(&b""[..]));
    set(&mut arr, "iconv_ver_num", Value::Int(0));
    set(&mut arr, "libssh_version", Value::from(&b""[..]));
    Ok(Value::Array(arr))
}

// ------------------------------------------------------------ setopt

pub(super) fn curl_setopt(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = handle_arg(a);
    let opt = a.int(1);
    let v = a.val(2);
    if !valid_option(opt) {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #2 ($option) is not a valid cURL option",
        );
    }
    Ok(Value::Bool(set_option(vm, a, &o, opt, v, false)?))
}

pub(super) fn curl_setopt_array(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = handle_arg(a);
    let arr = a.array(1);
    for (k, v) in arr.iter() {
        let goro_runtime::KeyRef::Int(opt) = k else {
            return vm.fthrow(
                a,
                "ValueError",
                "Argument #2 ($options) contains an invalid cURL option",
            );
        };
        if !valid_option(opt) {
            return vm.fthrow(
                a,
                "ValueError",
                "Argument #2 ($options) must contain only valid cURL options",
            );
        }
        if !set_option(vm, a, &o, opt, v.deref(), true)? {
            return Ok(Value::Bool(false));
        }
    }
    Ok(Value::Bool(true))
}

/// Whether `opt` is a CURLOPT_* value (or CURLINFO_HEADER_OUT).
fn valid_option(opt: i64) -> bool {
    if opt == c::CURLINFO_HEADER_OUT {
        return true;
    }
    c::ALL
        .iter()
        .any(|(n, v)| *v == opt && n.starts_with("CURLOPT_"))
}

/// The CURLOPT_* name of an option value (the first one PHP registers).
fn option_name(opt: i64) -> &'static str {
    c::ALL
        .iter()
        .find(|(n, v)| *v == opt && n.starts_with("CURLOPT_"))
        .map(|(n, _)| *n)
        .unwrap_or("")
}

/// Options taking an array of strings (curl_slist).
fn is_slist(opt: i64) -> bool {
    matches!(
        opt,
        c::CURLOPT_HTTPHEADER
            | c::CURLOPT_QUOTE
            | c::CURLOPT_POSTQUOTE
            | c::CURLOPT_PREQUOTE
            | c::CURLOPT_HTTP200ALIASES
            | c::CURLOPT_TELNETOPTIONS
            | c::CURLOPT_MAIL_RCPT
            | c::CURLOPT_RESOLVE
            | c::CURLOPT_PROXYHEADER
            | c::CURLOPT_CONNECT_TO
    )
}

fn opt_str(vm: &mut Vm, v: &Value) -> R<Option<Vec<u8>>> {
    if v.is_null() {
        return Ok(None);
    }
    Ok(Some(vm.to_php_str(v)?.as_bytes().to_vec()))
}

/// A stream resource option's value: the resource, or None for null.
fn stream_of(v: &Value) -> Option<Option<*const RefCell<Stream>>> {
    match v {
        Value::Null => Some(None),
        Value::Resource(r) => {
            if r.is_closed() {
                return None;
            }
            let data = r.res().data.borrow();
            let cell = data
                .as_ref()
                .and_then(|d| d.downcast_ref::<RefCell<Stream>>())?;
            Some(Some(cell as *const RefCell<Stream>))
        }
        _ => None,
    }
}

/// Whether a stream resource is still open.
pub(super) fn stream_alive(v: &Value) -> bool {
    matches!(stream_of(v), Some(Some(_)))
}

#[allow(clippy::too_many_lines)]
fn set_option(vm: &mut Vm, a: &Args, o: &ObjRef, opt: i64, v: Value, arr: bool) -> R<bool> {
    let argdesc = if arr {
        "Argument #2 ($options)"
    } else {
        "Argument #3 ($value)"
    };
    // Callbacks.
    if matches!(
        opt,
        c::CURLOPT_WRITEFUNCTION
            | c::CURLOPT_READFUNCTION
            | c::CURLOPT_HEADERFUNCTION
            | c::CURLOPT_PROGRESSFUNCTION
            | c::CURLOPT_XFERINFOFUNCTION
            | c::CURLOPT_DEBUGFUNCTION
            | c::CURLOPT_PREREQFUNCTION
            | c::CURLOPT_FNMATCH_FUNCTION
            | c::CURLOPT_SSH_HOSTKEYFUNCTION
    ) {
        let cb = if v.is_null() {
            None
        } else {
            if !vm.is_callable_value(&v, false) {
                let why = vm.callable_error(&v);
                return vm.fthrow(
                    a,
                    "TypeError",
                    &format!(
                        "{argdesc} must be a valid callback for option {}, {why}",
                        option_name(opt)
                    ),
                );
            }
            Some(v)
        };
        with_easy(o, |e| match opt {
            c::CURLOPT_WRITEFUNCTION => {
                e.write_mode = if cb.is_some() {
                    WriteMode::User
                } else {
                    WriteMode::Stdout
                };
                e.writefunc = cb;
            }
            c::CURLOPT_READFUNCTION => e.readfunc = cb,
            c::CURLOPT_HEADERFUNCTION => e.headerfunc = cb,
            c::CURLOPT_PROGRESSFUNCTION => e.progressfunc = cb,
            c::CURLOPT_XFERINFOFUNCTION => e.xferinfofunc = cb,
            c::CURLOPT_DEBUGFUNCTION => e.debugfunc = cb,
            c::CURLOPT_PREREQFUNCTION => e.prereqfunc = cb,
            c::CURLOPT_FNMATCH_FUNCTION => e.fnmatchfunc = cb,
            _ => e.sshhostkeyfunc = cb,
        });
        return Ok(true);
    }
    // Streams.
    if matches!(
        opt,
        c::CURLOPT_FILE | c::CURLOPT_INFILE | c::CURLOPT_WRITEHEADER | c::CURLOPT_STDERR
    ) {
        let Some(s) = stream_of(&v) else {
            return vm.fthrow(
                a,
                "TypeError",
                "supplied argument is not a valid File-Handle resource",
            );
        };
        if let Some(cell) = s
            && opt != c::CURLOPT_INFILE
        {
            // SAFETY: the resource is held by `v` for the call.
            let st = unsafe { (*cell).borrow() };
            let writable = st.writable || st.mode.as_bytes().contains(&b'+');
            let ro =
                st.mode.as_bytes().first() == Some(&b'r') && !st.mode.as_bytes().contains(&b'+');
            drop(st);
            if !writable || ro {
                return vm.fthrow(a, "ValueError", "The provided file handle must be writable");
            }
        }
        let val = if v.is_null() { None } else { Some(v) };
        with_easy(o, |e| match opt {
            c::CURLOPT_FILE => {
                e.write_mode = if val.is_some() {
                    WriteMode::File
                } else {
                    WriteMode::Stdout
                };
                e.file = val;
            }
            c::CURLOPT_INFILE => e.infile = val,
            c::CURLOPT_WRITEHEADER => e.writeheader = val,
            _ => e.stderr = val,
        });
        return Ok(true);
    }
    if is_slist(opt) {
        let Value::Array(list) = &v else {
            return vm.fthrow(
                a,
                "TypeError",
                &format!("The {} option must have an array value", option_name(opt)),
            );
        };
        let mut out = Vec::new();
        for (_, item) in list.iter() {
            out.push(vm.to_php_str(&item.deref())?.as_bytes().to_vec());
        }
        with_easy(o, |e| match opt {
            c::CURLOPT_HTTPHEADER => e.opts.headers = out,
            c::CURLOPT_PROXYHEADER => e.opts.proxy_headers = out,
            c::CURLOPT_RESOLVE => e.opts.resolve = out,
            c::CURLOPT_CONNECT_TO => e.opts.connect_to = out,
            _ => {}
        });
        return Ok(true);
    }
    match opt {
        c::CURLOPT_SAFE_UPLOAD => {
            if !vm.to_bool(&v) {
                return vm.fthrow(
                    a,
                    "ValueError",
                    "Disabling safe uploads is no longer supported",
                );
            }
            return Ok(true);
        }
        c::CURLOPT_RETURNTRANSFER => {
            let on = vm.to_bool(&v);
            with_easy(o, |e| {
                e.write_mode = if on {
                    WriteMode::Return
                } else {
                    WriteMode::Stdout
                }
            });
            return Ok(true);
        }
        c::CURLOPT_BINARYTRANSFER => return Ok(true),
        c::CURLINFO_HEADER_OUT => {
            let on = vm.to_bool(&v);
            let dbg = with_easy(o, |e| e.debugfunc.is_some()).unwrap_or(false);
            if on && dbg {
                return vm.throw(
                    "ValueError",
                    "CURLINFO_HEADER_OUT option must not be set when the CURLOPT_DEBUGFUNCTION option is set",
                );
            }
            with_easy(o, |e| e.header_out = on);
            return Ok(true);
        }
        c::CURLOPT_PRIVATE => {
            with_easy(o, |e| e.private = Some(v));
            return Ok(true);
        }
        c::CURLOPT_SHARE => {
            if let Value::Object(s) = &v {
                let l = vm.class_of(s).lname.clone();
                if matches!(
                    l.as_bytes(),
                    b"curlsharehandle" | b"curlsharepersistenthandle"
                ) {
                    let s = s.clone();
                    with_easy(o, |e| e.share = Some(s));
                }
            }
            return Ok(true);
        }
        c::CURLOPT_POSTFIELDS => {
            return set_postfields(vm, o, v).map(|_| true);
        }
        _ => {}
    }
    // Blobs: raw bytes.
    if (40000..50000).contains(&opt) {
        let b = opt_str(vm, &v)?;
        with_easy(o, |e| match opt {
            c::CURLOPT_SSLCERT_BLOB => e.opts.sslcert_blob = b,
            c::CURLOPT_SSLKEY_BLOB => e.opts.sslkey_blob = b,
            c::CURLOPT_ISSUERCERT_BLOB => e.opts.issuercert_blob = b,
            c::CURLOPT_CAINFO_BLOB => e.opts.cainfo_blob = b,
            _ => {}
        });
        return Ok(true);
    }
    // Strings.
    if (10000..20000).contains(&opt) {
        let s = opt_str(vm, &v)?;
        if let Some(b) = &s
            && has_nul(b)
        {
            return vm.fthrow(
                a,
                "ValueError",
                "cURL option must not contain any null bytes",
            );
        }
        return set_string(vm, a, o, opt, s);
    }
    // Integers (long and curl_off_t options).
    let n = vm.to_int(&v)?;
    set_long(vm, a, o, opt, n)
}

fn set_string(vm: &mut Vm, a: &Args, o: &ObjRef, opt: i64, s: Option<Vec<u8>>) -> R<bool> {
    match opt {
        c::CURLOPT_COOKIEFILE | c::CURLOPT_COOKIEJAR => {
            if let Some(p) = &s
                && !p.is_empty()
                && !vm.basedir_allows(p)
            {
                let bd = vm
                    .get_ini(b"open_basedir")
                    .map(|v| v.to_string_lossy())
                    .unwrap_or_default();
                vm.fwarn(
                    a,
                    &format!(
                        "open_basedir restriction in effect. File({}) is not within the allowed path(s): ({})",
                        String::from_utf8_lossy(p),
                        bd
                    ),
                )?;
                return Ok(false);
            }
        }
        c::CURLOPT_PROTOCOLS_STR | c::CURLOPT_REDIR_PROTOCOLS_STR => {
            let Some(mask) = s.as_deref().map(parse_protocols_str) else {
                return Ok(true);
            };
            let Some(mask) = mask else {
                return Ok(false);
            };
            if mask & c::CURLPROTO_FILE != 0 && basedir_set(vm) {
                vm.fwarn(
                    a,
                    "The FILE protocol cannot be activated when an open_basedir is set",
                )?;
                return Ok(false);
            }
            with_easy(o, |e| {
                if opt == c::CURLOPT_PROTOCOLS_STR {
                    e.opts.protocols = mask;
                } else {
                    e.opts.redir_protocols = mask;
                }
            });
            return Ok(true);
        }
        c::CURLOPT_COOKIELIST => {
            let Some(cmd) = s else { return Ok(true) };
            return cookielist(vm, o, &cmd).map(|_| true);
        }
        c::CURLOPT_SSL_SIGNATURE_ALGORITHMS => return Ok(true),
        _ => {}
    }
    if opt == c::CURLOPT_URL {
        with_easy(o, |e| set_url(e, s));
        return Ok(true);
    }
    with_easy(o, |e| {
        let x = &mut e.opts;
        match opt {
            c::CURLOPT_PROXY => x.proxy = s,
            c::CURLOPT_USERPWD => x.userpwd = s,
            c::CURLOPT_PROXYUSERPWD => x.proxyuserpwd = s,
            c::CURLOPT_RANGE => x.range = s,
            c::CURLOPT_REFERER => x.referer = s,
            c::CURLOPT_USERAGENT => x.useragent = s,
            c::CURLOPT_COOKIE => x.cookie = s,
            c::CURLOPT_CUSTOMREQUEST => x.custom_request = s,
            c::CURLOPT_ACCEPT_ENCODING => x.accept_encoding = s,
            c::CURLOPT_INTERFACE => x.interface = s,
            c::CURLOPT_CAINFO => x.cainfo = s,
            c::CURLOPT_CAPATH => x.capath = s,
            c::CURLOPT_SSLCERT => x.sslcert = s,
            c::CURLOPT_SSLKEY => x.sslkey = s,
            c::CURLOPT_KEYPASSWD => x.keypasswd = s,
            c::CURLOPT_ISSUERCERT => x.issuercert = s,
            c::CURLOPT_USERNAME => x.username = s,
            c::CURLOPT_PASSWORD => x.password = s,
            c::CURLOPT_PROXYUSERNAME => x.proxyusername = s,
            c::CURLOPT_PROXYPASSWORD => x.proxypassword = s,
            c::CURLOPT_NOPROXY => x.noproxy = s,
            c::CURLOPT_UNIX_SOCKET_PATH => x.unix_socket_path = s,
            c::CURLOPT_DEFAULT_PROTOCOL => {
                x.default_protocol = s.map(|v| String::from_utf8_lossy(&v).to_ascii_lowercase())
            }
            c::CURLOPT_REQUEST_TARGET => x.request_target = s,
            c::CURLOPT_COOKIEFILE => {
                if let Some(p) = s {
                    e.jar.borrow_mut().enabled = true;
                    if !p.is_empty() {
                        e.cookie_files.push(p.clone());
                        e.all_cookie_files.push(p);
                    }
                }
            }
            c::CURLOPT_COOKIEJAR => {
                if s.is_some() {
                    e.jar.borrow_mut().enabled = true;
                }
                e.cookiejar = s;
            }
            _ => {}
        }
    });
    Ok(true)
}

/// CURLOPT_COOKIELIST (libcurl documentation): "ALL", "SESS", "FLUSH",
/// "RELOAD", or a cookie line.
fn cookielist(vm: &mut Vm, o: &ObjRef, cmd: &[u8]) -> R<()> {
    let Some((jar, jarfile, files)) = with_easy(o, |e| {
        (
            current_jar(vm, e),
            e.cookiejar.clone(),
            e.all_cookie_files.clone(),
        )
    }) else {
        return Ok(());
    };
    match cmd {
        b"ALL" => jar.borrow_mut().clear(),
        b"SESS" => jar.borrow_mut().clear_session(),
        b"FLUSH" => {
            if let Some(p) = jarfile {
                write_jar(vm, &p, &jar);
            }
        }
        b"RELOAD" => {
            for f in files {
                if let Ok(d) = std::fs::read(vm.fs_path(&f)) {
                    jar.borrow_mut().load(&d);
                }
            }
        }
        _ => {
            let mut j = jar.borrow_mut();
            j.enabled = true;
            j.add_line(cmd);
        }
    }
    Ok(())
}

/// The cookie store a handle uses: its share's when cookies are shared.
fn current_jar(_vm: &Vm, e: &Easy) -> Rc<RefCell<CookieJar>> {
    if let Some(s) = &e.share
        && let Some(j) = super::multi::share_jar(s)
    {
        return j;
    }
    e.jar.clone()
}

fn set_long(vm: &mut Vm, a: &Args, o: &ObjRef, opt: i64, n: i64) -> R<bool> {
    match opt {
        c::CURLOPT_SSL_VERIFYHOST => {
            let n = if n == 1 {
                vm.fnotice(
                    a,
                    "CURLOPT_SSL_VERIFYHOST no longer accepts the value 1, value 2 will be used instead",
                )?;
                2
            } else {
                n
            };
            with_easy(o, |e| e.opts.ssl_verifyhost = n);
            return Ok(true);
        }
        c::CURLOPT_PROTOCOLS | c::CURLOPT_REDIR_PROTOCOLS => {
            if n & c::CURLPROTO_FILE != 0 && basedir_set(vm) {
                vm.fwarn(
                    a,
                    "CURLPROTO_FILE cannot be activated when an open_basedir is set",
                )?;
                return Ok(false);
            }
            with_easy(o, |e| {
                if opt == c::CURLOPT_PROTOCOLS {
                    e.opts.protocols = n;
                } else {
                    e.opts.redir_protocols = n;
                }
            });
            return Ok(true);
        }
        c::CURLOPT_TIMEOUT
        | c::CURLOPT_TIMEOUT_MS
        | c::CURLOPT_CONNECTTIMEOUT
        | c::CURLOPT_CONNECTTIMEOUT_MS
        | c::CURLOPT_EXPECT_100_TIMEOUT_MS
            if n < 0 =>
        {
            return Ok(false);
        }
        c::CURLOPT_FOLLOWLOCATION if !(0..=3).contains(&n) => return Ok(false),
        c::CURLOPT_MAXREDIRS if n < -1 => return Ok(false),
        c::CURLOPT_HTTP_VERSION if !matches!(n, 0..=5 | 30 | 31) => return Ok(false),
        c::CURLOPT_PORT | c::CURLOPT_PROXYPORT | c::CURLOPT_LOCALPORT
            if !(0..=65535).contains(&n) =>
        {
            return Ok(false);
        }
        _ => {}
    }
    let b = n != 0;
    with_easy(o, |e| {
        let x = &mut e.opts;
        match opt {
            c::CURLOPT_PORT => x.port = n,
            c::CURLOPT_TIMEOUT => x.timeout_ms = n.saturating_mul(1000),
            c::CURLOPT_TIMEOUT_MS => x.timeout_ms = n,
            c::CURLOPT_CONNECTTIMEOUT => x.connect_timeout_ms = n.saturating_mul(1000),
            c::CURLOPT_CONNECTTIMEOUT_MS => x.connect_timeout_ms = n,
            c::CURLOPT_EXPECT_100_TIMEOUT_MS => x.expect_100_timeout_ms = n,
            c::CURLOPT_INFILESIZE | c::CURLOPT_INFILESIZE_LARGE => x.infilesize = n,
            c::CURLOPT_VERBOSE => x.verbose = b,
            c::CURLOPT_HEADER => x.header = b,
            c::CURLOPT_NOPROGRESS => x.noprogress = b,
            c::CURLOPT_NOBODY => x.nobody = b,
            c::CURLOPT_FAILONERROR => x.failonerror = b,
            c::CURLOPT_UPLOAD | c::CURLOPT_PUT => x.upload = b,
            c::CURLOPT_POST => x.post = b,
            c::CURLOPT_HTTPGET => {
                if b {
                    x.post = false;
                    x.upload = false;
                    x.nobody = false;
                }
            }
            c::CURLOPT_FOLLOWLOCATION => x.followlocation = n,
            c::CURLOPT_MAXREDIRS => x.maxredirs = n,
            c::CURLOPT_AUTOREFERER => x.autoreferer = b,
            c::CURLOPT_POSTREDIR => x.postredir = n,
            c::CURLOPT_UNRESTRICTED_AUTH => x.unrestricted_auth = b,
            c::CURLOPT_HTTP_VERSION => x.http_version = n,
            c::CURLOPT_HTTPAUTH => x.httpauth = n,
            c::CURLOPT_PROXYAUTH => x.proxyauth = n,
            c::CURLOPT_PROXYPORT => x.proxyport = n,
            c::CURLOPT_PROXYTYPE => x.proxytype = n,
            c::CURLOPT_HTTPPROXYTUNNEL => x.httpproxytunnel = b,
            c::CURLOPT_RESUME_FROM => x.resume_from = n,
            c::CURLOPT_BUFFERSIZE => x.buffersize = n,
            c::CURLOPT_LOCALPORT => x.localport = n,
            c::CURLOPT_IPRESOLVE => x.ipresolve = n,
            c::CURLOPT_SSL_VERIFYPEER => x.ssl_verifypeer = b,
            c::CURLOPT_SSLVERSION => x.sslversion = n,
            c::CURLOPT_CERTINFO => x.certinfo = b,
            c::CURLOPT_FILETIME => x.filetime = b,
            c::CURLOPT_TIMECONDITION => x.timecondition = n,
            c::CURLOPT_TIMEVALUE | c::CURLOPT_TIMEVALUE_LARGE => x.timevalue = n,
            c::CURLOPT_MAXFILESIZE | c::CURLOPT_MAXFILESIZE_LARGE => x.maxfilesize = n,
            c::CURLOPT_IGNORE_CONTENT_LENGTH => x.ignore_content_length = b,
            c::CURLOPT_HTTP_CONTENT_DECODING => x.content_decoding = b,
            c::CURLOPT_HTTP_TRANSFER_DECODING => x.transfer_decoding = b,
            c::CURLOPT_HTTP09_ALLOWED => x.http09_allowed = b,
            c::CURLOPT_PATH_AS_IS => x.path_as_is = b,
            c::CURLOPT_KEEP_SENDING_ON_ERROR => x.keep_sending_on_error = b,
            c::CURLOPT_FORBID_REUSE => x.forbid_reuse = b,
            c::CURLOPT_FRESH_CONNECT => x.fresh_connect = b,
            c::CURLOPT_MIME_OPTIONS => x.mime_formescape = n & c::CURLMIMEOPT_FORMESCAPE != 0,
            c::CURLOPT_APPEND => x.append = b,
            c::CURLOPT_NEW_FILE_PERMS => x.new_file_perms = n,
            c::CURLOPT_LOW_SPEED_LIMIT => x.low_speed_limit = n,
            c::CURLOPT_LOW_SPEED_TIME => x.low_speed_time = n,
            c::CURLOPT_TRANSFERTEXT => x.transfertext = b,
            c::CURLOPT_CRLF => x.crlf = b,
            c::CURLOPT_COOKIESESSION if b => e.jar.borrow_mut().clear_session(),
            _ => {}
        }
    });
    Ok(true)
}

/// CURLOPT_POSTFIELDS: a string body, or an array for a multipart form
/// (CURLFile and CURLStringFile values become file parts; an array value
/// gives one part per element; an empty array is an empty body).
fn set_postfields(vm: &mut Vm, o: &ObjRef, v: Value) -> R<()> {
    let Value::Array(arr) = &v else {
        let s = if v.is_null() {
            Vec::new()
        } else {
            vm.to_php_str(&v)?.as_bytes().to_vec()
        };
        with_easy(o, |e| {
            e.form = None;
            e.opts.body = Body::Fields(s);
            e.opts.post = true;
        });
        return Ok(());
    };
    if arr.is_empty() {
        with_easy(o, |e| {
            e.form = None;
            e.opts.body = Body::Fields(Vec::new());
            e.opts.post = true;
        });
        return Ok(());
    }
    let mut fields = Vec::new();
    for (k, item) in arr.iter() {
        let name = match k {
            goro_runtime::KeyRef::Int(i) => i.to_string().into_bytes(),
            goro_runtime::KeyRef::Str(s) => s.as_bytes().to_vec(),
        };
        let item = item.deref();
        if let Value::Array(sub) = &item {
            for (_, x) in sub.iter() {
                let d = vm.to_php_str(&x.deref())?.as_bytes().to_vec();
                fields.push((name.clone(), FormData::Bytes(d)));
            }
            continue;
        }
        if let Value::Object(fo) = &item {
            let cls = vm.class_of(fo);
            if instance_of(&cls, b"curlfile") {
                let path = vm.get_prop_raw(fo, b"name");
                let mime = vm.get_prop_raw(fo, b"mime");
                let post = vm.get_prop_raw(fo, b"postname");
                fields.push((
                    name,
                    FormData::File {
                        path: vm.to_php_str(&path)?.as_bytes().to_vec(),
                        mime: vm.to_php_str(&mime)?.as_bytes().to_vec(),
                        postname: vm.to_php_str(&post)?.as_bytes().to_vec(),
                    },
                ));
                continue;
            }
            if instance_of(&cls, b"curlstringfile") {
                let data = vm.get_prop_raw(fo, b"data");
                let mime = vm.get_prop_raw(fo, b"mime");
                let post = vm.get_prop_raw(fo, b"postname");
                fields.push((
                    name,
                    FormData::StringFile {
                        data: vm.to_php_str(&data)?.as_bytes().to_vec(),
                        mime: vm.to_php_str(&mime)?.as_bytes().to_vec(),
                        postname: vm.to_php_str(&post)?.as_bytes().to_vec(),
                    },
                ));
                continue;
            }
        }
        let d = vm.to_php_str(&item)?.as_bytes().to_vec();
        fields.push((name, FormData::Bytes(d)));
    }
    with_easy(o, |e| {
        e.form = Some(fields);
        e.opts.body = Body::None;
        e.opts.post = true;
    });
    Ok(())
}

fn instance_of(cls: &crate::class::Class, lname: &[u8]) -> bool {
    let mut k = Some(cls);
    while let Some(c) = k {
        if c.lname.as_bytes() == lname {
            return true;
        }
        k = c.parent.as_deref();
    }
    false
}

// ------------------------------------------------------------ getinfo

fn opt_bytes(v: &Option<Vec<u8>>) -> Value {
    match v {
        Some(b) => Value::from(b.as_slice()),
        None => Value::Bool(false),
    }
}

fn secs(us: i64) -> Value {
    Value::Float(us as f64 / 1_000_000.0)
}

fn certinfo(i: &Info) -> Value {
    let mut out = PhpArray::new();
    for cert in &i.certinfo {
        let mut m = PhpArray::new();
        for (k, v) in cert {
            m.insert(Key::from(k.as_str()), Value::from(v.as_slice()));
        }
        let _ = out.push(Value::Array(m));
    }
    Value::Array(out)
}

const CAPATH: &str = "/etc/ssl/certs";
const CAINFO: &str = "/etc/ssl/certs/ca-certificates.crt";

pub(super) fn curl_getinfo(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = handle_arg(a);
    let opt = a.opt_int(1);
    let Some((i, private, header_out, cookies, capath, cainfo)) = with_easy(&o, |e| {
        (
            e.info.clone(),
            e.private.clone(),
            e.info.header_out.clone(),
            current_jar(vm, e).borrow_mut().lines(),
            e.opts.capath.clone(),
            e.opts.cainfo.clone(),
        )
    }) else {
        return Ok(Value::Bool(false));
    };
    let capath = capath.unwrap_or_else(|| CAPATH.as_bytes().to_vec());
    let cainfo = cainfo.unwrap_or_else(|| CAINFO.as_bytes().to_vec());
    let s_or_empty = |s: &str| Value::from(s.as_bytes());
    let Some(opt) = opt else {
        let mut m = PhpArray::new();
        let mut set = |k: &str, v: Value| {
            m.insert(Key::from(k), v);
        };
        if let Some(u) = &i.effective_url {
            set("url", Value::from(u.as_slice()));
        }
        set(
            "content_type",
            match &i.content_type {
                Some(t) => Value::from(t.as_slice()),
                None => Value::Null,
            },
        );
        set("http_code", Value::Int(i.response_code));
        set("header_size", Value::Int(i.header_size));
        set("request_size", Value::Int(i.request_size));
        set("filetime", Value::Int(i.filetime));
        set("ssl_verify_result", Value::Int(i.ssl_verify_result));
        set("redirect_count", Value::Int(i.redirect_count));
        set("total_time", secs(i.total_us));
        set("namelookup_time", secs(i.namelookup_us));
        set("connect_time", secs(i.connect_us));
        set("pretransfer_time", secs(i.pretransfer_us));
        set("size_upload", Value::Float(i.size_upload as f64));
        set("size_download", Value::Float(i.size_download as f64));
        set("speed_download", Value::Float(i.speed_download() as f64));
        set("speed_upload", Value::Float(i.speed_upload() as f64));
        set(
            "download_content_length",
            Value::Float(i.download_content_length as f64),
        );
        set(
            "upload_content_length",
            Value::Float(i.upload_content_length as f64),
        );
        set("starttransfer_time", secs(i.starttransfer_us));
        set("redirect_time", secs(i.redirect_us));
        set(
            "redirect_url",
            Value::from(i.redirect_url.as_deref().unwrap_or(b"")),
        );
        set("primary_ip", s_or_empty(&i.primary_ip));
        set("certinfo", certinfo(&i));
        set("primary_port", Value::Int(i.primary_port));
        set("local_ip", s_or_empty(&i.local_ip));
        set("local_port", Value::Int(i.local_port));
        set("http_version", Value::Int(i.http_version));
        set("protocol", Value::Int(i.protocol));
        set("ssl_verifyresult", Value::Int(0));
        set("scheme", s_or_empty(i.scheme.as_deref().unwrap_or("")));
        set("appconnect_time_us", Value::Int(i.appconnect_us));
        set("queue_time_us", Value::Int(i.queue_us));
        set("connect_time_us", Value::Int(i.connect_us));
        set("namelookup_time_us", Value::Int(i.namelookup_us));
        set("pretransfer_time_us", Value::Int(i.pretransfer_us));
        set("redirect_time_us", Value::Int(i.redirect_us));
        set("starttransfer_time_us", Value::Int(i.starttransfer_us));
        set("posttransfer_time_us", Value::Int(i.posttransfer_us));
        set("total_time_us", Value::Int(i.total_us));
        set(
            "effective_method",
            Value::from(i.effective_method.as_slice()),
        );
        set("capath", Value::from(capath.as_slice()));
        set("cainfo", Value::from(cainfo.as_slice()));
        set("used_proxy", Value::Int(i.used_proxy));
        set("httpauth_used", Value::Int(i.httpauth_used));
        set("proxyauth_used", Value::Int(i.proxyauth_used));
        set("conn_id", Value::Int(i.conn_id));
        if let Some(h) = &header_out {
            set("request_header", Value::from(h.as_slice()));
        }
        return Ok(Value::Array(m));
    };
    Ok(match opt {
        c::CURLINFO_PRIVATE => private.unwrap_or(Value::Bool(false)),
        c::CURLINFO_HEADER_OUT => opt_bytes(&header_out),
        c::CURLINFO_EFFECTIVE_URL => opt_bytes(&i.effective_url),
        c::CURLINFO_CONTENT_TYPE => opt_bytes(&i.content_type),
        c::CURLINFO_REDIRECT_URL => opt_bytes(&i.redirect_url),
        c::CURLINFO_PRIMARY_IP => s_or_empty(&i.primary_ip),
        c::CURLINFO_LOCAL_IP => s_or_empty(&i.local_ip),
        c::CURLINFO_SCHEME => match &i.scheme {
            Some(s) => Value::from(s.as_bytes()),
            None => Value::Bool(false),
        },
        c::CURLINFO_EFFECTIVE_METHOD => Value::from(i.effective_method.as_slice()),
        c::CURLINFO_CAPATH => Value::from(capath.as_slice()),
        c::CURLINFO_CAINFO => Value::from(cainfo.as_slice()),
        c::CURLINFO_REFERER => opt_bytes(&i.referer),
        c::CURLINFO_FTP_ENTRY_PATH | c::CURLINFO_RTSP_SESSION_ID => Value::Bool(false),
        c::CURLINFO_RESPONSE_CODE => Value::Int(i.response_code),
        c::CURLINFO_HTTP_CONNECTCODE => Value::Int(i.http_connectcode),
        c::CURLINFO_HEADER_SIZE => Value::Int(i.header_size),
        c::CURLINFO_REQUEST_SIZE => Value::Int(i.request_size),
        c::CURLINFO_FILETIME | c::CURLINFO_FILETIME_T => Value::Int(i.filetime),
        c::CURLINFO_SSL_VERIFYRESULT => Value::Int(i.ssl_verify_result),
        c::CURLINFO_PROXY_SSL_VERIFYRESULT => Value::Int(0),
        c::CURLINFO_REDIRECT_COUNT => Value::Int(i.redirect_count),
        c::CURLINFO_HTTPAUTH_AVAIL => Value::Int(i.httpauth_avail),
        c::CURLINFO_PROXYAUTH_AVAIL => Value::Int(i.proxyauth_avail),
        c::CURLINFO_HTTPAUTH_USED => Value::Int(i.httpauth_used),
        c::CURLINFO_PROXYAUTH_USED => Value::Int(i.proxyauth_used),
        c::CURLINFO_OS_ERRNO => Value::Int(i.os_errno),
        c::CURLINFO_NUM_CONNECTS => Value::Int(i.num_connects),
        c::CURLINFO_CONDITION_UNMET => Value::Int(i.condition_unmet),
        c::CURLINFO_RTSP_CLIENT_CSEQ
        | c::CURLINFO_RTSP_CSEQ_RECV
        | c::CURLINFO_RTSP_SERVER_CSEQ => Value::Int(0),
        c::CURLINFO_LOCAL_PORT => Value::Int(i.local_port),
        c::CURLINFO_PRIMARY_PORT => Value::Int(i.primary_port),
        c::CURLINFO_HTTP_VERSION => Value::Int(i.http_version),
        c::CURLINFO_PROTOCOL => Value::Int(i.protocol),
        c::CURLINFO_USED_PROXY => Value::Int(i.used_proxy),
        c::CURLINFO_PROXY_ERROR => Value::Int(0),
        c::CURLINFO_RETRY_AFTER => Value::Int(i.retry_after),
        c::CURLINFO_CONN_ID => Value::Int(i.conn_id),
        c::CURLINFO_TOTAL_TIME => secs(i.total_us),
        c::CURLINFO_NAMELOOKUP_TIME => secs(i.namelookup_us),
        c::CURLINFO_CONNECT_TIME => secs(i.connect_us),
        c::CURLINFO_APPCONNECT_TIME => secs(i.appconnect_us),
        c::CURLINFO_PRETRANSFER_TIME => secs(i.pretransfer_us),
        c::CURLINFO_STARTTRANSFER_TIME => secs(i.starttransfer_us),
        c::CURLINFO_REDIRECT_TIME => secs(i.redirect_us),
        c::CURLINFO_SIZE_UPLOAD => Value::Float(i.size_upload as f64),
        c::CURLINFO_SIZE_DOWNLOAD => Value::Float(i.size_download as f64),
        c::CURLINFO_SPEED_DOWNLOAD => Value::Float(i.speed_download() as f64),
        c::CURLINFO_SPEED_UPLOAD => Value::Float(i.speed_upload() as f64),
        c::CURLINFO_CONTENT_LENGTH_DOWNLOAD => Value::Float(i.download_content_length as f64),
        c::CURLINFO_CONTENT_LENGTH_UPLOAD => Value::Float(i.upload_content_length as f64),
        c::CURLINFO_SIZE_UPLOAD_T => Value::Int(i.size_upload),
        c::CURLINFO_SIZE_DOWNLOAD_T => Value::Int(i.size_download),
        c::CURLINFO_SPEED_DOWNLOAD_T => Value::Int(i.speed_download()),
        c::CURLINFO_SPEED_UPLOAD_T => Value::Int(i.speed_upload()),
        c::CURLINFO_CONTENT_LENGTH_DOWNLOAD_T => Value::Int(i.download_content_length),
        c::CURLINFO_CONTENT_LENGTH_UPLOAD_T => Value::Int(i.upload_content_length),
        c::CURLINFO_QUEUE_TIME_T => Value::Int(i.queue_us),
        c::CURLINFO_APPCONNECT_TIME_T => Value::Int(i.appconnect_us),
        c::CURLINFO_CONNECT_TIME_T => Value::Int(i.connect_us),
        c::CURLINFO_NAMELOOKUP_TIME_T => Value::Int(i.namelookup_us),
        c::CURLINFO_PRETRANSFER_TIME_T => Value::Int(i.pretransfer_us),
        c::CURLINFO_REDIRECT_TIME_T => Value::Int(i.redirect_us),
        c::CURLINFO_STARTTRANSFER_TIME_T => Value::Int(i.starttransfer_us),
        c::CURLINFO_POSTTRANSFER_TIME_T => Value::Int(i.posttransfer_us),
        c::CURLINFO_TOTAL_TIME_T => Value::Int(i.total_us),
        c::CURLINFO_COOKIELIST => {
            let mut arr = PhpArray::new();
            for l in cookies {
                let _ = arr.push(Value::from(l.as_slice()));
            }
            Value::Array(arr)
        }
        c::CURLINFO_CERTINFO => certinfo(&i),
        c::CURLINFO_SSL_ENGINES => Value::Array(PhpArray::new()),
        _ => Value::Bool(false),
    })
}

// ------------------------------------------------------------ exec

pub(super) fn curl_exec(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = handle_arg(a);
    let fname = a.fname();
    if with_easy(&o, |e| e.in_multi).unwrap_or(false) {
        // Observed on PHP 8.5: CURLE_FAILED_INIT.
        with_easy(&o, |e| {
            e.errno = c::CURLE_FAILED_INIT;
            e.error = "easy handle already used in multi handle".into();
        });
        return Ok(Value::Bool(false));
    }
    check_streams(vm, &fname, &o)?;
    let r = run(vm, a, &o, None)?;
    let mode = with_easy(&o, |e| e.write_mode).unwrap_or(WriteMode::Stdout);
    Ok(match r {
        Err(_) => Value::Bool(false),
        Ok(()) if mode == WriteMode::Return => {
            let b = with_easy(&o, |e| e.content.clone().unwrap_or_default()).unwrap_or_default();
            Value::from(b.as_slice())
        }
        Ok(()) => Value::Bool(true),
    })
}

/// The stream options whose resources were closed are reset, with a
/// warning (tests bug48203.phpt, bug54798.phpt, bug48203_multi.phpt).
pub(super) fn check_streams(vm: &mut Vm, fname: &str, o: &ObjRef) -> R<()> {
    let Some((verbose, stderr, writeheader, file, infile)) = with_easy(o, |e| {
        (
            e.opts.verbose,
            e.stderr.clone(),
            e.writeheader.clone(),
            e.file.clone(),
            e.infile.clone(),
        )
    }) else {
        return Ok(());
    };
    if let Some(s) = stderr
        && !stream_alive(&s)
    {
        let _ = verbose;
        vm.warn(&format!(
            "{fname}(): CURLOPT_STDERR resource has gone away, resetting to stderr"
        ))?;
        with_easy(o, |e| e.stderr = None);
    }
    if let Some(s) = writeheader
        && !stream_alive(&s)
    {
        vm.warn(&format!(
            "{fname}(): CURLOPT_WRITEHEADER resource has gone away, resetting to default"
        ))?;
        with_easy(o, |e| e.writeheader = None);
    }
    if let Some(s) = file
        && !stream_alive(&s)
    {
        vm.warn(&format!(
            "{fname}(): CURLOPT_FILE resource has gone away, resetting to default"
        ))?;
        with_easy(o, |e| {
            e.file = None;
            e.write_mode = WriteMode::Stdout;
        });
    }
    if let Some(s) = infile
        && !stream_alive(&s)
    {
        vm.warn(&format!(
            "{fname}(): CURLOPT_INFILE resource has gone away, resetting to default"
        ))?;
        with_easy(o, |e| e.infile = None);
    }
    Ok(())
}

/// Runs a transfer on handle `o` (with the connection cache `pool`, or
/// the handle's own) and stores its results in the handle.
pub(super) fn run(
    vm: &mut Vm,
    a: &Args,
    o: &ObjRef,
    pool: Option<Rc<RefCell<Pool>>>,
) -> R<Result<(), Failure>> {
    let Some(snapshot) = with_easy(o, |e| {
        if e.running {
            return None;
        }
        e.running = true;
        Some(e.clone())
    })
    .flatten() else {
        let f = Failure {
            code: 93,
            message: "API function called from within callback".into(),
        };
        with_easy(o, |e| {
            e.errno = f.code;
            e.error = f.message.clone();
        });
        return Ok(Err(f));
    };
    let r = run_inner(vm, a, o, snapshot, pool);
    with_easy(o, |e| e.running = false);
    r
}

fn run_inner(
    vm: &mut Vm,
    a: &Args,
    o: &ObjRef,
    mut e: Easy,
    pool: Option<Rc<RefCell<Pool>>>,
) -> R<Result<(), Failure>> {
    let jar = current_jar(vm, &e);
    // Cookie files are read when a transfer starts.
    if !e.cookie_files.is_empty() {
        for f in std::mem::take(&mut e.cookie_files) {
            if let Ok(d) = std::fs::read(vm.fs_path(&f)) {
                jar.borrow_mut().load(&d);
            }
        }
        with_easy(o, |h| h.cookie_files.clear());
    }
    let mut opts = e.opts.clone();
    opts.record_header_out = e.header_out || (e.debugfunc.is_some() && opts.verbose);
    if let Some(form) = &e.form {
        match build_form(vm, a, form)? {
            Ok(parts) => opts.body = Body::Form(parts),
            Err(f) => {
                with_easy(o, |h| {
                    h.errno = f.code;
                    h.error = f.message.clone();
                });
                return Ok(Err(f));
            }
        }
    }
    let pool = pool
        .or_else(|| e.share.as_ref().and_then(super::multi::share_pool))
        .unwrap_or_else(|| e.pool.clone());
    let fname = a.fname();
    let mut host = ExecHost {
        vm,
        fname,
        handle: o.clone(),
        e,
        buffer: Vec::new(),
        err: None,
        stop: false,
        rng: 0,
    };
    let (mut info, result) = {
        let mut jar_ref = jar.borrow_mut();
        let mut pool_ref = pool.borrow_mut();
        transfer::perform(&opts, &mut host, &mut jar_ref, &mut pool_ref)
    };
    if !opts.record_header_out {
        info.header_out = None;
    }
    let ExecHost {
        buffer,
        err,
        e: snap,
        ..
    } = host;
    let mode = snap.write_mode;
    with_easy(o, |h| {
        h.info = info;
        match &result {
            Ok(()) => {
                h.errno = 0;
                h.error.clear();
            }
            Err(f) => {
                h.errno = f.code;
                h.error = f.message.clone();
            }
        }
        if mode == WriteMode::Return {
            h.content = Some(buffer);
        }
    });
    if let Some(u) = err {
        return Err(u);
    }
    Ok(result)
}

/// Reads the files of a form (CURLFile names go through the stream
/// layer, so data: URLs work: test curl_file_upload_stream.phpt).
fn build_form(
    vm: &mut Vm,
    a: &Args,
    form: &[(Vec<u8>, FormData)],
) -> R<Result<Vec<Part>, Failure>> {
    let mut parts = Vec::new();
    let cstr = |b: &[u8]| -> Vec<u8> {
        // C strings: the value ends at a NUL byte (test
        // curl_string_file_upload.phpt).
        match b.iter().position(|&c| c == 0) {
            Some(p) => b[..p].to_vec(),
            None => b.to_vec(),
        }
    };
    for (name, d) in form {
        let part = match d {
            FormData::Bytes(b) => Part {
                name: name.clone(),
                filename: None,
                content_type: None,
                data: b.clone(),
            },
            FormData::StringFile {
                data,
                mime,
                postname,
            } => Part {
                name: name.clone(),
                filename: Some(cstr(postname)),
                content_type: Some(if mime.is_empty() {
                    b"application/octet-stream".to_vec()
                } else {
                    cstr(mime)
                }),
                data: data.clone(),
            },
            FormData::File {
                path,
                mime,
                postname,
            } => {
                let data = match super::super::file::open_stream(vm, a, path, b"rb", true)? {
                    Some(mut s) => {
                        let d = super::super::file::read_rest(vm, &mut s);
                        drop(s);
                        d
                    }
                    None => {
                        return Ok(Err(Failure {
                            code: c::CURLE_READ_ERROR,
                            message: "Failed to open/read local data from file/application"
                                .to_string(),
                        }));
                    }
                };
                let filename = if postname.is_empty() {
                    let p = path.as_slice();
                    let b = match p.iter().rposition(|&c| c == b'/') {
                        Some(i) => &p[i + 1..],
                        None => p,
                    };
                    b.to_vec()
                } else {
                    cstr(postname)
                };
                Part {
                    name: name.clone(),
                    filename: Some(filename),
                    content_type: Some(if mime.is_empty() {
                        b"application/octet-stream".to_vec()
                    } else {
                        cstr(mime)
                    }),
                    data,
                }
            }
        };
        parts.push(part);
    }
    Ok(Ok(parts))
}

/// The application side of a running transfer.
struct ExecHost<'v> {
    vm: &'v mut Vm,
    fname: String,
    handle: ObjRef,
    e: Easy,
    buffer: Vec<u8>,
    /// A PHP error raised by a callback (thrown after the transfer).
    err: Option<Unwind>,
    /// Whether the transfer must stop.
    stop: bool,
    rng: u64,
}

impl ExecHost<'_> {
    /// Calls a callback. None when it threw (the exception waits for the
    /// end of the transfer, which goes on as if the callback had accepted
    /// everything: observed on PHP 8.5) or when one threw earlier (no more
    /// callbacks run); an exit or fatal error stops the transfer.
    fn call(&mut self, cb: &Value, args: Vec<Value>) -> Option<Value> {
        if self.err.is_some() {
            return None;
        }
        match self.vm.call_value(cb, args) {
            Ok(v) => Some(v),
            Err(u) => {
                if !matches!(u, Unwind::Throw(_)) {
                    self.stop = true;
                }
                self.err = Some(u);
                None
            }
        }
    }

    fn me(&self) -> Value {
        Value::Object(self.handle.clone())
    }

    fn int_of(&mut self, v: &Value) -> i64 {
        match self.vm.to_int(v) {
            Ok(n) => n,
            Err(u) => {
                self.err = Some(u);
                self.stop = true;
                0
            }
        }
    }

    /// Writes to a stream option; false when the resource is gone.
    fn stream_write(&mut self, v: &Value, data: &[u8]) -> bool {
        let Some(Some(cell)) = stream_of(v) else {
            return false;
        };
        // SAFETY: the resource is held by `v` (in the handle snapshot) for
        // the call.
        let mut s = unsafe { (*cell).borrow_mut() };
        let _ = s.write_all(self.vm, data);
        true
    }

    fn stderr_write(&mut self, data: &[u8]) {
        if let Some(s) = self.e.stderr.clone() {
            if self.stream_write(&s, data) {
                return;
            }
            self.e.stderr = None;
        }
        use std::io::Write;
        self.vm.out.flush();
        let _ = std::io::stderr().write_all(data);
    }
}

impl Host for ExecHost<'_> {
    fn write(&mut self, data: &[u8]) -> usize {
        if self.stop {
            return 0;
        }
        match self.e.write_mode {
            WriteMode::Return => {
                self.buffer.extend_from_slice(data);
                data.len()
            }
            WriteMode::Stdout => {
                self.vm.echo(data);
                data.len()
            }
            WriteMode::File => {
                let f = self.e.file.clone().unwrap_or(Value::Null);
                if self.stream_write(&f, data) {
                    return data.len();
                }
                // The stream was closed during the transfer (test
                // bug69316.phpt): back to standard output.
                let msg = format!(
                    "{}(): CURLOPT_FILE resource has gone away, resetting to default",
                    self.fname
                );
                if let Err(u) = self.vm.warn(&msg) {
                    self.err = Some(u);
                    self.stop = true;
                    return 0;
                }
                self.e.write_mode = WriteMode::Stdout;
                self.e.file = None;
                with_easy(&self.handle, |h| {
                    if h.write_mode == WriteMode::File {
                        h.write_mode = WriteMode::Stdout;
                        h.file = None;
                    }
                });
                self.vm.echo(data);
                data.len()
            }
            WriteMode::User => {
                let Some(cb) = self.e.writefunc.clone() else {
                    self.vm.echo(data);
                    return data.len();
                };
                let me = self.me();
                match self.call(&cb, vec![me, Value::from(data)]) {
                    Some(r) => {
                        let n = self.int_of(&r);
                        if n < 0 { usize::MAX } else { n as usize }
                    }
                    None if self.stop => 0,
                    None => data.len(),
                }
            }
        }
    }

    fn header(&mut self, data: &[u8]) -> usize {
        if self.stop {
            return 0;
        }
        if let Some(cb) = self.e.headerfunc.clone() {
            let me = self.me();
            return match self.call(&cb, vec![me, Value::from(data)]) {
                Some(r) => {
                    let n = self.int_of(&r);
                    if n < 0 { usize::MAX } else { n as usize }
                }
                None if self.stop => 0,
                None => data.len(),
            };
        }
        if let Some(s) = self.e.writeheader.clone()
            && !self.stream_write(&s, data)
        {
            let msg = format!(
                "{}(): CURLOPT_WRITEHEADER resource has gone away, resetting to default",
                self.fname
            );
            if let Err(u) = self.vm.warn(&msg) {
                self.err = Some(u);
                self.stop = true;
                return 0;
            }
            self.e.writeheader = None;
            with_easy(&self.handle, |h| h.writeheader = None);
        }
        data.len()
    }

    fn read(&mut self, max: usize) -> ReadResult {
        if self.stop {
            return ReadResult::Abort;
        }
        if let Some(cb) = self.e.readfunc.clone() {
            let me = self.me();
            let inf = self.e.infile.clone().unwrap_or(Value::Null);
            let Some(r) = self.call(&cb, vec![me, inf, Value::Int(max as i64)]) else {
                return if self.stop {
                    ReadResult::Abort
                } else {
                    ReadResult::Data(Vec::new())
                };
            };
            return match r.deref() {
                Value::Str(s) => {
                    let mut d = s.as_bytes().to_vec();
                    d.truncate(max);
                    ReadResult::Data(d)
                }
                Value::Int(n) if n == c::CURL_READFUNC_PAUSE => ReadResult::Pause,
                Value::Int(0x1000_0000) => ReadResult::Abort,
                Value::Int(_) => ReadResult::Funny,
                _ => ReadResult::Data(Vec::new()),
            };
        }
        if let Some(inf) = self.e.infile.clone()
            && let Some(Some(cell)) = stream_of(&inf)
        {
            // SAFETY: the resource is held by `inf` for the call.
            let mut s = unsafe { (*cell).borrow_mut() };
            let mut buf = vec![0u8; max];
            let n = s.read_some(self.vm, &mut buf).unwrap_or(0);
            buf.truncate(n);
            return ReadResult::Data(buf);
        }
        ReadResult::Data(Vec::new())
    }

    fn progress(&mut self, dltotal: i64, dlnow: i64, ultotal: i64, ulnow: i64) -> bool {
        if self.stop {
            return false;
        }
        let Some(cb) = self
            .e
            .xferinfofunc
            .clone()
            .or_else(|| self.e.progressfunc.clone())
        else {
            return true;
        };
        let me = self.me();
        let args = vec![
            me,
            Value::Int(dltotal),
            Value::Int(dlnow),
            Value::Int(ultotal),
            Value::Int(ulnow),
        ];
        match self.call(&cb, args) {
            Some(r) => self.int_of(&r) == 0,
            None => !self.stop,
        }
    }

    fn debug(&mut self, kind: i64, data: &[u8]) {
        if let Some(cb) = self.e.debugfunc.clone() {
            // An exception in the debug callback is thrown after the
            // transfer, which goes on (test
            // curl_setopt_CURLOPT_DEBUGFUNCTION.phpt).
            let me = self.me();
            let _ = self.call(&cb, vec![me, Value::Int(kind), Value::from(data)]);
            return;
        }
        let prefix: &[u8] = match kind {
            0 => b"* ",
            1 => b"< ",
            2 => b"> ",
            _ => return,
        };
        let mut out = Vec::with_capacity(data.len() + 2);
        out.extend_from_slice(prefix);
        out.extend_from_slice(data);
        self.stderr_write(&out);
    }

    fn prereq(
        &mut self,
        remote_ip: &str,
        local_ip: &str,
        remote_port: u16,
        local_port: u16,
    ) -> Option<bool> {
        let cb = self.e.prereqfunc.clone()?;
        let me = self.me();
        let args = vec![
            me,
            Value::from(remote_ip.as_bytes()),
            Value::from(local_ip.as_bytes()),
            Value::Int(remote_port as i64),
            Value::Int(local_port as i64),
        ];
        let Some(r) = self.call(&cb, args) else {
            self.stop = true;
            return Some(false);
        };
        const MSG: &str = "The CURLOPT_PREREQFUNCTION callback must return either CURL_PREREQFUNC_OK or CURL_PREREQFUNC_ABORT";
        match r.deref() {
            Value::Int(0) => Some(true),
            Value::Int(1) => Some(false),
            Value::Int(_) => {
                let e = self.vm.throw::<()>("ValueError", MSG);
                if let Err(u) = e {
                    self.err = Some(u);
                }
                self.stop = true;
                Some(false)
            }
            _ => {
                let e = self.vm.throw::<()>("TypeError", MSG);
                if let Err(u) = e {
                    self.err = Some(u);
                }
                self.stop = true;
                Some(false)
            }
        }
    }

    fn stopped(&self) -> bool {
        self.stop
    }

    fn random(&mut self) -> u64 {
        if self.rng == 0 {
            let t = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(1);
            self.rng = t ^ ((self.handle.handle() as u64) << 32) | 1;
        }
        // xorshift64*
        let mut x = self.rng;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.rng = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 11
    }
}
