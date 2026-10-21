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
