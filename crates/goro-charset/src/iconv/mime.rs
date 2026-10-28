//! MIME header encoding and decoding (stub).

use super::Diag;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MimeScheme {
    B,
    Q,
}

#[derive(Clone, Debug)]
pub struct MimeEncodeOptions {
    pub scheme: MimeScheme,
    pub input_charset: Vec<u8>,
    pub output_charset: Vec<u8>,
    pub line_length: i64,
    pub line_break_chars: Vec<u8>,
}

pub fn mime_encode(
    _name: &[u8],
    _value: &[u8],
    _opts: &MimeEncodeOptions,
    _diags: &mut Vec<Diag>,
) -> Option<Vec<u8>> {
    None
}

pub fn mime_decode(
    _s: &[u8],
    _mode: i64,
    _charset: &[u8],
    _diags: &mut Vec<Diag>,
) -> Option<Vec<u8>> {
    None
}

pub fn mime_decode_headers(
    _s: &[u8],
    _mode: i64,
    _charset: &[u8],
    _diags: &mut Vec<Diag>,
) -> Option<Vec<(Vec<u8>, Vec<u8>)>> {
    None
}
