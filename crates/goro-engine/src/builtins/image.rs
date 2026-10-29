//! The image functions of ext/standard (PHP manual: getimagesize,
//! getimagesizefromstring, image_type_to_mime_type,
//! image_type_to_extension, iptcparse, iptcembed) and the IMAGETYPE_*
//! constants, on the goro-ext-exif crate.

use goro_ext_exif::{imageinfo, iptc};
use goro_runtime::{Key, PhpArray, PhpStr, Value};

use super::exif::emit_bare;
use super::file::open_stream;
use crate::func::Args;
use crate::registry::{ConstVal, Module};
use crate::vm::{R, Vm};

pub static MODULE: Module = Module {
    name: "image",
    functions: &[
        (
            "function getimagesize(string $filename, &$image_info = null): array|false",
            getimagesize,
        ),
        (
            "function getimagesizefromstring(string $string, &$image_info = null): array|false",
            getimagesizefromstring,
        ),
        (
            "function image_type_to_mime_type(int $image_type): string",
            image_type_to_mime_type,
        ),
        (
            "function image_type_to_extension(int $image_type, bool $include_dot = true): string|false",
            image_type_to_extension,
        ),
        (
            "function iptcparse(string $iptc_block): array|false",
            iptcparse,
        ),
        (
            "function iptcembed(string $iptc_data, string $filename, int $spool = 0): string|bool",
            iptcembed,
        ),
    ],
    constants: &CONSTANTS,
    listed: false,
    ..Module::EMPTY
};

const fn consts() -> [(&'static str, ConstVal); 24] {
    let mut out = [("", ConstVal::Null); 24];
    let mut i = 0;
    while i < 24 {
        let (n, v) = imageinfo::CONSTANTS[i];
        out[i] = (n, ConstVal::Int(v));
        i += 1;
    }
    out
}

static CONSTANTS: [(&str, ConstVal); 24] = consts();

/// The getimagesize() array for `d`; sets `$image_info` when passed.
fn size_of(vm: &mut Vm, a: &mut Args, d: &[u8], name: &[u8]) -> R<Value> {
    let want = a.len() > 1;
    let mut diags = Vec::new();
    let (r, info) = imageinfo::image_size(d, want, &mut diags);
    emit_bare(vm, a, name, diags)?;
    if want {
        let mut arr = PhpArray::new();
        for (k, v) in info {
            arr.insert(Key::from(k.as_str()), Value::Str(PhpStr::from(v)));
        }
        a.set_ref(1, Value::Array(arr));
    }
    let Some(s) = r else {
        return Ok(Value::Bool(false));
    };
    let mut arr = PhpArray::new();
    let _ = arr.push(Value::Int(s.width as i64));
    let _ = arr.push(Value::Int(s.height as i64));
    let _ = arr.push(Value::Int(s.itype));
    if let Some(h) = s.html {
        let _ = arr.push(Value::Str(PhpStr::from(h)));
    }
    if s.bits != 0 {
        arr.insert(Key::from("bits"), Value::Int(s.bits as i64));
    }
    if s.channels != 0 {
        arr.insert(Key::from("channels"), Value::Int(s.channels as i64));
    }
    arr.insert(
        Key::from("mime"),
        Value::Str(PhpStr::from(imageinfo::mime_type(s.itype))),
    );
    arr.insert(
        Key::from("width_unit"),
        Value::Str(PhpStr::from(s.width_unit)),
    );
    arr.insert(
        Key::from("height_unit"),
        Value::Str(PhpStr::from(s.height_unit)),
    );
    Ok(Value::Array(arr))
}

fn getimagesize(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let f = a.str(0);
    if f.contains(&0) {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #1 ($filename) must not contain any null bytes",
        );
    }
    if a.len() > 1 {
        a.set_ref(1, Value::Array(PhpArray::new()));
    }
    let Some(mut s) = open_stream(vm, a, &f, b"rb", false)? else {
        return Ok(Value::Bool(false));
    };
    let d = super::file::read_rest(vm, &mut s);
    super::streams::close_stream(vm, &mut s)?;
    size_of(vm, a, &d, f.as_bytes())
}

fn getimagesizefromstring(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let d = a.str(0);
    size_of(vm, a, d.as_bytes(), d.as_bytes())
}

fn image_type_to_mime_type(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(Value::Str(PhpStr::from(imageinfo::mime_type(a.int(0)))))
}

fn image_type_to_extension(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(match imageinfo::extension(a.int(0)) {
        Some(e) if a.bool_or(1, true) => Value::Str(PhpStr::from(format!(".{e}"))),
        Some(e) => Value::Str(PhpStr::from(e)),
        None => Value::Bool(false),
    })
}

fn iptcparse(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let Some(sets) = iptc::parse(a.bytes(0)) else {
        return Ok(Value::Bool(false));
    };
    let mut arr = PhpArray::new();
    for (k, vals) in sets {
        let mut l = PhpArray::new();
        for v in vals {
            let _ = l.push(Value::Str(PhpStr::from(v)));
        }
        arr.insert(Key::from(k.as_str()), Value::Array(l));
    }
    Ok(Value::Array(arr))
}

fn iptcembed(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let data = a.str(0);
    let f = a.str(1);
    let spool = a.int_or(2, 0);
    let s = if f.contains(&0) {
        None
    } else {
        open_stream(vm, a, &f, b"rb", true)?
    };
    let Some(mut s) = s else {
        vm.fwarn(
            a,
            &format!("Unable to open {}", String::from_utf8_lossy(&f)),
        )?;
        return Ok(Value::Bool(false));
    };
    let d = super::file::read_rest(vm, &mut s);
    super::streams::close_stream(vm, &mut s)?;
    let Some(out) = iptc::embed(data.as_bytes(), &d) else {
        return Ok(Value::Bool(false));
    };
    // PHP manual: with $spool of 1 or more the JPEG is printed, and from
    // 2 it is not returned (observed on PHP 8.5).
    if spool >= 1 {
        vm.echo(&out);
    }
    if spool >= 2 {
        return Ok(Value::Bool(true));
    }
    Ok(Value::Str(PhpStr::from(out)))
}
