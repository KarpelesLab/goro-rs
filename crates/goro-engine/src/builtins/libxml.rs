//! ext/libxml (PHP manual: libxml): error collection (`LibXMLError`,
//! `libxml_use_internal_errors()`), the external entity loader, the
//! streams context, the `LIBXML_*` constants, and the helpers the other
//! XML extensions share: reporting parser errors as PHP does, and loading
//! documents and external resources through PHP's streams or the user's
//! entity loader. Formats observed on PHP 8.5 (libxml2 2.15).

use goro_runtime::{Key, PhpArray, PhpStr, Value};
use goro_xml::error::{Domain, LEVEL_WARNING, XmlError};
use goro_xml::xml::{EntityLoader, LoadContext, LoadError};

use crate::func::Args;
use crate::registry::{ConstVal, Module};
use crate::vm::{R, Unwind, Vm};

pub static MODULE: Module = Module {
    name: "libxml",
    functions: &[
        (
            "function libxml_set_streams_context($context): void",
            libxml_set_streams_context,
        ),
        (
            "function libxml_use_internal_errors(?bool $use_errors = null): bool",
            libxml_use_internal_errors,
        ),
        (
            "function libxml_get_last_error(): LibXMLError|false",
            libxml_get_last_error,
        ),
        ("function libxml_get_errors(): array", libxml_get_errors),
        ("function libxml_clear_errors(): void", libxml_clear_errors),
        (
            "function libxml_disable_entity_loader(bool $disable = true): bool",
            libxml_disable_entity_loader,
        ),
        (
            "function libxml_set_external_entity_loader(?callable $resolver_function): true",
            libxml_set_external_entity_loader,
        ),
        (
            "function libxml_get_external_entity_loader(): ?callable",
            libxml_get_external_entity_loader,
        ),
    ],
    classes: &[(
        "libxmlerror",
        r#"<?php
class LibXMLError {
    public int $level;
    public int $code;
    public int $column;
    public string $message;
    public string $file;
    public int $line;
}"#,
    )],
    constants: &[
        ("LIBXML_VERSION", ConstVal::Int(21502)),
        ("LIBXML_DOTTED_VERSION", ConstVal::Str("2.15.2")),
        ("LIBXML_LOADED_VERSION", ConstVal::Str("21502")),
        ("LIBXML_RECOVER", ConstVal::Int(1)),
        ("LIBXML_NOENT", ConstVal::Int(2)),
        ("LIBXML_NO_XXE", ConstVal::Int(8388608)),
        ("LIBXML_DTDLOAD", ConstVal::Int(4)),
        ("LIBXML_DTDATTR", ConstVal::Int(8)),
        ("LIBXML_DTDVALID", ConstVal::Int(16)),
        ("LIBXML_NOERROR", ConstVal::Int(32)),
        ("LIBXML_NOWARNING", ConstVal::Int(64)),
        ("LIBXML_NOBLANKS", ConstVal::Int(256)),
        ("LIBXML_XINCLUDE", ConstVal::Int(1024)),
        ("LIBXML_NSCLEAN", ConstVal::Int(8192)),
        ("LIBXML_NOCDATA", ConstVal::Int(16384)),
        ("LIBXML_NONET", ConstVal::Int(2048)),
        ("LIBXML_PEDANTIC", ConstVal::Int(128)),
        ("LIBXML_COMPACT", ConstVal::Int(65536)),
        ("LIBXML_NOXMLDECL", ConstVal::Int(2)),
        ("LIBXML_PARSEHUGE", ConstVal::Int(524288)),
        ("LIBXML_BIGLINES", ConstVal::Int(4194304)),
        ("LIBXML_NOEMPTYTAG", ConstVal::Int(4)),
        ("LIBXML_SCHEMA_CREATE", ConstVal::Int(1)),
        ("LIBXML_HTML_NOIMPLIED", ConstVal::Int(8192)),
        ("LIBXML_HTML_NODEFDTD", ConstVal::Int(4)),
        ("LIBXML_ERR_NONE", ConstVal::Int(0)),
        ("LIBXML_ERR_WARNING", ConstVal::Int(1)),
        ("LIBXML_ERR_ERROR", ConstVal::Int(2)),
        ("LIBXML_ERR_FATAL", ConstVal::Int(3)),
    ],
    shutdown: Some(|vm| {
        let st = vm.ext::<LibxmlState>();
        st.loader = None;
        st.context = None;
        st.errors.clear();
    }),
    ..Module::EMPTY
};

/// The extension's request state.
#[derive(Default)]
pub struct LibxmlState {
    /// `libxml_use_internal_errors(true)`.
    pub internal: bool,
    /// Errors collected while `internal` is on.
    pub errors: Vec<XmlError>,
    /// The last error (kept whatever `internal` is).
    pub last: Option<XmlError>,
    /// `libxml_set_external_entity_loader()`.
    pub loader: Option<Value>,
    /// `libxml_set_streams_context()`.
    pub context: Option<Value>,
}

fn libxml_set_streams_context(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let v = a.val(0);
    if !matches!(v, Value::Resource(_)) {
        return vm.fthrow(
            a,
            "TypeError",
            &format!(
                "Argument #1 ($context) must be of type resource, {} given",
                crate::class::TypeName::tname(&v)
            ),
        );
    }
    vm.ext::<LibxmlState>().context = Some(v);
    Ok(Value::Null)
}

fn libxml_use_internal_errors(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let st = vm.ext::<LibxmlState>();
    let old = st.internal;
    if a.has(0) && !matches!(a.val(0), Value::Null) {
        let on = a.bool(0);
        st.internal = on;
        if !on {
            st.errors.clear();
        }
    }
    Ok(Value::Bool(old))
}

/// A `LibXMLError` object for `e`.
pub fn error_object(vm: &mut Vm, e: &XmlError) -> R<Value> {
    let cls = vm
        .find_class(b"LibXMLError", false)?
        .expect("LibXMLError is declared");
    let o = vm.instantiate_raw(&cls)?;
    vm.set_prop_raw(&o, b"level", Value::Int(e.level as i64));
    vm.set_prop_raw(&o, b"code", Value::Int(e.code as i64));
    vm.set_prop_raw(&o, b"column", Value::Int(e.column as i64));
    vm.set_prop_raw(&o, b"message", Value::from(e.message.as_bytes()));
    vm.set_prop_raw(
        &o,
        b"file",
        Value::from(e.file.as_deref().unwrap_or("").as_bytes()),
    );
    vm.set_prop_raw(&o, b"line", Value::Int(e.line as i64));
    Ok(Value::Object(o))
}

fn libxml_get_last_error(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    match vm.ext::<LibxmlState>().last.clone() {
        Some(e) => error_object(vm, &e),
        None => Ok(Value::Bool(false)),
    }
}

fn libxml_get_errors(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    let errs = vm.ext::<LibxmlState>().errors.clone();
    let mut arr = PhpArray::new();
    for e in &errs {
        let o = error_object(vm, e)?;
        let _ = arr.push(o);
    }
    Ok(Value::Array(arr))
}

fn libxml_clear_errors(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    let st = vm.ext::<LibxmlState>();
    st.errors.clear();
    st.last = None;
    Ok(Value::Null)
}

fn libxml_disable_entity_loader(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    vm.deprecated("Function libxml_disable_entity_loader() is deprecated since 8.0, as external entity loading is disabled by default")?;
    // External entity loading is off by default since PHP 8.0 (PHP
    // manual: libxml_disable_entity_loader); the call changes nothing.
    Ok(Value::Bool(false))
}

fn libxml_set_external_entity_loader(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let v = a.val(0);
    vm.ext::<LibxmlState>().loader = if matches!(v, Value::Null) {
        None
    } else {
        Some(v)
    };
    Ok(Value::Bool(true))
}

fn libxml_get_external_entity_loader(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    Ok(vm
        .ext::<LibxmlState>()
        .loader
        .clone()
        .unwrap_or(Value::Null))
}

// ------------------------------------------------------------------ errors

/// How an extension shows libxml's errors when they are not collected.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Style {
    /// ext/dom: one diagnostic per error, "msg in Entity, line: N";
    /// warnings are notices (observed on PHP 8.5).
    Dom,
    /// libxml's generic handler (SimpleXML, XMLReader...): the message
    /// line, the source line and a caret, each as a warning.
    Generic,
}

/// `LIBXML_NOERROR` / `LIBXML_NOWARNING` hide errors from display (they are
/// still collected with internal errors on; observed on PHP 8.5).
pub fn suppressed(e: &XmlError, flags: u32) -> bool {
    (e.level == LEVEL_WARNING && flags & goro_xml::xml::opt::NOWARNING != 0)
        || (e.level > LEVEL_WARNING && flags & goro_xml::xml::opt::NOERROR != 0)
}

/// Records errors (`libxml_get_last_error()`, the internal list) and,
/// unless internal errors are on, shows them as diagnostics of `fname`.
/// `input` is the parsed text (for the generic style's source lines).
pub fn report(
    vm: &mut Vm,
    fname: &str,
    errors: &[XmlError],
    style: Style,
    flags: u32,
    input: Option<&[u8]>,
) -> R<()> {
    if errors.is_empty() {
        return Ok(());
    }
    let st = vm.ext::<LibxmlState>();
    st.last = errors.last().cloned();
    if st.internal {
        st.errors.extend(errors.iter().cloned());
        return Ok(());
    }
    for e in errors {
        if suppressed(e, flags) {
            continue;
        }
        show(vm, fname, e, style, input)?;
    }
    Ok(())
}

/// Shows one error (not recorded).
pub fn show(vm: &mut Vm, fname: &str, e: &XmlError, style: Style, input: Option<&[u8]>) -> R<()> {
    match style {
        Style::Dom => {
            let m = format!("{fname}(): {}", e.dom_warning());
            if e.level == LEVEL_WARNING {
                vm.notice_msg(&m)
            } else {
                vm.warn(&m)
            }
        }
        Style::Generic => {
            for l in e.generic_lines(input.unwrap_or(b"")) {
                vm.warn(&format!("{fname}(): {l}"))?;
            }
            Ok(())
        }
    }
}

/// Records one error without showing it (errors PHP's extensions raise
/// themselves through libxml, such as DOM's validation messages).
pub fn record(vm: &mut Vm, e: XmlError) -> bool {
    let st = vm.ext::<LibxmlState>();
    st.last = Some(e.clone());
    if st.internal {
        st.errors.push(e);
        true
    } else {
        false
    }
}

/// True if errors are being collected (`libxml_use_internal_errors(true)`).
pub fn internal_errors(vm: &mut Vm) -> bool {
    vm.ext::<LibxmlState>().internal
}

/// A libxml-style I/O error (`failed to load external entity "url"`).
pub fn io_error(url: &str) -> XmlError {
    XmlError {
        domain: Domain::Io,
        code: 1549,
        level: LEVEL_WARNING,
        message: format!("failed to load external entity \"{url}\"\n"),
        file: None,
        line: 0,
        column: 0,
    }
}

// ----------------------------------------------------------------- loading

/// True if `libxml_set_external_entity_loader()` set a loader (parses then
/// run without the node store borrowed, the loader being PHP code).
pub fn has_user_loader(vm: &mut Vm) -> bool {
    vm.ext::<LibxmlState>().loader.is_some()
}

/// What the user loader returned.
enum UserLoad {
    Bytes(Vec<u8>),
    /// Returned null.
    Null,
    /// Anything else (already reported).
    Invalid,
}

fn call_user_loader(
    vm: &mut Vm,
    fname: &str,
    public: Option<&str>,
    system: &str,
    ctx: &LoadContext,
    directory: Option<&str>,
) -> R<UserLoad> {
    let Some(cb) = vm.ext::<LibxmlState>().loader.clone() else {
        return Ok(UserLoad::Null);
    };
    let s = |o: Option<&str>| o.map_or(Value::Null, |s| Value::from(s.as_bytes()));
    let mut c = PhpArray::new();
    c.insert(Key::from_bytes(b"directory"), s(directory));
    c.insert(
        Key::from_bytes(b"intSubName"),
        s(ctx.int_sub_name.as_deref()),
    );
    c.insert(Key::from_bytes(b"extSubURI"), s(ctx.ext_sub_uri.as_deref()));
    c.insert(
        Key::from_bytes(b"extSubSystem"),
        s(ctx.ext_sub_system.as_deref()),
    );
    let r = vm.call_value(
        &cb,
        vec![s(public), Value::from(system.as_bytes()), Value::Array(c)],
    )?;
    match r {
        Value::Null => Ok(UserLoad::Null),
        Value::Str(path) => match read_url_quiet(vm, path.as_bytes())? {
            Some(b) => Ok(UserLoad::Bytes(b)),
            None => Ok(UserLoad::Invalid),
        },
        Value::Resource(res) => Ok(match read_resource(vm, &res) {
            Some(b) => UserLoad::Bytes(b),
            None => UserLoad::Invalid,
        }),
        other => {
            vm.warn(&format!(
                "{fname}(): The user entity loader callback '{}' has returned a value of type {}; string or resource expected",
                callable_name(&cb),
                crate::class::TypeName::tname(&other)
            ))?;
            Ok(UserLoad::Invalid)
        }
    }
}

fn callable_name(cb: &Value) -> String {
    match cb {
        Value::Str(s) => s.to_string_lossy(),
        _ => "{closure}".to_string(),
    }
}

/// Reads a stream resource to its end.
pub fn read_resource(vm: &mut Vm, res: &goro_runtime::ResRef) -> Option<Vec<u8>> {
    let data = res.res().data.borrow();
    let cell = data
        .as_ref()?
        .downcast_ref::<std::cell::RefCell<super::file::Stream>>()?;
    let cell: *const std::cell::RefCell<super::file::Stream> = cell;
    drop(data);
    // SAFETY: the resource is alive (held by the caller) for the call.
    let mut s = unsafe { (*cell).borrow_mut() };
    Some(super::file::read_rest(vm, &mut s))
}

/// Reads a file or stream URL without PHP-level warnings (libxml reports
/// failures itself). `None` when it can't be opened.
pub fn read_url_quiet(vm: &mut Vm, path: &[u8]) -> R<Option<Vec<u8>>> {
    // file: URLs are percent-decoded (RFC 8089; test ext/xmlreader 008).
    let decoded;
    let p = match path.strip_prefix(b"file://") {
        Some(rest) if rest.contains(&b'%') => {
            decoded = percent_decode(rest);
            &decoded[..]
        }
        Some(rest) => rest,
        None => path,
    };
    // Plain files directly; other URLs through the stream layer.
    if !p.windows(3).any(|w| w == b"://") && !p.starts_with(b"data:") {
        let fp = vm.fs_path(p);
        return Ok(std::fs::read(&fp).ok());
    }
    let f = vm.call_value(
        &Value::from(&b"fopen"[..]),
        vec![Value::from(path), Value::from(&b"rb"[..])],
    );
    match f {
        Ok(Value::Resource(r)) => {
            let b = read_resource(vm, &r);
            let _ = vm.call_value(&Value::from(&b"fclose"[..]), vec![Value::Resource(r)]);
            Ok(b)
        }
        Ok(_) => Ok(None),
        Err(Unwind::Throw(e)) => Err(Unwind::Throw(e)),
        Err(e) => Err(e),
    }
}

fn percent_decode(s: &[u8]) -> Vec<u8> {
    let hex = |b: u8| (b as char).to_digit(16).map(|d| d as u8);
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        if s[i] == b'%'
            && i + 2 < s.len()
            && let (Some(h), Some(l)) = (hex(s[i + 1]), hex(s[i + 2]))
        {
            out.push(h * 16 + l);
            i += 3;
            continue;
        }
        out.push(s[i]);
        i += 1;
    }
    out
}

/// Resolves a system ID against a base URL (or the current directory),
/// as libxml does for relative references.
pub fn resolve(vm: &Vm, system: &str, base: Option<&str>) -> String {
    let r = goro_xml::xml::resolve_system_id(system, base);
    if r.starts_with('/') || r.contains("://") || base.is_some() {
        return r;
    }
    vm.fs_path(r.as_bytes()).to_string_lossy().into_owned()
}

/// Loads a document's bytes (`DOMDocument::load()`, `simplexml_load_file()`,
/// `XMLReader::open()`), through the user entity loader when one is set.
/// `Err(Some(msg))`: a warning to show; `Err(None)`: libxml's I/O warning
/// (`failed to load external entity`) applies.
pub fn load_document(vm: &mut Vm, fname: &str, path: &str) -> R<Result<Vec<u8>, Option<String>>> {
    if has_user_loader(vm) {
        let ctx = LoadContext::default();
        return Ok(match call_user_loader(vm, fname, None, path, &ctx, None)? {
            UserLoad::Bytes(b) => Ok(b),
            UserLoad::Null => Err(Some(
                "Failed to load external entity because the resolver function returned null".into(),
            )),
            UserLoad::Invalid => Err(None),
        });
    }
    Ok(match read_url_quiet(vm, path.as_bytes())? {
        Some(b) => Ok(b),
        None => Err(None),
    })
}

/// The entity loader handed to goro-xml's parser: the user's loader, or
/// files and PHP streams. Exceptions thrown by the user's loader are kept
/// in `pending` (the parse continues as if loading failed) and must be
/// rethrown by the caller with [`PhpLoader::finish`].
pub struct PhpLoader {
    vm: *mut Vm,
    fname: String,
    pub pending: Option<Unwind>,
    /// Diagnostics PHP itself raises while loading (shown after the parse).
    pub warnings: Vec<String>,
    /// Network access forbidden (LIBXML_NONET).
    pub nonet: bool,
}

impl PhpLoader {
    /// A loader for a parse run by `fname`. The VM must outlive the loader
    /// and not be used otherwise while the parser runs.
    pub fn new(vm: &mut Vm, fname: &str) -> PhpLoader {
        PhpLoader {
            vm: vm as *mut Vm,
            fname: fname.to_string(),
            pending: None,
            warnings: Vec::new(),
            nonet: false,
        }
    }

    /// Shows the deferred diagnostics and rethrows a pending exception.
    pub fn finish(self, vm: &mut Vm) -> R<()> {
        for w in &self.warnings {
            vm.warn(&format!("{}(): {}", self.fname, w))?;
        }
        match self.pending {
            Some(u) => Err(u),
            None => Ok(()),
        }
    }
}

impl EntityLoader for PhpLoader {
    fn load(
        &mut self,
        public_id: Option<&str>,
        system_id: &str,
        base: Option<&str>,
    ) -> Result<Vec<u8>, LoadError> {
        self.load_with(public_id, system_id, base, &LoadContext::default())
    }

    fn load_with(
        &mut self,
        public_id: Option<&str>,
        system_id: &str,
        base: Option<&str>,
        ctx: &LoadContext,
    ) -> Result<Vec<u8>, LoadError> {
        if self.pending.is_some() {
            return Err(LoadError::Refused);
        }
        // SAFETY: see `new`.
        let vm = unsafe { &mut *self.vm };
        let url = resolve(vm, system_id, base);
        if has_user_loader(vm) {
            let dir = match base {
                Some(b) => {
                    let d = super::file::dirname_bytes(b.as_bytes());
                    Some(String::from_utf8_lossy(&d).into_owned())
                }
                None => Some(vm.cwd.to_string_lossy().into_owned()),
            };
            return match call_user_loader(vm, &self.fname, public_id, &url, ctx, dir.as_deref()) {
                Ok(UserLoad::Bytes(b)) => Ok(b),
                Ok(UserLoad::Null) => {
                    self.warnings.push(format!(
                        "Failed to load external entity \"{}\"",
                        public_id.unwrap_or(system_id)
                    ));
                    Err(LoadError::Refused)
                }
                Ok(UserLoad::Invalid) => Err(LoadError::Refused),
                Err(u) => {
                    self.pending = Some(u);
                    Err(LoadError::Refused)
                }
            };
        }
        if self.nonet && (url.starts_with("http://") || url.starts_with("ftp://")) {
            return Err(LoadError::NotFound);
        }
        match read_url_quiet(vm, url.as_bytes()) {
            Ok(Some(b)) => Ok(b),
            Ok(None) => Err(LoadError::NotFound),
            Err(u) => {
                self.pending = Some(u);
                Err(LoadError::Refused)
            }
        }
    }
}

/// Converts PHP's byte string to the UTF-8 text goro-xml takes for names
/// and values (invalid bytes replaced).
pub fn utf8(s: &PhpStr) -> String {
    String::from_utf8_lossy(s.as_bytes()).into_owned()
}
