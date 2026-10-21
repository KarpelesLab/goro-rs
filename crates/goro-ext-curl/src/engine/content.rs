//! Response decoding: HTTP chunked transfer-encoding (lib/http_chunks.c)
//! and gzip/deflate content decoding (lib/content_encoding.c).

use goro_zlib::{Inflate, Io, MAX_WBITS, Z_BLOCK, Z_BUF_ERROR, Z_DATA_ERROR, Z_OK, Z_STREAM_END};

use super::CurlCode;
use super::consts::code;

const CHUNK_MAXNUM_LEN: usize = 16;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ChunkState {
    Hex,
    Lf,
    Data,
    PostLf,
    Stop,
    Trailer,
    TrailerCr,
    TrailerPostCr,
    Done,
    Failed,
}

/// Output of the chunk decoder
pub(crate) enum ChunkOut<'a> {
    Body(&'a [u8]),
    Trailer(Vec<u8>),
}

/// struct Curl_chunker
pub(crate) struct Chunker {
    state: ChunkState,
    hex: Vec<u8>,
    datasize: u64,
    trailer: Vec<u8>,
    /// CHUNKE_* last error as text for "%s in chunked-encoding"
    pub last_err: &'static str,
    pub passthru_err: bool,
}

impl Chunker {
    pub fn new() -> Chunker {
        Chunker {
            state: ChunkState::Hex,
            hex: Vec::new(),
            datasize: 0,
            trailer: Vec::new(),
            last_err: "",
            passthru_err: false,
        }
    }

    pub fn is_done(&self) -> bool {
        self.state == ChunkState::Done
    }

    fn reset(&mut self) {
        self.state = ChunkState::Hex;
        self.hex.clear();
        self.datasize = 0;
        self.trailer.clear();
    }

    /// httpchunk_readwrite(): feeds `buf`; returns bytes consumed. `out` is
    /// called with decoded pieces; an error from `out` aborts.
    /// Err((code, failf message or None))
    pub fn process(
        &mut self,
        mut buf: &[u8],
        te_skip: bool,
        out: &mut dyn FnMut(ChunkOut) -> Result<(), CurlCode>,
    ) -> Result<usize, (CurlCode, Option<String>)> {
        let mut consumed = 0usize;
        if self.state == ChunkState::Done {
            return Ok(0);
        }
        if self.state == ChunkState::Failed {
            return Err((code::RECV_ERROR, None));
        }
        if te_skip && let Err(e) = out(ChunkOut::Body(buf)) {
            self.state = ChunkState::Failed;
            self.passthru_err = true;
            return Err((e, None));
        }
        while !buf.is_empty() {
            match self.state {
                ChunkState::Hex => {
                    let c = buf[0];
                    if c.is_ascii_hexdigit() {
                        if self.hex.len() >= CHUNK_MAXNUM_LEN {
                            self.state = ChunkState::Failed;
                            self.last_err = "Too long hexadecimal number";
                            return Err((
                                code::RECV_ERROR,
                                Some(format!("chunk hex-length longer than {CHUNK_MAXNUM_LEN}")),
                            ));
                        }
                        self.hex.push(c);
                        buf = &buf[1..];
                        consumed += 1;
                    } else {
                        if self.hex.is_empty() {
                            self.state = ChunkState::Failed;
                            self.last_err = "Illegal or missing hexadecimal sequence";
                            return Err((
                                code::RECV_ERROR,
                                Some(format!(
                                    "chunk hex-length char not a hex digit: 0x{:x}",
                                    c as i8 as i32 as u32
                                )),
                            ));
                        }
                        let s = std::str::from_utf8(&self.hex).unwrap_or("");
                        match u64::from_str_radix(s, 16) {
                            Ok(v) if v <= i64::MAX as u64 => self.datasize = v,
                            _ => {
                                self.state = ChunkState::Failed;
                                self.last_err = "Illegal or missing hexadecimal sequence";
                                return Err((
                                    code::RECV_ERROR,
                                    Some(format!("invalid chunk size: '{s}'")),
                                ));
                            }
                        }
                        self.state = ChunkState::Lf;
                    }
                }
                ChunkState::Lf => {
                    if buf[0] == b'\n' {
                        self.state = if self.datasize == 0 {
                            ChunkState::Trailer
                        } else {
                            ChunkState::Data
                        };
                    }
                    buf = &buf[1..];
                    consumed += 1;
                }
                ChunkState::Data => {
                    let piece = (buf.len() as u64).min(self.datasize) as usize;
                    if !te_skip && let Err(e) = out(ChunkOut::Body(&buf[..piece])) {
                        self.state = ChunkState::Failed;
                        self.passthru_err = true;
                        return Err((e, None));
                    }
                    consumed += piece;
                    self.datasize -= piece as u64;
                    buf = &buf[piece..];
                    if self.datasize == 0 {
                        self.state = ChunkState::PostLf;
                    }
                }
                ChunkState::PostLf => {
                    if buf[0] == b'\n' {
                        self.reset();
                    } else if buf[0] != b'\r' {
                        self.state = ChunkState::Failed;
                        self.last_err = "Malformed encoding found";
                        return Err((code::RECV_ERROR, None));
                    }
                    buf = &buf[1..];
                    consumed += 1;
                }
                ChunkState::Trailer => {
                    let c = buf[0];
                    if c == b'\r' || c == b'\n' {
                        if !self.trailer.is_empty() {
                            self.trailer.extend_from_slice(b"\r\n");
                            let tr = std::mem::take(&mut self.trailer);
                            if !te_skip && let Err(e) = out(ChunkOut::Trailer(tr)) {
                                self.state = ChunkState::Failed;
                                self.passthru_err = true;
                                return Err((e, None));
                            }
                            self.state = ChunkState::TrailerCr;
                            if c == b'\n' {
                                continue;
                            }
                        } else {
                            self.state = ChunkState::TrailerPostCr;
                            continue;
                        }
                    } else {
                        self.trailer.push(c);
                    }
                    buf = &buf[1..];
                    consumed += 1;
                }
                ChunkState::TrailerCr => {
                    if buf[0] == b'\n' {
                        self.state = ChunkState::TrailerPostCr;
                        buf = &buf[1..];
                        consumed += 1;
                    } else {
                        self.state = ChunkState::Failed;
                        self.last_err = "Malformed encoding found";
                        return Err((code::RECV_ERROR, None));
                    }
                }
                ChunkState::TrailerPostCr => {
                    if buf[0] != b'\r' && buf[0] != b'\n' {
                        self.state = ChunkState::Trailer;
                        continue;
                    }
                    if buf[0] == b'\r' {
                        buf = &buf[1..];
                        consumed += 1;
                    }
                    self.state = ChunkState::Stop;
                }
                ChunkState::Stop => {
                    if buf[0] == b'\n' {
                        consumed += 1;
                        self.state = ChunkState::Done;
                        return Ok(consumed);
                    }
                    self.state = ChunkState::Failed;
                    self.last_err = "Malformed encoding found";
                    return Err((code::RECV_ERROR, None));
                }
                ChunkState::Done => return Ok(consumed),
                ChunkState::Failed => return Err((code::RECV_ERROR, None)),
            }
        }
        Ok(consumed)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ZState {
    Uninit,
    Init,
    Inflating,
    ExternalTrailer,
    InitGzip,
}

/// A gzip / deflate content decoder (struct zlib_writer)
pub(crate) struct ZDecoder {
    z: Box<Inflate>,
    state: ZState,
    trailerlen: usize,
    gzip: bool,
}

/// One decoder of the content-encoding stack
pub(crate) enum ContentDecoder {
    Identity,
    Zlib(ZDecoder),
    /// unknown encoding: error at first body write
    Error,
}

pub(crate) const ALL_ENCODINGS: &str = "deflate, gzip";

impl ContentDecoder {
    /// find_unencode_writer() for the general decoders
    pub fn by_name(name: &[u8]) -> ContentDecoder {
        let n = name.to_ascii_lowercase();
        match &n[..] {
            b"identity" | b"none" => ContentDecoder::Identity,
            b"deflate" => match Inflate::new(MAX_WBITS) {
                Ok(z) => ContentDecoder::Zlib(ZDecoder {
                    z,
                    state: ZState::Init,
                    trailerlen: 0,
                    gzip: false,
                }),
                Err(_) => ContentDecoder::Error,
            },
            b"gzip" | b"x-gzip" => match Inflate::new(MAX_WBITS + 32) {
                Ok(z) => ContentDecoder::Zlib(ZDecoder {
                    z,
                    state: ZState::InitGzip,
                    trailerlen: 0,
                    gzip: true,
                }),
                Err(_) => ContentDecoder::Error,
            },
            _ => ContentDecoder::Error,
        }
    }

    /// Decode `buf`, handing output to `out`. Err((code, failf message))
    pub fn write(
        &mut self,
        buf: &[u8],
        out: &mut dyn FnMut(&[u8]) -> Result<(), CurlCode>,
    ) -> Result<(), (CurlCode, Option<String>)> {
        match self {
            ContentDecoder::Identity => out(buf).map_err(|e| (e, None)),
            ContentDecoder::Error => Err((
                code::BAD_CONTENT_ENCODING,
                Some(format!(
                    "Unrecognized content encoding type. libcurl understands {ALL_ENCODINGS} content encodings."
                )),
            )),
            ContentDecoder::Zlib(zd) => zd.write(buf, out),
        }
    }
}

fn zlib_error(z: &Inflate) -> (CurlCode, Option<String>) {
    let msg = match z.msg {
        Some(m) => format!("Error while processing content unencoding: {m}"),
        None => "Error while processing content unencoding: Unknown failure within decompression software."
            .to_string(),
    };
    (code::BAD_CONTENT_ENCODING, Some(msg))
}

impl ZDecoder {
    fn process_trailer(&mut self, avail: &mut &[u8]) -> Result<(), (CurlCode, Option<String>)> {
        let len = avail.len().min(self.trailerlen);
        self.trailerlen -= len;
        *avail = &avail[len..];
        if !avail.is_empty() {
            self.state = ZState::Uninit;
            return Err((code::WRITE_ERROR, None));
        }
        if self.trailerlen == 0 {
            self.state = ZState::Uninit;
        } else {
            self.state = ZState::ExternalTrailer;
        }
        Ok(())
    }

    fn write(
        &mut self,
        buf: &[u8],
        out: &mut dyn FnMut(&[u8]) -> Result<(), CurlCode>,
    ) -> Result<(), (CurlCode, Option<String>)> {
        if buf.is_empty() {
            return Ok(());
        }
        if self.state == ZState::ExternalTrailer {
            let mut b = buf;
            return self.process_trailer(&mut b);
        }
        let started = if self.gzip {
            ZState::InitGzip
        } else {
            ZState::Inflating
        };
        if self.state != ZState::Init
            && self.state != ZState::Inflating
            && self.state != ZState::InitGzip
        {
            self.state = ZState::Uninit;
            return Err((code::WRITE_ERROR, None));
        }
        let mut input: &[u8] = buf;
        let orig = buf;
        let mut outbuf = vec![0u8; 16384];
        loop {
            let mut io = Io::new(input, &mut outbuf);
            let status = self.z.inflate(&mut io, Z_BLOCK);
            let used = io.in_pos;
            let produced = io.out_pos;
            input = &input[used..];
            if produced > 0 && (status == Z_OK || status == Z_STREAM_END) {
                self.state = started;
                if let Err(e) = out(&outbuf[..produced]) {
                    self.state = ZState::Uninit;
                    return Err((e, None));
                }
            }
            match status {
                Z_OK => continue,
                Z_BUF_ERROR => break,
                Z_STREAM_END => {
                    let mut rest = input;
                    let r = self.process_trailer(&mut rest);
                    return r;
                }
                Z_DATA_ERROR => {
                    if self.state == ZState::Init {
                        if self.z.reset2(-MAX_WBITS) == Z_OK {
                            input = orig;
                            self.state = ZState::Inflating;
                            self.trailerlen = 4;
                            continue;
                        }
                        self.state = ZState::Uninit;
                    }
                    let e = zlib_error(&self.z);
                    self.state = ZState::Uninit;
                    return Err(e);
                }
                _ => {
                    let e = zlib_error(&self.z);
                    self.state = ZState::Uninit;
                    return Err(e);
                }
            }
        }
        if self.state == ZState::Init {
            self.state = started;
        }
        Ok(())
    }
}
