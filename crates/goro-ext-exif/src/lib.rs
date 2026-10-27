//! php-src's ext/exif (exif.c): exif_read_data(), exif_thumbnail(),
//! exif_imagetype(), exif_tagname() and the exif.* INI settings.

use std::cell::RefCell;
use std::rc::Rc;

use goro_core::array::{ArrayKey, PhpArray};
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};
use goro_ext_standard::ext_io;

mod parse;
mod tables;

use parse::*;

/// The extension name (`extension_loaded('exif')`).
pub const NAME: &[u8] = b"exif";

type R = Result<Value, VmError>;

const FUNCTIONS: &[(&str, goro_core::vm::BuiltinFn)] = &[
    ("exif_tagname", exif_tagname),
    ("exif_read_data", exif_read_data),
    ("exif_thumbnail", exif_thumbnail),
    ("exif_imagetype", exif_imagetype),
];

const INI: &[(&str, &str, bool)] = &[
    ("exif.encode_unicode", "ISO-8859-15", true),
    ("exif.decode_unicode_motorola", "UCS-2BE", false),
    ("exif.decode_unicode_intel", "UCS-2LE", false),
    ("exif.encode_jis", "", true),
    ("exif.decode_jis_motorola", "JIS", false),
    ("exif.decode_jis_intel", "JIS", false),
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
        vm.zpp_extension_verified.insert(
            name.as_bytes().to_vec(),
            goro_core::arginfo::CHECK_ARITY | goro_core::arginfo::CHECK_TYPES,
        );
    }
    // USE_MBSTRING: mbstring is always there
    let use_mb = vm.loaded_extensions.contains(&b"mbstring"[..]) as i64;
    vm.constants
        .insert(b"EXIF_USE_MBSTRING".to_vec(), Value::Long(use_mb));
    vm.module_constants
        .push((NAME.to_vec(), vec![b"EXIF_USE_MBSTRING".to_vec()]));
    for &(k, def, encode) in INI {
        vm.constants
            .entry(k.as_bytes().to_vec())
            .or_insert_with(|| Value::String(PhpString::from_bytes(def.as_bytes())));
        vm.ini_validators.insert(
            k.as_bytes().to_vec(),
            if encode {
                on_update_encode
            } else {
                on_update_decode
            },
        );
    }
}

/// OnUpdateEncode / OnUpdateDecode: zend_multibyte_parse_encoding_list()
fn check_encoding(vm: &mut Vm, value: &[u8]) -> bool {
    let bad = goro_ext_mbstring::state::MBSTRG
        .with(|s| goro_ext_mbstring::state::parse_encoding_list(&s.borrow(), value).err());
    if let Some(bad) = bad {
        let f = vm
            .current_builtin_name()
            .map(|f| format!("{}(): ", f))
            .unwrap_or_else(|| "PHP Startup: ".to_string());
        let line = vm.current_line;
        vm.emit_warning_at(
            &format!(
                "{}INI setting contains invalid encoding \"{}\"",
                f,
                String::from_utf8_lossy(&bad)
            ),
            line,
        );
        vm.emit_warning_at(
            &format!(
                "{}Illegal encoding ignored: '{}'",
                f,
                String::from_utf8_lossy(value)
            ),
            line,
        );
        return false;
    }
    true
}

fn on_update_encode(vm: &mut Vm, value: &[u8], _startup: bool) -> bool {
    value.is_empty() || check_encoding(vm, value)
}

fn on_update_decode(vm: &mut Vm, value: &[u8], _startup: bool) -> bool {
    check_encoding(vm, value)
}

fn ini_str(vm: &Vm, k: &str) -> Vec<u8> {
    vm.constants
        .get(k.as_bytes())
        .map(|v| v.to_php_string().as_bytes().to_vec())
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// argument helpers

fn throw(vm: &mut Vm, class: &[u8], msg: String) -> VmError {
    let exc = vm.create_exception(class, &msg, vm.current_line);
    vm.current_exception = Some(exc);
    VmError {
        message: msg,
        line: vm.current_line,
    }
}

fn type_name(v: &Value) -> String {
    match v {
        Value::Null | Value::Undef => "null".into(),
        Value::True | Value::False => "bool".into(),
        Value::Long(_) => "int".into(),
        Value::Double(_) => "float".into(),
        Value::String(_) => "string".into(),
        Value::Array(_) => "array".into(),
        Value::Object(o) => goro_core::value::display_class_name(&o.borrow().class_name),
        _ => "resource".into(),
    }
}

/// zpp of a scalar parameter after an untyped one (the VM leaves those to
/// the function): Z_PARAM_BOOL / Z_PARAM_STR_OR_NULL.
fn scalar_arg(
    vm: &mut Vm,
    func: &str,
    args: &[Value],
    idx: usize,
    pname: &str,
    mask: u16,
    tname: &str,
) -> Result<Option<Value>, VmError> {
    use goro_core::arginfo::T_NULL;
    let Some(v) = args.get(idx) else {
        return Ok(None);
    };
    let v = v.deref();
    if matches!(v, Value::Null | Value::Undef) {
        if mask & T_NULL != 0 {
            return Ok(None);
        }
        vm.emit_deprecated(&format!(
            "{}(): Passing null to parameter #{} (${}) of type {} is deprecated",
            func,
            idx + 1,
            pname,
            tname
        ));
        if let Some(e) = vm.current_exception.clone() {
            let _ = e;
            return Err(VmError {
                message: String::new(),
                line: vm.current_line,
            });
        }
        return Ok(Some(Value::False));
    }
    let line = vm.current_line;
    match vm.zpp_coerce_scalar(mask, &v, false, line) {
        Ok(Some(v)) => Ok(Some(v)),
        Ok(None) => Err(throw(
            vm,
            b"TypeError",
            format!(
                "{}(): Argument #{} (${}) must be of type {}, {} given",
                func,
                idx + 1,
                pname,
                tname,
                type_name(&v)
            ),
        )),
        Err(()) => Err(VmError {
            message: String::new(),
            line,
        }),
    }
}

fn bool_arg(
    vm: &mut Vm,
    func: &str,
    args: &[Value],
    idx: usize,
    pname: &str,
) -> Result<bool, VmError> {
    Ok(scalar_arg(
        vm,
        func,
        args,
        idx,
        pname,
        goro_core::arginfo::T_BOOL,
        "bool",
    )?
    .is_some_and(|v| v.to_bool()))
}

fn pstr(b: &[u8]) -> Value {
    Value::String(PhpString::from_bytes(b))
}

// ---------------------------------------------------------------------------
// functions

/// exif_tagname(int $index): string|false
fn exif_tagname(_vm: &mut Vm, args: &[Value]) -> R {
    let tag = args.first().map(|v| v.to_long()).unwrap_or(0);
    if tag < 0 {
        return Ok(Value::False);
    }
    match get_tagname(tag, tables::IFD) {
        Some(n) => Ok(pstr(n.as_bytes())),
        None => Ok(Value::False),
    }
}

/// exif_read_from_impl() on an open stream; Err: an exception.
fn read_from_stream(
    vm: &mut Vm,
    id: i64,
    read_thumbnail: bool,
    read_all: bool,
) -> (ImageInfo, bool) {
    let old_pos = ext_io::tell(vm, id);
    if old_pos != 0 {
        ext_io::seek(vm, id, 0, 0);
    }
    let r = read_from_impl(vm, id, read_thumbnail, read_all);
    if old_pos != 0 {
        ext_io::seek(vm, id, old_pos, 0);
    }
    r
}

fn read_from_impl(vm: &mut Vm, id: i64, read_thumbnail: bool, read_all: bool) -> (ImageInfo, bool) {
    let meta = ext_io::meta(vm, id);
    let mut ii = ImageInfo::new(Stream {
        data: Vec::new(),
        pos: 0,
        past_end: meta.is_stdio(),
    });
    if meta.is_stdio() {
        if let Some(uri) = &meta.uri {
            let path = String::from_utf8_lossy(uri).into_owned();
            if let Ok(st) = std::fs::metadata(&path) {
                if !st.is_file() {
                    ii.error(vm, Level::Warning, "Not a file");
                    return (ii, false);
                }
                ii.file_name = Some(ext_io::basename(uri));
                use std::os::unix::fs::MetadataExt;
                ii.file_date_time = st.mtime();
                ii.file_size = st.size();
            }
        }
    } else {
        ext_io::seek(vm, id, 0, 2);
        ii.file_size = ext_io::tell(vm, id).max(0) as u64;
        ext_io::seek(vm, id, 0, 0);
    }
    if ii.file_size >= 2 {
        ext_io::seek(vm, id, 0, 0);
        // (php_stream_read() loop, not stream_get_contents(): no stat)
        ii.stream.data = ext_io::read(vm, id, usize::MAX);
    }
    ii.read_thumbnail = read_thumbnail;
    ii.read_all = read_all;
    ii.thumb.filetype = IMAGE_FILETYPE_UNKNOWN;
    ii.encode_unicode = ini_str(vm, "exif.encode_unicode");
    ii.decode_unicode_be = ini_str(vm, "exif.decode_unicode_motorola");
    ii.decode_unicode_le = ini_str(vm, "exif.decode_unicode_intel");
    ii.encode_jis = ini_str(vm, "exif.encode_jis");
    ii.decode_jis_be = ini_str(vm, "exif.decode_jis_motorola");
    ii.decode_jis_le = ini_str(vm, "exif.decode_jis_intel");
    let ret = ii.scan_file_header(vm);
    (ii, ret)
}

/// The file argument: a stream resource or a path. Ok(None): the open
/// failed (reported).
fn read_from_arg(
    vm: &mut Vm,
    func: &str,
    file: &Value,
    read_thumbnail: bool,
) -> Result<Option<(ImageInfo, bool)>, VmError> {
    if let Value::Resource(id) = file {
        if !ext_io::is_stream(vm, file) {
            return Err(throw(
                vm,
                b"TypeError",
                format!(
                    "{}(): supplied resource is not a valid stream resource",
                    func
                ),
            ));
        }
        return Ok(Some(read_from_stream(vm, id.id(), read_thumbnail, false)));
    }
    // try_convert_to_string()
    let path = if let Value::Array(_) = file {
        let line = vm.current_line;
        vm.emit_warning_at("Array to string conversion", line);
        PhpString::from_bytes(b"Array")
    } else {
        vm.value_to_string_checked(file)?
    };
    if vm.current_exception.is_some() {
        return Err(VmError {
            message: String::new(),
            line: vm.current_line,
        });
    }
    if path.is_empty() {
        return Err(throw(
            vm,
            b"ValueError",
            format!("{}(): Argument #1 ($file) must not be empty", func),
        ));
    }
    if path.as_bytes().contains(&0) {
        return Err(throw(
            vm,
            b"ValueError",
            format!(
                "{}(): Argument #1 ($file) must not contain any null bytes",
                func
            ),
        ));
    }
    let Some(id) = ext_io::open(vm, path.as_bytes(), "rb", false)? else {
        let line = vm.current_line;
        vm.emit_warning_at(&format!("{}(): Unable to open file", func), line);
        return Ok(None);
    };
    let r = read_from_stream(vm, id, read_thumbnail, false);
    ext_io::close(vm, id);
    Ok(Some(r))
}

fn set_key(arr: &mut PhpArray, name: &[u8], v: Value) {
    arr.set(ArrayKey::String(PhpString::from_bytes(name)), v);
}

fn new_arr(a: PhpArray) -> Value {
    Value::Array(Rc::new(RefCell::new(a)))
}

/// add_assoc_image_info()
fn add_assoc_image_info(out: &mut PhpArray, sub_array: bool, ii: &ImageInfo, section: usize) {
    let list = &ii.info_list[section];
    if list.is_empty() {
        return;
    }
    let mut sub = PhpArray::new();
    let tmpi: &mut PhpArray = if sub_array { &mut sub } else { out };
    let mut idx = 0i64;
    for d in list {
        let name = &d.name;
        if d.length == 0 {
            set_key(tmpi, name, Value::Null);
            continue;
        }
        let fmt = d.format as u32;
        use tables::*;
        match fmt {
            TAG_FMT_STRING => {
                let s = match &d.value {
                    Val::S(s) => s.clone(),
                    _ => Vec::new(),
                };
                if section == SECTION_COMMENT {
                    tmpi.set(ArrayKey::Int(idx), pstr(&s));
                    idx += 1;
                } else {
                    set_key(tmpi, name, pstr(&s));
                }
            }
            TAG_FMT_URATIONAL | TAG_FMT_SRATIONAL | TAG_FMT_USHORT | TAG_FMT_SSHORT
            | TAG_FMT_SINGLE | TAG_FMT_DOUBLE | TAG_FMT_ULONG | TAG_FMT_SLONG => {
                let l = d.length as i32;
                let items: Vec<&Val> = match &d.value {
                    Val::List(v) if l > 1 => v.iter().collect(),
                    v => vec![v],
                };
                let conv = |v: &Val| -> Value {
                    match (fmt, v) {
                        (TAG_FMT_USHORT | TAG_FMT_ULONG, Val::U(u)) => {
                            Value::Long(*u as i32 as i64)
                        }
                        (TAG_FMT_URATIONAL, Val::Ur(n, dn)) => {
                            pstr(format!("{}/{}", n, dn).as_bytes())
                        }
                        (TAG_FMT_SSHORT | TAG_FMT_SLONG, Val::I(i)) => Value::Long(*i as i64),
                        (TAG_FMT_SRATIONAL, Val::Sr(n, dn)) => {
                            pstr(format!("{}/{}", n, dn).as_bytes())
                        }
                        (TAG_FMT_SINGLE, Val::F(f)) => Value::Double(*f as f64),
                        (TAG_FMT_DOUBLE, Val::D(f)) => Value::Double(*f),
                        (TAG_FMT_URATIONAL, _) => pstr(b"0/0"),
                        (TAG_FMT_SRATIONAL, _) => pstr(b"0/0"),
                        (TAG_FMT_SINGLE | TAG_FMT_DOUBLE, _) => Value::Double(0.0),
                        _ => Value::Long(0),
                    }
                };
                if l > 1 {
                    let mut arr = PhpArray::new();
                    for (ap, v) in items.iter().enumerate().take(l as usize) {
                        arr.set(ArrayKey::Int(ap as i64), conv(v));
                    }
                    set_key(tmpi, name, new_arr(arr));
                } else if let Some(v) = items.first() {
                    set_key(tmpi, name, conv(v));
                }
            }
            _ => {
                let s = match &d.value {
                    Val::S(s) => {
                        let n = (d.length as usize).min(s.len());
                        s[..n].to_vec()
                    }
                    _ => Vec::new(),
                };
                set_key(tmpi, name, pstr(&s));
            }
        }
    }
    if sub_array {
        set_key(out, get_sectionname(section).as_bytes(), new_arr(sub));
    }
}

/// exif_read_data($file, ?string $required_sections = null, bool $as_arrays = false, bool $read_thumbnail = false): array|false
fn exif_read_data(vm: &mut Vm, args: &[Value]) -> R {
    const F: &str = "exif_read_data";
    let file = args.first().map(|v| v.deref()).unwrap_or(Value::Null);
    let sections = scalar_arg(
        vm,
        F,
        args,
        1,
        "required_sections",
        goro_core::arginfo::T_STRING | goro_core::arginfo::T_NULL,
        "?string",
    )?;
    let sub_arrays = bool_arg(vm, F, args, 2, "as_arrays")?;
    let read_thumbnail = bool_arg(vm, F, args, 3, "read_thumbnail")?;

    let mut sections_needed = 0u32;
    if let Some(s) = sections {
        let s = s.to_php_string();
        let mut list = b",".to_vec();
        list.extend(
            s.as_bytes()
                .iter()
                .map(|&c| if c == b' ' { b',' } else { c }),
        );
        // (the C string stops at a NUL)
        if let Some(z) = list.iter().position(|&c| c == 0) {
            list.truncate(z);
        }
        list.push(b',');
        for i in 0..SECTION_COUNT {
            let tmp = format!(",{},", get_sectionname(i));
            if list.windows(tmp.len()).any(|w| w == tmp.as_bytes()) {
                sections_needed |= 1 << i;
            }
        }
    }

    let Some((mut ii, ret)) = read_from_arg(vm, F, &file, read_thumbnail)? else {
        return Ok(Value::False);
    };
    let sections_str = get_sectionlist(ii.sections_found);
    ii.sections_found |= FOUND_COMPUTED | FOUND_FILE;
    if !ret || (sections_needed != 0 && sections_needed & ii.sections_found == 0) {
        return Ok(Value::False);
    }

    let file_name = ii.file_name.clone();
    ii.add_str(SECTION_FILE, b"FileName", file_name.as_deref());
    ii.add_int(SECTION_FILE, "FileDateTime", ii.file_date_time as i32);
    ii.add_int(SECTION_FILE, "FileSize", ii.file_size as i32);
    ii.add_int(SECTION_FILE, "FileType", ii.file_type as i32);
    ii.add_str(
        SECTION_FILE,
        b"MimeType",
        Some(ext_io::image_type_to_mime_type(ii.file_type).as_bytes()),
    );
    ii.add_str(
        SECTION_FILE,
        b"SectionsFound",
        Some(sections_str.as_bytes()),
    );

    if ii.width > 0 && ii.height > 0 {
        let html = format!("width=\"{}\" height=\"{}\"", ii.width, ii.height);
        ii.add_str(SECTION_COMPUTED, b"html", Some(html.as_bytes()));
        ii.add_int(SECTION_COMPUTED, "Height", ii.height);
        ii.add_int(SECTION_COMPUTED, "Width", ii.width);
    }
    ii.add_int(SECTION_COMPUTED, "IsColor", ii.is_color);
    if ii.motorola_intel != -1 {
        ii.add_int(SECTION_COMPUTED, "ByteOrderMotorola", ii.motorola_intel);
    }
    if ii.focal_length != 0.0 {
        let s = format!("{:4.1}mm", ii.focal_length as f64);
        ii.add_str(SECTION_COMPUTED, b"FocalLength", Some(s.as_bytes()));
        if ii.ccd_width != 0.0 {
            let s = format!("{}mm", (ii.focal_length / ii.ccd_width * 35.0 + 0.5) as i32);
            ii.add_str(SECTION_COMPUTED, b"35mmFocalLength", Some(s.as_bytes()));
        }
    }
    if ii.ccd_width != 0.0 {
        let s = format!("{}mm", ii.ccd_width as i32);
        ii.add_str(SECTION_COMPUTED, b"CCDWidth", Some(s.as_bytes()));
    }
    if ii.exposure_time > 0.0 {
        let recip: f32 = 0.5f32 + 1.0f32 / ii.exposure_time;
        let s = if ii.exposure_time <= 0.5 && recip < i32::MAX as f32 {
            format!("{:.3} s (1/{})", ii.exposure_time as f64, recip as i32)
        } else {
            format!("{:.3} s", ii.exposure_time as f64)
        };
        ii.add_str(SECTION_COMPUTED, b"ExposureTime", Some(s.as_bytes()));
    }
    if ii.aperture_fnumber != 0.0 {
        let s = format!("f/{:.1}", ii.aperture_fnumber as f64);
        ii.add_str(SECTION_COMPUTED, b"ApertureFNumber", Some(s.as_bytes()));
    }
    if ii.distance != 0.0 {
        if ii.distance < 0.0 {
            ii.add_str(SECTION_COMPUTED, b"FocusDistance", Some(b"Infinite"));
        } else {
            let s = format!("{:.2}m", ii.distance as f64);
            ii.add_str(SECTION_COMPUTED, b"FocusDistance", Some(s.as_bytes()));
        }
    }
    if let Some(uc) = ii.user_comment.clone() {
        let n = ii.user_comment_length;
        ii.add_buffer(SECTION_COMPUTED, "UserComment", n, Some(&uc));
        if let Some(e) = ii.user_comment_encoding.clone() {
            if !e.is_empty() {
                ii.add_str(SECTION_COMPUTED, b"UserCommentEncoding", Some(&e));
            }
        }
    }
    let c = ii.copyright.clone();
    ii.add_str(SECTION_COMPUTED, b"Copyright", c.as_deref());
    let c = ii.copyright_photographer.clone();
    ii.add_str(SECTION_COMPUTED, b"Copyright.Photographer", c.as_deref());
    let c = ii.copyright_editor.clone();
    ii.add_str(SECTION_COMPUTED, b"Copyright.Editor", c.as_deref());

    let xp: Vec<(u32, Option<Vec<u8>>)> = ii
        .xp_fields
        .iter()
        .map(|f| (f.tag, f.value.clone()))
        .collect();
    for (tag, value) in xp {
        let name = get_tagname(tag as i64, get_tag_table(SECTION_WINXP)).unwrap_or("UndefinedTag");
        ii.add_str(SECTION_WINXP, name.as_bytes(), value.as_deref());
    }
    if ii.thumb.size != 0 {
        if read_thumbnail {
            let data = ii.thumb.data.clone();
            let size = ii.thumb.size;
            ii.add_tag(
                vm,
                SECTION_THUMBNAIL,
                "THUMBNAIL",
                tables::TAG_NONE,
                tables::TAG_FMT_UNDEFINED,
                size,
                data,
                size,
            );
        }
        if ii.thumb.width == 0 || ii.thumb.height == 0 {
            ii.scan_thumbnail(vm);
        }
        ii.add_int(
            SECTION_COMPUTED,
            "Thumbnail.FileType",
            ii.thumb.filetype as i32,
        );
        let m = ext_io::image_type_to_mime_type(ii.thumb.filetype);
        ii.add_str(SECTION_COMPUTED, b"Thumbnail.MimeType", Some(m.as_bytes()));
    }
    if ii.thumb.width != 0 && ii.thumb.height != 0 {
        ii.add_int(SECTION_COMPUTED, "Thumbnail.Height", ii.thumb.height as i32);
        ii.add_int(SECTION_COMPUTED, "Thumbnail.Width", ii.thumb.width as i32);
    }

    let mut out = PhpArray::new();
    for (s, sub) in [
        (SECTION_FILE, sub_arrays),
        (SECTION_COMPUTED, true),
        (SECTION_ANY_TAG, sub_arrays),
        (SECTION_IFD0, sub_arrays),
        (SECTION_THUMBNAIL, true),
        (SECTION_COMMENT, true),
        (SECTION_EXIF, sub_arrays),
        (SECTION_GPS, sub_arrays),
        (SECTION_INTEROP, sub_arrays),
        (SECTION_FPIX, sub_arrays),
        (SECTION_APP12, sub_arrays),
        (SECTION_WINXP, sub_arrays),
        (SECTION_MAKERNOTE, sub_arrays),
    ] {
        add_assoc_image_info(&mut out, sub, &ii, s);
    }
    Ok(new_arr(out))
}

fn assign_ref(v: Option<&Value>, val: Value) {
    if let Some(Value::Reference(r)) = v {
        *r.borrow_mut() = val;
    }
}

/// exif_thumbnail($file, &$width = null, &$height = null, &$image_type = null): string|false
fn exif_thumbnail(vm: &mut Vm, args: &[Value]) -> R {
    const F: &str = "exif_thumbnail";
    let file = args.first().map(|v| v.deref()).unwrap_or(Value::Null);
    let Some((mut ii, ret)) = read_from_arg(vm, F, &file, true)? else {
        return Ok(Value::False);
    };
    if !ret {
        return Ok(Value::False);
    }
    let data = match &ii.thumb.data {
        Some(d) if ii.thumb.size != 0 => {
            let mut d = d.clone();
            d.resize(ii.thumb.size as usize, 0);
            d
        }
        _ => return Ok(Value::False),
    };
    let (zw, zh, zt) = (args.get(1), args.get(2), args.get(3));
    if (zw.is_some() || zh.is_some())
        && (ii.thumb.width == 0 || ii.thumb.height == 0)
        && !ii.scan_thumbnail(vm)
    {
        ii.thumb.width = 0;
        ii.thumb.height = 0;
    }
    assign_ref(zw, Value::Long(ii.thumb.width as i64));
    assign_ref(zh, Value::Long(ii.thumb.height as i64));
    assign_ref(zt, Value::Long(ii.thumb.filetype));
    Ok(Value::String(PhpString::from_vec(data)))
}

/// exif_imagetype(string $filename): int|false
fn exif_imagetype(vm: &mut Vm, args: &[Value]) -> R {
    let path = args
        .first()
        .map(|v| v.to_php_string().as_bytes().to_vec())
        .unwrap_or_default();
    if path.contains(&0) {
        return Err(throw(
            vm,
            b"ValueError",
            "exif_imagetype(): Argument #1 ($filename) must not contain any null bytes".into(),
        ));
    }
    if path.is_empty() {
        return Err(throw(vm, b"ValueError", "Path must not be empty".into()));
    }
    let Some(id) = ext_io::open(vm, &path, "rb", true)? else {
        return Ok(Value::False);
    };
    let data = ext_io::read_all(vm, id);
    let t = ext_io::image_type_from_bytes(vm, "exif_imagetype", data, &path);
    ext_io::close(vm, id);
    if t == IMAGE_FILETYPE_UNKNOWN {
        Ok(Value::False)
    } else {
        Ok(Value::Long(t))
    }
}
