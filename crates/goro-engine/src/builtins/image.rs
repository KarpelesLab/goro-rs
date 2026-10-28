//! getimagesize(), getimagesizefromstring(), image_type_to_mime_type(),
//! image_type_to_extension(), iptcparse() and iptcembed() (PHP manual
//! pages of these functions; the parsers are in goro_ext_standard::image).

use goro_ext_standard::image as img;
use goro_runtime::{Key, PhpArray, PhpStr, Value};

use super::file::{open_stream, read_rest};
use crate::func::Args;
use crate::registry::{ConstVal, Module};
use crate::vm::{R, Vm};

pub static MODULE: Module = Module {
    name: "standard",
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
    constants: &[
        ("IMAGETYPE_GIF", ConstVal::Int(img::GIF)),
        ("IMAGETYPE_JPEG", ConstVal::Int(img::JPEG)),
        ("IMAGETYPE_PNG", ConstVal::Int(img::PNG)),
        ("IMAGETYPE_SWF", ConstVal::Int(img::SWF)),
        ("IMAGETYPE_PSD", ConstVal::Int(img::PSD)),
        ("IMAGETYPE_BMP", ConstVal::Int(img::BMP)),
        ("IMAGETYPE_TIFF_II", ConstVal::Int(img::TIFF_II)),
        ("IMAGETYPE_TIFF_MM", ConstVal::Int(img::TIFF_MM)),
        ("IMAGETYPE_JPC", ConstVal::Int(img::JPC)),
        ("IMAGETYPE_JP2", ConstVal::Int(img::JP2)),
        ("IMAGETYPE_JPX", ConstVal::Int(img::JPX)),
        ("IMAGETYPE_JB2", ConstVal::Int(img::JB2)),
        ("IMAGETYPE_SWC", ConstVal::Int(img::SWC)),
        ("IMAGETYPE_IFF", ConstVal::Int(img::IFF)),
        ("IMAGETYPE_WBMP", ConstVal::Int(img::WBMP)),
        ("IMAGETYPE_JPEG2000", ConstVal::Int(img::JPC)),
        ("IMAGETYPE_XBM", ConstVal::Int(img::XBM)),
        ("IMAGETYPE_ICO", ConstVal::Int(img::ICO)),
        ("IMAGETYPE_WEBP", ConstVal::Int(img::WEBP)),
        ("IMAGETYPE_AVIF", ConstVal::Int(img::AVIF)),
        ("IMAGETYPE_HEIF", ConstVal::Int(img::HEIF)),
        ("IMAGETYPE_SVG", ConstVal::Int(img::SVG)),
        ("IMAGETYPE_UNKNOWN", ConstVal::Int(0)),
        ("IMAGETYPE_COUNT", ConstVal::Int(img::COUNT)),
    ],
    listed: false,
    ..Module::EMPTY
};

fn s(v: &str) -> Value {
    Value::Str(PhpStr::new(v.as_bytes()))
}

fn analyze(vm: &mut Vm, a: &Args, data: &[u8], what: &[u8]) -> R<Value> {
    let want = a.has(1);
    if want {
        a.set_ref(1, Value::Array(PhpArray::new()));
    }
    if data.is_empty() {
        let msg = format!("Error reading from {}!", String::from_utf8_lossy(what));
        vm.fnotice(a, &msg)?;
        return Ok(Value::Bool(false));
    }
    let mut extra = img::Extra::default();
    let inflate = |z: &[u8]| goro_ext_zlib::gzuncompress(z, 0).ok();
    let info = img::image_size(data, want, &inflate, &mut extra);
    for w in &extra.warnings {
        vm.fwarn(a, w)?;
    }
    if want && !extra.app.is_empty() {
        let mut arr = PhpArray::new();
        for (n, d) in extra.app {
            arr.insert(
                Key::Str(PhpStr::new(format!("APP{n}").as_bytes())),
                Value::Str(PhpStr::from(d)),
            );
        }
        a.set_ref(1, Value::Array(arr));
    }
    let Some(i) = info else {
        return Ok(Value::Bool(false));
    };
    let mut r = PhpArray::new();
    let _ = r.push(Value::Int(i.width as i64));
    let _ = r.push(Value::Int(i.height as i64));
    let _ = r.push(Value::Int(i.kind));
    let _ = r.push(s(&format!("width=\"{}\" height=\"{}\"", i.width, i.height)));
    if let Some(b) = i.bits {
        r.insert(Key::from_bytes(b"bits"), Value::Int(b as i64));
    }
    if let Some(c) = i.channels {
        r.insert(Key::from_bytes(b"channels"), Value::Int(c as i64));
    }
    r.insert(Key::from_bytes(b"mime"), s(img::mime_type(i.kind)));
    r.insert(Key::from_bytes(b"width_unit"), s("px"));
    r.insert(Key::from_bytes(b"height_unit"), s("px"));
    Ok(Value::Array(r))
}

fn getimagesize(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let f = a.str(0);
    let Some(mut st) = open_stream(vm, a, &f, b"rb", false)? else {
        if a.has(1) {
            a.set_ref(1, Value::Array(PhpArray::new()));
        }
        return Ok(Value::Bool(false));
    };
    let data = read_rest(vm, &mut st);
    drop(st);
    analyze(vm, a, &data, &f)
}

fn getimagesizefromstring(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let d = a.str(0);
    analyze(vm, a, &d, b"")
}

fn image_type_to_mime_type(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(s(img::mime_type(a.int(0))))
}

fn image_type_to_extension(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(match img::extension(a.int(0)) {
        Some(e) if a.bool_or(1, true) => s(&format!(".{e}")),
        Some(e) => s(e),
        None => Value::Bool(false),
    })
}

fn iptcparse(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let Some(tags) = img::iptc_parse(a.bytes(0)) else {
        return Ok(Value::Bool(false));
    };
    let mut out = PhpArray::new();
    for (rec, ds, v) in tags {
        let k = Key::Str(PhpStr::new(format!("{rec}#{ds:03}").as_bytes()));
        let e = out.entry(k);
        if !matches!(e, Value::Array(_)) {
            *e = Value::Array(PhpArray::new());
        }
        if let Value::Array(l) = e {
            let _ = l.push(Value::Str(PhpStr::from(v)));
        }
    }
    Ok(Value::Array(out))
}

fn iptcembed(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let iptc = a.str(0);
    let f = a.str(1);
    let spool = a.int_or(2, 0);
    let jpg = match std::fs::read(vm.fs_path(&f)) {
        Ok(d) => d,
        Err(_) => {
            let msg = format!("Unable to open {}", String::from_utf8_lossy(&f));
            vm.fwarn(a, &msg)?;
            return Ok(Value::Bool(false));
        }
    };
    let Some(out) = img::iptc_embed(&iptc, &jpg) else {
        return Ok(Value::Bool(false));
    };
    if spool > 0 {
        vm.echo(&out);
    }
    if spool >= 2 {
        return Ok(Value::Bool(true));
    }
    Ok(Value::Str(PhpStr::from(out)))
}
