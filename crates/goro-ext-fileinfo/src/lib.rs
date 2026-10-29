//! PHP's fileinfo extension (PHP manual: Fileinfo) on byte buffers: a
//! magic(5) rule engine, text/encoding classification, the tests file(1)
//! documents as built in (tar, JSON, CSV, CDF, SIMH tape), and goro's own
//! default rule set, written from the format specifications of the types
//! it knows.
//!
//! Output strings follow the expected output of php-src's
//! `ext/fileinfo/tests` and observations of PHP 8.5. The engine wiring
//! (finfo objects, streams, warnings) lives in the engine's
//! `builtins/fileinfo.rs`.

pub mod cdf;
pub mod eval;
pub mod formats;
pub mod magic;
pub mod text;

use std::rc::Rc;

use eval::{Ctx, Found, Stop};
pub use magic::Magic;
use text::TextInfo;

pub const NONE: i64 = 0;
pub const DEBUG: i64 = 0x1;
pub const SYMLINK: i64 = 0x2;
pub const COMPRESS: i64 = 0x4;
pub const DEVICES: i64 = 0x8;
pub const MIME_TYPE: i64 = 0x10;
pub const CONTINUE: i64 = 0x20;
pub const CHECK: i64 = 0x40;
pub const PRESERVE_ATIME: i64 = 0x80;
pub const RAW: i64 = 0x100;
pub const MIME_ENCODING: i64 = 0x400;
pub const MIME: i64 = MIME_TYPE | MIME_ENCODING;
pub const APPLE: i64 = 0x800;
pub const EXTENSION: i64 = 0x1000000;

/// A loaded rule set.
pub struct Db {
    pub magic: Magic,
    /// goro's own default rules: the format-specific native checks run too.
    pub builtin: bool,
}

thread_local! {
    static BUILTIN: Rc<Db> = Rc::new(Db {
        magic: match Magic::parse(&[formats::BUILTIN_MAGIC.as_bytes()], true).0 {
            Ok(m) => m,
            Err(e) => panic!("built-in magic: {:?}", e.warnings),
        },
        builtin: true,
    });
}

/// The default database.
pub fn builtin() -> Rc<Db> {
    BUILTIN.with(|b| b.clone())
}

/// Loads magic files (their contents, in order). Returns the database, or
/// None on failure, and the warnings to print.
pub fn load(sources: &[&[u8]]) -> (Option<Db>, Vec<String>) {
    let (r, w) = Magic::parse(sources, false);
    (
        r.ok().map(|magic| Db {
            magic,
            builtin: false,
        }),
        w,
    )
}

/// What identification produced, before the flags pick the output.
struct Outcome {
    desc: Vec<u8>,
    mime: Vec<u8>,
    charset: &'static str,
    apple: Option<Vec<u8>>,
    ext: Option<Vec<u8>>,
}

/// Unprintable bytes are shown as octal escapes unless FILEINFO_RAW
/// (observed on PHP 8.5).
fn escape(s: &[u8]) -> Vec<u8> {
    let mut o = Vec::with_capacity(s.len());
    for &c in s {
        if (0x20..0x7f).contains(&c) {
            o.push(c);
        } else {
            o.extend_from_slice(format!("\\{:03o}", c).as_bytes());
        }
    }
    o
}

/// Identifies `buf` (`file_mode`: it is a whole file, so negative offsets
/// work). Err is the text after "Failed identify data 0:".
pub fn identify(db: &Db, buf: &[u8], flags: i64, file_mode: bool) -> Result<Vec<u8>, String> {
    let r = classify(db, buf, flags, file_mode)?;
    let out = if flags & (MIME_TYPE | MIME_ENCODING) != 0 {
        let mut o = Vec::new();
        if flags & MIME_TYPE != 0 {
            o.extend_from_slice(&r.mime);
        }
        if flags & MIME_ENCODING != 0 {
            if !o.is_empty() {
                o.extend_from_slice(b"; charset=");
            }
            o.extend_from_slice(r.charset.as_bytes());
        }
        o
    } else if flags & EXTENSION != 0 {
        r.ext.unwrap_or_else(|| b"???".to_vec())
    } else if flags & APPLE != 0 {
        r.apple.unwrap_or_else(|| b"UNKNUNKN".to_vec())
    } else {
        r.desc
    };
    Ok(if flags & RAW != 0 { out } else { escape(&out) })
}

fn classify(db: &Db, buf: &[u8], flags: i64, file_mode: bool) -> Result<Outcome, String> {
    let simple = |d: &str, m: &str| Outcome {
        desc: d.as_bytes().to_vec(),
        mime: m.as_bytes().to_vec(),
        charset: "binary",
        apple: None,
        ext: None,
    };
    if buf.is_empty() {
        return Ok(simple("empty", "application/x-empty"));
    }
    if buf.len() == 1 {
        return Ok(simple(
            "very short file (no magic)",
            "application/octet-stream",
        ));
    }
    let all = flags & CONTINUE != 0;
    let ti = TextInfo::analyze(buf);
    let is_text = ti.is_text();
    let mut found: Vec<Found> = Vec::new();
    formats::pre_tests(buf, &ti, &mut found);
    if db.builtin && (all || found.is_empty()) {
        formats::native(buf, &ti, &mut found);
    }
    if all || found.is_empty() {
        let mut c = Ctx::new(&db.magic, buf, file_mode);
        c.run(false, all, &mut found).map_err(|Stop::Error(m)| m)?;
    }
    if db.builtin && (all || found.is_empty()) {
        formats::native_late(buf, &mut found);
    }
    if all || found.is_empty() {
        formats::post_tests(buf, &mut found);
    }
    let text_desc = if is_text {
        ti.describe()
    } else {
        String::new()
    };
    let mut text_matched = false;
    if is_text && (all || found.is_empty()) {
        let tb = ti.text_buffer(buf);
        let mut c = Ctx::new(&db.magic, &tb, file_mode);
        let mut tf = Vec::new();
        c.run(true, all, &mut tf).map_err(|Stop::Error(m)| m)?;
        if db.builtin && (all || tf.is_empty()) {
            formats::native_text(&tb, &ti, &mut tf);
        }
        for mut f in tf {
            // "awk script text" + "ASCII text" reads "awk script, ASCII
            // text"; "... text executable" puts "executable" after the
            // encoding (observed on PHP 8.5).
            let mut td = text_desc.clone();
            if let Some(d) = f.desc.strip_suffix(b" text executable") {
                f.desc.truncate(d.len());
                if !td.contains(" text executable") {
                    td = td.replacen(" text", " text executable", 1);
                }
            } else if let Some(d) = f.desc.strip_suffix(b" text") {
                f.desc.truncate(d.len());
            }
            f.desc.extend_from_slice(b", ");
            f.desc.extend_from_slice(td.as_bytes());
            text_matched = true;
            found.push(f);
        }
    }
    let fallback_mime: &[u8] = if is_text {
        b"text/plain"
    } else {
        b"application/octet-stream"
    };
    let desc = if all {
        let mut d = Vec::new();
        for f in &found {
            if !d.is_empty() {
                d.extend_from_slice(b"\n- ");
            }
            d.extend_from_slice(&f.desc);
        }
        if !text_matched {
            if d.is_empty() {
                d.extend_from_slice(fallback_desc(&ti, &text_desc).as_bytes());
            } else if is_text {
                d.extend_from_slice(b"\n- , ");
                d.extend_from_slice(text_desc.as_bytes());
            } else {
                d.extend_from_slice(b"\n- data");
            }
        }
        d
    } else {
        match found.first() {
            Some(f) => f.desc.clone(),
            None => fallback_desc(&ti, &text_desc).into_bytes(),
        }
    };
    let (mime, apple, ext) = match found.into_iter().next() {
        Some(f) => (
            f.mime.unwrap_or_else(|| fallback_mime.to_vec()),
            f.apple,
            f.ext,
        ),
        None => (fallback_mime.to_vec(), None, None),
    };
    Ok(Outcome {
        desc,
        mime,
        charset: ti.charset(),
        apple,
        ext,
    })
}

fn fallback_desc(ti: &TextInfo, text_desc: &str) -> String {
    match ti.enc {
        // An invalid UTF-32 text keeps its encoding name (observed on
        // PHP 8.5, test gh13344.phpt).
        text::Enc::Utf32Le | text::Enc::Utf32Be => ti.describe(),
        _ if ti.is_text() => text_desc.to_string(),
        _ => "data".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(buf: &[u8], flags: i64) -> String {
        String::from_utf8(identify(&builtin(), buf, flags, false).unwrap()).unwrap()
    }

    #[test]
    fn text_classes() {
        assert_eq!(d(b"", NONE), "empty");
        assert_eq!(d(b"a", NONE), "very short file (no magic)");
        assert_eq!(
            d(b"a\r\nb\n", NONE),
            "ASCII text, with CRLF, LF line terminators"
        );
        assert_eq!(d(b"caf\xc3\xa9\n", MIME), "text/plain; charset=utf-8");
        assert_eq!(d(b"caf\xe9\n", NONE), "ISO-8859 text");
        assert_eq!(
            d(b"abc\x01\n", MIME),
            "application/octet-stream; charset=binary"
        );
        assert_eq!(d(b"{\"a\":1}", MIME_TYPE), "application/json");
        assert_eq!(d(b"<?php\n", MIME), "text/x-php; charset=us-ascii");
    }

    #[test]
    fn custom_magic() {
        // `default` matches when no sibling at its level did (magic(5)).
        let src: &[u8] = b"0 string AB AB\n>2 byte 1 one\n>2 default x other\n";
        let (db, w) = load(&[src]);
        assert!(w.is_empty());
        let db = db.unwrap();
        let r = |b: &[u8]| String::from_utf8(identify(&db, b, NONE, false).unwrap()).unwrap();
        assert_eq!(r(b"AB\x01\x00"), "AB one");
        assert_eq!(r(b"AB\x02\x00"), "AB other");
        let bad: &[u8] = b"0 foo x Hello\n";
        let (db, w) = load(&[bad]);
        assert!(db.is_none());
        assert_eq!(w, vec!["type `foo x Hello' invalid".to_string()]);
    }
}
