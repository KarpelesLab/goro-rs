//! ext/iconv (PHP manual: iconv) on goro-charset's glibc-flavoured
//! converter: the iconv* functions, ob_iconv_handler, the iconv.* ini
//! settings and the `convert.iconv.*` stream filters. Behaviour from the
//! manual, tests ext/iconv/tests and observations on PHP 8.5.

use goro_charset::iconv::{
    self as ic, CharsetArg, Diag, IconvError, MimeEncodeOptions, MimeScheme, StreamConverter,
};
use goro_runtime::{Key, PhpArray, PhpStr, Value};

use super::streams::{FilterSide, StreamFilter, register_filter};
use crate::func::Args;
use crate::registry::{ConstVal, Module};
use crate::vm::{R, Vm};

pub static MODULE: Module = Module {
    name: "iconv",
    functions: &[
        (
            "function iconv(string $from_encoding, string $to_encoding, string $string): string|false",
            iconv,
        ),
        (
            "function iconv_get_encoding(string $type = \"all\"): array|string|false",
            iconv_get_encoding,
        ),
        (
            "function iconv_set_encoding(string $type, string $encoding): bool",
            iconv_set_encoding,
        ),
        (
            "function iconv_strlen(string $string, ?string $encoding = null): int|false",
            iconv_strlen,
        ),
        (
            "function iconv_substr(string $string, int $offset, ?int $length = null, ?string $encoding = null): string|false",
            iconv_substr,
        ),
        (
            "function iconv_strpos(string $haystack, string $needle, int $offset = 0, ?string $encoding = null): int|false",
            iconv_strpos,
        ),
        (
            "function iconv_strrpos(string $haystack, string $needle, ?string $encoding = null): int|false",
            iconv_strrpos,
        ),
        (
            "function iconv_mime_encode(string $field_name, string $field_value, array $options = []): string|false",
            iconv_mime_encode,
        ),
        (
            "function iconv_mime_decode(string $string, int $mode = 0, ?string $encoding = null): string|false",
            iconv_mime_decode,
        ),
        (
            "function iconv_mime_decode_headers(string $headers, int $mode = 0, ?string $encoding = null): array|false",
            iconv_mime_decode_headers,
        ),
        (
            "function ob_iconv_handler(string $contents, int $status): string",
            ob_iconv_handler,
        ),
    ],
    constants: &[
        ("ICONV_IMPL", ConstVal::Str(ic::ICONV_IMPL)),
        ("ICONV_VERSION", ConstVal::Str(ic::ICONV_VERSION)),
        (
            "ICONV_MIME_DECODE_STRICT",
            ConstVal::Int(ic::MIME_DECODE_STRICT),
        ),
        (
            "ICONV_MIME_DECODE_CONTINUE_ON_ERROR",
            ConstVal::Int(ic::MIME_DECODE_CONTINUE_ON_ERROR),
        ),
    ],
    // PHP manual: iconv runtime configuration (empty defaults).
    ini: &[
        ("iconv.input_encoding", ""),
        ("iconv.output_encoding", ""),
        ("iconv.internal_encoding", ""),
    ],
    init: Some(init),
    on_ini_set: Some(on_ini_set),
    listed: true,
    ..Module::EMPTY
};

fn init(vm: &mut Vm) {
    register_filter(vm, "convert.iconv.*", iconv_filter);
}

/// Setting the iconv.* encodings is deprecated (observed on PHP 8.5).
fn on_ini_set(vm: &mut Vm, k: &[u8], v: &[u8]) -> R<bool> {
    if matches!(
        k,
        b"iconv.input_encoding" | b"iconv.output_encoding" | b"iconv.internal_encoding"
    ) {
        // a name too long is refused, silently (test
        // ob_iconv_handler-charset-length-cve-2007-4840.phpt)
        if v.len() >= ic::MAX_CHARSET_LEN {
            return Ok(false);
        }
        vm.deprecated(&format!(
            "ini_set(): Use of {} is deprecated",
            String::from_utf8_lossy(k)
        ))?;
    }
    Ok(true)
}

/// Raises the library's diagnostics with the function's prefix.
fn diags(vm: &mut Vm, a: &Args, d: Vec<Diag>) -> R<()> {
    for x in d {
        match x {
            Diag::Warning(m) => vm.fwarn(a, &m)?,
            Diag::Notice(m) => vm.fnotice(a, &m)?,
            Diag::Deprecated(m) => vm.fdeprecated(a, &m)?,
        }
    }
    Ok(())
}

fn ini(vm: &Vm, k: &[u8]) -> Vec<u8> {
    vm.get_ini(k)
        .map(|v| v.as_bytes().to_vec())
        .unwrap_or_default()
}

/// One of the iconv encodings: iconv.<kind>, else the PHP <kind> setting,
/// else default_charset (PHP manual: iconv runtime configuration).
fn encoding(vm: &Vm, kind: &str) -> Vec<u8> {
    let v = ini(vm, format!("iconv.{kind}").as_bytes());
    if !v.is_empty() {
        return v;
    }
    let v = ini(vm, kind.as_bytes());
    if !v.is_empty() {
        return v;
    }
    ini(vm, b"default_charset")
}

fn sv(b: Vec<u8>) -> Value {
    Value::Str(PhpStr::from(b))
}

fn iconv(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let mut d = Vec::new();
    let r = ic::iconv(a.bytes(0), a.bytes(1), a.bytes(2), &mut d);
    diags(vm, a, d)?;
    Ok(r.map_or(Value::Bool(false), sv))
}

fn iconv_get_encoding(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let t = if a.has(0) {
        a.str(0).to_ascii_lowercase()
    } else {
        b"all".to_vec()
    };
    let get = |vm: &Vm, k: &str| Value::Str(PhpStr::from(encoding(vm, k)));
    Ok(match &t[..] {
        b"all" => {
            let mut out = PhpArray::new();
            for k in ["input_encoding", "output_encoding", "internal_encoding"] {
                out.insert(Key::from(k), get(vm, k));
            }
            Value::Array(out)
        }
        b"input_encoding" => get(vm, "input_encoding"),
        b"output_encoding" => get(vm, "output_encoding"),
        b"internal_encoding" => get(vm, "internal_encoding"),
        _ => Value::Bool(false),
    })
}

fn iconv_set_encoding(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let t = a.str(0).to_ascii_lowercase();
    let k: &[u8] = match &t[..] {
        b"input_encoding" => b"iconv.input_encoding",
        b"output_encoding" => b"iconv.output_encoding",
        b"internal_encoding" => b"iconv.internal_encoding",
        _ => return Ok(Value::Bool(false)),
    };
    let e = a.str(1);
    let mut d = Vec::new();
    if !ic::check_charset_len(&e, &mut d) {
        diags(vm, a, d)?;
        return Ok(Value::Bool(false));
    }
    vm.fdeprecated(
        a,
        &format!("Use of {} is deprecated", String::from_utf8_lossy(k)),
    )?;
    let old = vm.get_ini(k).unwrap_or_else(PhpStr::empty);
    vm.ini.originals.entry(PhpStr::from(k)).or_insert(Some(old));
    vm.set_ini(k, &e);
    Ok(Value::Bool(true))
}

/// The charset argument `i`: given, or the internal encoding.
fn charset_arg(vm: &Vm, a: &Args, i: usize) -> (Option<PhpStr>, Vec<u8>) {
    match a.opt_str(i) {
        Some(s) => (Some(s), Vec::new()),
        None => (None, encoding(vm, "internal_encoding")),
    }
}

fn cs<'a>(given: &'a Option<PhpStr>, def: &'a [u8]) -> CharsetArg<'a> {
    match given {
        Some(s) => CharsetArg::Given(s.as_bytes()),
        None => CharsetArg::Default(def),
    }
}

fn iconv_strlen(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (g, def) = charset_arg(vm, a, 1);
    let mut d = Vec::new();
    let r = ic::strlen(a.bytes(0), cs(&g, &def), &mut d);
    diags(vm, a, d)?;
    Ok(r.map_or(Value::Bool(false), |n| Value::Int(n as i64)))
}

fn iconv_substr(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (g, def) = charset_arg(vm, a, 3);
    let mut d = Vec::new();
    let r = ic::substr(a.bytes(0), a.int(1), a.opt_int(2), cs(&g, &def), &mut d);
    diags(vm, a, d)?;
    Ok(r.map_or(Value::Bool(false), sv))
}

fn iconv_strpos(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (g, def) = charset_arg(vm, a, 3);
    let mut d = Vec::new();
    let r = ic::strpos(a.bytes(0), a.bytes(1), a.int_or(2, 0), cs(&g, &def), &mut d);
    diags(vm, a, d)?;
    match r {
        Ok(p) => Ok(p.map_or(Value::Bool(false), |n| Value::Int(n as i64))),
        Err(IconvError::ValueError(m)) => vm.throw("ValueError", &m),
    }
}

fn iconv_strrpos(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (g, def) = charset_arg(vm, a, 2);
    let mut d = Vec::new();
    let r = ic::strrpos(a.bytes(0), a.bytes(1), cs(&g, &def), &mut d);
    diags(vm, a, d)?;
    Ok(r.map_or(Value::Bool(false), |n| Value::Int(n as i64)))
}

fn iconv_mime_encode(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let prefs = a.array(2);
    let mut opts = MimeEncodeOptions {
        scheme: MimeScheme::B,
        input_charset: None,
        output_charset: None,
        line_length: 76,
        line_break_chars: b"\r\n".to_vec(),
    };
    if let Some(v) = prefs.get_bytes(b"scheme") {
        let s = vm.to_php_str(&v.deref())?;
        if matches!(s.first(), Some(b'Q' | b'q')) {
            opts.scheme = MimeScheme::Q;
        }
    }
    if let Some(v) = prefs.get_bytes(b"input-charset") {
        let s = vm.to_php_str(&v.deref())?;
        if !s.is_empty() {
            opts.input_charset = Some(s.as_bytes().to_vec());
        }
    }
    if let Some(v) = prefs.get_bytes(b"output-charset") {
        let s = vm.to_php_str(&v.deref())?;
        if !s.is_empty() {
            opts.output_charset = Some(s.as_bytes().to_vec());
        }
    }
    if let Some(v) = prefs.get_bytes(b"line-length") {
        opts.line_length = vm.to_int(&v.deref())?;
    }
    if let Some(v) = prefs.get_bytes(b"line-break-chars") {
        let s = vm.to_php_str(&v.deref())?;
        opts.line_break_chars = s.as_bytes().to_vec();
    }
    let def = encoding(vm, "internal_encoding");
    let mut d = Vec::new();
    let r = ic::mime_encode(a.bytes(0), a.bytes(1), &opts, &def, &mut d);
    diags(vm, a, d)?;
    Ok(r.map_or(Value::Bool(false), sv))
}

fn iconv_mime_decode(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (g, def) = charset_arg(vm, a, 2);
    let mut d = Vec::new();
    let r = ic::mime_decode(a.bytes(0), a.int_or(1, 0), cs(&g, &def), &mut d);
    diags(vm, a, d)?;
    Ok(r.map_or(Value::Bool(false), sv))
}

fn iconv_mime_decode_headers(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (g, def) = charset_arg(vm, a, 2);
    let mut d = Vec::new();
    let r = ic::mime_decode_headers(a.bytes(0), a.int_or(1, 0), cs(&g, &def), &mut d);
    diags(vm, a, d)?;
    let Some(h) = r else {
        return Ok(Value::Bool(false));
    };
    let mut out = PhpArray::new();
    for (name, vals) in h {
        let v = if vals.len() == 1 {
            sv(vals.into_iter().next().unwrap_or_default())
        } else {
            Value::Array(PhpArray::from_values(vals.into_iter().map(sv)))
        };
        out.insert(Key::from_bytes(&name), v);
    }
    Ok(Value::Array(out))
}

/// Converts output from iconv.internal_encoding to iconv.output_encoding
/// when the content type is text (default_mimetype in the CLI) (PHP
/// manual: ob_iconv_handler).
fn ob_iconv_handler(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let s = a.str(0);
    let mime = vm
        .get_ini(b"default_mimetype")
        .unwrap_or_else(|| PhpStr::from("text/html"));
    if !mime.starts_with(b"text/") {
        return Ok(Value::Str(s));
    }
    let from = encoding(vm, "internal_encoding");
    let to = encoding(vm, "output_encoding");
    let mut d = Vec::new();
    if !ic::check_charset_len(&from, &mut d) || !ic::check_charset_len(&to, &mut d) {
        diags(vm, a, d)?;
        return Ok(Value::Bool(false));
    }
    let r = ic::iconv(&from, &to, &s, &mut d);
    diags(vm, a, d)?;
    Ok(r.map_or(Value::Str(s), sv))
}

// ---------------------------------------------------------------- filters

/// `convert.iconv.<from>.<to>` or `convert.iconv.<from>/<to>` (PHP manual:
/// Conversion filters).
struct IconvFilter {
    from: String,
    to: String,
    conv: StreamConverter,
}

impl StreamFilter for IconvFilter {
    fn filter(&mut self, vm: &mut Vm, input: &[u8], closing: bool, out: &mut Vec<u8>) -> R<bool> {
        let mut tmp = Vec::new();
        let mut r = self.conv.feed(input, &mut tmp);
        if r.is_ok() && closing {
            r = self.conv.finish(&mut tmp);
        }
        match r {
            Ok(()) => {
                out.extend_from_slice(&tmp);
                Ok(true)
            }
            Err(e) => {
                let what = match e.kind {
                    ic::ConvertErrorKind::IllegalSequence => "invalid multibyte sequence",
                    ic::ConvertErrorKind::IncompleteSequence => "invalid multibyte sequence",
                };
                vm.warn(&format!(
                    "{}(): iconv stream filter (\"{}\"=>\"{}\"): {}",
                    super::streams::caller(vm),
                    self.from,
                    self.to,
                    what
                ))?;
                Ok(false)
            }
        }
    }
}

fn iconv_filter(
    _vm: &mut Vm,
    name: &[u8],
    _p: &Value,
    _s: FilterSide,
) -> R<Option<Box<dyn StreamFilter>>> {
    let spec = &name[b"convert.iconv.".len()..];
    if spec.contains(&0) {
        return Ok(None);
    }
    // "from/to" when there is a slash, else "from.to" at the first dot.
    let (from, to) = if let Some(p) = spec.iter().position(|&c| c == b'/') {
        (&spec[..p], &spec[p + 1..])
    } else if let Some(p) = spec.iter().position(|&c| c == b'.') {
        (&spec[..p], &spec[p + 1..])
    } else {
        return Ok(None);
    };
    if from.is_empty() || to.is_empty() {
        return Ok(None);
    }
    let (Some((src, _)), Some((dst, flags))) = (ic::parse_charset(from), ic::parse_charset(to))
    else {
        return Ok(None);
    };
    Ok(Some(Box::new(IconvFilter {
        from: String::from_utf8_lossy(from).into_owned(),
        to: String::from_utf8_lossy(to).into_owned(),
        conv: StreamConverter::new(src, dst, flags),
    })))
}
