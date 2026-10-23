//! php_rfc1867_callback events (main/rfc1867.h), used by the session
//! upload progress tracking.

/// One multipart/form-data parsing event. `post_bytes_processed` is
/// SG(read_post_bytes) at the time of the event.
pub enum Event<'a> {
    Start {
        content_length: i64,
        /// Raw query string and Cookie header, for finding the session ID
        /// before the request variables are registered.
        query_string: &'a [u8],
        cookie: &'a [u8],
    },
    FormData {
        post_bytes_processed: i64,
        name: &'a [u8],
        value: &'a [u8],
    },
    FileStart {
        post_bytes_processed: i64,
        name: &'a [u8],
        filename: &'a [u8],
    },
    FileData {
        post_bytes_processed: i64,
        offset: i64,
        length: i64,
    },
    FileEnd {
        post_bytes_processed: i64,
        temp_filename: Option<&'a [u8]>,
        cancel_upload: i64,
    },
    End {
        post_bytes_processed: i64,
    },
}

/// Callback: returns false for FAILURE (cancels the upload / the part).
pub type Callback = fn(&mut crate::vm::Vm, &Event) -> bool;

/// An encoding of the multibyte layer (an opaque id mbstring hands out).
pub type EncodingId = usize;

/// The multibyte callbacks mbstring installs for multipart/form-data
/// parsing (php_rfc1867_set_multibyte_callbacks() and the
/// zend_multibyte functions rfc1867.c uses).
#[derive(Clone, Copy)]
pub struct Multibyte {
    /// php_rfc1867_encoding_translation() with an internal encoding: the
    /// detect order (php_rfc1867_get_detect_order) is used, None otherwise.
    pub start: fn(&mut crate::vm::Vm) -> bool,
    /// zend_multibyte_encoding_detector() of a header line over the detect
    /// order.
    pub detect: fn(&mut crate::vm::Vm, &[u8]) -> Option<EncodingId>,
    /// zend_multibyte_encoding_converter() from `from` to the internal
    /// encoding.
    pub convert: fn(&mut crate::vm::Vm, &[u8], EncodingId) -> Vec<u8>,
    /// zend_multibyte_get_internal_encoding()
    pub internal: fn(&mut crate::vm::Vm) -> EncodingId,
    /// php_mb_mbchar_bytes(): the length of the character starting with
    /// the byte, in the encoding.
    pub mbchar_bytes: fn(EncodingId, u8) -> usize,
}
