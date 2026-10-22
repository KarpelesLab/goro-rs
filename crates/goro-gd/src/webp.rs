//! gd_webp.c on top of the pure-Rust OxideAV WebP codecs: oxideav-webp
//! (container, VP8L lossless decode/encode, VP8 lossy decode) and
//! oxideav-vp8 (VP8 lossy keyframe encoder).
//!
//! The pixel conversions are libgd's (7-bit alpha, `gdAlphaMax - (a >> 1)`
//! on input, `255 - ((a << 1) + (a >> 6))` on output with 127 mapped to a
//! fully transparent 0); the WebP bitstreams themselves are not those of
//! libwebp.

use crate::*;

/// gdWebpLossless: qualities from here on select the lossless encoder
pub const WEBP_LOSSLESS: i32 = 101;

fn le24(b: &[u8]) -> u32 {
    b[0] as u32 | (b[1] as u32) << 8 | (b[2] as u32) << 16
}

/// The width and height of a raw `VP8 ` keyframe bitstream.
fn vp8_dims(b: &[u8]) -> Option<(u32, u32)> {
    if b.len() < 10 {
        return None;
    }
    let tag = le24(b);
    // key frame, show_frame, partition length within the data
    if tag & 1 != 0 || (tag >> 4) & 1 == 0 || (tag >> 5) as usize >= b.len() {
        return None;
    }
    if b[3..6] != [0x9d, 0x01, 0x2a] {
        return None;
    }
    let w = (u16::from_le_bytes([b[6], b[7]]) & 0x3fff) as u32;
    let h = (u16::from_le_bytes([b[8], b[9]]) & 0x3fff) as u32;
    if w == 0 || h == 0 {
        return None;
    }
    Some((w, h))
}

/// The width and height of a raw `VP8L` bitstream.
fn vp8l_dims(b: &[u8]) -> Option<(u32, u32)> {
    if b.len() < 5 || b[0] != 0x2f {
        return None;
    }
    let bits = u32::from_le_bytes([b[1], b[2], b[3], b[4]]);
    if bits >> 29 != 0 {
        // version must be 0
        return None;
    }
    Some(((bits & 0x3fff) + 1, ((bits >> 14) & 0x3fff) + 1))
}

/// WebPGetInfo(): the dimensions of a RIFF/WEBP file or of a bare
/// VP8 / VP8L bitstream.
pub fn get_info(data: &[u8]) -> Option<(u32, u32)> {
    if data.len() >= 12 && &data[0..4] == b"RIFF" && &data[8..12] == b"WEBP" {
        let riff_size = u32::from_le_bytes([data[4], data[5], data[6], data[7]]) as usize;
        if riff_size < 12 {
            return None;
        }
        let end = data.len().min(riff_size + 8);
        let mut pos = 12;
        while pos + 8 <= end {
            let fourcc = &data[pos..pos + 4];
            let len =
                u32::from_le_bytes([data[pos + 4], data[pos + 5], data[pos + 6], data[pos + 7]])
                    as usize;
            let payload = &data[pos + 8..end.min(pos + 8 + len)];
            match fourcc {
                b"VP8X" => {
                    if payload.len() < 10 {
                        return None;
                    }
                    let w = le24(&payload[4..7]) + 1;
                    let h = le24(&payload[7..10]) + 1;
                    return Some((w, h));
                }
                b"VP8 " => return vp8_dims(payload),
                b"VP8L" => return vp8l_dims(payload),
                _ => {}
            }
            pos += 8 + len + (len & 1);
        }
        return None;
    }
    vp8_dims(data).or_else(|| vp8l_dims(data))
}

/// A bare VP8 / VP8L bitstream wrapped in a simple RIFF/WEBP container.
fn wrap_bare(data: &[u8]) -> Vec<u8> {
    let fourcc: &[u8; 4] = if vp8l_dims(data).is_some() && vp8_dims(data).is_none() {
        b"VP8L"
    } else {
        b"VP8 "
    };
    riff(&[(fourcc, data)])
}

/// A RIFF/WEBP file of the given chunks (odd chunks padded to even size).
fn riff(chunks: &[(&[u8; 4], &[u8])]) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(b"WEBP");
    for (fourcc, payload) in chunks {
        body.extend_from_slice(*fourcc);
        body.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        body.extend_from_slice(payload);
        if payload.len() & 1 != 0 {
            body.push(0);
        }
    }
    let mut out = Vec::with_capacity(body.len() + 8);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend_from_slice(&body);
    out
}

/// The payload of the first chunk `fourcc` of a RIFF/WEBP file.
fn find_chunk<'a>(data: &'a [u8], fourcc: &[u8; 4]) -> Option<&'a [u8]> {
    let mut pos = 12;
    while pos + 8 <= data.len() {
        let len = u32::from_le_bytes([data[pos + 4], data[pos + 5], data[pos + 6], data[pos + 7]])
            as usize;
        if &data[pos..pos + 4] == fourcc {
            return Some(&data[pos + 8..data.len().min(pos + 8 + len)]);
        }
        pos += 8 + len + (len & 1);
    }
    None
}

/// libwebp's VP8YuvToRgb() (dsp/yuv.h): BT.601 studio range, 14-bit
/// fixed point.
fn yuv_to_rgb(y: i32, u: i32, v: i32, out: &mut [u8]) {
    let mult_hi = |v: i32, c: i32| (v * c) >> 8;
    let clip8 = |v: i32| -> u8 {
        if v & !16383 == 0 {
            (v >> 6) as u8
        } else if v < 0 {
            0
        } else {
            255
        }
    };
    out[0] = clip8(mult_hi(y, 19077) + mult_hi(v, 26149) - 14234);
    out[1] = clip8(mult_hi(y, 19077) - mult_hi(u, 6419) - mult_hi(v, 13320) + 8708);
    out[2] = clip8(mult_hi(y, 19077) + mult_hi(u, 33050) - 17685);
}

/// libwebp's fancy upsampler (dsp/upsampling.c UPSAMPLE_FUNC) for one
/// pair of output rows (`bottom` optional).
#[allow(clippy::too_many_arguments)]
fn upsample_pair(
    top_y: &[u8],
    bottom_y: Option<&[u8]>,
    top_u: &[u8],
    top_v: &[u8],
    cur_u: &[u8],
    cur_v: &[u8],
    top_dst: &mut [u8],
    mut bottom_dst: Option<&mut [u8]>,
    len: usize,
) {
    let load = |u: u8, v: u8| (u as u32) | ((v as u32) << 16);
    let put = |y: u8, uv: u32, dst: &mut [u8]| {
        yuv_to_rgb(y as i32, (uv & 0xff) as i32, (uv >> 16) as i32, dst)
    };
    let last_pair = (len - 1) >> 1;
    let mut tl_uv = load(top_u[0], top_v[0]);
    let mut l_uv = load(cur_u[0], cur_v[0]);
    put(
        top_y[0],
        (3 * tl_uv + l_uv + 0x0002_0002) >> 2,
        &mut top_dst[0..],
    );
    if let (Some(by), Some(bd)) = (bottom_y, &mut bottom_dst) {
        put(by[0], (3 * l_uv + tl_uv + 0x0002_0002) >> 2, &mut bd[0..]);
    }
    for x in 1..=last_pair {
        let t_uv = load(top_u[x], top_v[x]);
        let uv = load(cur_u[x], cur_v[x]);
        let avg = tl_uv + t_uv + l_uv + uv + 0x0008_0008;
        let diag_12 = (avg + 2 * (t_uv + l_uv)) >> 3;
        let diag_03 = (avg + 2 * (tl_uv + uv)) >> 3;
        put(
            top_y[2 * x - 1],
            (diag_12 + tl_uv) >> 1,
            &mut top_dst[(2 * x - 1) * 4..],
        );
        put(
            top_y[2 * x],
            (diag_03 + t_uv) >> 1,
            &mut top_dst[2 * x * 4..],
        );
        if let (Some(by), Some(bd)) = (bottom_y, &mut bottom_dst) {
            put(
                by[2 * x - 1],
                (diag_03 + l_uv) >> 1,
                &mut bd[(2 * x - 1) * 4..],
            );
            put(by[2 * x], (diag_12 + uv) >> 1, &mut bd[2 * x * 4..]);
        }
        tl_uv = t_uv;
        l_uv = uv;
    }
    if len & 1 == 0 {
        put(
            top_y[len - 1],
            (3 * tl_uv + l_uv + 0x0002_0002) >> 2,
            &mut top_dst[(len - 1) * 4..],
        );
        if let (Some(by), Some(bd)) = (bottom_y, &mut bottom_dst) {
            put(
                by[len - 1],
                (3 * l_uv + tl_uv + 0x0002_0002) >> 2,
                &mut bd[(len - 1) * 4..],
            );
        }
    }
}

/// A VP8 keyframe to RGBA the way WebPDecodeRGBA() does (fancy
/// upsampling, EmitFancyRGB row pairing); alpha 255.
fn vp8_to_rgba(f: &oxideav_vp8::Vp8DecodedFrame) -> Vec<u8> {
    let (w, h) = (f.width as usize, f.height as usize);
    let uvw = w.div_ceil(2);
    let row =
        |y: usize, stride: usize| -> std::ops::Range<usize> { y * stride..y * stride + stride };
    let mut out = vec![255u8; w * h * 4];
    let stride = w * 4;
    {
        let (u0, v0) = (&f.u[row(0, uvw)], &f.v[row(0, uvw)]);
        upsample_pair(
            &f.y[0..w],
            None,
            u0,
            v0,
            u0,
            v0,
            &mut out[0..stride],
            None,
            w,
        );
    }
    let mut y = 1;
    while y + 1 < h {
        let k = y.div_ceil(2);
        let (tu, tv) = (&f.u[row(k - 1, uvw)], &f.v[row(k - 1, uvw)]);
        let (cu, cv) = (&f.u[row(k, uvw)], &f.v[row(k, uvw)]);
        let (top, bottom) = out[y * stride..(y + 2) * stride].split_at_mut(stride);
        upsample_pair(
            &f.y[y * w..y * w + w],
            Some(&f.y[(y + 1) * w..(y + 2) * w]),
            tu,
            tv,
            cu,
            cv,
            top,
            Some(bottom),
            w,
        );
        y += 2;
    }
    if h > 1 && h % 2 == 0 {
        let k = (h - 1) / 2;
        let (cu, cv) = (&f.u[row(k, uvw)], &f.v[row(k, uvw)]);
        upsample_pair(
            &f.y[(h - 1) * w..h * w],
            None,
            cu,
            cv,
            cu,
            cv,
            &mut out[(h - 1) * stride..h * stride],
            None,
            w,
        );
    }
    out
}

/// The RGBA pixels of a still RIFF/WEBP image of the given size.
fn decode_rgba(file: &[u8], width: u32, height: u32) -> Option<Vec<u8>> {
    if find_chunk(file, b"ANIM").is_some() {
        return None;
    }
    let n = (width * height * 4) as usize;
    if find_chunk(file, b"VP8L").is_none() {
        let vp8 = find_chunk(file, b"VP8 ")?;
        let frame = oxideav_vp8::decode_vp8(vp8).ok()?;
        if frame.width != width || frame.height != height {
            return None;
        }
        let mut rgba = vp8_to_rgba(&frame);
        if let Ok(Some(alpha)) = oxideav_webp::decode_alpha_plane(file) {
            for (p, a) in rgba.as_chunks_mut::<4>().0.iter_mut().zip(alpha) {
                p[3] = a;
            }
        }
        return Some(rgba);
    }
    let img = oxideav_webp::decode_webp(file).ok()?;
    let frame = img.frames.into_iter().next()?;
    if frame.width != width || frame.height != height || frame.rgba.len() < n {
        return None;
    }
    Some(frame.rgba)
}

/// gdImageCreateFromWebpCtx()
pub fn create_from_webp(data: &[u8]) -> Option<Image> {
    let Some((width, height)) = get_info(data) else {
        gd_error("gd-webp cannot get webp info");
        return None;
    };
    let mut im = Image::create_true_color(width as i32, height as i32)?;
    let wrapped;
    let file = if data.starts_with(b"RIFF") {
        data
    } else {
        wrapped = wrap_bare(data);
        &wrapped
    };
    // WebPDecodeARGB(): still images only
    let Some(rgba) = decode_rgba(file, width, height) else {
        gd_error("gd-webp cannot allocate temporary buffer");
        return None;
    };
    let mut p = 0;
    for y in 0..height as usize {
        for x in 0..width as usize {
            let px = &rgba[p..p + 4];
            let a = ALPHA_MAX - (px[3] as i32 >> 1);
            im.tpixels[y][x] = true_color_alpha(px[0] as i32, px[1] as i32, px[2] as i32, a);
            p += 4;
        }
    }
    im.save_alpha_flag = 1;
    Some(im)
}

/// VP8RGBToY / VP8RGBToU / VP8RGBToV of libwebp's yuv.h (16-bit fixed
/// point BT.601, studio range); `rnd` is the rounding term.
fn rgb_to_y(r: i32, g: i32, b: i32) -> u8 {
    let luma = 16839 * r + 33059 * g + 6420 * b;
    ((luma + (1 << 15) + (16 << 16)) >> 16) as u8
}

fn clip_uv(uv: i32, rounding: i32) -> u8 {
    let uv = (uv + rounding + (128 << 18)) >> 18;
    if uv & !0xff == 0 {
        uv as u8
    } else if uv < 0 {
        0
    } else {
        255
    }
}

/// Lossy (VP8) encode of an RGBA buffer, with an ALPH chunk when the
/// image is not opaque.
fn encode_lossy(rgba: &[u8], w: u32, h: u32, quality: i32) -> Option<Vec<u8>> {
    let (wu, hu) = (w as usize, h as usize);
    let uvw = wu.div_ceil(2);
    let uvh = hu.div_ceil(2);
    let mut yp = vec![0u8; wu * hu];
    let mut up = vec![0u8; uvw * uvh];
    let mut vp = vec![0u8; uvw * uvh];
    let px = |x: usize, y: usize| -> (i32, i32, i32) {
        let o = (y * wu + x) * 4;
        (rgba[o] as i32, rgba[o + 1] as i32, rgba[o + 2] as i32)
    };
    for y in 0..hu {
        for x in 0..wu {
            let (r, g, b) = px(x, y);
            yp[y * wu + x] = rgb_to_y(r, g, b);
        }
    }
    for cy in 0..uvh {
        for cx in 0..uvw {
            // sum of the (up to) 2x2 block, scaled to 4 samples
            let (mut r, mut g, mut b, mut n) = (0, 0, 0, 0);
            for dy in 0..2 {
                for dx in 0..2 {
                    let (x, y) = (cx * 2 + dx, cy * 2 + dy);
                    if x < wu && y < hu {
                        let (pr, pg, pb) = px(x, y);
                        r += pr;
                        g += pg;
                        b += pb;
                        n += 1;
                    }
                }
            }
            r = r * 4 / n;
            g = g * 4 / n;
            b = b * 4 / n;
            up[cy * uvw + cx] = clip_uv(-9719 * r - 19081 * g + 28800 * b, 1 << 17);
            vp[cy * uvw + cx] = clip_uv(28800 * r - 24116 * g - 4684 * b, 1 << 17);
        }
    }
    let frame = oxideav_vp8::I420Frame {
        width: w,
        height: h,
        y: &yp,
        u: &up,
        v: &vp,
        y_stride: wu,
        uv_stride: uvw,
    };
    let params = oxideav_vp8::KeyframeParams {
        y_ac_qi: oxideav_vp8::quality_to_qindex(quality as f32),
        ..Default::default()
    };
    let vp8 = oxideav_vp8::encode_keyframe(&frame, &params).ok()?;
    let opaque = rgba.as_chunks::<4>().0.iter().all(|p| p[3] == 255);
    if opaque {
        return Some(riff(&[(b"VP8 ", &vp8)]));
    }
    // VP8X (alpha flag) + raw, unfiltered ALPH + VP8
    let mut vp8x = vec![0x10u8, 0, 0, 0];
    vp8x.extend_from_slice(&(w - 1).to_le_bytes()[..3]);
    vp8x.extend_from_slice(&(h - 1).to_le_bytes()[..3]);
    let mut alph = Vec::with_capacity(wu * hu + 1);
    alph.push(0);
    alph.extend(rgba.as_chunks::<4>().0.iter().map(|p| p[3]));
    Some(riff(&[(b"VP8X", &vp8x), (b"ALPH", &alph), (b"VP8 ", &vp8)]))
}

/// Lossless (VP8L) encode in a simple RIFF/WEBP file, as
/// WebPEncodeLosslessRGBA() writes it.
///
/// The entropy coder is oxideav-webp's subtract-green / LZ77 /
/// color-cache chooser; its predictor-transform encoder is not used
/// because oxideav-webp 0.2.3 takes the top-right neighbour of the
/// rightmost column from the previous row's first pixel instead of the
/// current row's (libwebp `upper[width]`), which other decoders read
/// differently.
fn encode_lossless(rgba: &[u8], w: u32, h: u32) -> Option<Vec<u8>> {
    let argb: Vec<u32> = rgba
        .as_chunks::<4>()
        .0
        .iter()
        .map(|p| (p[3] as u32) << 24 | (p[0] as u32) << 16 | (p[1] as u32) << 8 | p[2] as u32)
        .collect();
    let has_alpha = rgba.as_chunks::<4>().0.iter().any(|p| p[3] != 255);
    let body = oxideav_webp::vp8l_encode::encode_argb_literals_with_width(&argb, w);
    let bits = (w - 1) | (h - 1) << 14 | (has_alpha as u32) << 28;
    let mut vp8l = Vec::with_capacity(body.len() + 5);
    vp8l.push(0x2f);
    vp8l.extend_from_slice(&bits.to_le_bytes());
    vp8l.extend_from_slice(&body);
    Some(riff(&[(b"VP8L", &vp8l)]))
}

/// gdImageWebpCtx(): None when nothing is written
pub fn image_webp(im: &Image, quality: i32) -> Option<Vec<u8>> {
    if !im.true_color {
        gd_error("Palette image not supported by webp");
        return None;
    }
    let quality = if quality == -1 { 80 } else { quality };
    if overflow2(im.sx, 4) {
        return None;
    }
    if overflow2(im.sx * 4, im.sy) {
        return None;
    }
    let (w, h) = (im.sx as u32, im.sy as u32);
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for row in &im.tpixels {
        for &c in row {
            let a = true_color_get_alpha(c);
            let a = if a == 127 {
                0
            } else {
                255 - ((a << 1) + (a >> 6))
            };
            rgba.push(true_color_get_red(c) as u8);
            rgba.push(true_color_get_green(c) as u8);
            rgba.push(true_color_get_blue(c) as u8);
            rgba.push(a as u8);
        }
    }
    // WebP dimensions are limited to 16383 (VP8 / VP8L 14-bit fields)
    let out = if w > 16383 || h > 16383 {
        None
    } else if quality >= WEBP_LOSSLESS {
        encode_lossless(&rgba, w, h)
    } else {
        encode_lossy(&rgba, w, h, quality)
    };
    match out {
        Some(o) if !o.is_empty() => Some(o),
        _ => {
            gd_error("gd-webp encoding failed");
            None
        }
    }
}
