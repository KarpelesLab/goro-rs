//! ext/exif (PHP manual: Exchangeable image information) on the
//! goro-ext-exif crate: exif_read_data(), exif_thumbnail(),
//! exif_imagetype(), exif_tagname(), the exif.* ini settings.

use goro_ext_exif::Diag;
use goro_ext_exif::exif::{self as ex, Charset, FileMeta, Ini, Node, ReadOptions};
use goro_ext_exif::{imageinfo, tags};
use goro_runtime::{Key, PhpArray, PhpStr, Value};

use super::file::{Backing, Stream, open_stream, with_stream};
use crate::func::Args;
use crate::registry::{ConstVal, Module};
use crate::vm::{R, Vm};

pub static MODULE: Module = Module {
    name: "exif",
    functions: &[
        ("function exif_tagname(int $index): string|false", exif_tagname),
        (
            "function exif_read_data($file, ?string $required_sections = null, bool $as_arrays = false, bool $read_thumbnail = false): array|false",
            exif_read_data,
        ),
        (
            "function exif_thumbnail($file, &$width = null, &$height = null, &$image_type = null): string|false",
            exif_thumbnail,
        ),
        (
            "function exif_imagetype(string $filename): int|false",
            exif_imagetype,
        ),
    ],
    constants: &[("EXIF_USE_MBSTRING", ConstVal::Int(1))],
    // PHP manual: Exif runtime configuration (defaults observed on PHP 8.5).
    ini: &[
        ("exif.encode_unicode", "ISO-8859-15"),
        ("exif.decode_unicode_motorola", "UCS-2BE"),
        ("exif.decode_unicode_intel", "UCS-2LE"),
        ("exif.encode_jis", ""),
        ("exif.decode_jis_motorola", "JIS"),
        ("exif.decode_jis_intel", "JIS"),
    ],
    listed: true,
    ..Module::EMPTY
};

/// mbstring's converters, '?' for unmappable characters.
struct MbCharset;

impl Charset for MbCharset {
    fn convert(&self, data: &[u8], to: &[u8], from: &[u8]) -> Option<Vec<u8>> {
        let to = goro_charset::Encoding::from_name(to)?;
        let from = goro_charset::Encoding::from_name(from)?;
        let sub = goro_charset::Subst::default();
        Some(goro_charset::convert(data, to, from, &sub).bytes)
    }
}

fn ini(vm: &Vm) -> Ini {
    let g = |k: &[u8], d: &[u8]| {
        vm.get_ini(k)
            .map(|v| v.as_bytes().to_vec())
            .unwrap_or_else(|| d.to_vec())
    };
    Ini {
        encode_unicode: g(b"exif.encode_unicode", b"ISO-8859-15"),
        decode_unicode_motorola: g(b"exif.decode_unicode_motorola", b"UCS-2BE"),
        decode_unicode_intel: g(b"exif.decode_unicode_intel", b"UCS-2LE"),
        encode_jis: g(b"exif.encode_jis", b""),
        decode_jis_motorola: g(b"exif.decode_jis_motorola", b"JIS"),
        decode_jis_intel: g(b"exif.decode_jis_intel", b"JIS"),
    }
}

/// A PHP value from the library's result tree.
pub(crate) fn to_value(n: Node) -> Value {
    match n {
        Node::Null => Value::Null,
        Node::Int(i) => Value::Int(i),
        Node::Float(f) => Value::Float(f),
        Node::Str(s) => Value::Str(PhpStr::from(s)),
        Node::List(l) => {
            let mut a = PhpArray::new();
            for v in l {
                let _ = a.push(to_value(v));
            }
            Value::Array(a)
        }
        Node::Map(m) => {
            let mut a = PhpArray::new();
            for (k, v) in m {
                a.insert(Key::from_bytes(&k), to_value(v));
            }
            Value::Array(a)
        }
    }
}

/// The bytes of a stream, from its start; its position is restored
/// (test exif_read_data_streams_seek.phpt).
pub(crate) fn stream_bytes(vm: &mut Vm, s: &mut Stream) -> Vec<u8> {
    let pos = stream_tell(s);
    super::file::do_seek(vm, s, 0, 0);
    let d = super::file::read_rest(vm, s);
    if let Some(p) = pos {
        super::file::do_seek(vm, s, p, 0);
    }
    d
}

fn stream_tell(s: &mut Stream) -> Option<i64> {
    use std::io::Seek;
    match &mut s.backing {
        Backing::File(f) => f.stream_position().ok().map(|p| p as i64),
        Backing::Memory { pos, .. } => Some(*pos as i64),
        Backing::User { .. } => Some(s.upos),
        Backing::Native(n) => n.tell().map(|p| p as i64),
        _ => None,
    }
}

/// The FileName and FileDateTime of a stream: plain files only.
fn stream_meta(s: &Stream) -> FileMeta {
    if s.wrapper != "plainfile" {
        return FileMeta::default();
    }
    let uri = s.uri.as_bytes();
    let path = uri.strip_prefix(b"file://").unwrap_or(uri);
    let name = super::file::basename_bytes(path, b"");
    let mtime = match &s.backing {
        Backing::File(f) => f
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0),
        _ => 0,
    };
    FileMeta {
        name: Some(name),
        mtime,
    }
}

/// The data and file information of `$file` (a path or a stream), or
/// None after a warning.
fn read_file(vm: &mut Vm, a: &Args) -> R<Option<(Vec<u8>, FileMeta)>> {
    let v = a.val(0);
    if let Value::Resource(_) = v {
        return with_stream(vm, a, 0, |vm, s| {
            let meta = stream_meta(s);
            Ok(Some((stream_bytes(vm, s), meta)))
        });
    }
    let f = match &v {
        Value::Str(s) => s.clone(),
        other => vm.to_php_str(other)?,
    };
    if f.is_empty() {
        return vm.fthrow(a, "ValueError", "Argument #1 ($file) must not be empty");
    }
    if f.contains(&0) {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #1 ($file) must not contain any null bytes",
        );
    }
    let Some(mut s) = open_stream(vm, a, &f, b"rb", true)? else {
        vm.fwarn(a, "Unable to open file")?;
        return Ok(None);
    };
    if let Backing::File(file) = &s.backing
        && file.metadata().is_ok_and(|m| m.is_dir())
    {
        vm.fwarn(a, "Unable to open file")?;
        return Ok(None);
    }
    let meta = stream_meta(&s);
    let d = stream_bytes(vm, &mut s);
    super::streams::close_stream(vm, &mut s)?;
    Ok(Some((d, meta)))
}

/// Raises the library's diagnostics: `f(name): msg`, or `f(): msg`.
pub(crate) fn emit(vm: &mut Vm, a: &Args, name: Option<&[u8]>, diags: Vec<Diag>) -> R<()> {
    let fname = a.fname();
    let prefix = match name {
        Some(n) => format!("{}({})", fname, String::from_utf8_lossy(n)),
        None => format!("{fname}()"),
    };
    for d in diags {
        match d {
            Diag::ReadError => {
                let n = name.map(|n| String::from_utf8_lossy(n).into_owned());
                vm.notice_msg(&format!(
                    "{fname}(): Error reading from {}!",
                    n.unwrap_or_default()
                ))?
            }
            Diag::Warning(m) => vm.warn(&format!("{prefix}: {m}"))?,
            Diag::Notice(m) => vm.notice_msg(&format!("{prefix}: {m}"))?,
            Diag::WarningBare(m) => vm.warn(&format!("{fname}(): {m}"))?,
            Diag::NoticeBare(m) => vm.notice_msg(&format!("{fname}(): {m}"))?,
        }
    }
    Ok(())
}

fn exif_tagname(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let i = a.int(0);
    if !(0..=0xFFFF).contains(&i) {
        return Ok(Value::Bool(false));
    }
    Ok(match tags::ifd_name(i as u16) {
        Some(n) => Value::Str(PhpStr::from(n)),
        None => Value::Bool(false),
    })
}

fn exif_read_data(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let Some((d, meta)) = read_file(vm, a)? else {
        return Ok(Value::Bool(false));
    };
    let opts = ReadOptions {
        required: a.opt_str(1).map(|s| s.as_bytes().to_vec()),
        as_arrays: a.bool_or(2, false),
        read_thumbnail: a.bool_or(3, false),
    };
    let ini = ini(vm);
    let (r, diags) = ex::read_data(&d, &meta, &opts, &ini, &MbCharset);
    emit(vm, a, meta.name.as_deref(), diags)?;
    Ok(match r {
        Some(n) => to_value(n),
        None => Value::Bool(false),
    })
}

fn exif_thumbnail(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let Some((d, meta)) = read_file(vm, a)? else {
        return Ok(Value::Bool(false));
    };
    let ini = ini(vm);
    let (r, diags) = ex::thumbnail(&d, &ini, &MbCharset);
    emit(vm, a, meta.name.as_deref(), diags)?;
    let Some(t) = r else {
        return Ok(Value::Bool(false));
    };
    if a.len() > 1 {
        a.set_ref(1, Value::Int(t.width as i64));
    }
    if a.len() > 2 {
        a.set_ref(2, Value::Int(t.height as i64));
    }
    if a.len() > 3 {
        a.set_ref(3, Value::Int(t.image_type));
    }
    Ok(Value::Str(PhpStr::from(t.data)))
}

fn exif_imagetype(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let f = a.str(0);
    if f.contains(&0) {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #1 ($filename) must not contain any null bytes",
        );
    }
    let Some(mut s) = open_stream(vm, a, &f, b"rb", false)? else {
        return Ok(Value::Bool(false));
    };
    let d = super::file::read_rest(vm, &mut s);
    super::streams::close_stream(vm, &mut s)?;
    let mut diags = Vec::new();
    let t = imageinfo::image_type(&d, &mut diags);
    emit_bare(vm, a, f.as_bytes(), diags)?;
    Ok(if t == imageinfo::IMAGETYPE_UNKNOWN {
        Value::Bool(false)
    } else {
        Value::Int(t)
    })
}

/// getimagesize()-style diagnostics: `f(): msg`, with `name` in the read
/// error notice.
pub(crate) fn emit_bare(vm: &mut Vm, a: &Args, name: &[u8], diags: Vec<Diag>) -> R<()> {
    for d in diags {
        match d {
            Diag::ReadError => vm.fnotice(
                a,
                &format!("Error reading from {}!", String::from_utf8_lossy(cut_nul(name))),
            )?,
            Diag::Warning(m) | Diag::WarningBare(m) => vm.fwarn(a, &m)?,
            Diag::Notice(m) | Diag::NoticeBare(m) => vm.fnotice(a, &m)?,
        }
    }
    Ok(())
}

/// The name as PHP prints it with `%s`: up to its first NUL byte.
fn cut_nul(b: &[u8]) -> &[u8] {
    match b.iter().position(|&c| c == 0) {
        Some(p) => &b[..p],
        None => b,
    }
}
