//! Image input/output of gd.c: imagecreatefrom*(), imagecreatefromstring(),
//! image<format>() writers (to a file, a stream or the output), and
//! imageloadfont().

use goro_core::value::Value;
use goro_core::vm::Vm;
use goro_ext_standard::ext_io;
use goro_gd::{Font, Image};

use crate::zpp::{self, Arg, Func};
use crate::*;

fn args(
    vm: &mut Vm,
    name: &'static str,
    spec: &str,
    a: &[Value],
) -> Result<(Func, Vec<Arg>), VmError> {
    let f = Func::new(name);
    let v = zpp::parse(vm, &f, spec, a)?;
    Ok((f, v))
}

/// The image formats (PHP_GDIMG_TYPE_*)
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Gif,
    Png,
    Jpg,
    Wbm,
    Xbm,
    Xpm,
    Gd,
    Gd2,
    Gd2Part,
    Webp,
    Bmp,
    Tga,
    Avif,
}

/// The ...Ctx() decoder of a format; None when it is not compiled in.
fn decode(
    kind: Kind,
    data: &[u8],
    part: (i32, i32, i32, i32),
    jpeg_ignore: bool,
) -> Option<Option<Image>> {
    use goro_gd::*;

    Some(match kind {
        Kind::Png => png::create_from_png(data),
        Kind::Gif => gif_in::create_from_gif(data),
        Kind::Wbm => wbmp::create_from_wbmp(data),
        Kind::Xbm => xbm::create_from_xbm(data),
        Kind::Xpm => xpm::create_from_xpm(data),
        Kind::Gd => gd_gd::create_from_gd(data),
        Kind::Gd2 => gd2::create_from_gd2(data),
        Kind::Gd2Part => gd2::create_from_gd2_part(data, part.0, part.1, part.2, part.3),
        Kind::Bmp => bmp::create_from_bmp(data),
        Kind::Tga => tga::create_from_tga(data),
        Kind::Jpg => jpeg::create_from_jpeg(data, jpeg_ignore),
        Kind::Webp => webp::create_from_webp(data),
        Kind::Avif => avif::create_from_avif(data),
    })
}

fn kind_name(kind: Kind) -> &'static str {
    match kind {
        Kind::Gif => "GIF",
        Kind::Png => "PNG",
        Kind::Jpg => "JPEG",
        Kind::Wbm => "WBMP",
        Kind::Xbm => "XBM",
        Kind::Xpm => "XPM",
        Kind::Gd => "GD",
        Kind::Gd2 | Kind::Gd2Part => "GD2",
        Kind::Webp => "WEBP",
        Kind::Bmp => "BMP",
        Kind::Tga => "TGA",
        Kind::Avif => "AVIF",
    }
}

const FUNCTIONS: &[(&str, goro_core::vm::BuiltinFn)] = &[
    ("imagecreatefromstring", imagecreatefromstring),
    ("imagecreatefrompng", imagecreatefrompng),
    ("imagepng", imagepng),
    ("imagecreatefromgif", imagecreatefromgif),
    ("imagecreatefromwebp", imagecreatefromwebp),
    ("imagecreatefromavif", imagecreatefromavif),
    ("imagewebp", imagewebp),
    ("imageavif", imageavif),
    ("imagecreatefromjpeg", imagecreatefromjpeg),
    ("imagejpeg", imagejpeg),
    ("imagecreatefromxbm", imagecreatefromxbm),
    ("imagecreatefromxpm", imagecreatefromxpm),
    ("imagecreatefromwbmp", imagecreatefromwbmp),
    ("imagecreatefromgd", imagecreatefromgd),
    ("imagecreatefromgd2", imagecreatefromgd2),
    ("imagecreatefromgd2part", imagecreatefromgd2part),
    ("imagecreatefrombmp", imagecreatefrombmp),
    ("imagecreatefromtga", imagecreatefromtga),
    ("imagegif", imagegif),
    ("imagewbmp", imagewbmp),
    ("imagebmp", imagebmp),
    ("imagexbm", imagexbm),
    ("imagegd", imagegd),
    ("imagegd2", imagegd2),
    ("imageloadfont", imageloadfont),
];

pub fn register(vm: &mut Vm) {
    for &(name, f) in FUNCTIONS {
        register_fn(vm, name, f);
    }
}

/// _php_image_create_from()
fn create_from(vm: &mut Vm, name: &'static str, a: &[Value], kind: Kind) -> R {
    let (f, a) = if kind == Kind::Gd2Part {
        args(vm, name, "pllll", a)?
    } else {
        args(vm, name, "p", a)?
    };
    let file = a[0].str_or(b"").to_vec();
    let mut part = (0, 0, 0, 0);
    if kind == Kind::Gd2Part {
        let w = a[3].long_or(0);
        let h = a[4].long_or(0);
        if w < 1 {
            return Err(f.value_error(vm, 4, "must be greater than or equal to 1"));
        }
        if h < 1 {
            return Err(f.value_error(vm, 5, "must be greater than or equal to 1"));
        }
        part = (
            a[1].long_or(0) as i32,
            a[2].long_or(0) as i32,
            w as i32,
            h as i32,
        );
    }
    let Some(id) = ext_io::open(vm, &file, "rb", true)? else {
        return Ok(Value::False);
    };
    let data = ext_io::read_all(vm, id);
    let jpeg_ignore = !ext_io::meta(vm, id).is_stdio() || jpeg_ignore_warning(vm);
    if data.len() > i32::MAX as usize {
        f.warn(
            vm,
            &format!("Cannot read images with more than {} bytes", i32::MAX),
        );
        ext_io::close(vm, id);
        return Ok(Value::False);
    }
    let im = decode(kind, &data, part, jpeg_ignore).flatten();
    flush_errors(vm, &f);
    ext_io::close(vm, id);
    match im {
        Some(im) => wrap_image(vm, im),
        None => {
            f.warn(
                vm,
                &format!(
                    "\"{}\" is not a valid {} file",
                    String::from_utf8_lossy(&file),
                    kind_name(kind)
                ),
            );
            Ok(Value::False)
        }
    }
}

fn imagecreatefrompng(vm: &mut Vm, a: &[Value]) -> R {
    create_from(vm, "imagecreatefrompng", a, Kind::Png)
}

/// INI_INT("gd.jpeg_ignore_warning")
fn jpeg_ignore_warning(vm: &Vm) -> bool {
    let v = vm
        .constants
        .get(&b"gd.jpeg_ignore_warning"[..])
        .map(|v| v.to_php_string().as_bytes().to_vec())
        .unwrap_or_default();
    // ZEND_ATOL()
    let s = String::from_utf8_lossy(&v);
    let t = s.trim_start();
    let end = t
        .char_indices()
        .take_while(|&(i, c)| c.is_ascii_digit() || (i == 0 && (c == '-' || c == '+')))
        .map(|(i, c)| i + c.len_utf8())
        .last()
        .unwrap_or(0);
    t[..end].parse::<i64>().unwrap_or(0) != 0
}

fn imagecreatefromjpeg(vm: &mut Vm, a: &[Value]) -> R {
    create_from(vm, "imagecreatefromjpeg", a, Kind::Jpg)
}

fn imagejpeg(vm: &mut Vm, a: &[Value]) -> R {
    let (f, a) = args(vm, "imagejpeg", "O|z!l", a)?;
    let im = a[0].img();
    let quality = a[2].long_or(-1);
    let Some(sink) = output_context(vm, &f, &a[1], 2)? else {
        return Ok(Value::False);
    };
    if !(-1..=100).contains(&quality) {
        sink.finish(vm);
        return Err(f.value_error(vm, 3, "must be at between -1 and 100"));
    }
    let data = goro_gd::jpeg::image_jpeg(&im.borrow(), quality as i32);
    emit(vm, &f, sink, data)
}

fn imagecreatefromwebp(vm: &mut Vm, a: &[Value]) -> R {
    create_from(vm, "imagecreatefromwebp", a, Kind::Webp)
}

fn imagecreatefromavif(vm: &mut Vm, a: &[Value]) -> R {
    create_from(vm, "imagecreatefromavif", a, Kind::Avif)
}

fn imagewebp(vm: &mut Vm, a: &[Value]) -> R {
    let (f, a) = args(vm, "imagewebp", "O|z!l", a)?;
    let im = a[0].img();
    let quality = a[2].long_or(-1);
    let Some(sink) = output_context(vm, &f, &a[1], 2)? else {
        return Ok(Value::False);
    };
    if quality < -1 {
        sink.finish(vm);
        return Err(f.value_error(vm, 3, "must be greater than or equal to -1"));
    }
    let data = goro_gd::webp::image_webp(&im.borrow(), quality as i32);
    emit(vm, &f, sink, data)
}

fn imageavif(vm: &mut Vm, a: &[Value]) -> R {
    let (f, a) = args(vm, "imageavif", "O|z!ll", a)?;
    let im = a[0].img();
    let quality = a[2].long_or(-1);
    let mut speed = a[3].long_or(-1);
    let Some(sink) = output_context(vm, &f, &a[1], 2)? else {
        return Ok(Value::False);
    };
    if !(-1..=100).contains(&quality) {
        sink.finish(vm);
        return Err(f.value_error(vm, 3, "must be between -1 and 100"));
    }
    if !(-1..=10).contains(&speed) {
        sink.finish(vm);
        return Err(f.value_error(vm, 4, "must be between -1 and 10"));
    } else if speed == -1 {
        speed = 6;
    }
    let data = goro_gd::avif::image_avif(&im.borrow(), quality as i32, speed as i32);
    emit(vm, &f, sink, data)
}

fn imagecreatefromgif(vm: &mut Vm, a: &[Value]) -> R {
    create_from(vm, "imagecreatefromgif", a, Kind::Gif)
}

fn imagecreatefromxbm(vm: &mut Vm, a: &[Value]) -> R {
    create_from(vm, "imagecreatefromxbm", a, Kind::Xbm)
}

fn imagecreatefromxpm(vm: &mut Vm, a: &[Value]) -> R {
    create_from(vm, "imagecreatefromxpm", a, Kind::Xpm)
}

fn imagecreatefromwbmp(vm: &mut Vm, a: &[Value]) -> R {
    create_from(vm, "imagecreatefromwbmp", a, Kind::Wbm)
}

fn imagecreatefromgd(vm: &mut Vm, a: &[Value]) -> R {
    create_from(vm, "imagecreatefromgd", a, Kind::Gd)
}

fn imagecreatefromgd2(vm: &mut Vm, a: &[Value]) -> R {
    create_from(vm, "imagecreatefromgd2", a, Kind::Gd2)
}

fn imagecreatefromgd2part(vm: &mut Vm, a: &[Value]) -> R {
    create_from(vm, "imagecreatefromgd2part", a, Kind::Gd2Part)
}

fn imagecreatefrombmp(vm: &mut Vm, a: &[Value]) -> R {
    create_from(vm, "imagecreatefrombmp", a, Kind::Bmp)
}

fn imagecreatefromtga(vm: &mut Vm, a: &[Value]) -> R {
    create_from(vm, "imagecreatefromtga", a, Kind::Tga)
}

/// _php_image_type()
fn image_type(data: &[u8]) -> Option<Kind> {
    if data.len() < 12 {
        return None;
    }
    if data.starts_with(b"gd2") {
        return Some(Kind::Gd2);
    }
    if data.starts_with(b"\xff\xd8\xff") {
        return Some(Kind::Jpg);
    }
    if data.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Some(Kind::Png);
    }
    if data.starts_with(b"GIF") {
        return Some(Kind::Gif);
    }
    if data.starts_with(b"BM") {
        return Some(Kind::Bmp);
    }
    if data.starts_with(b"RIFF") && &data[8..12] == b"WEBP" {
        return Some(Kind::Webp);
    }
    if ext_io::is_avif_data(data) {
        return Some(Kind::Avif);
    }
    if data[0] == 0 {
        return Some(Kind::Wbm);
    }
    None
}

fn imagecreatefromstring(vm: &mut Vm, a: &[Value]) -> R {
    let (f, a) = args(vm, "imagecreatefromstring", "s", a)?;
    let data = a[0].str_or(b"").to_vec();
    if data.len() > i32::MAX as usize {
        return Err(f.value_error(vm, 1, "is too long"));
    }
    let Some(kind) = image_type(&data) else {
        f.warn(vm, "Data is not in a recognized format");
        return Ok(Value::False);
    };
    let Some(im) = decode(kind, &data, (0, 0, 0, 0), true) else {
        f.warn(
            vm,
            &format!("No {} support in this PHP build", kind_name(kind)),
        );
        return Ok(Value::False);
    };
    flush_errors(vm, &f);
    match im {
        Some(im) => wrap_image(vm, im),
        None => {
            f.warn(
                vm,
                &format!("Passed data is not in \"{}\" format", kind_name(kind)),
            );
            f.warn(vm, "Couldn't create GD Image Stream out of Data");
            Ok(Value::False)
        }
    }
}

/// Where an image writer sends its bytes (gdIOCtx of create_output_context())
pub enum Sink {
    Output,
    Stream { id: i64, close: bool },
}

impl Sink {
    pub fn write(&self, vm: &mut Vm, data: &[u8]) {
        match self {
            Sink::Output => vm.write_output(data),
            Sink::Stream { id, .. } => {
                ext_io::write(vm, *id, data);
            }
        }
    }

    pub fn finish(self, vm: &mut Vm) {
        if let Sink::Stream { id, close: true } = self {
            ext_io::close(vm, id);
        }
    }
}

/// create_output_context(): None when the stream could not be opened (or
/// an exception was thrown).
pub fn output_context(
    vm: &mut Vm,
    f: &Func,
    to: &Arg,
    arg_num: usize,
) -> Result<Option<Sink>, VmError> {
    let v = match to {
        Arg::Any(v) => v.deref(),
        _ => return Ok(Some(Sink::Output)),
    };
    match &v {
        Value::Null => Ok(Some(Sink::Output)),
        Value::Resource(id) => {
            if ext_io::is_stream(vm, &v) {
                Ok(Some(Sink::Stream {
                    id: id.id(),
                    close: false,
                }))
            } else {
                Ok(None)
            }
        }
        Value::String(s) => {
            if s.as_bytes().contains(&0) {
                return Err(f.arg_type_error(vm, arg_num, "must not contain null bytes"));
            }
            match ext_io::open(vm, s.as_bytes(), "wb", true)? {
                Some(id) => Ok(Some(Sink::Stream { id, close: true })),
                None => Ok(None),
            }
        }
        other => {
            let m = format!(
                "must be a file name or a stream resource, {} given",
                Vm::value_type_name(other)
            );
            Err(f.arg_type_error(vm, arg_num, &m))
        }
    }
}

fn imagepng(vm: &mut Vm, a: &[Value]) -> R {
    let (f, a) = args(vm, "imagepng", "O|z!ll", a)?;
    let im = a[0].img();
    let quality = a[2].long_or(-1);
    let basefilter = a[3].long_or(-1);
    let Some(sink) = output_context(vm, &f, &a[1], 2)? else {
        return Ok(Value::False);
    };
    if !(-1..=9).contains(&quality) {
        sink.finish(vm);
        return Err(f.value_error(vm, 3, "must be between -1 and 9"));
    }
    let data = goro_gd::png::image_png(&im.borrow(), quality as i32, basefilter as i32);
    flush_errors(vm, &f);
    if let Some(d) = data {
        sink.write(vm, &d);
    }
    sink.finish(vm);
    Ok(Value::True)
}

/// Write an encoded image to the sink of argument #2.
fn emit(vm: &mut Vm, f: &Func, sink: Sink, data: Option<Vec<u8>>) -> R {
    flush_errors(vm, f);
    if let Some(d) = data {
        sink.write(vm, &d);
    }
    sink.finish(vm);
    Ok(Value::True)
}

fn imagegif(vm: &mut Vm, a: &[Value]) -> R {
    let (f, a) = args(vm, "imagegif", "O|z!", a)?;
    let im = a[0].img();
    let Some(sink) = output_context(vm, &f, &a[1], 2)? else {
        return Ok(Value::False);
    };
    let data = goro_gd::gif_out::image_gif(&im.borrow());
    emit(vm, &f, sink, data)
}

/// The default foreground of imagewbmp() / imagexbm(): the first black
/// palette entry (or colorsTotal).
fn default_fg(im: &Image) -> i32 {
    let mut i = 0;
    while i < im.colors_total {
        if im.red_of(i) == 0 && im.green_of(i) == 0 && im.blue_of(i) == 0 {
            break;
        }
        i += 1;
    }
    i
}

fn imagewbmp(vm: &mut Vm, a: &[Value]) -> R {
    let (f, a) = args(vm, "imagewbmp", "O|z!l!", a)?;
    let im = a[0].img();
    let Some(sink) = output_context(vm, &f, &a[1], 2)? else {
        return Ok(Value::False);
    };
    let im = im.borrow();
    let fg = match a[2] {
        Arg::Long(n) => n as i32,
        _ => default_fg(&im),
    };
    let data = goro_gd::wbmp::image_wbmp(&im, fg);
    drop(im);
    emit(vm, &f, sink, data)
}

fn imagebmp(vm: &mut Vm, a: &[Value]) -> R {
    let (f, a) = args(vm, "imagebmp", "O|z!b", a)?;
    let im = a[0].img();
    let compressed = a[2].bool_or(true);
    let Some(sink) = output_context(vm, &f, &a[1], 2)? else {
        return Ok(Value::False);
    };
    let data = goro_gd::bmp::image_bmp(&im.borrow(), compressed as i32);
    emit(vm, &f, sink, data)
}

fn imagexbm(vm: &mut Vm, a: &[Value]) -> R {
    let (f, a) = args(vm, "imagexbm", "Op!|l!", a)?;
    let im = a[0].img();
    let file = match &a[1] {
        Arg::Str(s) => Some(s.clone()),
        _ => None,
    };
    let sink = match &file {
        Some(p) => match ext_io::open(vm, p, "wb", true)? {
            Some(id) => Sink::Stream { id, close: true },
            None => return Ok(Value::False),
        },
        None => Sink::Output,
    };
    let im = im.borrow();
    let fg = match a[2] {
        Arg::Long(n) => n as i32,
        _ => default_fg(&im),
    };
    let data = goro_gd::xbm::image_xbm(&im, file.as_deref().unwrap_or(b""), fg);
    drop(im);
    emit(vm, &f, sink, Some(data))
}

/// _php_image_output(): imagegd() / imagegd2()
fn gd_output(vm: &mut Vm, name: &'static str, a: &[Value], gd2: bool) -> R {
    let (f, a) = if gd2 {
        args(vm, name, "O|p!ll", a)?
    } else {
        args(vm, name, "O|p!", a)?
    };
    let im = a[0].img();
    let mut quality = if gd2 { a[2].long_or(128) } else { 128 };
    let ty = if gd2 { a[3].long_or(1) } else { 1 };
    if quality < i32::MIN as i64 || quality > i32::MAX as i64 {
        f.warn(
            vm,
            &format!(
                "Argument #3 ($chunk_size) must be between {} and {}",
                i32::MIN,
                i32::MAX
            ),
        );
        return Ok(Value::False);
    }
    let file = match &a[1] {
        Arg::Str(s) if !s.is_empty() => Some(s.clone()),
        _ => None,
    };
    let sink = match &file {
        Some(p) => {
            let ps = String::from_utf8_lossy(p).into_owned();
            if !vm.check_open_basedir(f.name, &ps) {
                f.warn(vm, "Invalid filename");
                return Ok(Value::False);
            }
            match ext_io::open(vm, p, "wb", false)? {
                Some(id) => Sink::Stream { id, close: true },
                None => {
                    f.warn(vm, &format!("Unable to open \"{}\" for writing", ps));
                    return Ok(Value::False);
                }
            }
        }
        None => Sink::Output,
    };
    let data = if gd2 {
        if quality == -1 {
            quality = 128;
        }
        goro_gd::gd2::image_gd2(&im.borrow(), quality as i32, ty as i32)
    } else {
        Some(goro_gd::gd_gd::image_gd(&im.borrow()))
    };
    emit(vm, &f, sink, data)
}

fn imagegd(vm: &mut Vm, a: &[Value]) -> R {
    gd_output(vm, "imagegd", a, false)
}

fn imagegd2(vm: &mut Vm, a: &[Value]) -> R {
    gd_output(vm, "imagegd2", a, true)
}

/// imageloadfont()
fn imageloadfont(vm: &mut Vm, a: &[Value]) -> R {
    let (f, a) = args(vm, "imageloadfont", "p", a)?;
    let file = a[0].str_or(b"").to_vec();
    let Some(id) = ext_io::open(vm, &file, "rb", true)? else {
        return Ok(Value::False);
    };
    let r = load_font(vm, &f, id);
    ext_io::close(vm, id);
    match r {
        Some(font) => Ok(wrap_font(vm, font)),
        None => Ok(Value::False),
    }
}

fn load_font(vm: &mut Vm, f: &Func, id: i64) -> Option<Font> {
    const HDR: usize = 16;
    let hdr = ext_io::read(vm, id, HDR);
    if hdr.len() < HDR {
        // php_stream_read() returned 0: EOF or an error
        f.warn(vm, "End of file while reading header");
        return None;
    }
    let int = |i: usize| i32::from_ne_bytes([hdr[i], hdr[i + 1], hdr[i + 2], hdr[i + 3]]);
    let (mut nchars, offset, mut w, mut h) = (int(0), int(4), int(8), int(12));
    let pos = ext_io::tell(vm, id);
    ext_io::seek(vm, id, 0, 2);
    let body_size_check = ext_io::tell(vm, id) - HDR as i64;
    ext_io::seek(vm, id, pos, 0);
    let bad_header = |nchars: i32, w: i32, h: i32| {
        goro_gd::overflow2(nchars, h) || goro_gd::overflow2(nchars.wrapping_mul(h), w)
    };
    if bad_header(nchars, w, h) {
        flush_errors(vm, f);
        f.warn(vm, "Error reading font, invalid font header");
        return None;
    }
    let mut body_size = (w as i64) * (h as i64) * (nchars as i64);
    if body_size != body_size_check {
        w = w.swap_bytes();
        h = h.swap_bytes();
        nchars = nchars.swap_bytes();
        if bad_header(nchars, w, h) {
            flush_errors(vm, f);
            f.warn(vm, "Error reading font, invalid font header");
            return None;
        }
        body_size = (w as i64) * (h as i64) * (nchars as i64);
    }
    flush_errors(vm, f);
    if body_size != body_size_check {
        f.warn(vm, "Error reading font");
        return None;
    }
    let data = ext_io::read(vm, id, body_size as usize);
    if (data.len() as i64) < body_size {
        f.warn(vm, "End of file while reading body");
        return None;
    }
    Some(Font {
        nchars,
        offset,
        w,
        h,
        data,
    })
}
