//! SAPI header state shared between extensions (SG(sapi_headers)): the
//! content type the script set, and whether the default one is still to be
//! sent.

use std::cell::{Cell, RefCell};

thread_local! {
    static MIMETYPE: RefCell<Option<Vec<u8>>> = const { RefCell::new(None) };
    static SEND_DEFAULT_CONTENT_TYPE: Cell<bool> = const { Cell::new(true) };
}

/// SG(sapi_headers).mimetype: the first Content-Type set by header()
/// (with the default charset applied).
pub fn mimetype() -> Option<Vec<u8>> {
    MIMETYPE.with(|m| m.borrow().clone())
}

/// sapi_header_op() on a Content-Type header: records its MIME type unless
/// one is already known, and the default content type is no longer sent.
pub fn content_type_set(mimetype: &[u8]) {
    MIMETYPE.with(|m| {
        let mut m = m.borrow_mut();
        if m.is_none() {
            *m = Some(mimetype.to_vec());
        }
    });
    SEND_DEFAULT_CONTENT_TYPE.with(|s| s.set(false));
}

/// SG(sapi_headers).send_default_content_type
pub fn send_default_content_type() -> bool {
    SEND_DEFAULT_CONTENT_TYPE.with(|s| s.get())
}

thread_local! {
    static ADD_HEADER: Cell<Option<fn(&[u8], bool)>> = const { Cell::new(None) };
}

/// Install the SAPI's header list (sapi_header_add_op()).
pub fn set_add_header_hook(f: fn(&[u8], bool)) {
    ADD_HEADER.with(|h| h.set(Some(f)));
}

/// sapi_add_header_ex() for extensions: a raw "Name: value" line.
pub fn add_header(line: &[u8], replace: bool) {
    if let Some(colon) = line.iter().position(|&c| c == b':') {
        if line[..colon].eq_ignore_ascii_case(b"Content-Type") {
            content_type_set(line[colon + 1..].trim_ascii_start());
        }
    }
    if let Some(f) = ADD_HEADER.with(|h| h.get()) {
        f(line, replace);
    }
}

/// sapi_activate(): a new request starts with no headers.
pub fn reset() {
    MIMETYPE.with(|m| *m.borrow_mut() = None);
    SEND_DEFAULT_CONTENT_TYPE.with(|s| s.set(true));
}
