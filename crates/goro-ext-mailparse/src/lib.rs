//! PECL mailparse 3.2: a port of the mailparse extension (mailparse.c,
//! php_mailparse_mime.c, php_mailparse_rfc822.re, mailparse_encoding.c):
//! RFC 822 address parsing, an incremental MIME message parser with part
//! extraction and transfer decoding, uuencoded attachments, and the
//! (undocumented) mimemessage class.

use std::cell::RefCell;
use std::rc::Rc;

use goro_core::array::{ArrayKey, PhpArray};
use goro_core::object::{ClassEntry, NativeData, PhpObject};
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{NativeClassHandler, NativeObjectHandlers, Vm, VmError};

mod encoding;
mod io;
mod mime;
mod rfc822;
mod zpp;

use io::Src;
use zpp::{Arg, Func};

/// The extension name (`extension_loaded('mailparse')`).
pub const NAME: &[u8] = b"mailparse";
/// PHP_MAILPARSE_VERSION
pub const VERSION: &str = "3.2.1-dev";

type R = Result<Value, VmError>;
type Obj = Rc<RefCell<PhpObject>>;

const MAILPARSE_EXTRACT_OUTPUT: i64 = 0;
const MAILPARSE_EXTRACT_STREAM: i64 = 1;
const MAILPARSE_EXTRACT_RETURN: i64 = 2;

/// include the headers and leave the section untouched
const DECODE_NONE: u32 = 0;
/// decode the body into 8-bit
const DECODE_8BIT: u32 = 1;
/// leave the headers out
const DECODE_NOHEADERS: u32 = 2;
/// leave the body out
const DECODE_NOBODY: u32 = 4;

const MAILPARSE_BUFSIZ: usize = 4096;

const CLASS: &[u8] = b"mimemessage";

const FUNCTIONS: &[(&str, goro_core::vm::BuiltinFn)] = &[
    ("mailparse_msg_parse_file", mailparse_msg_parse_file),
    ("mailparse_msg_get_part", mailparse_msg_get_part),
    ("mailparse_msg_get_structure", mailparse_msg_get_structure),
    ("mailparse_msg_get_part_data", mailparse_msg_get_part_data),
    ("mailparse_msg_extract_part", mailparse_msg_extract_part),
    (
        "mailparse_msg_extract_part_file",
        mailparse_msg_extract_part_file,
    ),
    (
        "mailparse_msg_extract_whole_part_file",
        mailparse_msg_extract_whole_part_file,
    ),
    ("mailparse_msg_create", mailparse_msg_create),
    ("mailparse_msg_free", mailparse_msg_free),
    ("mailparse_msg_parse", mailparse_msg_parse),
    (
        "mailparse_rfc822_parse_addresses",
        mailparse_rfc822_parse_addresses,
    ),
    (
        "mailparse_determine_best_xfer_encoding",
        mailparse_determine_best_xfer_encoding,
    ),
    ("mailparse_stream_encode", mailparse_stream_encode),
    ("mailparse_uudecode_all", mailparse_uudecode_all),
    ("mailparse_test", mailparse_test),
];

const CONSTANTS: &[(&str, i64)] = &[
    ("MAILPARSE_EXTRACT_OUTPUT", MAILPARSE_EXTRACT_OUTPUT),
    ("MAILPARSE_EXTRACT_STREAM", MAILPARSE_EXTRACT_STREAM),
    ("MAILPARSE_EXTRACT_RETURN", MAILPARSE_EXTRACT_RETURN),
];

const INI_DEF_CHARSET: &[u8] = b"mailparse.def_charset";

/// MINIT
pub fn register(vm: &mut Vm) {
    if vm.loaded_extensions.contains(NAME) {
        return;
    }
    vm.register_extension(NAME);
    vm.phpinfo_hooks.push(minfo);

    let mut ce = match goro_core::stubs::class_entry(vm, CLASS) {
        Some(stub) => stub.clone(),
        None => ClassEntry::new(CLASS.to_vec()),
    };
    // the methods are served by the native class handler
    ce.methods.clear();
    ce.allow_dynamic_properties = true;
    vm.classes.insert(CLASS.to_vec(), ce);
    vm.native_class_handlers.insert(
        CLASS.to_vec(),
        NativeClassHandler {
            has_method: |_c, m, _static| find_method(m).is_some(),
            call: method_call,
            prop_added: None,
            prop_write: None,
            compare_check: None,
            prop_read: None,
            param_names: None,
        },
    );

    vm.object_create_handlers
        .insert(CLASS.to_vec(), create_object);

    for &(name, f) in FUNCTIONS {
        let params: Vec<&[u8]> = goro_core::stubs::find_function(name.as_bytes())
            .map(|s| s.args.iter().map(|a| a.name.as_bytes()).collect())
            .unwrap_or_default();
        vm.register_function_with_params(name.as_bytes(), f, &params);
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
    // PHP_INI_ALL
    vm.constants
        .entry(INI_DEF_CHARSET.to_vec())
        .or_insert_with(|| Value::String(PhpString::from_bytes(b"us-ascii")));
}

/// PHP_MINFO(mailparse)
fn minfo(vm: &mut Vm) -> String {
    let cs = String::from_utf8_lossy(&mime::def_charset(vm)).into_owned();
    let cs = if cs.is_empty() {
        "no value".to_string()
    } else {
        cs
    };
    format!(
        "\nmailparse\n\nmailparse support => enabled\nExtension Version => {}\n\nDirective => Local Value => Master Value\nmailparse.def_charset => {} => {}\n",
        VERSION, cs, cs
    )
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn err(vm: &Vm) -> VmError {
    VmError {
        message: String::new(),
        line: vm.current_line,
    }
}

fn parse_args(vm: &mut Vm, f: &Func, spec: &str, args: &[Value]) -> Result<Vec<Arg>, VmError> {
    zpp::parse(vm, f, spec, args).map_err(|_| err(vm))
}

fn str_val(s: &[u8]) -> Value {
    Value::String(PhpString::from_bytes(s))
}

fn arr_val(a: PhpArray) -> Value {
    Value::Array(Rc::new(RefCell::new(a)))
}

fn set(a: &mut PhpArray, k: &str, v: Value) {
    a.set(ArrayKey::String(PhpString::from_bytes(k.as_bytes())), v);
}

fn bool_val(b: bool) -> Value {
    if b { Value::True } else { Value::False }
}

fn cstr(v: &[u8]) -> &[u8] {
    match v.iter().position(|&b| b == 0) {
        Some(n) => &v[..n],
        None => v,
    }
}

/// The C code opens a stream (a memory stream on a string, a temporary
/// file...) where goro needs none: it takes a resource id all the same, so
/// that the ids of the resources PHP code sees are the same.
fn stream_resource_id() {
    goro_core::value::alloc_resource_id();
}

/// A warning with the function's name (php_error_docref()).
fn warn(vm: &mut Vm, fname: &str, msg: &str) {
    vm.emit_warning(&format!("{}(): {}", fname, msg));
}

/// zend_fetch_resource() of a mime part.
fn fetch_part(vm: &mut Vm, f: &Func, v: &Value) -> Result<i64, VmError> {
    match mime::part_id(v) {
        Some(id) => Ok(id),
        None => {
            let m = format!(
                "{}(): supplied resource is not a valid mailparse_mail_structure resource",
                f.name
            );
            zpp::throw(vm, b"TypeError", &m);
            Err(err(vm))
        }
    }
}

/// php_stream_from_zval(): a stream resource argument.
fn fetch_stream(vm: &mut Vm, fname: &str, v: &Value) -> Result<Value, VmError> {
    let v = v.deref();
    if io::is_stream(&v) {
        return Ok(v);
    }
    let what = if matches!(v, Value::Resource(_)) {
        "resource"
    } else {
        "argument"
    };
    let m = format!(
        "{}(): supplied {} is not a valid stream resource",
        fname, what
    );
    zpp::throw(vm, b"TypeError", &m);
    Err(err(vm))
}

/// Process the pending end of the message before the tree is inspected.
fn finalize(vm: &mut Vm, fname: &str, id: i64) {
    let def = mime::def_charset(vm);
    for w in mime::finalize(id, &def) {
        warn(vm, fname, w);
    }
}

fn parse_data(vm: &mut Vm, fname: &str, id: i64, data: &[u8]) -> bool {
    let def = mime::def_charset(vm);
    match mime::parse(id, &def, data) {
        Ok(()) => true,
        Err(e) => {
            warn(vm, fname, e.message());
            false
        }
    }
}

// ---------------------------------------------------------------------------
// Extraction
// ---------------------------------------------------------------------------

/// Where extracted data goes (php_mimepart_extract_func_t).
enum Dest {
    /// the output (ZEND_WRITE)
    Output,
    Stream(Value),
    Buffer(Vec<u8>),
    /// a user function
    User(Value),
}

impl Dest {
    fn emit(&mut self, vm: &mut Vm, fname: &str, data: &[u8]) -> Result<(), VmError> {
        if data.is_empty() {
            return Ok(());
        }
        match self {
            Dest::Output => vm.write_output(data),
            Dest::Stream(s) => io::write(vm, s, data)?,
            Dest::Buffer(b) => b.extend_from_slice(data),
            Dest::User(cb) => {
                if !vm.zend_is_callable_ex(cb, 0).ok {
                    warn(vm, fname, "unable to call user function");
                    return Ok(());
                }
                // zend_call_function() does not call with an exception
                // pending
                if vm.current_exception.is_some() {
                    return Ok(());
                }
                let cb = cb.clone();
                let line = vm.current_line;
                let r = vm.call_callback(&cb, &[str_val(data)]);
                vm.current_line = line;
                if r.is_err() && vm.current_exception.is_none() {
                    warn(vm, fname, "unable to call user function");
                }
            }
        }
        Ok(())
    }
}

/// extract_part(): copy a section of the message from `src` to `dest`,
/// decoding its transfer encoding with DECODE_8BIT. False on failure.
fn extract_part(
    vm: &mut Vm,
    fname: &str,
    id: i64,
    decode: u32,
    src: &mut Src,
    dest: &mut Dest,
) -> Result<bool, VmError> {
    let Some((startpos, bodystart, end_any, cte)) = mime::get(id).map(|p| {
        let p = p.borrow();
        let end = if p.parent.is_some() {
            p.bodyend
        } else {
            p.endpos
        };
        (
            p.startpos,
            p.bodystart,
            end,
            p.content_transfer_encoding.clone(),
        )
    }) else {
        return Ok(false);
    };
    let mut start = if decode & DECODE_NOHEADERS != 0 {
        bodystart
    } else {
        startpos
    };
    let end = if decode & DECODE_NOBODY != 0 {
        bodystart
    } else {
        end_any
    };

    // php_mimepart_decoder_prepare()
    let mut filter = None;
    if decode & DECODE_8BIT != 0 {
        if let Some(cte) = &cte {
            match encoding::name2encoding(cstr(cte)) {
                Some(enc) => filter = encoding::Decoder::new(enc),
                None => {
                    if !cstr(cte).eq_ignore_ascii_case(b"binary") {
                        let m = format!(
                            "{}(): unknown transfer encoding {}!",
                            active_name(fname),
                            String::from_utf8_lossy(cstr(cte))
                        );
                        vm.emit_warning(&m);
                    }
                }
            }
        }
    }
    let mut workbuf: Vec<u8> = Vec::new();

    let mut ok = true;
    if !src.seek(vm, start)? {
        let m = format!("{}(): unable to seek to section start", active_name(fname));
        vm.emit_warning(&m);
        ok = false;
    } else {
        while start < end {
            let n = ((MAILPARSE_BUFSIZ - 1) as i64).min(end - start) as usize;
            let buf = src.read(vm, n)?;
            if buf.is_empty() {
                let m = format!(
                    "{}(): error reading from file at offset{}",
                    active_name(fname),
                    start
                );
                vm.emit_warning(&m);
                ok = false;
                break;
            }
            // php_mimepart_decoder_feed()
            match filter.as_mut() {
                Some(d) => {
                    d.feed(&buf, &mut workbuf);
                    if workbuf.len() >= MAILPARSE_BUFSIZ {
                        dest.emit(vm, fname, &workbuf)?;
                        workbuf.clear();
                    }
                }
                None => dest.emit(vm, fname, &buf)?,
            }
            start += buf.len() as i64;
        }
    }
    // php_mimepart_decoder_finish()
    if let Some(d) = filter.as_mut() {
        d.flush(&mut workbuf);
    }
    if !workbuf.is_empty() {
        dest.emit(vm, fname, &workbuf)?;
    }
    Ok(ok)
}

/// get_active_function_name(): the method name without its class.
fn active_name(fname: &str) -> &str {
    match fname.rfind("::") {
        Some(i) => &fname[i + 2..],
        None => fname,
    }
}

/// mailparse_do_extract()
fn do_extract(vm: &mut Vm, args: &[Value], name: &str, decode: u32, isfile: bool) -> R {
    let f = Func::function(name);
    let a = parse_args(vm, &f, "rz|z", args)?;
    let Arg::Any(zpart) = &a[0] else {
        return Ok(Value::False);
    };
    let id = fetch_part(vm, &f, zpart)?;
    let filename = match &a[1] {
        Arg::Any(v) => v.deref(),
        _ => Value::Null,
    };

    // filename can be a filename or a stream
    let mut close_src = None;
    let mut src = if matches!(filename, Value::Resource(_)) {
        Src::Stream(fetch_stream(vm, name, &filename)?)
    } else if isfile {
        let path = filename.to_php_string();
        match io::open(vm, name, &path)? {
            Some(s) => {
                close_src = Some(s.clone());
                Src::Stream(s)
            }
            None => return Ok(Value::False),
        }
    } else {
        // the message data itself
        stream_resource_id();
        Src::bytes(filename.to_php_string())
    };

    let mut dest = match &a.get(2) {
        Some(Arg::Any(cb)) => match cb.deref() {
            Value::Null => {
                stream_resource_id();
                Dest::Buffer(Vec::new())
            }
            v @ Value::Resource(_) => Dest::Stream(fetch_stream(vm, name, &v)?),
            v => Dest::User(v),
        },
        _ => Dest::Output,
    };

    finalize(vm, name, id);
    let r = extract_part(vm, name, id, decode, &mut src, &mut dest);
    if let Some(s) = close_src {
        io::close(vm, &s);
    }
    Ok(if r? {
        match dest {
            Dest::Buffer(b) => str_val(&b),
            _ => Value::True,
        }
    } else {
        Value::False
    })
}

// ---------------------------------------------------------------------------
// uuencode
// ---------------------------------------------------------------------------

/// mailparse_do_uudecode(): decode the lines after a "begin" line from
/// `src` into `out` (or just measure them); the decoded size.
fn do_uudecode(
    vm: &mut Vm,
    src: &mut Src,
    mut out: Option<&mut Vec<u8>>,
) -> Result<usize, VmError> {
    fn dec(c: u8) -> i32 {
        ((c as i32) - (b' ' as i32)) & 0o77
    }
    let mut file_size = 0usize;
    let mut backtick_line = false;
    'lines: while !src.eof(vm)? {
        let Some(line) = src.gets(vm, 128)? else {
            break;
        };
        let at = |i: usize| line.get(i).copied().unwrap_or(0);
        if out.is_some() {
            if backtick_line && line.starts_with(b"end") && matches!(at(3), b'\r' | b'\n') {
                break;
            }
            backtick_line = at(0) == b'`' && matches!(at(1), b'\r' | b'\n');
        }
        let mut x = 0;
        // UU_NEXT(v): the next character, or leave the loop at the end of
        // the line
        macro_rules! uu_next {
            ($label:lifetime) => {{
                let c = at(x);
                if c == 0 || c == b'\r' || c == b'\n' {
                    break $label;
                }
                x += 1;
                dec(c)
            }};
        }
        let mut n = uu_next!('lines);
        match out.as_deref_mut() {
            Some(o) => {
                'group: while n != 0 {
                    let a = uu_next!('group);
                    let b = uu_next!('group);
                    let c = uu_next!('group);
                    let d = uu_next!('group);
                    if n > 0 {
                        file_size += 1;
                        o.push(((a << 2) | (b >> 4)) as u8);
                    }
                    n -= 1;
                    if n > 0 {
                        file_size += 1;
                        o.push(((b << 4) | (c >> 2)) as u8);
                    }
                    n -= 1;
                    if n > 0 {
                        file_size += 1;
                        o.push(((c << 6) | d) as u8);
                    }
                    n -= 1;
                }
            }
            None => {
                'group: while at(x) != 0 && n != 0 {
                    let _ = uu_next!('group);
                    let _ = uu_next!('group);
                    let _ = uu_next!('group);
                    let _ = uu_next!('group);
                    for _ in 0..3 {
                        if n > 0 {
                            file_size += 1;
                        }
                        n -= 1;
                    }
                }
            }
        }
    }
    Ok(file_size)
}

/// The file name of a "begin <mode> <name>" line: from offset 10, without
/// trailing white space.
fn begin_filename(line: &[u8]) -> Vec<u8> {
    let line = cstr(line);
    let mut name = &line[line.len().min(10)..];
    while let [rest @ .., last] = name {
        if matches!(*last, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r') {
            name = rest;
        } else {
            break;
        }
    }
    name.to_vec()
}

/// php_open_temporary_fd(NULL, "mailparse"): a new file in the temporary
/// directory, kept after use.
fn temp_file(vm: &mut Vm) -> Option<(std::fs::File, String)> {
    let dir = match io::call(vm, b"sys_get_temp_dir", &[]) {
        Ok(Value::String(s)) => s.to_string_lossy(),
        _ => "/tmp".to_string(),
    };
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    for _ in 0..100 {
        let mut r = [0u8; 6];
        goro_core::random::csprng_bytes(&mut r);
        let suffix: String = r
            .iter()
            .map(|b| CHARS[*b as usize % CHARS.len()] as char)
            .collect();
        let path = format!("{}/mailparse{}", dir.trim_end_matches('/'), suffix);
        let mut opts = std::fs::OpenOptions::new();
        opts.read(true).write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        match opts.open(&path) {
            Ok(f) => {
                stream_resource_id();
                return Some((f, path));
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(_) => return None,
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Functions
// ---------------------------------------------------------------------------

fn mailparse_msg_create(_vm: &mut Vm, _args: &[Value]) -> R {
    Ok(Value::resource(mime::alloc()))
}

fn mailparse_msg_free(vm: &mut Vm, args: &[Value]) -> R {
    let f = Func::function("mailparse_msg_free");
    let a = parse_args(vm, &f, "r", args)?;
    if let Arg::Any(v) = &a[0] {
        if let Some(id) = mime::part_id(v) {
            mime::close(id);
        } else if io::is_stream(v) {
            // zend_list_close() of whatever resource it is
            io::close(vm, &v.deref());
        }
    }
    Ok(Value::True)
}

fn mailparse_msg_parse(vm: &mut Vm, args: &[Value]) -> R {
    let f = Func::function("mailparse_msg_parse");
    let a = parse_args(vm, &f, "rS", args)?;
    let Arg::Any(v) = &a[0] else {
        return Ok(Value::False);
    };
    let id = fetch_part(vm, &f, v)?;
    let data = a[1].str_or(b"").to_vec();
    Ok(bool_val(parse_data(vm, "mailparse_msg_parse", id, &data)))
}

fn mailparse_msg_parse_file(vm: &mut Vm, args: &[Value]) -> R {
    let name = "mailparse_msg_parse_file";
    let f = Func::function(name);
    let a = parse_args(vm, &f, "S", args)?;
    let path = PhpString::from_bytes(a[0].str_or(b""));
    let Some(stream) = io::open(vm, name, &path)? else {
        return Ok(Value::False);
    };
    let id = mime::alloc();
    let ret = Value::resource(id);
    let mut src = Src::Stream(stream.clone());
    let mut result = Ok(ret);
    loop {
        match src.eof(vm) {
            Ok(false) => {}
            Ok(true) => break,
            Err(e) => {
                result = Err(e);
                break;
            }
        }
        match src.read(vm, MAILPARSE_BUFSIZ) {
            Ok(buf) if buf.is_empty() => break,
            Ok(buf) => {
                if !parse_data(vm, name, id, &buf) {
                    // the part is destroyed, not returned
                    mime::close(id);
                    result = Ok(Value::False);
                    break;
                }
            }
            Err(e) => {
                result = Err(e);
                break;
            }
        }
    }
    io::close(vm, &stream);
    result
}

fn mailparse_msg_get_structure(vm: &mut Vm, args: &[Value]) -> R {
    let name = "mailparse_msg_get_structure";
    let f = Func::function(name);
    let a = parse_args(vm, &f, "r", args)?;
    let Arg::Any(v) = &a[0] else {
        return Ok(Value::False);
    };
    let id = fetch_part(vm, &f, v)?;
    finalize(vm, name, id);
    let mut out = PhpArray::new();
    for (_, path) in mime::enum_parts(id) {
        let s: Vec<String> = path.iter().map(|n| (*n as i32).to_string()).collect();
        out.push(str_val(s.join(".").as_bytes()));
    }
    Ok(arr_val(out))
}

fn mailparse_msg_get_part(vm: &mut Vm, args: &[Value]) -> R {
    let name = "mailparse_msg_get_part";
    let f = Func::function(name);
    let a = parse_args(vm, &f, "rS", args)?;
    let Arg::Any(v) = &a[0] else {
        return Ok(Value::False);
    };
    let id = fetch_part(vm, &f, v)?;
    let section = a[1].str_or(b"").to_vec();
    finalize(vm, name, id);
    match mime::find_by_name(id, &section) {
        Some(found) => Ok(Value::resource(found)),
        None => {
            let m = format!(
                "cannot find section {} in message",
                String::from_utf8_lossy(cstr(&section))
            );
            warn(vm, name, &m);
            Ok(Value::False)
        }
    }
}

fn mailparse_msg_get_part_data(vm: &mut Vm, args: &[Value]) -> R {
    let name = "mailparse_msg_get_part_data";
    let f = Func::function(name);
    let a = parse_args(vm, &f, "r", args)?;
    let Arg::Any(v) = &a[0] else {
        return Ok(Value::False);
    };
    let id = fetch_part(vm, &f, v)?;
    finalize(vm, name, id);
    Ok(arr_val(mime::part_data(vm, id)))
}

fn mailparse_msg_extract_part(vm: &mut Vm, args: &[Value]) -> R {
    do_extract(
        vm,
        args,
        "mailparse_msg_extract_part",
        DECODE_8BIT | DECODE_NOHEADERS,
        false,
    )
}

fn mailparse_msg_extract_part_file(vm: &mut Vm, args: &[Value]) -> R {
    do_extract(
        vm,
        args,
        "mailparse_msg_extract_part_file",
        DECODE_8BIT | DECODE_NOHEADERS,
        true,
    )
}

fn mailparse_msg_extract_whole_part_file(vm: &mut Vm, args: &[Value]) -> R {
    do_extract(
        vm,
        args,
        "mailparse_msg_extract_whole_part_file",
        DECODE_NONE,
        true,
    )
}

fn mailparse_rfc822_parse_addresses(vm: &mut Vm, args: &[Value]) -> R {
    let f = Func::function("mailparse_rfc822_parse_addresses");
    let a = parse_args(vm, &f, "S", args)?;
    let (toks, warnings) = rfc822::tokenize(a[0].str_or(b""), true);
    for w in warnings {
        vm.emit_warning(&format!("input is not rfc822 compliant: {}", w));
    }
    let mut out = PhpArray::new();
    for addr in rfc822::parse_addresses(&toks) {
        let mut item = PhpArray::new();
        if let Some(n) = &addr.name {
            set(&mut item, "display", str_val(n));
        }
        if let Some(ad) = &addr.address {
            set(&mut item, "address", str_val(ad));
        }
        set(&mut item, "is_group", bool_val(addr.is_group));
        out.push(arr_val(item));
    }
    Ok(arr_val(out))
}

fn mailparse_determine_best_xfer_encoding(vm: &mut Vm, args: &[Value]) -> R {
    let name = "mailparse_determine_best_xfer_encoding";
    let f = Func::function(name);
    let a = parse_args(vm, &f, "r", args)?;
    let Arg::Any(v) = &a[0] else {
        return Ok(Value::False);
    };
    let stream = fetch_stream(vm, name, v)?;
    let mut src = Src::Stream(stream);
    src.rewind(vm)?;
    let mut bestenc = encoding::Enc::SevenBit;
    let mut longline = false;
    let mut linelen = 0;
    'read: while !src.eof(vm)? {
        let buf = src.read(vm, 8192)?;
        if buf.is_empty() {
            break;
        }
        for c in buf {
            if c > 0x80 {
                bestenc = encoding::Enc::EightBit;
            } else if c == 0 {
                bestenc = encoding::Enc::Base64;
                longline = false;
                break 'read;
            }
            if c == b'\n' {
                linelen = 0;
            } else {
                linelen += 1;
                if linelen > 200 {
                    longline = true;
                }
            }
        }
    }
    if longline {
        bestenc = encoding::Enc::Qprint;
    }
    src.rewind(vm)?;
    let name: &[u8] = match bestenc {
        encoding::Enc::SevenBit => b"7bit",
        encoding::Enc::EightBit => b"8bit",
        encoding::Enc::Base64 => b"BASE64",
        encoding::Enc::Qprint => b"quoted-printable",
    };
    Ok(str_val(name))
}

fn mailparse_stream_encode(vm: &mut Vm, args: &[Value]) -> R {
    let name = "mailparse_stream_encode";
    let f = Func::function(name);
    let a = parse_args(vm, &f, "rrS", args)?;
    let (Arg::Any(s), Arg::Any(d)) = (&a[0], &a[1]) else {
        return Ok(Value::False);
    };
    let src = fetch_stream(vm, name, s)?;
    let dest = fetch_stream(vm, name, d)?;
    let encname = a[2].str_or(b"").to_vec();
    let Some(enc) = encoding::name2encoding(cstr(&encname)) else {
        let m = format!(
            "{}(): unknown encoding \"{}\"",
            name,
            String::from_utf8_lossy(cstr(&encname))
        );
        vm.emit_warning(&m);
        return Ok(Value::False);
    };
    let mut src = Src::Stream(src);
    let mut conv = encoding::Encoder::new(enc);
    let mut out: Vec<u8> = Vec::new();
    const BUFSIZE: usize = 2048;
    if enc == encoding::Enc::Qprint {
        // lines starting "From " get their F encoded, so that MTAs do not
        // prefix them with ">" (which breaks signatures)
        while !src.eof(vm)? {
            // (nothing read before the end: stop rather than spin)
            let Some(line) = src.gets(vm, BUFSIZE)? else {
                break;
            };
            let line = cstr(&line);
            let mut i = 0;
            if line.starts_with(b"From ") {
                conv.flush(&mut out);
                out.extend_from_slice(b"=46rom ");
                i = 5;
            }
            for &c in &line[i..] {
                conv.feed(c, &mut out);
            }
            if out.len() >= 8192 {
                io::write(vm, &dest, &out)?;
                out.clear();
            }
        }
    } else {
        while !src.eof(vm)? {
            let buf = src.read(vm, BUFSIZE)?;
            if buf.is_empty() {
                break;
            }
            for c in buf {
                conv.feed(c, &mut out);
            }
            if out.len() >= 8192 {
                io::write(vm, &dest, &out)?;
                out.clear();
            }
        }
    }
    conv.flush(&mut out);
    io::write(vm, &dest, &out)?;
    let _ = io::call(vm, b"fflush", std::slice::from_ref(&dest));
    Ok(Value::True)
}

fn mailparse_uudecode_all(vm: &mut Vm, args: &[Value]) -> R {
    let name = "mailparse_uudecode_all";
    let f = Func::function(name);
    let a = parse_args(vm, &f, "r", args)?;
    let Arg::Any(v) = &a[0] else {
        return Ok(Value::False);
    };
    let stream = fetch_stream(vm, name, v)?;
    let Some((mut outfile, outpath)) = temp_file(vm) else {
        vm.emit_warning(&format!("{}(): unable to open temp file", name));
        return Ok(Value::False);
    };
    use std::io::Write;
    let mut src = Src::Stream(stream);
    src.rewind(vm)?;
    let mut ret = PhpArray::new();
    let mut nparts = 0;
    let mut seen_begin = false;
    while let Some(line) = src.gets(vm, 4096)? {
        if line.starts_with(b"begin ") {
            let origfilename = begin_filename(&line);
            if !seen_begin {
                seen_begin = true;
                // the file with the uuencoded parts removed
                let mut item = PhpArray::new();
                set(&mut item, "filename", str_val(outpath.as_bytes()));
                ret.push(arr_val(item));
            }
            let mut item = PhpArray::new();
            set(&mut item, "origfilename", str_val(&origfilename));
            if let Some((mut partfile, partpath)) = temp_file(vm) {
                nparts += 1;
                set(&mut item, "filename", str_val(partpath.as_bytes()));
                ret.push(arr_val(item));
                let mut data = Vec::new();
                do_uudecode(vm, &mut src, Some(&mut data))?;
                let _ = partfile.write_all(&data);
            }
        } else {
            // copied to the output file (as a C string)
            let _ = outfile.write_all(cstr(&line));
        }
    }
    drop(outfile);
    src.rewind(vm)?;
    if nparts == 0 {
        return Ok(Value::False);
    }
    Ok(arr_val(ret))
}

fn mailparse_test(vm: &mut Vm, args: &[Value]) -> R {
    let f = Func::function("mailparse_test");
    let a = parse_args(vm, &f, "s", args)?;
    let (toks, warnings) = rfc822::tokenize(a[0].str_or(b""), true);
    for w in warnings {
        vm.emit_warning(&format!("input is not rfc822 compliant: {}", w));
    }
    // php_rfc822_print_tokens()
    let mut out = Vec::new();
    for i in 0..toks.len() {
        let t = toks.tokens[i];
        out.extend_from_slice(format!("token {}:  token={}/", i, t.token).as_bytes());
        out.push(t.token);
        out.extend_from_slice(format!(" len={} value=", t.valuelen).as_bytes());
        out.extend_from_slice(toks.value_tail(i));
        out.push(b'\n');
    }
    // php_rfc822_print_addresses()
    let addrs = rfc822::parse_addresses(&toks);
    out.extend_from_slice(b"printing addresses 0x1\n");
    for (i, ad) in addrs.iter().enumerate() {
        out.extend_from_slice(format!("addr {}: name=", i).as_bytes());
        out.extend_from_slice(ad.name.as_deref().unwrap_or(b"(null)"));
        out.extend_from_slice(b" address=");
        out.extend_from_slice(ad.address.as_deref().unwrap_or(b"(null)"));
        out.push(b'\n');
    }
    vm.write_output(&out);
    Ok(Value::Null)
}

// ---------------------------------------------------------------------------
// The mimemessage class
// ---------------------------------------------------------------------------

type Method = fn(&mut Vm, &Obj, &[Value]) -> R;

const METHODS: &[(&str, Method)] = &[
    ("__construct", mm_construct),
    ("get_child", mm_get_child),
    ("get_child_count", mm_get_child_count),
    ("get_parent", mm_get_parent),
    ("extract_headers", mm_extract_headers),
    ("extract_body", mm_extract_body),
    ("enum_uue", mm_enum_uue),
    ("extract_uue", mm_extract_uue),
    ("remove", mm_remove),
    ("add_child", mm_add_child),
];

fn find_method(m: &[u8]) -> Option<&'static (&'static str, Method)> {
    METHODS
        .iter()
        .find(|(n, _)| n.as_bytes().eq_ignore_ascii_case(m))
}

fn method_call(
    vm: &mut Vm,
    _class: &[u8],
    method: &[u8],
    _called: &[u8],
    this: Option<&Value>,
    args: &[Value],
) -> Value {
    let Some(&(name, imp)) = find_method(method) else {
        return Value::Null;
    };
    let Some(Value::Object(o)) = this.map(|t| t.deref()) else {
        zpp::throw(
            vm,
            b"Error",
            &format!(
                "Non-static method mimemessage::{}() cannot be called statically",
                name
            ),
        );
        return Value::Null;
    };
    match imp(vm, &o, args) {
        Ok(v) => v,
        Err(e) => {
            if vm.current_exception.is_none() && !e.message.is_empty() {
                zpp::throw(vm, b"Error", &e.message);
            }
            Value::Null
        }
    }
}

fn mfunc(method: &str) -> Func {
    let mut f = Func::method("mimemessage", method);
    f.name = format!("mimemessage::{}", method);
    f
}

/// The key of the part resource in the object's properties (index 0).
const PART_PROP: &[u8] = b"0";

static MM_HANDLERS: NativeObjectHandlers = NativeObjectHandlers {
    get_properties: Some(mm_properties),
    ..NativeObjectHandlers::EMPTY
};

fn create_object(_vm: &mut Vm, o: &Obj) {
    o.borrow_mut().native = Some(NativeData {
        data: Rc::new(()),
        handlers: &MM_HANDLERS,
    });
}

/// The property table var_dump() & co. show: the part resource is under
/// the integer key 0, as the C code inserts it.
fn mm_properties(_vm: &mut Vm, o: &Obj, purpose: u8) -> Option<PhpArray> {
    if purpose != 0 {
        return None;
    }
    let mut a = PhpArray::new();
    for (k, v) in &o.borrow().properties {
        let key = if k.as_slice() == PART_PROP {
            ArrayKey::Int(0)
        } else {
            ArrayKey::String(PhpString::from_bytes(k))
        };
        a.set(key, v.clone());
    }
    Some(a)
}

/// mimemsg_get_object()
fn object_part(o: &Obj) -> Option<i64> {
    let v = o.borrow().get_property(PART_PROP);
    mime::part_id(&v)
}

/// mailparse_mimemessage_populate(): the "data" property.
fn populate(vm: &mut Vm, fname: &str, o: &Obj, id: i64) {
    finalize(vm, fname, id);
    let data = mime::part_data(vm, id);
    o.borrow_mut().set_property(b"data".to_vec(), arr_val(data));
}

/// mailparse_mimemessage_export(): a new mimemessage object for a part.
fn export(vm: &mut Vm, fname: &str, id: i64) -> Value {
    let o = vm.spl_instantiate(CLASS);
    vm.run_object_create_hook(&o);
    o.borrow_mut()
        .set_property(PART_PROP.to_vec(), Value::resource(id));
    populate(vm, fname, &o, id);
    Value::Object(o)
}

/// The message source of a part as a readable stream.
fn part_source(vm: &mut Vm, fname: &str, id: i64) -> Result<Src, VmError> {
    let source = mime::get(id)
        .map(|p| p.borrow().source.clone())
        .unwrap_or_default();
    match source {
        mime::Source::Str(s) => {
            stream_resource_id();
            Ok(Src::bytes(s))
        }
        mime::Source::Stream(s) => Ok(Src::Stream(fetch_stream(vm, fname, &s)?)),
        mime::Source::None => Ok(Src::Stream(fetch_stream(vm, fname, &Value::Null)?)),
    }
}

fn mm_construct(vm: &mut Vm, o: &Obj, args: &[Value]) -> R {
    let fname = "mimemessage::__construct";
    let f = mfunc("__construct");
    let a = match zpp::parse(vm, &f, "Sz!", args) {
        Ok(a) => a,
        Err(()) => return Err(err(vm)),
    };
    let mode = a[0].str_or(b"").to_vec();
    let source = match &a[1] {
        Arg::Any(v) => Some(v.deref()),
        _ => None,
    };

    // the mime part of the object
    let id = mime::alloc();
    o.borrow_mut()
        .set_property(PART_PROP.to_vec(), Value::resource(id));

    if mode == b"new" {
        return Ok(Value::True);
    }
    let Some(source) = source else {
        return Ok(Value::False);
    };

    let src = match (mode.as_slice(), &source) {
        // the source is the message
        (b"var", Value::String(s)) => Some(mime::Source::Str(s.clone())),
        // the source is the name of a file
        (b"file", _) => {
            let path = source.to_php_string();
            match io::open(vm, fname, &path)? {
                Some(s) => Some(mime::Source::Stream(s)),
                None => return Ok(Value::False),
            }
        }
        (b"stream", _) => Some(mime::Source::Stream(source.clone())),
        _ => None,
    };
    if let Some(src) = &src {
        if let Some(p) = mime::get(id) {
            p.borrow_mut().source = src.clone();
        }
    }

    // parse the data from the source
    match src {
        Some(mime::Source::Str(s)) => {
            parse_data(vm, fname, id, s.as_bytes());
        }
        Some(mime::Source::Stream(s)) => {
            let mut src = Src::Stream(fetch_stream(vm, fname, &s)?);
            src.rewind(vm)?;
            while !src.eof(vm)? {
                let buf = src.read(vm, 1024)?;
                if buf.is_empty() {
                    break;
                }
                parse_data(vm, fname, id, &buf);
            }
        }
        _ => {}
    }
    populate(vm, fname, o, id);
    Ok(Value::Null)
}

fn mm_remove(_vm: &mut Vm, o: &Obj, _args: &[Value]) -> R {
    let Some(id) = object_part(o) else {
        return Ok(Value::False);
    };
    mime::remove_from_parent(id);
    Ok(Value::Null)
}

/// (as in the C code: the same as remove())
fn mm_add_child(vm: &mut Vm, o: &Obj, args: &[Value]) -> R {
    mm_remove(vm, o, args)
}

fn mm_get_child_count(vm: &mut Vm, o: &Obj, _args: &[Value]) -> R {
    let Some(id) = object_part(o) else {
        return Ok(Value::False);
    };
    finalize(vm, "mimemessage::get_child_count", id);
    Ok(Value::Long(mime::children(id).len() as i64))
}

fn mm_get_parent(vm: &mut Vm, o: &Obj, _args: &[Value]) -> R {
    let fname = "mimemessage::get_parent";
    match object_part(o).and_then(mime::parent) {
        Some(parent) => Ok(export(vm, fname, parent)),
        None => Ok(Value::Null),
    }
}

fn mm_get_child(vm: &mut Vm, o: &Obj, args: &[Value]) -> R {
    let fname = "mimemessage::get_child";
    let Some(id) = object_part(o) else {
        return Ok(Value::Null);
    };
    let f = mfunc("get_child");
    let a = match zpp::parse(vm, &f, "z", args) {
        Ok(a) => a,
        Err(()) => return Err(err(vm)),
    };
    finalize(vm, fname, id);
    let found = match &a[0] {
        Arg::Any(v) => match v.deref() {
            Value::String(s) => mime::find_by_name(id, s.as_bytes()),
            Value::Long(n) => mime::find_child_by_position(id, n as i32),
            _ => return Ok(Value::Null),
        },
        _ => return Ok(Value::Null),
    };
    match found {
        Some(p) => Ok(export(vm, fname, p)),
        None => Ok(Value::Null),
    }
}

/// mailparse_mimemessage_extract()
fn mm_extract(vm: &mut Vm, o: &Obj, args: &[Value], method: &str, decode: u32) -> R {
    let fname = format!("mimemessage::{}", method);
    let Some(id) = object_part(o) else {
        return Ok(Value::Null);
    };
    let f = mfunc(method);
    let a = match zpp::parse(vm, &f, "|lz!", args) {
        Ok(a) => a,
        Err(()) => return Err(err(vm)),
    };
    let mode = a[0].long_or(MAILPARSE_EXTRACT_OUTPUT);
    let zarg = match a.get(1) {
        Some(Arg::Any(v)) => Some(v.deref()),
        _ => None,
    };
    let mut dest = match mode {
        MAILPARSE_EXTRACT_STREAM => match zarg {
            None => {
                warn(vm, &fname, "Parameter 2 must be a stream");
                return Ok(Value::Null);
            }
            Some(z) => Dest::Stream(fetch_stream(vm, &fname, &z)?),
        },
        MAILPARSE_EXTRACT_RETURN => {
            stream_resource_id();
            Dest::Buffer(Vec::new())
        }
        _ => Dest::Output,
    };
    let mut src = part_source(vm, &fname, id)?;
    finalize(vm, &fname, id);
    if extract_part(vm, &fname, id, decode, &mut src, &mut dest)? {
        return Ok(match dest {
            Dest::Buffer(b) if mode == MAILPARSE_EXTRACT_RETURN => str_val(&b),
            _ => Value::True,
        });
    }
    Ok(Value::Null)
}

fn mm_extract_headers(vm: &mut Vm, o: &Obj, args: &[Value]) -> R {
    mm_extract(vm, o, args, "extract_headers", DECODE_NOBODY)
}

fn mm_extract_body(vm: &mut Vm, o: &Obj, args: &[Value]) -> R {
    mm_extract(vm, o, args, "extract_body", DECODE_NOHEADERS | DECODE_8BIT)
}

/// The start of the body of a part and the end of its section.
fn body_range(id: i64) -> (i64, i64) {
    mime::get(id)
        .map(|p| {
            let p = p.borrow();
            let end = if p.parent.is_some() {
                p.bodyend
            } else {
                p.endpos
            };
            (p.bodystart, end)
        })
        .unwrap_or((0, 0))
}

fn mm_extract_uue(vm: &mut Vm, o: &Obj, args: &[Value]) -> R {
    let fname = "mimemessage::extract_uue";
    let Some(id) = object_part(o) else {
        return Ok(Value::Null);
    };
    let f = mfunc("extract_uue");
    let a = match zpp::parse(vm, &f, "l|lz", args) {
        Ok(a) => a,
        Err(()) => return Err(err(vm)),
    };
    let index = a[0].long_or(0);
    let mode = a.get(1).map_or(MAILPARSE_EXTRACT_OUTPUT, |m| {
        m.long_or(MAILPARSE_EXTRACT_OUTPUT)
    });
    let zarg = match a.get(2) {
        Some(Arg::Any(v)) => Some(v.deref()),
        _ => None,
    };
    let mut dest = match mode {
        MAILPARSE_EXTRACT_STREAM => match zarg {
            None => {
                warn(vm, fname, "Parameter 2 must be a stream");
                return Ok(Value::Null);
            }
            Some(z) => Some(Dest::Stream(fetch_stream(vm, fname, &z)?)),
        },
        MAILPARSE_EXTRACT_RETURN => {
            stream_resource_id();
            Some(Dest::Buffer(Vec::new()))
        }
        MAILPARSE_EXTRACT_OUTPUT => {
            // php://output
            stream_resource_id();
            Some(Dest::Output)
        }
        _ => None,
    };
    let mut src = part_source(vm, fname, id)?;
    finalize(vm, fname, id);
    let (start_pos, end) = body_range(id);
    src.seek(vm, start_pos)?;
    let mut nparts = 0;
    while !src.eof(vm)? {
        let Some(line) = src.gets(vm, 4096)? else {
            break;
        };
        if line.starts_with(b"begin ") {
            if nparts == index {
                let mut data = Vec::new();
                do_uudecode(vm, &mut src, Some(&mut data))?;
                return Ok(match dest.as_mut() {
                    Some(Dest::Buffer(b)) => {
                        b.extend_from_slice(&data);
                        str_val(b)
                    }
                    Some(d) => {
                        d.emit(vm, fname, &data)?;
                        Value::True
                    }
                    // no stream to write to
                    None => Value::True,
                });
            }
            // skip that part
            do_uudecode(vm, &mut src, None)?;
            nparts += 1;
        } else if src.tell(vm)? >= end {
            break;
        }
    }
    Ok(Value::Null)
}

fn mm_enum_uue(vm: &mut Vm, o: &Obj, _args: &[Value]) -> R {
    let fname = "mimemessage::enum_uue";
    let Some(id) = object_part(o) else {
        return Ok(Value::False);
    };
    let mut src = part_source(vm, fname, id)?;
    finalize(vm, fname, id);
    let (start_pos, end) = body_range(id);
    src.seek(vm, start_pos)?;
    let mut ret: Option<PhpArray> = None;
    while !src.eof(vm)? {
        let Some(line) = src.gets(vm, 4096)? else {
            break;
        };
        if line.starts_with(b"begin ") {
            let origfilename = begin_filename(&line);
            let list = ret.get_or_insert_with(PhpArray::new);
            let mut item = PhpArray::new();
            set(&mut item, "filename", str_val(&origfilename));
            let pos = src.tell(vm)?;
            set(&mut item, "start-pos", Value::Long(pos));
            // decode it and remember the file size
            let size = do_uudecode(vm, &mut src, None)?;
            set(&mut item, "filesize", Value::Long(size as i64));
            let curr = src.tell(vm)?;
            if curr > end {
                // the message is probably bogus: cancel this item
                warn(
                    vm,
                    fname,
                    "uue attachment overran part boundary; this should not happen, message is probably malformed",
                );
                break;
            }
            set(&mut item, "end-pos", Value::Long(curr));
            list.push(arr_val(item));
        } else if src.tell(vm)? >= end {
            break;
        }
    }
    Ok(match ret {
        Some(a) => arr_val(a),
        None => Value::False,
    })
}
