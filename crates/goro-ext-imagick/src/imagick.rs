//! Imagick (imagick_class.c): the object, the iterator, reading and
//! writing, attributes, properties/artifacts/options/profiles, pixels.
//! The image operations are in `ops.rs`.

use std::rc::Rc;

use goro_core::array::{ArrayKey, PhpArray};
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::Vm;
use goro_imagick::codec;
use goro_imagick::error::severity;
use goro_imagick::geometry::Rect;
use goro_imagick::image::Image;
use goro_imagick::types::*;
use goro_imagick::{MResult, MagickError, PixelInfo, Wand};

use crate::zpp::{self, Arg, Func, Obj};
use crate::{
    C_IMAGICK, Cls, ImagickData, Impl, R, arg_obj, arr_val, bool_val, bytes_val, convert_exception,
    imagick_data, new_imagick, new_pixel, set, str_val, throw, throw_cls, zval_to_pixel,
};

pub(crate) const CORE: &[(&str, Impl, bool)] = &[
    ("__construct", construct, false),
    ("__toString", to_string, false),
    ("count", count, false),
    ("valid", valid, false),
    ("current", current, false),
    ("key", key, false),
    ("next", next, false),
    ("rewind", rewind, false),
    ("getIteratorIndex", get_iterator_index, false),
    ("setIteratorIndex", set_iterator_index, false),
    ("getImageIndex", get_iterator_index, false),
    ("setImageIndex", set_image_index, false),
    ("resetIterator", reset_iterator, false),
    ("setFirstIterator", set_first_iterator, false),
    ("setLastIterator", set_last_iterator, false),
    ("previousImage", previous_image, false),
    ("nextImage", next_image, false),
    ("hasPreviousImage", has_previous_image, false),
    ("hasNextImage", has_next_image, false),
    ("removeImage", remove_image, false),
    ("clear", clear, false),
    ("destroy", clear, false),
    ("clone", clone_method, false),
    ("getImage", get_image, false),
    ("setImage", set_image, false),
    ("addImage", add_image, false),
    ("newImage", new_image, false),
    ("newPseudoImage", new_pseudo_image, false),
    ("readImage", read_image, false),
    ("readImages", read_images, false),
    ("pingImage", ping_image, false),
    ("readImageBlob", read_image_blob, false),
    ("pingImageBlob", ping_image_blob, false),
    ("readImageFile", read_image_file, false),
    ("pingImageFile", ping_image_file, false),
    ("writeImage", write_image, false),
    ("writeImages", write_images, false),
    ("writeImageFile", write_image_file, false),
    ("writeImagesFile", write_images_file, false),
    ("getImageBlob", get_image_blob, false),
    ("getImagesBlob", get_images_blob, false),
    ("setImageFormat", set_image_format, false),
    ("getImageFormat", get_image_format, false),
    ("setFormat", set_format, false),
    ("getFormat", get_format, false),
    ("getImageMimeType", get_image_mime_type, false),
    ("getImageFilename", get_image_filename, false),
    ("setImageFilename", set_image_filename, false),
    ("getFilename", get_filename, false),
    ("setFilename", set_filename, false),
    ("getImageWidth", get_image_width, false),
    ("getImageHeight", get_image_height, false),
    ("getImageGeometry", get_image_geometry, false),
    ("getImagePage", get_image_page, false),
    ("setImagePage", set_image_page, false),
    ("resetImagePage", reset_image_page, false),
    ("getPage", get_page, false),
    ("setPage", set_page, false),
    ("getImageLength", get_image_length, false),
    ("getImageSize", get_image_length, false),
    ("getNumberImages", get_number_images, false),
    ("identifyImage", identify_image, false),
    ("identifyFormat", identify_format, false),
    ("identifyImageType", identify_image_type, false),
    (
        "getImageCompressionQuality",
        get_image_compression_quality,
        false,
    ),
    (
        "setImageCompressionQuality",
        set_image_compression_quality,
        false,
    ),
    ("getCompressionQuality", get_compression_quality, false),
    ("setCompressionQuality", set_compression_quality, false),
    ("getImageCompression", get_image_compression, false),
    ("setImageCompression", set_image_compression, false),
    ("getCompression", get_compression, false),
    ("setCompression", set_compression, false),
    ("stripImage", strip_image, false),
    ("getImageProperty", get_image_property, false),
    ("setImageProperty", set_image_property, false),
    ("deleteImageProperty", delete_image_property, false),
    ("getImageProperties", get_image_properties, false),
    ("commentImage", comment_image, false),
    ("labelImage", label_image, false),
    ("getImageArtifact", get_image_artifact, false),
    ("setImageArtifact", set_image_artifact, false),
    ("deleteImageArtifact", delete_image_artifact, false),
    ("getImageArtifacts", get_image_artifacts, false),
    ("getOption", get_option, false),
    ("setOption", set_option, false),
    ("deleteOption", delete_option, false),
    ("getOptions", get_options, false),
    ("getImageProfile", get_image_profile, false),
    ("getImageProfiles", get_image_profiles, false),
    ("setImageProfile", set_image_profile, false),
    ("removeImageProfile", remove_image_profile, false),
    ("profileImage", profile_image, false),
    ("getVersion", get_version, true),
    ("getCopyright", get_copyright, true),
    ("getPackageName", get_package_name, true),
    ("getReleaseDate", get_release_date, true),
    ("getHomeURL", get_home_url, true),
    ("getQuantum", get_quantum, true),
    ("getQuantumDepth", get_quantum_depth, true),
    ("getQuantumRange", get_quantum_range, true),
    ("getHDRIEnabled", get_hdri_enabled, true),
    ("getFeatures", get_features, true),
    ("getConfigureOptions", get_configure_options, true),
    ("getResource", get_resource, true),
    ("getResourceLimit", get_resource_limit, true),
    ("setResourceLimit", set_resource_limit, true),
    ("queryFormats", query_formats, true),
    ("queryFonts", query_fonts, true),
    ("setRegistry", set_registry, true),
    ("getRegistry", get_registry, true),
    ("listRegistry", list_registry, true),
    ("getImageMagickLicense", get_imagemagick_license, true),
    ("setSeed", set_seed, true),
    ("calculateCrop", calculate_crop, true),
    ("setSize", set_size, false),
    ("getSize", get_size, false),
    ("setSizeOffset", set_size_offset, false),
    ("getSizeOffset", get_size_offset, false),
    ("setResolution", set_resolution, false),
    ("getResolution", get_resolution, false),
    ("setImageResolution", set_image_resolution, false),
    ("getImageResolution", get_image_resolution, false),
    ("setImageUnits", set_image_units, false),
    ("getImageUnits", get_image_units, false),
    ("setBackgroundColor", set_background_color, false),
    ("getBackgroundColor", get_background_color, false),
    ("setImageBackgroundColor", set_image_background_color, false),
    ("getImageBackgroundColor", get_image_background_color, false),
    ("setImageBorderColor", set_image_border_color, false),
    ("getImageBorderColor", get_image_border_color, false),
    ("setImageMatteColor", set_image_matte_color, false),
    ("getImageMatteColor", get_image_matte_color, false),
    ("setImageDelay", set_image_delay, false),
    ("getImageDelay", get_image_delay, false),
    ("setImageTicksPerSecond", set_image_ticks_per_second, false),
    ("getImageTicksPerSecond", get_image_ticks_per_second, false),
    ("setImageIterations", set_image_iterations, false),
    ("getImageIterations", get_image_iterations, false),
    ("setImageDispose", set_image_dispose, false),
    ("getImageDispose", get_image_dispose, false),
    ("setImageScene", set_image_scene, false),
    ("getImageScene", get_image_scene, false),
    ("setImageGravity", set_image_gravity, false),
    ("getImageGravity", get_image_gravity, false),
    ("setGravity", set_gravity, false),
    ("getGravity", get_gravity, false),
    ("setImageCompose", set_image_compose, false),
    ("getImageCompose", get_image_compose, false),
    ("setImageDepth", set_image_depth, false),
    ("getImageDepth", get_image_depth, false),
    ("setDepth", set_depth, false),
    ("setImageChannelDepth", set_image_channel_depth, false),
    ("getImageChannelDepth", get_image_channel_depth, false),
    ("setImageType", set_image_type, false),
    ("getImageType", get_image_type, false),
    ("setType", set_type, false),
    ("getType", get_type, false),
    ("setImageColorspace", set_image_colorspace, false),
    ("getImageColorspace", get_image_colorspace, false),
    ("setColorspace", set_colorspace, false),
    ("getColorspace", get_colorspace, false),
    ("setImageInterlaceScheme", set_image_interlace_scheme, false),
    ("getImageInterlaceScheme", get_image_interlace_scheme, false),
    ("setInterlaceScheme", set_interlace_scheme, false),
    ("getInterlaceScheme", get_interlace_scheme, false),
    ("setImageOrientation", set_image_orientation, false),
    ("getImageOrientation", get_image_orientation, false),
    ("setOrientation", set_orientation, false),
    ("getOrientation", get_orientation, false),
    (
        "setImageVirtualPixelMethod",
        set_image_virtual_pixel_method,
        false,
    ),
    (
        "getImageVirtualPixelMethod",
        get_image_virtual_pixel_method,
        false,
    ),
    (
        "setImageInterpolateMethod",
        set_image_interpolate_method,
        false,
    ),
    (
        "getImageInterpolateMethod",
        get_image_interpolate_method,
        false,
    ),
    ("setInterpolateMethod", set_interpolate_method, false),
    ("getInterpolateMethod", get_interpolate_method, false),
    ("setImageGamma", set_image_gamma, false),
    ("getImageGamma", get_image_gamma, false),
    ("setImageRenderingIntent", set_image_rendering_intent, false),
    ("getImageRenderingIntent", get_image_rendering_intent, false),
    ("setImageRedPrimary", set_image_red_primary, false),
    ("getImageRedPrimary", get_image_red_primary, false),
    ("setImageGreenPrimary", set_image_green_primary, false),
    ("getImageGreenPrimary", get_image_green_primary, false),
    ("setImageBluePrimary", set_image_blue_primary, false),
    ("getImageBluePrimary", get_image_blue_primary, false),
    ("setImageWhitePoint", set_image_white_point, false),
    ("getImageWhitePoint", get_image_white_point, false),
    ("setAntialias", set_antialias, false),
    ("getAntialias", get_antialias, false),
    ("setFont", set_font, false),
    ("getFont", get_font, false),
    ("setPointSize", set_point_size, false),
    ("getPointSize", get_point_size, false),
    ("setExtract", set_extract, false),
    ("setSamplingFactors", set_sampling_factors, false),
    ("getSamplingFactors", get_sampling_factors, false),
    ("setImageAlphaChannel", set_image_alpha_channel, false),
    ("getImageAlphaChannel", get_image_alpha_channel, false),
    ("setImageMatte", set_image_matte, false),
    ("setImageAlpha", set_image_alpha, false),
    ("setImageOpacity", set_image_alpha, false),
    ("getImageSignature", get_image_signature, false),
    ("getImagePixelColor", get_image_pixel_color, false),
    ("setImagePixelColor", set_image_pixel_color, false),
    ("exportImagePixels", export_image_pixels, false),
    ("importImagePixels", import_image_pixels, false),
    ("getPixelIterator", get_pixel_iterator, false),
    ("getPixelRegionIterator", get_pixel_region_iterator, false),
    ("getImageColors", get_image_colors, false),
    ("getImageHistogram", get_image_histogram, false),
    (
        "getImageTotalInkDensity",
        get_image_total_ink_density,
        false,
    ),
    ("setImageProgressMonitor", set_image_progress_monitor, false),
    ("setProgressMonitor", set_progress_monitor, false),
    ("setImageChannelMask", set_image_channel_mask, false),
];

// ---------------------------------------------------------------------------
// helpers

pub(crate) fn this(o: Option<&Obj>) -> Rc<ImagickData> {
    imagick_data(o.expect("method with $this"))
}

/// IMAGICK_NOT_EMPTY
pub(crate) fn not_empty(vm: &mut Vm, d: &ImagickData) -> Result<(), ()> {
    if d.wand.borrow().is_empty() {
        throw_cls(vm, Cls::Imagick, "Can not process empty Imagick object")?;
    }
    Ok(())
}

/// Run `f` on the current image (after IMAGICK_NOT_EMPTY).
pub(crate) fn with_image<T>(
    vm: &mut Vm,
    d: &ImagickData,
    f: impl FnOnce(&mut Image) -> T,
) -> Result<T, ()> {
    not_empty(vm, d)?;
    let mut w = d.wand.borrow_mut();
    let img = w.image_mut().unwrap();
    Ok(f(img))
}

/// An operation on the current image: Ok(true), or the wand exception /
/// the default message.
pub(crate) fn image_op(
    vm: &mut Vm,
    d: &ImagickData,
    msg: &str,
    f: impl FnOnce(&mut Image) -> MResult<()>,
) -> R {
    not_empty(vm, d)?;
    if !progress(vm, d)? {
        return convert_exception(vm, None, Cls::Imagick, msg);
    }
    let r = {
        let mut w = d.wand.borrow_mut();
        let img = w.image_mut().unwrap();
        f(img)
    };
    match r {
        Ok(()) => Ok(Value::True),
        Err(e) => convert_exception(vm, Some(e), Cls::Imagick, msg),
    }
}

/// The progress monitor (setProgressMonitor()): called as the operation
/// proceeds; false when it asked to cancel.
fn progress(vm: &mut Vm, d: &ImagickData) -> Result<bool, ()> {
    let Some(cb) = d.progress.borrow().clone() else {
        return Ok(true);
    };
    let span = d.wand.borrow().image().map_or(1, |i| i.rows.max(1)) as i64;
    for k in 0..=4 {
        let offset = span * k / 4;
        let r = vm.call_callback(&cb, &[Value::Long(offset), Value::Long(span)]);
        if vm.current_exception.is_some() {
            return Err(());
        }
        if let Ok(Value::False) = r {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Replace the current image by a new one computed from it.
pub(crate) fn replace_op(
    vm: &mut Vm,
    d: &ImagickData,
    msg: &str,
    f: impl FnOnce(&Image) -> MResult<Image>,
) -> R {
    image_op(vm, d, msg, |img| {
        let n = f(img)?;
        *img = n;
        Ok(())
    })
}

fn s(a: &Arg) -> String {
    String::from_utf8_lossy(a.str_or(b"")).into_owned()
}

fn rect_val(r: Rect) -> Value {
    let mut a = PhpArray::new();
    set(&mut a, "width", Value::Long(r.width as i64));
    set(&mut a, "height", Value::Long(r.height as i64));
    set(&mut a, "x", Value::Long(r.x));
    set(&mut a, "y", Value::Long(r.y));
    arr_val(a)
}

fn obj_arg(vm: &mut Vm, f: &Func, n: usize, a: &Arg) -> Result<Obj, ()> {
    match a {
        Arg::Any(v) => arg_obj(vm, f, n, v, C_IMAGICK, "Imagick"),
        _ => Err(()),
    }
}

/// The Imagick argument's data, not empty.
pub(crate) fn other_imagick(
    vm: &mut Vm,
    f: &Func,
    n: usize,
    a: &Arg,
) -> Result<Rc<ImagickData>, ()> {
    let o = obj_arg(vm, f, n, a)?;
    let d = imagick_data(&o);
    not_empty(vm, &d)?;
    Ok(d)
}

// ---------------------------------------------------------------------------
// object handlers

/// php_imagick_read_property(): width/height/format of the current image
/// for undeclared properties.
pub(crate) fn read_property(vm: &mut Vm, o: &Obj, name: &[u8], mode: u8) -> Option<Value> {
    if o.borrow()
        .properties
        .iter()
        .any(|(k, _)| k.as_slice() == name)
    {
        return None;
    }
    if !matches!(name, b"width" | b"height" | b"format") {
        // undefined properties read as null, without a warning (unless a
        // subclass has __get())
        let cls = o.borrow().class_name.to_ascii_lowercase();
        let magic = vm
            .classes
            .get(&cls)
            .is_some_and(|c| c.get_method(b"__get").is_some());
        return (mode == 0 && !magic).then_some(Value::Null);
    }
    let d = imagick_data(o);
    let w = d.wand.borrow();
    let img = w.image()?;
    Some(match name {
        b"width" => Value::Long(img.columns as i64),
        b"height" => Value::Long(img.rows as i64),
        _ => str_val(&w.image_format(img).to_ascii_lowercase()),
    })
}

/// The string conversion (Imagick::__toString()): the current image blob,
/// or "" when there is none / no format.
pub(crate) fn cast(o: &Obj, kind: u8) -> Option<Value> {
    if kind != 3 {
        return None;
    }
    let d = imagick_data(o);
    let w = d.wand.borrow();
    let Some(img) = w.image() else {
        return Some(str_val(""));
    };
    if w.image_format(img).is_empty() {
        return Some(str_val(""));
    }
    Some(match w.image_blob() {
        Ok(b) => Value::String(PhpString::from_vec(b)),
        Err(_) => str_val(""),
    })
}

pub(crate) fn count_elements(_vm: &mut Vm, o: &Obj) -> Option<i64> {
    Some(imagick_data(o).wand.borrow().len() as i64)
}

/// php_imagick_clone_imagick_object(): CloneMagickWand().
pub(crate) fn clone_obj(_vm: &mut Vm, src: &Obj, dst: &Obj) {
    let mut w = imagick_data(src).wand.borrow().clone();
    // the cloned images have no blob behind them
    for img in w.images.iter_mut() {
        img.file_size = 0;
    }
    crate::attach_imagick(dst, w);
}

// ---------------------------------------------------------------------------
// files

enum RwFail {
    Library(MagickError),
    IsDir,
    Message(String),
}

/// php_imagick_read_file(): a file name (with a "FORMAT:" prefix, a
/// pseudo format...) read into the wand, then MagickSetLastIterator.
fn read_file(vm: &mut Vm, d: &ImagickData, filename: &str, ping: bool) -> Result<(), RwFail> {
    let spec = goro_imagick::wand::parse_filespec(filename);
    let is_virtual = spec
        .magick
        .as_deref()
        .is_some_and(goro_imagick::pseudo::is_pseudo);
    if !is_virtual && spec.magick.is_none() && !spec.path.is_empty() {
        // open_basedir (php_imagick_safe_mode_check)
        if let Some(msg) = open_basedir_denied(vm, &spec.path) {
            return Err(RwFail::Message(msg));
        }
    }
    let r = {
        let w = d.wand.borrow();
        w.read_images(filename, ping)
    };
    match r {
        Ok(images) => {
            let abs = if is_virtual {
                String::new()
            } else {
                absolute_path(&spec.path)
            };
            let mut w = d.wand.borrow_mut();
            let n = images.len();
            w.insert_images(images);
            // MagickSetImageFilename(wand, absolute path) of the current
            // image, then MagickSetLastIterator
            if n > 0 {
                let cur = w.current;
                if let Some(img) = w.images.get_mut(cur) {
                    img.filename = abs;
                }
            }
            w.set_last_iterator();
            Ok(())
        }
        Err(e) => {
            if !is_virtual {
                if let Ok(md) = std::fs::metadata(&spec.path) {
                    if md.is_dir() {
                        return Err(RwFail::IsDir);
                    }
                }
            }
            Err(RwFail::Library(e))
        }
    }
}

fn absolute_path(p: &str) -> String {
    if p.is_empty() {
        return String::new();
    }
    let path = std::path::Path::new(p);
    let abs = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|c| c.join(path))
            .unwrap_or_else(|_| path.to_path_buf())
    };
    // expand_filepath() normalises "." and ".." lexically
    let mut out = std::path::PathBuf::new();
    for c in abs.components() {
        match c {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            c => out.push(c.as_os_str()),
        }
    }
    out.to_string_lossy().into_owned()
}

fn open_basedir_denied(vm: &mut Vm, path: &str) -> Option<String> {
    let ob = vm.constants.get(b"open_basedir".as_slice())?;
    let ob = ob.to_php_string();
    if ob.as_bytes().is_empty() {
        return None;
    }
    let abs = absolute_path(path);
    let allowed = String::from_utf8_lossy(ob.as_bytes())
        .split(':')
        .filter(|s| !s.is_empty())
        .any(|dir| {
            let dir = absolute_path(dir);
            abs.starts_with(&dir)
        });
    if allowed {
        None
    } else {
        Some(format!(
            "open_basedir restriction in effect. File({}) is not within the allowed path(s)",
            abs
        ))
    }
}

fn rw_fail(vm: &mut Vm, f: RwFail, filename: &str) -> R {
    match f {
        RwFail::Library(e) => {
            convert_exception(vm, Some(e), Cls::Imagick, "Failed to read the file")
        }
        RwFail::IsDir => {
            throw(
                vm,
                b"ImagickException",
                &format!("The path is a directory: {}", filename),
                1,
            );
            Err(())
        }
        RwFail::Message(m) => {
            throw(vm, b"ImagickException", &m, 1);
            Err(())
        }
    }
}

fn read_one(vm: &mut Vm, d: &ImagickData, filename: &[u8], ping: bool) -> Result<(), ()> {
    if filename.is_empty() {
        throw_cls(vm, Cls::Imagick, "Invalid filename provided")?;
    }
    let name = String::from_utf8_lossy(filename).into_owned();
    if name.len() >= 4096 {
        throw_cls(vm, Cls::Imagick, "Invalid filename provided")?;
    }
    if let Err(f) = read_file(vm, d, &name, ping) {
        rw_fail(vm, f, &name)?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// construction, iteration

fn construct(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "|z!", args)?;
    let d = this(o);
    let v = match &a[0] {
        Arg::Any(v) => v.deref(),
        _ => return Ok(Value::Null),
    };
    match &v {
        Value::Null => Ok(Value::Null),
        Value::Long(_) | Value::Double(_) | Value::String(_) => {
            let s = v.to_php_string();
            read_one(vm, &d, s.as_bytes(), false)?;
            Ok(Value::Null)
        }
        Value::Array(arr) => {
            let items: Vec<Value> = arr.borrow().values().cloned().collect();
            for it in items {
                let s = it.deref().to_php_string();
                read_one(vm, &d, s.as_bytes(), false)?;
            }
            Ok(Value::Null)
        }
        Value::True | Value::False => throw_cls(
            vm,
            Cls::Imagick,
            "Constructor shouldn't be called with a boolean as the filename",
        ),
        _ => Ok(Value::Null),
    }
}

fn to_string(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    let d = this(o);
    let w = d.wand.borrow();
    let Some(img) = w.image() else {
        return Ok(str_val(""));
    };
    if w.image_format(img).is_empty() {
        return Ok(str_val(""));
    }
    match w.image_blob() {
        Ok(b) => Ok(Value::String(PhpString::from_vec(b))),
        Err(_) => Ok(str_val("")),
    }
}

fn count(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "|l", args)?;
    Ok(Value::Long(this(o).wand.borrow().len() as i64))
}

fn valid(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    let d = this(o);
    not_empty(vm, &d)?;
    Ok(bool_val(!d.next_out_of_bound.get()))
}

fn current(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    Ok(Value::Object(o.unwrap().clone()))
}

fn iterator_index_value(w: &Wand) -> Value {
    // MagickGetIteratorIndex() returns -1 for an empty wand, which the C
    // code stores in a MagickBooleanType (unsigned int).
    Value::Long(w.iterator_index() as u32 as i64)
}

fn key(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    Ok(iterator_index_value(&this(o).wand.borrow()))
}

fn next(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    let d = this(o);
    if !d.wand.borrow_mut().next_image() {
        d.next_out_of_bound.set(true);
    }
    Ok(Value::Null)
}

fn rewind(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    let d = this(o);
    d.next_out_of_bound.set(false);
    d.wand.borrow_mut().set_first_iterator();
    Ok(Value::Null)
}

fn get_iterator_index(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    Ok(iterator_index_value(&this(o).wand.borrow()))
}

fn set_iterator_index(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "l", args)?;
    let d = this(o);
    if !d.wand.borrow_mut().set_iterator_index(a[0].long_or(0)) {
        return convert_exception(vm, None, Cls::Imagick, "Unable to set iterator index");
    }
    d.next_out_of_bound.set(false);
    Ok(Value::True)
}

fn set_image_index(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "l", args)?;
    let d = this(o);
    if !d.wand.borrow_mut().set_iterator_index(a[0].long_or(0)) {
        return convert_exception(vm, None, Cls::Imagick, "Unable to set image index");
    }
    d.next_out_of_bound.set(false);
    Ok(Value::True)
}

fn reset_iterator(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    let d = this(o);
    d.next_out_of_bound.set(false);
    d.wand.borrow_mut().reset_iterator();
    Ok(Value::Null)
}

fn set_first_iterator(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    let d = this(o);
    d.next_out_of_bound.set(false);
    d.wand.borrow_mut().set_first_iterator();
    Ok(Value::True)
}

fn set_last_iterator(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    let d = this(o);
    d.next_out_of_bound.set(false);
    d.wand.borrow_mut().set_last_iterator();
    Ok(Value::True)
}

fn previous_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    let d = this(o);
    if !d.wand.borrow_mut().previous_image() {
        return Ok(Value::False);
    }
    d.next_out_of_bound.set(false);
    Ok(Value::True)
}

fn next_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    let d = this(o);
    if !d.wand.borrow_mut().next_image() {
        d.next_out_of_bound.set(true);
        return Ok(Value::False);
    }
    Ok(Value::True)
}

fn has_previous_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    Ok(bool_val(this(o).wand.borrow().has_previous()))
}

fn has_next_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    Ok(bool_val(this(o).wand.borrow().has_next()))
}

fn remove_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    let d = this(o);
    not_empty(vm, &d)?;
    if !d.wand.borrow_mut().remove_image() {
        return convert_exception(vm, None, Cls::Imagick, "Unable to remove image");
    }
    d.next_out_of_bound.set(false);
    d.wand.borrow_mut().set_last_iterator();
    Ok(Value::True)
}

fn clear(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    let d = this(o);
    let mut w = d.wand.borrow_mut();
    w.clear();
    w.settings = Default::default();
    Ok(Value::True)
}

fn clone_method(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    vm.emit_deprecated("Imagick::clone method is deprecated and it's use should be avoided");
    let w = this(o).wand.borrow().clone();
    Ok(new_imagick(vm, w))
}

fn get_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    let d = this(o);
    not_empty(vm, &d)?;
    let w = {
        let src = d.wand.borrow();
        let mut w = Wand::new();
        w.settings = src.settings.clone();
        w.images = vec![src.image().unwrap().clone()];
        w
    };
    Ok(new_imagick(vm, w))
}

fn set_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "z", args)?;
    let other = obj_arg(vm, f, 1, &a[0])?;
    let d = this(o);
    not_empty(vm, &d)?;
    let od = imagick_data(&other);
    not_empty(vm, &od)?;
    let replacement: Vec<Image> = od.wand.borrow().images.clone();
    let mut w = d.wand.borrow_mut();
    // MagickSetImage(): the current image is replaced by the other wand's images
    let cur = w.current;
    let n = replacement.len();
    w.images.splice(cur..cur + 1, replacement);
    w.current = cur + n - 1;
    Ok(Value::True)
}

fn add_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "z", args)?;
    let other = obj_arg(vm, f, 1, &a[0])?;
    let od = imagick_data(&other);
    not_empty(vm, &od)?;
    let images = od.wand.borrow().images.clone();
    let d = this(o);
    let mut w = d.wand.borrow_mut();
    w.insert_images(images);
    w.set_last_iterator();
    Ok(Value::True)
}

fn new_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "llz|s!", args)?;
    let d = this(o);
    let (w, h) = (a[0].long_or(0), a[1].long_or(0));
    let color = match &a[2] {
        Arg::Any(v) => zval_to_pixel(vm, v, Cls::Imagick)?,
        _ => return Err(()),
    };
    if w < 0 || h < 0 {
        let e = MagickError::with_desc(
            severity::OPTION_ERROR,
            "NegativeOrZeroImageSize",
            "",
            "error/image.c/SetImageExtent/2771",
        );
        return convert_exception(vm, Some(e), Cls::Imagick, "Unable to create new image");
    }
    if let Err(e) = d
        .wand
        .borrow_mut()
        .new_image(w as usize, h as usize, &color)
    {
        return convert_exception(vm, Some(e), Cls::Imagick, "Unable to create new image");
    }
    if let Arg::Str(fmt) = &a[3] {
        if !fmt.is_empty() {
            let fmt = String::from_utf8_lossy(fmt).into_owned();
            if let Err(e) = set_format_of(&mut d.wand.borrow_mut(), &fmt) {
                return convert_exception(
                    vm,
                    Some(e),
                    Cls::Imagick,
                    "Unable to set the image format",
                );
            }
        }
    }
    d.wand.borrow_mut().set_last_iterator();
    Ok(Value::True)
}

fn new_pseudo_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "lls", args)?;
    let (w, h) = (a[0].long_or(0), a[1].long_or(0));
    let pseudo = a[2].str_or(b"").to_vec();
    if !crate::ini_bool(vm, "imagick.allow_zero_dimension_images") {
        if w == 0 {
            vm.emit_deprecated("Creating images with zero columns is deprecated. If you think you need to do this, please open an issue at https://phpimagick.com/issues");
        }
        if h == 0 {
            vm.emit_deprecated("Creating images with zero rows is deprecated. If you think you need to do this, please open an issue at https://phpimagick.com/issues");
        }
        if vm.current_exception.is_some() {
            return Err(());
        }
    }
    if !pseudo.contains(&b':') {
        return throw_cls(vm, Cls::Imagick, "Invalid pseudo format string");
    }
    let d = this(o);
    d.wand.borrow_mut().settings.size = Some((w.max(0) as usize, h.max(0) as usize));
    read_one(vm, &d, &pseudo, false)?;
    Ok(Value::True)
}

fn read_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "s", args)?;
    let d = this(o);
    read_one(vm, &d, a[0].str_or(b""), false)?;
    Ok(Value::True)
}

fn ping_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "s", args)?;
    let d = this(o);
    read_one(vm, &d, a[0].str_or(b""), true)?;
    Ok(Value::True)
}

fn read_images(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "a", args)?;
    let d = this(o);
    let Arg::Arr(arr) = &a[0] else {
        return Err(());
    };
    let items: Vec<Value> = arr.borrow().values().cloned().collect();
    for it in items {
        let s = it.deref().to_php_string();
        read_one(vm, &d, s.as_bytes(), false)?;
    }
    Ok(Value::True)
}

fn blob_read(
    vm: &mut Vm,
    d: &ImagickData,
    blob: &[u8],
    filename: Option<String>,
    ping: bool,
    msg: &str,
) -> R {
    let r = d.wand.borrow().decode_blob(blob, filename.as_deref());
    match r {
        Ok(mut images) => {
            for img in images.iter_mut() {
                img.ping = ping;
            }
            let mut w = d.wand.borrow_mut();
            w.insert_images(images);
            let cur = w.current;
            if let Some(img) = w.images.get_mut(cur) {
                img.filename = filename.unwrap_or_default();
            }
            w.set_last_iterator();
            Ok(Value::True)
        }
        Err(e) => convert_exception(vm, Some(e), Cls::Imagick, msg),
    }
}

fn read_image_blob(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "s|s!", args)?;
    let blob = a[0].str_or(b"").to_vec();
    if blob.is_empty() {
        return throw_cls(vm, Cls::Imagick, "Zero size image string passed");
    }
    let filename = a[1].str().map(|s| String::from_utf8_lossy(s).into_owned());
    blob_read(
        vm,
        &this(o),
        &blob,
        filename,
        false,
        "Unable to read image blob",
    )
}

fn ping_image_blob(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "s", args)?;
    let blob = a[0].str_or(b"").to_vec();
    if blob.is_empty() {
        return throw_cls(vm, Cls::Imagick, "Empty image string passed");
    }
    blob_read(vm, &this(o), &blob, None, true, "Unable to ping image blob")
}

/// Call a built-in function (the stream functions of ext/standard).
fn call(vm: &mut Vm, name: &[u8], args: &[Value]) -> Value {
    match vm.functions.get(name).copied() {
        Some(f) => f(vm, args).unwrap_or(Value::False),
        None => Value::False,
    }
}

/// The whole content of a stream resource.
fn read_stream(vm: &mut Vm, v: &Value) -> Result<Vec<u8>, ()> {
    let mut out = Vec::new();
    for _ in 0..1_000_000 {
        let chunk = call(vm, b"fread", &[v.clone(), Value::Long(65536)]);
        if vm.current_exception.is_some() {
            return Err(());
        }
        match chunk {
            Value::String(s) if !s.as_bytes().is_empty() => out.extend_from_slice(s.as_bytes()),
            _ => break,
        }
    }
    Ok(out)
}

fn file_arg(vm: &mut Vm, f: &Func, a: &Arg) -> Result<Value, ()> {
    match a {
        Arg::Any(v) if v.deref().is_resource() => Ok(v.deref()),
        Arg::Any(v) => {
            f.type_error(vm, 1, "resource", &v.deref());
            Err(())
        }
        _ => Err(()),
    }
}

fn read_image_file(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "z|s!", args)?;
    let res = file_arg(vm, f, &a[0])?;
    let data = read_stream(vm, &res)?;
    let filename = a[1].str().map(|s| String::from_utf8_lossy(s).into_owned());
    if data.is_empty() {
        return convert_exception(
            vm,
            None,
            Cls::Imagick,
            "Unable to read image from the filehandle",
        );
    }
    blob_read(
        vm,
        &this(o),
        &data,
        filename,
        false,
        "Unable to read image from the filehandle",
    )
}

fn ping_image_file(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "z|s!", args)?;
    let res = file_arg(vm, f, &a[0])?;
    let data = read_stream(vm, &res)?;
    let filename = a[1].str().map(|s| String::from_utf8_lossy(s).into_owned());
    if data.is_empty() {
        return convert_exception(
            vm,
            None,
            Cls::Imagick,
            "Unable to ping image from the filehandle",
        );
    }
    blob_read(
        vm,
        &this(o),
        &data,
        filename,
        true,
        "Unable to ping image from the filehandle",
    )
}

fn write_one(vm: &mut Vm, d: &ImagickData, filename: &str, all: bool, adjoin: bool) -> R {
    let spec = goro_imagick::wand::parse_filespec(filename);
    if spec.magick.is_none() && !spec.path.is_empty() {
        if let Some(msg) = open_basedir_denied(vm, &spec.path) {
            throw(vm, b"ImagickException", &msg, 1);
            return Err(());
        }
    }
    let r = d.wand.borrow_mut().write(filename, all, adjoin);
    match r {
        Ok(()) => Ok(Value::True),
        Err(e) => convert_exception(vm, Some(e), Cls::Imagick, "Failed to read the file"),
    }
}

fn write_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "|s!", args)?;
    let d = this(o);
    not_empty(vm, &d)?;
    let filename = match &a[0] {
        Arg::Str(s) => String::from_utf8_lossy(s).into_owned(),
        _ => d.wand.borrow().image().unwrap().filename.clone(),
    };
    if filename.is_empty() {
        return throw_cls(vm, Cls::Imagick, "Can not use empty string as a filename");
    }
    write_one(vm, &d, &filename, false, false)
}

fn write_images(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "sb", args)?;
    let d = this(o);
    not_empty(vm, &d)?;
    let filename = s(&a[0]);
    if filename.is_empty() {
        return convert_exception(
            vm,
            None,
            Cls::Imagick,
            "Can not use empty string as a filename",
        );
    }
    write_one(vm, &d, &filename, true, a[1].bool_or(false))
}

fn write_to_stream(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value], all: bool) -> R {
    let a = zpp::parse(vm, f, "z|s!", args)?;
    let res = file_arg(vm, f, &a[0])?;
    let d = this(o);
    not_empty(vm, &d)?;
    let msg = if all {
        "Unable to write images to the filehandle"
    } else {
        "Unable to write image to the filehandle"
    };
    let blob = {
        let mut w = d.wand.borrow_mut();
        // MagickWriteImageFile(): the format of the image's filename
        // ("png:", ".jpg"), else the image's own
        let name = match &a[1] {
            Arg::Str(fmt) => format!("{}:", String::from_utf8_lossy(fmt)),
            _ => w.image().unwrap().filename.clone(),
        };
        let spec = goro_imagick::wand::parse_filespec(&name);
        let fmt = spec
            .magick
            .or_else(|| goro_imagick::wand::extension_format(&spec.path))
            .filter(|f| codec::format_info(f).is_some());
        let saved = w.settings.format.clone();
        if let Some(fmt) = fmt {
            w.settings.format = fmt;
        }
        let r = if all { w.images_blob() } else { w.image_blob() };
        w.settings.format = saved;
        r
    };
    match blob {
        Ok(b) => {
            call(vm, b"fwrite", &[res, Value::String(PhpString::from_vec(b))]);
            if vm.current_exception.is_some() {
                return Err(());
            }
            Ok(Value::True)
        }
        Err(e) => convert_exception(vm, Some(e), Cls::Imagick, msg),
    }
}

fn write_image_file(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    write_to_stream(vm, f, o, args, false)
}

fn write_images_file(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    write_to_stream(vm, f, o, args, true)
}

fn has_format(w: &Wand) -> bool {
    w.image().is_some_and(|i| !w.image_format(i).is_empty())
}

fn get_image_blob(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    let d = this(o);
    not_empty(vm, &d)?;
    if !has_format(&d.wand.borrow()) {
        return throw_cls(vm, Cls::Imagick, "Image has no format");
    }
    let r = d.wand.borrow().image_blob();
    match r {
        Ok(b) => Ok(Value::String(PhpString::from_vec(b))),
        Err(e) => {
            d.wand.borrow_mut().throw(e);
            throw_cls(
                vm,
                Cls::Imagick,
                "Failed to get the image contents (empty or invalid image?)",
            )
        }
    }
}

fn get_images_blob(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    let d = this(o);
    not_empty(vm, &d)?;
    {
        let w = d.wand.borrow();
        if w.images.iter().any(|i| w.image_format(i).is_empty()) {
            drop(w);
            return throw_cls(vm, Cls::Imagick, "Image has no format");
        }
    }
    let r = d.wand.borrow().images_blob();
    match r {
        Ok(b) => Ok(Value::String(PhpString::from_vec(b))),
        Err(_) => throw_cls(
            vm,
            Cls::Imagick,
            "Failed to get the image contents (empty or invalid image?)",
        ),
    }
}

/// MagickSetImageFormat(): a known format, else false.
fn set_format_of(w: &mut Wand, fmt: &str) -> MResult<()> {
    let up = codec::canonical_format(fmt);
    if codec::format_info(&up).is_none() && !goro_imagick::pseudo::is_pseudo(&up) {
        return Err(MagickError::with_desc(
            severity::OPTION_ERROR,
            "UnrecognizedImageFormat",
            fmt,
            "error/image.c/SetImageInfo",
        ));
    }
    if let Some(img) = w.image_mut() {
        img.format = match up.as_str() {
            "JPG" => "JPG".to_string(),
            _ => up,
        };
    }
    Ok(())
}

fn set_image_format(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "s", args)?;
    let d = this(o);
    not_empty(vm, &d)?;
    let fmt = s(&a[0]);
    let r = set_format_of(&mut d.wand.borrow_mut(), &fmt);
    match r {
        Ok(()) => Ok(Value::True),
        Err(_) => convert_exception(vm, None, Cls::Imagick, "Unable to set image format"),
    }
}

fn get_image_format(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    let d = this(o);
    not_empty(vm, &d)?;
    let w = d.wand.borrow();
    let fmt = w.image_format(w.image().unwrap());
    if fmt.is_empty() {
        drop(w);
        return throw_cls(vm, Cls::Imagick, "Image has no format");
    }
    Ok(str_val(&fmt))
}

fn set_format(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "s", args)?;
    let fmt = s(&a[0]);
    let up = codec::canonical_format(&fmt);
    if !fmt.is_empty() && codec::format_info(&up).is_none() && !goro_imagick::pseudo::is_pseudo(&up)
    {
        return convert_exception(vm, None, Cls::Imagick, "Unable to set format");
    }
    this(o).wand.borrow_mut().settings.format = fmt;
    Ok(Value::True)
}

fn get_format(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    Ok(str_val(&this(o).wand.borrow().settings.format))
}

fn get_image_mime_type(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    let d = this(o);
    not_empty(vm, &d)?;
    let fmt = {
        let w = d.wand.borrow();
        w.image_format(w.image().unwrap())
    };
    if fmt.is_empty() {
        return throw_cls(vm, Cls::Imagick, "Image has no format");
    }
    match codec::format_info(&fmt) {
        Some(fi) => Ok(str_val(fi.mime)),
        None => throw_cls(vm, Cls::Imagick, "Unable to get image mime-type"),
    }
}

fn get_image_filename(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    let d = this(o);
    let n = with_image(vm, &d, |i| i.filename.clone())?;
    Ok(str_val(&n))
}

fn set_image_filename(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "s", args)?;
    let n = s(&a[0]);
    image_op(vm, &this(o), "Unable to set image filename", |i| {
        i.filename = n;
        Ok(())
    })
}

fn get_filename(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    Ok(str_val(&this(o).wand.borrow().settings.filename))
}

fn set_filename(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "s", args)?;
    this(o).wand.borrow_mut().settings.filename = s(&a[0]);
    Ok(Value::True)
}

fn get_image_width(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    let v = with_image(vm, &this(o), |i| i.columns)?;
    Ok(Value::Long(v as i64))
}

fn get_image_height(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    let v = with_image(vm, &this(o), |i| i.rows)?;
    Ok(Value::Long(v as i64))
}

fn get_image_geometry(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    let (w, h) = with_image(vm, &this(o), |i| (i.columns, i.rows))?;
    let mut a = PhpArray::new();
    set(&mut a, "width", Value::Long(w as i64));
    set(&mut a, "height", Value::Long(h as i64));
    Ok(arr_val(a))
}

fn get_image_page(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    let p = with_image(vm, &this(o), |i| i.page)?;
    Ok(rect_val(p))
}

fn set_image_page(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "llll", args)?;
    let r = Rect::new(
        a[0].long_or(0).max(0) as usize,
        a[1].long_or(0).max(0) as usize,
        a[2].long_or(0),
        a[3].long_or(0),
    );
    image_op(vm, &this(o), "Unable to set image page", |i| {
        i.page = r;
        Ok(())
    })
}

fn reset_image_page(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "s", args)?;
    let spec = s(&a[0]);
    image_op(vm, &this(o), "Unable to reset image page", |i| {
        // ResetImagePage(): "" resets to the image size; a geometry sets it
        if spec.is_empty() {
            i.page = Rect::new(i.columns, i.rows, 0, 0);
            return Ok(());
        }
        let Some(g) = goro_imagick::Geometry::parse(&spec) else {
            return Err(MagickError::option("InvalidArgument", &spec));
        };
        if let Some(w) = g.width {
            i.page.width = w as usize;
            i.page.height = g.height.unwrap_or(w) as usize;
        }
        if g.x.is_some() || g.y.is_some() {
            i.page.x = g.x.unwrap_or(0.0) as i64;
            i.page.y = g.y.unwrap_or(0.0) as i64;
        }
        if spec.starts_with('0') && g.width == Some(0.0) && g.x.is_none() {
            i.page = Rect::new(i.columns, i.rows, 0, 0);
        }
        Ok(())
    })
}

fn get_page(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    let p = this(o).wand.borrow().settings.page.unwrap_or_default();
    Ok(rect_val(p))
}

fn set_page(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "llll", args)?;
    let r = Rect::new(
        a[0].long_or(0).max(0) as usize,
        a[1].long_or(0).max(0) as usize,
        a[2].long_or(0),
        a[3].long_or(0),
    );
    this(o).wand.borrow_mut().settings.page = Some(r);
    Ok(Value::True)
}

fn get_image_length(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    let v = with_image(vm, &this(o), |i| i.file_size)?;
    Ok(Value::Long(v as i64))
}

fn get_number_images(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    Ok(Value::Long(this(o).wand.borrow().len() as i64))
}

/// FormatMagickSize() of a file size.
fn format_size(n: u64) -> String {
    let f = n as f64;
    if f >= 1024.0 * 1024.0 * 1024.0 {
        format!(
            "{}GiB",
            goro_imagick::color::fmt_g(f / (1024.0 * 1024.0 * 1024.0))
        )
    } else if f >= 1024.0 * 1024.0 {
        format!("{}MiB", goro_imagick::color::fmt_g(f / (1024.0 * 1024.0)))
    } else {
        format!("{}B", n)
    }
}

fn format_description(fmt: &str) -> String {
    match codec::format_info(fmt) {
        Some(fi) => format!("{} ({})", fmt, fi.description),
        None => fmt.to_string(),
    }
}

fn identify_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "|b", args)?;
    let raw = a[0].bool_or(false);
    let d = this(o);
    not_empty(vm, &d)?;
    let w = d.wand.borrow();
    let img = w.image().unwrap();
    let fmt = w.image_format(img);
    let mut arr = PhpArray::new();
    set(&mut arr, "imageName", str_val(&img.filename));
    let mime = codec::format_info(&fmt)
        .map(|f| f.mime)
        .unwrap_or("unknown");
    set(&mut arr, "mimetype", str_val(mime));
    if !fmt.is_empty() {
        set(&mut arr, "format", str_val(&format_description(&fmt)));
    }
    set(&mut arr, "units", str_val(img.units.name()));
    set(&mut arr, "colorSpace", str_val(img.colorspace.name()));
    set(&mut arr, "type", str_val(img.identify_type().name()));
    set(&mut arr, "compression", str_val(img.compression.name()));
    set(&mut arr, "fileSize", str_val(&format_size(img.file_size)));
    let mut g = PhpArray::new();
    set(&mut g, "width", Value::Long(img.columns as i64));
    set(&mut g, "height", Value::Long(img.rows as i64));
    set(&mut arr, "geometry", arr_val(g));
    let mut r = PhpArray::new();
    set(&mut r, "x", Value::Double(img.x_resolution));
    set(&mut r, "y", Value::Double(img.y_resolution));
    set(&mut arr, "resolution", arr_val(r));
    set(&mut arr, "signature", str_val(&signature(img)));
    if raw {
        set(&mut arr, "rawOutput", str_val(&identify_text(&w, img)));
    }
    Ok(arr_val(arr))
}

/// A short "identify -verbose"-like text.
fn identify_text(w: &Wand, img: &Image) -> String {
    let fmt = w.image_format(img);
    let mut s = String::new();
    s.push_str(&format!("Image:\n  Filename: {}\n", img.filename));
    s.push_str(&format!("  Format: {}\n", format_description(&fmt)));
    s.push_str(&format!(
        "  Geometry: {}x{}+{}+{}\n",
        img.columns, img.rows, img.page.x, img.page.y
    ));
    s.push_str(&format!("  Units: {}\n", img.units.name()));
    s.push_str(&format!("  Colorspace: {}\n", img.colorspace.name()));
    s.push_str(&format!("  Type: {}\n", img.identify_type().name()));
    s.push_str(&format!("  Depth: {}-bit\n", img.depth));
    s.push_str(&format!("  Compression: {}\n", img.compression.name()));
    s.push_str(&format!("  Filesize: {}\n", format_size(img.file_size)));
    s.push_str(&format!("  Number pixels: {}\n", img.columns * img.rows));
    s
}

/// The image signature: SHA-256 of the pixels.
pub(crate) fn signature(img: &Image) -> String {
    let mut data = Vec::with_capacity(img.pixels.len() * 2);
    for p in img.pixels.chunks_exact(4) {
        for &v in &p[..3] {
            data.extend_from_slice(&v.to_be_bytes());
        }
        if img.alpha {
            data.extend_from_slice(&p[3].to_be_bytes());
        }
    }
    sha256_hex(&data)
}

fn sha256_hex(data: &[u8]) -> String {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    let mut msg = data.to_vec();
    let bitlen = (data.len() as u64).wrapping_mul(8);
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bitlen.to_be_bytes());
    for chunk in msg.chunks_exact(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                chunk[i * 4],
                chunk[i * 4 + 1],
                chunk[i * 4 + 2],
                chunk[i * 4 + 3],
            ]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let mut v = h;
        for i in 0..64 {
            let s1 = v[4].rotate_right(6) ^ v[4].rotate_right(11) ^ v[4].rotate_right(25);
            let ch = (v[4] & v[5]) ^ (!v[4] & v[6]);
            let t1 = v[7]
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = v[0].rotate_right(2) ^ v[0].rotate_right(13) ^ v[0].rotate_right(22);
            let maj = (v[0] & v[1]) ^ (v[0] & v[2]) ^ (v[1] & v[2]);
            let t2 = s0.wrapping_add(maj);
            v[7] = v[6];
            v[6] = v[5];
            v[5] = v[4];
            v[4] = v[3].wrapping_add(t1);
            v[3] = v[2];
            v[2] = v[1];
            v[1] = v[0];
            v[0] = t1.wrapping_add(t2);
        }
        for i in 0..8 {
            h[i] = h[i].wrapping_add(v[i]);
        }
    }
    h.iter().map(|x| format!("{:08x}", x)).collect()
}

fn identify_format(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "s", args)?;
    let spec = s(&a[0]);
    let d = this(o);
    not_empty(vm, &d)?;
    let w = d.wand.borrow();
    let img = w.image().unwrap();
    Ok(str_val(&interpret_escapes(&w, img, &spec)))
}

/// InterpretImageProperties(): the %-escapes of identify -format.
pub(crate) fn interpret_escapes(w: &Wand, img: &Image, spec: &str) -> String {
    let mut out = String::new();
    let b: Vec<char> = spec.chars().collect();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c == '\\' && i + 1 < b.len() {
            out.push(match b[i + 1] {
                'n' => '\n',
                'r' => '\r',
                't' => '\t',
                x => x,
            });
            i += 2;
            continue;
        }
        if c != '%' || i + 1 >= b.len() {
            out.push(c);
            i += 1;
            continue;
        }
        let e = b[i + 1];
        i += 2;
        let fmt = w.image_format(img);
        let v = match e {
            'w' => img.columns.to_string(),
            'h' => img.rows.to_string(),
            'W' => img.page.width.to_string(),
            'H' => img.page.height.to_string(),
            'X' => format!("{:+}", img.page.x),
            'Y' => format!("{:+}", img.page.y),
            'm' => fmt,
            'f' => img.filename.rsplit('/').next().unwrap_or("").to_string(),
            'd' => img
                .filename
                .rsplit_once('/')
                .map(|(d, _)| d.to_string())
                .unwrap_or_default(),
            'e' => img
                .filename
                .rsplit_once('.')
                .map(|(_, e)| e.to_string())
                .unwrap_or_default(),
            't' => {
                let base = img.filename.rsplit('/').next().unwrap_or("");
                base.rsplit_once('.')
                    .map(|(t, _)| t.to_string())
                    .unwrap_or(base.to_string())
            }
            'M' | 'i' => img.filename.clone(),
            'n' => w.len().to_string(),
            's' => img.scene.to_string(),
            'p' => w.current.to_string(),
            'z' => img.depth.to_string(),
            'k' => img.number_colors().to_string(),
            'r' => format!(
                "{}{} {}",
                if img.colors.is_some() {
                    "PseudoClass"
                } else {
                    "DirectClass"
                },
                if img.alpha { "Alpha" } else { "" },
                img.colorspace.name()
            ),
            'g' => format!(
                "{}x{}{:+}{:+}",
                img.page.width, img.page.height, img.page.x, img.page.y
            ),
            'x' => goro_imagick::color::fmt_g(img.x_resolution),
            'y' => goro_imagick::color::fmt_g(img.y_resolution),
            'b' => format!("{}B", img.file_size),
            'q' => img.depth.to_string(),
            'Q' => img.quality.to_string(),
            'T' => img.delay.to_string(),
            'U' => img.units.name().to_string(),
            'C' => img.compression.name().to_string(),
            'G' => format!("{}x{}", img.magick_columns, img.magick_rows),
            'c' => img.property("comment").unwrap_or("").to_string(),
            'l' => img.property("label").unwrap_or("").to_string(),
            '#' => signature(img),
            '%' => "%".to_string(),
            '[' => {
                let end = b[i..].iter().position(|&x| x == ']');
                match end {
                    Some(n) => {
                        let key: String = b[i..i + n].iter().collect();
                        i += n + 1;
                        escape_property(w, img, &key)
                    }
                    None => "%[".to_string(),
                }
            }
            x => format!("%{}", x),
        };
        out.push_str(&v);
    }
    out
}

fn escape_property(w: &Wand, img: &Image, key: &str) -> String {
    match key {
        "type" => img.get_type().name().to_string(),
        "colorspace" => img.colorspace.name().to_string(),
        "magick" => w.image_format(img),
        "width" => img.columns.to_string(),
        "height" => img.rows.to_string(),
        "channels" => (if img.alpha { "srgba" } else { "srgb" }).to_string(),
        "opaque" => (if img.is_opaque() { "True" } else { "False" }).to_string(),
        "orientation" => img.orientation.name().to_string(),
        "page" => format!(
            "{}x{}{:+}{:+}",
            img.page.width, img.page.height, img.page.x, img.page.y
        ),
        "quality" => img.quality.to_string(),
        "scene" => img.scene.to_string(),
        "units" => img.units.name().to_string(),
        "version" => goro_imagick::VERSION_STRING.to_string(),
        "density" => format!(
            "{}x{}",
            goro_imagick::color::fmt_g(img.x_resolution),
            goro_imagick::color::fmt_g(img.y_resolution)
        ),
        _ => img
            .properties
            .get(key)
            .or_else(|| img.artifacts.get(key))
            .or_else(|| w.settings.options.get(key))
            .unwrap_or("")
            .to_string(),
    }
}

fn identify_image_type(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    let t = with_image(vm, &this(o), |i| i.identify_type())?;
    Ok(Value::Long(t.0 as i64))
}

fn get_image_compression_quality(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    let q = with_image(vm, &this(o), |i| i.quality)?;
    Ok(Value::Long(q as i64))
}

fn set_image_compression_quality(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "l", args)?;
    let q = a[0].long_or(0).max(0) as u32;
    image_op(
        vm,
        &this(o),
        "Unable to set image compression quality",
        |i| {
            i.quality = q;
            Ok(())
        },
    )
}

fn get_compression_quality(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    Ok(Value::Long(this(o).wand.borrow().settings.quality as i64))
}

fn set_compression_quality(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "l", args)?;
    this(o).wand.borrow_mut().settings.quality = a[0].long_or(0).max(0) as u32;
    Ok(Value::True)
}

fn get_image_compression(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    let c = with_image(vm, &this(o), |i| i.compression)?;
    Ok(Value::Long(c.0 as i64))
}

fn set_image_compression(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "l", args)?;
    let c = CompressionType(a[0].long_or(0) as i32);
    image_op(vm, &this(o), "Unable to set image compression", |i| {
        i.compression = c;
        Ok(())
    })
}

fn get_compression(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    Ok(Value::Long(
        this(o).wand.borrow().settings.compression.0 as i64,
    ))
}

fn set_compression(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "l", args)?;
    this(o).wand.borrow_mut().settings.compression = CompressionType(a[0].long_or(0) as i32);
    Ok(Value::True)
}

fn strip_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    image_op(vm, &this(o), "Unable to strip image", |i| {
        i.strip();
        Ok(())
    })
}

// ---------------------------------------------------------------------------
// properties, artifacts, options, profiles

fn get_image_property(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "s", args)?;
    let k = s(&a[0]);
    let d = this(o);
    not_empty(vm, &d)?;
    let w = d.wand.borrow();
    let img = w.image().unwrap();
    let v = img.properties.get(&k).map(|v| v.to_string()).or_else(|| {
        // computed properties
        match k.to_ascii_lowercase().as_str() {
            "exif:*" => None,
            _ => None,
        }
    });
    Ok(match v {
        Some(v) => str_val(&v),
        None => Value::False,
    })
}

fn set_image_property(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "ss", args)?;
    let (k, v) = (s(&a[0]), s(&a[1]));
    image_op(vm, &this(o), "Unable to set image property", |i| {
        i.properties.set(&k, &v);
        Ok(())
    })
}

fn delete_image_property(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "s", args)?;
    let k = s(&a[0]);
    let r = with_image(vm, &this(o), |i| i.properties.remove(&k))?;
    Ok(bool_val(r))
}

fn list_to_value(list: Vec<(String, String)>, values: bool) -> Value {
    let mut a = PhpArray::new();
    for (k, v) in list {
        if values {
            a.set(
                ArrayKey::String(PhpString::from_bytes(k.as_bytes())),
                str_val(&v),
            );
        } else {
            a.push(str_val(&k));
        }
    }
    arr_val(a)
}

fn get_image_properties(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "|sb", args)?;
    let pat = a[0]
        .str()
        .map(|p| String::from_utf8_lossy(p).into_owned())
        .unwrap_or("*".into());
    let values = a[1].bool_or(true);
    let list = with_image(vm, &this(o), |i| i.properties.matching(&pat))?;
    Ok(list_to_value(list, values))
}

fn comment_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "s", args)?;
    let v = s(&a[0]);
    image_op(vm, &this(o), "Unable to comment image", |i| {
        i.properties.set("comment", &v);
        Ok(())
    })
}

fn label_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "s", args)?;
    let v = s(&a[0]);
    image_op(vm, &this(o), "Unable to label image", |i| {
        i.properties.set("label", &v);
        Ok(())
    })
}

fn get_image_artifact(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "s", args)?;
    let k = s(&a[0]);
    let v = with_image(vm, &this(o), |i| i.artifacts.get(&k).map(|v| v.to_string()))?;
    Ok(match v {
        Some(v) => str_val(&v),
        None => Value::Null,
    })
}

fn set_image_artifact(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "ss!", args)?;
    let k = s(&a[0]);
    let v = a[1].str().map(|v| String::from_utf8_lossy(v).into_owned());
    image_op(vm, &this(o), "Unable to set image artifact", |i| {
        match v {
            Some(v) => i.artifacts.set(&k, &v),
            None => {
                i.artifacts.remove(&k);
            }
        }
        Ok(())
    })
}

fn delete_image_artifact(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "s", args)?;
    let k = s(&a[0]);
    image_op(vm, &this(o), "Unable to delete image artifact", |i| {
        if i.artifacts.remove(&k) {
            Ok(())
        } else {
            Err(MagickError::new(0, ""))
        }
    })
}

fn get_image_artifacts(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "|s", args)?;
    let pat = a[0]
        .str()
        .map(|p| String::from_utf8_lossy(p).into_owned())
        .unwrap_or("*".into());
    let list = with_image(vm, &this(o), |i| i.artifacts.matching(&pat))?;
    Ok(list_to_value(list, true))
}

fn get_option(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "s", args)?;
    let k = s(&a[0]);
    let v = this(o)
        .wand
        .borrow()
        .settings
        .options
        .get(&k)
        .map(|v| v.to_string());
    Ok(match v {
        Some(v) => str_val(&v),
        None => Value::Null,
    })
}

fn set_option(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "ss", args)?;
    let (k, v) = (s(&a[0]), s(&a[1]));
    let d = this(o);
    let mut w = d.wand.borrow_mut();
    w.settings.options.set(&k, &v);
    // a define also reaches the images (the coders read them there)
    for img in w.images.iter_mut() {
        img.artifacts.set(&k, &v);
    }
    Ok(Value::True)
}

fn delete_option(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "s", args)?;
    let k = s(&a[0]);
    let d = this(o);
    let removed = d.wand.borrow_mut().settings.options.remove(&k);
    if !removed {
        return convert_exception(vm, None, Cls::Imagick, "Unable to deleteOption");
    }
    Ok(Value::True)
}

fn get_options(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "|s", args)?;
    let pat = a[0]
        .str()
        .map(|p| String::from_utf8_lossy(p).into_owned())
        .unwrap_or("*".into());
    let list = this(o).wand.borrow().settings.options.matching(&pat);
    Ok(list_to_value(list, true))
}

fn get_image_profile(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "s", args)?;
    let k = s(&a[0]);
    let v = with_image(vm, &this(o), |i| i.profile(&k).map(|p| p.to_vec()))?;
    match v {
        Some(v) => Ok(Value::String(PhpString::from_vec(v))),
        None => throw_cls(vm, Cls::Imagick, "Can not get image profile"),
    }
}

fn get_image_profiles(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "|sb", args)?;
    let pat = a[0]
        .str()
        .map(|p| String::from_utf8_lossy(p).into_owned())
        .unwrap_or("*".into());
    let values = a[1].bool_or(true);
    let list = with_image(vm, &this(o), |i| {
        let mut v: Vec<(String, Vec<u8>)> = i
            .profiles
            .iter()
            .filter(|(k, _)| goro_imagick::image::glob_match(&pat, k))
            .cloned()
            .collect();
        v.sort_by(|a, b| a.0.cmp(&b.0));
        v
    })?;
    let mut arr = PhpArray::new();
    for (k, v) in list {
        if values {
            arr.set(
                ArrayKey::String(PhpString::from_bytes(k.as_bytes())),
                Value::String(PhpString::from_vec(v)),
            );
        } else {
            arr.push(str_val(&k));
        }
    }
    Ok(arr_val(arr))
}

fn set_image_profile(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "ss", args)?;
    let k = s(&a[0]);
    let v = a[1].str_or(b"").to_vec();
    image_op(vm, &this(o), "Unable to set image profile", |i| {
        i.set_profile(&k, v);
        if k.eq_ignore_ascii_case("exif") {
            goro_imagick::properties::parse_profiles(i);
        }
        Ok(())
    })
}

fn remove_image_profile(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "s", args)?;
    let k = s(&a[0]);
    let v = with_image(vm, &this(o), |i| i.remove_profile(&k))?;
    match v {
        Some(v) => Ok(Value::String(PhpString::from_vec(v))),
        None => throw_cls(vm, Cls::Imagick, "The image profile does not exist"),
    }
}

fn profile_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "ss!", args)?;
    let k = s(&a[0]);
    let v = a[1].str().map(|v| v.to_vec());
    image_op(vm, &this(o), "Unable to profile image", |i| {
        // without colour management the profile is attached / removed
        match v {
            Some(v) if !v.is_empty() => {
                i.set_profile(&k, v);
            }
            _ => {
                if k == "*" {
                    i.profiles.clear();
                } else {
                    let pat = k.clone();
                    i.profiles
                        .retain(|(n, _)| !goro_imagick::image::glob_match(&pat, n));
                }
            }
        }
        Ok(())
    })
}

// ---------------------------------------------------------------------------
// library information (static)

fn get_version(vm: &mut Vm, f: &Func, _o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    let mut a = PhpArray::new();
    set(
        &mut a,
        "versionNumber",
        Value::Long(goro_imagick::LIB_VERSION as i64),
    );
    set(
        &mut a,
        "versionString",
        str_val(goro_imagick::VERSION_STRING),
    );
    Ok(arr_val(a))
}

fn get_copyright(vm: &mut Vm, f: &Func, _o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    Ok(str_val("(C) 1999 ImageMagick Studio LLC"))
}

fn get_package_name(vm: &mut Vm, f: &Func, _o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    Ok(str_val("ImageMagick"))
}

fn get_release_date(vm: &mut Vm, f: &Func, _o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    Ok(str_val(goro_imagick::RELEASE_DATE))
}

fn get_home_url(vm: &mut Vm, f: &Func, _o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    Ok(str_val("https://imagemagick.org"))
}

fn get_quantum(vm: &mut Vm, f: &Func, _o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    Ok(Value::Long(65535))
}

fn get_quantum_depth(vm: &mut Vm, f: &Func, _o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    let mut a = PhpArray::new();
    set(&mut a, "quantumDepthLong", Value::Long(16));
    set(&mut a, "quantumDepthString", str_val("Q16"));
    Ok(arr_val(a))
}

fn get_quantum_range(vm: &mut Vm, f: &Func, _o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    let mut a = PhpArray::new();
    set(&mut a, "quantumRangeLong", Value::Long(65535));
    set(&mut a, "quantumRangeString", str_val("65535"));
    Ok(arr_val(a))
}

fn get_hdri_enabled(vm: &mut Vm, f: &Func, _o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    Ok(Value::False)
}

fn get_features(vm: &mut Vm, f: &Func, _o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    Ok(str_val("Cipher DPC "))
}

const CONFIGURE_OPTIONS: &[(&str, &str)] = &[
    ("CC", "rustc"),
    ("CHANNEL_MASK_DEPTH", "32"),
    ("COPYRIGHT", "Copyright (C) 1999 ImageMagick Studio LLC"),
    ("DOCUMENTATION_PATH", "https://imagemagick.org/script/"),
    ("FEATURES", "Cipher DPC"),
    ("HOST", "x86_64-pc-linux-gnu"),
    ("LIB_VERSION", "0x712"),
    ("LIB_VERSION_NUMBER", "7,1,2,18"),
    ("NAME", "ImageMagick"),
    ("QuantumDepth", "16"),
    ("RELEASE_DATE", "2026-03-22"),
    ("VERSION", "7.1.2"),
];

fn get_configure_options(vm: &mut Vm, f: &Func, _o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "|s", args)?;
    let pat = a[0]
        .str()
        .map(|p| String::from_utf8_lossy(p).into_owned())
        .unwrap_or("*".into());
    let mut arr = PhpArray::new();
    for (k, v) in CONFIGURE_OPTIONS {
        if goro_imagick::image::glob_match(&pat, k) {
            set(&mut arr, k, str_val(v));
        }
    }
    Ok(arr_val(arr))
}

thread_local! {
    static RESOURCE_LIMITS: std::cell::RefCell<Vec<(i64, f64)>> = const { std::cell::RefCell::new(Vec::new()) };
    static REGISTRY: std::cell::RefCell<Vec<(String, String)>> = const { std::cell::RefCell::new(Vec::new()) };
}

fn default_limit(t: i64) -> f64 {
    match t {
        // area, disk, file, map, memory, thread, time, height, width, list length
        0 => 0.0,
        1 => 3.4359738368e10,
        2 => 1.8446744073709552e19,
        3 => 768.0,
        4 => 3.4359738368e10,
        5 => 1.7179869184e10,
        6 => 1.0,
        8 => 1.8446744073709552e19,
        _ => 1.8446744073709552e19,
    }
}

fn get_resource(vm: &mut Vm, f: &Func, _o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "l", args)?;
    Ok(Value::Long(0))
}

fn get_resource_limit(vm: &mut Vm, f: &Func, _o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "l", args)?;
    let t = a[0].long_or(0);
    let v = RESOURCE_LIMITS.with(|l| {
        l.borrow()
            .iter()
            .find(|(k, _)| *k == t)
            .map(|e| e.1)
            .unwrap_or_else(|| default_limit(t))
    });
    if v >= 9.2e18 {
        Ok(Value::Double(v))
    } else {
        Ok(Value::Long(v as i64))
    }
}

fn set_resource_limit(vm: &mut Vm, f: &Func, _o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "ld", args)?;
    let t = a[0].long_or(0);
    let v = a[1].double_or(0.0);
    RESOURCE_LIMITS.with(|l| {
        let mut l = l.borrow_mut();
        l.retain(|(k, _)| *k != t);
        l.push((t, v));
    });
    Ok(Value::True)
}

fn query_formats(vm: &mut Vm, f: &Func, _o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "|s", args)?;
    let pat = a[0]
        .str()
        .map(|p| String::from_utf8_lossy(p).into_owned())
        .unwrap_or("*".into());
    let mut arr = PhpArray::new();
    for fi in codec::FORMATS {
        if goro_imagick::image::glob_match(&pat, fi.name) {
            arr.push(str_val(fi.name));
        }
    }
    Ok(arr_val(arr))
}

fn query_fonts(vm: &mut Vm, f: &Func, _o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "|s", args)?;
    let pat = a[0]
        .str()
        .map(|p| String::from_utf8_lossy(p).into_owned())
        .unwrap_or("*".into());
    let mut arr = PhpArray::new();
    for (name, _) in goro_imagick::fonts::list() {
        if goro_imagick::image::glob_match(&pat, name) {
            arr.push(str_val(name));
        }
    }
    Ok(arr_val(arr))
}

fn set_registry(vm: &mut Vm, f: &Func, _o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "ss", args)?;
    let (k, v) = (s(&a[0]), s(&a[1]));
    REGISTRY.with(|r| {
        let mut r = r.borrow_mut();
        r.retain(|(n, _)| *n != k);
        r.push((k, v));
    });
    Ok(Value::True)
}

fn get_registry(vm: &mut Vm, f: &Func, _o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "s", args)?;
    let k = s(&a[0]);
    let v = REGISTRY.with(|r| {
        r.borrow()
            .iter()
            .find(|(n, _)| *n == k)
            .map(|e| e.1.clone())
    });
    match v {
        Some(v) => Ok(str_val(&v)),
        None => Ok(Value::False),
    }
}

fn list_registry(vm: &mut Vm, f: &Func, _o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    let mut arr = PhpArray::new();
    REGISTRY.with(|r| {
        for (k, v) in r.borrow().iter() {
            set(&mut arr, k, str_val(v));
        }
    });
    Ok(arr_val(arr))
}

fn get_imagemagick_license(vm: &mut Vm, f: &Func, _o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    Ok(str_val(
        "goro-imagick is a clean-room implementation of the ImageMagick 7 API semantics in Rust, under the MIT license; it contains no ImageMagick code.\n",
    ))
}

fn set_seed(vm: &mut Vm, f: &Func, _o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "l", args)?;
    Ok(Value::Null)
}

fn round_helper(v: f64) -> i64 {
    if v >= 0.0 {
        (v + 0.5).floor() as i64
    } else {
        (v - 0.5).ceil() as i64
    }
}

/// s_calculate_crop()
pub(crate) fn calc_crop(ow: i64, oh: i64, dw: i64, dh: i64, legacy: bool) -> (i64, i64, i64, i64) {
    let rx = dw as f64 / ow as f64;
    let ry = dh as f64 / oh as f64;
    let (nw, nh) = if rx > ry {
        (
            dw,
            if legacy {
                (rx * oh as f64) as i64
            } else {
                round_helper(rx * oh as f64)
            },
        )
    } else {
        (
            if legacy {
                (ry * ow as f64) as i64
            } else {
                round_helper(ry * ow as f64)
            },
            dh,
        )
    };
    (nw, nh, (nw - dw) / 2, (nh - dh) / 2)
}

fn calculate_crop(vm: &mut Vm, f: &Func, _o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "llll|b", args)?;
    let (ow, oh, dw, dh) = (
        a[0].long_or(0),
        a[1].long_or(0),
        a[2].long_or(0),
        a[3].long_or(0),
    );
    if ow <= 0 || oh <= 0 || dw <= 0 || dh <= 0 {
        let _ = throw_cls(vm, Cls::Imagick, "All values must be above zero.");
    }
    let (nw, nh, ox, oy) = calc_crop(ow, oh, dw, dh, a[4].bool_or(false));
    let mut arr = PhpArray::new();
    set(&mut arr, "width", Value::Long(nw));
    set(&mut arr, "height", Value::Long(nh));
    set(&mut arr, "offset_x", Value::Long(ox));
    set(&mut arr, "offset_y", Value::Long(oy));
    Ok(arr_val(arr))
}

// ---------------------------------------------------------------------------
// settings

fn set_size(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "ll", args)?;
    this(o).wand.borrow_mut().settings.size = Some((
        a[0].long_or(0).max(0) as usize,
        a[1].long_or(0).max(0) as usize,
    ));
    Ok(Value::True)
}

fn get_size(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    let (w, h) = this(o).wand.borrow().settings.size.unwrap_or((0, 0));
    let mut a = PhpArray::new();
    set(&mut a, "columns", Value::Long(w as i64));
    set(&mut a, "rows", Value::Long(h as i64));
    Ok(arr_val(a))
}

fn set_size_offset(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "lll", args)?;
    let d = this(o);
    let mut w = d.wand.borrow_mut();
    w.settings.size = Some((
        a[0].long_or(0).max(0) as usize,
        a[1].long_or(0).max(0) as usize,
    ));
    w.settings.size_offset = a[2].long_or(0);
    Ok(Value::True)
}

fn get_size_offset(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    Ok(Value::Long(this(o).wand.borrow().settings.size_offset))
}

fn set_resolution(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "dd", args)?;
    this(o).wand.borrow_mut().settings.density = Some((a[0].double_or(0.0), a[1].double_or(0.0)));
    Ok(Value::True)
}

fn xy_val(x: f64, y: f64) -> Value {
    let mut a = PhpArray::new();
    set(&mut a, "x", Value::Double(x));
    set(&mut a, "y", Value::Double(y));
    arr_val(a)
}

fn get_resolution(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    let (x, y) = this(o)
        .wand
        .borrow()
        .settings
        .density
        .unwrap_or((72.0, 72.0));
    Ok(xy_val(x, y))
}

fn set_image_resolution(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "dd", args)?;
    let (x, y) = (a[0].double_or(0.0), a[1].double_or(0.0));
    image_op(vm, &this(o), "Unable to set image resolution", |i| {
        i.x_resolution = x;
        i.y_resolution = y;
        Ok(())
    })
}

fn get_image_resolution(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    let (x, y) = with_image(vm, &this(o), |i| (i.x_resolution, i.y_resolution))?;
    Ok(xy_val(x, y))
}

macro_rules! int_attr {
    ($get:ident, $set:ident, $field:ident, $ty:ident, $msg:expr) => {
        fn $get(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
            zpp::parse(vm, f, "", args)?;
            let v = with_image(vm, &this(o), |i| i.$field.0)?;
            Ok(Value::Long(v as i64))
        }
        fn $set(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
            let a = zpp::parse(vm, f, "l", args)?;
            let v = $ty(a[0].long_or(0) as i32);
            image_op(vm, &this(o), $msg, |i| {
                i.$field = v;
                Ok(())
            })
        }
    };
}

int_attr!(
    get_image_units,
    set_image_units,
    units,
    ResolutionType,
    "Unable to set image units"
);
int_attr!(
    get_image_dispose,
    set_image_dispose,
    dispose,
    DisposeType,
    "Unable to set image dispose"
);
int_attr!(
    get_image_gravity,
    set_image_gravity,
    gravity,
    Gravity,
    "Unable to set image gravity"
);
int_attr!(
    get_image_compose,
    set_image_compose,
    compose,
    CompositeOp,
    "Unable to set image composite operator"
);
int_attr!(
    get_image_interlace_scheme,
    set_image_interlace_scheme,
    interlace,
    InterlaceType,
    "Unable to set image interlace scheme"
);
int_attr!(
    get_image_orientation,
    set_image_orientation,
    orientation,
    OrientationType,
    "Unable to set image orientation"
);
int_attr!(
    get_image_virtual_pixel_method,
    set_image_virtual_pixel_method,
    virtual_pixel,
    VirtualPixelMethod,
    "Unable to set image virtual pixel method"
);
int_attr!(
    get_image_interpolate_method,
    set_image_interpolate_method,
    interpolate,
    PixelInterpolateMethod,
    "Unable to set the image interpolate method"
);

macro_rules! u64_attr {
    ($get:ident, $set:ident, $field:ident, $t:ty, $msg:expr) => {
        fn $get(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
            zpp::parse(vm, f, "", args)?;
            let v = with_image(vm, &this(o), |i| i.$field)?;
            Ok(Value::Long(v as i64))
        }
        fn $set(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
            let a = zpp::parse(vm, f, "l", args)?;
            let v = a[0].long_or(0).max(0) as $t;
            image_op(vm, &this(o), $msg, |i| {
                i.$field = v;
                Ok(())
            })
        }
    };
}

u64_attr!(
    get_image_delay,
    set_image_delay,
    delay,
    u64,
    "Unable to set image delay"
);
u64_attr!(
    get_image_iterations,
    set_image_iterations,
    iterations,
    u64,
    "Unable to set image iterations"
);
u64_attr!(
    get_image_scene,
    set_image_scene,
    scene,
    usize,
    "Unable to set image scene"
);

fn get_image_ticks_per_second(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    let v = with_image(vm, &this(o), |i| i.ticks_per_second)?;
    Ok(Value::Long(v))
}

fn set_image_ticks_per_second(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "l", args)?;
    let v = a[0].long_or(0);
    image_op(vm, &this(o), "Unable to set image ticks per second", |i| {
        // MagickSetImageTicksPerSecond() rescales the delay
        if i.ticks_per_second > 0 && v > 0 {
            i.delay = (i.delay as f64 * v as f64 / i.ticks_per_second as f64).round() as u64;
        }
        i.ticks_per_second = v;
        Ok(())
    })
}

fn set_gravity(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "l", args)?;
    this(o).wand.borrow_mut().settings.gravity = Gravity(a[0].long_or(0) as i32);
    Ok(Value::True)
}

fn get_gravity(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    Ok(Value::Long(this(o).wand.borrow().settings.gravity.0 as i64))
}

fn get_image_depth(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    let v = with_image(vm, &this(o), |i| i.depth)?;
    Ok(Value::Long(v as i64))
}

fn set_image_depth(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "l", args)?;
    let v = a[0].long_or(0).clamp(1, 64) as u32;
    image_op(vm, &this(o), "Unable to set image depth", |i| {
        set_depth_of(i, v);
        Ok(())
    })
}

/// SetImageDepth(): quantize the samples to the depth (black-box: the
/// scale down to the depth truncates, the scale back up rounds).
fn set_depth_of(i: &mut Image, depth: u32) {
    quantize_depth(i, depth, [true; 5]);
    i.depth = depth.min(16);
}

/// Quantize the samples of the channels `mask` selects (red, green,
/// blue, alpha, black) to `depth` bits.
fn quantize_depth(i: &mut Image, depth: u32, mask: [bool; 5]) {
    if depth >= 16 {
        return;
    }
    let levels = (1u64 << depth) - 1;
    let quantize = |v: &mut u16| {
        let q = *v as u64 * levels / 65535;
        *v = ((q * 65535 * 2 + levels) / (2 * levels)) as u16;
    };
    for p in i.pixels.chunks_exact_mut(4) {
        for c in 0..4 {
            if mask[c] {
                quantize(&mut p[c]);
            }
        }
    }
    if mask[4] {
        if let Some(k) = i.black.as_mut() {
            k.iter_mut().for_each(quantize);
        }
    }
    i.colors = None;
}

fn set_depth(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "l", args)?;
    this(o).wand.borrow_mut().settings.depth = a[0].long_or(16).clamp(1, 64) as u32;
    Ok(Value::True)
}

fn set_image_channel_depth(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "ll", args)?;
    let v = a[1].long_or(0).clamp(1, 64) as u32;
    let ch = a[0].long_or(0);
    image_op(vm, &this(o), "Unable to set image channel depth", |i| {
        // only the channels named change, the image depth follows
        let all = ch == goro_imagick::types::channel::ALL || ch == 0x7fffffff;
        let bit = |b: i64| all || ch & b != 0;
        use goro_imagick::types::channel as c;
        quantize_depth(
            i,
            v,
            [
                bit(c::RED),
                bit(c::GREEN),
                bit(c::BLUE),
                bit(c::ALPHA),
                bit(c::BLACK),
            ],
        );
        i.depth = v.min(16);
        Ok(())
    })
}

fn get_image_channel_depth(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "l", args)?;
    let v = with_image(vm, &this(o), |i| i.depth)?;
    Ok(Value::Long(v as i64))
}

fn get_image_type(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    let t = with_image(vm, &this(o), |i| {
        if i.image_type != ImageType::Undefined {
            return i.image_type;
        }
        // GetImageType(): from the storage class / colorspace / alpha
        let alpha = i.alpha;
        if i.colorspace == Colorspace::CMYK {
            if alpha {
                ImageType::ColorSeparationAlpha
            } else {
                ImageType::ColorSeparation
            }
        } else if i.colorspace == Colorspace::Gray {
            if i.depth == 1 && !alpha {
                ImageType::Bilevel
            } else if alpha {
                ImageType::GrayscaleAlpha
            } else {
                ImageType::Grayscale
            }
        } else if i.colors.is_some() {
            if alpha {
                ImageType::PaletteAlpha
            } else {
                ImageType::Palette
            }
        } else if alpha {
            ImageType::TrueColorAlpha
        } else {
            ImageType::TrueColor
        }
    })?;
    Ok(Value::Long(t.0 as i64))
}

fn set_image_type(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "l", args)?;
    let t = ImageType(a[0].long_or(0) as i32);
    image_op(vm, &this(o), "Unable to set image type", |i| {
        crate::ops::set_type_of(i, t);
        Ok(())
    })
}

fn set_type(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "l", args)?;
    this(o).wand.borrow_mut().settings.image_type = ImageType(a[0].long_or(0) as i32);
    Ok(Value::True)
}

fn get_type(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    Ok(Value::Long(
        this(o).wand.borrow().settings.image_type.0 as i64,
    ))
}

fn get_image_colorspace(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    let v = with_image(vm, &this(o), |i| i.colorspace.0)?;
    Ok(Value::Long(v as i64))
}

fn set_image_colorspace(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "l", args)?;
    let cs = Colorspace(a[0].long_or(0) as i32);
    image_op(vm, &this(o), "Unable to set image colorspace", |i| {
        // SetImageColorspace(): the pixels are not converted
        if cs == Colorspace::CMYK && i.black.is_none() {
            i.black = Some(vec![0; i.columns * i.rows]);
        }
        if cs != Colorspace::CMYK {
            i.black = None;
        }
        i.colorspace = cs;
        Ok(())
    })
}

fn set_colorspace(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "l", args)?;
    this(o).wand.borrow_mut().settings.colorspace = Colorspace(a[0].long_or(0) as i32);
    Ok(Value::True)
}

fn get_colorspace(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    Ok(Value::Long(
        this(o).wand.borrow().settings.colorspace.0 as i64,
    ))
}

fn set_interlace_scheme(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "l", args)?;
    this(o).wand.borrow_mut().settings.interlace = InterlaceType(a[0].long_or(0) as i32);
    Ok(Value::True)
}

fn get_interlace_scheme(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    Ok(Value::Long(
        this(o).wand.borrow().settings.interlace.0 as i64,
    ))
}

fn set_orientation(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "l", args)?;
    this(o).wand.borrow_mut().settings.orientation = OrientationType(a[0].long_or(0) as i32);
    Ok(Value::True)
}

fn get_orientation(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    Ok(Value::Long(
        this(o).wand.borrow().settings.orientation.0 as i64,
    ))
}

fn set_interpolate_method(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "l", args)?;
    this(o).wand.borrow_mut().settings.interpolate = PixelInterpolateMethod(a[0].long_or(0) as i32);
    Ok(Value::True)
}

fn get_interpolate_method(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    Ok(Value::Long(
        this(o).wand.borrow().settings.interpolate.0 as i64,
    ))
}

fn get_image_gamma(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    let v = with_image(vm, &this(o), |i| i.gamma)?;
    Ok(Value::Double(v))
}

fn set_image_gamma(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "d", args)?;
    let v = a[0].double_or(0.0);
    image_op(vm, &this(o), "Unable to set image gamma", |i| {
        i.gamma = v;
        Ok(())
    })
}

fn get_image_rendering_intent(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    let v = with_image(vm, &this(o), |i| i.rendering_intent)?;
    Ok(Value::Long(v as i64))
}

fn set_image_rendering_intent(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "l", args)?;
    let v = a[0].long_or(0) as i32;
    image_op(vm, &this(o), "Unable to set image rendering intent", |i| {
        i.rendering_intent = v;
        Ok(())
    })
}

macro_rules! primary {
    ($get:ident, $set:ident, $field:ident, $msg_set:expr) => {
        fn $get(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
            zpp::parse(vm, f, "", args)?;
            let (x, y, z) = with_image(vm, &this(o), |i| i.chromaticity.$field)?;
            let mut a = PhpArray::new();
            set(&mut a, "x", Value::Double(x));
            set(&mut a, "y", Value::Double(y));
            set(&mut a, "z", Value::Double(z));
            Ok(arr_val(a))
        }
        fn $set(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
            let a = zpp::parse(vm, f, "dd|d", args)?;
            let (x, y) = (a[0].double_or(0.0), a[1].double_or(0.0));
            let z = a[2].double_or(1.0 - x - y);
            image_op(vm, &this(o), $msg_set, |i| {
                i.chromaticity.$field = (x, y, z);
                Ok(())
            })
        }
    };
}

primary!(
    get_image_red_primary,
    set_image_red_primary,
    red,
    "Unable to set image red primary"
);
primary!(
    get_image_green_primary,
    set_image_green_primary,
    green,
    "Unable to set image green primary"
);
primary!(
    get_image_blue_primary,
    set_image_blue_primary,
    blue,
    "Unable to set image blue primary"
);
primary!(
    get_image_white_point,
    set_image_white_point,
    white,
    "Unable to set image white point"
);

fn set_antialias(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "b", args)?;
    this(o).wand.borrow_mut().settings.antialias = a[0].bool_or(true);
    Ok(Value::True)
}

fn get_antialias(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    Ok(bool_val(this(o).wand.borrow().settings.antialias))
}

fn set_font(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "s", args)?;
    let font = s(&a[0]);
    if font.is_empty() {
        return throw_cls(vm, Cls::Imagick, "Can not set empty font");
    }
    // php_imagick_check_font(): a known font, or a font file
    if goro_imagick::fonts::resolve(&font).is_none() {
        let abs = absolute_path(&font);
        throw(
            vm,
            b"ImagickException",
            &format!("The path does not exist: {}", abs),
            1,
        );
        return Err(());
    }
    this(o).wand.borrow_mut().settings.font = font;
    Ok(Value::True)
}

fn get_font(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    let font = this(o).wand.borrow().settings.font.clone();
    if font.is_empty() {
        return Ok(Value::False);
    }
    Ok(str_val(&font))
}

fn set_point_size(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "d", args)?;
    this(o).wand.borrow_mut().settings.pointsize = a[0].double_or(12.0);
    Ok(Value::True)
}

fn get_point_size(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    Ok(Value::Double(this(o).wand.borrow().settings.pointsize))
}

fn set_extract(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "s", args)?;
    this(o).wand.borrow_mut().settings.extract = s(&a[0]);
    Ok(Value::True)
}

fn set_sampling_factors(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "a", args)?;
    let Arg::Arr(arr) = &a[0] else {
        return Err(());
    };
    let mut v = Vec::new();
    for x in arr.borrow().values() {
        match x.deref() {
            Value::Long(n) => v.push(n as f64),
            Value::Double(d) => v.push(d),
            Value::String(_) => v.push(x.deref().to_double()),
            _ => return throw_cls(vm, Cls::Imagick, "Can't read array"),
        }
    }
    this(o).wand.borrow_mut().settings.sampling_factors = v;
    Ok(Value::True)
}

fn get_sampling_factors(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    let mut arr = PhpArray::new();
    for v in this(o).wand.borrow().settings.sampling_factors.iter() {
        arr.push(Value::Double(*v));
    }
    Ok(arr_val(arr))
}

fn set_background_color(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "z", args)?;
    let c = match &a[0] {
        Arg::Any(v) => zval_to_pixel(vm, v, Cls::Imagick)?,
        _ => return Err(()),
    };
    this(o).wand.borrow_mut().settings.background_color = c;
    Ok(Value::True)
}

fn get_background_color(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    let c = this(o).wand.borrow().settings.background_color.clone();
    Ok(new_pixel(vm, c))
}

macro_rules! color_attr {
    ($get:ident, $set:ident, $field:ident, $msg_set:expr) => {
        fn $get(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
            zpp::parse(vm, f, "", args)?;
            let c = with_image(vm, &this(o), |i| i.$field.clone())?;
            Ok(new_pixel(vm, c))
        }
        fn $set(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
            let a = zpp::parse(vm, f, "z", args)?;
            let c = match &a[0] {
                Arg::Any(v) => zval_to_pixel(vm, v, Cls::Imagick)?,
                _ => return Err(()),
            };
            image_op(vm, &this(o), $msg_set, |i| {
                i.$field = c;
                Ok(())
            })
        }
    };
}

color_attr!(
    get_image_background_color,
    set_image_background_color,
    background_color,
    "Unable to set image background color"
);
color_attr!(
    get_image_border_color,
    set_image_border_color,
    border_color,
    "Unable to set image border color"
);
color_attr!(
    get_image_matte_color,
    set_image_matte_color,
    matte_color,
    "Unable to set image matte color"
);

fn get_image_alpha_channel(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    let a = with_image(vm, &this(o), |i| i.alpha)?;
    Ok(bool_val(a))
}

fn set_image_alpha_channel(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "l", args)?;
    let op = AlphaChannelOption(a[0].long_or(0) as i32);
    image_op(vm, &this(o), "Unable to set image alpha channel", |i| {
        crate::ops::alpha_channel(i, op);
        Ok(())
    })
}

fn set_image_matte(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "b", args)?;
    let on = a[0].bool_or(false);
    image_op(vm, &this(o), "Unable to set image matte", |i| {
        if on {
            i.enable_alpha();
        } else {
            i.alpha = false;
        }
        Ok(())
    })
}

fn set_image_alpha(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "d", args)?;
    let v = goro_imagick::color::to_q16(a[0].double_or(1.0) * 65535.0);
    image_op(vm, &this(o), "Unable to set image alpha", |i| {
        // SetImageAlpha(): every pixel gets the alpha
        i.enable_alpha();
        for p in i.pixels.chunks_exact_mut(4) {
            p[3] = v;
        }
        i.colors = None;
        Ok(())
    })
}

fn get_image_signature(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    let s = with_image(vm, &this(o), |i| signature(i))?;
    Ok(str_val(&s))
}

fn get_image_pixel_color(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "ll", args)?;
    let (x, y) = (a[0].long_or(0), a[1].long_or(0));
    let c = with_image(vm, &this(o), |i| {
        if x >= 0 && y >= 0 && (x as usize) < i.columns && (y as usize) < i.rows {
            i.pixel_info(x as usize, y as usize)
        } else {
            // a virtual pixel
            let p = i.virtual_px(x, y);
            PixelInfo {
                red: p[0] as f64,
                green: p[1] as f64,
                blue: p[2] as f64,
                alpha: p[3] as f64,
                alpha_trait: i.alpha && p[3] != 65535,
                ..Default::default()
            }
        }
    })?;
    Ok(new_pixel(vm, c))
}

fn set_image_pixel_color(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "llz", args)?;
    let (x, y) = (a[0].long_or(0), a[1].long_or(0));
    let c = match &a[2] {
        Arg::Any(v) => zval_to_pixel(vm, v, Cls::Imagick)?,
        _ => return Err(()),
    };
    let d = this(o);
    let r = image_op(vm, &d, "Unable to set image pixel color", |i| {
        if x < 0 || y < 0 || x as usize >= i.columns || y as usize >= i.rows {
            return Err(MagickError::new(0, ""));
        }
        if c.alpha_trait && c.alpha < 65535.0 {
            i.enable_alpha();
        }
        let q = c.to_rgba16();
        i.set_px(x as usize, y as usize, q);
        i.colors = None;
        Ok(())
    })?;
    let _ = r;
    let c2 = with_image(vm, &d, |i| i.pixel_info(x as usize, y as usize))?;
    Ok(new_pixel(vm, c2))
}

const STORAGE_CHAR: i64 = 1;
const STORAGE_DOUBLE: i64 = 2;
const STORAGE_FLOAT: i64 = 3;
const STORAGE_LONG: i64 = 4;
const STORAGE_LONGLONG: i64 = 5;
const STORAGE_QUANTUM: i64 = 6;
const STORAGE_SHORT: i64 = 7;

/// php_imagick_validate_map()
fn validate_map(map: &str) -> bool {
    map.bytes()
        .all(|c| b"RGBAOCYMKIP".contains(&c.to_ascii_uppercase()))
}

fn export_image_pixels(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "llllsl", args)?;
    let (x, y, w, h) = (
        a[0].long_or(0),
        a[1].long_or(0),
        a[2].long_or(0),
        a[3].long_or(0),
    );
    let map = s(&a[4]);
    let storage = a[5].long_or(0);
    if x < 0 || y < 0 {
        return throw_cls(vm, Cls::Imagick, "The coordinates must be non-negative");
    }
    if w <= 0 || h <= 0 {
        return throw_cls(
            vm,
            Cls::Imagick,
            "The width and height must be greater than zero",
        );
    }
    if !validate_map(&map) {
        return throw_cls(vm, Cls::Imagick, "The map contains disallowed characters");
    }
    let d = this(o);
    not_empty(vm, &d)?;
    if !matches!(
        storage,
        STORAGE_CHAR
            | STORAGE_DOUBLE
            | STORAGE_FLOAT
            | STORAGE_LONG
            | STORAGE_LONGLONG
            | STORAGE_QUANTUM
            | STORAGE_SHORT
    ) {
        return throw_cls(vm, Cls::Imagick, "Unknown pixel type ");
    }
    let r = {
        let w_ = d.wand.borrow();
        goro_imagick::pixels::export_q16(w_.image().unwrap(), x, y, w as usize, h as usize, &map)
    };
    let samples = match r {
        Ok(s) => s,
        Err(e) => {
            return convert_exception(vm, Some(e), Cls::Imagick, "Unable to export image pixels");
        }
    };
    let mut arr = PhpArray::new();
    for q in samples {
        arr.push(match storage {
            STORAGE_CHAR => Value::Long(goro_imagick::image::q_to_u8(q) as i64),
            STORAGE_SHORT | STORAGE_QUANTUM => Value::Long(q as i64),
            STORAGE_LONG => Value::Long(q as i64 * 65537),
            STORAGE_LONGLONG => Value::Long((q as u64).wrapping_mul(281479271743489) as i64),
            STORAGE_FLOAT => Value::Double((q as f64 / 65535.0) as f32 as f64),
            _ => Value::Double(q as f64 / 65535.0),
        });
    }
    Ok(arr_val(arr))
}

fn import_image_pixels(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "llllsla", args)?;
    let (x, y, w, h) = (
        a[0].long_or(0),
        a[1].long_or(0),
        a[2].long_or(0),
        a[3].long_or(0),
    );
    let map = s(&a[4]);
    let storage = a[5].long_or(0);
    let d = this(o);
    not_empty(vm, &d)?;
    if x < 0 || y < 0 {
        return throw_cls(vm, Cls::Imagick, "The coordinates must be non-negative");
    }
    if w <= 0 || h <= 0 {
        return throw_cls(
            vm,
            Cls::Imagick,
            "The width and height must be greater than zero",
        );
    }
    let Arg::Arr(arr) = &a[6] else {
        return Err(());
    };
    let vals: Vec<Value> = arr.borrow().values().map(|v| v.deref()).collect();
    let expected = w * h * map.len() as i64;
    if vals.len() as i64 != expected {
        throw(
            vm,
            b"ImagickException",
            &format!(
                "The map contains incorrect number of elements. Expected {}, array has {}",
                expected,
                vals.len()
            ),
            0,
        );
        return Err(());
    }
    if !validate_map(&map) {
        return throw_cls(vm, Cls::Imagick, "The map contains disallowed characters");
    }
    let mut samples = Vec::with_capacity(vals.len());
    match storage {
        STORAGE_FLOAT | STORAGE_DOUBLE => {
            for v in &vals {
                let d = match v {
                    Value::Long(n) => *n as f64,
                    Value::Double(d) => *d,
                    _ => {
                        return throw_cls(
                            vm,
                            Cls::Imagick,
                            "The map must contain only numeric values",
                        );
                    }
                };
                samples.push(goro_imagick::color::to_q16(d * 65535.0));
            }
        }
        STORAGE_SHORT | STORAGE_LONG => {
            for v in &vals {
                let n = match v {
                    Value::Long(n) => *n,
                    Value::Double(d) => *d as i64,
                    _ => {
                        return throw_cls(
                            vm,
                            Cls::Imagick,
                            "The map must contain only numeric values",
                        );
                    }
                };
                // imported as LongPixel (32-bit)
                samples.push(((n.clamp(0, u32::MAX as i64) as u64 + 32768) / 65537) as u16);
            }
        }
        STORAGE_CHAR => {
            for v in &vals {
                let n = match v {
                    Value::Long(n) => *n,
                    _ => {
                        return throw_cls(
                            vm,
                            Cls::Imagick,
                            "The character array contains incorrect values",
                        );
                    }
                };
                samples.push((n.clamp(0, 255) as u16) * 257);
            }
        }
        _ => return throw_cls(vm, Cls::Imagick, "Unknown storage format"),
    }
    let r = {
        let mut w_ = d.wand.borrow_mut();
        let img = w_.image_mut().unwrap();
        let r = goro_imagick::pixels::import_q16(img, x, y, w as usize, h as usize, &map, &samples);
        img.colors = None;
        r
    };
    match r {
        Ok(()) => Ok(Value::True),
        Err(e) => convert_exception(vm, Some(e), Cls::Imagick, "Unable to import image pixels"),
    }
}

fn get_pixel_iterator(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    let d = this(o);
    not_empty(vm, &d)?;
    crate::pixel_iter::iterator_for(vm, o.unwrap(), None)
}

fn get_pixel_region_iterator(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "llll", args)?;
    let d = this(o);
    not_empty(vm, &d)?;
    crate::pixel_iter::iterator_for(
        vm,
        o.unwrap(),
        Some((
            a[0].long_or(0),
            a[1].long_or(0),
            a[2].long_or(0),
            a[3].long_or(0),
        )),
    )
}

fn get_image_colors(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    let n = with_image(vm, &this(o), |i| i.number_colors())?;
    Ok(Value::Long(n as i64))
}

fn get_image_histogram(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    let hist = with_image(vm, &this(o), |i| {
        let mut m: std::collections::BTreeMap<[u16; 4], u64> = Default::default();
        for p in i.pixels.chunks_exact(4) {
            let a = if i.alpha { p[3] } else { 65535 };
            *m.entry([p[0], p[1], p[2], a]).or_default() += 1;
        }
        (m, i.alpha)
    })?;
    let (m, alpha) = hist;
    let mut arr = PhpArray::new();
    for (c, n) in m {
        let mut pi = PixelInfo::from_rgba_q(c[0] as f64, c[1] as f64, c[2] as f64, c[3] as f64);
        pi.alpha_trait = alpha;
        pi.count = n;
        let v = new_pixel(vm, pi);
        arr.push(v);
    }
    Ok(arr_val(arr))
}

fn get_image_total_ink_density(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    let v = with_image(vm, &this(o), |i| {
        let mut img = i.clone();
        img.rgb_to_cmyk();
        let k = img.black.clone().unwrap_or_default();
        let mut max = 0.0f64;
        for (p, kk) in img.pixels.chunks_exact(4).zip(k.iter()) {
            let s = p[0] as f64 + p[1] as f64 + p[2] as f64 + *kk as f64;
            max = max.max(s);
        }
        max
    })?;
    Ok(Value::Double(v))
}

fn set_image_progress_monitor(vm: &mut Vm, f: &Func, _o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "s", args)?;
    if !crate::ini_bool(vm, "imagick.progress_monitor") {
        return throw_cls(
            vm,
            Cls::Imagick,
            "Progress monitoring is disabled in ini-settings",
        );
    }
    Ok(Value::True)
}

fn set_progress_monitor(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "z", args)?;
    if let Arg::Any(v) = &a[0] {
        let cb = v.deref();
        if matches!(cb, Value::Null) {
            *this(o).progress.borrow_mut() = None;
            return Ok(Value::True);
        }
        let r = vm.zend_is_callable_ex(&cb, 0);
        if !r.ok {
            return throw_cls(
                vm,
                Cls::Imagick,
                "First argument to setProgressMonitor is expected to be a valid callback",
            );
        }
        *this(o).progress.borrow_mut() = Some(cb);
    }
    Ok(Value::True)
}

fn set_image_channel_mask(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "l", args)?;
    let mask = a[0].long_or(0);
    // MagickSetImageChannelMask() returns the previous mask
    let prev = with_image(vm, &this(o), |i| {
        std::mem::replace(&mut i.channel_mask, mask)
    })?;
    Ok(Value::Long(prev))
}

#[allow(dead_code)]
fn unused(vm: &mut Vm) {
    let _ = bytes_val(b"");
    let _ = vm;
}
