//! One image (a frame of an image list): Q16 pixels and the image's
//! attributes, properties, artifacts and profiles.

use crate::color::{PixelInfo, clamp_to_quantum};
use crate::geometry::Rect;
use crate::types::*;
use crate::{QUANTUM_RANGE, QUANTUM_SCALE};

/// An ordered, case-insensitive string map (properties, artifacts, options).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StrMap(pub Vec<(String, String)>);

impl StrMap {
    pub fn get(&self, k: &str) -> Option<&str> {
        self.0
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(k))
            .map(|(_, v)| v.as_str())
    }
    pub fn set(&mut self, k: &str, v: &str) {
        if let Some(e) = self.0.iter_mut().find(|(n, _)| n.eq_ignore_ascii_case(k)) {
            e.1 = v.to_string();
        } else {
            self.0.push((k.to_string(), v.to_string()));
        }
    }
    pub fn remove(&mut self, k: &str) -> bool {
        let n = self.0.len();
        self.0.retain(|(n, _)| !n.eq_ignore_ascii_case(k));
        n != self.0.len()
    }
    /// Keys matching a glob pattern (`*`, `?`), sorted as ImageMagick's
    /// splay tree iterates them.
    pub fn matching(&self, pattern: &str) -> Vec<(String, String)> {
        let mut v: Vec<(String, String)> = self
            .0
            .iter()
            .filter(|(k, _)| glob_match(pattern, k))
            .cloned()
            .collect();
        v.sort_by_key(|a| a.0.to_ascii_lowercase());
        v
    }
}

/// GlobExpression() (case-insensitive `*` / `?` / `[...]`).
pub fn glob_match(pattern: &str, s: &str) -> bool {
    fn rec(p: &[u8], s: &[u8]) -> bool {
        if p.is_empty() {
            return s.is_empty();
        }
        match p[0] {
            b'*' => (0..=s.len()).any(|i| rec(&p[1..], &s[i..])),
            b'?' => !s.is_empty() && rec(&p[1..], &s[1..]),
            b'[' => {
                if s.is_empty() {
                    return false;
                }
                if let Some(end) = p.iter().position(|&c| c == b']') {
                    let set = &p[1..end];
                    let c = s[0].to_ascii_lowercase();
                    let mut ok = false;
                    let mut i = 0;
                    while i < set.len() {
                        if i + 2 < set.len() && set[i + 1] == b'-' {
                            if (set[i].to_ascii_lowercase()..=set[i + 2].to_ascii_lowercase())
                                .contains(&c)
                            {
                                ok = true;
                            }
                            i += 3;
                        } else {
                            if set[i].to_ascii_lowercase() == c {
                                ok = true;
                            }
                            i += 1;
                        }
                    }
                    ok && rec(&p[end + 1..], &s[1..])
                } else {
                    p[0] == s[0] && rec(&p[1..], &s[1..])
                }
            }
            c => !s.is_empty() && c.eq_ignore_ascii_case(&s[0]) && rec(&p[1..], &s[1..]),
        }
    }
    rec(pattern.as_bytes(), s.as_bytes())
}

/// An image of the list.
#[derive(Clone, Debug)]
pub struct Image {
    pub columns: usize,
    pub rows: usize,
    /// RGBA quadruples, row-major (alpha is opaque when `alpha` is off).
    pub pixels: Vec<u16>,
    /// The black channel of CMYK images.
    pub black: Option<Vec<u16>>,
    /// Whether the alpha channel is active (alpha_trait).
    pub alpha: bool,
    pub colorspace: Colorspace,
    /// The type requested by setImageType (Undefined: computed).
    pub image_type: ImageType,
    pub depth: u32,
    pub page: Rect,
    /// The image format (its "magick"), e.g. "PNG"; empty for none.
    pub format: String,
    pub filename: String,
    /// The size of the file the image came from.
    pub file_size: u64,
    pub compression: CompressionType,
    pub quality: u32,
    pub interlace: InterlaceType,
    pub x_resolution: f64,
    pub y_resolution: f64,
    pub units: ResolutionType,
    pub delay: u64,
    pub ticks_per_second: i64,
    pub iterations: u64,
    pub dispose: DisposeType,
    pub gravity: Gravity,
    pub compose: CompositeOp,
    pub virtual_pixel: VirtualPixelMethod,
    pub interpolate: PixelInterpolateMethod,
    pub orientation: OrientationType,
    pub background_color: PixelInfo,
    pub border_color: PixelInfo,
    pub matte_color: PixelInfo,
    pub fuzz: f64,
    pub gamma: f64,
    pub rendering_intent: i32,
    pub chromaticity: Chromaticity,
    pub scene: usize,
    pub properties: StrMap,
    pub artifacts: StrMap,
    /// Profiles by lower-case name ("icc", "exif", "xmp", "iptc"...).
    pub profiles: Vec<(String, Vec<u8>)>,
    /// Whether the image was "pinged" (attributes only, no pixels decoded).
    pub ping: bool,
    /// Palette images keep their colour count (from the file).
    pub colors: Option<usize>,
    /// The size of the image as read (magick_columns/rows).
    pub magick_columns: usize,
    pub magick_rows: usize,
    /// The read/write mask (a grayscale image) when set.
    pub read_mask: Option<Box<Image>>,
    pub write_mask: Option<Box<Image>>,
    /// The channel mask (SetImageChannelMask).
    pub channel_mask: i64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Chromaticity {
    pub red: (f64, f64, f64),
    pub green: (f64, f64, f64),
    pub blue: (f64, f64, f64),
    pub white: (f64, f64, f64),
}

impl Chromaticity {
    /// The sRGB / BT.709 primaries and D65 white point that images get.
    pub fn srgb() -> Chromaticity {
        Chromaticity {
            red: (0.64, 0.33, 0.03),
            green: (0.3, 0.6, 0.1),
            blue: (0.15, 0.06, 0.79),
            white: (0.3127, 0.329, 0.3583),
        }
    }
}

impl Image {
    /// AcquireImage() + SetImageBackgroundColor(): a canvas of a colour.
    pub fn new(columns: usize, rows: usize, color: &PixelInfo) -> Image {
        let mut img = Image::blank(columns, rows);
        img.fill(color);
        img.background_color = color.clone();
        if color.alpha_trait {
            img.alpha = true;
        }
        if color.colorspace == Colorspace::Gray {
            // a gray(...) or grayNN colour makes a gray canvas, though its
            // type stays the canvas's true colour
            img.colorspace = Colorspace::Gray;
            img.image_type = if img.alpha {
                ImageType::TrueColorAlpha
            } else {
                ImageType::TrueColor
            };
        }
        if color.colorspace == Colorspace::CMYK {
            img.colorspace = Colorspace::CMYK;
            let k = crate::color::to_q16(color.black);
            img.black = Some(vec![k; columns * rows]);
            let p = [
                crate::color::to_q16(color.red),
                crate::color::to_q16(color.green),
                crate::color::to_q16(color.blue),
                crate::color::to_q16(color.alpha),
            ];
            for px in img.pixels.chunks_exact_mut(4) {
                px.copy_from_slice(&p);
            }
        }
        img
    }

    /// An image with zeroed (opaque black) pixels and default attributes.
    pub fn blank(columns: usize, rows: usize) -> Image {
        let mut pixels = vec![0u16; columns * rows * 4];
        for px in pixels.chunks_exact_mut(4) {
            px[3] = 65535;
        }
        Image {
            columns,
            rows,
            pixels,
            black: None,
            alpha: false,
            colorspace: Colorspace::SRGB,
            image_type: ImageType::Undefined,
            depth: 16,
            page: Rect::default(),
            format: String::new(),
            filename: String::new(),
            file_size: 0,
            compression: CompressionType::Undefined,
            quality: 0,
            interlace: InterlaceType::No,
            x_resolution: 0.0,
            y_resolution: 0.0,
            units: ResolutionType::Undefined,
            delay: 0,
            ticks_per_second: 100,
            iterations: 0,
            dispose: DisposeType::Undefined,
            gravity: Gravity::Undefined,
            compose: CompositeOp::Over,
            virtual_pixel: VirtualPixelMethod::Undefined,
            interpolate: PixelInterpolateMethod::Undefined,
            orientation: OrientationType::Undefined,
            background_color: PixelInfo::from_rgba8([255, 255, 255, 255]),
            border_color: PixelInfo::from_rgba8([223, 223, 223, 255]),
            matte_color: PixelInfo::from_rgba8([189, 189, 189, 255]),
            fuzz: 0.0,
            gamma: 1.0 / 2.2,
            rendering_intent: 2,
            chromaticity: Chromaticity::srgb(),
            scene: 0,
            properties: StrMap::default(),
            artifacts: StrMap::default(),
            profiles: Vec::new(),
            ping: false,
            colors: None,
            magick_columns: columns,
            magick_rows: rows,
            read_mask: None,
            write_mask: None,
            channel_mask: crate::types::channel::ALL,
        }
    }

    /// An image of another size with the same attributes (CloneImage(w, h)).
    pub fn clone_attributes(&self, columns: usize, rows: usize) -> Image {
        let mut img = Image::blank(columns, rows);
        img.copy_attributes_from(self);
        img
    }

    pub fn copy_attributes_from(&mut self, o: &Image) {
        let (c, r, p, b) = (
            self.columns,
            self.rows,
            std::mem::take(&mut self.pixels),
            self.black.take(),
        );
        *self = Image {
            columns: c,
            rows: r,
            pixels: p,
            black: b,
            read_mask: None,
            write_mask: None,
            ..o.clone_header()
        };
        if self.colorspace == Colorspace::CMYK && self.black.is_none() {
            self.black = Some(vec![0; c * r]);
        }
    }

    /// A clone without pixel data (cheap).
    pub fn clone_header(&self) -> Image {
        Image {
            columns: self.columns,
            rows: self.rows,
            pixels: Vec::new(),
            black: None,
            alpha: self.alpha,
            colorspace: self.colorspace,
            image_type: self.image_type,
            depth: self.depth,
            page: self.page,
            format: self.format.clone(),
            filename: self.filename.clone(),
            file_size: self.file_size,
            compression: self.compression,
            quality: self.quality,
            interlace: self.interlace,
            x_resolution: self.x_resolution,
            y_resolution: self.y_resolution,
            units: self.units,
            delay: self.delay,
            ticks_per_second: self.ticks_per_second,
            iterations: self.iterations,
            dispose: self.dispose,
            gravity: self.gravity,
            compose: self.compose,
            virtual_pixel: self.virtual_pixel,
            interpolate: self.interpolate,
            orientation: self.orientation,
            background_color: self.background_color.clone(),
            border_color: self.border_color.clone(),
            matte_color: self.matte_color.clone(),
            fuzz: self.fuzz,
            gamma: self.gamma,
            rendering_intent: self.rendering_intent,
            chromaticity: self.chromaticity.clone(),
            scene: self.scene,
            properties: self.properties.clone(),
            artifacts: self.artifacts.clone(),
            profiles: self.profiles.clone(),
            ping: self.ping,
            colors: self.colors,
            magick_columns: self.magick_columns,
            magick_rows: self.magick_rows,
            read_mask: None,
            write_mask: None,
            channel_mask: self.channel_mask,
        }
    }

    pub fn fill(&mut self, color: &PixelInfo) {
        let p = color.to_rgba16();
        for px in self.pixels.chunks_exact_mut(4) {
            px.copy_from_slice(&p);
        }
        if let Some(k) = &mut self.black {
            k.fill(crate::color::to_q16(color.black));
        }
    }

    #[inline]
    pub fn idx(&self, x: usize, y: usize) -> usize {
        (y * self.columns + x) * 4
    }

    #[inline]
    pub fn px(&self, x: usize, y: usize) -> [u16; 4] {
        let i = self.idx(x, y);
        [
            self.pixels[i],
            self.pixels[i + 1],
            self.pixels[i + 2],
            self.pixels[i + 3],
        ]
    }

    #[inline]
    pub fn set_px(&mut self, x: usize, y: usize, p: [u16; 4]) {
        let i = self.idx(x, y);
        self.pixels[i..i + 4].copy_from_slice(&p);
    }

    /// The pixel at (x, y) as a PixelInfo (alpha_trait from the image).
    pub fn pixel_info(&self, x: usize, y: usize) -> PixelInfo {
        let p = self.px(x, y);
        let mut c = PixelInfo {
            colorspace: self.colorspace,
            alpha_trait: self.alpha && p[3] != 65535,
            red: p[0] as f64,
            green: p[1] as f64,
            blue: p[2] as f64,
            alpha: if self.alpha {
                p[3] as f64
            } else {
                QUANTUM_RANGE
            },
            ..Default::default()
        };
        if let Some(k) = &self.black {
            c.black = k[y * self.columns + x] as f64;
        }
        if c.colorspace != Colorspace::CMYK && c.colorspace != Colorspace::Gray {
            c.colorspace = Colorspace::SRGB;
        }
        c
    }

    /// The pixel at (x, y), with the virtual pixel method outside the image.
    pub fn virtual_px(&self, x: i64, y: i64) -> [u16; 4] {
        self.virtual_px_with(x, y, self.virtual_pixel)
    }

    /// The pixel at (x, y), with the virtual pixel method `vp` outside.
    pub fn virtual_px_with(&self, x: i64, y: i64, vp: VirtualPixelMethod) -> [u16; 4] {
        let (w, h) = (self.columns as i64, self.rows as i64);
        if x >= 0 && y >= 0 && x < w && y < h {
            return self.px(x as usize, y as usize);
        }
        if w == 0 || h == 0 {
            return [0, 0, 0, 0];
        }
        let edge = |v: i64, n: i64| v.clamp(0, n - 1);
        let tile = |v: i64, n: i64| v.rem_euclid(n);
        let mirror = |v: i64, n: i64| {
            let p = v.rem_euclid(2 * n);
            if p < n { p } else { 2 * n - 1 - p }
        };
        match vp {
            VirtualPixelMethod::Edge | VirtualPixelMethod::Undefined => {
                self.px(edge(x, w) as usize, edge(y, h) as usize)
            }
            VirtualPixelMethod::Tile => self.px(tile(x, w) as usize, tile(y, h) as usize),
            VirtualPixelMethod::Mirror => self.px(mirror(x, w) as usize, mirror(y, h) as usize),
            VirtualPixelMethod::Transparent => [0, 0, 0, 0],
            VirtualPixelMethod::Black => [0, 0, 0, 65535],
            VirtualPixelMethod::White => [65535, 65535, 65535, 65535],
            VirtualPixelMethod::Gray => [32768, 32768, 32768, 65535],
            VirtualPixelMethod::HorizontalTile => {
                if y < 0 || y >= h {
                    self.background_color.to_rgba16()
                } else {
                    self.px(tile(x, w) as usize, y as usize)
                }
            }
            VirtualPixelMethod::VerticalTile => {
                if x < 0 || x >= w {
                    self.background_color.to_rgba16()
                } else {
                    self.px(x as usize, tile(y, h) as usize)
                }
            }
            VirtualPixelMethod::HorizontalTileEdge => {
                self.px(tile(x, w) as usize, edge(y, h) as usize)
            }
            VirtualPixelMethod::VerticalTileEdge => {
                self.px(edge(x, w) as usize, tile(y, h) as usize)
            }
            VirtualPixelMethod::CheckerTile => {
                if ((x.div_euclid(w) + y.div_euclid(h)) & 1) != 0 {
                    [0, 0, 0, 0]
                } else {
                    self.px(tile(x, w) as usize, tile(y, h) as usize)
                }
            }
            _ => self.background_color.to_rgba16(),
        }
    }

    /// Bilinear sample at a real position (pixel centres at +0.5).
    pub fn sample_bilinear(&self, fx: f64, fy: f64) -> [f64; 4] {
        let x = fx - 0.5;
        let y = fy - 0.5;
        let x0 = x.floor();
        let y0 = y.floor();
        let ax = x - x0;
        let ay = y - y0;
        let (x0, y0) = (x0 as i64, y0 as i64);
        let p00 = self.virtual_px(x0, y0);
        let p10 = self.virtual_px(x0 + 1, y0);
        let p01 = self.virtual_px(x0, y0 + 1);
        let p11 = self.virtual_px(x0 + 1, y0 + 1);
        let w = [
            (1.0 - ax) * (1.0 - ay),
            ax * (1.0 - ay),
            (1.0 - ax) * ay,
            ax * ay,
        ];
        let ps = [p00, p10, p01, p11];
        // alpha-weighted colour
        let mut out = [0f64; 4];
        let mut asum = 0.0;
        for (p, &wt) in ps.iter().zip(w.iter()) {
            let a = p[3] as f64 * QUANTUM_SCALE;
            asum += wt * a;
            for c in 0..3 {
                out[c] += wt * a * p[c] as f64;
            }
            out[3] += wt * p[3] as f64;
        }
        if asum > 0.0 {
            for v in out.iter_mut().take(3) {
                *v /= asum;
            }
        }
        out
    }

    /// Whether every pixel is fully opaque.
    pub fn is_opaque(&self) -> bool {
        !self.alpha || self.pixels.chunks_exact(4).all(|p| p[3] == 65535)
    }

    /// Whether every pixel is gray (R = G = B).
    pub fn is_gray(&self) -> bool {
        self.colorspace == Colorspace::Gray
            || self
                .pixels
                .chunks_exact(4)
                .all(|p| p[0] == p[1] && p[1] == p[2])
    }

    /// The number of distinct colours (GetNumberColors()).
    pub fn number_colors(&self) -> usize {
        let mut v: Vec<u64> = self
            .pixels
            .chunks_exact(4)
            .map(|p| {
                let a = if self.alpha { p[3] as u64 } else { 65535 };
                ((p[0] as u64) << 48) | ((p[1] as u64) << 32) | ((p[2] as u64) << 16) | a
            })
            .collect();
        v.sort_unstable();
        v.dedup();
        v.len()
    }

    /// IdentifyImageType() / GetImageType().
    pub fn get_type(&self) -> ImageType {
        if self.image_type != ImageType::Undefined {
            return self.image_type;
        }
        self.identify_type()
    }

    /// IdentifyImageType(): the type from the pixels.
    pub fn identify_type(&self) -> ImageType {
        let opaque = self.is_opaque();
        if self.colorspace == Colorspace::CMYK {
            return if opaque {
                ImageType::ColorSeparation
            } else {
                ImageType::ColorSeparationAlpha
            };
        }
        if self.is_gray() {
            let bilevel = self
                .pixels
                .chunks_exact(4)
                .all(|p| p[0] == 0 || p[0] == 65535);
            if bilevel && opaque {
                return ImageType::Bilevel;
            }
            return if opaque {
                ImageType::Grayscale
            } else {
                ImageType::GrayscaleAlpha
            };
        }
        if self.colors.is_some() || self.number_colors() <= 256 {
            return if opaque {
                ImageType::Palette
            } else {
                ImageType::PaletteAlpha
            };
        }
        if opaque {
            ImageType::TrueColor
        } else {
            ImageType::TrueColorAlpha
        }
    }

    pub fn profile(&self, name: &str) -> Option<&[u8]> {
        self.profiles
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_slice())
    }

    pub fn set_profile(&mut self, name: &str, data: Vec<u8>) {
        let name = name.to_ascii_lowercase();
        if let Some(e) = self.profiles.iter_mut().find(|(n, _)| *n == name) {
            e.1 = data;
        } else {
            self.profiles.push((name, data));
        }
    }

    pub fn remove_profile(&mut self, name: &str) -> Option<Vec<u8>> {
        let i = self
            .profiles
            .iter()
            .position(|(n, _)| n.eq_ignore_ascii_case(name))?;
        Some(self.profiles.remove(i).1)
    }

    /// StripImage(): profiles, comments and the like go.
    pub fn strip(&mut self) {
        // StripImage(): the profiles, the comment and the file dates
        self.profiles.clear();
        self.properties.remove("comment");
        self.properties
            .0
            .retain(|(k, _)| !k.to_ascii_lowercase().starts_with("date:"));
        // what the PNG writer then leaves out
        self.artifacts.set(
            "png:exclude-chunk",
            "bKGD,caNv,cHRM,eXIf,gAMA,iCCP,iTXt,pHYs,sRGB,tEXt,zCCP,zTXt,date",
        );
    }

    /// SetImageAlphaChannel(Set/Activate...): ensure an alpha channel.
    pub fn enable_alpha(&mut self) {
        if !self.alpha {
            self.alpha = true;
            for p in self.pixels.chunks_exact_mut(4) {
                p[3] = 65535;
            }
        }
    }

    /// Deactivate the alpha channel (pixels become opaque).
    pub fn disable_alpha(&mut self) {
        self.alpha = false;
        for p in self.pixels.chunks_exact_mut(4) {
            p[3] = 65535;
        }
    }

    /// A crop that leaves the attributes (page...) to the caller.
    pub fn crop_raw(&self, r: Rect) -> Image {
        let mut out = self.clone_attributes(r.width, r.height);
        for y in 0..r.height {
            let sy = r.y + y as i64;
            for x in 0..r.width {
                let sx = r.x + x as i64;
                let p = if sx >= 0
                    && sy >= 0
                    && (sx as usize) < self.columns
                    && (sy as usize) < self.rows
                {
                    self.px(sx as usize, sy as usize)
                } else {
                    self.background_color.to_rgba16()
                };
                out.set_px(x, y, p);
            }
        }
        if let (Some(k), Some(ok)) = (&self.black, &mut out.black) {
            for y in 0..r.height {
                for x in 0..r.width {
                    let sx = r.x + x as i64;
                    let sy = r.y + y as i64;
                    if sx >= 0
                        && sy >= 0
                        && (sx as usize) < self.columns
                        && (sy as usize) < self.rows
                    {
                        ok[y * r.width + x] = k[sy as usize * self.columns + sx as usize];
                    }
                }
            }
        }
        out
    }

    /// The pixels as 8-bit RGBA (ScaleQuantumToChar).
    pub fn to_rgba8(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.pixels.len());
        let cmyk = self.colorspace == Colorspace::CMYK;
        for (i, p) in self.pixels.chunks_exact(4).enumerate() {
            let (r, g, b) = if cmyk {
                let k = self.black.as_ref().map_or(0, |k| k[i]) as f64 * QUANTUM_SCALE;
                let f = |c: u16| {
                    clamp_to_quantum(
                        QUANTUM_RANGE * (1.0 - (c as f64 * QUANTUM_SCALE * (1.0 - k) + k)),
                    ) as u16
                };
                (f(p[0]), f(p[1]), f(p[2]))
            } else {
                (p[0], p[1], p[2])
            };
            out.push(q_to_u8(r));
            out.push(q_to_u8(g));
            out.push(q_to_u8(b));
            out.push(if self.alpha { q_to_u8(p[3]) } else { 255 });
        }
        out
    }

    /// Replace the pixels from 8-bit RGBA (alpha honoured if the image has it).
    pub fn set_from_rgba8(&mut self, rgba: &[u8]) {
        for (d, s) in self.pixels.chunks_exact_mut(4).zip(rgba.chunks_exact(4)) {
            d[0] = s[0] as u16 * 257;
            d[1] = s[1] as u16 * 257;
            d[2] = s[2] as u16 * 257;
            d[3] = if self.alpha { s[3] as u16 * 257 } else { 65535 };
        }
    }

    /// Convert CMYK pixels to sRGB in place.
    pub fn cmyk_to_rgb(&mut self) {
        if self.colorspace != Colorspace::CMYK {
            return;
        }
        let k = self
            .black
            .take()
            .unwrap_or_else(|| vec![0; self.columns * self.rows]);
        for (p, &kk) in self.pixels.chunks_exact_mut(4).zip(k.iter()) {
            let k = kk as f64 * QUANTUM_SCALE;
            for c in p.iter_mut().take(3) {
                *c = clamp_to_quantum(
                    QUANTUM_RANGE * (1.0 - (*c as f64 * QUANTUM_SCALE * (1.0 - k) + k)),
                ) as u16;
            }
        }
        self.colorspace = Colorspace::SRGB;
    }

    /// Convert sRGB pixels to CMYK in place.
    pub fn rgb_to_cmyk(&mut self) {
        if self.colorspace == Colorspace::CMYK {
            return;
        }
        let mut ks = vec![0u16; self.columns * self.rows];
        for (p, kk) in self.pixels.chunks_exact_mut(4).zip(ks.iter_mut()) {
            let c = 1.0 - p[0] as f64 * QUANTUM_SCALE;
            let m = 1.0 - p[1] as f64 * QUANTUM_SCALE;
            let y = 1.0 - p[2] as f64 * QUANTUM_SCALE;
            let k = c.min(m).min(y);
            let f = |v: f64| {
                if k >= 1.0 { 0.0 } else { (v - k) / (1.0 - k) }
            };
            p[0] = clamp_to_quantum(f(c) * QUANTUM_RANGE) as u16;
            p[1] = clamp_to_quantum(f(m) * QUANTUM_RANGE) as u16;
            p[2] = clamp_to_quantum(f(y) * QUANTUM_RANGE) as u16;
            *kk = clamp_to_quantum(k * QUANTUM_RANGE) as u16;
        }
        self.black = Some(ks);
        self.colorspace = Colorspace::CMYK;
    }

    /// Luma (Rec. 709 luma of the sRGB-encoded values, as IM's default
    /// intensity) of every pixel, in quantum scale.
    pub fn intensity(p: &[u16]) -> f64 {
        0.212656 * p[0] as f64 + 0.715158 * p[1] as f64 + 0.072186 * p[2] as f64
    }

    pub fn property(&self, k: &str) -> Option<&str> {
        self.properties.get(k)
    }
}

/// ScaleQuantumToChar().
#[inline]
pub fn q_to_u8(v: u16) -> u8 {
    ((v as u32 + 128) / 257) as u8
}
