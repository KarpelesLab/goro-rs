//! Compression methods (PKWARE APPNOTE.TXT 4.4.5): store and deflate are
//! handled here and by goro-ext-zlib; bzip2, deflate64, LZMA, zstd, xz and
//! the legacy shrink/reduce/implode codecs come from compcol.

use crate::error::{Error, Result, er};

/// The method numbers (PHP's `ZipArchive::CM_*`).
pub mod cm {
    pub const DEFAULT: i32 = -1;
    pub const STORE: u16 = 0;
    pub const SHRINK: u16 = 1;
    pub const REDUCE_1: u16 = 2;
    pub const REDUCE_2: u16 = 3;
    pub const REDUCE_3: u16 = 4;
    pub const REDUCE_4: u16 = 5;
    pub const IMPLODE: u16 = 6;
    pub const DEFLATE: u16 = 8;
    pub const DEFLATE64: u16 = 9;
    pub const PKWARE_IMPLODE: u16 = 10;
    pub const BZIP2: u16 = 12;
    pub const LZMA: u16 = 14;
    pub const TERSE: u16 = 18;
    pub const LZ77: u16 = 19;
    pub const LZMA2: u16 = 33;
    pub const ZSTD: u16 = 93;
    pub const XZ: u16 = 95;
    pub const JPEG: u16 = 96;
    pub const WAVPACK: u16 = 97;
    pub const PPMD: u16 = 98;
    /// WinZip AES: the real method is in extra field 0x9901.
    pub const AES: u16 = 99;
}

/// Whether entries of `method` can be decompressed.
pub fn can_decompress(method: u16) -> bool {
    matches!(
        method,
        cm::STORE | cm::SHRINK | cm::REDUCE_1
            ..=cm::REDUCE_4
                | cm::IMPLODE
                | cm::DEFLATE
                | cm::DEFLATE64
                | cm::BZIP2
                | cm::LZMA
                | cm::ZSTD
                | cm::XZ
    )
}

/// Whether data can be compressed with `method`.
pub fn can_compress(method: u16) -> bool {
    matches!(
        method,
        cm::STORE | cm::DEFLATE | cm::BZIP2 | cm::ZSTD | cm::XZ
    )
}

/// The "version needed to extract" for a method (APPNOTE 4.4.3).
pub fn version_needed(method: u16) -> u16 {
    match method {
        cm::STORE => 10,
        cm::DEFLATE => 20,
        cm::DEFLATE64 => 21,
        cm::PKWARE_IMPLODE => 25,
        cm::BZIP2 => 46,
        cm::LZMA | cm::ZSTD | cm::XZ | cm::PPMD => 63,
        _ => 10,
    }
}

fn codec_err(_: compcol::Error) -> Error {
    Error::new(er::COMPRESSED_DATA)
}

/// The cap on decoded output: the declared size plus slack (an entry that
/// decodes to more than it declares is reported by the caller).
fn cap(size: u64) -> u64 {
    size.saturating_add(1 << 16)
}

/// Raw deflate decoding up to `max` bytes.
pub fn inflate(data: &[u8], max: u64) -> Result<Vec<u8>> {
    use goro_ext_zlib::{InflateFlush, InflateStatus, InflateWrap, Inflater};
    let mut inf = Inflater::new(InflateWrap::Raw, 15);
    let mut out = Vec::with_capacity(max.min(1 << 26) as usize);
    let mut pos = 0;
    loop {
        let room = (max as usize).saturating_sub(out.len()).clamp(1, 1 << 20);
        let before = out.len();
        let (n, st) = inf.inflate(&data[pos..], &mut out, room, InflateFlush::None);
        pos += n;
        match st {
            InflateStatus::StreamEnd => return Ok(out),
            InflateStatus::NeedDict | InflateStatus::DataError(_) => {
                return Err(Error::new(er::COMPRESSED_DATA));
            }
            InflateStatus::Ok | InflateStatus::BufError => {
                if out.len() as u64 > max {
                    return Ok(out);
                }
                if n == 0 && out.len() == before {
                    // truncated stream
                    return Err(Error::new(er::EOF));
                }
            }
        }
    }
}

/// Decompresses an entry's data. `flags` is the general purpose bit flag
/// (the implode window and tree bits, the LZMA end marker), `size` the
/// declared uncompressed size.
pub fn decompress(method: u16, flags: u16, data: &[u8], size: u64) -> Result<Vec<u8>> {
    use compcol::vec::decompress_to_vec_capped_with as dec;
    match method {
        cm::STORE => Ok(data.to_vec()),
        cm::DEFLATE => inflate(data, cap(size)),
        cm::DEFLATE64 => dec::<compcol::deflate64::Deflate64>(data, Default::default(), cap(size))
            .map_err(codec_err),
        cm::BZIP2 => {
            dec::<compcol::bzip2::Bzip2>(data, Default::default(), cap(size)).map_err(codec_err)
        }
        cm::ZSTD => dec::<compcol::zstd::Zstd>(data, (), cap(size)).map_err(codec_err),
        cm::XZ => dec::<compcol::xz::Xz>(data, (), cap(size)).map_err(codec_err),
        cm::LZMA => {
            // APPNOTE 5.8.8: version (2), properties size (2), properties,
            // then the raw stream; bit 1 marks an end-of-stream marker.
            // Re-framed as the legacy .lzma header compcol reads.
            if data.len() < 4 {
                return Err(Error::new(er::COMPRESSED_DATA));
            }
            let psize = u16::from_le_bytes([data[2], data[3]]) as usize;
            if psize != 5 || data.len() < 4 + psize {
                return Err(Error::new(er::COMPRESSED_DATA));
            }
            let mut framed = Vec::with_capacity(data.len() + 9);
            framed.extend_from_slice(&data[4..9]);
            let n = if flags & 2 != 0 { u64::MAX } else { size };
            framed.extend_from_slice(&n.to_le_bytes());
            framed.extend_from_slice(&data[9..]);
            compcol::vec::decompress_to_vec_capped::<compcol::lzma::Lzma>(&framed, cap(size))
                .map_err(codec_err)
        }
        cm::SHRINK => {
            let n = u32::try_from(size).map_err(|_| Error::new(er::COMPNOTSUPP))?;
            let mut framed = n.to_le_bytes().to_vec();
            framed.extend_from_slice(data);
            compcol::vec::decompress_to_vec_capped::<compcol::zip_shrink::ZipShrink>(
                &framed,
                cap(size),
            )
            .map_err(codec_err)
        }
        cm::REDUCE_1..=cm::REDUCE_4 => {
            let n = u32::try_from(size).map_err(|_| Error::new(er::COMPNOTSUPP))?;
            let mut framed = vec![(method - 1) as u8];
            framed.extend_from_slice(&n.to_le_bytes());
            framed.extend_from_slice(data);
            compcol::vec::decompress_to_vec_capped::<compcol::zip_reduce::ZipReduce>(
                &framed,
                cap(size),
            )
            .map_err(codec_err)
        }
        cm::IMPLODE => {
            // APPNOTE 4.4.4: bit 1 = 8K window, bit 2 = 3 Shannon-Fano trees
            let n = u32::try_from(size).map_err(|_| Error::new(er::COMPNOTSUPP))?;
            let f = ((flags >> 1) & 1) as u8 | (((flags >> 2) & 1) as u8) << 1;
            let mut framed = vec![f];
            framed.extend_from_slice(&n.to_le_bytes());
            framed.extend_from_slice(data);
            compcol::vec::decompress_to_vec_capped::<compcol::zip_implode::ZipImplode>(
                &framed,
                cap(size),
            )
            .map_err(codec_err)
        }
        _ => Err(Error::new(er::COMPNOTSUPP)),
    }
}

/// Compresses with `method` at `level` (-1 or 0 for the codec's default).
pub fn compress(method: u16, level: i32, data: &[u8]) -> Result<Vec<u8>> {
    use compcol::vec::compress_to_vec_with as enc;
    match method {
        cm::STORE => Ok(data.to_vec()),
        cm::DEFLATE => {
            let l = if (1..=9).contains(&level) { level } else { 6 };
            Ok(goro_ext_zlib::compress(data, l, goro_ext_zlib::Wrap::Raw))
        }
        cm::BZIP2 => {
            let l = if (1..=9).contains(&level) {
                level as u8
            } else {
                9
            };
            enc::<compcol::bzip2::Bzip2>(data, compcol::bzip2::EncoderConfig { level: l })
                .map_err(codec_err)
        }
        cm::XZ => {
            let l = if (0..=9).contains(&level) {
                level as u8
            } else {
                6
            };
            enc::<compcol::xz::Xz>(data, compcol::xz::EncoderConfig { level: l }).map_err(codec_err)
        }
        cm::ZSTD => {
            let l = if level > 0 { level } else { 0 };
            let cfg = compcol::zstd::EncoderConfig { level: l };
            enc::<compcol::zstd::Zstd>(data, cfg).map_err(codec_err)
        }
        _ => Err(Error::new(er::COMPNOTSUPP)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrips() {
        let data: Vec<u8> = (0..20000u32)
            .map(|i| (i % 251) as u8 ^ (i / 97) as u8)
            .collect();
        for m in [cm::STORE, cm::DEFLATE, cm::BZIP2, cm::XZ, cm::ZSTD] {
            let c = compress(m, -1, &data).unwrap();
            assert_eq!(
                decompress(m, 0, &c, data.len() as u64).unwrap(),
                data,
                "method {m}"
            );
        }
    }

    #[test]
    fn empty_deflate() {
        let c = compress(cm::DEFLATE, -1, b"").unwrap();
        assert_eq!(decompress(cm::DEFLATE, 0, &c, 0).unwrap(), b"");
    }
}
