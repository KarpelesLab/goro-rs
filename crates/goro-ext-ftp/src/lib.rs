//! php-src's ext/ftp (php_ftp.c): the ftp_* functions and the
//! FTP\Connection class over the client of `ftp` (ftp.c). FTPS uses the
//! openssl extension's TLS layer (`Vm::crypto_hooks`).

use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;

use goro_core::array::PhpArray;
use goro_core::object::{ClassEntry, NativeData, PhpObject};
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{NativeObjectHandlers, Vm, VmError};

mod ftp;
mod streams;
mod zpp;

use ftp::{Ftp, FtpType, PHP_FTP_FAILED, PHP_FTP_FINISHED, PHP_FTP_MOREDATA};
use zpp::{Arg, Obj};

/// The extension name (`extension_loaded('ftp')`).
pub const NAME: &[u8] = b"ftp";

pub(crate) const CLASS_NAME: &[u8] = b"FTP\\Connection";

type R = Result<Value, VmError>;

const PHP_FTP_OPT_TIMEOUT_SEC: i64 = 0;
const PHP_FTP_OPT_AUTOSEEK: i64 = 1;
const PHP_FTP_OPT_USEPASVADDRESS: i64 = 2;
const PHP_FTP_AUTORESUME: i64 = -1;

const CONSTANTS: &[(&str, i64)] = &[
    ("FTP_ASCII", 1),
    ("FTP_TEXT", 1),
    ("FTP_BINARY", 2),
    ("FTP_IMAGE", 2),
    ("FTP_AUTORESUME", PHP_FTP_AUTORESUME),
    ("FTP_TIMEOUT_SEC", PHP_FTP_OPT_TIMEOUT_SEC),
    ("FTP_AUTOSEEK", PHP_FTP_OPT_AUTOSEEK),
    ("FTP_USEPASVADDRESS", PHP_FTP_OPT_USEPASVADDRESS),
    ("FTP_FAILED", PHP_FTP_FAILED),
    ("FTP_FINISHED", PHP_FTP_FINISHED),
    ("FTP_MOREDATA", PHP_FTP_MOREDATA),
];

const FUNCTIONS: &[(&str, goro_core::vm::BuiltinFn)] = &[
    ("ftp_connect", ftp_connect),
    ("ftp_ssl_connect", ftp_ssl_connect),
    ("ftp_login", ftp_login),
    ("ftp_pwd", ftp_pwd),
    ("ftp_cdup", ftp_cdup),
    ("ftp_chdir", ftp_chdir),
    ("ftp_exec", ftp_exec),
    ("ftp_raw", ftp_raw),
    ("ftp_mkdir", ftp_mkdir),
    ("ftp_rmdir", ftp_rmdir),
    ("ftp_chmod", ftp_chmod),
    ("ftp_alloc", ftp_alloc),
    ("ftp_nlist", ftp_nlist),
    ("ftp_rawlist", ftp_rawlist),
    ("ftp_mlsd", ftp_mlsd),
    ("ftp_systype", ftp_systype),
    ("ftp_fget", ftp_fget),
    ("ftp_nb_fget", ftp_nb_fget),
    ("ftp_pasv", ftp_pasv),
    ("ftp_get", ftp_get),
    ("ftp_nb_get", ftp_nb_get),
    ("ftp_nb_continue", ftp_nb_continue),
    ("ftp_fput", ftp_fput),
    ("ftp_nb_fput", ftp_nb_fput),
    ("ftp_put", ftp_put),
    ("ftp_append", ftp_append),
    ("ftp_nb_put", ftp_nb_put),
    ("ftp_size", ftp_size),
    ("ftp_mdtm", ftp_mdtm),
    ("ftp_rename", ftp_rename),
    ("ftp_delete", ftp_delete),
    ("ftp_site", ftp_site),
    ("ftp_close", ftp_close),
    ("ftp_quit", ftp_close),
    ("ftp_set_option", ftp_set_option),
    ("ftp_get_option", ftp_get_option),
];

/// php_ftp_object
struct FtpObj {
    ftp: RefCell<Option<Box<Ftp>>>,
}

/// zend_objects_not_comparable
fn not_comparable(a: &Value, b: &Value) -> Option<i64> {
    match (a, b) {
        (Value::Object(x), Value::Object(y)) if Rc::ptr_eq(x, y) => Some(0),
        (Value::Object(_), Value::Object(_)) => Some(1),
        _ => None,
    }
}

static FTP_HANDLERS: NativeObjectHandlers = NativeObjectHandlers {
    compare: Some(not_comparable),
    ..NativeObjectHandlers::EMPTY
};

/// MINIT
pub fn register(vm: &mut Vm) {
    if vm.loaded_extensions.contains(NAME) {
        return;
    }
    vm.register_extension(NAME);
    // PHP_MINFO_FUNCTION(ftp)
    vm.phpinfo_hooks
        .push(|_| "\nftp\n\nFTP support => enabled\nFTPS support => enabled\n".to_string());
    let mut ce = match goro_core::stubs::class_entry(vm, CLASS_NAME) {
        Some(stub) => stub.clone(),
        None => ClassEntry::new(CLASS_NAME.to_vec()),
    };
    ce.is_final = true;
    let lower = CLASS_NAME.to_ascii_lowercase();
    vm.classes.insert(lower.clone(), ce);
    vm.native_class_handlers.insert(
        lower,
        goro_core::vm::NativeClassHandler {
            has_method: |_c, m, _s| m.eq_ignore_ascii_case(b"__construct"),
            call: ctor_call,
            prop_added: None,
            prop_write: None,
            compare_check: None,
            prop_read: None,
            param_names: None,
        },
    );
    for &(name, f) in FUNCTIONS {
        let params: Vec<&[u8]> = goro_core::stubs::find_function(name.as_bytes())
            .map(|s| s.args.iter().map(|a| a.name.as_bytes()).collect())
            .unwrap_or_default();
        vm.register_function_with_params(name.as_bytes(), f, &params);
        vm.zpp_extension_verified
            .insert(name.as_bytes().to_vec(), goro_core::arginfo::CHECK_ARITY);
    }
    for &(name, v) in CONSTANTS {
        vm.constants
            .insert(name.as_bytes().to_vec(), Value::Long(v));
    }
    vm.module_constants.push((
        NAME.to_vec(),
        CONSTANTS
            .iter()
            .map(|(n, _)| n.as_bytes().to_vec())
            .collect(),
    ));
}

/// ftp_object_get_constructor()
fn ctor_call(
    vm: &mut Vm,
    _class: &[u8],
    method: &[u8],
    _called: &[u8],
    _this: Option<&Value>,
    _args: &[Value],
) -> Value {
    if method.eq_ignore_ascii_case(b"__construct") {
        zpp::throw(
            vm,
            b"Error",
            "Cannot directly construct FTP\\Connection, use ftp_connect() or ftp_ssl_connect() instead",
        );
    }
    Value::Null
}

fn new_object(vm: &mut Vm, ftp: Box<Ftp>) -> Value {
    let id = vm.next_object_id();
    let mut obj = PhpObject::new(CLASS_NAME.to_vec(), id);
    obj.native = Some(NativeData {
        data: Rc::new(FtpObj {
            ftp: RefCell::new(Some(ftp)),
        }) as Rc<dyn Any>,
        handlers: &FTP_HANDLERS,
    });
    Value::Object(Rc::new(RefCell::new(obj)))
}

fn handle_of(o: &Obj) -> Option<Rc<FtpObj>> {
    let d = o.borrow().native.as_ref().map(|n| n.data.clone())?;
    d.downcast::<FtpObj>().ok()
}

/// GET_FTPBUF(): run `f` on the connection, or throw when it is closed.
fn with_ftp(vm: &mut Vm, func: &'static str, o: &Obj, f: impl FnOnce(&mut Vm, &mut Ftp) -> R) -> R {
    let h = handle_of(o);
    let taken = h.as_ref().and_then(|h| h.ftp.borrow_mut().take());
    let Some(mut ftp) = taken else {
        zpp::throw(vm, b"ValueError", "FTP\\Connection is already closed");
        return Ok(Value::Null);
    };
    ftp.func = func;
    let r = f(vm, &mut ftp);
    if let Some(h) = h {
        *h.ftp.borrow_mut() = Some(ftp);
    }
    r
}

fn parse(vm: &mut Vm, f: &str, spec: &str, args: &[Value]) -> Option<Vec<Arg>> {
    zpp::parse(vm, f, spec, args).ok()
}

fn obj(a: &Arg) -> Obj {
    match a {
        Arg::Obj(o) => o.clone(),
        _ => unreachable!(),
    }
}

fn sval(b: &[u8]) -> Value {
    Value::String(PhpString::from_bytes(b))
}

fn bval(b: bool) -> Value {
    if b { Value::True } else { Value::False }
}

/// `if (*ftp->inbuf) php_error_docref(NULL, E_WARNING, "%s", ftp->inbuf)`
fn warn_inbuf(vm: &mut Vm, ftp: &Ftp) {
    let t = ftp.inbuf_str();
    if !t.is_empty() {
        let msg = format!("{}(): {}", ftp.func, String::from_utf8_lossy(t));
        vm.emit_warning(&msg);
    }
}

/// XTYPE()
fn xtype(vm: &mut Vm, f: &str, mode: i64) -> Option<FtpType> {
    let t = FtpType::from_mode(mode);
    if t.is_none() {
        zpp::value_error(vm, f, 4, "must be either FTP_ASCII or FTP_BINARY");
    }
    t
}

// ---------------------------------------------------------------------------
// Connections
// ---------------------------------------------------------------------------

fn connect(vm: &mut Vm, args: &[Value], func: &'static str, ssl: bool) -> R {
    let Some(a) = parse(vm, func, "s|ll", args) else {
        return Ok(Value::Null);
    };
    let host = a[0].str().to_vec();
    let port = a[1].long_or(0);
    let timeout = a[2].long_or(ftp::FTP_DEFAULT_TIMEOUT);
    if timeout <= 0 {
        zpp::value_error(vm, func, 3, "must be greater than 0");
        return Ok(Value::Null);
    }
    if !ssl {
        let timeoutmax = (u64::MAX as f64 / 1000000.0) as u64;
        if timeout as u64 >= timeoutmax {
            zpp::value_error(vm, func, 3, &format!("must be less than {}", timeoutmax));
            return Ok(Value::Null);
        }
    }
    // (short)port, then (unsigned short)
    let Some(mut ftp) = Ftp::open(vm, func, &host, port as i16 as u16, timeout) else {
        return Ok(Value::False);
    };
    ftp.autoseek = true;
    ftp.usepasvaddress = true;
    ftp.use_ssl = ssl;
    Ok(new_object(vm, ftp))
}

fn ftp_connect(vm: &mut Vm, args: &[Value]) -> R {
    connect(vm, args, "ftp_connect", false)
}

fn ftp_ssl_connect(vm: &mut Vm, args: &[Value]) -> R {
    connect(vm, args, "ftp_ssl_connect", true)
}

fn ftp_login(vm: &mut Vm, args: &[Value]) -> R {
    let f = "ftp_login";
    let Some(a) = parse(vm, f, "Oss", args) else {
        return Ok(Value::Null);
    };
    let (user, pass) = (a[1].str().to_vec(), a[2].str().to_vec());
    with_ftp(vm, f, &obj(&a[0]), |vm, ftp| {
        if !ftp.login(vm, &user, &pass) {
            warn_inbuf(vm, ftp);
            return Ok(Value::False);
        }
        Ok(Value::True)
    })
}

fn ftp_pwd(vm: &mut Vm, args: &[Value]) -> R {
    let f = "ftp_pwd";
    let Some(a) = parse(vm, f, "O", args) else {
        return Ok(Value::Null);
    };
    with_ftp(vm, f, &obj(&a[0]), |vm, ftp| match ftp.pwd(vm) {
        Some(p) => Ok(sval(&p)),
        None => {
            warn_inbuf(vm, ftp);
            Ok(Value::False)
        }
    })
}

/// A function taking the connection and an optional string, returning
/// true or false with the response as a warning.
fn bool_cmd(
    vm: &mut Vm,
    args: &[Value],
    f: &'static str,
    spec: &str,
    op: fn(&mut Ftp, &mut Vm, &[u8]) -> bool,
) -> R {
    let Some(a) = parse(vm, f, spec, args) else {
        return Ok(Value::Null);
    };
    let s = a.get(1).map(|x| x.str().to_vec()).unwrap_or_default();
    with_ftp(vm, f, &obj(&a[0]), |vm, ftp| {
        if !op(ftp, vm, &s) {
            warn_inbuf(vm, ftp);
            return Ok(Value::False);
        }
        Ok(Value::True)
    })
}

fn ftp_cdup(vm: &mut Vm, args: &[Value]) -> R {
    bool_cmd(vm, args, "ftp_cdup", "O", |ftp, vm, _| ftp.cdup(vm))
}

fn ftp_chdir(vm: &mut Vm, args: &[Value]) -> R {
    bool_cmd(vm, args, "ftp_chdir", "Os", |ftp, vm, d| ftp.chdir(vm, d))
}

fn ftp_exec(vm: &mut Vm, args: &[Value]) -> R {
    bool_cmd(vm, args, "ftp_exec", "Os", |ftp, vm, c| ftp.exec(vm, c))
}

fn ftp_rmdir(vm: &mut Vm, args: &[Value]) -> R {
    bool_cmd(vm, args, "ftp_rmdir", "Os", |ftp, vm, d| ftp.rmdir(vm, d))
}

fn ftp_delete(vm: &mut Vm, args: &[Value]) -> R {
    bool_cmd(vm, args, "ftp_delete", "Os", |ftp, vm, p| ftp.delete(vm, p))
}

fn ftp_site(vm: &mut Vm, args: &[Value]) -> R {
    bool_cmd(vm, args, "ftp_site", "Os", |ftp, vm, c| ftp.site(vm, c))
}

fn ftp_raw(vm: &mut Vm, args: &[Value]) -> R {
    let f = "ftp_raw";
    let Some(a) = parse(vm, f, "Os", args) else {
        return Ok(Value::Null);
    };
    let cmd = a[1].str().to_vec();
    with_ftp(vm, f, &obj(&a[0]), |vm, ftp| {
        Ok(match ftp.raw(vm, &cmd) {
            Some(arr) => Value::Array(Rc::new(RefCell::new(arr))),
            None => Value::Null,
        })
    })
}

fn ftp_mkdir(vm: &mut Vm, args: &[Value]) -> R {
    let f = "ftp_mkdir";
    let Some(a) = parse(vm, f, "Os", args) else {
        return Ok(Value::Null);
    };
    let dir = a[1].str().to_vec();
    with_ftp(vm, f, &obj(&a[0]), |vm, ftp| match ftp.mkdir(vm, &dir) {
        Some(d) => Ok(sval(&d)),
        None => {
            warn_inbuf(vm, ftp);
            Ok(Value::False)
        }
    })
}

fn ftp_chmod(vm: &mut Vm, args: &[Value]) -> R {
    let f = "ftp_chmod";
    let Some(a) = parse(vm, f, "Olp", args) else {
        return Ok(Value::Null);
    };
    let mode = a[1].long_or(0);
    let file = a[2].str().to_vec();
    with_ftp(vm, f, &obj(&a[0]), |vm, ftp| {
        if !ftp.chmod(vm, mode as i32, &file) {
            warn_inbuf(vm, ftp);
            return Ok(Value::False);
        }
        Ok(Value::Long(mode))
    })
}

fn ftp_alloc(vm: &mut Vm, args: &[Value]) -> R {
    let f = "ftp_alloc";
    let Some(a) = parse(vm, f, "Ol|z", args) else {
        return Ok(Value::Null);
    };
    let size = a[1].long_or(0);
    let zresponse = match &a[2] {
        Arg::Any(v) => Some(v.clone()),
        _ => None,
    };
    with_ftp(vm, f, &obj(&a[0]), |vm, ftp| {
        let (ok, response) = ftp.alloc(vm, size);
        if let (Some(r), Some(Value::Reference(z))) = (response, &zresponse) {
            if zresponse.is_some() {
                *z.borrow_mut() = sval(&r);
            }
        }
        let _ = vm;
        Ok(bval(ok))
    })
}

fn list_result(lines: Option<Vec<Vec<u8>>>) -> Value {
    match lines {
        Some(l) => {
            let mut arr = PhpArray::new();
            for line in l {
                arr.push(sval(&line));
            }
            Value::Array(Rc::new(RefCell::new(arr)))
        }
        None => Value::False,
    }
}

fn ftp_nlist(vm: &mut Vm, args: &[Value]) -> R {
    let f = "ftp_nlist";
    let Some(a) = parse(vm, f, "Op", args) else {
        return Ok(Value::Null);
    };
    let dir = a[1].str().to_vec();
    with_ftp(vm, f, &obj(&a[0]), |vm, ftp| {
        Ok(list_result(ftp.genlist(vm, b"NLST", &dir)))
    })
}

fn ftp_rawlist(vm: &mut Vm, args: &[Value]) -> R {
    let f = "ftp_rawlist";
    let Some(a) = parse(vm, f, "Os|b", args) else {
        return Ok(Value::Null);
    };
    let dir = a[1].str().to_vec();
    let recursive = a[2].bool_or(false);
    with_ftp(vm, f, &obj(&a[0]), |vm, ftp| {
        let cmd: &[u8] = if recursive { b"LIST -R" } else { b"LIST" };
        Ok(list_result(ftp.genlist(vm, cmd, &dir)))
    })
}

fn ftp_mlsd(vm: &mut Vm, args: &[Value]) -> R {
    let f = "ftp_mlsd";
    let Some(a) = parse(vm, f, "Os", args) else {
        return Ok(Value::Null);
    };
    let dir = a[1].str().to_vec();
    with_ftp(vm, f, &obj(&a[0]), |vm, ftp| {
        let Some(lines) = ftp.genlist(vm, b"MLSD", &dir) else {
            return Ok(Value::False);
        };
        let mut arr = PhpArray::new();
        for line in lines {
            match ftp::mlsd_parse_line(&line) {
                Ok(e) => arr.push(Value::Array(Rc::new(RefCell::new(e)))),
                Err(w) => vm.emit_warning(&format!("{}(): {}", f, w)),
            }
        }
        Ok(Value::Array(Rc::new(RefCell::new(arr))))
    })
}

fn ftp_systype(vm: &mut Vm, args: &[Value]) -> R {
    let f = "ftp_systype";
    let Some(a) = parse(vm, f, "O", args) else {
        return Ok(Value::Null);
    };
    with_ftp(vm, f, &obj(&a[0]), |vm, ftp| match ftp.syst(vm) {
        Some(s) => Ok(sval(&s)),
        None => {
            warn_inbuf(vm, ftp);
            Ok(Value::False)
        }
    })
}

fn ftp_pasv(vm: &mut Vm, args: &[Value]) -> R {
    let f = "ftp_pasv";
    let Some(a) = parse(vm, f, "Ob", args) else {
        return Ok(Value::Null);
    };
    let pasv = a[1].bool_or(false);
    with_ftp(vm, f, &obj(&a[0]), |vm, ftp| {
        Ok(bval(ftp.set_pasv(vm, pasv)))
    })
}

// ---------------------------------------------------------------------------
// Transfers
// ---------------------------------------------------------------------------

/// The resume position handling of the get functions on an open stream.
fn seek_for_resume(vm: &mut Vm, ftp: &Ftp, stream: i64, resumepos: i64) -> i64 {
    let mut resumepos = resumepos;
    if !ftp.autoseek && resumepos == PHP_FTP_AUTORESUME {
        resumepos = 0;
    }
    if ftp.autoseek && resumepos != 0 {
        if resumepos == PHP_FTP_AUTORESUME {
            streams::seek(vm, stream, 0, 2);
            resumepos = streams::tell(vm, stream);
        } else {
            streams::seek(vm, stream, resumepos, 0);
        }
    }
    resumepos
}

fn ftp_fget(vm: &mut Vm, args: &[Value]) -> R {
    fget_common(vm, args, "ftp_fget", false)
}

fn ftp_nb_fget(vm: &mut Vm, args: &[Value]) -> R {
    fget_common(vm, args, "ftp_nb_fget", true)
}

fn fget_common(vm: &mut Vm, args: &[Value], f: &'static str, nb: bool) -> R {
    let Some(a) = parse(vm, f, "Ors|ll", args) else {
        return Ok(Value::Null);
    };
    let Arg::Res(stream) = a[1] else {
        return Ok(Value::Null);
    };
    let file = a[2].str().to_vec();
    let mode = a[3].long_or(2);
    let resumepos = a[4].long_or(0);
    with_ftp(vm, f, &obj(&a[0]), |vm, ftp| {
        let Some(typ) = xtype(vm, f, mode) else {
            return Ok(Value::Null);
        };
        let resumepos = seek_for_resume(vm, ftp, stream, resumepos);
        if nb {
            ftp.direction = false;
            ftp.closestream = false;
            let ret = ftp.nb_get(vm, stream, &file, typ, resumepos);
            if ret == PHP_FTP_FAILED {
                warn_inbuf(vm, ftp);
            }
            return Ok(Value::Long(ret));
        }
        if !ftp.get(vm, stream, &file, typ, resumepos) {
            warn_inbuf(vm, ftp);
            return Ok(Value::False);
        }
        Ok(Value::True)
    })
}

/// The local file of ftp_get() / ftp_nb_get(), positioned for a resume.
fn open_local_for_get(
    vm: &mut Vm,
    ftp: &Ftp,
    f: &str,
    local: &[u8],
    mode: i64,
    resumepos: &mut i64,
) -> Option<i64> {
    if !ftp.autoseek && *resumepos == PHP_FTP_AUTORESUME {
        *resumepos = 0;
    }
    let ascii = mode == 1;
    let out = if ftp.autoseek && *resumepos != 0 {
        let s = streams::open(vm, f, local, if ascii { "rt+" } else { "rb+" })
            .or_else(|| streams::open(vm, f, local, if ascii { "wt" } else { "wb" }));
        if let Some(s) = s {
            if *resumepos == PHP_FTP_AUTORESUME {
                streams::seek(vm, s, 0, 2);
                *resumepos = streams::tell(vm, s);
            } else {
                streams::seek(vm, s, *resumepos, 0);
            }
        }
        s
    } else {
        streams::open(vm, f, local, if ascii { "wt" } else { "wb" })
    };
    if out.is_none() {
        vm.emit_warning(&format!(
            "{}(): Error opening {}",
            f,
            String::from_utf8_lossy(local)
        ));
    }
    out
}

fn unlink(path: &[u8]) {
    use std::os::unix::ffi::OsStrExt;
    let _ = std::fs::remove_file(std::ffi::OsStr::from_bytes(path));
}

fn ftp_get(vm: &mut Vm, args: &[Value]) -> R {
    let f = "ftp_get";
    let Some(a) = parse(vm, f, "Opp|ll", args) else {
        return Ok(Value::Null);
    };
    let local = a[1].str().to_vec();
    let remote = a[2].str().to_vec();
    let mode = a[3].long_or(2);
    let mut resumepos = a[4].long_or(0);
    with_ftp(vm, f, &obj(&a[0]), |vm, ftp| {
        let Some(typ) = xtype(vm, f, mode) else {
            return Ok(Value::Null);
        };
        let Some(out) = open_local_for_get(vm, ftp, f, &local, mode, &mut resumepos) else {
            return Ok(Value::False);
        };
        if !ftp.get(vm, out, &remote, typ, resumepos) {
            streams::close(vm, out);
            unlink(&local);
            warn_inbuf(vm, ftp);
            return Ok(Value::False);
        }
        streams::close(vm, out);
        Ok(Value::True)
    })
}

fn ftp_nb_get(vm: &mut Vm, args: &[Value]) -> R {
    let f = "ftp_nb_get";
    let Some(a) = parse(vm, f, "Oss|ll", args) else {
        return Ok(Value::Null);
    };
    let local = a[1].str().to_vec();
    let remote = a[2].str().to_vec();
    let mode = a[3].long_or(2);
    let mut resumepos = a[4].long_or(0);
    with_ftp(vm, f, &obj(&a[0]), |vm, ftp| {
        let Some(typ) = xtype(vm, f, mode) else {
            return Ok(Value::Null);
        };
        let Some(out) = open_local_for_get(vm, ftp, f, &local, mode, &mut resumepos) else {
            return Ok(Value::False);
        };
        ftp.direction = false;
        ftp.closestream = true;
        let ret = ftp.nb_get(vm, out, &remote, typ, resumepos);
        if ret == PHP_FTP_FAILED {
            streams::close(vm, out);
            ftp.stream = None;
            unlink(&local);
            warn_inbuf(vm, ftp);
            return Ok(Value::Long(PHP_FTP_FAILED));
        }
        if ret == PHP_FTP_FINISHED {
            streams::close(vm, out);
            ftp.stream = None;
        }
        Ok(Value::Long(ret))
    })
}

fn ftp_nb_continue(vm: &mut Vm, args: &[Value]) -> R {
    let f = "ftp_nb_continue";
    let Some(a) = parse(vm, f, "O", args) else {
        return Ok(Value::Null);
    };
    with_ftp(vm, f, &obj(&a[0]), |vm, ftp| {
        if !ftp.nb {
            vm.emit_warning(&format!("{}(): No non-blocking transfer to continue", f));
            return Ok(Value::Long(PHP_FTP_FAILED));
        }
        let ret = if ftp.direction {
            ftp.nb_continue_write(vm)
        } else {
            ftp.nb_continue_read(vm)
        };
        if ret != PHP_FTP_MOREDATA && ftp.closestream {
            if let Some(s) = ftp.stream.take() {
                streams::close(vm, s);
            }
        }
        if ret == PHP_FTP_FAILED {
            let t = String::from_utf8_lossy(ftp.inbuf_str()).into_owned();
            vm.emit_warning(&format!("{}(): {}", f, t));
        }
        Ok(Value::Long(ret))
    })
}

/// The resume position handling of the put functions.
fn startpos_for_put(vm: &mut Vm, ftp: &mut Ftp, remote: &[u8], stream: i64, startpos: i64) -> i64 {
    let mut startpos = startpos;
    if !ftp.autoseek && startpos == PHP_FTP_AUTORESUME {
        startpos = 0;
    }
    if ftp.autoseek && startpos != 0 {
        if startpos == PHP_FTP_AUTORESUME {
            startpos = ftp.size(vm, remote);
            if startpos < 0 {
                startpos = 0;
            }
        }
        if startpos != 0 {
            streams::seek(vm, stream, startpos, 0);
        }
    }
    startpos
}

fn ftp_fput(vm: &mut Vm, args: &[Value]) -> R {
    fput_common(vm, args, "ftp_fput", false)
}

fn ftp_nb_fput(vm: &mut Vm, args: &[Value]) -> R {
    fput_common(vm, args, "ftp_nb_fput", true)
}

fn fput_common(vm: &mut Vm, args: &[Value], f: &'static str, nb: bool) -> R {
    let Some(a) = parse(vm, f, "Osr|ll", args) else {
        return Ok(Value::Null);
    };
    let remote = a[1].str().to_vec();
    let Arg::Res(stream) = a[2] else {
        return Ok(Value::Null);
    };
    let mode = a[3].long_or(2);
    let startpos = a[4].long_or(0);
    with_ftp(vm, f, &obj(&a[0]), |vm, ftp| {
        let Some(typ) = xtype(vm, f, mode) else {
            return Ok(Value::Null);
        };
        let startpos = startpos_for_put(vm, ftp, &remote, stream, startpos);
        if nb {
            ftp.direction = true;
            ftp.closestream = false;
            let ret = ftp.nb_put(vm, &remote, stream, typ, startpos);
            if ret == PHP_FTP_FAILED {
                warn_inbuf(vm, ftp);
            }
            return Ok(Value::Long(ret));
        }
        if !ftp.put(vm, &remote, stream, typ, startpos, false) {
            warn_inbuf(vm, ftp);
            return Ok(Value::False);
        }
        Ok(Value::True)
    })
}

fn open_local_for_put(vm: &mut Vm, f: &str, local: &[u8], mode: i64) -> Option<i64> {
    streams::open(vm, f, local, if mode == 1 { "rt" } else { "rb" })
}

fn ftp_put(vm: &mut Vm, args: &[Value]) -> R {
    let f = "ftp_put";
    let Some(a) = parse(vm, f, "Opp|ll", args) else {
        return Ok(Value::Null);
    };
    let remote = a[1].str().to_vec();
    let local = a[2].str().to_vec();
    let mode = a[3].long_or(2);
    let startpos = a[4].long_or(0);
    with_ftp(vm, f, &obj(&a[0]), |vm, ftp| {
        let Some(typ) = xtype(vm, f, mode) else {
            return Ok(Value::Null);
        };
        let Some(instream) = open_local_for_put(vm, f, &local, mode) else {
            return Ok(Value::False);
        };
        let startpos = startpos_for_put(vm, ftp, &remote, instream, startpos);
        if !ftp.put(vm, &remote, instream, typ, startpos, false) {
            streams::close(vm, instream);
            warn_inbuf(vm, ftp);
            return Ok(Value::False);
        }
        streams::close(vm, instream);
        Ok(Value::True)
    })
}

fn ftp_append(vm: &mut Vm, args: &[Value]) -> R {
    let f = "ftp_append";
    let Some(a) = parse(vm, f, "Opp|l", args) else {
        return Ok(Value::Null);
    };
    let remote = a[1].str().to_vec();
    let local = a[2].str().to_vec();
    let mode = a[3].long_or(2);
    with_ftp(vm, f, &obj(&a[0]), |vm, ftp| {
        let Some(typ) = xtype(vm, f, mode) else {
            return Ok(Value::Null);
        };
        let Some(instream) = open_local_for_put(vm, f, &local, mode) else {
            return Ok(Value::False);
        };
        if !ftp.put(vm, &remote, instream, typ, 0, true) {
            streams::close(vm, instream);
            warn_inbuf(vm, ftp);
            return Ok(Value::False);
        }
        streams::close(vm, instream);
        Ok(Value::True)
    })
}

fn ftp_nb_put(vm: &mut Vm, args: &[Value]) -> R {
    let f = "ftp_nb_put";
    let Some(a) = parse(vm, f, "Opp|ll", args) else {
        return Ok(Value::Null);
    };
    let remote = a[1].str().to_vec();
    let local = a[2].str().to_vec();
    let mode = a[3].long_or(2);
    let startpos = a[4].long_or(0);
    with_ftp(vm, f, &obj(&a[0]), |vm, ftp| {
        let Some(typ) = xtype(vm, f, mode) else {
            return Ok(Value::Null);
        };
        let Some(instream) = open_local_for_put(vm, f, &local, mode) else {
            return Ok(Value::False);
        };
        let startpos = startpos_for_put(vm, ftp, &remote, instream, startpos);
        ftp.direction = true;
        ftp.closestream = true;
        let ret = ftp.nb_put(vm, &remote, instream, typ, startpos);
        if ret != PHP_FTP_MOREDATA {
            streams::close(vm, instream);
            ftp.stream = None;
        }
        if ret == PHP_FTP_FAILED {
            let t = String::from_utf8_lossy(ftp.inbuf_str()).into_owned();
            vm.emit_warning(&format!("{}(): {}", f, t));
        }
        Ok(Value::Long(ret))
    })
}

fn ftp_size(vm: &mut Vm, args: &[Value]) -> R {
    let f = "ftp_size";
    let Some(a) = parse(vm, f, "Op", args) else {
        return Ok(Value::Null);
    };
    let file = a[1].str().to_vec();
    with_ftp(vm, f, &obj(&a[0]), |vm, ftp| {
        Ok(Value::Long(ftp.size(vm, &file)))
    })
}

fn ftp_mdtm(vm: &mut Vm, args: &[Value]) -> R {
    let f = "ftp_mdtm";
    let Some(a) = parse(vm, f, "Op", args) else {
        return Ok(Value::Null);
    };
    let file = a[1].str().to_vec();
    with_ftp(vm, f, &obj(&a[0]), |vm, ftp| {
        Ok(Value::Long(ftp.mdtm(vm, &file)))
    })
}

fn ftp_rename(vm: &mut Vm, args: &[Value]) -> R {
    let f = "ftp_rename";
    let Some(a) = parse(vm, f, "Oss", args) else {
        return Ok(Value::Null);
    };
    let (src, dest) = (a[1].str().to_vec(), a[2].str().to_vec());
    with_ftp(vm, f, &obj(&a[0]), |vm, ftp| {
        if !ftp.rename(vm, &src, &dest) {
            warn_inbuf(vm, ftp);
            return Ok(Value::False);
        }
        Ok(Value::True)
    })
}

fn ftp_close(vm: &mut Vm, args: &[Value]) -> R {
    let f = "ftp_close";
    let Some(a) = parse(vm, f, "O", args) else {
        return Ok(Value::Null);
    };
    let o = obj(&a[0]);
    let Some(h) = handle_of(&o) else {
        return Ok(Value::True);
    };
    let taken = h.ftp.borrow_mut().take();
    let mut success = true;
    if let Some(mut ftp) = taken {
        ftp.func = f;
        success = ftp.quit(vm);
        // ftp_close()
        if ftp.closestream {
            if let Some(s) = ftp.stream.take() {
                streams::close(vm, s);
            }
        }
        ftp.close_connections(Some(vm));
    }
    Ok(bval(success))
}

fn ftp_set_option(vm: &mut Vm, args: &[Value]) -> R {
    let f = "ftp_set_option";
    let Some(a) = parse(vm, f, "Olz", args) else {
        return Ok(Value::Null);
    };
    let option = a[1].long_or(0);
    let value = match &a[2] {
        Arg::Any(v) => v.deref(),
        _ => Value::Null,
    };
    with_ftp(vm, f, &obj(&a[0]), |vm, ftp| match option {
        PHP_FTP_OPT_TIMEOUT_SEC => {
            let Value::Long(n) = value else {
                zpp::type_error(
                    vm,
                    f,
                    3,
                    &format!(
                        "must be of type int for the FTP_TIMEOUT_SEC option, {} given",
                        Vm::value_type_name(&value)
                    ),
                );
                return Ok(Value::Null);
            };
            if n <= 0 {
                zpp::value_error(
                    vm,
                    f,
                    3,
                    "must be greater than 0 for the FTP_TIMEOUT_SEC option",
                );
                return Ok(Value::Null);
            }
            ftp.timeout_sec = n;
            Ok(Value::True)
        }
        PHP_FTP_OPT_AUTOSEEK | PHP_FTP_OPT_USEPASVADDRESS => {
            let name = if option == PHP_FTP_OPT_AUTOSEEK {
                "FTP_AUTOSEEK"
            } else {
                "FTP_USEPASVADDRESS"
            };
            let b = match value {
                Value::True => true,
                Value::False => false,
                other => {
                    zpp::type_error(
                        vm,
                        f,
                        3,
                        &format!(
                            "must be of type bool for the {} option, {} given",
                            name,
                            Vm::value_type_name(&other)
                        ),
                    );
                    return Ok(Value::Null);
                }
            };
            if option == PHP_FTP_OPT_AUTOSEEK {
                ftp.autoseek = b;
            } else {
                ftp.usepasvaddress = b;
            }
            Ok(Value::True)
        }
        _ => {
            zpp::value_error(
                vm,
                f,
                2,
                "must be one of FTP_TIMEOUT_SEC, FTP_AUTOSEEK, or FTP_USEPASVADDRESS",
            );
            Ok(Value::Null)
        }
    })
}

fn ftp_get_option(vm: &mut Vm, args: &[Value]) -> R {
    let f = "ftp_get_option";
    let Some(a) = parse(vm, f, "Ol", args) else {
        return Ok(Value::Null);
    };
    let option = a[1].long_or(0);
    with_ftp(vm, f, &obj(&a[0]), |vm, ftp| match option {
        PHP_FTP_OPT_TIMEOUT_SEC => Ok(Value::Long(ftp.timeout_sec)),
        PHP_FTP_OPT_AUTOSEEK => Ok(bval(ftp.autoseek)),
        PHP_FTP_OPT_USEPASVADDRESS => Ok(bval(ftp.usepasvaddress)),
        _ => {
            zpp::value_error(
                vm,
                f,
                2,
                "must be one of FTP_TIMEOUT_SEC, FTP_AUTOSEEK, or FTP_USEPASVADDRESS",
            );
            Ok(Value::Null)
        }
    })
}
