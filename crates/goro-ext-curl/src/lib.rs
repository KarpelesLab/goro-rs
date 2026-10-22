//! ext/curl: a port of php-src's ext/curl (interface.c, multi.c, share.c,
//! curl_file.c) on top of `engine`, a pure-Rust emulation of libcurl.

pub mod engine;

mod consts;
mod curl_file;
mod multi;
mod share;
mod zpp;

use std::cell::RefCell;
use std::rc::Rc;

use goro_core::array::{ArrayKey, PhpArray};
use goro_core::object::{ClassEntry, NativeData, PhpObject};
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{NativeClassHandler, NativeObjectHandlers, Vm, VmError};
use goro_ext_standard::curl_streams as streams;

use consts::c::*;
use engine::{Easy, InfoVal, Mime, MimeData, MimePart, OptVal, ReadResult};
use zpp::{Arg, Obj};

// php_curl handler methods (curl_private.h)
pub(crate) const PHP_CURL_STDOUT: i32 = 0;
pub(crate) const PHP_CURL_FILE: i32 = 1;
pub(crate) const PHP_CURL_USER: i32 = 2;
pub(crate) const PHP_CURL_DIRECT: i32 = 3;
pub(crate) const PHP_CURL_RETURN: i32 = 4;
pub(crate) const PHP_CURL_IGNORE: i32 = 7;

// libcurl options and values PHP uses but does not expose
const CURLOPT_ERRORBUFFER: i64 = 10010;
const CURLOPT_POSTFIELDSIZE: i64 = 60;
const CURLOPT_COPYPOSTFIELDS: i64 = 10165;
const CURLOPT_MIMEPOST: i64 = 10269;
const CURLOPT_PROGRESSFUNCTION_C: i64 = 20056;
const CURLOPT_XFERINFOFUNCTION_C: i64 = 20219;
const CURLOPT_SEEKFUNCTION: i64 = 20167;
const CURLPROTO_ALL: i64 = !0;
const CURL_REDIR_POST_ALL: i64 = 7;
const CURLE_RECURSIVE_API_CALL: i64 = 93;
const CURLE_UNKNOWN_OPTION: i64 = 48;
const CURL_READFUNC_ABORT_V: i64 = 0x10000000;
const CURL_READFUNC_PAUSE_V: i64 = 0x10000001;

const CURLINFO_STRING: i64 = 0x100000;
const CURLINFO_LONG: i64 = 0x200000;
const CURLINFO_DOUBLE: i64 = 0x300000;
const CURLINFO_SLIST: i64 = 0x400000;
const CURLINFO_OFF_T: i64 = 0x600000;
const CURLINFO_TYPEMASK: i64 = 0xf00000;

#[derive(Clone, Default)]
pub(crate) struct WriteH {
    pub method: i32,
    pub fcc: Option<Value>,
    pub stream: Option<Value>,
    pub buf: Vec<u8>,
    /// smart_str buf.s allocated (something was written)
    pub buf_set: bool,
}

#[derive(Clone, Default)]
pub(crate) struct ReadH {
    pub method: i32,
    pub fcc: Option<Value>,
    pub stream: Option<Value>,
}

#[derive(Clone, Default)]
pub(crate) struct Handlers {
    pub write: WriteH,
    pub write_header: WriteH,
    pub read: ReadH,
    pub std_err: Option<Value>,
    pub progress: Option<Value>,
    pub xferinfo: Option<Value>,
    pub fnmatch: Option<Value>,
    pub debug: Option<Value>,
    pub prereq: Option<Value>,
    pub sshhostkey: Option<Value>,
}

#[derive(Default)]
pub(crate) struct CurlState {
    pub handlers: Handlers,
    /// ch->header.str (CURLINFO_HEADER_OUT)
    pub header_out: Option<Vec<u8>>,
    pub err_no: i64,
    pub err_str: Vec<u8>,
    pub in_callback: bool,
    pub postfields: Option<Value>,
    pub private_data: Option<Value>,
    pub share: Option<Value>,
    /// The multi handle (object id) this handle was added to.
    pub multi: Option<u64>,
}

/// php_curl
pub(crate) struct CurlObj {
    /// None while a transfer is running (curl_easy_perform has the handle).
    pub easy: RefCell<Option<Easy>>,
    pub st: RefCell<CurlState>,
}

impl Drop for CurlObj {
    fn drop(&mut self) {
        // curl_easy_cleanup(): the cookie jar is written out.
        if let Some(e) = self.easy.get_mut().as_mut() {
            e.flush_cookies();
        }
    }
}

/// zend_objects_not_comparable
fn not_comparable(a: &Value, b: &Value) -> Option<i64> {
    match (a, b) {
        (Value::Object(x), Value::Object(y)) if Rc::ptr_eq(x, y) => Some(0),
        (Value::Object(_), Value::Object(_)) => Some(1),
        _ => None,
    }
}

/// curl_cast_object(): (int) gives the object handle.
fn curl_cast(o: &Rc<RefCell<PhpObject>>, ty: u8) -> Option<Value> {
    if ty == 1 {
        Some(Value::Long(o.borrow().object_id as i64))
    } else {
        None
    }
}

static CURL_HANDLERS: NativeObjectHandlers = NativeObjectHandlers {
    compare: Some(not_comparable),
    cast: Some(curl_cast),
    clone_obj: Some(curl_clone_obj),
    ..NativeObjectHandlers::EMPTY
};

pub(crate) static MULTI_HANDLERS: NativeObjectHandlers = NativeObjectHandlers {
    compare: Some(not_comparable),
    cast: Some(curl_cast),
    ..NativeObjectHandlers::EMPTY
};

pub(crate) static SHARE_HANDLERS: NativeObjectHandlers = NativeObjectHandlers {
    compare: Some(not_comparable),
    ..NativeObjectHandlers::EMPTY
};

pub(crate) fn curl_of(o: &Obj) -> Option<Rc<CurlObj>> {
    let d = o.borrow().native.as_ref().map(|n| n.data.clone())?;
    d.downcast::<CurlObj>().ok()
}

fn new_curl_object(vm: &mut Vm, easy: Easy, st: CurlState) -> (Value, Rc<CurlObj>) {
    let id = vm.next_object_id();
    let mut obj = PhpObject::new(b"CurlHandle".to_vec(), id);
    let c = Rc::new(CurlObj {
        easy: RefCell::new(Some(easy)),
        st: RefCell::new(st),
    });
    obj.native = Some(NativeData {
        data: c.clone() as Rc<dyn std::any::Any>,
        handlers: &CURL_HANDLERS,
    });
    (Value::Object(Rc::new(RefCell::new(obj))), c)
}

/// Run `f` on the easy handle (no-op when a transfer holds it).
pub(crate) fn with_easy<R>(c: &CurlObj, f: impl FnOnce(&mut Easy) -> R) -> Option<R> {
    c.easy.borrow_mut().as_mut().map(f)
}

fn setopt_raw(c: &CurlObj, option: i64, v: OptVal) -> u32 {
    with_easy(c, |e| e.setopt(option as u32, v)).unwrap_or(0)
}

/// _php_curl_set_default_options()
fn set_default_options(vm: &Vm, c: &CurlObj) {
    setopt_raw(c, CURLOPT_NOPROGRESS, OptVal::Long(1));
    setopt_raw(c, CURLOPT_VERBOSE, OptVal::Long(0));
    setopt_raw(c, CURLOPT_WRITEFUNCTION, OptVal::Callback(true));
    setopt_raw(c, CURLOPT_READFUNCTION, OptVal::Callback(true));
    setopt_raw(c, CURLOPT_HEADERFUNCTION, OptVal::Callback(true));
    setopt_raw(c, CURLOPT_DNS_CACHE_TIMEOUT, OptVal::Long(120));
    setopt_raw(c, CURLOPT_MAXREDIRS, OptVal::Long(20));
    let ini = |k: &[u8]| {
        vm.constants
            .get(k)
            .map(|v| v.to_php_string().as_bytes().to_vec())
            .unwrap_or_default()
    };
    let mut cainfo = ini(b"openssl.cafile");
    if cainfo.is_empty() {
        cainfo = ini(b"curl.cainfo");
    }
    if !cainfo.is_empty() {
        setopt_raw(c, CURLOPT_CAINFO, OptVal::Str(Some(&cainfo)));
    }
    let _ = CURLOPT_ERRORBUFFER;
}

fn new_state() -> CurlState {
    let mut st = CurlState::default();
    st.handlers.write.method = PHP_CURL_STDOUT;
    st.handlers.read.method = PHP_CURL_DIRECT;
    st.handlers.write_header.method = PHP_CURL_IGNORE;
    st
}

pub(crate) fn stream_id(v: &Option<Value>) -> Option<i64> {
    match v {
        Some(Value::Resource(id)) => Some(*id),
        _ => None,
    }
}

/// _php_curl_verify_handlers()
pub(crate) fn verify_handlers(vm: &mut Vm, c: &CurlObj, reporterror: bool) {
    let mut warn: Vec<&'static str> = Vec::new();
    {
        let mut st = c.st.borrow_mut();
        let h = &mut st.handlers;
        if let Some(id) = stream_id(&h.std_err) {
            if !streams::is_stream(id) {
                warn.push("CURLOPT_STDERR resource has gone away, resetting to stderr");
                h.std_err = None;
            }
        }
        if let Some(id) = stream_id(&h.read.stream) {
            if !streams::is_stream(id) {
                warn.push("CURLOPT_INFILE resource has gone away, resetting to default");
                h.read.stream = None;
            }
        }
        if let Some(id) = stream_id(&h.write_header.stream) {
            if !streams::is_stream(id) {
                warn.push("CURLOPT_WRITEHEADER resource has gone away, resetting to default");
                h.write_header.stream = None;
                h.write_header.method = PHP_CURL_IGNORE;
            }
        }
        if let Some(id) = stream_id(&h.write.stream) {
            if !streams::is_stream(id) {
                warn.push("CURLOPT_FILE resource has gone away, resetting to default");
                h.write.stream = None;
                h.write.method = PHP_CURL_STDOUT;
            }
        }
    }
    if reporterror {
        for w in warn {
            let f = vm.current_builtin_name().unwrap_or_default();
            vm.emit_warning(&format!("{}(): {}", f, w));
        }
    }
}

/// _php_curl_cleanup_handle()
pub(crate) fn cleanup_handle(c: &CurlObj) {
    let mut st = c.st.borrow_mut();
    st.handlers.write.buf.clear();
    st.handlers.write.buf_set = false;
    st.header_out = None;
    st.err_str.clear();
    st.err_no = 0;
}

/// The callbacks of a transfer (curl_write, curl_read, curl_write_header,
/// curl_progress / curl_xferinfo, curl_debug, ...).
pub(crate) struct Transfer<'a> {
    pub vm: &'a mut Vm,
    pub c: Rc<CurlObj>,
    pub this: Value,
}

/// zend_call_known_fcc(): through call_user_func(), which also handles
/// __call() trampolines and invokable objects.
pub(crate) fn call_php(vm: &mut Vm, cb: &Value, args: &[Value]) -> Result<Value, VmError> {
    match vm.functions.get(b"call_user_func".as_slice()).copied() {
        Some(f) => {
            let mut a = Vec::with_capacity(args.len() + 1);
            a.push(cb.clone());
            a.extend_from_slice(args);
            f(vm, &a)
        }
        None => vm.call_callback(cb, args),
    }
}

/// php_curl_get_long()
fn get_long(vm: &mut Vm, v: &Value) -> i64 {
    match v {
        Value::Long(n) => *n,
        other => zpp::zval_get_long(vm, other),
    }
}

impl Transfer<'_> {
    /// zend_call_known_fcc() with ch->in_callback set; None when the call
    /// did not complete (exception).
    fn call(&mut self, cb: &Value, args: &[Value]) -> Option<Value> {
        self.c.st.borrow_mut().in_callback = true;
        let r = call_php(self.vm, cb, args);
        self.c.st.borrow_mut().in_callback = false;
        match r {
            Ok(v) if self.vm.current_exception.is_none() => Some(v),
            _ => None,
        }
    }

    fn write_to_stream(&mut self, s: &Option<Value>, data: &[u8]) -> usize {
        match stream_id(s) {
            Some(id) => streams::write(self.vm, id, data),
            None => 0,
        }
    }
}

impl engine::Callbacks for Transfer<'_> {
    fn write(&mut self, data: &[u8]) -> usize {
        let (method, fcc, stream) = {
            let st = self.c.st.borrow();
            let w = &st.handlers.write;
            (w.method, w.fcc.clone(), w.stream.clone())
        };
        let mut length = data.len();
        match method {
            PHP_CURL_STDOUT => self.vm.write_output(data),
            PHP_CURL_FILE => return self.write_to_stream(&stream, data),
            PHP_CURL_RETURN => {
                if !data.is_empty() {
                    let mut st = self.c.st.borrow_mut();
                    st.handlers.write.buf.extend_from_slice(data);
                    st.handlers.write.buf_set = true;
                }
            }
            PHP_CURL_USER => {
                if let Some(cb) = fcc {
                    let args = [
                        self.this.clone(),
                        Value::String(PhpString::from_bytes(data)),
                    ];
                    if let Some(rv) = self.call(&cb, &args) {
                        verify_handlers(self.vm, &self.c, true);
                        length = get_long(self.vm, &rv) as usize;
                    }
                }
            }
            _ => {}
        }
        length
    }

    fn header(&mut self, data: &[u8]) -> usize {
        let (method, fcc, stream, wmethod) = {
            let st = self.c.st.borrow();
            let w = &st.handlers.write_header;
            (
                w.method,
                w.fcc.clone(),
                w.stream.clone(),
                st.handlers.write.method,
            )
        };
        let mut length = data.len();
        match method {
            PHP_CURL_STDOUT => {
                if wmethod == PHP_CURL_RETURN && !data.is_empty() {
                    let mut st = self.c.st.borrow_mut();
                    st.handlers.write.buf.extend_from_slice(data);
                    st.handlers.write.buf_set = true;
                } else {
                    self.vm.write_output(data);
                }
            }
            PHP_CURL_FILE => return self.write_to_stream(&stream, data),
            PHP_CURL_USER => {
                if let Some(cb) = fcc {
                    let args = [
                        self.this.clone(),
                        Value::String(PhpString::from_bytes(data)),
                    ];
                    if let Some(rv) = self.call(&cb, &args) {
                        verify_handlers(self.vm, &self.c, true);
                        length = get_long(self.vm, &rv) as usize;
                    }
                }
            }
            PHP_CURL_IGNORE => return length,
            _ => return usize::MAX,
        }
        length
    }

    fn read(&mut self, max: usize) -> ReadResult {
        let (method, fcc, stream) = {
            let st = self.c.st.borrow();
            let r = &st.handlers.read;
            (r.method, r.fcc.clone(), r.stream.clone())
        };
        match method {
            PHP_CURL_DIRECT => match stream_id(&stream) {
                Some(id) => ReadResult::Data(streams::read(self.vm, id, max).unwrap_or_default()),
                None => ReadResult::Data(Vec::new()),
            },
            PHP_CURL_USER => {
                let Some(cb) = fcc else {
                    return ReadResult::Data(Vec::new());
                };
                let args = [
                    self.this.clone(),
                    stream.clone().unwrap_or(Value::Null),
                    Value::Long(max as i64),
                ];
                match self.call(&cb, &args) {
                    Some(rv) => {
                        verify_handlers(self.vm, &self.c, true);
                        match rv {
                            Value::String(s) => {
                                let b = s.as_bytes();
                                ReadResult::Data(b[..b.len().min(max)].to_vec())
                            }
                            Value::Long(CURL_READFUNC_PAUSE_V) => ReadResult::Pause,
                            Value::Long(CURL_READFUNC_ABORT_V) => ReadResult::Abort,
                            Value::Long(n) if n > 0 => {
                                ReadResult::Data(vec![0u8; (n as usize).min(max)])
                            }
                            _ => ReadResult::Data(Vec::new()),
                        }
                    }
                    None => ReadResult::Data(Vec::new()),
                }
            }
            _ => ReadResult::Data(Vec::new()),
        }
    }

    fn progress(&mut self, dltotal: i64, dlnow: i64, ultotal: i64, ulnow: i64) -> i64 {
        let (x, p) = {
            let st = self.c.st.borrow();
            (st.handlers.xferinfo.clone(), st.handlers.progress.clone())
        };
        let Some(cb) = x.or(p) else {
            return 0;
        };
        let args = [
            self.this.clone(),
            Value::Long(dltotal),
            Value::Long(dlnow),
            Value::Long(ultotal),
            Value::Long(ulnow),
        ];
        match self.call(&cb, &args) {
            Some(rv) => {
                verify_handlers(self.vm, &self.c, true);
                if get_long(self.vm, &rv) != 0 { 1 } else { 0 }
            }
            None => 0,
        }
    }

    fn debug(&mut self, kind: u32, data: &[u8]) {
        if kind as i64 == CURLINFO_HEADER_OUT {
            self.c.st.borrow_mut().header_out = Some(data.to_vec());
        }
        let cb = self.c.st.borrow().handlers.debug.clone();
        if let Some(cb) = cb {
            let args = [
                self.this.clone(),
                Value::Long(kind as i64),
                Value::String(PhpString::from_bytes(data)),
            ];
            let _ = self.call(&cb, &args);
        }
    }

    fn stderr(&mut self, data: &[u8]) {
        let s = self.c.st.borrow().handlers.std_err.clone();
        match stream_id(&s) {
            Some(id) => {
                streams::write(self.vm, id, data);
            }
            None => {
                use std::io::Write;
                self.vm.flush_cli_output();
                let _ = std::io::stderr().write_all(data);
            }
        }
    }

    fn prereq(&mut self, primary_ip: &str, local_ip: &str, pport: i32, lport: i32) -> i32 {
        let cb = self.c.st.borrow().handlers.prereq.clone();
        let Some(cb) = cb else {
            return 0;
        };
        let args = [
            self.this.clone(),
            Value::String(PhpString::from_string(primary_ip.to_string())),
            Value::String(PhpString::from_string(local_ip.to_string())),
            Value::Long(pport as i64),
            Value::Long(lport as i64),
        ];
        let mut rval = 0;
        if let Some(rv) = self.call(&cb, &args) {
            verify_handlers(self.vm, &self.c, true);
            match rv {
                Value::Long(n) if n == CURL_PREREQFUNC_OK || n == CURL_PREREQFUNC_ABORT => {
                    rval = n as i32
                }
                Value::Long(_) => zpp::plain_value_error(
                    self.vm,
                    "The CURLOPT_PREREQFUNCTION callback must return either CURL_PREREQFUNC_OK or CURL_PREREQFUNC_ABORT",
                ),
                _ => zpp::throw(
                    self.vm,
                    b"TypeError",
                    "The CURLOPT_PREREQFUNCTION callback must return either CURL_PREREQFUNC_OK or CURL_PREREQFUNC_ABORT",
                ),
            }
        }
        rval
    }

    fn fnmatch(&mut self, pattern: &[u8], string: &[u8]) -> i32 {
        let cb = self.c.st.borrow().handlers.fnmatch.clone();
        let Some(cb) = cb else {
            return CURL_FNMATCHFUNC_FAIL as i32;
        };
        let args = [
            self.this.clone(),
            Value::String(PhpString::from_bytes(pattern)),
            Value::String(PhpString::from_bytes(string)),
        ];
        match self.call(&cb, &args) {
            Some(rv) => {
                verify_handlers(self.vm, &self.c, true);
                get_long(self.vm, &rv) as i32
            }
            None => CURL_FNMATCHFUNC_FAIL as i32,
        }
    }

    fn ssh_hostkey(&mut self, keytype: i32, key: &[u8]) -> i32 {
        let cb = self.c.st.borrow().handlers.sshhostkey.clone();
        let mut rval = CURLKHMATCH_MISMATCH as i32;
        let Some(cb) = cb else {
            return rval;
        };
        let args = [
            self.this.clone(),
            Value::Long(keytype as i64),
            Value::String(PhpString::from_bytes(key)),
            Value::Long(key.len() as i64),
        ];
        if let Some(rv) = self.call(&cb, &args) {
            verify_handlers(self.vm, &self.c, true);
            match rv {
                Value::Long(n) if n == CURLKHMATCH_OK || n == CURLKHMATCH_MISMATCH => {
                    rval = n as i32
                }
                _ => zpp::throw(
                    self.vm,
                    b"Error",
                    "The CURLOPT_SSH_HOSTKEYFUNCTION callback must return either CURLKHMATCH_OK or CURLKHMATCH_MISMATCH",
                ),
            }
        }
        rval
    }
}

/// curl_easy_perform() of a handle with the PHP callbacks; records the
/// error (SAVE_CURL_ERROR and the error buffer).
pub(crate) fn perform(vm: &mut Vm, this: &Value, c: &Rc<CurlObj>) -> u32 {
    let Some(mut easy) = c.easy.borrow_mut().take() else {
        // curl_easy_perform() from one of its own callbacks
        return CURLE_RECURSIVE_API_CALL as u32;
    };
    let code = {
        let mut t = Transfer {
            vm,
            c: c.clone(),
            this: this.clone(),
        };
        easy.perform(&mut t)
    };
    let err = easy.error_buffer().to_vec();
    *c.easy.borrow_mut() = Some(easy);
    let mut st = c.st.borrow_mut();
    st.err_no = code as i64;
    st.err_str = err;
    code
}

fn arg_curl(a: &Arg) -> (Value, Rc<CurlObj>) {
    let o = a.obj();
    let c = curl_of(&o).unwrap_or_else(|| {
        Rc::new(CurlObj {
            easy: RefCell::new(Some(Easy::new())),
            st: RefCell::new(new_state()),
        })
    });
    (Value::Object(o), c)
}

fn vstr(b: &[u8]) -> Value {
    Value::String(PhpString::from_bytes(b))
}

fn key(s: &str) -> ArrayKey {
    ArrayKey::String(PhpString::from_string(s.to_string()))
}

fn new_arr(a: PhpArray) -> Value {
    Value::Array(Rc::new(RefCell::new(a)))
}

// ---------------------------------------------------------------------------
// curl_version()

const FEATURES: &[(&str, i64)] = &[
    ("AsynchDNS", CURL_VERSION_ASYNCHDNS),
    ("CharConv", CURL_VERSION_CONV),
    ("Debug", CURL_VERSION_DEBUG),
    ("GSS-Negotiate", CURL_VERSION_GSSNEGOTIATE),
    ("IDN", CURL_VERSION_IDN),
    ("IPv6", CURL_VERSION_IPV6),
    ("krb4", CURL_VERSION_KERBEROS4),
    ("Largefile", CURL_VERSION_LARGEFILE),
    ("libz", CURL_VERSION_LIBZ),
    ("NTLM", CURL_VERSION_NTLM),
    ("NTLMWB", CURL_VERSION_NTLM_WB),
    ("SPNEGO", CURL_VERSION_SPNEGO),
    ("SSL", CURL_VERSION_SSL),
    ("SSPI", CURL_VERSION_SSPI),
    ("TLS-SRP", CURL_VERSION_TLSAUTH_SRP),
    ("HTTP2", CURL_VERSION_HTTP2),
    ("GSSAPI", CURL_VERSION_GSSAPI),
    ("KERBEROS5", CURL_VERSION_KERBEROS5),
    ("UNIX_SOCKETS", CURL_VERSION_UNIX_SOCKETS),
    ("PSL", CURL_VERSION_PSL),
    ("HTTPS_PROXY", CURL_VERSION_HTTPS_PROXY),
    ("MULTI_SSL", CURL_VERSION_MULTI_SSL),
    ("BROTLI", CURL_VERSION_BROTLI),
    ("ALTSVC", CURL_VERSION_ALTSVC),
    ("HTTP3", CURL_VERSION_HTTP3),
    ("UNICODE", CURL_VERSION_UNICODE),
    ("ZSTD", CURL_VERSION_ZSTD),
    ("HSTS", CURL_VERSION_HSTS),
    ("GSASL", CURL_VERSION_GSASL),
];

fn curl_version(vm: &mut Vm, f: &str, args: &[Value]) -> Value {
    if zpp::parse(vm, f, "", args).is_err() {
        return Value::Null;
    }
    let d = engine::version_info();
    let mut a = PhpArray::new();
    a.set(key("version_number"), Value::Long(d.version_num));
    a.set(key("age"), Value::Long(d.age));
    a.set(key("features"), Value::Long(d.features));
    let mut fl = PhpArray::new();
    for (name, bit) in FEATURES {
        fl.set(
            key(name),
            if d.features & bit != 0 {
                Value::True
            } else {
                Value::False
            },
        );
    }
    a.set(key("feature_list"), new_arr(fl));
    a.set(key("ssl_version_number"), Value::Long(d.ssl_version_num));
    a.set(key("version"), vstr(d.version.as_bytes()));
    a.set(key("host"), vstr(d.host.as_bytes()));
    a.set(key("ssl_version"), vstr(d.ssl_version.as_bytes()));
    a.set(key("libz_version"), vstr(d.libz_version.as_bytes()));
    let mut pl = PhpArray::new();
    for p in &d.protocols {
        pl.push(vstr(p.as_bytes()));
    }
    a.set(key("protocols"), new_arr(pl));
    let os = |s: &Option<String>| vstr(s.as_deref().unwrap_or("").as_bytes());
    if d.age >= 1 {
        a.set(key("ares"), os(&d.ares));
        a.set(key("ares_num"), Value::Long(d.ares_num));
    }
    if d.age >= 2 {
        a.set(key("libidn"), os(&d.libidn));
    }
    if d.age >= 3 {
        a.set(key("iconv_ver_num"), Value::Long(d.iconv_ver_num));
        a.set(key("libssh_version"), os(&d.libssh_version));
    }
    if d.age >= 4 {
        a.set(key("brotli_ver_num"), Value::Long(d.brotli_ver_num));
        a.set(key("brotli_version"), os(&d.brotli_version));
    }
    new_arr(a)
}

// ---------------------------------------------------------------------------
// curl_init(), curl_copy_handle(), clone

fn curl_init(vm: &mut Vm, f: &str, args: &[Value]) -> Value {
    let a = match zpp::parse(vm, f, "|s!", args) {
        Ok(a) => a,
        Err(()) => return Value::Null,
    };
    let (ret, c) = new_curl_object(vm, Easy::new(), new_state());
    set_default_options(vm, &c);
    if a[0].is_given() {
        let url = a[0].str().to_vec();
        if option_url(vm, f, &c, &url).is_err() {
            return Value::False;
        }
    }
    ret
}

/// _php_setup_easy_copy_handlers() + curl_easy_duphandle()
fn dup_curl(src: &CurlObj) -> (Easy, CurlState) {
    let easy = src
        .easy
        .borrow()
        .as_ref()
        .map(|e| e.duphandle())
        .unwrap_or_default();
    let s = src.st.borrow();
    let mut st = new_state();
    st.handlers = s.handlers.clone();
    st.handlers.write.buf.clear();
    st.handlers.write.buf_set = false;
    st.private_data = s.private_data.clone();
    st.share = s.share.clone();
    (easy, st)
}

fn curl_clone_obj(vm: &mut Vm, src: &Rc<RefCell<PhpObject>>, clone: &Rc<RefCell<PhpObject>>) {
    let Some(c) = curl_of(src) else {
        return;
    };
    let (easy, st) = dup_curl(&c);
    let postfields = c.st.borrow().postfields.clone();
    let nc = Rc::new(CurlObj {
        easy: RefCell::new(Some(easy)),
        st: RefCell::new(st),
    });
    clone.borrow_mut().native = Some(NativeData {
        data: nc.clone() as Rc<dyn std::any::Any>,
        handlers: &CURL_HANDLERS,
    });
    if let Some(Value::Array(pf)) = postfields {
        let this = Value::Object(clone.clone());
        if build_mime(vm, "curl_copy_handle", &nc, &this, &pf).is_err() && !vm_has_exc(vm) {
            zpp::throw(vm, b"Exception", "Failed to clone CurlHandle");
        }
    }
}

/// instanceof_function() against a (lower-case) class name
fn is_a(vm: &Vm, o: &Obj, target: &[u8]) -> bool {
    let cn = o.borrow().class_name.to_ascii_lowercase();
    cn == target || vm.class_extends(&cn, target)
}

fn vm_has_exc(vm: &Vm) -> bool {
    vm.current_exception.is_some()
}

fn curl_copy_handle(vm: &mut Vm, f: &str, args: &[Value]) -> Value {
    let a = match zpp::parse(vm, f, "C", args) {
        Ok(a) => a,
        Err(()) => return Value::Null,
    };
    let (_, c) = arg_curl(&a[0]);
    let (easy, st) = dup_curl(&c);
    let postfields = c.st.borrow().postfields.clone();
    let (ret, nc) = new_curl_object(vm, easy, st);
    if let Some(Value::Array(pf)) = postfields {
        if build_mime(vm, f, &nc, &ret, &pf).is_err() {
            vm.current_exception = None;
            vm.emit_warning("curl_copy_handle(): Cannot duplicate cURL handle");
            return Value::False;
        }
    }
    ret
}

// ---------------------------------------------------------------------------
// curl_setopt()

type SetoptResult = Result<(), ()>;

fn save_error(c: &CurlObj, code: u32) {
    c.st.borrow_mut().err_no = code as i64;
}

fn open_basedir_set(vm: &Vm) -> bool {
    !goro_core::basedir::open_basedir_value(vm).is_empty()
}

/// php_curl_option_str()
fn option_str(vm: &mut Vm, f: &str, c: &CurlObj, option: i64, s: &[u8]) -> SetoptResult {
    if s.contains(&0) {
        zpp::plain_value_error(
            vm,
            &format!("{}(): cURL option must not contain any null bytes", f),
        );
        return Err(());
    }
    let code = setopt_raw(c, option, OptVal::Str(Some(s)));
    save_error(c, code);
    if code == 0 { Ok(()) } else { Err(()) }
}

/// php_curl_option_url()
fn option_url(vm: &mut Vm, f: &str, c: &CurlObj, url: &[u8]) -> SetoptResult {
    if open_basedir_set(vm) {
        setopt_raw(
            c,
            CURLOPT_PROTOCOLS,
            OptVal::Long(CURLPROTO_ALL & !CURLPROTO_FILE),
        );
    }
    option_str(vm, f, c, CURLOPT_URL, url)
}

const LONG_OPTIONS: &[i64] = &[
    CURLOPT_AUTOREFERER,
    CURLOPT_BUFFERSIZE,
    CURLOPT_CONNECTTIMEOUT,
    CURLOPT_COOKIESESSION,
    CURLOPT_CRLF,
    CURLOPT_DNS_CACHE_TIMEOUT,
    CURLOPT_FAILONERROR,
    CURLOPT_FILETIME,
    CURLOPT_FORBID_REUSE,
    CURLOPT_FRESH_CONNECT,
    CURLOPT_FTP_USE_EPRT,
    CURLOPT_FTP_USE_EPSV,
    CURLOPT_HEADER,
    CURLOPT_HTTPGET,
    CURLOPT_HTTPPROXYTUNNEL,
    CURLOPT_HTTP_VERSION,
    CURLOPT_INFILESIZE,
    CURLOPT_LOW_SPEED_LIMIT,
    CURLOPT_LOW_SPEED_TIME,
    CURLOPT_MAXCONNECTS,
    CURLOPT_MAXREDIRS,
    CURLOPT_NETRC,
    CURLOPT_NOBODY,
    CURLOPT_NOPROGRESS,
    CURLOPT_NOSIGNAL,
    CURLOPT_PORT,
    CURLOPT_POST,
    CURLOPT_PROXYPORT,
    CURLOPT_PROXYTYPE,
    CURLOPT_PUT,
    CURLOPT_RESUME_FROM,
    CURLOPT_SSLVERSION,
    CURLOPT_SSL_VERIFYPEER,
    CURLOPT_TIMECONDITION,
    CURLOPT_TIMEOUT,
    CURLOPT_TIMEVALUE,
    CURLOPT_TRANSFERTEXT,
    CURLOPT_UNRESTRICTED_AUTH,
    CURLOPT_UPLOAD,
    CURLOPT_VERBOSE,
    CURLOPT_HTTPAUTH,
    CURLOPT_FTP_CREATE_MISSING_DIRS,
    CURLOPT_PROXYAUTH,
    CURLOPT_SERVER_RESPONSE_TIMEOUT,
    CURLOPT_IPRESOLVE,
    CURLOPT_MAXFILESIZE,
    CURLOPT_TCP_NODELAY,
    CURLOPT_FTPSSLAUTH,
    CURLOPT_IGNORE_CONTENT_LENGTH,
    CURLOPT_FTP_SKIP_PASV_IP,
    CURLOPT_FTP_FILEMETHOD,
    CURLOPT_CONNECT_ONLY,
    CURLOPT_LOCALPORT,
    CURLOPT_LOCALPORTRANGE,
    CURLOPT_SSL_SESSIONID_CACHE,
    CURLOPT_FTP_SSL_CCC,
    CURLOPT_SSH_AUTH_TYPES,
    CURLOPT_CONNECTTIMEOUT_MS,
    CURLOPT_HTTP_CONTENT_DECODING,
    CURLOPT_HTTP_TRANSFER_DECODING,
    CURLOPT_TIMEOUT_MS,
    CURLOPT_NEW_DIRECTORY_PERMS,
    CURLOPT_NEW_FILE_PERMS,
    CURLOPT_USE_SSL,
    CURLOPT_APPEND,
    CURLOPT_DIRLISTONLY,
    CURLOPT_PROXY_TRANSFER_MODE,
    CURLOPT_ADDRESS_SCOPE,
    CURLOPT_CERTINFO,
    CURLOPT_PROTOCOLS,
    CURLOPT_REDIR_PROTOCOLS,
    CURLOPT_SOCKS5_GSSAPI_NEC,
    CURLOPT_TFTP_BLKSIZE,
    CURLOPT_FTP_USE_PRET,
    CURLOPT_RTSP_CLIENT_CSEQ,
    CURLOPT_RTSP_REQUEST,
    CURLOPT_RTSP_SERVER_CSEQ,
    CURLOPT_WILDCARDMATCH,
    CURLOPT_GSSAPI_DELEGATION,
    CURLOPT_ACCEPTTIMEOUT_MS,
    CURLOPT_SSL_OPTIONS,
    CURLOPT_TCP_KEEPALIVE,
    CURLOPT_TCP_KEEPIDLE,
    CURLOPT_TCP_KEEPINTVL,
    CURLOPT_SASL_IR,
    CURLOPT_EXPECT_100_TIMEOUT_MS,
    CURLOPT_SSL_ENABLE_ALPN,
    CURLOPT_SSL_ENABLE_NPN,
    CURLOPT_HEADEROPT,
    CURLOPT_SSL_VERIFYSTATUS,
    CURLOPT_PATH_AS_IS,
    CURLOPT_SSL_FALSESTART,
    CURLOPT_PIPEWAIT,
    CURLOPT_STREAM_WEIGHT,
    CURLOPT_TFTP_NO_OPTIONS,
    CURLOPT_TCP_FASTOPEN,
    CURLOPT_KEEP_SENDING_ON_ERROR,
    CURLOPT_PROXY_SSL_OPTIONS,
    CURLOPT_PROXY_SSL_VERIFYHOST,
    CURLOPT_PROXY_SSL_VERIFYPEER,
    CURLOPT_PROXY_SSLVERSION,
    CURLOPT_SUPPRESS_CONNECT_HEADERS,
    CURLOPT_SOCKS5_AUTH,
    CURLOPT_SSH_COMPRESSION,
    CURLOPT_HAPPY_EYEBALLS_TIMEOUT_MS,
    CURLOPT_DNS_SHUFFLE_ADDRESSES,
    CURLOPT_HAPROXYPROTOCOL,
    CURLOPT_DISALLOW_USERNAME_IN_URL,
    CURLOPT_UPKEEP_INTERVAL_MS,
    CURLOPT_UPLOAD_BUFFERSIZE,
    CURLOPT_HTTP09_ALLOWED,
    CURLOPT_ALTSVC_CTRL,
    CURLOPT_MAXAGE_CONN,
    CURLOPT_MAIL_RCPT_ALLLOWFAILS,
    CURLOPT_HSTS_CTRL,
    CURLOPT_DOH_SSL_VERIFYHOST,
    CURLOPT_DOH_SSL_VERIFYPEER,
    CURLOPT_DOH_SSL_VERIFYSTATUS,
    CURLOPT_MAXLIFETIME_CONN,
    CURLOPT_MIME_OPTIONS,
    CURLOPT_WS_OPTIONS,
    CURLOPT_CA_CACHE_TIMEOUT,
    CURLOPT_QUICK_EXIT,
    CURLOPT_TCP_KEEPCNT,
    CURLOPT_FOLLOWLOCATION,
];

const STRING_OPTIONS: &[i64] = &[
    CURLOPT_CAINFO,
    CURLOPT_CAPATH,
    CURLOPT_COOKIE,
    CURLOPT_EGDSOCKET,
    CURLOPT_INTERFACE,
    CURLOPT_PROXY,
    CURLOPT_PROXYUSERPWD,
    CURLOPT_REFERER,
    CURLOPT_SSLCERTTYPE,
    CURLOPT_SSLENGINE,
    CURLOPT_SSLENGINE_DEFAULT,
    CURLOPT_SSLKEY,
    CURLOPT_SSLKEYPASSWD,
    CURLOPT_SSLKEYTYPE,
    CURLOPT_SSL_CIPHER_LIST,
    CURLOPT_USERAGENT,
    CURLOPT_COOKIELIST,
    CURLOPT_FTP_ALTERNATIVE_TO_USER,
    CURLOPT_SSH_HOST_PUBLIC_KEY_MD5,
    CURLOPT_PROXYPASSWORD,
    CURLOPT_PROXYUSERNAME,
    CURLOPT_NOPROXY,
    CURLOPT_SOCKS5_GSSAPI_SERVICE,
    CURLOPT_MAIL_FROM,
    CURLOPT_RTSP_STREAM_URI,
    CURLOPT_RTSP_TRANSPORT,
    CURLOPT_TLSAUTH_TYPE,
    CURLOPT_TLSAUTH_PASSWORD,
    CURLOPT_TLSAUTH_USERNAME,
    CURLOPT_TRANSFER_ENCODING,
    CURLOPT_DNS_SERVERS,
    CURLOPT_MAIL_AUTH,
    CURLOPT_LOGIN_OPTIONS,
    CURLOPT_PINNEDPUBLICKEY,
    CURLOPT_PROXY_SERVICE_NAME,
    CURLOPT_SERVICE_NAME,
    CURLOPT_DEFAULT_PROTOCOL,
    CURLOPT_PRE_PROXY,
    CURLOPT_PROXY_CAINFO,
    CURLOPT_PROXY_CAPATH,
    CURLOPT_PROXY_CRLFILE,
    CURLOPT_PROXY_KEYPASSWD,
    CURLOPT_PROXY_PINNEDPUBLICKEY,
    CURLOPT_PROXY_SSL_CIPHER_LIST,
    CURLOPT_PROXY_SSLCERT,
    CURLOPT_PROXY_SSLCERTTYPE,
    CURLOPT_PROXY_SSLKEY,
    CURLOPT_PROXY_SSLKEYTYPE,
    CURLOPT_PROXY_TLSAUTH_PASSWORD,
    CURLOPT_PROXY_TLSAUTH_TYPE,
    CURLOPT_PROXY_TLSAUTH_USERNAME,
    CURLOPT_ABSTRACT_UNIX_SOCKET,
    CURLOPT_REQUEST_TARGET,
    CURLOPT_PROXY_TLS13_CIPHERS,
    CURLOPT_TLS13_CIPHERS,
    CURLOPT_ALTSVC,
    CURLOPT_SASL_AUTHZID,
    CURLOPT_PROXY_ISSUERCERT,
    CURLOPT_SSL_EC_CURVES,
    CURLOPT_AWS_SIGV4,
    CURLOPT_SSH_HOST_PUBLIC_KEY_SHA256,
    CURLOPT_PROTOCOLS_STR,
    CURLOPT_REDIR_PROTOCOLS_STR,
];

const NULLABLE_STRING_OPTIONS: &[i64] = &[
    CURLOPT_CUSTOMREQUEST,
    CURLOPT_FTPPORT,
    CURLOPT_RANGE,
    CURLOPT_FTP_ACCOUNT,
    CURLOPT_RTSP_SESSION_ID,
    CURLOPT_ACCEPT_ENCODING,
    CURLOPT_DNS_INTERFACE,
    CURLOPT_DNS_LOCAL_IP4,
    CURLOPT_DNS_LOCAL_IP6,
    CURLOPT_XOAUTH2_BEARER,
    CURLOPT_UNIX_SOCKET_PATH,
    CURLOPT_DOH_URL,
    CURLOPT_HSTS,
    CURLOPT_KRBLEVEL,
    CURLOPT_USERPWD,
    CURLOPT_USERNAME,
    CURLOPT_PASSWORD,
    CURLOPT_SSL_SIGNATURE_ALGORITHMS,
];

const SLIST_OPTIONS: &[(i64, &str)] = &[
    (CURLOPT_HTTP200ALIASES, "CURLOPT_HTTP200ALIASES"),
    (CURLOPT_HTTPHEADER, "CURLOPT_HTTPHEADER"),
    (CURLOPT_POSTQUOTE, "CURLOPT_POSTQUOTE"),
    (CURLOPT_PREQUOTE, "CURLOPT_PREQUOTE"),
    (CURLOPT_QUOTE, "CURLOPT_QUOTE"),
    (CURLOPT_TELNETOPTIONS, "CURLOPT_TELNETOPTIONS"),
    (CURLOPT_MAIL_RCPT, "CURLOPT_MAIL_RCPT"),
    (CURLOPT_RESOLVE, "CURLOPT_RESOLVE"),
    (CURLOPT_PROXYHEADER, "CURLOPT_PROXYHEADER"),
    (CURLOPT_CONNECT_TO, "CURLOPT_CONNECT_TO"),
];

const OFF_T_OPTIONS: &[i64] = &[
    CURLOPT_INFILESIZE_LARGE,
    CURLOPT_MAX_RECV_SPEED_LARGE,
    CURLOPT_MAX_SEND_SPEED_LARGE,
    CURLOPT_MAXFILESIZE_LARGE,
    CURLOPT_TIMEVALUE_LARGE,
];

const FILE_PATH_OPTIONS: &[i64] = &[
    CURLOPT_COOKIEFILE,
    CURLOPT_COOKIEJAR,
    CURLOPT_RANDOM_FILE,
    CURLOPT_SSLCERT,
    CURLOPT_NETRC_FILE,
    CURLOPT_SSH_PRIVATE_KEYFILE,
    CURLOPT_SSH_PUBLIC_KEYFILE,
    CURLOPT_CRLFILE,
    CURLOPT_ISSUERCERT,
    CURLOPT_SSH_KNOWNHOSTS,
];

const BLOB_OPTIONS: &[i64] = &[
    CURLOPT_ISSUERCERT_BLOB,
    CURLOPT_PROXY_ISSUERCERT_BLOB,
    CURLOPT_PROXY_SSLCERT_BLOB,
    CURLOPT_PROXY_SSLKEY_BLOB,
    CURLOPT_SSLCERT_BLOB,
    CURLOPT_SSLKEY_BLOB,
    CURLOPT_CAINFO_BLOB,
    CURLOPT_PROXY_CAINFO_BLOB,
];

/// php_curl_set_callable_handler()
fn set_callable(
    vm: &mut Vm,
    f: &str,
    slot: &mut Option<Value>,
    v: &Value,
    is_array_config: bool,
    option_name: &str,
) -> bool {
    *slot = None;
    if matches!(v, Value::Null) {
        return true;
    }
    if let Err(e) = streams::check_callable(vm, v) {
        if !vm_has_exc(vm) {
            zpp::arg_type_error(
                vm,
                f,
                if is_array_config { 2 } else { 3 },
                &format!("must be a valid callback for option {}, {}", option_name, e),
            );
        }
        return false;
    }
    *slot = Some(v.clone());
    true
}

/// Which handler slot a callable option fills.
fn callable_slot(h: &mut Handlers, option: i64) -> Option<(&mut Option<Value>, &'static str)> {
    Some(match option {
        CURLOPT_PROGRESSFUNCTION => (&mut h.progress, "CURLOPT_PROGRESSFUNCTION"),
        CURLOPT_XFERINFOFUNCTION => (&mut h.xferinfo, "CURLOPT_XFERINFOFUNCTION"),
        CURLOPT_FNMATCH_FUNCTION => (&mut h.fnmatch, "CURLOPT_FNMATCH_FUNCTION"),
        CURLOPT_DEBUGFUNCTION => (&mut h.debug, "CURLOPT_DEBUGFUNCTION"),
        CURLOPT_PREREQFUNCTION => (&mut h.prereq, "CURLOPT_PREREQFUNCTION"),
        CURLOPT_SSH_HOSTKEYFUNCTION => (&mut h.sshhostkey, "CURLOPT_SSH_HOSTKEYFUNCTION"),
        _ => return None,
    })
}

/// The PHP stream behind a File-Handle option value.
fn fetch_file_handle(vm: &mut Vm, v: &Value) -> Result<i64, ()> {
    match v {
        Value::Resource(id) if streams::is_stream(*id) => Ok(*id),
        Value::Resource(_) => {
            let f = vm.current_builtin_name().unwrap_or_default();
            zpp::throw(
                vm,
                b"TypeError",
                &format!(
                    "{}(): supplied resource is not a valid File-Handle resource",
                    f
                ),
            );
            Err(())
        }
        _ => {
            let f = vm.current_builtin_name().unwrap_or_default();
            zpp::throw(
                vm,
                b"TypeError",
                &format!(
                    "{}(): supplied argument is not a valid File-Handle resource",
                    f
                ),
            );
            Err(())
        }
    }
}

fn writable_mode(fid: i64) -> bool {
    let m = streams::mode(fid).unwrap_or_default();
    let b = m.as_bytes();
    b.first() != Some(&b'r') || b.get(1) == Some(&b'+')
}

/// _php_curl_setopt()
pub(crate) fn php_curl_setopt(
    vm: &mut Vm,
    f: &str,
    this: &Value,
    c: &Rc<CurlObj>,
    option: i64,
    zvalue: &Value,
    is_array_config: bool,
) -> SetoptResult {
    let zvalue = &zvalue.deref();
    let mut error: u32 = 0;
    match option {
        // Callable options with a PHP_CURL_USER handler method
        CURLOPT_WRITEFUNCTION | CURLOPT_HEADERFUNCTION | CURLOPT_READFUNCTION => {
            let (name, default) = match option {
                CURLOPT_WRITEFUNCTION => ("CURLOPT_WRITEFUNCTION", PHP_CURL_STDOUT),
                CURLOPT_HEADERFUNCTION => ("CURLOPT_HEADERFUNCTION", PHP_CURL_IGNORE),
                _ => ("CURLOPT_READFUNCTION", PHP_CURL_DIRECT),
            };
            let mut slot = None;
            let ok = set_callable(vm, f, &mut slot, zvalue, is_array_config, name);
            let mut st = c.st.borrow_mut();
            let set = slot.is_some();
            let method = match option {
                CURLOPT_WRITEFUNCTION => {
                    st.handlers.write.fcc = slot;
                    &mut st.handlers.write.method
                }
                CURLOPT_HEADERFUNCTION => {
                    st.handlers.write_header.fcc = slot;
                    &mut st.handlers.write_header.method
                }
                _ => {
                    st.handlers.read.fcc = slot;
                    &mut st.handlers.read.method
                }
            };
            if !ok {
                *method = default;
                return Err(());
            }
            if !set {
                *method = default;
                return Ok(());
            }
            *method = PHP_CURL_USER;
        }
        CURLOPT_PROGRESSFUNCTION
        | CURLOPT_XFERINFOFUNCTION
        | CURLOPT_FNMATCH_FUNCTION
        | CURLOPT_DEBUGFUNCTION
        | CURLOPT_PREREQFUNCTION
        | CURLOPT_SSH_HOSTKEYFUNCTION => {
            let mut slot = None;
            let name = {
                let mut st = c.st.borrow_mut();
                let (s, name) = callable_slot(&mut st.handlers, option).unwrap();
                *s = None;
                name
            };
            if !set_callable(vm, f, &mut slot, zvalue, is_array_config, name) {
                return Err(());
            }
            {
                let mut st = c.st.borrow_mut();
                let (s, _) = callable_slot(&mut st.handlers, option).unwrap();
                *s = slot;
            }
            let engine_opt = match option {
                CURLOPT_PROGRESSFUNCTION => CURLOPT_PROGRESSFUNCTION_C,
                CURLOPT_XFERINFOFUNCTION => CURLOPT_XFERINFOFUNCTION_C,
                o => o,
            };
            setopt_raw(c, engine_opt, OptVal::Callback(true));
        }
        CURLOPT_SSL_VERIFYHOST => {
            let lval = zpp::zval_get_long(vm, zvalue);
            if lval == 1 {
                vm.emit_notice_at(
                    &format!(
                        "{}(): CURLOPT_SSL_VERIFYHOST no longer accepts the value 1, value 2 will be used instead",
                        f
                    ),
                    vm.current_line,
                );
                error = setopt_raw(c, option, OptVal::Long(2));
            } else {
                error = setopt_raw(c, option, OptVal::Long(lval));
            }
        }
        o if LONG_OPTIONS.contains(&o) => {
            let lval = zpp::zval_get_long(vm, zvalue);
            if (o == CURLOPT_PROTOCOLS || o == CURLOPT_REDIR_PROTOCOLS)
                && open_basedir_set(vm)
                && (lval & CURLPROTO_FILE) != 0
            {
                vm.emit_warning(&format!(
                    "{}(): CURLPROTO_FILE cannot be activated when an open_basedir is set",
                    f
                ));
                return Err(());
            }
            error = setopt_raw(c, o, OptVal::Long(lval));
        }
        CURLOPT_SAFE_UPLOAD => {
            if !zvalue.is_truthy() {
                zpp::plain_value_error(
                    vm,
                    &format!("{}(): Disabling safe uploads is no longer supported", f),
                );
                return Err(());
            }
        }
        o if STRING_OPTIONS.contains(&o) => {
            let s = zpp::zval_get_string(vm, zvalue);
            if vm_has_exc(vm) {
                return Err(());
            }
            if (o == CURLOPT_PROTOCOLS_STR || o == CURLOPT_REDIR_PROTOCOLS_STR)
                && open_basedir_set(vm)
            {
                let l = s.to_ascii_lowercase();
                if l.windows(4).any(|w| w == b"file") || l.windows(3).any(|w| w == b"all") {
                    vm.emit_warning(&format!(
                        "{}(): The FILE protocol cannot be activated when an open_basedir is set",
                        f
                    ));
                    return Err(());
                }
            }
            return option_str(vm, f, c, o, &s);
        }
        o if NULLABLE_STRING_OPTIONS.contains(&o) => {
            if matches!(zvalue, Value::Null) {
                error = setopt_raw(c, o, OptVal::Str(None));
            } else {
                let s = zpp::zval_get_string(vm, zvalue);
                if vm_has_exc(vm) {
                    return Err(());
                }
                return option_str(vm, f, c, o, &s);
            }
        }
        CURLOPT_PRIVATE => {
            c.st.borrow_mut().private_data = Some(zvalue.clone());
            return Ok(());
        }
        CURLOPT_URL => {
            let s = zpp::zval_get_string(vm, zvalue);
            if vm_has_exc(vm) {
                return Err(());
            }
            return option_url(vm, f, c, &s);
        }
        CURLOPT_FILE | CURLOPT_INFILE | CURLOPT_STDERR | CURLOPT_WRITEHEADER => {
            let what = if matches!(zvalue, Value::Null) {
                None
            } else {
                Some(fetch_file_handle(vm, zvalue)?)
            };
            let mut st = c.st.borrow_mut();
            let h = &mut st.handlers;
            match option {
                CURLOPT_FILE => match what {
                    None => {
                        h.write.stream = None;
                        h.write.method = PHP_CURL_STDOUT;
                    }
                    Some(id) if writable_mode(id) => {
                        h.write.stream = Some(Value::Resource(id));
                        h.write.method = PHP_CURL_FILE;
                    }
                    Some(_) => {
                        drop(st);
                        zpp::plain_value_error(
                            vm,
                            &format!("{}(): The provided file handle must be writable", f),
                        );
                        return Err(());
                    }
                },
                CURLOPT_WRITEHEADER => match what {
                    None => {
                        h.write_header.stream = None;
                        h.write_header.method = PHP_CURL_IGNORE;
                    }
                    Some(id) if writable_mode(id) => {
                        h.write_header.stream = Some(Value::Resource(id));
                        h.write_header.method = PHP_CURL_FILE;
                    }
                    Some(_) => {
                        drop(st);
                        zpp::plain_value_error(
                            vm,
                            &format!("{}(): The provided file handle must be writable", f),
                        );
                        return Err(());
                    }
                },
                CURLOPT_INFILE => {
                    h.read.stream = what.map(Value::Resource);
                }
                _ => match what {
                    None => h.std_err = None,
                    Some(id) if writable_mode(id) => h.std_err = Some(Value::Resource(id)),
                    Some(_) => {
                        drop(st);
                        zpp::plain_value_error(
                            vm,
                            &format!("{}(): The provided file handle must be writable", f),
                        );
                        return Err(());
                    }
                },
            }
        }
        o if SLIST_OPTIONS.iter().any(|(x, _)| *x == o) => {
            let Value::Array(arr) = zvalue else {
                let name = SLIST_OPTIONS.iter().find(|(x, _)| *x == o).unwrap().1;
                zpp::throw(
                    vm,
                    b"TypeError",
                    &format!("{}(): The {} option must have an array value", f, name),
                );
                return Err(());
            };
            let vals: Vec<Value> = arr.borrow().values().cloned().collect();
            let mut list = Vec::with_capacity(vals.len());
            for v in vals {
                let s = zpp::zval_get_string(vm, &v.deref());
                if vm_has_exc(vm) {
                    return Err(());
                }
                list.push(s);
            }
            let slist = if list.is_empty() { None } else { Some(list) };
            error = setopt_raw(c, o, OptVal::Slist(slist));
        }
        CURLOPT_BINARYTRANSFER | CURLOPT_DNS_USE_GLOBAL_CACHE => {}
        CURLOPT_POSTFIELDS => {
            if let Value::Array(arr) = zvalue {
                if arr.borrow().is_empty() {
                    setopt_raw(c, CURLOPT_POSTFIELDS, OptVal::Str(Some(b"")));
                    error = setopt_raw(c, CURLOPT_POSTFIELDSIZE, OptVal::Long(0));
                } else {
                    return build_mime(vm, f, c, this, arr);
                }
            } else {
                let s = zpp::zval_get_string(vm, zvalue);
                if vm_has_exc(vm) {
                    return Err(());
                }
                setopt_raw(c, CURLOPT_POSTFIELDSIZE, OptVal::Long(s.len() as i64));
                error = setopt_raw(c, CURLOPT_COPYPOSTFIELDS, OptVal::Str(Some(&s)));
            }
        }
        CURLOPT_RETURNTRANSFER => {
            c.st.borrow_mut().handlers.write.method = if zvalue.is_truthy() {
                PHP_CURL_RETURN
            } else {
                PHP_CURL_STDOUT
            };
        }
        o if OFF_T_OPTIONS.contains(&o) => {
            let lval = zpp::zval_get_long(vm, zvalue);
            error = setopt_raw(c, o, OptVal::OffT(lval));
        }
        CURLOPT_POSTREDIR => {
            let lval = zpp::zval_get_long(vm, zvalue);
            error = setopt_raw(
                c,
                CURLOPT_POSTREDIR,
                OptVal::Long(lval & CURL_REDIR_POST_ALL),
            );
        }
        o if FILE_PATH_OPTIONS.contains(&o) => {
            let s = zpp::zval_get_string(vm, zvalue);
            if vm_has_exc(vm) {
                return Err(());
            }
            if !s.is_empty() && !vm.check_open_basedir(f, &String::from_utf8_lossy(&s)) {
                return Err(());
            }
            return option_str(vm, f, c, o, &s);
        }
        CURLINFO_HEADER_OUT => {
            if c.st.borrow().handlers.debug.is_some() {
                zpp::plain_value_error(
                    vm,
                    "CURLINFO_HEADER_OUT option must not be set when the CURLOPT_DEBUGFUNCTION option is set",
                );
                return Err(());
            }
            if zvalue.is_truthy() {
                setopt_raw(c, CURLOPT_DEBUGFUNCTION, OptVal::Callback(true));
                setopt_raw(c, CURLOPT_VERBOSE, OptVal::Long(1));
            } else {
                setopt_raw(c, CURLOPT_DEBUGFUNCTION, OptVal::Callback(false));
                setopt_raw(c, CURLOPT_VERBOSE, OptVal::Long(0));
            }
        }
        CURLOPT_SHARE => {
            if let Value::Object(o) = zvalue {
                if let Some(sh) = share::share_of(o) {
                    setopt_raw(c, CURLOPT_SHARE, OptVal::Share(Some(sh.share.clone())));
                    c.st.borrow_mut().share = Some(zvalue.clone());
                }
            }
        }
        o if BLOB_OPTIONS.contains(&o) => {
            let s = zpp::zval_get_string(vm, zvalue);
            if vm_has_exc(vm) {
                return Err(());
            }
            error = setopt_raw(c, o, OptVal::Blob(&s));
        }
        _ => {
            if is_array_config {
                zpp::value_error(vm, f, 2, "must contain only valid cURL options");
            } else {
                zpp::value_error(vm, f, 2, "is not a valid cURL option");
            }
            error = CURLE_UNKNOWN_OPTION as u32;
        }
    }
    save_error(c, error);
    if error != 0 { Err(()) } else { Ok(()) }
}

/// build_mime_structure_from_hash()
fn build_mime(
    vm: &mut Vm,
    _f: &str,
    c: &Rc<CurlObj>,
    _this: &Value,
    postfields: &Rc<RefCell<PhpArray>>,
) -> SetoptResult {
    let entries: Vec<(ArrayKey, Value)> = postfields
        .borrow()
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    let mut parts: Vec<MimePart> = Vec::new();
    let simple = |vm: &mut Vm, name: &[u8], v: &Value| -> Option<MimePart> {
        let s = zpp::zval_get_string(vm, &v.deref());
        if vm_has_exc(vm) {
            return None;
        }
        Some(MimePart {
            name: name.to_vec(),
            data: MimeData::Bytes(s),
            filename: None,
            content_type: None,
        })
    };
    for (k, v) in entries {
        let name = k.to_string().into_bytes();
        let v = v.deref();
        if let Value::Object(o) = &v {
            if is_a(vm, o, b"curlfile") {
                let prop = o.borrow().get_property(b"name");
                let Value::String(path) = prop.deref() else {
                    vm.emit_warning(&format!(
                        "{}(): Invalid filename for key {}",
                        vm.current_builtin_name().unwrap_or_default(),
                        String::from_utf8_lossy(&name)
                    ));
                    continue;
                };
                let path = path.as_bytes().to_vec();
                let fname = vm.current_builtin_name().unwrap_or_default();
                if !vm.check_open_basedir(&fname, &String::from_utf8_lossy(&path)) {
                    return Err(());
                }
                let nonempty = |p: Value| match p {
                    Value::String(s) if !s.as_bytes().is_empty() => Some(s.as_bytes().to_vec()),
                    _ => None,
                };
                let ty = nonempty(o.borrow().get_property(b"mime").deref());
                let postname = nonempty(o.borrow().get_property(b"postname").deref());
                c.st.borrow_mut().postfields = Some(Value::Array(postfields.clone()));
                let data = if path.windows(3).any(|w| w == b"://") || path.starts_with(b"data:") {
                    match streams::read_file(vm, &path) {
                        Some(d) => MimeData::Bytes(d),
                        None => MimeData::File(std::path::PathBuf::from(
                            String::from_utf8_lossy(&path).into_owned(),
                        )),
                    }
                } else {
                    use std::os::unix::ffi::OsStrExt;
                    MimeData::File(std::path::PathBuf::from(std::ffi::OsStr::from_bytes(&path)))
                };
                parts.push(MimePart {
                    name,
                    data,
                    filename: Some(postname.unwrap_or(path)),
                    content_type: Some(ty.unwrap_or_else(|| b"application/octet-stream".to_vec())),
                });
                continue;
            }
            if is_a(vm, o, b"curlstringfile") {
                let get = |n: &[u8]| o.borrow().get_property(n).deref().to_php_string();
                let postname = get(b"postname").as_bytes().to_vec();
                let ty = get(b"mime").as_bytes().to_vec();
                let data = get(b"data").as_bytes().to_vec();
                c.st.borrow_mut().postfields = Some(Value::Array(postfields.clone()));
                parts.push(MimePart {
                    name,
                    data: MimeData::Bytes(data),
                    filename: Some(postname),
                    content_type: Some(ty),
                });
                continue;
            }
        }
        if let Value::Array(a) = &v {
            let vals: Vec<Value> = a.borrow().values().cloned().collect();
            for e in vals {
                match simple(vm, &name, &e) {
                    Some(p) => parts.push(p),
                    None => return Err(()),
                }
            }
            continue;
        }
        match simple(vm, &name, &v) {
            Some(p) => parts.push(p),
            None => return Err(()),
        }
    }
    let error = setopt_raw(c, CURLOPT_MIMEPOST, OptVal::Mime(Some(Mime { parts })));
    save_error(c, error);
    let _ = CURLOPT_SEEKFUNCTION;
    if error == 0 { Ok(()) } else { Err(()) }
}

fn curl_setopt(vm: &mut Vm, f: &str, args: &[Value]) -> Value {
    let a = match zpp::parse(vm, f, "Clz", args) {
        Ok(a) => a,
        Err(()) => return Value::Null,
    };
    let (this, c) = arg_curl(&a[0]);
    match php_curl_setopt(vm, f, &this, &c, a[1].long(), &a[2].any(), false) {
        Ok(()) => Value::True,
        Err(()) => Value::False,
    }
}

fn curl_setopt_array(vm: &mut Vm, f: &str, args: &[Value]) -> Value {
    let a = match zpp::parse(vm, f, "Ca", args) {
        Ok(a) => a,
        Err(()) => return Value::Null,
    };
    let (this, c) = arg_curl(&a[0]);
    let Arg::Arr(arr) = &a[1] else {
        return Value::Null;
    };
    let entries: Vec<(ArrayKey, Value)> = arr
        .borrow()
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    for (k, v) in entries {
        let ArrayKey::Int(option) = k else {
            zpp::value_error(vm, f, 2, "contains an invalid cURL option");
            return Value::Null;
        };
        if php_curl_setopt(vm, f, &this, &c, option, &v.deref(), true).is_err() {
            return Value::False;
        }
    }
    Value::True
}

// ---------------------------------------------------------------------------
// curl_exec() and friends

fn curl_exec(vm: &mut Vm, f: &str, args: &[Value]) -> Value {
    let a = match zpp::parse(vm, f, "C", args) {
        Ok(a) => a,
        Err(()) => return Value::Null,
    };
    let (this, c) = arg_curl(&a[0]);
    verify_handlers(vm, &c, true);
    cleanup_handle(&c);
    let error = perform(vm, &this, &c);
    if error != 0 {
        let mut st = c.st.borrow_mut();
        st.handlers.write.buf.clear();
        st.handlers.write.buf_set = false;
        return Value::False;
    }
    let (std_err, wmethod, wstream, hmethod, hstream, buf) = {
        let st = c.st.borrow();
        let h = &st.handlers;
        (
            stream_id(&h.std_err),
            h.write.method,
            stream_id(&h.write.stream),
            h.write_header.method,
            stream_id(&h.write_header.stream),
            if h.write.buf_set {
                Some(h.write.buf.clone())
            } else {
                None
            },
        )
    };
    if let Some(id) = std_err {
        if streams::is_stream(id) {
            streams::flush(vm, id);
        }
    }
    if wmethod == PHP_CURL_RETURN {
        if let Some(b) = buf {
            return vstr(&b);
        }
    }
    if wmethod == PHP_CURL_FILE {
        if let Some(id) = wstream {
            streams::flush(vm, id);
        }
    }
    if hmethod == PHP_CURL_FILE {
        if let Some(id) = hstream {
            streams::flush(vm, id);
        }
    }
    if wmethod == PHP_CURL_RETURN {
        vstr(b"")
    } else {
        Value::True
    }
}

fn info_long(e: &Easy, info: i64) -> Option<i64> {
    match e.getinfo(info as u32)? {
        InfoVal::Long(n) | InfoVal::OffT(n) => Some(n),
        InfoVal::Double(d) => Some(d as i64),
        _ => None,
    }
}

fn info_double(e: &Easy, info: i64) -> Option<f64> {
    match e.getinfo(info as u32)? {
        InfoVal::Double(d) => Some(d),
        InfoVal::Long(n) | InfoVal::OffT(n) => Some(n as f64),
        _ => None,
    }
}

fn info_str(e: &Easy, info: i64) -> Option<Option<Vec<u8>>> {
    match e.getinfo(info as u32)? {
        InfoVal::Str(s) => Some(s),
        _ => None,
    }
}

/// create_certinfo()
fn certinfo_array(vm: &mut Vm, certs: Vec<Vec<Vec<u8>>>) -> PhpArray {
    let mut list = PhpArray::new();
    for cert in certs {
        let mut h = PhpArray::new();
        for line in cert {
            let head = &line[..line.len().min(63)];
            match head.iter().position(|&b| b == b':') {
                Some(p) => {
                    h.set(
                        ArrayKey::String(PhpString::from_bytes(&line[..p])),
                        vstr(&line[p + 1..]),
                    );
                }
                None => vm.emit_warning(&format!(
                    "{}(): Could not extract hash key from certificate info",
                    vm.current_builtin_name().unwrap_or_default()
                )),
            }
        }
        list.push(new_arr(h));
    }
    list
}

fn curl_getinfo(vm: &mut Vm, f: &str, args: &[Value]) -> Value {
    let a = match zpp::parse(vm, f, "C|l!", args) {
        Ok(a) => a,
        Err(()) => return Value::Null,
    };
    let (_, c) = arg_curl(&a[0]);
    let easy_ref = c.easy.borrow();
    let fallback = Easy::new();
    let e = easy_ref.as_ref().unwrap_or(&fallback);
    if !a[1].is_given() {
        let mut r = PhpArray::new();
        let s = |r: &mut PhpArray, k: &str, info: i64| {
            if let Some(v) = info_str(e, info) {
                r.set(key(k), vstr(&v.unwrap_or_default()));
            }
        };
        let l = |r: &mut PhpArray, k: &str, info: i64| {
            if let Some(v) = info_long(e, info) {
                r.set(key(k), Value::Long(v));
            }
        };
        let d = |r: &mut PhpArray, k: &str, info: i64| {
            if let Some(v) = info_double(e, info) {
                r.set(key(k), Value::Double(v));
            }
        };
        s(&mut r, "url", CURLINFO_EFFECTIVE_URL);
        if let Some(ct) = info_str(e, CURLINFO_CONTENT_TYPE) {
            r.set(
                key("content_type"),
                match ct {
                    Some(v) => vstr(&v),
                    None => Value::Null,
                },
            );
        }
        l(&mut r, "http_code", CURLINFO_HTTP_CODE);
        l(&mut r, "header_size", CURLINFO_HEADER_SIZE);
        l(&mut r, "request_size", CURLINFO_REQUEST_SIZE);
        l(&mut r, "filetime", CURLINFO_FILETIME);
        l(&mut r, "ssl_verify_result", CURLINFO_SSL_VERIFYRESULT);
        l(&mut r, "redirect_count", CURLINFO_REDIRECT_COUNT);
        d(&mut r, "total_time", CURLINFO_TOTAL_TIME);
        d(&mut r, "namelookup_time", CURLINFO_NAMELOOKUP_TIME);
        d(&mut r, "connect_time", CURLINFO_CONNECT_TIME);
        d(&mut r, "pretransfer_time", CURLINFO_PRETRANSFER_TIME);
        d(&mut r, "size_upload", CURLINFO_SIZE_UPLOAD);
        d(&mut r, "size_download", CURLINFO_SIZE_DOWNLOAD);
        d(&mut r, "speed_download", CURLINFO_SPEED_DOWNLOAD);
        d(&mut r, "speed_upload", CURLINFO_SPEED_UPLOAD);
        d(
            &mut r,
            "download_content_length",
            CURLINFO_CONTENT_LENGTH_DOWNLOAD,
        );
        d(
            &mut r,
            "upload_content_length",
            CURLINFO_CONTENT_LENGTH_UPLOAD,
        );
        d(&mut r, "starttransfer_time", CURLINFO_STARTTRANSFER_TIME);
        d(&mut r, "redirect_time", CURLINFO_REDIRECT_TIME);
        s(&mut r, "redirect_url", CURLINFO_REDIRECT_URL);
        s(&mut r, "primary_ip", CURLINFO_PRIMARY_IP);
        if let Some(InfoVal::CertInfo(ci)) = e.getinfo(CURLINFO_CERTINFO as u32) {
            let arr = certinfo_array(vm, ci);
            r.set(key("certinfo"), new_arr(arr));
        } else {
            r.set(key("certinfo"), new_arr(PhpArray::new()));
        }
        l(&mut r, "primary_port", CURLINFO_PRIMARY_PORT);
        s(&mut r, "local_ip", CURLINFO_LOCAL_IP);
        l(&mut r, "local_port", CURLINFO_LOCAL_PORT);
        l(&mut r, "http_version", CURLINFO_HTTP_VERSION);
        l(&mut r, "protocol", CURLINFO_PROTOCOL);
        l(&mut r, "ssl_verifyresult", CURLINFO_PROXY_SSL_VERIFYRESULT);
        s(&mut r, "scheme", CURLINFO_SCHEME);
        l(&mut r, "appconnect_time_us", CURLINFO_APPCONNECT_TIME_T);
        l(&mut r, "queue_time_us", CURLINFO_QUEUE_TIME_T);
        l(&mut r, "connect_time_us", CURLINFO_CONNECT_TIME_T);
        l(&mut r, "namelookup_time_us", CURLINFO_NAMELOOKUP_TIME_T);
        l(&mut r, "pretransfer_time_us", CURLINFO_PRETRANSFER_TIME_T);
        l(&mut r, "redirect_time_us", CURLINFO_REDIRECT_TIME_T);
        l(
            &mut r,
            "starttransfer_time_us",
            CURLINFO_STARTTRANSFER_TIME_T,
        );
        l(&mut r, "posttransfer_time_us", CURLINFO_POSTTRANSFER_TIME_T);
        l(&mut r, "total_time_us", CURLINFO_TOTAL_TIME_T);
        if let Some(h) = &c.st.borrow().header_out {
            r.set(key("request_header"), vstr(h));
        }
        s(&mut r, "effective_method", CURLINFO_EFFECTIVE_METHOD);
        s(&mut r, "capath", CURLINFO_CAPATH);
        s(&mut r, "cainfo", CURLINFO_CAINFO);
        l(&mut r, "used_proxy", CURLINFO_USED_PROXY);
        l(&mut r, "httpauth_used", CURLINFO_HTTPAUTH_USED);
        l(&mut r, "proxyauth_used", CURLINFO_PROXYAUTH_USED);
        l(&mut r, "conn_id", CURLINFO_CONN_ID);
        return new_arr(r);
    }
    let option = a[1].long();
    match option {
        CURLINFO_HEADER_OUT => match &c.st.borrow().header_out {
            Some(h) => vstr(h),
            None => Value::False,
        },
        CURLINFO_CERTINFO => match e.getinfo(option as u32) {
            Some(InfoVal::CertInfo(ci)) => new_arr(certinfo_array(vm, ci)),
            Some(_) => new_arr(PhpArray::new()),
            None => Value::False,
        },
        CURLINFO_PRIVATE => match &c.st.borrow().private_data {
            Some(v) => v.clone(),
            None => Value::False,
        },
        _ => match option & CURLINFO_TYPEMASK {
            CURLINFO_STRING => match info_str(e, option) {
                Some(Some(s)) => vstr(&s),
                _ => Value::False,
            },
            CURLINFO_LONG | CURLINFO_OFF_T => match info_long(e, option) {
                Some(n) => Value::Long(n),
                None => Value::False,
            },
            CURLINFO_DOUBLE => match info_double(e, option) {
                Some(d) => Value::Double(d),
                None => Value::False,
            },
            CURLINFO_SLIST => match e.getinfo(option as u32) {
                Some(InfoVal::Slist(l)) => {
                    let mut arr = PhpArray::new();
                    for s in l {
                        arr.push(vstr(&s));
                    }
                    new_arr(arr)
                }
                _ => Value::False,
            },
            _ => Value::False,
        },
    }
}

fn curl_error(vm: &mut Vm, f: &str, args: &[Value]) -> Value {
    let a = match zpp::parse(vm, f, "C", args) {
        Ok(a) => a,
        Err(()) => return Value::Null,
    };
    let (_, c) = arg_curl(&a[0]);
    let st = c.st.borrow();
    if st.err_no != 0 {
        let s = &st.err_str[..st.err_str.len().min(256)];
        let s = match s.iter().position(|&b| b == 0) {
            Some(z) => &s[..z],
            None => s,
        };
        if !s.is_empty() {
            vstr(s)
        } else {
            vstr(engine::easy_strerror(st.err_no as u32).as_bytes())
        }
    } else {
        vstr(b"")
    }
}

fn curl_errno(vm: &mut Vm, f: &str, args: &[Value]) -> Value {
    let a = match zpp::parse(vm, f, "C", args) {
        Ok(a) => a,
        Err(()) => return Value::Null,
    };
    let (_, c) = arg_curl(&a[0]);
    Value::Long(c.st.borrow().err_no)
}

fn curl_close(vm: &mut Vm, f: &str, args: &[Value]) -> Value {
    vm.emit_deprecated(
        "Function curl_close() is deprecated since 8.5, as it has no effect since PHP 8.0",
    );
    if vm.current_exception.is_some() {
        return Value::Null;
    }
    let a = match zpp::parse(vm, f, "C", args) {
        Ok(a) => a,
        Err(()) => return Value::Null,
    };
    let (_, c) = arg_curl(&a[0]);
    if c.st.borrow().in_callback {
        zpp::throw(
            vm,
            b"Error",
            &format!("{}(): Attempt to close cURL handle from a callback", f),
        );
    }
    Value::Null
}

fn curl_strerror(vm: &mut Vm, f: &str, args: &[Value]) -> Value {
    let a = match zpp::parse(vm, f, "l", args) {
        Ok(a) => a,
        Err(()) => return Value::Null,
    };
    vstr(engine::easy_strerror(a[0].long() as u32).as_bytes())
}

/// _php_curl_reset_handlers()
fn reset_handlers(c: &CurlObj) {
    let mut st = c.st.borrow_mut();
    let h = &mut st.handlers;
    let buf = std::mem::take(&mut h.write.buf);
    let buf_set = h.write.buf_set;
    *h = Handlers::default();
    h.write.method = PHP_CURL_STDOUT;
    h.write.buf = buf;
    h.write.buf_set = buf_set;
    h.write_header.method = PHP_CURL_IGNORE;
    h.read.method = PHP_CURL_DIRECT;
}

fn curl_reset(vm: &mut Vm, f: &str, args: &[Value]) -> Value {
    let a = match zpp::parse(vm, f, "C", args) {
        Ok(a) => a,
        Err(()) => return Value::Null,
    };
    let (_, c) = arg_curl(&a[0]);
    if c.st.borrow().in_callback {
        zpp::throw(
            vm,
            b"Error",
            &format!("{}(): Attempt to reset cURL handle from a callback", f),
        );
        return Value::Null;
    }
    with_easy(&c, |e| e.reset());
    reset_handlers(&c);
    set_default_options(vm, &c);
    Value::Null
}

fn curl_escape(vm: &mut Vm, f: &str, args: &[Value]) -> Value {
    let a = match zpp::parse(vm, f, "Cs", args) {
        Ok(a) => a,
        Err(()) => return Value::Null,
    };
    if a[1].str().len() > i32::MAX as usize {
        return Value::False;
    }
    vstr(&engine::easy_escape(a[1].str()))
}

fn curl_unescape(vm: &mut Vm, f: &str, args: &[Value]) -> Value {
    let a = match zpp::parse(vm, f, "Cs", args) {
        Ok(a) => a,
        Err(()) => return Value::Null,
    };
    if a[1].str().len() > i32::MAX as usize {
        return Value::False;
    }
    vstr(&engine::easy_unescape(a[1].str()))
}

fn curl_pause(vm: &mut Vm, f: &str, args: &[Value]) -> Value {
    let a = match zpp::parse(vm, f, "Cl", args) {
        Ok(a) => a,
        Err(()) => return Value::Null,
    };
    let (_, c) = arg_curl(&a[0]);
    let bitmask = a[1].long();
    Value::Long(with_easy(&c, |e| e.pause(bitmask)).unwrap_or(0) as i64)
}

fn curl_upkeep(vm: &mut Vm, f: &str, args: &[Value]) -> Value {
    let a = match zpp::parse(vm, f, "C", args) {
        Ok(a) => a,
        Err(()) => return Value::Null,
    };
    let (_, c) = arg_curl(&a[0]);
    let error = with_easy(&c, |e| e.upkeep()).unwrap_or(0);
    save_error(&c, error);
    if error == 0 {
        Value::True
    } else {
        Value::False
    }
}

// ---------------------------------------------------------------------------
// registration

/// A native class without constructor (get_constructor throws).
pub(crate) fn register_handle_class(vm: &mut Vm, name: &[u8], ctor_msg: &'static str) {
    let ce = match goro_core::stubs::class_entry(vm, name) {
        Some(stub) => stub.clone(),
        None => {
            let mut ce = ClassEntry::new(name.to_vec());
            ce.is_final = true;
            ce
        }
    };
    let lower = name.to_ascii_lowercase();
    vm.classes.insert(lower.clone(), ce);
    CTOR_MSGS.with(|m| m.borrow_mut().push((lower.clone(), ctor_msg)));
    vm.native_class_handlers.insert(
        lower,
        NativeClassHandler {
            has_method: handle_has_method,
            call: handle_call_method,
            prop_added: None,
            prop_write: None,
            compare_check: None,
            prop_read: None,
            param_names: None,
        },
    );
}

thread_local! {
    static CTOR_MSGS: RefCell<Vec<(Vec<u8>, &'static str)>> = const { RefCell::new(Vec::new()) };
}

fn handle_has_method(_class: &[u8], method: &[u8], _is_static: bool) -> bool {
    method.eq_ignore_ascii_case(b"__construct")
}

fn handle_call_method(
    vm: &mut Vm,
    class: &[u8],
    method: &[u8],
    _called: &[u8],
    _this: Option<&Value>,
    _args: &[Value],
) -> Value {
    if method.eq_ignore_ascii_case(b"__construct") {
        let lower = class.to_ascii_lowercase();
        let msg = CTOR_MSGS.with(|m| {
            m.borrow()
                .iter()
                .find(|(c, _)| *c == lower)
                .map(|(_, s)| *s)
        });
        if let Some(msg) = msg {
            zpp::throw(vm, b"Error", msg);
        }
    }
    Value::Null
}

macro_rules! php_functions {
    ($($name:literal => $imp:path),* $(,)?) => {
        fn register_functions(vm: &mut Vm) {
            $(
                {
                    fn w(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
                        Ok($imp(vm, $name, args))
                    }
                    vm.register_function($name.as_bytes(), w);
                }
            )*
        }
    };
}

php_functions! {
    "curl_init" => curl_init,
    "curl_copy_handle" => curl_copy_handle,
    "curl_setopt" => curl_setopt,
    "curl_setopt_array" => curl_setopt_array,
    "curl_exec" => curl_exec,
    "curl_getinfo" => curl_getinfo,
    "curl_error" => curl_error,
    "curl_errno" => curl_errno,
    "curl_close" => curl_close,
    "curl_strerror" => curl_strerror,
    "curl_reset" => curl_reset,
    "curl_escape" => curl_escape,
    "curl_unescape" => curl_unescape,
    "curl_pause" => curl_pause,
    "curl_upkeep" => curl_upkeep,
    "curl_version" => curl_version,
    "curl_multi_init" => multi::curl_multi_init,
    "curl_multi_add_handle" => multi::curl_multi_add_handle,
    "curl_multi_remove_handle" => multi::curl_multi_remove_handle,
    "curl_multi_get_handles" => multi::curl_multi_get_handles,
    "curl_multi_select" => multi::curl_multi_select,
    "curl_multi_exec" => multi::curl_multi_exec,
    "curl_multi_getcontent" => multi::curl_multi_getcontent,
    "curl_multi_info_read" => multi::curl_multi_info_read,
    "curl_multi_close" => multi::curl_multi_close,
    "curl_multi_errno" => multi::curl_multi_errno,
    "curl_multi_strerror" => multi::curl_multi_strerror,
    "curl_multi_setopt" => multi::curl_multi_setopt,
    "curl_share_init" => share::curl_share_init,
    "curl_share_init_persistent" => share::curl_share_init_persistent,
    "curl_share_close" => share::curl_share_close,
    "curl_share_setopt" => share::curl_share_setopt,
    "curl_share_errno" => share::curl_share_errno,
    "curl_share_strerror" => share::curl_share_strerror,
    "curl_file_create" => curl_file::curl_file_create,
}

pub fn register(vm: &mut Vm) {
    vm.register_extension(b"curl");
    register_handle_class(
        vm,
        b"CurlHandle",
        "Cannot directly construct CurlHandle, use curl_init() instead",
    );
    register_handle_class(
        vm,
        b"CurlMultiHandle",
        "Cannot directly construct CurlMultiHandle, use curl_multi_init() instead",
    );
    register_handle_class(
        vm,
        b"CurlShareHandle",
        "Cannot directly construct CurlShareHandle, use curl_share_init() instead",
    );
    register_handle_class(
        vm,
        b"CurlSharePersistentHandle",
        "Cannot directly construct CurlSharePersistentHandle, use curl_share_init_persistent() instead",
    );
    curl_file::register(vm);
    register_functions(vm);
    for (name, v, deprecated) in consts::CONSTANTS {
        vm.constants.insert(name.to_vec(), Value::Long(*v));
        if *deprecated {
            vm.internal_deprecated_constants.insert(
                name.to_vec(),
                format!(
                    "Constant {} is deprecated since 8.4, as it had no effect since 5.1.2",
                    String::from_utf8_lossy(name)
                ),
            );
        }
    }
    vm.constants
        .entry(b"curl.cainfo".to_vec())
        .or_insert(Value::String(PhpString::from_bytes(b"")));
}
