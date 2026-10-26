//! Port of php-src ext/standard/image.c (getimagesize & friends),
//! ext/standard/libavifinfo/avifinfo.c (AVIF/HEIF feature parsing) and
//! ext/standard/iptc.c (iptcembed / iptcparse).

use goro_core::array::{ArrayKey, PhpArray};
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};
use std::cell::RefCell;
use std::rc::Rc;

pub fn register(vm: &mut Vm) {
    vm.register_function(b"getimagesize", getimagesize_fn);
    vm.register_function(b"getimagesizefromstring", getimagesizefromstring_fn);
    vm.register_function(b"image_type_to_mime_type", image_type_to_mime_type_fn);
    vm.register_function(b"image_type_to_extension", image_type_to_extension_fn);
    vm.register_function(b"iptcembed", iptcembed_fn);
    vm.register_function(b"iptcparse", iptcparse_fn);

    let p = |n: &[u8]| n.to_vec();
    vm.builtin_param_names
        .insert(p(b"getimagesize"), vec![p(b"filename"), p(b"image_info")]);
    vm.builtin_param_names.insert(
        p(b"getimagesizefromstring"),
        vec![p(b"string"), p(b"image_info")],
    );
    vm.builtin_param_names
        .insert(p(b"image_type_to_mime_type"), vec![p(b"image_type")]);
    vm.builtin_param_names.insert(
        p(b"image_type_to_extension"),
        vec![p(b"image_type"), p(b"include_dot")],
    );
    vm.builtin_param_names.insert(
        p(b"iptcembed"),
        vec![p(b"iptc_data"), p(b"filename"), p(b"spool")],
    );
    vm.builtin_param_names
        .insert(p(b"iptcparse"), vec![p(b"iptc_block")]);

    use goro_core::opcode::ParamType;
    let i = || Some(ParamType::Simple(b"int".to_vec()));
    let s = || Some(ParamType::Simple(b"string".to_vec()));
    let b = || Some(ParamType::Simple(b"bool".to_vec()));
    vm.register_builtin_param_types(b"getimagesize", vec![s(), None]);
    vm.register_builtin_param_types(b"getimagesizefromstring", vec![s(), None]);
    vm.register_builtin_param_types(b"image_type_to_mime_type", vec![i()]);
    vm.register_builtin_param_types(b"image_type_to_extension", vec![i(), b()]);
    vm.register_builtin_param_types(b"iptcembed", vec![s(), s(), i()]);
    vm.register_builtin_param_types(b"iptcparse", vec![s()]);
}

// ---------------------------------------------------------------------------
// Parameter parsing helpers (subset of zend_parse_parameters semantics)

fn throw(vm: &mut Vm, class: &[u8], msg: String) -> VmError {
    let exc = vm.create_exception(class, &msg, vm.current_line);
    vm.current_exception = Some(exc);
    VmError {
        message: msg,
        line: vm.current_line,
    }
}

fn arg_type_error(
    vm: &mut Vm,
    func: &str,
    n: usize,
    name: &str,
    expected: &str,
    v: &Value,
) -> VmError {
    let given = Vm::value_type_name(v);
    throw(
        vm,
        b"TypeError",
        format!(
            "{}(): Argument #{} (${}) must be of type {}, {} given",
            func, n, name, expected, given
        ),
    )
}

fn zpp_long(
    vm: &mut Vm,
    func: &str,
    n: usize,
    name: &str,
    v: Option<&Value>,
) -> Result<i64, VmError> {
    let v = v.map(|v| v.deref()).unwrap_or(Value::Null);
    match &v {
        Value::Long(l) => Ok(*l),
        Value::Null | Value::Undef => Ok(0),
        Value::True => Ok(1),
        Value::False => Ok(0),
        Value::Double(d) => {
            if !d.is_finite() || *d < -9.223_372_036_854_776e18 || *d >= 9.223_372_036_854_776e18 {
                Err(arg_type_error(vm, func, n, name, "int", &v))
            } else {
                Ok(*d as i64)
            }
        }
        Value::String(s) => match goro_core::value::parse_numeric_str(s.as_bytes(), false) {
            Some((goro_core::value::NumStr::Long(l), _)) => Ok(l),
            Some((goro_core::value::NumStr::Double(d), _)) => {
                if !d.is_finite() || d < -9.223_372_036_854_776e18 || d >= 9.223_372_036_854_776e18
                {
                    Err(arg_type_error(vm, func, n, name, "int", &v))
                } else {
                    Ok(d as i64)
                }
            }
            None => Err(arg_type_error(vm, func, n, name, "int", &v)),
        },
        _ => Err(arg_type_error(vm, func, n, name, "int", &v)),
    }
}

fn zpp_bool(
    vm: &mut Vm,
    func: &str,
    n: usize,
    name: &str,
    v: Option<&Value>,
) -> Result<bool, VmError> {
    let v = v.map(|v| v.deref()).unwrap_or(Value::Null);
    match &v {
        Value::Array(_) | Value::Object(_) | Value::Resource(_) | Value::Generator(_) => {
            Err(arg_type_error(vm, func, n, name, "bool", &v))
        }
        other => Ok(other.is_truthy()),
    }
}

fn zpp_str(
    vm: &mut Vm,
    func: &str,
    n: usize,
    name: &str,
    v: Option<&Value>,
) -> Result<PhpString, VmError> {
    let v = v.map(|v| v.deref()).unwrap_or(Value::Null);
    match &v {
        Value::Array(_) | Value::Resource(_) | Value::Generator(_) => {
            Err(arg_type_error(vm, func, n, name, "string", &v))
        }
        Value::Null | Value::Undef => Ok(PhpString::empty()),
        other => Ok(other.to_php_string()),
    }
}

// ---------------------------------------------------------------------------
// Image type constants

const IMAGE_FILETYPE_UNKNOWN: i64 = 0;
const IMAGE_FILETYPE_GIF: i64 = 1;
const IMAGE_FILETYPE_JPEG: i64 = 2;
const IMAGE_FILETYPE_PNG: i64 = 3;
const IMAGE_FILETYPE_SWF: i64 = 4;
const IMAGE_FILETYPE_PSD: i64 = 5;
const IMAGE_FILETYPE_BMP: i64 = 6;
const IMAGE_FILETYPE_TIFF_II: i64 = 7;
const IMAGE_FILETYPE_TIFF_MM: i64 = 8;
const IMAGE_FILETYPE_JPC: i64 = 9;
const IMAGE_FILETYPE_JP2: i64 = 10;
const IMAGE_FILETYPE_JPX: i64 = 11;
const IMAGE_FILETYPE_JB2: i64 = 12;
const IMAGE_FILETYPE_SWC: i64 = 13;
const IMAGE_FILETYPE_IFF: i64 = 14;
const IMAGE_FILETYPE_WBMP: i64 = 15;
const IMAGE_FILETYPE_XBM: i64 = 16;
const IMAGE_FILETYPE_ICO: i64 = 17;
const IMAGE_FILETYPE_WEBP: i64 = 18;
const IMAGE_FILETYPE_AVIF: i64 = 19;
const IMAGE_FILETYPE_HEIF: i64 = 20;

fn php_image_type_to_mime_type(image_type: i64) -> &'static str {
    match image_type {
        IMAGE_FILETYPE_GIF => "image/gif",
        IMAGE_FILETYPE_JPEG => "image/jpeg",
        IMAGE_FILETYPE_PNG => "image/png",
        IMAGE_FILETYPE_SWF | IMAGE_FILETYPE_SWC => "application/x-shockwave-flash",
        IMAGE_FILETYPE_PSD => "image/psd",
        IMAGE_FILETYPE_BMP => "image/bmp",
        IMAGE_FILETYPE_TIFF_II | IMAGE_FILETYPE_TIFF_MM => "image/tiff",
        IMAGE_FILETYPE_IFF => "image/iff",
        IMAGE_FILETYPE_WBMP => "image/vnd.wap.wbmp",
        IMAGE_FILETYPE_JPC => "application/octet-stream",
        IMAGE_FILETYPE_JP2 => "image/jp2",
        IMAGE_FILETYPE_XBM => "image/xbm",
        IMAGE_FILETYPE_ICO => "image/vnd.microsoft.icon",
        IMAGE_FILETYPE_WEBP => "image/webp",
        IMAGE_FILETYPE_AVIF => "image/avif",
        IMAGE_FILETYPE_HEIF => "image/heif",
        t => goro_core::image_handlers::find(t)
            .map(|h| h.mime_type)
            .unwrap_or("application/octet-stream"),
    }
}

/// php_image_type_to_mime_type()
pub fn image_type_to_mime_type(image_type: i64) -> &'static str {
    php_image_type_to_mime_type(image_type)
}

/// php_getimagetype() on the content of a stream (`input` names it in
/// the "Error reading from" notice).
pub fn image_type_from_bytes(vm: &mut Vm, func: &str, data: Vec<u8>, input: &[u8]) -> i64 {
    let mut s = ImgStream::new(data);
    getimagetype(vm, func, &mut s, input)
}

fn image_type_to_mime_type_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let t = zpp_long(vm, "image_type_to_mime_type", 1, "image_type", args.first())?;
    Ok(Value::String(PhpString::from_bytes(
        php_image_type_to_mime_type(t).as_bytes(),
    )))
}

fn image_type_to_extension_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let t = zpp_long(vm, "image_type_to_extension", 1, "image_type", args.first())?;
    let inc_dot = if args.len() > 1 {
        zpp_bool(vm, "image_type_to_extension", 2, "include_dot", args.get(1))?
    } else {
        true
    };
    let ext = match t {
        IMAGE_FILETYPE_GIF => ".gif",
        IMAGE_FILETYPE_JPEG => ".jpeg",
        IMAGE_FILETYPE_PNG => ".png",
        IMAGE_FILETYPE_SWF | IMAGE_FILETYPE_SWC => ".swf",
        IMAGE_FILETYPE_PSD => ".psd",
        IMAGE_FILETYPE_BMP | IMAGE_FILETYPE_WBMP => ".bmp",
        IMAGE_FILETYPE_TIFF_II | IMAGE_FILETYPE_TIFF_MM => ".tiff",
        IMAGE_FILETYPE_IFF => ".iff",
        IMAGE_FILETYPE_JPC => ".jpc",
        IMAGE_FILETYPE_JP2 => ".jp2",
        IMAGE_FILETYPE_JPX => ".jpx",
        IMAGE_FILETYPE_JB2 => ".jb2",
        IMAGE_FILETYPE_XBM => ".xbm",
        IMAGE_FILETYPE_ICO => ".ico",
        IMAGE_FILETYPE_WEBP => ".webp",
        IMAGE_FILETYPE_AVIF => ".avif",
        IMAGE_FILETYPE_HEIF => ".heif",
        t => match goro_core::image_handlers::find(t) {
            Some(h) => h.extension,
            None => return Ok(Value::False),
        },
    };
    let s = if inc_dot { ext } else { &ext[1..] };
    Ok(Value::String(PhpString::from_bytes(s.as_bytes())))
}

// ---------------------------------------------------------------------------
// In-memory stream with php_stream-like read/seek semantics.

struct ImgStream {
    data: Vec<u8>,
    pos: i64,
    eof: bool,
}

impl ImgStream {
    fn new(data: Vec<u8>) -> Self {
        ImgStream {
            data,
            pos: 0,
            eof: false,
        }
    }

    /// php_stream_read: returns up to `n` bytes.
    fn read(&mut self, n: usize) -> &[u8] {
        let len = self.data.len() as i64;
        if self.pos >= len {
            self.eof = true;
            return &[];
        }
        let start = self.pos as usize;
        let end = (start + n).min(self.data.len());
        if end - start < n {
            self.eof = true;
        }
        self.pos = end as i64;
        &self.data[start..end]
    }

    /// Reads exactly into `buf`; returns the number of bytes read.
    fn read_into(&mut self, buf: &mut [u8]) -> usize {
        let n = buf.len();
        let got = self.read(n);
        let l = got.len();
        buf[..l].copy_from_slice(got);
        l
    }

    /// php_stream_getc: byte or -1 (EOF).
    fn getc(&mut self) -> i32 {
        let r = self.read(1);
        if r.is_empty() { -1 } else { r[0] as i32 }
    }

    /// php_stream_seek(SEEK_CUR); returns true on success.
    fn seek_cur(&mut self, off: i64) -> bool {
        let new = self.pos.saturating_add(off);
        if new < 0 {
            return false;
        }
        self.pos = new;
        self.eof = false;
        true
    }

    fn seek_set(&mut self, off: i64) -> bool {
        if off < 0 {
            return false;
        }
        self.pos = off;
        self.eof = false;
        true
    }

    fn rewind(&mut self) -> bool {
        self.seek_set(0)
    }

    /// php_stream_gets(stream, NULL, 0): one line including the newline.
    fn gets(&mut self) -> Option<Vec<u8>> {
        let len = self.data.len() as i64;
        if self.pos >= len {
            self.eof = true;
            return None;
        }
        let start = self.pos as usize;
        let end = match self.data[start..].iter().position(|&c| c == b'\n') {
            Some(i) => start + i + 1,
            None => self.data.len(),
        };
        self.pos = end as i64;
        Some(self.data[start..end].to_vec())
    }

    /// php_stream_copy_to_mem(stream, PHP_STREAM_COPY_ALL)
    fn copy_rest(&mut self) -> Vec<u8> {
        let len = self.data.len() as i64;
        if self.pos >= len {
            self.eof = true;
            return Vec::new();
        }
        let start = self.pos as usize;
        self.pos = len;
        self.eof = true;
        self.data[start..].to_vec()
    }
}

#[derive(Default)]
struct GfxInfo {
    width: u32,
    height: u32,
    bits: u32,
    channels: u32,
}

// ---------------------------------------------------------------------------
// Format handlers

fn handle_gif(s: &mut ImgStream) -> Option<GfxInfo> {
    if !s.seek_cur(3) {
        return None;
    }
    let mut dim = [0u8; 5];
    if s.read_into(&mut dim) != 5 {
        return None;
    }
    Some(GfxInfo {
        width: dim[0] as u32 | ((dim[1] as u32) << 8),
        height: dim[2] as u32 | ((dim[3] as u32) << 8),
        bits: if dim[4] & 0x80 != 0 {
            (dim[4] as u32 & 0x07) + 1
        } else {
            0
        },
        channels: 3,
    })
}

fn handle_psd(s: &mut ImgStream) -> Option<GfxInfo> {
    if !s.seek_cur(11) {
        return None;
    }
    let mut dim = [0u8; 8];
    if s.read_into(&mut dim) != 8 {
        return None;
    }
    Some(GfxInfo {
        height: u32::from_be_bytes([dim[0], dim[1], dim[2], dim[3]]),
        width: u32::from_be_bytes([dim[4], dim[5], dim[6], dim[7]]),
        ..Default::default()
    })
}

fn handle_bmp(s: &mut ImgStream) -> Option<GfxInfo> {
    if !s.seek_cur(11) {
        return None;
    }
    let mut dim = [0u8; 16];
    if s.read_into(&mut dim) != 16 {
        return None;
    }
    let size = i32::from_le_bytes([dim[0], dim[1], dim[2], dim[3]]);
    if size == 12 {
        Some(GfxInfo {
            width: ((dim[5] as u32) << 8) + dim[4] as u32,
            height: ((dim[7] as u32) << 8) + dim[6] as u32,
            bits: dim[11] as u32,
            channels: 0,
        })
    } else if size > 12 && (size <= 64 || size == 108 || size == 124) {
        let width = u32::from_le_bytes([dim[4], dim[5], dim[6], dim[7]]);
        let height = u32::from_le_bytes([dim[8], dim[9], dim[10], dim[11]]);
        Some(GfxInfo {
            width,
            height: (height as i32).wrapping_abs() as u32,
            bits: ((dim[15] as u32) << 8) + dim[14] as u32,
            channels: 0,
        })
    } else {
        None
    }
}

fn swf_get_bits(buffer: &[u8], pos: u32, count: u32) -> u64 {
    let mut result: u64 = 0;
    let mut lp = pos;
    while lp < pos + count {
        let byte = buffer.get((lp / 8) as usize).copied().unwrap_or(0) as u64;
        let bit = (byte >> (7 - (lp % 8))) & 0x01;
        let shift = count - (lp - pos) - 1;
        result = result.wrapping_add(bit.checked_shl(shift).unwrap_or(0));
        lp += 1;
    }
    result
}

fn swf_dims(buf: &[u8]) -> (u32, u32) {
    let bits = swf_get_bits(buf, 0, 5) as u32;
    let width = swf_get_bits(buf, 5 + bits, bits).wrapping_sub(swf_get_bits(buf, 5, bits)) / 20;
    let height =
        swf_get_bits(buf, 5 + 3 * bits, bits).wrapping_sub(swf_get_bits(buf, 5 + 2 * bits, bits))
            / 20;
    (width as u32, height as u32)
}

/// zlib `uncompress()` emulation: returns (ok, output). `ok` is true only
/// when the whole zlib stream was decoded and fit in `max_out` bytes.
fn zlib_uncompress(input: &[u8], max_out: usize) -> (bool, Vec<u8>) {
    let mut out = vec![0u8; max_out];
    let Ok(mut d) = goro_zlib::Inflate::new(goro_zlib::MAX_WBITS) else {
        return (false, Vec::new());
    };
    let mut io = goro_zlib::Io::new(input, &mut out);
    let st = d.inflate(&mut io, goro_zlib::Z_FINISH);
    let n = io.out_pos;
    out.truncate(n);
    (st == goro_zlib::Z_STREAM_END, out)
}

fn handle_swc(s: &mut ImgStream) -> Option<GfxInfo> {
    if !s.seek_cur(5) {
        return None;
    }
    let mut a = [0u8; 64];
    if s.read_into(&mut a) != 64 {
        return None;
    }
    let mut b = vec![0u8; 65];
    let (ok, first) = zlib_uncompress(&a, 64);
    let len = first.len();
    b[..len].copy_from_slice(&first);
    let mut status_ok = ok;
    if !ok {
        if !s.seek_set(8) {
            return None;
        }
        let bufz = s.copy_rest();
        // uncompress() is retried with output buffers of input*2 .. input*2^15.
        let max = bufz.len().saturating_mul(1 << 15);
        let (ok2, out) = zlib_uncompress(&bufz, max);
        status_ok = ok2;
        if ok2 {
            let n = len.min(out.len());
            b[..n].copy_from_slice(&out[..n]);
        }
    }
    if status_ok {
        let (w, h) = swf_dims(&b);
        Some(GfxInfo {
            width: w,
            height: h,
            ..Default::default()
        })
    } else {
        None
    }
}

fn handle_swf(s: &mut ImgStream) -> Option<GfxInfo> {
    if !s.seek_cur(5) {
        return None;
    }
    let mut a = [0u8; 32];
    if s.read_into(&mut a) != 32 {
        return None;
    }
    let (w, h) = swf_dims(&a);
    Some(GfxInfo {
        width: w,
        height: h,
        bits: 0,
        channels: 0,
    })
}

fn handle_png(s: &mut ImgStream) -> Option<GfxInfo> {
    if !s.seek_cur(8) {
        return None;
    }
    let mut dim = [0u8; 9];
    if s.read_into(&mut dim) < 9 {
        return None;
    }
    Some(GfxInfo {
        width: u32::from_be_bytes([dim[0], dim[1], dim[2], dim[3]]),
        height: u32::from_be_bytes([dim[4], dim[5], dim[6], dim[7]]),
        bits: dim[8] as u32,
        channels: 0,
    })
}

// JPEG markers
const M_SOF0: u32 = 0xC0;
const M_SOF1: u32 = 0xC1;
const M_SOF2: u32 = 0xC2;
const M_SOF3: u32 = 0xC3;
const M_SOF5: u32 = 0xC5;
const M_SOF6: u32 = 0xC6;
const M_SOF7: u32 = 0xC7;
const M_SOF9: u32 = 0xC9;
const M_SOF10: u32 = 0xCA;
const M_SOF11: u32 = 0xCB;
const M_SOF13: u32 = 0xCD;
const M_SOF14: u32 = 0xCE;
const M_SOF15: u32 = 0xCF;
const M_EOI: u32 = 0xD9;
const M_SOS: u32 = 0xDA;
const M_APP0: u32 = 0xE0;
const M_APP13: u32 = 0xED;
const M_APP15: u32 = 0xEF;

fn read2(s: &mut ImgStream) -> u16 {
    let mut a = [0u8; 2];
    if s.read_into(&mut a) < 2 {
        return 0;
    }
    ((a[0] as u16) << 8) + a[1] as u16
}

fn read4(s: &mut ImgStream) -> u32 {
    let mut a = [0u8; 4];
    if s.read_into(&mut a) != 4 {
        return 0;
    }
    u32::from_be_bytes(a)
}

fn next_marker(vm: &mut Vm, func: &str, s: &mut ImgStream, ff_read: bool) -> u32 {
    if !ff_read {
        let mut extraneous: usize = 0;
        loop {
            let m = s.getc();
            if m == 0xff {
                break;
            }
            if m == -1 {
                return M_EOI;
            }
            extraneous += 1;
        }
        if extraneous > 0 {
            let line = vm.current_line;
            vm.emit_warning_at(
                &format!(
                    "{}(): Corrupt JPEG data: {} extraneous bytes before marker",
                    func, extraneous
                ),
                line,
            );
        }
    }
    let mut marker;
    loop {
        marker = s.getc();
        if marker == -1 {
            return M_EOI;
        }
        if marker != 0xff {
            break;
        }
    }
    marker as u32
}

fn skip_variable(s: &mut ImgStream) -> bool {
    let length = read2(s) as i64;
    if length < 2 {
        return false;
    }
    s.seek_cur(length - 2);
    true
}

fn read_app(s: &mut ImgStream, marker: u32, info: &mut PhpArray) -> bool {
    let length = read2(s) as usize;
    if length < 2 {
        return false;
    }
    let length = length - 2;
    let got = s.read(length).to_vec();
    if got.len() != length {
        return false;
    }
    let name = format!("APP{}", marker - M_APP0);
    let key = ArrayKey::String(PhpString::from_string(name));
    if !info.contains_key(&key) {
        info.set(key, Value::String(PhpString::from_vec(got)));
    }
    true
}

fn handle_jpeg(
    vm: &mut Vm,
    func: &str,
    s: &mut ImgStream,
    mut info: Option<&mut PhpArray>,
) -> Option<GfxInfo> {
    let mut result: Option<GfxInfo> = None;
    let mut ff_read = true;
    loop {
        let marker = next_marker(vm, func, s, ff_read);
        ff_read = false;
        match marker {
            M_SOF0 | M_SOF1 | M_SOF2 | M_SOF3 | M_SOF5 | M_SOF6 | M_SOF7 | M_SOF9 | M_SOF10
            | M_SOF11 | M_SOF13 | M_SOF14 | M_SOF15 => {
                if result.is_none() {
                    let length = read2(s);
                    let bits = s.getc() as u32;
                    let height = read2(s) as u32;
                    let width = read2(s) as u32;
                    let channels = s.getc() as u32;
                    result = Some(GfxInfo {
                        width,
                        height,
                        bits,
                        channels,
                    });
                    if info.is_none() || length < 8 {
                        return result;
                    }
                    if !s.seek_cur(length as i64 - 8) {
                        return result;
                    }
                } else if !skip_variable(s) {
                    return result;
                }
            }
            m if (M_APP0..=M_APP15).contains(&m) => {
                if let Some(inf) = info.as_deref_mut() {
                    if !read_app(s, m, inf) {
                        return result;
                    }
                } else if !skip_variable(s) {
                    return result;
                }
            }
            M_SOS | M_EOI => return result,
            _ => {
                if !skip_variable(s) {
                    return result;
                }
            }
        }
    }
}

fn handle_jpc(vm: &mut Vm, func: &str, s: &mut ImgStream) -> Option<GfxInfo> {
    let first_marker_id = s.getc() as u8;
    if first_marker_id != 0x51 {
        let line = vm.current_line;
        vm.emit_warning_at(
            &format!(
                "{}(): JPEG2000 codestream corrupt(Expected SIZ marker not found after SOC)",
                func
            ),
            line,
        );
        return None;
    }
    let mut result = GfxInfo::default();
    read2(s); // Lsiz
    read2(s); // Rsiz
    result.width = read4(s);
    result.height = read4(s);
    if !s.seek_cur(24) {
        return None;
    }
    result.channels = read2(s) as u32;
    if (result.channels == 0 && s.eof) || result.channels > 256 {
        return None;
    }
    let mut highest_bit_depth: i32 = 0;
    for _ in 0..result.channels {
        let bit_depth = s.getc() + 1;
        if bit_depth > highest_bit_depth {
            highest_bit_depth = bit_depth;
        }
        s.getc();
        s.getc();
    }
    result.bits = highest_bit_depth as u32;
    Some(result)
}

fn handle_jp2(vm: &mut Vm, func: &str, s: &mut ImgStream) -> Option<GfxInfo> {
    let mut result = None;
    loop {
        let box_length = read4(s);
        let mut box_type = [0u8; 4];
        if s.read_into(&mut box_type) != 4 {
            break;
        }
        if box_length == 1 {
            return None;
        }
        if &box_type == b"jp2c" {
            s.seek_cur(3);
            result = handle_jpc(vm, func, s);
            break;
        }
        if (box_length as i32) <= 0 {
            break;
        }
        if !s.seek_cur(box_length.wrapping_sub(8) as i64) {
            break;
        }
    }
    if result.is_none() {
        let line = vm.current_line;
        vm.emit_warning_at(
            &format!("{}(): JP2 file has no codestreams at root level", func),
            line,
        );
    }
    result
}

fn ifd_get16u(b: &[u8], motorola: bool) -> u32 {
    if motorola {
        ((b[0] as u32) << 8) | b[1] as u32
    } else {
        ((b[1] as u32) << 8) | b[0] as u32
    }
}

fn ifd_get32s(b: &[u8], motorola: bool) -> i32 {
    if motorola {
        i32::from_be_bytes([b[0], b[1], b[2], b[3]])
    } else {
        i32::from_le_bytes([b[0], b[1], b[2], b[3]])
    }
}

fn handle_tiff(s: &mut ImgStream, motorola: bool) -> Option<GfxInfo> {
    let mut ifd_ptr = [0u8; 4];
    if s.read_into(&mut ifd_ptr) != 4 {
        return None;
    }
    let ifd_addr = ifd_get32s(&ifd_ptr, motorola) as u32 as i64;
    if !s.seek_cur(ifd_addr - 8) {
        return None;
    }
    let mut head = [0u8; 2];
    if s.read_into(&mut head) != 2 {
        return None;
    }
    let num_entries = ifd_get16u(&head, motorola) as usize;
    let dir_size = 2 + 12 * num_entries + 4;
    let rest = s.read(dir_size - 2).to_vec();
    if rest.len() != dir_size - 2 {
        return None;
    }
    let mut ifd = head.to_vec();
    ifd.extend_from_slice(&rest);
    let (mut width, mut height): (u64, u64) = (0, 0);
    for i in 0..num_entries {
        let e = &ifd[2 + i * 12..2 + i * 12 + 12];
        let tag = ifd_get16u(&e[0..], motorola);
        let typ = ifd_get16u(&e[2..], motorola);
        let value: u64 = match typ {
            1 | 6 => e[8] as u64,                                           // BYTE / SBYTE
            3 => ifd_get16u(&e[8..], motorola) as u64,                      // USHORT
            8 => ifd_get16u(&e[8..], motorola) as u16 as i16 as i64 as u64, // SSHORT
            4 => ifd_get32s(&e[8..], motorola) as u32 as u64,               // ULONG
            9 => ifd_get32s(&e[8..], motorola) as i64 as u64,               // SLONG
            _ => continue,
        };
        match tag {
            0x0100 | 0xA002 => width = value,
            0x0101 | 0xA003 => height = value,
            _ => {}
        }
    }
    if width != 0 && height != 0 {
        return Some(GfxInfo {
            width: width as u32,
            height: height as u32,
            bits: 0,
            channels: 0,
        });
    }
    None
}

fn handle_iff(s: &mut ImgStream) -> Option<GfxInfo> {
    let mut a = [0u8; 10];
    if s.read_into(&mut a[..8]) != 8 {
        return None;
    }
    if &a[4..8] != b"ILBM" && &a[4..8] != b"PBM " {
        return None;
    }
    loop {
        if s.read_into(&mut a[..8]) != 8 {
            return None;
        }
        let chunk_id = ifd_get32s(&a[0..], true);
        let mut size = ifd_get32s(&a[4..], true);
        if size < 0 {
            return None;
        }
        if size & 1 == 1 {
            size = size.wrapping_add(1);
        }
        if chunk_id == 0x424d4844 {
            if size < 9 || s.read_into(&mut a[..9]) != 9 {
                return None;
            }
            let width = ifd_get16u(&a[0..], true) as u16 as i16;
            let height = ifd_get16u(&a[2..], true) as u16 as i16;
            let bits = a[8] as i16;
            if width > 0 && height > 0 && bits > 0 && bits < 33 {
                return Some(GfxInfo {
                    width: width as u32,
                    height: height as u32,
                    bits: bits as u32,
                    channels: 0,
                });
            }
        } else if !s.seek_cur(size as i64) {
            return None;
        }
    }
}

/// php_get_wbmp; returns Some((w, h)) when valid.
fn get_wbmp(s: &mut ImgStream) -> Option<(i32, i32)> {
    if !s.rewind() {
        return None;
    }
    if s.getc() != 0 {
        return None;
    }
    loop {
        let i = s.getc();
        if i < 0 {
            return None;
        }
        if i & 0x80 == 0 {
            break;
        }
    }
    let mut width: i32 = 0;
    loop {
        let i = s.getc();
        if i < 0 {
            return None;
        }
        width = (width << 7) | (i & 0x7f);
        if width > 2048 {
            return None;
        }
        if i & 0x80 == 0 {
            break;
        }
    }
    let mut height: i32 = 0;
    loop {
        let i = s.getc();
        if i < 0 {
            return None;
        }
        height = (height << 7) | (i & 0x7f);
        if height > 2048 {
            return None;
        }
        if i & 0x80 == 0 {
            break;
        }
    }
    if height == 0 || width == 0 {
        return None;
    }
    Some((width, height))
}

fn is_c_space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

/// sscanf(line, "#define %s %d", name, &value) == 2
fn scan_define(line: &[u8]) -> Option<(Vec<u8>, i32)> {
    let mut p = 0usize;
    // sscanf stops at the C string terminator.
    let line = match line.iter().position(|&c| c == 0) {
        Some(n) => &line[..n],
        None => line,
    };
    if !line.starts_with(b"#define") {
        return None;
    }
    p += 7;
    while p < line.len() && is_c_space(line[p]) {
        p += 1;
    }
    let start = p;
    while p < line.len() && !is_c_space(line[p]) {
        p += 1;
    }
    if p == start {
        return None;
    }
    let name = line[start..p].to_vec();
    while p < line.len() && is_c_space(line[p]) {
        p += 1;
    }
    let mut neg = false;
    if p < line.len() && (line[p] == b'-' || line[p] == b'+') {
        neg = line[p] == b'-';
        p += 1;
    }
    let dstart = p;
    let mut v: i64 = 0;
    let mut overflow = false;
    while p < line.len() && line[p].is_ascii_digit() {
        if !overflow {
            match v
                .checked_mul(10)
                .and_then(|x| x.checked_add((line[p] - b'0') as i64))
            {
                Some(x) => v = x,
                None => overflow = true,
            }
        }
        p += 1;
    }
    if p == dstart {
        return None;
    }
    let val: i64 = if overflow {
        if neg { i64::MIN } else { i64::MAX }
    } else if neg {
        -v
    } else {
        v
    };
    Some((name, val as i32))
}

/// php_get_xbm; returns Some((w, h)) when the file looks like XBM.
fn get_xbm(s: &mut ImgStream) -> Option<(u32, u32)> {
    if !s.rewind() {
        return None;
    }
    let (mut width, mut height) = (0u32, 0u32);
    while let Some(line) = s.gets() {
        if let Some((name, value)) = scan_define(&line) {
            let typ: &[u8] = match name.iter().rposition(|&c| c == b'_') {
                Some(i) => &name[i + 1..],
                None => &name,
            };
            if typ == b"width" {
                width = value as u32;
                if height != 0 {
                    break;
                }
            }
            if typ == b"height" {
                height = value as u32;
                if width != 0 {
                    break;
                }
            }
        }
    }
    if width != 0 && height != 0 {
        Some((width, height))
    } else {
        None
    }
}

fn handle_ico(s: &mut ImgStream) -> Option<GfxInfo> {
    let mut dim = [0u8; 16];
    if s.read_into(&mut dim[..2]) != 2 {
        return None;
    }
    let mut num_icons = ((dim[1] as u32) << 8) + dim[0] as u32;
    if !(1..=255).contains(&num_icons) {
        return None;
    }
    let mut r = GfxInfo::default();
    while num_icons > 0 {
        if s.read_into(&mut dim) != 16 {
            break;
        }
        let bpp = ((dim[7] as u32) << 8) + dim[6] as u32;
        if bpp >= r.bits {
            r.width = dim[0] as u32;
            r.height = dim[1] as u32;
            r.bits = bpp;
        }
        num_icons -= 1;
    }
    if r.width == 0 {
        r.width = 256;
    }
    if r.height == 0 {
        r.height = 256;
    }
    Some(r)
}

fn handle_webp(s: &mut ImgStream) -> Option<GfxInfo> {
    let mut buf = [0u8; 18];
    if s.read_into(&mut buf) != 18 {
        return None;
    }
    if &buf[0..3] != b"VP8" {
        return None;
    }
    let b = |i: usize| buf[i] as u32;
    let (width, height) = match buf[3] {
        b' ' => (b(14) + ((b(15) & 0x3F) << 8), b(16) + ((b(17) & 0x3F) << 8)),
        b'L' => (
            b(9) + ((b(10) & 0x3F) << 8) + 1,
            (b(10) >> 6) + (b(11) << 2) + ((b(12) & 0xF) << 10) + 1,
        ),
        b'X' => (
            b(12) + (b(13) << 8) + (b(14) << 16) + 1,
            b(15) + (b(16) << 8) + (b(17) << 16) + 1,
        ),
        _ => return None,
    };
    Some(GfxInfo {
        width,
        height,
        bits: 8,
        channels: 0,
    })
}

// ---------------------------------------------------------------------------
// libavifinfo port

const AVIFINFO_MAX_NUM_READ_BYTES: u32 = 64;
const AVIFINFO_MAX_SIZE: u32 = u32::MAX;
const AVIFINFO_MAX_VALUE: u32 = u8::MAX as u32;
const AVIFINFO_MAX_TILES: usize = 16;
const AVIFINFO_MAX_PROPS: usize = 32;
const AVIFINFO_MAX_FEATURES: usize = 8;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum AvSt {
    Found,
    NotFound,
    Truncated,
    Aborted,
    Invalid,
}

/// Wraps an ImgStream like php_avif_stream: a failed read/skip poisons it.
struct AvifStream<'a> {
    s: &'a mut ImgStream,
    dead: bool,
}

impl AvifStream<'_> {
    fn read(&mut self, n: u32) -> Result<Vec<u8>, AvSt> {
        if self.dead {
            return Err(AvSt::Truncated);
        }
        debug_assert!(n <= AVIFINFO_MAX_NUM_READ_BYTES);
        let r = self.s.read(n as usize).to_vec();
        if r.len() != n as usize {
            self.dead = true;
            return Err(AvSt::Truncated);
        }
        Ok(r)
    }

    fn skip(&mut self, n: u32) -> Result<(), AvSt> {
        if n > 0 && !self.dead && !self.s.seek_cur(n as i64) {
            self.dead = true;
        }
        Ok(())
    }
}

fn be(d: &[u8], n: usize) -> u32 {
    let mut v: u32 = 0;
    for &b in &d[..n] {
        v = (v << 8) | b as u32;
    }
    v
}

#[derive(Default, Clone, Copy)]
struct AvifFeatures {
    width: u32,
    height: u32,
    bit_depth: u32,
    num_channels: u32,
}

#[derive(Default)]
struct AvifInternal {
    has_primary_item: bool,
    has_alpha: bool,
    primary_item_id: u32,
    primary: AvifFeatures,
    data_was_skipped: bool,
    tiles: Vec<(u32, u32)>,           // (tile_item_id, parent_item_id)
    props: Vec<(u32, u32)>,           // (property_index, item_id)
    dim_props: Vec<(u32, u32, u32)>,  // (property_index, width, height)
    chan_props: Vec<(u32, u32, u32)>, // (property_index, bit_depth, num_channels)
}

struct AvifBox {
    size: u32,
    typ: [u8; 4],
    version: u32,
    flags: u32,
    content_size: u32,
}

macro_rules! av_try {
    ($e:expr) => {
        match $e {
            Ok(v) => v,
            Err(st) => return st,
        }
    };
}
macro_rules! av_check {
    ($cond:expr, $st:expr) => {
        if !($cond) {
            return $st;
        }
    };
}
macro_rules! av_found {
    ($e:expr) => {{
        let st = $e;
        if st != AvSt::Found {
            return st;
        }
    }};
}
macro_rules! av_not_found {
    ($e:expr) => {{
        let st = $e;
        if st != AvSt::NotFound {
            return st;
        }
    }};
}

fn avif_get_item_features(f: &mut AvifInternal, target: u32, tile_depth: u32) -> AvSt {
    for pi in 0..f.props.len() {
        let (property_index, item_id) = f.props[pi];
        if item_id != target {
            continue;
        }
        if target == f.primary_item_id && (f.primary.width == 0 || f.primary.height == 0) {
            if let Some(&(_, w, h)) = f.dim_props.iter().find(|d| d.0 == property_index) {
                f.primary.width = w;
                f.primary.height = h;
                if f.primary.bit_depth != 0 && f.primary.num_channels != 0 {
                    return AvSt::Found;
                }
            }
        }
        if f.primary.bit_depth == 0 || f.primary.num_channels == 0 {
            if let Some(&(_, bd, nc)) = f.chan_props.iter().find(|c| c.0 == property_index) {
                f.primary.bit_depth = bd;
                f.primary.num_channels = nc;
                if f.primary.width != 0 && f.primary.height != 0 {
                    return AvSt::Found;
                }
            }
        }
    }
    if tile_depth < 3 {
        for t in 0..f.tiles.len() {
            let (tile_item, parent) = f.tiles[t];
            if parent != target {
                continue;
            }
            av_not_found!(avif_get_item_features(f, tile_item, tile_depth + 1));
        }
    }
    AvSt::NotFound
}

fn avif_get_primary_item_features(f: &mut AvifInternal) -> AvSt {
    av_check!(f.has_primary_item, AvSt::NotFound);
    av_check!(
        !f.dim_props.is_empty() && !f.chan_props.is_empty(),
        AvSt::NotFound
    );
    let id = f.primary_item_id;
    av_found!(avif_get_item_features(f, id, 0));
    if f.has_alpha {
        f.primary.num_channels += 1;
    }
    AvSt::Found
}

fn avif_parse_box(
    st: &mut AvifStream,
    num_remaining: u32,
    num_parsed: &mut u32,
) -> Result<AvifBox, AvSt> {
    let mut header_size: u32 = 8;
    if header_size > num_remaining {
        return Err(AvSt::Invalid);
    }
    let data = st.read(8)?;
    let mut size = be(&data, 4);
    let mut typ = [data[4], data[5], data[6], data[7]];
    if size == 1 {
        header_size += 8;
        if header_size > num_remaining {
            return Err(AvSt::Invalid);
        }
        let d = st.read(8)?;
        if be(&d, 4) != 0 {
            return Err(AvSt::Aborted);
        }
        size = be(&d[4..], 4);
    } else if size == 0 {
        size = num_remaining;
    }
    if size < header_size || size > num_remaining {
        return Err(AvSt::Invalid);
    }
    let full = matches!(
        &typ,
        b"meta" | b"pitm" | b"ipma" | b"ispe" | b"pixi" | b"iref" | b"auxC"
    );
    if full {
        header_size += 4;
    }
    if size < header_size {
        return Err(AvSt::Invalid);
    }
    let content_size = size - header_size;
    *num_parsed += 1;
    if *num_parsed >= 4096 {
        return Err(AvSt::Aborted);
    }
    let (mut version, mut flags) = (0, 0);
    if full {
        let d = st.read(4)?;
        version = be(&d, 1);
        flags = be(&d[1..], 3);
        let parsable = match &typ {
            b"meta" => version == 0,
            b"pitm" => version <= 1,
            b"ipma" => version <= 1,
            b"ispe" => version == 0,
            b"pixi" => version == 0,
            b"iref" => version <= 1,
            b"auxC" => version == 0,
            _ => true,
        };
        if !parsable {
            typ = *b"\0skp";
        }
    }
    Ok(AvifBox {
        size,
        typ,
        version,
        flags,
        content_size,
    })
}

fn avif_parse_ipco(
    st: &mut AvifStream,
    mut remaining: u32,
    np: &mut u32,
    f: &mut AvifInternal,
) -> AvSt {
    let mut box_index: u32 = 1;
    loop {
        let bx = av_try!(avif_parse_box(st, remaining, np));
        match &bx.typ {
            b"ispe" => {
                av_check!(bx.content_size >= 8, AvSt::Invalid);
                let d = av_try!(st.read(8));
                let w = be(&d, 4);
                let h = be(&d[4..], 4);
                av_check!(w != 0 && h != 0, AvSt::Invalid);
                if f.dim_props.len() < AVIFINFO_MAX_FEATURES && box_index <= AVIFINFO_MAX_VALUE {
                    f.dim_props.push((box_index, w, h));
                } else {
                    f.data_was_skipped = true;
                }
                av_try!(st.skip(bx.content_size - 8));
            }
            b"pixi" => {
                av_check!(bx.content_size >= 1, AvSt::Invalid);
                let d = av_try!(st.read(1));
                let num_channels = d[0] as u32;
                av_check!(num_channels >= 1, AvSt::Invalid);
                av_check!(bx.content_size > num_channels, AvSt::Invalid);
                let d = av_try!(st.read(1));
                let bit_depth = d[0] as u32;
                av_check!(bit_depth >= 1, AvSt::Invalid);
                for i in 1..num_channels {
                    let d = av_try!(st.read(1));
                    av_check!(d[0] as u32 == bit_depth, AvSt::Invalid);
                    av_check!(i <= 32, AvSt::Aborted);
                }
                if f.chan_props.len() < AVIFINFO_MAX_FEATURES
                    && box_index <= AVIFINFO_MAX_VALUE
                    && bit_depth <= AVIFINFO_MAX_VALUE
                    && num_channels <= AVIFINFO_MAX_VALUE
                {
                    f.chan_props.push((box_index, bit_depth, num_channels));
                } else {
                    f.data_was_skipped = true;
                }
                av_try!(st.skip(bx.content_size - (1 + num_channels)));
            }
            b"av1C" => {
                av_check!(bx.content_size >= 3, AvSt::Invalid);
                let d = av_try!(st.read(3));
                let high = d[2] & 0x40 != 0;
                let twelve = d[2] & 0x20 != 0;
                let mono = d[2] & 0x10 != 0;
                if twelve {
                    av_check!(high, AvSt::Invalid);
                }
                if f.chan_props.len() < AVIFINFO_MAX_FEATURES && box_index <= AVIFINFO_MAX_VALUE {
                    let bd = if high {
                        if twelve { 12 } else { 10 }
                    } else {
                        8
                    };
                    f.chan_props.push((box_index, bd, if mono { 1 } else { 3 }));
                } else {
                    f.data_was_skipped = true;
                }
                av_try!(st.skip(bx.content_size - 3));
            }
            b"auxC" => {
                const ALPHA: &[u8] = b"urn:mpeg:mpegB:cicp:systems:auxiliary:alpha";
                const ALPHA_LEN: u32 = 44;
                if bx.content_size >= ALPHA_LEN {
                    // Read in two chunks of at most 64 bytes (44 fits in one).
                    let d = av_try!(st.read(ALPHA_LEN));
                    let z = d.iter().position(|&c| c == 0).unwrap_or(d.len());
                    if &d[..z] == ALPHA {
                        f.has_alpha = true;
                    }
                    av_try!(st.skip(bx.content_size - ALPHA_LEN));
                } else {
                    av_try!(st.skip(bx.content_size));
                }
            }
            _ => {
                av_try!(st.skip(bx.content_size));
            }
        }
        box_index += 1;
        remaining -= bx.size;
        if remaining == 0 {
            break;
        }
    }
    AvSt::NotFound
}

fn avif_parse_iprp(
    st: &mut AvifStream,
    mut remaining: u32,
    np: &mut u32,
    f: &mut AvifInternal,
) -> AvSt {
    loop {
        let bx = av_try!(avif_parse_box(st, remaining, np));
        match &bx.typ {
            b"ipco" => {
                av_not_found!(avif_parse_ipco(st, bx.content_size, np, f));
            }
            b"ipma" => {
                let mut num_read: u32 = 4;
                av_check!(bx.content_size >= num_read, AvSt::Invalid);
                let d = av_try!(st.read(4));
                let entry_count = be(&d, 4);
                let id_num_bytes: u32 = if bx.version < 1 { 2 } else { 4 };
                let index_num_bytes: u32 = if bx.flags & 1 != 0 { 2 } else { 1 };
                let essential_mask: u32 = if bx.flags & 1 != 0 { 0x8000 } else { 0x80 };
                let mut entry: u32 = 0;
                while entry < entry_count {
                    if entry as usize >= AVIFINFO_MAX_PROPS || f.props.len() >= AVIFINFO_MAX_PROPS {
                        f.data_was_skipped = true;
                        break;
                    }
                    num_read += id_num_bytes + 1;
                    av_check!(bx.content_size >= num_read, AvSt::Invalid);
                    let d = av_try!(st.read(id_num_bytes + 1));
                    let item_id = be(&d, id_num_bytes as usize);
                    let assoc_count = be(&d[id_num_bytes as usize..], 1);
                    let mut property: u32 = 0;
                    while property < assoc_count {
                        if property as usize >= AVIFINFO_MAX_PROPS
                            || f.props.len() >= AVIFINFO_MAX_PROPS
                        {
                            f.data_was_skipped = true;
                            break;
                        }
                        num_read += index_num_bytes;
                        av_check!(bx.content_size >= num_read, AvSt::Invalid);
                        let d = av_try!(st.read(index_num_bytes));
                        let value = be(&d, index_num_bytes as usize);
                        let property_index = value & !essential_mask;
                        if property_index <= AVIFINFO_MAX_VALUE && item_id <= AVIFINFO_MAX_VALUE {
                            f.props.push((property_index, item_id));
                        } else {
                            f.data_was_skipped = true;
                        }
                        property += 1;
                    }
                    if property < assoc_count {
                        break;
                    }
                    entry += 1;
                }
                av_not_found!(avif_get_primary_item_features(f));
                av_try!(st.skip(bx.content_size - num_read));
            }
            _ => {
                av_try!(st.skip(bx.content_size));
            }
        }
        remaining -= bx.size;
        if remaining == 0 {
            break;
        }
    }
    AvSt::NotFound
}

fn avif_parse_iref(
    st: &mut AvifStream,
    mut remaining: u32,
    np: &mut u32,
    f: &mut AvifInternal,
) -> AvSt {
    loop {
        let bx = av_try!(avif_parse_box(st, remaining, np));
        if &bx.typ == b"dimg" {
            let nb: u32 = if bx.version == 0 { 2 } else { 4 };
            let mut num_read = nb + 2;
            av_check!(bx.content_size >= num_read, AvSt::Invalid);
            let d = av_try!(st.read(nb + 2));
            let from_item = be(&d, nb as usize);
            let ref_count = be(&d[nb as usize..], 2);
            for i in 0..ref_count {
                if i as usize >= AVIFINFO_MAX_TILES {
                    f.data_was_skipped = true;
                    break;
                }
                num_read += nb;
                av_check!(bx.content_size >= num_read, AvSt::Invalid);
                let d = av_try!(st.read(nb));
                let to_item = be(&d, nb as usize);
                if from_item <= AVIFINFO_MAX_VALUE
                    && to_item <= AVIFINFO_MAX_VALUE
                    && f.tiles.len() < AVIFINFO_MAX_TILES
                {
                    f.tiles.push((to_item, from_item));
                } else {
                    f.data_was_skipped = true;
                }
            }
            av_not_found!(avif_get_primary_item_features(f));
        } else {
            av_try!(st.skip(bx.content_size));
        }
        remaining -= bx.size;
        if remaining == 0 {
            break;
        }
    }
    AvSt::NotFound
}

fn avif_parse_meta(
    st: &mut AvifStream,
    mut remaining: u32,
    np: &mut u32,
    f: &mut AvifInternal,
) -> AvSt {
    loop {
        let bx = av_try!(avif_parse_box(st, remaining, np));
        match &bx.typ {
            b"pitm" => {
                let nb: u32 = if bx.version == 0 { 2 } else { 4 };
                av_check!(nb <= remaining, AvSt::Invalid);
                let d = av_try!(st.read(nb));
                let id = be(&d, nb as usize);
                av_check!(id <= AVIFINFO_MAX_VALUE, AvSt::Aborted);
                f.has_primary_item = true;
                f.primary_item_id = id;
                av_try!(st.skip(bx.content_size.wrapping_sub(nb)));
            }
            b"iprp" => {
                av_not_found!(avif_parse_iprp(st, bx.content_size, np, f));
            }
            b"iref" => {
                av_not_found!(avif_parse_iref(st, bx.content_size, np, f));
            }
            _ => {
                av_try!(st.skip(bx.content_size));
            }
        }
        remaining -= bx.size;
        if remaining == 0 {
            break;
        }
    }
    if f.data_was_skipped {
        AvSt::Aborted
    } else {
        AvSt::Invalid
    }
}

fn avif_parse_ftyp(st: &mut AvifStream) -> AvSt {
    let mut np = 0;
    let bx = av_try!(avif_parse_box(st, AVIFINFO_MAX_SIZE, &mut np));
    av_check!(&bx.typ == b"ftyp", AvSt::Invalid);
    av_check!(bx.content_size >= 8, AvSt::Invalid);
    let mut i: u32 = 0;
    while i + 4 <= bx.content_size {
        let d = av_try!(st.read(4));
        if i != 4 {
            if &d[..] == b"avif" || &d[..] == b"avis" {
                av_try!(st.skip(bx.content_size - (i + 4)));
                return AvSt::Found;
            }
            av_check!(i <= 32 * 4, AvSt::Aborted);
        }
        i += 4;
    }
    AvSt::Invalid
}

fn avif_parse_file(st: &mut AvifStream, np: &mut u32, f: &mut AvifInternal) -> AvSt {
    loop {
        let bx = av_try!(avif_parse_box(st, AVIFINFO_MAX_SIZE, np));
        if &bx.typ == b"meta" {
            return avif_parse_meta(st, bx.content_size, np, f);
        }
        av_try!(st.skip(bx.content_size));
    }
}

fn is_image_avif(s: &mut ImgStream) -> bool {
    let mut st = AvifStream { s, dead: false };
    avif_parse_ftyp(&mut st) == AvSt::Found
}

/// php_is_image_avif() on in-memory data (ext/gd's _php_image_type()).
pub fn is_avif_data(data: &[u8]) -> bool {
    is_image_avif(&mut ImgStream::new(data.to_vec()))
}

fn handle_avif(s: &mut ImgStream) -> Option<GfxInfo> {
    let mut st = AvifStream { s, dead: false };
    let mut np = 0;
    let mut f = AvifInternal::default();
    if avif_parse_file(&mut st, &mut np, &mut f) == AvSt::Found {
        Some(GfxInfo {
            width: f.primary.width,
            height: f.primary.height,
            bits: f.primary.bit_depth,
            channels: f.primary.num_channels,
        })
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Type detection

fn getimagetype(vm: &mut Vm, func: &str, s: &mut ImgStream, input: &[u8]) -> i64 {
    let input_disp = {
        let z = input.iter().position(|&c| c == 0).unwrap_or(input.len());
        String::from_utf8_lossy(&input[..z]).into_owned()
    };
    let err_reading = |vm: &mut Vm| {
        let line = vm.current_line;
        vm.emit_notice_at(
            &format!("{}(): Error reading from {}!", func, input_disp),
            line,
        );
    };
    let mut ft = [0u8; 12];
    if s.read_into(&mut ft[..3]) != 3 {
        err_reading(vm);
        return IMAGE_FILETYPE_UNKNOWN;
    }
    const PNG: [u8; 8] = [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a];
    const JP2: [u8; 12] = [
        0x00, 0x00, 0x00, 0x0c, 0x6a, 0x50, 0x20, 0x20, 0x0d, 0x0a, 0x87, 0x0a,
    ];
    if &ft[..3] == b"GIF" {
        return IMAGE_FILETYPE_GIF;
    } else if ft[..3] == [0xff, 0xd8, 0xff] {
        return IMAGE_FILETYPE_JPEG;
    } else if ft[..3] == PNG[..3] {
        if s.read_into(&mut ft[3..8]) != 5 {
            err_reading(vm);
            return IMAGE_FILETYPE_UNKNOWN;
        }
        if ft[..8] == PNG {
            return IMAGE_FILETYPE_PNG;
        }
        let line = vm.current_line;
        vm.emit_warning_at(
            &format!("{}(): PNG file corrupted by ASCII conversion", func),
            line,
        );
        return IMAGE_FILETYPE_UNKNOWN;
    } else if &ft[..3] == b"FWS" {
        return IMAGE_FILETYPE_SWF;
    } else if &ft[..3] == b"CWS" {
        return IMAGE_FILETYPE_SWC;
    } else if &ft[..3] == b"8BP" {
        return IMAGE_FILETYPE_PSD;
    } else if &ft[..2] == b"BM" {
        return IMAGE_FILETYPE_BMP;
    } else if ft[..3] == [0xff, 0x4f, 0xff] {
        return IMAGE_FILETYPE_JPC;
    } else if &ft[..3] == b"RIF" {
        if s.read_into(&mut ft[3..12]) != 9 {
            err_reading(vm);
            return IMAGE_FILETYPE_UNKNOWN;
        }
        if &ft[8..12] == b"WEBP" {
            return IMAGE_FILETYPE_WEBP;
        }
        return IMAGE_FILETYPE_UNKNOWN;
    }

    if s.read_into(&mut ft[3..4]) != 1 {
        err_reading(vm);
        return IMAGE_FILETYPE_UNKNOWN;
    }
    if &ft[..4] == b"II\x2a\x00" {
        return IMAGE_FILETYPE_TIFF_II;
    } else if &ft[..4] == b"MM\x00\x2a" {
        return IMAGE_FILETYPE_TIFF_MM;
    } else if &ft[..4] == b"FORM" {
        return IMAGE_FILETYPE_IFF;
    } else if ft[..4] == [0, 0, 1, 0] {
        return IMAGE_FILETYPE_ICO;
    }

    let twelve = s.read_into(&mut ft[4..12]) == 8;
    if twelve && ft == JP2 {
        return IMAGE_FILETYPE_JP2;
    }
    if s.rewind() && is_image_avif(s) {
        return IMAGE_FILETYPE_AVIF;
    }
    if twelve
        && &ft[4..8] == b"ftyp"
        && (&ft[8..12] == b"mif1" || &ft[8..12] == b"heic" || &ft[8..12] == b"heix")
    {
        return IMAGE_FILETYPE_HEIF;
    }
    if get_wbmp(s).is_some() {
        return IMAGE_FILETYPE_WBMP;
    }
    if !twelve {
        err_reading(vm);
        return IMAGE_FILETYPE_UNKNOWN;
    }
    if get_xbm(s).is_some() {
        return IMAGE_FILETYPE_XBM;
    }
    // php_image_handlers registered by extensions
    if let Some(t) = goro_core::image_handlers::identify(&s.data) {
        return t;
    }
    IMAGE_FILETYPE_UNKNOWN
}

fn getimagesize_from_stream(
    vm: &mut Vm,
    func: &str,
    s: &mut ImgStream,
    input: &[u8],
    info: Option<&mut PhpArray>,
) -> Value {
    let itype = getimagetype(vm, func, s, input);
    // width_unit / height_unit of a registered handler's result
    let mut units: (Option<String>, Option<String>) = (None, None);
    let result = match itype {
        IMAGE_FILETYPE_GIF => handle_gif(s),
        IMAGE_FILETYPE_JPEG => handle_jpeg(vm, func, s, info),
        IMAGE_FILETYPE_PNG => handle_png(s),
        IMAGE_FILETYPE_SWF => handle_swf(s),
        IMAGE_FILETYPE_SWC => handle_swc(s),
        IMAGE_FILETYPE_PSD => handle_psd(s),
        IMAGE_FILETYPE_BMP => handle_bmp(s),
        IMAGE_FILETYPE_TIFF_II => handle_tiff(s, false),
        IMAGE_FILETYPE_TIFF_MM => handle_tiff(s, true),
        IMAGE_FILETYPE_JPC => handle_jpc(vm, func, s),
        IMAGE_FILETYPE_JP2 => handle_jp2(vm, func, s),
        IMAGE_FILETYPE_IFF => handle_iff(s),
        IMAGE_FILETYPE_WBMP => get_wbmp(s).map(|(w, h)| GfxInfo {
            width: w as u32,
            height: h as u32,
            ..Default::default()
        }),
        IMAGE_FILETYPE_XBM => get_xbm(s).map(|(w, h)| GfxInfo {
            width: w,
            height: h,
            ..Default::default()
        }),
        IMAGE_FILETYPE_ICO => handle_ico(s),
        IMAGE_FILETYPE_WEBP => handle_webp(s),
        IMAGE_FILETYPE_AVIF => handle_avif(s),
        IMAGE_FILETYPE_HEIF => {
            if s.rewind() {
                handle_avif(s)
            } else {
                None
            }
        }
        t => goro_core::image_handlers::find(t)
            .and_then(|h| (h.get_info)(&s.data))
            .map(|i| {
                units = (i.width_unit, i.height_unit);
                GfxInfo {
                    width: i.width,
                    height: i.height,
                    ..Default::default()
                }
            }),
    };
    let r = match result {
        Some(r) => r,
        None => return Value::False,
    };
    let mut arr = PhpArray::new();
    let sk = |k: &[u8]| ArrayKey::String(PhpString::from_bytes(k));
    arr.set(ArrayKey::Int(0), Value::Long(r.width as i64));
    arr.set(ArrayKey::Int(1), Value::Long(r.height as i64));
    arr.set(ArrayKey::Int(2), Value::Long(itype));
    let px = |u: &Option<String>| u.as_deref().is_none_or(|u| u == "px");
    if px(&units.0) && px(&units.1) {
        arr.set(
            ArrayKey::Int(3),
            Value::String(PhpString::from_string(format!(
                "width=\"{}\" height=\"{}\"",
                r.width as i32, r.height as i32
            ))),
        );
    }
    if r.bits != 0 {
        arr.set(sk(b"bits"), Value::Long(r.bits as i64));
    }
    if r.channels != 0 {
        arr.set(sk(b"channels"), Value::Long(r.channels as i64));
    }
    arr.set(
        sk(b"mime"),
        Value::String(PhpString::from_bytes(
            php_image_type_to_mime_type(itype).as_bytes(),
        )),
    );
    arr.set(
        sk(b"width_unit"),
        Value::String(PhpString::from_string(
            units.0.clone().unwrap_or_else(|| "px".to_string()),
        )),
    );
    arr.set(
        sk(b"height_unit"),
        Value::String(PhpString::from_string(
            units.1.clone().unwrap_or_else(|| "px".to_string()),
        )),
    );
    Value::Array(Rc::new(RefCell::new(arr)))
}

/// Opens `path` for reading ("rb") and returns its whole content, emitting
/// the same warnings as php_stream_open_wrapper(REPORT_ERRORS).
fn open_and_read(vm: &mut Vm, func: &str, path: &str) -> Option<Vec<u8>> {
    let lower = path.to_ascii_lowercase();
    let plain = if let Some(rest) = lower.strip_prefix("file://") {
        let _ = rest;
        Some(path[7..].to_string())
    } else if !path.contains("://") && !lower.starts_with("data:") {
        Some(path.to_string())
    } else {
        None
    };
    match plain {
        Some(p) => {
            if !vm.check_open_basedir(func, &p) {
                let line = vm.current_line;
                vm.emit_warning_at(
                    &format!(
                        "{}({}): Failed to open stream: Operation not permitted",
                        func, path
                    ),
                    line,
                );
                return None;
            }
            let np = crate::misc::normalize_path_lexical(&p);
            match std::fs::File::open(&np) {
                Ok(mut f) => {
                    if std::fs::metadata(&np).map(|m| m.is_dir()).unwrap_or(false) {
                        let line = vm.current_line;
                        vm.emit_notice_at(
                            &format!(
                                "{}(): Read of 8192 bytes failed with errno=21 Is a directory",
                                func
                            ),
                            line,
                        );
                        return Some(Vec::new());
                    }
                    use std::io::Read;
                    let mut data = Vec::new();
                    let _ = f.read_to_end(&mut data);
                    Some(data)
                }
                Err(e) => {
                    let msg = crate::misc::strip_os_error(&e.to_string());
                    let line = vm.current_line;
                    vm.emit_warning_at(
                        &format!("{}({}): Failed to open stream: {}", func, path, msg),
                        line,
                    );
                    None
                }
            }
        }
        None => {
            // Other wrappers go through the generic fopen machinery.
            let fargs = [
                Value::String(PhpString::from_string(path.to_string())),
                Value::String(PhpString::from_bytes(b"rb")),
            ];
            // (through the registered functions: userspace wrappers)
            let f = |vm: &Vm, n: &[u8], d: goro_core::vm::BuiltinFn| {
                vm.functions.get(n).copied().unwrap_or(d)
            };
            let fopen = f(vm, b"fopen", crate::misc::fopen_fn);
            let fp = fopen(vm, &fargs).ok()?;
            let fid = match fp {
                Value::Resource(n) => n.id(),
                Value::Long(n) => n,
                _ => return None,
            };
            let get = f(
                vm,
                b"stream_get_contents",
                crate::misc::stream_get_contents_fn,
            );
            let data = get(vm, &[Value::resource(fid)]).ok();
            let fclose = f(vm, b"fclose", crate::misc::fclose_fn);
            let _ = fclose(vm, &[Value::resource(fid)]);
            match data {
                Some(Value::String(s)) => Some(s.as_bytes().to_vec()),
                _ => Some(Vec::new()),
            }
        }
    }
}

fn getimagesize_from_any(vm: &mut Vm, args: &[Value], from_path: bool) -> Result<Value, VmError> {
    let func = if from_path {
        "getimagesize"
    } else {
        "getimagesizefromstring"
    };
    let pname = if from_path { "filename" } else { "string" };
    let input = zpp_str(vm, func, 1, pname, args.first())?;
    if from_path && input.as_bytes().contains(&0) {
        return Err(throw(
            vm,
            b"ValueError",
            format!(
                "{}(): Argument #1 (${}) must not contain any null bytes",
                func, pname
            ),
        ));
    }
    let info_ref = if args.len() >= 2 {
        match &args[1] {
            Value::Reference(r) => {
                // zend_try_array_init(): the old value is released (its
                // destructor sees NULL there) before the array is stored.
                let empty = Value::Array(Rc::new(RefCell::new(PhpArray::new())));
                // (a destructor that threw: the stream is not opened, as
                // php_stream_display_wrapper_errors() stays silent then)
                if vm.try_assign_ref(r, empty).is_err() || vm.current_exception.is_some() {
                    return Err(VmError {
                        message: String::new(),
                        line: vm.current_line,
                    });
                }
                Some(r.clone())
            }
            _ => None,
        }
    } else {
        None
    };
    let want_info = args.len() >= 2;

    let data = if from_path {
        if input.as_bytes().is_empty() {
            return Err(throw(vm, b"ValueError", "Path must not be empty".to_string()));
        }
        match open_and_read(vm, func, &input.to_string_lossy()) {
            Some(d) => d,
            None => return Ok(Value::False),
        }
    } else {
        input.as_bytes().to_vec()
    };
    let mut stream = ImgStream::new(data);
    let mut info_arr = PhpArray::new();
    let ret = getimagesize_from_stream(
        vm,
        func,
        &mut stream,
        input.as_bytes(),
        if want_info { Some(&mut info_arr) } else { None },
    );
    if let Some(r) = info_ref {
        if !info_arr.is_empty() {
            *r.borrow_mut() = Value::Array(Rc::new(RefCell::new(info_arr)));
        }
    }
    Ok(ret)
}

fn getimagesize_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    // php_stream_open_wrapper(..., NULL): the stream gets no context
    let saved = std::mem::replace(&mut vm.open_context_mode, goro_core::vm::OPEN_CTX_NONE);
    let r = getimagesize_from_any(vm, args, true);
    vm.open_context_mode = saved;
    r
}

fn getimagesizefromstring_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    getimagesize_from_any(vm, args, false)
}

// ---------------------------------------------------------------------------
// iptc.c

struct IptcCtx<'a> {
    fp: &'a [u8],
    fpos: usize,
    spool: i64,
    spoolbuf: Option<Vec<u8>>,
    spool_cap: usize,
    out: Vec<u8>,
}

const EOF: i32 = -1;

impl IptcCtx<'_> {
    fn getc(&mut self) -> i32 {
        if self.fpos >= self.fp.len() {
            return EOF;
        }
        let c = self.fp[self.fpos];
        self.fpos += 1;
        c as i32
    }

    fn put1(&mut self, spool: i64, use_buf: bool, c: u8) -> i32 {
        if spool > 0 {
            self.out.push(c);
        }
        if use_buf {
            if let Some(buf) = self.spoolbuf.as_mut() {
                if buf.len() >= self.spool_cap {
                    return EOF;
                }
                buf.push(c);
            }
        }
        c as i32
    }

    fn get1(&mut self, spool: i64, use_buf: bool) -> i32 {
        let c = self.getc();
        if c == EOF {
            return EOF;
        }
        if spool > 0 {
            self.out.push(c as u8);
        }
        if use_buf {
            if let Some(buf) = self.spoolbuf.as_mut() {
                if buf.len() >= self.spool_cap {
                    return EOF;
                }
                buf.push(c as u8);
            }
        }
        c
    }

    fn read_remaining(&mut self, spool: i64, use_buf: bool) -> u32 {
        while self.get1(spool, use_buf) != EOF {}
        M_EOI
    }

    fn skip_variable(&mut self, spool: i64, use_buf: bool) -> u32 {
        let c1 = self.get1(spool, use_buf);
        if c1 == EOF {
            return M_EOI;
        }
        let c2 = self.get1(spool, use_buf);
        if c2 == EOF {
            return M_EOI;
        }
        let mut length: u32 = (((c1 as u8) as u32) << 8) + (c2 as u8) as u32;
        length = length.wrapping_sub(2);
        while length != 0 {
            length -= 1;
            if self.get1(spool, use_buf) == EOF {
                return M_EOI;
            }
        }
        0
    }

    fn next_marker(&mut self, spool: i64, use_buf: bool) -> u32 {
        let mut c = self.get1(spool, use_buf);
        if c == EOF {
            return M_EOI;
        }
        while c != 0xff {
            c = self.get1(spool, use_buf);
            if c == EOF {
                return M_EOI;
            }
        }
        loop {
            c = self.get1(0, false);
            if c == EOF {
                return M_EOI;
            } else if c == 0xff {
                self.put1(spool, use_buf, c as u8);
            }
            if c != 0xff {
                break;
            }
        }
        c as u32
    }
}

fn iptcembed_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let iptcdata = zpp_str(vm, "iptcembed", 1, "iptc_data", args.first())?;
    let jpeg_file = zpp_str(vm, "iptcembed", 2, "filename", args.get(1))?;
    if jpeg_file.as_bytes().contains(&0) {
        return Err(throw(
            vm,
            b"ValueError",
            "iptcembed(): Argument #2 ($filename) must not contain any null bytes".to_string(),
        ));
    }
    let spool = if args.len() > 2 {
        zpp_long(vm, "iptcembed", 3, "spool", args.get(2))?
    } else {
        0
    };
    let path = jpeg_file.to_string_lossy().to_string();
    if !vm.check_open_basedir("iptcembed", &path) {
        return Ok(Value::False);
    }
    let file = match std::fs::read(crate::misc::normalize_path_lexical(&path)) {
        Ok(d) if !std::path::Path::new(&path).is_dir() => d,
        _ => {
            let line = vm.current_line;
            vm.emit_warning_at(&format!("iptcembed(): Unable to open {}", path), line);
            return Ok(Value::False);
        }
    };
    let st_size = std::fs::metadata(crate::misc::normalize_path_lexical(&path))
        .map(|m| m.len() as usize)
        .unwrap_or(0);
    let mut iptc = iptcdata.as_bytes().to_vec();
    let mut iptcdata_len = iptc.len();
    let mut psheader: [u8; 28] = *b"\xFF\xED\0\0Photoshop 3.0\x008BIM\x04\x04\0\0\0\0";
    let use_buf = spool < 2;
    let mut ctx = IptcCtx {
        fp: &file,
        fpos: 0,
        spool,
        spoolbuf: if use_buf { Some(Vec::new()) } else { None },
        // fstat()'s st_size: 0 for a pipe, whatever is read from it
        spool_cap: iptcdata_len + 29 + 1024 + 1 + st_size,
        out: Vec::new(),
    };
    let sp = ctx.spool;
    let mut ok = true;
    if ctx.get1(sp, use_buf) != 0xFF || ctx.get1(sp, use_buf) != 0xD8 {
        ok = false;
    }
    let mut written = false;
    let mut done = false;
    while ok && !done {
        let marker = ctx.next_marker(sp, use_buf);
        if marker == M_EOI {
            break;
        } else if marker != M_APP13 && ctx.put1(sp, use_buf, marker as u8) < 0 {
            ok = false;
            break;
        }
        match marker {
            M_APP13 => {
                ctx.skip_variable(0, false);
                ctx.getc();
                ctx.read_remaining(sp, use_buf);
                done = true;
            }
            0xE0 | 0xE1 => {
                if written {
                    continue;
                }
                written = true;
                ctx.skip_variable(sp, use_buf);
                if iptcdata_len & 1 == 1 {
                    iptcdata_len += 1;
                    iptc.push(0);
                }
                psheader[2] = ((iptcdata_len + 28) >> 8) as u8;
                psheader[3] = ((iptcdata_len + 28) & 0xff) as u8;
                for &b in psheader.iter() {
                    if ctx.put1(sp, use_buf, b) < 0 {
                        ok = false;
                        break;
                    }
                }
                if !ok
                    || ctx.put1(sp, use_buf, (iptcdata_len >> 8) as u8) < 0
                    || ctx.put1(sp, use_buf, (iptcdata_len & 0xff) as u8) < 0
                {
                    ok = false;
                    break;
                }
                for &b in iptc.iter().take(iptcdata_len) {
                    if ctx.put1(sp, use_buf, b) < 0 {
                        ok = false;
                        break;
                    }
                }
            }
            M_SOS => {
                ctx.read_remaining(sp, use_buf);
                done = true;
            }
            _ => {
                ctx.skip_variable(sp, use_buf);
            }
        }
    }
    let out = std::mem::take(&mut ctx.out);
    let spoolbuf = ctx.spoolbuf.take();
    if !out.is_empty() {
        vm.write_output(&out);
    }
    if !ok {
        return Ok(Value::False);
    }
    if use_buf {
        Ok(Value::String(PhpString::from_vec(
            spoolbuf.unwrap_or_default(),
        )))
    } else {
        Ok(Value::True)
    }
}

fn iptcparse_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = zpp_str(vm, "iptcparse", 1, "iptc_block", args.first())?;
    let buf = s.as_bytes();
    let str_len = buf.len();
    let at = |i: usize| -> u8 { buf.get(i).copied().unwrap_or(0) };
    let mut inx = 0usize;
    while inx < str_len {
        if at(inx) == 0x1c && (at(inx + 1) == 0x01 || at(inx + 1) == 0x02) {
            break;
        }
        inx += 1;
    }
    let mut tagsfound = 0u32;
    let mut ret = PhpArray::new();
    while inx < str_len {
        let c = at(inx);
        inx += 1;
        if c != 0x1c {
            break;
        }
        if inx + 4 >= str_len {
            break;
        }
        let dataset = at(inx);
        inx += 1;
        let recnum = at(inx);
        inx += 1;
        let len: usize;
        if at(inx) & 0x80 != 0 {
            if inx + 6 >= str_len {
                break;
            }
            len = ((at(inx + 2) as usize) << 24)
                + ((at(inx + 3) as usize) << 16)
                + ((at(inx + 4) as usize) << 8)
                + at(inx + 5) as usize;
            inx += 6;
        } else {
            len = ((at(inx) as usize) << 8) | at(inx + 1) as usize;
            inx += 2;
        }
        if len > str_len || inx + len > str_len {
            break;
        }
        let key = ArrayKey::String(PhpString::from_string(format!("{}#{:03}", dataset, recnum)));
        let val = Value::String(PhpString::from_vec(buf[inx..inx + len].to_vec()));
        match ret.get(&key) {
            Some(Value::Array(a)) => a.borrow_mut().push(val),
            _ => {
                let mut a = PhpArray::new();
                a.push(val);
                ret.set(key, Value::Array(Rc::new(RefCell::new(a))));
            }
        }
        inx += len;
        tagsfound += 1;
    }
    if tagsfound == 0 {
        return Ok(Value::False);
    }
    Ok(Value::Array(Rc::new(RefCell::new(ret))))
}
