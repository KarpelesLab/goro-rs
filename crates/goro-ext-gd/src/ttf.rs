//! TrueType text (imagettftext, imagettfbbox, imagefttext,
//! imageftbbox).

use crate::image::Image;

/// A text request.
pub struct TextReq<'a> {
    pub font: &'a [u8],
    pub size: f64,
    pub angle: f64,
    pub x: i32,
    pub y: i32,
    pub color: i32,
    pub text: &'a [u8],
    pub linespacing: f64,
}

/// Renders (when `im` is given) and returns the 8 bounding box
/// coordinates, or an error message.
pub fn text(_im: Option<&mut Image>, _req: &TextReq) -> Result<[i32; 8], String> {
    Err("Could not read font".to_string())
}
