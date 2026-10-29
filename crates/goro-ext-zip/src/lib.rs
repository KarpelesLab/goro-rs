//! The ZIP archive format on bytes, for PHP's zip extension (`ZipArchive`,
//! `zip://`, the `zip_*` functions) and the zip form of phar archives.
//!
//! Written from PKWARE's APPNOTE.TXT (the .ZIP file format
//! specification), the WinZip AES specification (AE-1/AE-2), the
//! published TorrentZip format and the error numbering PHP documents as
//! `ZipArchive::ER_*`; behaviour checked against php-src's
//! `ext/zip/tests` expectations.
//!
//! # Low-level API
//!
//! - Reading: [`read::parse`] gives an [`read::Archive`]: the
//!   [`read::Entry`] records (every central header field, the local extra
//!   field, the data offset, WinZip AES parameters) and the archive
//!   comment. [`read::Archive::raw`] is an entry's stored bytes,
//!   [`read::Archive::decode`]/[`read::Archive::read`] its data after
//!   decryption and decompression (with the CRC check), and
//!   [`read::decode_entry`] does the same for raw bytes from elsewhere.
//! - Writing: [`write::ArchiveWriter`] (or [`write::write_archive`]) takes
//!   [`write::WriteEntry`] values whose [`write::Payload`] is either
//!   copied as stored or compressed/encrypted by the writer, with
//!   per-entry method, level, attributes, comment, extra fields and DOS
//!   time; [`write::torrentzip`] writes TorrentZip archives.
//! - Pieces: [`codec`] (methods), [`crypto`] (PKWARE and WinZip AES),
//!   [`crc`], [`dostime`], [`text`] (CP437 and the UTF-8 flag),
//!   [`error`] (libzip codes and messages).
//!
//! Times in DOS fields are read and written as UTC.

pub mod codec;
pub mod crc;
pub mod crypto;
pub mod dostime;
pub mod error;
pub mod read;
pub mod text;
pub mod write;

pub use codec::cm;
pub use error::{Error, Result, er};
pub use read::{Archive, Entry, ReadOptions, em, parse};
pub use write::{ArchiveWriter, Encryption, Payload, WriteEntry};

/// The libzip version PHP reports as `ZipArchive::LIBZIP_VERSION`: the
/// feature set implemented here is that of libzip 1.11.
pub const LIBZIP_VERSION: &str = "1.11.4";
