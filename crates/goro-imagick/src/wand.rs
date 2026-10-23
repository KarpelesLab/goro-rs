//! The MagickWand: an image list with an iterator (current image,
//! insertion side, "pending" flag) and the wand-wide settings (the
//! ImageInfo): format, size, density, options...

use crate::codec::{self, ReadOptions, WriteOptions};
use crate::color::PixelInfo;
use crate::error::{MResult, MagickError, severity};
use crate::geometry::{Geometry, Rect};
use crate::image::{Image, StrMap};
use crate::pseudo;
use crate::types::*;

/// The wand settings (ImageInfo).
#[derive(Clone, Debug)]
pub struct Settings {
    pub format: String,
    pub filename: String,
    pub size: Option<(usize, usize)>,
    pub size_offset: i64,
    pub page: Option<Rect>,
    pub density: Option<(f64, f64)>,
    pub units: ResolutionType,
    pub compression: CompressionType,
    pub quality: u32,
    pub depth: u32,
    pub colorspace: Colorspace,
    pub image_type: ImageType,
    pub interlace: InterlaceType,
    pub orientation: OrientationType,
    pub background_color: PixelInfo,
    pub antialias: bool,
    pub font: String,
    pub pointsize: f64,
    pub gravity: Gravity,
    pub extract: String,
    pub sampling_factors: Vec<f64>,
    pub interpolate: PixelInterpolateMethod,
    pub options: StrMap,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            format: String::new(),
            filename: String::new(),
            size: None,
            size_offset: 0,
            page: None,
            density: None,
            units: ResolutionType::Undefined,
            compression: CompressionType::Undefined,
            quality: 0,
            depth: 16,
            colorspace: Colorspace::SRGB,
            image_type: ImageType::Undefined,
            interlace: InterlaceType::No,
            orientation: OrientationType::Undefined,
            background_color: PixelInfo::white(),
            antialias: true,
            font: String::new(),
            pointsize: 0.0,
            gravity: Gravity::Undefined,
            extract: String::new(),
            sampling_factors: Vec::new(),
            interpolate: PixelInterpolateMethod::Undefined,
            options: StrMap::default(),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Wand {
    pub images: Vec<Image>,
    pub current: usize,
    pub insert_before: bool,
    pub image_pending: bool,
    pub settings: Settings,
    pub exception: Option<MagickError>,
}

/// A split "FORMAT:filename[frames]".
#[derive(Debug, Default, PartialEq)]
pub struct FileSpec {
    pub magick: Option<String>,
    pub path: String,
    /// "[...]" suffix (scenes or a geometry)
    pub subimage: Option<String>,
}

/// Split the "FORMAT:" prefix and "[...]" suffix from a filename.
pub fn parse_filespec(name: &str) -> FileSpec {
    let mut spec = FileSpec {
        path: name.to_string(),
        ..Default::default()
    };
    if let Some(colon) = name.find(':') {
        let pre = &name[..colon];
        if pre.len() >= 2
            && pre
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
            && (codec::format_info(pre).is_some() || pseudo::is_pseudo(pre) || is_known_prefix(pre))
        {
            spec.magick = Some(pre.to_ascii_uppercase());
            spec.path = name[colon + 1..].to_string();
        }
    }
    if spec.path.ends_with(']') {
        if let Some(open) = spec.path.rfind('[') {
            let inner = spec.path[open + 1..spec.path.len() - 1].to_string();
            if !inner.is_empty()
                && inner
                    .chars()
                    .all(|c| c.is_ascii_digit() || ",-x+%!<>^@.".contains(c))
            {
                spec.subimage = Some(inner);
                spec.path.truncate(open);
            }
        }
    }
    spec
}

/// Format names that are prefixes only (no coder of their own here).
fn is_known_prefix(p: &str) -> bool {
    matches!(
        p.to_ascii_uppercase().as_str(),
        "INFO"
            | "MIFF"
            | "SVG"
            | "PDF"
            | "PS"
            | "EPS"
            | "HEIC"
            | "AVIF"
            | "JXL"
            | "PSD"
            | "DDS"
            | "PCX"
            | "XPM"
            | "XBM"
            | "SGI"
            | "HDR"
            | "EXR"
            | "JNG"
            | "MNG"
            | "APNG"
            | "LABEL"
            | "CAPTION"
            | "TEXT"
            | "MSL"
            | "MVG"
            | "FD"
            | "INLINE"
            | "EPHEMERAL"
            | "MPR"
            | "MPRI"
            | "CLIPBOARD"
            | "X"
            | "SCREENSHOT"
            | "VID"
            | "TILE"
            | "HISTOGRAM"
            | "THUMBNAIL"
    )
}

/// The scenes selected by a "[...]" spec ("0", "1-3", "0,2").
fn select_scenes(spec: &str, n: usize) -> Option<Vec<usize>> {
    if spec.contains('x') || spec.contains('%') || spec.contains('+') {
        return None;
    }
    let mut out = Vec::new();
    for part in spec.split(',') {
        if let Some((a, b)) = part.split_once('-') {
            let a: usize = a.trim().parse().ok()?;
            let b: usize = b.trim().parse().ok()?;
            let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
            for i in lo..=hi {
                if i < n {
                    out.push(i);
                }
            }
        } else {
            let i: usize = part.trim().parse().ok()?;
            if i < n {
                out.push(i);
            }
        }
    }
    Some(out)
}

pub fn extension_format(path: &str) -> Option<String> {
    let base = path.rsplit('/').next().unwrap_or(path);
    let ext = base.rsplit_once('.')?.1;
    if ext.is_empty() {
        return None;
    }
    Some(codec::canonical_format(ext))
}

impl Wand {
    pub fn new() -> Wand {
        Wand::default()
    }

    pub fn is_empty(&self) -> bool {
        self.images.is_empty()
    }

    pub fn len(&self) -> usize {
        self.images.len()
    }

    pub fn image(&self) -> Option<&Image> {
        self.images.get(self.current)
    }

    pub fn image_mut(&mut self) -> Option<&mut Image> {
        self.images.get_mut(self.current)
    }

    pub fn throw(&mut self, e: MagickError) {
        self.exception = Some(e);
    }

    pub fn take_exception(&mut self) -> Option<MagickError> {
        self.exception.take()
    }

    // --- iterator -------------------------------------------------------

    /// MagickGetIteratorIndex() (-1 when empty).
    pub fn iterator_index(&self) -> i64 {
        if self.images.is_empty() {
            -1
        } else {
            self.current as i64
        }
    }

    pub fn set_iterator_index(&mut self, index: i64) -> bool {
        let n = self.images.len() as i64;
        if n == 0 {
            return false;
        }
        let i = if index < 0 { n + index } else { index };
        if i < 0 || i >= n {
            return false;
        }
        self.current = i as usize;
        self.insert_before = false;
        self.image_pending = false;
        true
    }

    pub fn reset_iterator(&mut self) {
        self.current = 0;
        self.insert_before = false;
        self.image_pending = true;
    }

    pub fn set_first_iterator(&mut self) {
        self.current = 0;
        self.insert_before = true;
        self.image_pending = false;
    }

    pub fn set_last_iterator(&mut self) {
        self.current = self.images.len().saturating_sub(1);
        self.insert_before = false;
        self.image_pending = true;
    }

    pub fn next_image(&mut self) -> bool {
        if self.images.is_empty() {
            return false;
        }
        self.insert_before = false;
        if self.image_pending {
            self.image_pending = false;
            return true;
        }
        if self.current + 1 >= self.images.len() {
            self.image_pending = true;
            return false;
        }
        self.current += 1;
        true
    }

    pub fn previous_image(&mut self) -> bool {
        if self.images.is_empty() {
            return false;
        }
        if self.image_pending {
            self.image_pending = false;
            return true;
        }
        if self.current == 0 {
            self.image_pending = true;
            self.insert_before = true;
            return false;
        }
        self.current -= 1;
        true
    }

    pub fn has_next(&self) -> bool {
        !self.images.is_empty() && self.current + 1 < self.images.len()
    }

    pub fn has_previous(&self) -> bool {
        !self.images.is_empty() && self.current > 0
    }

    /// Insert images at the iterator (MagickAddImage / ReadImage).
    pub fn insert_images(&mut self, new: Vec<Image>) {
        if new.is_empty() {
            return;
        }
        let k = new.len();
        if self.images.is_empty() {
            self.images = new;
            self.current = k - 1;
        } else if self.insert_before {
            let at = self.current;
            self.images.splice(at..at, new);
            self.current = at + k - 1;
        } else {
            let at = self.current + 1;
            self.images.splice(at..at, new);
            self.current = at + k - 1;
        }
        self.image_pending = false;
    }

    /// MagickRemoveImage().
    pub fn remove_image(&mut self) -> bool {
        if self.images.is_empty() {
            return false;
        }
        self.images.remove(self.current);
        if self.current >= self.images.len() {
            self.current = self.images.len().saturating_sub(1);
        }
        true
    }

    pub fn clear(&mut self) {
        self.images.clear();
        self.current = 0;
        self.insert_before = false;
        self.image_pending = false;
    }

    // --- reading --------------------------------------------------------

    fn read_options(&self) -> ReadOptions {
        ReadOptions {
            size: self.settings.size,
            depth: if self.settings.depth != 16 {
                Some(self.settings.depth)
            } else {
                None
            },
            ping: false,
            density: self.settings.density,
            background: self.settings.background_color.clone(),
        }
    }

    /// Read images (a file, a "FORMAT:" pseudo image...) into a list
    /// without inserting them.
    pub fn read_images(&self, filename: &str, ping: bool) -> MResult<Vec<Image>> {
        let spec = parse_filespec(filename);
        if let Some(m) = &spec.magick {
            let up = m.to_ascii_uppercase();
            if up == "LABEL" || up == "CAPTION" {
                let o = crate::label::LabelOptions {
                    font: &self.settings.font,
                    pointsize: self.settings.pointsize,
                    size: self.settings.size,
                    gravity: self.settings.gravity,
                    background: &self.settings.background_color,
                    fill: self
                        .settings
                        .options
                        .get("fill")
                        .and_then(PixelInfo::parse)
                        .unwrap_or_else(PixelInfo::black),
                };
                let mut img = crate::label::render(&o, &spec.path, up == "CAPTION")?;
                self.apply_read_settings(&mut img);
                return Ok(vec![img]);
            }
            if pseudo::is_pseudo(m) {
                let mut img =
                    pseudo::read_with(m, &spec.path, self.settings.size, &self.settings.options)?;
                self.apply_read_settings(&mut img);
                return Ok(vec![img]);
            }
        }
        let path = if spec.path.is_empty() && spec.magick.is_some() {
            return Err(MagickError::with_desc(
                severity::BLOB_ERROR,
                "unable to open image",
                filename,
                "error/blob.c/OpenBlob/3710",
            ));
        } else {
            spec.path.clone()
        };
        let data = match std::fs::read(&path) {
            Ok(d) => d,
            Err(e) => {
                return Err(MagickError::new(
                    severity::BLOB_ERROR,
                    format!(
                        "unable to open image '{}': {} @ error/blob.c/OpenBlob/3710",
                        path,
                        io_reason(&e)
                    ),
                ));
            }
        };
        let magick = spec
            .magick
            .clone()
            .filter(|m| codec::format_info(m).is_some())
            .or_else(|| codec::sniff(&data).map(|s| s.to_string()))
            .or_else(|| extension_format(&path));
        let Some(magick) = magick else {
            return Err(MagickError::no_decode_delegate(
                &extension_format(&path).unwrap_or_default(),
            ));
        };
        if codec::format_info(&magick).is_none_or(|f| !f.read) {
            return Err(MagickError::with_desc(
                severity::MISSING_DELEGATE_ERROR,
                "no decode delegate for this image format",
                &magick,
                "error/constitute.c/ReadImage/741",
            ));
        }
        let mut images = codec::decode(&data, &magick, &self.read_options())?;
        if let Some(sub) = &spec.subimage {
            if let Some(sel) = select_scenes(sub, images.len()) {
                images = sel.into_iter().map(|i| images[i].clone()).collect();
            } else if let Some(g) = Geometry::parse(sub) {
                // a size: resize (or crop with an offset) as it is read
                for img in images.iter_mut() {
                    if g.x.is_some() {
                        let r = g.to_rect();
                        *img = crate::transform::crop(img, r).0;
                    } else {
                        let (w, h) = g.meta_size(img.columns, img.rows);
                        if let Ok(r) = crate::transform::resize(img, w, h, FilterType::Undefined) {
                            *img = r;
                        }
                    }
                }
            }
        }
        for img in images.iter_mut() {
            img.filename = filename.to_string();
            img.ping = ping;
            self.apply_read_settings(img);
            // -extract: a region (or a size) of what is read
            if let Some(g) = Geometry::parse(&self.settings.extract) {
                if g.x.is_some() || g.y.is_some() {
                    *img = crate::transform::crop(img, g.to_rect()).0;
                    img.page = Rect::new(img.columns, img.rows, 0, 0);
                } else if g.width.is_some() || g.height.is_some() {
                    let (w, h) = g.meta_size(img.columns, img.rows);
                    if let Ok(r) = crate::transform::resize(img, w, h, FilterType::Undefined) {
                        *img = r;
                    }
                }
            }
        }
        Ok(images)
    }

    /// Settings of the wand that images take when read.
    fn apply_read_settings(&self, img: &mut Image) {
        if let Some((x, y)) = self.settings.density {
            if img.x_resolution == 0.0 {
                img.x_resolution = x;
                img.y_resolution = y;
            }
        }
        if self.settings.units != ResolutionType::Undefined
            && img.units == ResolutionType::Undefined
        {
            img.units = self.settings.units;
        }
        if self.settings.quality > 0 && img.quality == 0 {
            img.quality = self.settings.quality;
        }
        if self.settings.compression != CompressionType::Undefined {
            img.compression = self.settings.compression;
        }
        if let Some(p) = self.settings.page {
            img.page = p;
        }
        for (k, v) in &self.settings.options.0 {
            if img.artifacts.get(k).is_none() {
                img.artifacts.set(k, v);
            }
        }
    }

    pub fn read_image(&mut self, filename: &str) -> MResult<()> {
        let images = self.read_images(filename, false)?;
        self.insert_images(images);
        Ok(())
    }

    /// MagickReadImageBlob(): the format from the wand's format setting,
    /// or sniffed.
    pub fn decode_blob(&self, data: &[u8], filename: Option<&str>) -> MResult<Vec<Image>> {
        let mut magick = None;
        if let Some(f) = filename {
            let spec = parse_filespec(f);
            magick = spec.magick.or_else(|| extension_format(&spec.path));
        }
        let magick = codec::sniff(data)
            .map(|s| s.to_string())
            .or(magick)
            .or_else(|| {
                if self.settings.format.is_empty() {
                    None
                } else {
                    Some(codec::canonical_format(&self.settings.format))
                }
            });
        let Some(magick) = magick else {
            return Err(MagickError::no_blob_delegate(""));
        };
        let mut images = codec::decode(data, &magick, &self.read_options())?;
        for img in images.iter_mut() {
            if let Some(f) = filename {
                img.filename = f.to_string();
            }
            self.apply_read_settings(img);
        }
        Ok(images)
    }

    // --- writing --------------------------------------------------------

    fn write_options(&self, img: &Image) -> WriteOptions {
        let mut defines: Vec<(String, String)> = self.settings.options.0.clone();
        for (k, v) in &img.artifacts.0 {
            defines.push((k.clone(), v.clone()));
        }
        WriteOptions { defines }
    }

    /// The format of an image (its "magick").
    pub fn image_format(&self, img: &Image) -> String {
        img.format.clone()
    }

    /// The format a blob of an image is made in: the wand's format
    /// setting (setFormat) when there is one, else the image's.
    pub fn blob_format(&self, img: &Image) -> String {
        if !self.settings.format.is_empty() {
            self.settings.format.clone()
        } else {
            img.format.clone()
        }
    }

    /// MagickGetImageBlob(): the current image in its format.
    pub fn image_blob(&self) -> MResult<Vec<u8>> {
        let img = self
            .image()
            .ok_or_else(|| MagickError::new(severity::WAND_ERROR, "ContainsNoImages"))?;
        let fmt = self.blob_format(img);
        let mut img = img.clone();
        self.apply_write_settings(&mut img);
        codec::encode(&[&img], &fmt, &self.write_options(&img))
    }

    /// MagickGetImagesBlob(): all images in the current image's format
    /// (one image only for single-image formats).
    pub fn images_blob(&self) -> MResult<Vec<u8>> {
        let img = self
            .image()
            .ok_or_else(|| MagickError::new(severity::WAND_ERROR, "ContainsNoImages"))?;
        let fmt = self.blob_format(img);
        let multi = codec::format_info(&fmt).is_some_and(|f| f.multi);
        let mut list: Vec<Image> = if multi {
            self.images.clone()
        } else {
            vec![self.images[0].clone()]
        };
        for i in list.iter_mut() {
            self.apply_write_settings(i);
        }
        let refs: Vec<&Image> = list.iter().collect();
        codec::encode(&refs, &fmt, &self.write_options(img))
    }

    fn apply_write_settings(&self, img: &mut Image) {
        if img.quality == 0 && self.settings.quality > 0 {
            img.quality = self.settings.quality;
        }
        if img.compression == CompressionType::Undefined {
            img.compression = self.settings.compression;
        }
        if self.settings.interlace != InterlaceType::No && img.interlace == InterlaceType::No {
            img.interlace = self.settings.interlace;
        }
    }

    /// MagickWriteImage() / MagickWriteImages().
    pub fn write(&mut self, filename: &str, all: bool, adjoin: bool) -> MResult<()> {
        let Some(img) = self.image() else {
            return Err(MagickError::new(severity::WAND_ERROR, "ContainsNoImages"));
        };
        let spec = parse_filespec(filename);
        let fmt = spec
            .magick
            .clone()
            .or_else(|| extension_format(&spec.path).filter(|f| codec::format_info(f).is_some()))
            .unwrap_or_else(|| self.image_format(img));
        let fmt = codec::canonical_format(&fmt);
        let Some(info) = codec::format_info(&fmt).filter(|f| f.write) else {
            return Err(MagickError::with_desc(
                severity::MISSING_DELEGATE_ERROR,
                "no encode delegate for this image format",
                &fmt,
                "error/constitute.c/WriteImage/1404",
            ));
        };
        let opts = self.write_options(img);
        let path = spec.path.clone();
        let mut outputs: Vec<(String, Vec<u8>)> = Vec::new();
        if all && self.images.len() > 1 {
            if info.multi && adjoin {
                let mut list = self.images.clone();
                for i in list.iter_mut() {
                    self.apply_write_settings(i);
                }
                let refs: Vec<&Image> = list.iter().collect();
                outputs.push((path.clone(), codec::encode(&refs, &fmt, &opts)?));
            } else {
                for (n, img) in self.images.iter().enumerate() {
                    let mut i2 = img.clone();
                    self.apply_write_settings(&mut i2);
                    let name = numbered_filename(&path, n);
                    outputs.push((name, codec::encode(&[&i2], &fmt, &opts)?));
                }
            }
        } else {
            let mut i2 = img.clone();
            self.apply_write_settings(&mut i2);
            outputs.push((path.clone(), codec::encode(&[&i2], &fmt, &opts)?));
        }
        for (name, data) in outputs {
            if let Err(e) = std::fs::write(&name, &data) {
                let _ = e;
                return Err(MagickError::with_desc(
                    severity::CODER_ERROR,
                    "WriteBlob Failed",
                    &name,
                    &format!(
                        "error/{}.c/Magick{}Error/1305",
                        fmt.to_ascii_lowercase(),
                        fmt
                    ),
                ));
            }
        }
        let fmt_set = fmt.clone();
        if let Some(img) = self.image_mut() {
            img.filename = filename.to_string();
            if img.format.is_empty() {
                img.format = fmt_set;
            }
        }
        Ok(())
    }

    /// MagickNewImage().
    pub fn new_image(&mut self, w: usize, h: usize, color: &PixelInfo) -> MResult<()> {
        let mut img = Image::new(w, h, color);
        img.background_color = PixelInfo::white();
        img.page = Rect::new(w, h, 0, 0);
        // NewImage() makes an 8-bit image, whatever the colour
        img.depth = 8;
        img.format = self.settings.format.clone();
        self.apply_read_settings(&mut img);
        self.insert_images(vec![img]);
        Ok(())
    }
}

/// "file.png" -> "file-0.png" for the images of a list written apart.
pub fn numbered_filename(path: &str, n: usize) -> String {
    if path.contains("%d") {
        return path.replacen("%d", &n.to_string(), 1);
    }
    match path.rsplit_once('.') {
        Some((base, ext)) if !base.is_empty() && !ext.contains('/') => {
            format!("{}-{}.{}", base, n, ext)
        }
        _ => format!("{}-{}", path, n),
    }
}

fn io_reason(e: &std::io::Error) -> String {
    match e.kind() {
        std::io::ErrorKind::NotFound => "No such file or directory".to_string(),
        std::io::ErrorKind::PermissionDenied => "Permission denied".to_string(),
        _ => {
            if e.raw_os_error() == Some(21) {
                "Is a directory".to_string()
            } else {
                let s = e.to_string();
                match s.find(" (os error") {
                    Some(p) => s[..p].to_string(),
                    None => s,
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filespec() {
        let s = parse_filespec("png:out.png");
        assert_eq!(s.magick.as_deref(), Some("PNG"));
        assert_eq!(s.path, "out.png");
        let s = parse_filespec("magick:logo");
        assert_eq!(s.magick.as_deref(), Some("MAGICK"));
        let s = parse_filespec("anim.gif[0]");
        assert_eq!(s.subimage.as_deref(), Some("0"));
        assert_eq!(s.path, "anim.gif");
        let s = parse_filespec("C:/x.png");
        assert_eq!(s.magick, None);
    }

    #[test]
    fn iterator() {
        let mut w = Wand::new();
        for i in 0..3 {
            let mut img = Image::blank(1, 1);
            img.scene = i;
            w.insert_images(vec![img]);
            w.set_last_iterator();
        }
        w.reset_iterator();
        let mut seq = Vec::new();
        for _ in 0..5 {
            let r = w.next_image();
            seq.push((r, w.current));
        }
        assert_eq!(
            seq,
            vec![(true, 0), (true, 1), (true, 2), (false, 2), (true, 2)]
        );
    }
}
