//! php_image_register_handler(): image types provided by extensions
//! (ext/standard/image.c's php_image_handlers), e.g. ext/libxml's SVG.

use crate::value::Value;
use crate::vm::Vm;
use std::cell::RefCell;

/// IMAGE_FILETYPE_FIXED_COUNT: the first id handed to a registered handler.
pub const IMAGE_FILETYPE_FIXED_COUNT: i64 = 21;

/// php_gfxinfo as produced by a handler.
pub struct ImageInfo {
    pub width: u32,
    pub height: u32,
    pub width_unit: Option<String>,
    pub height_unit: Option<String>,
}

/// struct php_image_handler. The callbacks see the whole image data.
pub struct ImageHandler {
    pub mime_type: &'static str,
    pub extension: &'static str,
    pub const_name: &'static str,
    pub identify: fn(&[u8]) -> bool,
    pub get_info: fn(&[u8]) -> Option<ImageInfo>,
}

thread_local! {
    static HANDLERS: RefCell<Vec<(i64, &'static ImageHandler)>> = const { RefCell::new(Vec::new()) };
}

/// php_image_register_handler(): the handler's image type id; its
/// IMAGETYPE_* constant is registered and IMAGETYPE_COUNT bumped.
pub fn register(vm: &mut Vm, h: &'static ImageHandler) -> i64 {
    let id = HANDLERS.with(|hs| {
        let mut hs = hs.borrow_mut();
        if let Some((id, _)) = hs.iter().find(|(_, x)| x.const_name == h.const_name) {
            return *id;
        }
        let id = IMAGE_FILETYPE_FIXED_COUNT + hs.len() as i64;
        hs.push((id, h));
        id
    });
    vm.constants
        .insert(h.const_name.as_bytes().to_vec(), Value::Long(id));
    let count = vm
        .constants
        .get(b"IMAGETYPE_COUNT".as_slice())
        .map(|v| v.to_long())
        .unwrap_or(IMAGE_FILETYPE_FIXED_COUNT);
    if count <= id {
        vm.constants
            .insert(b"IMAGETYPE_COUNT".to_vec(), Value::Long(id + 1));
    }
    id
}

/// The handler registered for `image_type`.
pub fn find(image_type: i64) -> Option<&'static ImageHandler> {
    HANDLERS.with(|hs| {
        hs.borrow()
            .iter()
            .find(|(id, _)| *id == image_type)
            .map(|(_, h)| *h)
    })
}

/// php_getimagetype()'s last step: the first handler identifying `data`.
pub fn identify(data: &[u8]) -> Option<i64> {
    let hs: Vec<(i64, &'static ImageHandler)> = HANDLERS.with(|hs| hs.borrow().clone());
    hs.into_iter()
        .find(|(_, h)| (h.identify)(data))
        .map(|(id, _)| id)
}
