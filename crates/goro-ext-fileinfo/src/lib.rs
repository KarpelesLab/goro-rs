//! php-src's ext/fileinfo on top of a port of libmagic 5.46: the finfo
//! class, finfo_*() and mime_content_type(), with the bundled magic
//! database.

use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;

use goro_core::object::{ClassEntry, NativeData, PhpObject};
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{NativeClassHandler, NativeObjectHandlers, Vm, VmError};
use goro_ext_standard::ext_io;

mod apprentice;
mod cdf;
mod encoding;
mod fmt;
mod funcs;
mod magic;
mod softmagic;
mod zpp;

use magic::*;
use zpp::{Arg, Func, Obj};

/// The extension name (`extension_loaded('fileinfo')`).
pub const NAME: &[u8] = b"fileinfo";

/// MAGIC_VERSION as "5.46"
pub fn libmagic_version() -> String {
    format!("{}.{}", MAGIC_VERSION / 100, MAGIC_VERSION % 100)
}

struct FinfoData {
    ms: RefCell<Option<MagicSet>>,
}

static HANDLERS: NativeObjectHandlers = NativeObjectHandlers {
    read_property: None,
    write_property: None,
    has_property: None,
    unset_property: None,
    read_dimension: None,
    write_dimension: None,
    has_dimension: None,
    unset_dimension: None,
    get_properties: None,
    count_elements: None,
    cast: None,
    compare: None,
    clone_obj: Some(h_clone),
};

fn h_clone(vm: &mut Vm, src: &Obj, _dst: &Obj) {
    let m = format!(
        "Trying to clone an uncloneable object of class {}",
        goro_core::value::display_class_name(&src.borrow().class_name)
    );
    zpp::throw(vm, b"Error", &m);
}

fn data_of(o: &Obj) -> Option<Rc<FinfoData>> {
    let b = o.borrow();
    let n = b.native.as_ref()?;
    n.data.clone().downcast::<FinfoData>().ok()
}

fn attach(o: &Obj) -> Rc<FinfoData> {
    if let Some(d) = data_of(o) {
        return d;
    }
    let d = Rc::new(FinfoData {
        ms: RefCell::new(None),
    });
    o.borrow_mut().native = Some(NativeData {
        data: d.clone() as Rc<dyn Any>,
        handlers: &HANDLERS,
    });
    d
}

fn create_object(_vm: &mut Vm, o: &Obj) {
    attach(o);
}

// ---------------------------------------------------------------------------
// the PHP side of libmagic

struct PhpHost<'a> {
    vm: &'a mut Vm,
    /// "finfo_open" / "finfo::__construct"
    func: String,
}

impl Host for PhpHost<'_> {
    fn warn(&mut self, msg: &[u8]) {
        let line = self.vm.current_line;
        let m = format!("{}(): {}", self.func, String::from_utf8_lossy(msg));
        self.vm.emit_warning_at(&m, line);
    }

    fn read_file(&mut self, path: &[u8]) -> Option<Vec<u8>> {
        // reading a directory fails (silently) after its open
        if let Some((mode, _)) = self.stat(path)
            && mode & S_IFMT == S_IFDIR
        {
            return None;
        }
        let func = self.func.clone();
        let id = as_caller(self.vm, &func, |vm| {
            ext_io::open_as(vm, Some(&func), path, "rb", true)
        })
        .ok()??;
        let data = ext_io::read(self.vm, id, usize::MAX >> 1);
        ext_io::close(self.vm, id);
        Some(data)
    }

    fn stat(&mut self, path: &[u8]) -> Option<(u32, i64)> {
        use std::os::unix::fs::MetadataExt;
        let p = String::from_utf8_lossy(path).into_owned();
        let md = std::fs::metadata(p).ok()?;
        Some((md.mode(), md.size() as i64))
    }

    fn readdir(&mut self, path: &[u8]) -> Option<Vec<Vec<u8>>> {
        use std::os::unix::ffi::OsStrExt;
        let p = String::from_utf8_lossy(path).into_owned();
        let rd = std::fs::read_dir(p).ok()?;
        let mut v: Vec<Vec<u8>> = vec![b".".to_vec(), b"..".to_vec()];
        for e in rd.flatten() {
            v.push(e.file_name().as_bytes().to_vec());
        }
        Some(v)
    }

    fn regex_compile(&mut self, pattern: &[u8]) -> bool {
        goro_ext_standard::regex::get_compiled_regex(self.vm, pattern, &self.func).is_some()
    }

    fn regex_match(
        &mut self,
        pattern: &[u8],
        subject: &[u8],
    ) -> Result<Option<(usize, usize)>, ()> {
        let Some(pce) = goro_ext_standard::regex::get_compiled_regex(self.vm, pattern, &self.func)
        else {
            return Err(());
        };
        let mut md = goro_pcre::MatchData::new(pce.capture_count as usize + 1);
        let mctx = goro_pcre::MatchContext::default();
        let rc = pce.re.match_at(subject, 0, 0, &mut md, &mctx);
        if rc >= 0 {
            let s = md.ovector[0];
            let e = md.ovector[1];
            Ok(Some((s, e.saturating_sub(s))))
        } else if rc == goro_pcre::constants::PCRE2_ERROR_NOMATCH {
            Ok(None)
        } else {
            Err(())
        }
    }
}

// ---------------------------------------------------------------------------
// helpers

/// Run `f` with the diagnostics of the builtins it calls named after the
/// method `func` (native methods have no builtin frame of their own).
fn as_caller<T>(vm: &mut Vm, func: &str, f: impl FnOnce(&mut Vm) -> T) -> T {
    if !func.contains("::") {
        return f(vm);
    }
    let name = func.as_bytes().to_vec();
    let line = vm.current_line;
    vm.push_builtin_frame(&name, &[], line);
    let r = f(vm);
    vm.pop_builtin_frame();
    r
}

fn verr(vm: &Vm) -> VmError {
    VmError {
        message: String::new(),
        line: vm.current_line,
    }
}

fn vstr(b: impl AsRef<[u8]>) -> Value {
    Value::String(PhpString::from_bytes(b.as_ref()))
}

fn obj_of(v: &Value) -> Option<Obj> {
    match v.deref() {
        Value::Object(o) => Some(o),
        _ => None,
    }
}

fn is_finfo(vm: &Vm, o: &Obj) -> bool {
    let cn = o.borrow().class_name.to_ascii_lowercase();
    cn == b"finfo" || vm.is_subclass_of(&cn, b"finfo")
}

/// The finfo object: `this` or the leading "O" argument. Err: thrown.
fn self_obj<'a>(
    vm: &mut Vm,
    f: &Func,
    this: Option<&Obj>,
    args: &'a [Value],
) -> Result<(Obj, &'a [Value]), ()> {
    if let Some(o) = this {
        return Ok((o.clone(), args));
    }
    let v = args.first().map(|v| v.deref()).unwrap_or(Value::Null);
    match obj_of(&v) {
        Some(o) if is_finfo(vm, &o) => Ok((o, &args[1..])),
        _ => {
            f.type_error(vm, 1, "finfo", &v);
            Err(())
        }
    }
}

fn func_for(this: bool, function: &str, method: &str) -> Func {
    if this {
        Func::method("finfo", method)
    } else {
        Func::function(function)
    }
}

/// zpp with the leading "O" counted in the arity for the procedural form
fn parse_args(
    vm: &mut Vm,
    f: &Func,
    this: Option<&Obj>,
    spec: &str,
    args: &[Value],
) -> Result<(Obj, Vec<Arg>), ()> {
    let full = if this.is_none() {
        format!("O{}", spec)
    } else {
        spec.to_string()
    };
    zpp::arity_check(vm, f, &full, args)?;
    let (o, rest) = self_obj(vm, f, this, args)?;
    let off = if this.is_none() { 1 } else { 0 };
    let a = zpp::parse_offset(vm, f, spec, rest, off)?;
    Ok((o, a))
}

fn invalid_object(vm: &mut Vm) {
    zpp::throw(vm, b"Error", "Invalid finfo object");
}

// ---------------------------------------------------------------------------
// finfo_open() / finfo::__construct()

fn finfo_open_impl(vm: &mut Vm, this: Option<&Obj>, args: &[Value]) -> Value {
    let f = func_for(this.is_some(), "finfo_open", "__construct");
    let Ok(a) = zpp::parse(vm, &f, "|lp!", args) else {
        return Value::Null;
    };
    let options = a[0].long_or(MAGIC_NONE as i64);
    let file: Option<Vec<u8>> = a[1].str().map(|s| s.to_vec());
    let saved = if this.is_some() {
        Some(vm.warning_capture.replace(Vec::new()))
    } else {
        None
    };
    if let Some(o) = this {
        let d = attach(o);
        *d.ms.borrow_mut() = None;
    }
    let r: Result<MagicSet, ()> = (|| {
        let file = match file {
            Some(fl) if !fl.is_empty() => {
                let p = String::from_utf8_lossy(&fl).into_owned();
                if !vm.check_open_basedir(&f.name, &p) {
                    return Err(());
                }
                match expand_filepath(&fl) {
                    Some(r) => Some(r.into_bytes()),
                    None => return Err(()),
                }
            }
            _ => None,
        };
        let mut ms = MagicSet::new(options as i32);
        let mut host = PhpHost {
            vm,
            func: f.name.clone(),
        };
        if apprentice::file_apprentice(&mut ms, &mut host, file.as_deref()) == -1 {
            let shown = match &file {
                Some(f) => String::from_utf8_lossy(f).into_owned(),
                None => "(null)".to_string(),
            };
            host.warn(format!("Failed to load magic database at \"{}\"", shown).as_bytes());
            return Err(());
        }
        Ok(ms)
    })();
    if let Some(saved) = saved {
        let captured = std::mem::replace(&mut vm.warning_capture, saved).unwrap_or_default();
        if let Some(first) = captured.first()
            && vm.current_exception.is_none()
        {
            zpp::throw(vm, b"Exception", first);
        }
    }
    match r {
        Ok(ms) => match this {
            Some(o) => {
                *attach(o).ms.borrow_mut() = Some(ms);
                Value::Null
            }
            None => {
                let id = vm.next_object_id();
                let obj = Rc::new(RefCell::new(PhpObject::new(b"finfo".to_vec(), id)));
                *attach(&obj).ms.borrow_mut() = Some(ms);
                Value::Object(obj)
            }
        },
        Err(()) => {
            if this.is_some() && vm.current_exception.is_none() {
                zpp::throw(vm, b"Exception", "Constructor failed");
            }
            Value::False
        }
    }
}

/// expand_filepath_with_mode(CWD_EXPAND)
fn expand_filepath(path: &[u8]) -> Option<String> {
    let p = String::from_utf8_lossy(path).into_owned();
    let full = if p.starts_with('/') {
        p
    } else {
        let cwd = std::env::current_dir().ok()?;
        format!("{}/{}", cwd.to_string_lossy(), p)
    };
    let mut parts: Vec<&str> = Vec::new();
    for seg in full.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    let r = format!("/{}", parts.join("/"));
    if r.len() >= 4096 {
        return None;
    }
    Some(r)
}

// ---------------------------------------------------------------------------
// identification

/// magic_stream(): the description of an open stream
fn magic_stream(vm: &mut Vm, func: &str, ms: &mut MagicSet, id: i64) -> Option<Vec<u8>> {
    if ms.reset(true).is_err() {
        return None;
    }
    let (mode, size) = match ext_io::call(vm, b"fstat", &[Value::Resource(id)]) {
        Ok(Value::Array(a)) => {
            let a = a.borrow();
            let get = |k: &[u8]| match a.get(&goro_core::array::ArrayKey::String(
                PhpString::from_bytes(k),
            )) {
                Some(Value::Long(n)) => *n,
                _ => 0,
            };
            (get(b"mode") as u32, get(b"size"))
        }
        _ => (0, 0),
    };
    let meta = ext_io::meta(vm, id);
    let greedy = meta.wrapper_type == b"plainfile"
        || meta.stream_type == b"MEMORY"
        || meta.stream_type == b"TEMP";
    let data = if greedy {
        ext_io::read(vm, id, ms.bytes_max)
    } else {
        match ext_io::call(
            vm,
            b"fread",
            &[Value::Resource(id), Value::Long(ms.bytes_max as i64)],
        ) {
            Ok(Value::String(s)) => s.as_bytes().to_vec(),
            _ => Vec::new(),
        }
    };
    let fd = if meta.wrapper_type == b"plainfile" {
        meta.uri
            .as_ref()
            .and_then(|u| std::fs::File::open(String::from_utf8_lossy(u).into_owned()).ok())
            .map(|f| Rc::new(RefCell::new(f)))
    } else {
        None
    };
    let b = Buffer::new(fd, mode, size, BufRef::new(data));
    let mut host = PhpHost {
        vm,
        func: func.to_string(),
    };
    if funcs::file_buffer(ms, &mut host, &b) == -1 {
        return None;
    }
    ms.getbuffer()
}

fn identify_warning(vm: &mut Vm, func: &str, ms: &MagicSet) {
    let line = vm.current_line;
    let e = ms
        .error_text()
        .map(|b| String::from_utf8_lossy(&b).into_owned())
        .unwrap_or_else(|| "(null)".to_string());
    vm.emit_warning_at(
        &format!("{}(): Failed identify data {}:{}", func, ms.errno(), e),
        line,
    );
}

/// php_fileinfo_from_path()
fn from_path(
    vm: &mut Vm,
    func: &str,
    ms: &mut MagicSet,
    path: &[u8],
    ctx: Option<i64>,
) -> Option<Vec<u8>> {
    let mut fargs = vec![vstr(path), vstr(b"rb")];
    if let Some(c) = ctx {
        fargs.push(Value::False);
        fargs.push(Value::Resource(c));
    }
    let id = match as_caller(vm, func, |vm| ext_io::call_as(vm, func, b"fopen", &fargs)) {
        Ok(Value::Resource(id)) => id,
        _ => return None,
    };
    let mut ret = None;
    if let Ok(Value::Array(a)) = ext_io::call(vm, b"fstat", &[Value::Resource(id)]) {
        let mode = match a
            .borrow()
            .get(&goro_core::array::ArrayKey::String(PhpString::from_bytes(
                b"mode",
            ))) {
            Some(Value::Long(n)) => *n as u32,
            _ => 0,
        };
        if mode & S_IFDIR != 0 {
            ret = Some(b"directory".to_vec());
        } else {
            ret = magic_stream(vm, func, ms, id);
            if ret.is_none() {
                identify_warning(vm, func, ms);
            }
        }
    }
    ext_io::close(vm, id);
    ret
}

fn context_arg(vm: &mut Vm, f: &Func, v: &Arg) -> Result<Option<i64>, ()> {
    match v {
        Arg::Any(v) => match v.deref() {
            Value::Resource(id) => {
                let t = goro_core::value::resource_type_of(id);
                if goro_core::value::is_open_resource(id)
                    && t.as_deref() == Some(b"stream-context".as_slice())
                {
                    Ok(Some(id))
                } else {
                    let m = format!(
                        "{}(): supplied resource is not a valid Stream-Context resource",
                        f.name
                    );
                    zpp::throw(vm, b"TypeError", &m);
                    Err(())
                }
            }
            _ => Ok(None),
        },
        _ => Ok(None),
    }
}

fn with_ms<T>(vm: &mut Vm, o: &Obj, f: impl FnOnce(&mut Vm, &mut MagicSet) -> T) -> Result<T, ()> {
    let d = attach(o);
    let taken = d.ms.borrow_mut().take();
    let Some(mut ms) = taken else {
        invalid_object(vm);
        return Err(());
    };
    let r = f(vm, &mut ms);
    *d.ms.borrow_mut() = Some(ms);
    Ok(r)
}

fn finfo_file_impl(vm: &mut Vm, this: Option<&Obj>, args: &[Value]) -> Value {
    let f = func_for(this.is_some(), "finfo_file", "file");
    let Ok((o, a)) = parse_args(vm, &f, this, "P|lr!", args) else {
        return Value::Null;
    };
    if attach(&o).ms.borrow().is_none() {
        invalid_object(vm);
        return Value::Null;
    }
    let path = a[0].str_or(b"").to_vec();
    let options = a[1].long_or(0) as i32;
    if path.is_empty() {
        let n = if this.is_some() { 1 } else { 2 };
        f.must_not_be_empty(vm, n);
        return Value::Null;
    }
    let Ok(ctx) = context_arg(vm, &f, &a[2]) else {
        return Value::Null;
    };
    let name = f.name.clone();
    let r = with_ms(vm, &o, |vm, ms| {
        let old = ms.flags;
        if options != 0 {
            ms.flags = options;
        }
        let r = from_path(vm, &name, ms, &path, ctx);
        if options != 0 {
            ms.flags = old;
        }
        r
    });
    match r {
        Ok(Some(s)) => vstr(s),
        Ok(None) => Value::False,
        Err(()) => Value::Null,
    }
}

fn finfo_buffer_impl(vm: &mut Vm, this: Option<&Obj>, args: &[Value]) -> Value {
    let f = func_for(this.is_some(), "finfo_buffer", "buffer");
    let Ok((o, a)) = parse_args(vm, &f, this, "S|lr!", args) else {
        return Value::Null;
    };
    let mut nargs = args.len();
    while nargs > 0 && matches!(args[nargs - 1], Value::Undef) {
        nargs -= 1;
    }
    if nargs == 4 || (this.is_some() && nargs == 3) {
        vm.emit_deprecated(&format!(
            "{}(): The $context parameter has no effect for finfo_buffer()",
            f.name
        ));
        if vm.current_exception.is_some() {
            return Value::Null;
        }
    }
    let buffer = a[0].str_or(b"").to_vec();
    let options = a[1].long_or(0) as i32;
    let name = f.name.clone();
    let r = with_ms(vm, &o, |vm, ms| {
        let old = ms.flags;
        if options != 0 {
            ms.flags = options;
        }
        let b = Buffer::new(None, 0, 0, BufRef::new(buffer));
        let mut host = PhpHost {
            vm,
            func: name.clone(),
        };
        let r = funcs::magic_buffer(ms, &mut host, &b);
        if options != 0 {
            ms.flags = old;
        }
        if r.is_none() {
            identify_warning(vm, &name, ms);
        }
        r
    });
    match r {
        Ok(Some(s)) => vstr(s),
        Ok(None) => Value::False,
        Err(()) => Value::Null,
    }
}

fn finfo_set_flags_impl(vm: &mut Vm, this: Option<&Obj>, args: &[Value]) -> Value {
    let f = func_for(this.is_some(), "finfo_set_flags", "set_flags");
    let Ok((o, a)) = parse_args(vm, &f, this, "l", args) else {
        return Value::Null;
    };
    let options = a[0].long_or(0) as i32;
    match with_ms(vm, &o, |_, ms| ms.flags = options) {
        Ok(()) => Value::True,
        Err(()) => Value::Null,
    }
}

// ---------------------------------------------------------------------------
// functions

fn finfo_open(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let r = finfo_open_impl(vm, None, args);
    done(vm, r)
}

fn done(vm: &Vm, r: Value) -> Result<Value, VmError> {
    if vm.current_exception.is_some() {
        return Err(verr(vm));
    }
    Ok(r)
}

fn finfo_close(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    vm.emit_deprecated(
        "Function finfo_close() is deprecated since 8.5, as finfo objects are freed automatically",
    );
    if vm.current_exception.is_some() {
        return Err(verr(vm));
    }
    let f = Func::function("finfo_close");
    if zpp::arity_check(vm, &f, "O", args).is_err() {
        return Err(verr(vm));
    }
    if self_obj(vm, &f, None, args).is_err() {
        return Err(verr(vm));
    }
    Ok(Value::True)
}

fn finfo_set_flags(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let r = finfo_set_flags_impl(vm, None, args);
    done(vm, r)
}

fn finfo_file(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let r = finfo_file_impl(vm, None, args);
    done(vm, r)
}

fn finfo_buffer(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let r = finfo_buffer_impl(vm, None, args);
    done(vm, r)
}

fn mime_content_type(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let f = Func::function("mime_content_type");
    if zpp::parse(vm, &f, "z", args).is_err() {
        return Err(verr(vm));
    }
    let v = args[0].deref();
    let mut path: Option<Vec<u8>> = None;
    let mut stream: Option<i64> = None;
    match &v {
        Value::String(s) => {
            if s.is_empty() {
                f.must_not_be_empty(vm, 1);
                return Err(verr(vm));
            }
            if s.as_bytes().contains(&0) {
                let m = format!("{} must not contain any null bytes", f.arg_prefix(1));
                zpp::throw(vm, b"TypeError", &m);
                return Err(verr(vm));
            }
            path = Some(s.as_bytes().to_vec());
        }
        Value::Resource(id) => {
            if !ext_io::is_stream(vm, &v) {
                let m = format!(
                    "{}(): supplied resource is not a valid stream resource",
                    f.name
                );
                zpp::throw(vm, b"TypeError", &m);
                return Err(verr(vm));
            }
            stream = Some(*id);
        }
        other => {
            let m = format!(
                "{} must be of type resource|string, {} given",
                f.arg_prefix(1),
                Vm::value_type_name(other)
            );
            zpp::throw(vm, b"TypeError", &m);
            return Err(verr(vm));
        }
    }
    let mut ms = MagicSet::new(MAGIC_MIME_TYPE);
    {
        let mut host = PhpHost {
            vm,
            func: f.name.clone(),
        };
        if apprentice::file_apprentice(&mut ms, &mut host, None) == -1 {
            identify_warning(vm, &f.name, &ms);
            return Ok(Value::False);
        }
    }
    let r = if let Some(p) = path {
        from_path(vm, &f.name, &mut ms, &p, None)
    } else {
        let id = stream.unwrap();
        let pos = ext_io::tell(vm, id);
        ext_io::seek(vm, id, 0, 0);
        let r = magic_stream(vm, &f.name, &mut ms, id);
        if r.is_none() {
            identify_warning(vm, &f.name, &ms);
        }
        ext_io::seek(vm, id, pos, 0);
        r
    };
    if vm.current_exception.is_some() {
        return Err(verr(vm));
    }
    Ok(match r {
        Some(s) => vstr(s),
        None => Value::False,
    })
}

// ---------------------------------------------------------------------------
// methods

const METHODS: &[&str] = &["__construct", "file", "buffer", "set_flags"];

fn has_method(_class: &[u8], method: &[u8], is_static: bool) -> bool {
    !is_static
        && METHODS
            .iter()
            .any(|m| m.as_bytes().eq_ignore_ascii_case(method))
}

fn param_names(_class: &[u8], method: &[u8]) -> Option<&'static [&'static str]> {
    Some(match method.to_ascii_lowercase().as_slice() {
        b"__construct" => &["flags", "magic_database"],
        b"file" => &["filename", "flags", "context"],
        b"buffer" => &["string", "flags", "context"],
        b"set_flags" => &["flags"],
        _ => return None,
    })
}

fn call_method(
    vm: &mut Vm,
    _class: &[u8],
    method: &[u8],
    _called: &[u8],
    this: Option<&Value>,
    args: &[Value],
) -> Value {
    let Some(name) = METHODS
        .iter()
        .find(|m| m.as_bytes().eq_ignore_ascii_case(method))
    else {
        return Value::Null;
    };
    let Some(o) = this.and_then(obj_of) else {
        zpp::throw(
            vm,
            b"Error",
            &format!(
                "Non-static method finfo::{}() cannot be called statically",
                name
            ),
        );
        return Value::Null;
    };
    let frame = format!("finfo->{}", name);
    let has = vm
        .call_stack
        .last()
        .map(|fr| fr.0.eq_ignore_ascii_case(&frame))
        .unwrap_or(false);
    if !has {
        let line = vm.current_line;
        let file = vm.current_file.clone();
        vm.call_stack.push((frame, file, line, args.to_vec(), true));
    }
    let r = match *name {
        "__construct" => finfo_open_impl(vm, Some(&o), args),
        "file" => finfo_file_impl(vm, Some(&o), args),
        "buffer" => finfo_buffer_impl(vm, Some(&o), args),
        _ => finfo_set_flags_impl(vm, Some(&o), args),
    };
    if !has {
        vm.call_stack.pop();
    }
    r
}

// ---------------------------------------------------------------------------
// registration

const CONSTANTS: &[(&str, i64)] = &[
    ("FILEINFO_NONE", MAGIC_NONE as i64),
    ("FILEINFO_SYMLINK", MAGIC_SYMLINK as i64),
    ("FILEINFO_MIME", MAGIC_MIME as i64),
    ("FILEINFO_MIME_TYPE", MAGIC_MIME_TYPE as i64),
    ("FILEINFO_MIME_ENCODING", MAGIC_MIME_ENCODING as i64),
    ("FILEINFO_DEVICES", MAGIC_DEVICES as i64),
    ("FILEINFO_CONTINUE", MAGIC_CONTINUE as i64),
    ("FILEINFO_PRESERVE_ATIME", MAGIC_PRESERVE_ATIME as i64),
    ("FILEINFO_RAW", MAGIC_RAW as i64),
    ("FILEINFO_APPLE", MAGIC_APPLE as i64),
    ("FILEINFO_EXTENSION", MAGIC_EXTENSION as i64),
];

const FUNCTIONS: &[(&str, goro_core::vm::BuiltinFn)] = &[
    ("finfo_open", finfo_open),
    ("finfo_close", finfo_close),
    ("finfo_set_flags", finfo_set_flags),
    ("finfo_file", finfo_file),
    ("finfo_buffer", finfo_buffer),
    ("mime_content_type", mime_content_type),
];

/// MINIT
pub fn register(vm: &mut Vm) {
    if vm.loaded_extensions.contains(NAME) {
        return;
    }
    vm.register_extension(NAME);
    for &(name, f) in FUNCTIONS {
        let params: Vec<&[u8]> = goro_core::stubs::find_function(name.as_bytes())
            .map(|s| s.args.iter().map(|a| a.name.as_bytes()).collect())
            .unwrap_or_default();
        vm.register_function_with_params(name.as_bytes(), f, &params);
    }
    for &(n, v) in CONSTANTS {
        vm.constants.insert(n.as_bytes().to_vec(), Value::Long(v));
    }
    let ce = ClassEntry::new(b"finfo".to_vec());
    vm.classes.insert(b"finfo".to_vec(), ce);
    let h = NativeClassHandler {
        has_method,
        call: call_method,
        prop_added: None,
        prop_write: None,
        compare_check: None,
        prop_read: None,
        param_names: Some(param_names),
    };
    vm.native_class_handlers.insert(b"finfo".to_vec(), h);
    vm.object_create_handlers
        .insert(b"finfo".to_vec(), create_object);
    vm.phpinfo_hooks.push(minfo);
}

/// PHP_MINFO(fileinfo)
fn minfo(_vm: &mut Vm) -> String {
    format!(
        "\nfileinfo\n\nfileinfo support => enabled\nlibmagic => {}\n",
        libmagic_version()
    )
}
