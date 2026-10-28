//! ext/zip (PHP manual: Zip): the `ZipArchive` class, the `zip://`
//! stream wrapper and the deprecated `zip_*` functions, on the
//! goro-ext-zip crate. Changes are kept in memory and written at
//! `close()` (or when the object goes away), as libzip does. Behaviour
//! from the manual and the expectations of ext/zip/tests/*.phpt.

use std::borrow::Cow;
use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;

use goro_ext_zip::text::Decode;
use goro_ext_zip::{self as zf, Error as ZErr, cm, em, er};
use goro_runtime::object::WeakRes;
use goro_runtime::{Key, ObjRef, PhpArray, PhpStr, ResRef, Value};

use super::file::{Backing, Stream, new_stream_res, open_stream};
use super::streams::{NativeStream, NativeWrapper, register_wrapper};
use crate::class::NativeHooks;
use crate::func::{Args, NativeFn};
use crate::registry::Module;
use crate::vm::{R, Unwind, Vm};

// ------------------------------------------------------------------ class

const CLASS: &str = r#"<?php
class ZipArchive implements Countable {
    const int CREATE = 1;
    const int EXCL = 2;
    const int CHECKCONS = 4;
    const int OVERWRITE = 8;
    const int RDONLY = 16;
    const int FL_NOCASE = 1;
    const int FL_NODIR = 2;
    const int FL_COMPRESSED = 4;
    const int FL_UNCHANGED = 8;
    const int FL_RECOMPRESS = 16;
    const int FL_ENCRYPTED = 32;
    const int FL_OVERWRITE = 8192;
    const int FL_LOCAL = 256;
    const int FL_CENTRAL = 512;
    const int FL_ENC_GUESS = 0;
    const int FL_ENC_RAW = 64;
    const int FL_ENC_STRICT = 128;
    const int FL_ENC_UTF_8 = 2048;
    const int FL_ENC_CP437 = 4096;
    const int FL_OPEN_FILE_NOW = 1073741824;
    const int CM_DEFAULT = -1;
    const int CM_STORE = 0;
    const int CM_SHRINK = 1;
    const int CM_REDUCE_1 = 2;
    const int CM_REDUCE_2 = 3;
    const int CM_REDUCE_3 = 4;
    const int CM_REDUCE_4 = 5;
    const int CM_IMPLODE = 6;
    const int CM_DEFLATE = 8;
    const int CM_DEFLATE64 = 9;
    const int CM_PKWARE_IMPLODE = 10;
    const int CM_BZIP2 = 12;
    const int CM_LZMA = 14;
    const int CM_LZMA2 = 33;
    const int CM_ZSTD = 93;
    const int CM_XZ = 95;
    const int CM_TERSE = 18;
    const int CM_LZ77 = 19;
    const int CM_WAVPACK = 97;
    const int CM_PPMD = 98;
    const int ER_OK = 0;
    const int ER_MULTIDISK = 1;
    const int ER_RENAME = 2;
    const int ER_CLOSE = 3;
    const int ER_SEEK = 4;
    const int ER_READ = 5;
    const int ER_WRITE = 6;
    const int ER_CRC = 7;
    const int ER_ZIPCLOSED = 8;
    const int ER_NOENT = 9;
    const int ER_EXISTS = 10;
    const int ER_OPEN = 11;
    const int ER_TMPOPEN = 12;
    const int ER_ZLIB = 13;
    const int ER_MEMORY = 14;
    const int ER_CHANGED = 15;
    const int ER_COMPNOTSUPP = 16;
    const int ER_EOF = 17;
    const int ER_INVAL = 18;
    const int ER_NOZIP = 19;
    const int ER_INTERNAL = 20;
    const int ER_INCONS = 21;
    const int ER_REMOVE = 22;
    const int ER_DELETED = 23;
    const int ER_ENCRNOTSUPP = 24;
    const int ER_RDONLY = 25;
    const int ER_NOPASSWD = 26;
    const int ER_WRONGPASSWD = 27;
    const int ER_OPNOTSUPP = 28;
    const int ER_INUSE = 29;
    const int ER_TELL = 30;
    const int ER_COMPRESSED_DATA = 31;
    const int ER_CANCELLED = 32;
    const int ER_DATA_LENGTH = 33;
    const int ER_NOT_ALLOWED = 34;
    const int ER_TRUNCATED_ZIP = 35;
    const int AFL_RDONLY = 2;
    const int AFL_IS_TORRENTZIP = 4;
    const int AFL_WANT_TORRENTZIP = 8;
    const int AFL_CREATE_OR_KEEP_FILE_FOR_EMPTY_ARCHIVE = 16;
    const int OPSYS_DOS = 0;
    const int OPSYS_AMIGA = 1;
    const int OPSYS_OPENVMS = 2;
    const int OPSYS_UNIX = 3;
    const int OPSYS_VM_CMS = 4;
    const int OPSYS_ATARI_ST = 5;
    const int OPSYS_OS_2 = 6;
    const int OPSYS_MACINTOSH = 7;
    const int OPSYS_Z_SYSTEM = 8;
    const int OPSYS_CPM = 9;
    const int OPSYS_WINDOWS_NTFS = 10;
    const int OPSYS_MVS = 11;
    const int OPSYS_VSE = 12;
    const int OPSYS_ACORN_RISC = 13;
    const int OPSYS_VFAT = 14;
    const int OPSYS_ALTERNATE_MVS = 15;
    const int OPSYS_BEOS = 16;
    const int OPSYS_TANDEM = 17;
    const int OPSYS_OS_400 = 18;
    const int OPSYS_OS_X = 19;
    const int OPSYS_DEFAULT = 3;
    const int EM_NONE = 0;
    const int EM_TRAD_PKWARE = 1;
    const int EM_AES_128 = 257;
    const int EM_AES_192 = 258;
    const int EM_AES_256 = 259;
    const int EM_UNKNOWN = 65535;
    const string LIBZIP_VERSION = "1.11.4";
    const int LENGTH_TO_END = 0;
    const int LENGTH_UNCHECKED = -2;
    public int $lastId { get {} set {} }
    public int $status { get {} set {} }
    public int $statusSys { get {} set {} }
    public int $numFiles { get {} set {} }
    public string $filename { get {} set {} }
    public string $comment { get {} set {} }
    public function open(string $filename, int $flags = 0): bool|int {}
    public function setPassword(#[\SensitiveParameter] string $password): bool {}
    public function close(): bool {}
    public function count(): int {}
    public function getStatusString(): string {}
    public function clearError(): void {}
    public function addEmptyDir(string $dirname, int $flags = 0): bool {}
    public function addFromString(string $name, string $content, int $flags = ZipArchive::FL_OVERWRITE): bool {}
    public function addFile(string $filepath, string $entryname = "", int $start = 0, int $length = ZipArchive::LENGTH_TO_END, int $flags = ZipArchive::FL_OVERWRITE): bool {}
    public function replaceFile(string $filepath, int $index, int $start = 0, int $length = ZipArchive::LENGTH_TO_END, int $flags = 0): bool {}
    public function addGlob(string $pattern, int $flags = 0, array $options = []): array|false {}
    public function addPattern(string $pattern, string $path = ".", array $options = []): array|false {}
    public function renameIndex(int $index, string $new_name): bool {}
    public function renameName(string $name, string $new_name): bool {}
    public function setArchiveComment(string $comment): bool {}
    public function getArchiveComment(int $flags = 0): string|false {}
    public function setArchiveFlag(int $flag, int $value): bool {}
    public function getArchiveFlag(int $flag, int $flags = 0): int {}
    public function setCommentIndex(int $index, string $comment): bool {}
    public function setCommentName(string $name, string $comment): bool {}
    public function setMtimeIndex(int $index, int $timestamp, int $flags = 0): bool {}
    public function setMtimeName(string $name, int $timestamp, int $flags = 0): bool {}
    public function getCommentIndex(int $index, int $flags = 0): string|false {}
    public function getCommentName(string $name, int $flags = 0): string|false {}
    public function deleteIndex(int $index): bool {}
    public function deleteName(string $name): bool {}
    public function statName(string $name, int $flags = 0): array|false {}
    public function statIndex(int $index, int $flags = 0): array|false {}
    public function locateName(string $name, int $flags = 0): int|false {}
    public function getNameIndex(int $index, int $flags = 0): string|false {}
    public function unchangeArchive(): bool {}
    public function unchangeAll(): bool {}
    public function unchangeIndex(int $index): bool {}
    public function unchangeName(string $name): bool {}
    public function extractTo(string $pathto, array|string|null $files = null): bool {}
    public function getFromName(string $name, int $len = 0, int $flags = 0): string|false {}
    public function getFromIndex(int $index, int $len = 0, int $flags = 0): string|false {}
    public function getStreamIndex(int $index, int $flags = 0) {}
    public function getStreamName(string $name, int $flags = 0) {}
    public function getStream(string $name) {}
    public function setExternalAttributesName(string $name, int $opsys, int $attr, int $flags = 0): bool {}
    public function setExternalAttributesIndex(int $index, int $opsys, int $attr, int $flags = 0): bool {}
    public function getExternalAttributesName(string $name, &$opsys, &$attr, int $flags = 0): bool {}
    public function getExternalAttributesIndex(int $index, &$opsys, &$attr, int $flags = 0): bool {}
    public function setCompressionName(string $name, int $method, int $compflags = 0): bool {}
    public function setCompressionIndex(int $index, int $method, int $compflags = 0): bool {}
    public function setEncryptionName(string $name, int $method, ?string $password = null): bool {}
    public function setEncryptionIndex(int $index, int $method, ?string $password = null): bool {}
    public function registerProgressCallback(float $rate, callable $callback): bool {}
    public function registerCancelCallback(callable $callback): bool {}
    public static function isCompressionMethodSupported(int $method, bool $enc = true): bool {}
    public static function isEncryptionMethodSupported(int $method, bool $enc = true): bool {}
}"#;

static METHODS: &[(&str, NativeFn)] = &[
    ("ziparchive::$", prop),
    ("ziparchive::open", m_open),
    ("ziparchive::setpassword", m_set_password),
    ("ziparchive::close", m_close),
    ("ziparchive::count", m_count),
    ("ziparchive::getstatusstring", m_get_status_string),
    ("ziparchive::clearerror", m_clear_error),
    ("ziparchive::addemptydir", m_add_empty_dir),
    ("ziparchive::addfromstring", m_add_from_string),
    ("ziparchive::addfile", m_add_file),
    ("ziparchive::replacefile", m_replace_file),
    ("ziparchive::addglob", m_add_glob),
    ("ziparchive::addpattern", m_add_pattern),
    ("ziparchive::renameindex", m_rename_index),
    ("ziparchive::renamename", m_rename_name),
    ("ziparchive::setarchivecomment", m_set_archive_comment),
    ("ziparchive::getarchivecomment", m_get_archive_comment),
    ("ziparchive::setarchiveflag", m_set_archive_flag),
    ("ziparchive::getarchiveflag", m_get_archive_flag),
    ("ziparchive::setcommentindex", m_set_comment_index),
    ("ziparchive::setcommentname", m_set_comment_name),
    ("ziparchive::setmtimeindex", m_set_mtime_index),
    ("ziparchive::setmtimename", m_set_mtime_name),
    ("ziparchive::getcommentindex", m_get_comment_index),
    ("ziparchive::getcommentname", m_get_comment_name),
    ("ziparchive::deleteindex", m_delete_index),
    ("ziparchive::deletename", m_delete_name),
    ("ziparchive::statname", m_stat_name),
    ("ziparchive::statindex", m_stat_index),
    ("ziparchive::locatename", m_locate_name),
    ("ziparchive::getnameindex", m_get_name_index),
    ("ziparchive::unchangearchive", m_unchange_archive),
    ("ziparchive::unchangeall", m_unchange_all),
    ("ziparchive::unchangeindex", m_unchange_index),
    ("ziparchive::unchangename", m_unchange_name),
    ("ziparchive::extractto", m_extract_to),
    ("ziparchive::getfromname", m_get_from_name),
    ("ziparchive::getfromindex", m_get_from_index),
    ("ziparchive::getstreamindex", m_get_stream_index),
    ("ziparchive::getstreamname", m_get_stream_name),
    ("ziparchive::getstream", m_get_stream),
    ("ziparchive::setexternalattributesname", m_set_ext_attr_name),
    (
        "ziparchive::setexternalattributesindex",
        m_set_ext_attr_index,
    ),
    ("ziparchive::getexternalattributesname", m_get_ext_attr_name),
    (
        "ziparchive::getexternalattributesindex",
        m_get_ext_attr_index,
    ),
    ("ziparchive::setcompressionname", m_set_compression_name),
    ("ziparchive::setcompressionindex", m_set_compression_index),
    ("ziparchive::setencryptionname", m_set_encryption_name),
    ("ziparchive::setencryptionindex", m_set_encryption_index),
    ("ziparchive::registerprogresscallback", m_register_progress),
    ("ziparchive::registercancelcallback", m_register_cancel),
    (
        "ziparchive::iscompressionmethodsupported",
        m_is_cm_supported,
    ),
    ("ziparchive::isencryptionmethodsupported", m_is_em_supported),
];

pub static MODULE: Module = Module {
    name: "zip",
    functions: &[
        ("function zip_open(string $filename)", zip_open),
        ("function zip_close($zip): void", zip_close),
        ("function zip_read($zip)", zip_read),
        (
            "function zip_entry_open($zip_dp, $zip_entry, string $mode = \"rb\"): bool",
            zip_entry_open,
        ),
        (
            "function zip_entry_close($zip_entry): bool",
            zip_entry_close,
        ),
        (
            "function zip_entry_read($zip_entry, int $len = 1024): string|false",
            zip_entry_read,
        ),
        (
            "function zip_entry_name($zip_entry): string|false",
            zip_entry_name,
        ),
        (
            "function zip_entry_compressedsize($zip_entry): int|false",
            zip_entry_compressedsize,
        ),
        (
            "function zip_entry_filesize($zip_entry): int|false",
            zip_entry_filesize,
        ),
        (
            "function zip_entry_compressionmethod($zip_entry): string|false",
            zip_entry_compressionmethod,
        ),
    ],
    classes: &[("ziparchive", CLASS)],
    methods: METHODS,
    hooks: &[("ziparchive", hooks)],
    init: Some(init),
    ..Module::EMPTY
};

static WRAPPER: NativeWrapper = NativeWrapper::new(wrapper_open);

fn init(vm: &mut Vm) {
    register_wrapper(vm, "zip", &WRAPPER);
}

// Flags (PHP manual: ZipArchive constants).
const CREATE: i64 = 1;
const EXCL: i64 = 2;
const CHECKCONS: i64 = 4;
const OVERWRITE: i64 = 8;
const RDONLY: i64 = 16;
const FL_NOCASE: i64 = 1;
const FL_NODIR: i64 = 2;
const FL_COMPRESSED: i64 = 4;
const FL_UNCHANGED: i64 = 8;
const FL_ENC_RAW: i64 = 64;
const FL_ENC_STRICT: i64 = 128;
const FL_OVERWRITE: i64 = 8192;
const FL_ENC_UTF_8: i64 = 2048;
const FL_ENC_CP437: i64 = 4096;
const FL_OPEN_FILE_NOW: i64 = 1 << 30;
const AFL_RDONLY: i64 = 2;
const AFL_IS_TORRENTZIP: i64 = 4;
const AFL_WANT_TORRENTZIP: i64 = 8;
const AFL_KEEP_EMPTY: i64 = 16;
const LENGTH_UNCHECKED: i64 = -2;
const MAXPATHLEN: usize = 4096;

// ------------------------------------------------------------------ state

/// New data of an entry.
#[derive(Clone)]
enum Src {
    Bytes(Rc<Vec<u8>>),
    /// A file read at close (libzip opens added files then).
    File {
        path: PathBuf,
        start: u64,
        len: i64,
    },
    Dir,
}

/// One index of the archive: an original entry, possibly changed, or a
/// new one.
#[derive(Clone)]
struct Slot {
    orig: Option<usize>,
    deleted: bool,
    name: Vec<u8>,
    utf8: bool,
    comment: Vec<u8>,
    src: Option<Src>,
    /// `(method, level)` set by setCompression*.
    method: Option<(i32, i32)>,
    /// `(method, password)` set by setEncryption*.
    enc: Option<(u16, Option<Vec<u8>>)>,
    mtime: Option<i64>,
    /// `(opsys, attributes)`.
    attr: Option<(u8, u32)>,
}

struct Open {
    path: PathBuf,
    filename: Vec<u8>,
    data: Rc<Vec<u8>>,
    arc: Rc<zf::Archive>,
    slots: Vec<Slot>,
    comment: Vec<u8>,
    comment_changed: bool,
    /// Opened read-only (RDONLY).
    opened_rdonly: bool,
    rdonly: bool,
    keep_empty: bool,
    want_torrent: bool,
    is_torrent: bool,
    /// The file is on disk.
    on_disk: bool,
    /// Opened with OVERWRITE over existing data.
    truncated: bool,
    password: Option<Vec<u8>>,
    err: ZErr,
    last_id: i64,
    progress: Option<(f64, Value)>,
    cancel: Option<Value>,
    /// Cleared when the archive is closed (streams of its entries fail).
    alive: Rc<Cell<bool>>,
}

#[derive(Default)]
struct ZipObj {
    open: Option<Box<Open>>,
    /// Status after close.
    err: ZErr,
}

impl goro_runtime::NativeData for ZipObj {}

fn hooks() -> NativeHooks {
    NativeHooks {
        create: Some(|_vm, o| {
            *o.obj().native.borrow_mut() = Some(Box::new(ZipObj::default()));
        }),
        clone: Some(|_, _, to| {
            *to.obj().native.borrow_mut() = Some(Box::new(ZipObj::default()));
        }),
        // var_dump/print_r show the virtual properties first, then the
        // subclass' own (test bug38943_2.phpt).
        debug_props: Some(|vm, o| {
            let mut a = PhpArray::new();
            for n in PROPS {
                a.insert(Key::from_bytes(n.as_bytes()), prop_value(o, n));
            }
            let plain = vm.plain_props_array(o, true);
            for (k, v) in plain.iter() {
                a.insert(k.to_owned(), v.clone());
            }
            Some(a)
        }),
        destroy: Some(|vm, o| {
            if let Some(mut op) = take_open(o) {
                let _ = commit(vm, &mut op);
                op.alive.set(false);
                let err = op.err;
                with_obj(o, |z| z.err = err);
            }
        }),
        ..Default::default()
    }
}

const PROPS: [&str; 6] = [
    "lastId",
    "status",
    "statusSys",
    "numFiles",
    "filename",
    "comment",
];

fn with_obj<T>(o: &ObjRef, f: impl FnOnce(&mut ZipObj) -> T) -> Option<T> {
    let mut n = o.obj().native.borrow_mut();
    let any: &mut dyn std::any::Any = &mut **n.as_mut()?;
    any.downcast_mut::<ZipObj>().map(f)
}

fn take_open(o: &ObjRef) -> Option<Box<Open>> {
    with_obj(o, |z| z.open.take()).flatten()
}

fn put_open(o: &ObjRef, op: Box<Open>) {
    with_obj(o, |z| z.open = Some(op));
}

fn prop_value(o: &ObjRef, name: &str) -> Value {
    with_obj(o, |z| match (&z.open, name) {
        (Some(op), "lastId") => Value::Int(op.last_id),
        (Some(op), "status") => Value::Int(op.err.code as i64),
        (Some(op), "statusSys") => Value::Int(op.err.sys as i64),
        (Some(op), "numFiles") => Value::Int(op.slots.len() as i64),
        (Some(op), "filename") => Value::from(op.filename.as_slice()),
        (Some(op), "comment") => Value::from(op.comment.as_slice()),
        (None, "lastId") => Value::Int(-1),
        (None, "status") => Value::Int(z.err.code as i64),
        (None, "statusSys") => Value::Int(z.err.sys as i64),
        (None, "numFiles") => Value::Int(0),
        _ => Value::from(""),
    })
    .unwrap_or(Value::Null)
}

/// The hooks of the virtual properties (`ZipArchive::$name::get`/`set`).
fn prop(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let fname = a.func.name.to_string_lossy();
    let name = fname
        .split('$')
        .nth(1)
        .and_then(|s| s.split("::").next())
        .unwrap_or("")
        .to_string();
    // The properties are read-only (test oo_readonly_properties.phpt).
    if fname.ends_with("set") {
        return _vm.throw(
            "Error",
            &format!("Cannot write read-only property ZipArchive::${name}"),
        );
    }
    Ok(prop_value(a.this_obj(), &name))
}

/// Runs `f` on the open archive (taken out of the object meanwhile, so
/// user code it calls can't alias it), after clearing the status as
/// each call does.
fn with_zip<T>(vm: &mut Vm, a: &Args, f: impl FnOnce(&mut Vm, &mut Open) -> R<T>) -> R<T> {
    let o = a.this_obj().clone();
    let Some(mut op) = take_open(&o) else {
        return vm.throw("ValueError", "Invalid or uninitialized Zip object");
    };
    op.err = ZErr::default();
    let r = f(vm, &mut op);
    // a method may have reopened or closed it: keep what's there
    let empty = with_obj(&o, |z| z.open.is_none()).unwrap_or(false);
    if empty {
        put_open(&o, op);
    }
    r
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// The name of the native function running (for stream diagnostics).
fn cur_fname(vm: &Vm) -> String {
    vm.ex
        .native_calls
        .last()
        .map(|nc| nc.func.display_name())
        .unwrap_or_default()
}

fn decode_mode(flags: i64) -> Decode {
    if flags & FL_ENC_RAW != 0 {
        Decode::Raw
    } else if flags & FL_ENC_STRICT != 0 {
        Decode::Strict
    } else {
        Decode::Guess
    }
}

impl Open {
    fn new(path: PathBuf, filename: Vec<u8>) -> Open {
        Open {
            path,
            filename,
            data: Rc::new(Vec::new()),
            arc: Rc::new(zf::Archive::default()),
            slots: Vec::new(),
            comment: Vec::new(),
            comment_changed: false,
            opened_rdonly: false,
            rdonly: false,
            keep_empty: false,
            want_torrent: false,
            is_torrent: false,
            on_disk: false,
            truncated: false,
            password: None,
            err: ZErr::default(),
            last_id: -1,
            progress: None,
            cancel: None,
            alive: Rc::new(Cell::new(true)),
        }
    }

    fn orig(&self, i: usize) -> Option<&zf::Entry> {
        self.slots.get(i)?.orig.map(|o| &self.arc.entries[o])
    }

    /// The index of `name` per the FL_NOCASE/FL_NODIR/FL_UNCHANGED flags.
    fn locate(&self, name: &[u8], flags: i64) -> Option<usize> {
        let unchanged = flags & FL_UNCHANGED != 0;
        let eq = |a: &[u8], b: &[u8]| {
            if flags & FL_NOCASE != 0 {
                a.eq_ignore_ascii_case(b)
            } else {
                a == b
            }
        };
        for (i, s) in self.slots.iter().enumerate() {
            let n: &[u8] = if unchanged {
                match s.orig {
                    Some(o) => &self.arc.entries[o].name,
                    None => continue,
                }
            } else {
                if s.deleted {
                    continue;
                }
                &s.name
            };
            let n = if flags & FL_NODIR != 0 {
                match n.iter().rposition(|&c| c == b'/') {
                    Some(p) => &n[p + 1..],
                    None => n,
                }
            } else {
                n
            };
            if eq(n, name) {
                return Some(i);
            }
        }
        None
    }

    fn slot_changed(&self, s: &Slot) -> bool {
        match s.orig {
            None => true,
            Some(o) => {
                let e = &self.arc.entries[o];
                s.deleted
                    || s.src.is_some()
                    || s.method.is_some()
                    || s.enc.is_some()
                    || s.mtime.is_some()
                    || s.attr.is_some()
                    || s.name != e.name
                    || s.comment != e.comment
            }
        }
    }

    fn changed(&self) -> bool {
        self.truncated
            || self.comment_changed
            || (self.want_torrent && !self.is_torrent)
            || self.slots.iter().any(|s| self.slot_changed(s))
    }

    /// Adds or (with FL_OVERWRITE) replaces `name`.
    fn add(&mut self, name: Vec<u8>, src: Src, flags: i64, mtime: i64) -> bool {
        if self.rdonly {
            self.err = ZErr::new(er::RDONLY);
            self.last_id = -1;
            return false;
        }
        let utf8 = if flags & FL_ENC_CP437 != 0 {
            false
        } else if flags & FL_ENC_UTF_8 != 0 {
            !name.is_ascii()
        } else {
            zf::text::needs_utf8_flag(&name)
        };
        if let Some(i) = self.locate(&name, 0) {
            if flags & FL_OVERWRITE == 0 {
                self.err = ZErr::new(er::EXISTS);
                self.last_id = -1;
                return false;
            }
            let s = &mut self.slots[i];
            s.src = Some(src);
            s.mtime = Some(mtime);
            s.method = None;
            self.last_id = i as i64;
            return true;
        }
        self.slots.push(Slot {
            orig: None,
            deleted: false,
            name,
            utf8,
            comment: Vec::new(),
            src: Some(src),
            method: None,
            enc: None,
            mtime: Some(mtime),
            attr: None,
        });
        self.last_id = self.slots.len() as i64 - 1;
        true
    }

    /// The current (or, with `unchanged`, original) data of slot `i`.
    fn read_data(&self, i: usize, unchanged: bool, compressed: bool) -> Result<Vec<u8>, ZErr> {
        let s = &self.slots[i];
        if !unchanged && let Some(src) = &s.src {
            return read_src(src);
        }
        let Some(o) = s.orig else {
            return Err(ZErr::new(er::CHANGED));
        };
        if compressed {
            return self.arc.raw(&self.data, o).map(|r| r.to_vec());
        }
        self.arc.read(&self.data, o, self.password.as_deref())
    }
}

fn read_src(src: &Src) -> Result<Vec<u8>, ZErr> {
    match src {
        Src::Bytes(b) => Ok(b.to_vec()),
        Src::Dir => Ok(Vec::new()),
        Src::File { path, start, len } => {
            use std::io::{Read, Seek, SeekFrom};
            let mut f = std::fs::File::open(path).map_err(|e| ZErr::io(er::OPEN, &e))?;
            if *start > 0 {
                f.seek(SeekFrom::Start(*start))
                    .map_err(|e| ZErr::io(er::SEEK, &e))?;
            }
            let mut out = Vec::new();
            if *len > 0 {
                f.take(*len as u64)
                    .read_to_end(&mut out)
                    .map_err(|e| ZErr::io(er::READ, &e))?;
            } else {
                f.read_to_end(&mut out)
                    .map_err(|e| ZErr::io(er::READ, &e))?;
            }
            Ok(out)
        }
    }
}

/// Resolves a file name argument like PHP's file functions.
fn resolve(vm: &Vm, p: &[u8]) -> PathBuf {
    vm.fs_path(p)
}

fn path_bytes(p: &std::path::Path) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;
    p.as_os_str().as_bytes().to_vec()
}

// ------------------------------------------------------------------ open/close

/// Opens `filename` with `flags`: the archive, or the libzip error code.
fn open_archive(vm: &mut Vm, filename: &[u8], flags: i64) -> R<Result<Open, ZErr>> {
    let path = resolve(vm, filename);
    let mut op = Open::new(path.clone(), path_bytes(&path));
    let meta = std::fs::metadata(&path);
    let exists = meta.is_ok();
    if exists && flags & EXCL != 0 {
        return Ok(Err(ZErr::new(er::EXISTS)));
    }
    if !exists {
        if flags & CREATE == 0 {
            return Ok(Err(ZErr::new(er::NOENT)));
        }
        op.rdonly = flags & RDONLY != 0;
        op.opened_rdonly = op.rdonly;
        return Ok(Ok(op));
    }
    op.on_disk = true;
    op.rdonly = flags & RDONLY != 0;
    op.opened_rdonly = op.rdonly;
    let data = match std::fs::read(&path) {
        Ok(d) => d,
        Err(e) => return Ok(Err(ZErr::io(er::READ, &e))),
    };
    if flags & OVERWRITE != 0 {
        // an existing file, even empty, is replaced or removed (test bug_gh8781.phpt)
        op.truncated = true;
        return Ok(Ok(op));
    }
    if data.is_empty() {
        vm.warn_deprecated_empty()?;
        return Ok(Ok(op));
    }
    let arc = match zf::parse(
        &data,
        zf::ReadOptions {
            check_consistency: flags & CHECKCONS != 0,
        },
    ) {
        Ok(a) => a,
        Err(e) => return Ok(Err(e)),
    };
    op.is_torrent = arc.is_torrentzip(&data);
    op.comment = arc.comment.clone();
    op.slots = arc
        .entries
        .iter()
        .enumerate()
        .map(|(i, e)| Slot {
            orig: Some(i),
            deleted: false,
            name: e.name.clone(),
            utf8: e.utf8(),
            comment: e.comment.clone(),
            src: None,
            method: None,
            enc: None,
            mtime: None,
            attr: None,
        })
        .collect();
    op.arc = Rc::new(arc);
    op.data = Rc::new(data);
    Ok(Ok(op))
}

trait DeprecatedEmpty {
    fn warn_deprecated_empty(&mut self) -> R<()>;
}

impl DeprecatedEmpty for Vm {
    /// PHP 8.5 deprecates empty files as archives (test bug53885.phpt).
    fn warn_deprecated_empty(&mut self) -> R<()> {
        let f = cur_fname(self);
        self.deprecated(&format!(
            "{f}(): Using empty file as ZipArchive is deprecated"
        ))
    }
}

/// What a callback run during close asked for.
enum CbOutcome {
    Go,
    Cancel,
}

/// Calls the cancel callback: a non-zero int cancels; anything else but
/// an int (or a bool) is a TypeError, which cancels too (tests
/// oo_cancel*.phpt, gh18439.phpt).
fn call_cancel(vm: &mut Vm, op: &Open, pending: &mut Option<Unwind>) -> R<CbOutcome> {
    let Some(cb) = op.cancel.clone() else {
        return Ok(CbOutcome::Go);
    };
    if pending.is_some() {
        return Ok(CbOutcome::Cancel);
    }
    match vm.call_value(&cb, Vec::new()) {
        Ok(v) => match v.deref() {
            Value::Int(n) => Ok(if n != 0 {
                CbOutcome::Cancel
            } else {
                CbOutcome::Go
            }),
            Value::Bool(b) => Ok(if b { CbOutcome::Cancel } else { CbOutcome::Go }),
            other => {
                let t = Vm::type_name_given(&other);
                if let Err(e) = vm.throw::<()>(
                    "TypeError",
                    &format!(
                        "Return value of callback provided to ZipArchive::registerCancelCallback() must be of type int, {t} returned"
                    ),
                ) {
                    *pending = Some(e);
                }
                Ok(CbOutcome::Cancel)
            }
        },
        Err(e @ Unwind::Throw(_)) => {
            *pending = Some(e);
            Ok(CbOutcome::Cancel)
        }
        Err(e) => Err(e),
    }
}

fn call_progress(vm: &mut Vm, op: &Open, p: f64, pending: &mut Option<Unwind>) -> R<()> {
    let Some((_, cb)) = op.progress.clone() else {
        return Ok(());
    };
    if pending.is_some() {
        return Ok(());
    }
    match vm.call_value(&cb, vec![Value::Float(p)]) {
        Ok(_) => Ok(()),
        Err(e @ Unwind::Throw(_)) => {
            *pending = Some(e);
            Ok(())
        }
        Err(e) => Err(e),
    }
}

/// Builds the entry written for slot `i`.
fn write_entry<'a>(op: &'a Open, i: usize) -> Result<zf::WriteEntry<'a>, ZErr> {
    let s = &op.slots[i];
    let orig = s.orig.map(|o| &op.arc.entries[o]);
    let mtime = s.mtime;
    let (dos_time, dos_date) = match (mtime, orig) {
        (Some(t), _) => zf::dostime::unix_to_dos(t),
        (None, Some(e)) => (e.dos_time, e.dos_date),
        (None, None) => zf::dostime::unix_to_dos(now()),
    };
    let is_dir =
        matches!(s.src, Some(Src::Dir)) || (s.src.is_none() && s.name.last() == Some(&b'/'));
    let (made_by, external) = match (s.attr, orig) {
        (Some((os, at)), Some(e)) => (((os as u16) << 8) | (e.version_made_by & 0xff), at),
        (Some((os, at)), None) => (((os as u16) << 8) | 63, at),
        (None, Some(e)) if s.src.is_none() => (e.version_made_by, e.external_attr),
        (None, _) => {
            let attr = if is_dir {
                0o040777u32 << 16 | 0x10
            } else {
                0o100666u32 << 16
            };
            ((3 << 8) | 63, attr)
        }
    };
    let (lx, cx) = match orig {
        Some(e) if s.src.is_none() => (e.local_extra.clone(), e.extra.clone()),
        _ => (Vec::new(), Vec::new()),
    };
    let name_same = orig.is_some_and(|e| e.name == s.name);
    let utf8 = if name_same {
        orig.is_some_and(|e| e.utf8())
    } else {
        s.utf8
    };
    // the data: copied as stored when unchanged
    let payload = match (orig, &s.src) {
        (Some(e), None) if s.method.is_none() && s.enc.is_none() => {
            let raw = op.arc.raw(&op.data, s.orig.unwrap_or(0))?;
            zf::Payload::Raw {
                data: Cow::Borrowed(raw),
                method: e.method,
                flags: e.flags,
                crc: e.crc,
                size: e.size,
                aes: e.aes,
            }
        }
        _ => {
            let data = match &s.src {
                Some(src) => read_src(src)?,
                None => op
                    .arc
                    .read(&op.data, s.orig.unwrap_or(0), op.password.as_deref())?,
            };
            let (method, level, fallback) = match s.method {
                Some((m, l)) if m >= 0 => (m as u16, l, false),
                Some((_, l)) => (cm::DEFLATE, l, true),
                None => match (orig, &s.src) {
                    (Some(e), None) => (e.real_method(), -1, false),
                    _ if is_dir => (cm::STORE, -1, false),
                    _ => (cm::DEFLATE, -1, true),
                },
            };
            let method = if is_dir && data.is_empty() {
                cm::STORE
            } else {
                method
            };
            let encryption = match &s.enc {
                Some((m, pw)) => {
                    let pw = pw
                        .clone()
                        .or_else(|| op.password.clone())
                        .unwrap_or_default();
                    match *m {
                        em::TRAD_PKWARE => zf::Encryption::Pkware(pw),
                        em::AES_128 => zf::Encryption::Aes(zf::crypto::AesStrength::Aes128, pw),
                        em::AES_192 => zf::Encryption::Aes(zf::crypto::AesStrength::Aes192, pw),
                        em::AES_256 => zf::Encryption::Aes(zf::crypto::AesStrength::Aes256, pw),
                        _ => zf::Encryption::None,
                    }
                }
                None => match orig {
                    // re-encoded original keeps its encryption
                    Some(e) if s.src.is_none() && e.is_encrypted() => {
                        let pw = op.password.clone().unwrap_or_default();
                        match (e.encryption_method(), e.aes) {
                            (em::TRAD_PKWARE, _) => zf::Encryption::Pkware(pw),
                            (_, Some(a)) => zf::Encryption::Aes(a.strength, pw),
                            _ => zf::Encryption::None,
                        }
                    }
                    _ => zf::Encryption::None,
                },
            };
            zf::Payload::Plain {
                data: Cow::Owned(data),
                method,
                level,
                encryption,
                store_if_larger: fallback,
            }
        }
    };
    Ok(zf::WriteEntry {
        name: s.name.clone(),
        comment: s.comment.clone(),
        local_extra: lx,
        central_extra: cx,
        dos_time,
        dos_date,
        version_made_by: made_by,
        internal_attr: orig.map(|e| e.internal_attr).unwrap_or(0),
        external_attr: external,
        utf8,
        payload,
    })
}

/// Writes the changes (libzip's zip_close): nothing when unchanged, the
/// file removed when no entry is left (unless
/// AFL_CREATE_OR_KEEP_FILE_FOR_EMPTY_ARCHIVE). The status is left in
/// `op.err`; an exception from a callback comes back after the write.
fn commit(vm: &mut Vm, op: &mut Open) -> R<Option<Unwind>> {
    let mut pending: Option<Unwind> = None;
    if op.rdonly || !op.changed() {
        return Ok(None);
    }
    let live: Vec<usize> = (0..op.slots.len())
        .filter(|&i| !op.slots[i].deleted)
        .collect();
    if live.is_empty() && !op.keep_empty {
        if op.on_disk
            && let Err(e) = std::fs::remove_file(&op.path)
        {
            op.err = ZErr::io(er::REMOVE, &e);
        }
        return Ok(None);
    }
    call_progress(vm, op, 0.0, &mut pending)?;
    if let CbOutcome::Cancel = call_cancel(vm, op, &mut pending)? {
        op.err = ZErr::new(er::CANCELLED);
        return Ok(pending);
    }
    let rate = op.progress.as_ref().map(|p| p.0).unwrap_or(1.0);
    let mut last = 0.0;
    let n = live.len();
    let bytes = if op.want_torrent {
        let mut items = Vec::with_capacity(n);
        for &i in &live {
            let s = &op.slots[i];
            let data = match &s.src {
                Some(src) => read_src(src),
                None => op
                    .arc
                    .read(&op.data, s.orig.unwrap_or(0), op.password.as_deref()),
            };
            match data {
                Ok(d) => items.push((s.name.clone(), d)),
                Err(e) => {
                    op.err = e;
                    return Ok(pending);
                }
            }
        }
        match zf::write::torrentzip(items) {
            Ok(b) => b,
            Err(e) => {
                op.err = e;
                return Ok(pending);
            }
        }
    } else {
        let mut w = zf::ArchiveWriter::new();
        for (k, &i) in live.iter().enumerate() {
            let r = write_entry(op, i).and_then(|e| w.add(e));
            if let Err(e) = r {
                op.err = e;
                return Ok(pending);
            }
            let p = (k + 1) as f64 / n as f64;
            if p < 1.0 && p - last >= rate {
                last = p;
                call_progress(vm, op, p, &mut pending)?;
            }
            if k + 1 < n
                && let CbOutcome::Cancel = call_cancel(vm, op, &mut pending)?
            {
                op.err = ZErr::new(er::CANCELLED);
                return Ok(pending);
            }
        }
        w.finish(&op.comment)
    };
    if let Err(e) = std::fs::write(&op.path, &bytes) {
        op.err = ZErr::io(er::OPEN, &e);
        return Ok(pending);
    }
    call_progress(vm, op, 1.0, &mut pending)?;
    Ok(pending)
}

/// Closes the archive of `o` (if open): writes it and keeps its status.
/// Returns the status and a pending exception.
fn close_obj(vm: &mut Vm, o: &ObjRef) -> R<Option<(ZErr, Option<Unwind>)>> {
    let Some(mut op) = take_open(o) else {
        return Ok(None);
    };
    op.err = ZErr::default();
    let pending = commit(vm, &mut op)?;
    op.alive.set(false);
    let err = op.err;
    with_obj(o, |z| z.err = err);
    Ok(Some((err, pending)))
}

fn m_open(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let f = a.str(0);
    if f.is_empty() {
        return vm.fthrow(a, "ValueError", "Argument #1 ($filename) must not be empty");
    }
    if f.contains(&0) {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #1 ($filename) must not contain any null bytes",
        );
    }
    let o = a.this_obj().clone();
    // an archive already open is closed first
    if let Some((_, Some(p))) = close_obj(vm, &o)? {
        return Err(p);
    }
    match open_archive(vm, &f, a.int_or(1, 0))? {
        Ok(op) => {
            put_open(&o, Box::new(op));
            Ok(Value::Bool(true))
        }
        Err(e) => {
            with_obj(&o, |z| z.err = e);
            Ok(Value::Int(e.code as i64))
        }
    }
}

fn m_close(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    match close_obj(vm, &o)? {
        None => vm.throw("ValueError", "Invalid or uninitialized Zip object"),
        Some((err, pending)) => {
            if err.code != er::OK {
                vm.fwarn(a, &err.message())?;
            }
            if let Some(p) = pending {
                return Err(p);
            }
            Ok(Value::Bool(err.code == er::OK))
        }
    }
}

fn m_set_password(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let pw = a.str(0).to_vec();
    with_zip(vm, a, |_, op| {
        op.password = Some(pw);
        Ok(Value::Bool(true))
    })
}

fn m_count(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(Value::Int(
        with_obj(a.this_obj(), |z| z.open.as_ref().map(|o| o.slots.len()))
            .flatten()
            .unwrap_or(0) as i64,
    ))
}

fn m_get_status_string(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let e = with_obj(a.this_obj(), |z| {
        z.open.as_ref().map(|o| o.err).unwrap_or(z.err)
    })
    .unwrap_or_default();
    Ok(Value::from(e.message().as_str()))
}

fn m_clear_error(vm: &mut Vm, a: &mut Args) -> R<Value> {
    with_zip(vm, a, |_, _| Ok(Value::Null))
}

fn m_register_progress(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let rate = a.float(0);
    let cb = a.val(1);
    with_zip(vm, a, |_, op| {
        op.progress = Some((rate, cb));
        Ok(Value::Bool(true))
    })
}

fn m_register_cancel(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let cb = a.val(0);
    with_zip(vm, a, |_, op| {
        op.cancel = Some(cb);
        Ok(Value::Bool(true))
    })
}

fn m_is_cm_supported(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let m = a.int(0);
    let enc = a.bool_or(1, true);
    let ok = if m == -1 {
        true
    } else if !(0..=0xffff).contains(&m) {
        false
    } else if enc {
        zf::codec::can_compress(m as u16)
    } else {
        zf::codec::can_decompress(m as u16)
    };
    Ok(Value::Bool(ok))
}

fn m_is_em_supported(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(Value::Bool(matches!(a.int(0), 0 | 1 | 257 | 258 | 259)))
}

// ------------------------------------------------------------------ adding

fn file_mtime(m: &std::fs::Metadata) -> i64 {
    m.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or_else(now)
}

fn m_add_empty_dir(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let mut name = a.str(0).to_vec();
    if name.is_empty() {
        return vm.fthrow(a, "ValueError", "Argument #1 ($dirname) must not be empty");
    }
    if name.last() != Some(&b'/') {
        name.push(b'/');
    }
    let flags = a.int_or(1, 0);
    with_zip(vm, a, |_, op| {
        Ok(Value::Bool(op.add(name, Src::Dir, flags, now())))
    })
}

fn m_add_from_string(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let name = a.str(0).to_vec();
    let data = Rc::new(a.str(1).to_vec());
    let flags = a.int_or(2, FL_OVERWRITE);
    with_zip(vm, a, |_, op| {
        Ok(Value::Bool(op.add(name, Src::Bytes(data), flags, now())))
    })
}

/// The source of addFile()/replaceFile(): None (after a warning) when the
/// file is missing.
fn file_src(
    vm: &mut Vm,
    a: &Args,
    path: &[u8],
    start: i64,
    len: i64,
    flags: i64,
) -> R<Option<(Src, i64)>> {
    let p = resolve(vm, path);
    let meta = match std::fs::metadata(&p) {
        Ok(m) => m,
        Err(e) => {
            vm.fwarn(a, &zf::error::strerror(e.raw_os_error().unwrap_or(2)))?;
            return Ok(None);
        }
    };
    let mtime = file_mtime(&meta);
    let len = if len == LENGTH_UNCHECKED { 0 } else { len };
    let src = Src::File {
        path: p,
        start: start as u64,
        len,
    };
    if flags & FL_OPEN_FILE_NOW != 0 {
        return match read_src(&src) {
            Ok(d) => Ok(Some((Src::Bytes(Rc::new(d)), mtime))),
            Err(e) => {
                vm.fwarn(a, &zf::error::strerror(e.sys))?;
                Ok(None)
            }
        };
    }
    Ok(Some((src, mtime)))
}

fn check_range(vm: &mut Vm, a: &Args, start_arg: usize) -> R<Option<(i64, i64)>> {
    let start = a.int_or(start_arg, 0);
    let len = a.int_or(start_arg + 1, 0);
    if start < 0 {
        return vm.fthrow(
            a,
            "ValueError",
            &format!(
                "Argument #{} ($start) must be greater than or equal to 0",
                start_arg + 1
            ),
        );
    }
    if len < 0 && len != LENGTH_UNCHECKED {
        return vm.fthrow(
            a,
            "ValueError",
            &format!(
                "Argument #{} ($length) must be greater than or equal to {}",
                start_arg + 2,
                LENGTH_UNCHECKED
            ),
        );
    }
    Ok(Some((start, len)))
}

fn m_add_file(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let path = a.str(0).to_vec();
    if path.is_empty() {
        return vm.fthrow(a, "ValueError", "Argument #1 ($filepath) must not be empty");
    }
    let mut name = a.str(1).to_vec();
    if name.is_empty() {
        name = path.clone();
    }
    let Some((start, len)) = check_range(vm, a, 2)? else {
        return Ok(Value::Bool(false));
    };
    let flags = a.int_or(4, FL_OVERWRITE);
    // the archive must be open before the file is checked
    if with_obj(a.this_obj(), |z| z.open.is_none()).unwrap_or(true) {
        return vm.throw("ValueError", "Invalid or uninitialized Zip object");
    }
    let Some((src, mtime)) = file_src(vm, a, &path, start, len, flags)? else {
        return Ok(Value::Bool(false));
    };
    with_zip(vm, a, |_, op| {
        Ok(Value::Bool(op.add(name, src, flags, mtime)))
    })
}

fn m_replace_file(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let path = a.str(0).to_vec();
    if path.is_empty() {
        return vm.fthrow(a, "ValueError", "Argument #1 ($filepath) must not be empty");
    }
    let idx = a.int(1);
    let Some((start, len)) = check_range(vm, a, 2)? else {
        return Ok(Value::Bool(false));
    };
    let flags = a.int_or(4, 0);
    let valid = with_obj(a.this_obj(), |z| {
        z.open.as_ref().map(|op| {
            idx >= 0 && (idx as usize) < op.slots.len() && !op.slots[idx as usize].deleted
        })
    })
    .flatten();
    match valid {
        None => return vm.throw("ValueError", "Invalid or uninitialized Zip object"),
        Some(false) => {
            with_obj(a.this_obj(), |z| {
                if let Some(op) = z.open.as_mut() {
                    op.err = ZErr::new(er::INVAL);
                }
            });
            return Ok(Value::Bool(false));
        }
        Some(true) => {}
    }
    let Some((src, mtime)) = file_src(vm, a, &path, start, len, flags)? else {
        return Ok(Value::Bool(false));
    };
    with_zip(vm, a, |_, op| {
        if op.rdonly {
            op.err = ZErr::new(er::RDONLY);
            return Ok(Value::Bool(false));
        }
        let s = &mut op.slots[idx as usize];
        s.src = Some(src);
        s.mtime = Some(mtime);
        Ok(Value::Bool(true))
    })
}

/// addGlob()/addPattern() options (PHP manual: ZipArchive::addGlob).
#[derive(Default)]
struct AddOptions {
    remove_path: Option<Vec<u8>>,
    remove_all_path: bool,
    add_path: Option<Vec<u8>>,
    flags: i64,
    comp: Option<(i64, i64)>,
    enc: Option<(i64, Option<Vec<u8>>)>,
}

fn add_options(vm: &mut Vm, opts: &PhpArray) -> R<AddOptions> {
    let mut o = AddOptions {
        flags: FL_OVERWRITE,
        ..Default::default()
    };
    if let Some(v) = opts.get_bytes(b"remove_path") {
        o.remove_path = Some(vm.to_php_str(&v.deref())?.to_vec());
    }
    if let Some(v) = opts.get_bytes(b"add_path") {
        o.add_path = Some(vm.to_php_str(&v.deref())?.to_vec());
    }
    if let Some(v) = opts.get_bytes(b"remove_all_path") {
        o.remove_all_path = vm.to_bool(&v.deref());
    }
    if let Some(v) = opts.get_bytes(b"flags") {
        o.flags = vm.to_int(&v.deref())?;
    }
    if let Some(v) = opts.get_bytes(b"comp_method") {
        let m = vm.to_int(&v.deref())?;
        let l = match opts.get_bytes(b"comp_flags") {
            Some(v) => vm.to_int(&v.deref())?,
            None => 0,
        };
        o.comp = Some((m, l));
    }
    if let Some(v) = opts.get_bytes(b"enc_method") {
        let m = vm.to_int(&v.deref())?;
        let pw = match opts.get_bytes(b"enc_password") {
            Some(v) => Some(vm.to_php_str(&v.deref())?.to_vec()),
            None => None,
        };
        o.enc = Some((m, pw));
    }
    Ok(o)
}

/// The entry name of `file` per the options; None (after a warning) when
/// too long.
fn entry_name(vm: &mut Vm, a: &Args, file: &[u8], o: &AddOptions) -> R<Option<Vec<u8>>> {
    let mut name: &[u8] = file;
    if o.remove_all_path {
        if let Some(p) = name.iter().rposition(|&c| c == b'/') {
            name = &name[p + 1..];
        }
    } else if let Some(rp) = &o.remove_path
        && !rp.is_empty()
        && name.len() > rp.len()
        && name.starts_with(rp)
    {
        name = &name[rp.len()..];
        if let Some(n) = name.strip_prefix(b"/") {
            name = n;
        }
    }
    let mut out = o.add_path.clone().unwrap_or_default();
    out.extend_from_slice(name);
    if out.len() >= MAXPATHLEN {
        vm.fwarn(
            a,
            &format!(
                "Entry name too long (max: {}, {} given)",
                MAXPATHLEN - 1,
                out.len()
            ),
        )?;
        return Ok(None);
    }
    Ok(Some(out))
}

/// Adds the regular files of `files` per the options: the list of added
/// paths, or false.
fn add_files(vm: &mut Vm, a: &Args, files: Vec<Vec<u8>>, o: &AddOptions) -> R<Value> {
    let mut added = PhpArray::new();
    for f in files {
        let p = resolve(vm, &f);
        let Ok(meta) = std::fs::metadata(&p) else {
            continue;
        };
        if !meta.is_file() {
            continue;
        }
        let Some(name) = entry_name(vm, a, &f, o)? else {
            return Ok(Value::Bool(false));
        };
        let mtime = file_mtime(&meta);
        let ok = with_zip(vm, a, |_, op| {
            if !op.add(
                name,
                Src::File {
                    path: p,
                    start: 0,
                    len: 0,
                },
                o.flags,
                mtime,
            ) {
                return Ok(false);
            }
            let i = op.last_id as usize;
            if let Some((m, l)) = o.comp {
                op.slots[i].method = Some((m as i32, l as i32));
            }
            if let Some((m, pw)) = &o.enc {
                op.slots[i].enc = Some((*m as u16, pw.clone()));
            }
            Ok(true)
        })?;
        if !ok {
            return Ok(Value::Bool(false));
        }
        let _ = added.push(Value::from(f.as_slice()));
    }
    Ok(Value::Array(added))
}

fn m_add_glob(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let pat = a.str(0);
    let flags = a.int_or(1, 0);
    let opts = a.array(2);
    if with_obj(a.this_obj(), |z| z.open.is_none()).unwrap_or(true) {
        return vm.throw("ValueError", "Invalid or uninitialized Zip object");
    }
    let o = add_options(vm, &opts)?;
    let r = vm.call_value(
        &Value::from("glob"),
        vec![Value::Str(pat), Value::Int(flags)],
    )?;
    let Value::Array(list) = r else {
        return Ok(Value::Bool(false));
    };
    let mut files = Vec::new();
    for (_, v) in list.iter() {
        files.push(vm.to_php_str(&v.deref())?.to_vec());
    }
    add_files(vm, a, files, &o)
}

fn m_add_pattern(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let pat = a.str(0);
    let dir = a
        .opt_str(1)
        .map(|s| s.to_vec())
        .unwrap_or_else(|| b".".to_vec());
    let opts = a.array(2);
    if with_obj(a.this_obj(), |z| z.open.is_none()).unwrap_or(true) {
        return vm.throw("ValueError", "Invalid or uninitialized Zip object");
    }
    let o = add_options(vm, &opts)?;
    // the directory's entries (through the stream layer for URLs)
    let mut names: Vec<Vec<u8>> = Vec::new();
    if dir.windows(3).any(|w| w == b"://") {
        let r = vm.call_value(&Value::from("scandir"), vec![Value::from(dir.as_slice())])?;
        if let Value::Array(l) = r {
            for (_, v) in l.iter() {
                let n = vm.to_php_str(&v.deref())?.to_vec();
                if n != b"." && n != b".." {
                    names.push(n);
                }
            }
        }
    } else if let Ok(rd) = std::fs::read_dir(resolve(vm, &dir)) {
        use std::os::unix::ffi::OsStrExt;
        for e in rd.flatten() {
            names.push(e.file_name().as_bytes().to_vec());
        }
    }
    names.sort();
    // nothing to match: the pattern isn't even compiled (test gh18438.phpt)
    if names.is_empty() {
        return Ok(Value::Array(PhpArray::new()));
    }
    let re = match goro_regex::php::compile(&pat) {
        Ok(r) => r,
        Err(e) => {
            vm.fwarn(a, &e.message())?;
            vm.fwarn(a, "Invalid expression")?;
            return Ok(Value::Bool(false));
        }
    };
    let mut files = Vec::new();
    for n in names {
        let mut full = dir.clone();
        if full.last() != Some(&b'/') {
            full.push(b'/');
        }
        full.extend_from_slice(&n);
        if re.is_match(&full).unwrap_or(false) {
            files.push(full);
        }
    }
    add_files(vm, a, files, &o)
}

// ------------------------------------------------------------------ edits

fn not_empty(vm: &mut Vm, a: &Args, i: usize, what: &str) -> R<bool> {
    if a.str(i).is_empty() {
        vm.fthrow::<()>(
            a,
            "ValueError",
            &format!("Argument #{} (${what}) must not be empty", i + 1),
        )?;
    }
    Ok(true)
}

/// Runs `f` on the index named by argument 0 (by name or index as
/// `by_name`); ER_NOENT/ER_INVAL and false otherwise.
fn on_entry(
    vm: &mut Vm,
    a: &Args,
    by_name: bool,
    flags: i64,
    f: impl FnOnce(&mut Vm, &mut Open, usize) -> R<Value>,
) -> R<Value> {
    let name = a.str(0).to_vec();
    let idx = a.int(0);
    with_zip(vm, a, |vm, op| {
        let i = if by_name {
            match op.locate(&name, flags) {
                Some(i) => i,
                None => {
                    op.err = ZErr::new(er::NOENT);
                    return Ok(Value::Bool(false));
                }
            }
        } else {
            let ok = idx >= 0
                && (idx as usize) < op.slots.len()
                && (flags & FL_UNCHANGED != 0 || !op.slots[idx as usize].deleted);
            if !ok {
                op.err = ZErr::new(er::INVAL);
                return Ok(Value::Bool(false));
            }
            idx as usize
        };
        f(vm, op, i)
    })
}

fn writable(op: &mut Open) -> bool {
    if op.rdonly {
        op.err = ZErr::new(er::RDONLY);
        false
    } else {
        true
    }
}

fn m_rename_index(vm: &mut Vm, a: &mut Args) -> R<Value> {
    not_empty(vm, a, 1, "new_name")?;
    rename(vm, a, false)
}

fn m_rename_name(vm: &mut Vm, a: &mut Args) -> R<Value> {
    not_empty(vm, a, 0, "name")?;
    not_empty(vm, a, 1, "new_name")?;
    rename(vm, a, true)
}

fn rename(vm: &mut Vm, a: &Args, by_name: bool) -> R<Value> {
    let new = a.str(1).to_vec();
    on_entry(vm, a, by_name, 0, |_, op, i| {
        if !writable(op) {
            return Ok(Value::Bool(false));
        }
        if let Some(j) = op.locate(&new, 0)
            && j != i
        {
            op.err = ZErr::new(er::EXISTS);
            return Ok(Value::Bool(false));
        }
        op.slots[i].utf8 = zf::text::needs_utf8_flag(&new);
        op.slots[i].name = new;
        Ok(Value::Bool(true))
    })
}

fn m_delete_index(vm: &mut Vm, a: &mut Args) -> R<Value> {
    on_entry(vm, a, false, 0, |_, op, i| {
        if !writable(op) {
            return Ok(Value::Bool(false));
        }
        op.slots[i].deleted = true;
        Ok(Value::Bool(true))
    })
}

fn m_delete_name(vm: &mut Vm, a: &mut Args) -> R<Value> {
    not_empty(vm, a, 0, "name")?;
    on_entry(vm, a, true, 0, |_, op, i| {
        if !writable(op) {
            return Ok(Value::Bool(false));
        }
        op.slots[i].deleted = true;
        Ok(Value::Bool(true))
    })
}

fn unchange(op: &mut Open, i: usize) {
    match op.slots[i].orig {
        Some(o) => {
            let e = &op.arc.entries[o];
            op.slots[i] = Slot {
                orig: Some(o),
                deleted: false,
                name: e.name.clone(),
                utf8: e.utf8(),
                comment: e.comment.clone(),
                src: None,
                method: None,
                enc: None,
                mtime: None,
                attr: None,
            };
        }
        None => op.slots[i].deleted = true,
    }
}

fn m_unchange_index(vm: &mut Vm, a: &mut Args) -> R<Value> {
    on_entry(vm, a, false, FL_UNCHANGED, |_, op, i| {
        unchange(op, i);
        Ok(Value::Bool(true))
    })
}

fn m_unchange_name(vm: &mut Vm, a: &mut Args) -> R<Value> {
    not_empty(vm, a, 0, "name")?;
    on_entry(vm, a, true, 0, |_, op, i| {
        unchange(op, i);
        Ok(Value::Bool(true))
    })
}

fn m_unchange_all(vm: &mut Vm, a: &mut Args) -> R<Value> {
    with_zip(vm, a, |_, op| {
        let n = op.arc.entries.len();
        op.slots.truncate(n);
        for i in 0..op.slots.len() {
            unchange(op, i);
        }
        op.comment = op.arc.comment.clone();
        op.comment_changed = false;
        Ok(Value::Bool(true))
    })
}

fn m_unchange_archive(vm: &mut Vm, a: &mut Args) -> R<Value> {
    with_zip(vm, a, |_, op| {
        op.comment = op.arc.comment.clone();
        op.comment_changed = false;
        Ok(Value::Bool(true))
    })
}

fn comment_len_check(vm: &mut Vm, a: &Args, i: usize) -> R<()> {
    if a.str(i).len() > 0xFFFF {
        vm.fthrow::<()>(
            a,
            "ValueError",
            &format!(
                "Argument #{} ($comment) must be less than 65535 bytes",
                i + 1
            ),
        )?;
    }
    Ok(())
}

fn m_set_archive_comment(vm: &mut Vm, a: &mut Args) -> R<Value> {
    comment_len_check(vm, a, 0)?;
    let c = a.str(0).to_vec();
    with_zip(vm, a, |_, op| {
        if !writable(op) {
            return Ok(Value::Bool(false));
        }
        op.comment = c;
        op.comment_changed = op.comment != op.arc.comment;
        Ok(Value::Bool(true))
    })
}

fn m_get_archive_comment(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let flags = a.int_or(0, 0);
    with_zip(vm, a, |_, op| {
        let c = if flags & FL_UNCHANGED != 0 {
            &op.arc.comment
        } else {
            &op.comment
        };
        Ok(Value::from(
            zf::text::decode(c, false, decode_mode(flags)).as_slice(),
        ))
    })
}

fn m_set_comment_index(vm: &mut Vm, a: &mut Args) -> R<Value> {
    comment_len_check(vm, a, 1)?;
    set_comment(vm, a, false)
}

fn m_set_comment_name(vm: &mut Vm, a: &mut Args) -> R<Value> {
    not_empty(vm, a, 0, "name")?;
    comment_len_check(vm, a, 1)?;
    set_comment(vm, a, true)
}

fn set_comment(vm: &mut Vm, a: &Args, by_name: bool) -> R<Value> {
    let c = a.str(1).to_vec();
    on_entry(vm, a, by_name, 0, |_, op, i| {
        if !writable(op) {
            return Ok(Value::Bool(false));
        }
        op.slots[i].comment = c;
        Ok(Value::Bool(true))
    })
}

fn get_comment(vm: &mut Vm, a: &Args, by_name: bool) -> R<Value> {
    let flags = a.int_or(1, 0);
    on_entry(vm, a, by_name, flags & FL_UNCHANGED, |_, op, i| {
        let s = &op.slots[i];
        let (c, utf8) = if flags & FL_UNCHANGED != 0 {
            match op.orig(i) {
                Some(e) => (e.comment.clone(), e.utf8()),
                None => (Vec::new(), false),
            }
        } else {
            (s.comment.clone(), s.utf8)
        };
        Ok(Value::from(
            zf::text::decode(&c, utf8, decode_mode(flags)).as_slice(),
        ))
    })
}

fn m_get_comment_index(vm: &mut Vm, a: &mut Args) -> R<Value> {
    get_comment(vm, a, false)
}

fn m_get_comment_name(vm: &mut Vm, a: &mut Args) -> R<Value> {
    not_empty(vm, a, 0, "name")?;
    get_comment(vm, a, true)
}

fn set_mtime(vm: &mut Vm, a: &Args, by_name: bool) -> R<Value> {
    let t = a.int(1);
    on_entry(vm, a, by_name, 0, |_, op, i| {
        if !writable(op) {
            return Ok(Value::Bool(false));
        }
        op.slots[i].mtime = Some(t);
        Ok(Value::Bool(true))
    })
}

fn m_set_mtime_index(vm: &mut Vm, a: &mut Args) -> R<Value> {
    set_mtime(vm, a, false)
}

fn m_set_mtime_name(vm: &mut Vm, a: &mut Args) -> R<Value> {
    not_empty(vm, a, 0, "name")?;
    set_mtime(vm, a, true)
}

fn set_ext_attr(vm: &mut Vm, a: &Args, by_name: bool) -> R<Value> {
    let os = a.int(1);
    let at = a.int(2);
    on_entry(vm, a, by_name, 0, |_, op, i| {
        if !writable(op) {
            return Ok(Value::Bool(false));
        }
        op.slots[i].attr = Some((os as u8, at as u32));
        Ok(Value::Bool(true))
    })
}

fn m_set_ext_attr_name(vm: &mut Vm, a: &mut Args) -> R<Value> {
    not_empty(vm, a, 0, "name")?;
    set_ext_attr(vm, a, true)
}

fn m_set_ext_attr_index(vm: &mut Vm, a: &mut Args) -> R<Value> {
    set_ext_attr(vm, a, false)
}

fn get_ext_attr(vm: &mut Vm, a: &Args, by_name: bool) -> R<Value> {
    let flags = a.int_or(3, 0);
    let r = on_entry(vm, a, by_name, flags & FL_UNCHANGED, |_, op, i| {
        let s = &op.slots[i];
        let cur = if flags & FL_UNCHANGED == 0 {
            s.attr
        } else {
            None
        };
        let v = match (cur, op.orig(i)) {
            (Some(x), _) => x,
            (None, Some(e)) => (e.opsys(), e.external_attr),
            (None, None) => (3, 0o100666u32 << 16),
        };
        Ok(Value::Array(PhpArray::from_values([
            Value::Int(v.0 as i64),
            Value::Int(v.1 as i64),
        ])))
    })?;
    match r {
        Value::Array(v) => {
            a.set_ref(1, v.get_int(0).cloned().unwrap_or(Value::Null));
            a.set_ref(2, v.get_int(1).cloned().unwrap_or(Value::Null));
            Ok(Value::Bool(true))
        }
        other => Ok(other),
    }
}

fn m_get_ext_attr_name(vm: &mut Vm, a: &mut Args) -> R<Value> {
    not_empty(vm, a, 0, "name")?;
    get_ext_attr(vm, a, true)
}

fn m_get_ext_attr_index(vm: &mut Vm, a: &mut Args) -> R<Value> {
    get_ext_attr(vm, a, false)
}

fn set_compression(vm: &mut Vm, a: &Args, by_name: bool) -> R<Value> {
    let m = a.int(1);
    let l = a.int_or(2, 0);
    on_entry(vm, a, by_name, 0, |_, op, i| {
        if !writable(op) {
            return Ok(Value::Bool(false));
        }
        if m != -1 && !((0..=0xffff).contains(&m) && zf::codec::can_compress(m as u16)) {
            op.err = ZErr::new(er::COMPNOTSUPP);
            return Ok(Value::Bool(false));
        }
        op.slots[i].method = Some((m as i32, l as i32));
        Ok(Value::Bool(true))
    })
}

fn m_set_compression_name(vm: &mut Vm, a: &mut Args) -> R<Value> {
    not_empty(vm, a, 0, "name")?;
    set_compression(vm, a, true)
}

fn m_set_compression_index(vm: &mut Vm, a: &mut Args) -> R<Value> {
    set_compression(vm, a, false)
}

fn set_encryption(vm: &mut Vm, a: &Args, by_name: bool) -> R<Value> {
    let m = a.int(1);
    let pw = a.opt_str(2).map(|s| s.to_vec());
    on_entry(vm, a, by_name, 0, |_, op, i| {
        if !writable(op) {
            return Ok(Value::Bool(false));
        }
        if !matches!(m, 0 | 1 | 257 | 258 | 259) {
            op.err = ZErr::new(er::ENCRNOTSUPP);
            return Ok(Value::Bool(false));
        }
        if m != 0 && pw.is_none() && op.password.is_none() {
            op.err = ZErr::new(er::NOPASSWD);
            return Ok(Value::Bool(false));
        }
        op.slots[i].enc = Some((m as u16, pw));
        Ok(Value::Bool(true))
    })
}

fn m_set_encryption_name(vm: &mut Vm, a: &mut Args) -> R<Value> {
    not_empty(vm, a, 0, "name")?;
    set_encryption(vm, a, true)
}

fn m_set_encryption_index(vm: &mut Vm, a: &mut Args) -> R<Value> {
    set_encryption(vm, a, false)
}

fn m_set_archive_flag(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let flag = a.int(0);
    let v = a.int(1) != 0;
    with_zip(vm, a, |_, op| {
        match flag {
            AFL_RDONLY => {
                if !v && op.opened_rdonly {
                    op.err = ZErr::new(er::RDONLY);
                    return Ok(Value::Bool(false));
                }
                op.rdonly = v;
            }
            AFL_WANT_TORRENTZIP => op.want_torrent = v,
            AFL_KEEP_EMPTY => op.keep_empty = v,
            _ => {
                op.err = ZErr::new(er::INVAL);
                return Ok(Value::Bool(false));
            }
        }
        Ok(Value::Bool(true))
    })
}

fn m_get_archive_flag(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let flag = a.int(0);
    with_zip(vm, a, |_, op| {
        let v = match flag {
            AFL_RDONLY => op.rdonly,
            AFL_IS_TORRENTZIP => op.is_torrent,
            AFL_WANT_TORRENTZIP => op.want_torrent,
            AFL_KEEP_EMPTY => op.keep_empty,
            _ => false,
        };
        Ok(Value::Int(v as i64))
    })
}

// ------------------------------------------------------------------ queries

/// statIndex()'s array (PHP manual: ZipArchive::statIndex).
fn stat_array(op: &Open, i: usize, flags: i64) -> Option<PhpArray> {
    let s = &op.slots[i];
    let unchanged = flags & FL_UNCHANGED != 0;
    let orig = op.orig(i);
    let mode = decode_mode(flags);
    let (name, crc, size, mtime, comp_size, comp_method, enc) = if unchanged || s.src.is_none() {
        let e = orig?;
        let (name, utf8) = if unchanged {
            (e.name.clone(), e.utf8())
        } else {
            (s.name.clone(), s.utf8)
        };
        let mtime = match (unchanged, s.mtime) {
            (false, Some(t)) => t,
            _ => e.mtime(),
        };
        (
            zf::text::decode(&name, utf8, mode),
            e.crc as i64,
            e.size as i64,
            mtime,
            e.comp_size as i64,
            e.real_method() as i64,
            e.encryption_method() as i64,
        )
    } else {
        let (size, crc) = match s.src.as_ref()? {
            Src::Bytes(b) => (b.len() as i64, zf::crc::crc32(b) as i64),
            Src::Dir => (0, 0),
            Src::File { path, start, len } => {
                let fl = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
                let n = if *len > 0 {
                    *len as u64
                } else {
                    fl.saturating_sub(*start)
                };
                (n as i64, 0)
            }
        };
        let method = match s.method {
            Some((m, _)) if m >= 0 => m as i64,
            _ if matches!(s.src, Some(Src::Dir)) => 0,
            _ => 8,
        };
        (
            zf::text::decode(&s.name, s.utf8, mode),
            crc,
            size,
            s.mtime.unwrap_or_else(now),
            size,
            method,
            s.enc.as_ref().map(|e| e.0 as i64).unwrap_or(0),
        )
    };
    let mut a = PhpArray::new();
    a.insert(Key::from("name"), Value::from(name.as_slice()));
    a.insert(Key::from("index"), Value::Int(i as i64));
    a.insert(Key::from("crc"), Value::Int(crc));
    a.insert(Key::from("size"), Value::Int(size));
    a.insert(Key::from("mtime"), Value::Int(mtime));
    a.insert(Key::from("comp_size"), Value::Int(comp_size));
    a.insert(Key::from("comp_method"), Value::Int(comp_method));
    a.insert(Key::from("encryption_method"), Value::Int(enc));
    Some(a)
}

fn stat(vm: &mut Vm, a: &Args, by_name: bool) -> R<Value> {
    let flags = a.int_or(1, 0);
    on_entry(
        vm,
        a,
        by_name,
        flags & (FL_UNCHANGED | FL_NOCASE | FL_NODIR),
        |_, op, i| {
            Ok(match stat_array(op, i, flags) {
                Some(arr) => Value::Array(arr),
                None => {
                    op.err = ZErr::new(er::CHANGED);
                    Value::Bool(false)
                }
            })
        },
    )
}

fn m_stat_name(vm: &mut Vm, a: &mut Args) -> R<Value> {
    stat(vm, a, true)
}

fn m_stat_index(vm: &mut Vm, a: &mut Args) -> R<Value> {
    stat(vm, a, false)
}

fn m_locate_name(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let name = a.str(0).to_vec();
    let flags = a.int_or(1, 0);
    with_zip(vm, a, |_, op| {
        Ok(match op.locate(&name, flags) {
            Some(i) => Value::Int(i as i64),
            None => {
                op.err = ZErr::new(er::NOENT);
                Value::Bool(false)
            }
        })
    })
}

fn m_get_name_index(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let flags = a.int_or(1, 0);
    on_entry(vm, a, false, flags & FL_UNCHANGED, |_, op, i| {
        let s = &op.slots[i];
        let (n, utf8) = if flags & FL_UNCHANGED != 0 {
            match op.orig(i) {
                Some(e) => (e.name.clone(), e.utf8()),
                None => return Ok(Value::Bool(false)),
            }
        } else {
            (s.name.clone(), s.utf8)
        };
        Ok(Value::from(
            zf::text::decode(&n, utf8, decode_mode(flags)).as_slice(),
        ))
    })
}

fn get_from(vm: &mut Vm, a: &Args, by_name: bool) -> R<Value> {
    let len = a.int_or(1, 0);
    let flags = a.int_or(2, 0);
    if len < 0 {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #2 ($len) must be greater than or equal to 0",
        );
    }
    on_entry(
        vm,
        a,
        by_name,
        flags & (FL_UNCHANGED | FL_NOCASE | FL_NODIR),
        |_, op, i| match op.read_data(i, flags & FL_UNCHANGED != 0, flags & FL_COMPRESSED != 0) {
            Ok(mut d) => {
                if len > 0 && (len as usize) < d.len() {
                    d.truncate(len as usize);
                }
                Ok(Value::Str(PhpStr::from(d)))
            }
            Err(e) => {
                op.err = e;
                Ok(Value::Bool(false))
            }
        },
    )
}

fn m_get_from_name(vm: &mut Vm, a: &mut Args) -> R<Value> {
    not_empty(vm, a, 0, "name")?;
    get_from(vm, a, true)
}

fn m_get_from_index(vm: &mut Vm, a: &mut Args) -> R<Value> {
    get_from(vm, a, false)
}

// ------------------------------------------------------------------ streams

/// An entry being read: decoded at open, served from memory. Only stored
/// unencrypted entries seek (test oo_stream_seek.phpt).
struct EntryStream {
    data: Vec<u8>,
    pos: usize,
    seekable: bool,
    crc_bad: bool,
    failed: bool,
    alive: Option<Rc<Cell<bool>>>,
    /// stream_get_meta_data()'s "uri" for getStream() streams, which have
    /// no wrapper (test stream_meta_data.phpt).
    uri: Option<Vec<u8>>,
}

impl EntryStream {
    fn error(&mut self, vm: &mut Vm, msg: &str) {
        self.failed = true;
        let f = cur_fname(vm);
        let _ = vm.warn(&format!("{f}(): Zip stream error: {msg}"));
    }
}

impl NativeStream for EntryStream {
    fn read(&mut self, vm: &mut Vm, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.failed {
            return Ok(0);
        }
        if let Some(a) = &self.alive
            && !a.get()
        {
            self.error(vm, zf::error::description(er::ZIPCLOSED));
            return Ok(0);
        }
        let n = buf.len().min(self.data.len() - self.pos);
        if n == 0 && self.crc_bad {
            self.error(vm, zf::error::description(er::CRC));
            return Ok(0);
        }
        buf[..n].copy_from_slice(&self.data[self.pos..self.pos + n]);
        self.pos += n;
        Ok(n)
    }
    fn seek(&mut self, vm: &mut Vm, pos: std::io::SeekFrom) -> Option<std::io::Result<u64>> {
        if !self.seekable {
            let _ = vm.warn("fseek(): zip stream does not support seeking");
            return Some(Err(std::io::Error::other("not seekable")));
        }
        let np = match pos {
            std::io::SeekFrom::Start(n) => n as i64,
            std::io::SeekFrom::Current(n) => self.pos as i64 + n,
            std::io::SeekFrom::End(n) => self.data.len() as i64 + n,
        };
        if np < 0 || np as usize > self.data.len() {
            return Some(Err(std::io::Error::other("bad offset")));
        }
        self.pos = np as usize;
        Some(Ok(np as u64))
    }
    fn tell(&mut self) -> Option<u64> {
        Some(self.pos as u64)
    }
    fn eof(&self) -> bool {
        self.pos >= self.data.len() && (!self.crc_bad || self.failed)
    }
    fn meta(&self, m: &mut PhpArray) {
        if let Some(u) = &self.uri {
            m.insert(Key::from("uri"), Value::from(u.as_slice()));
        }
    }
}

/// Decodes entry `e` of `arc` for a stream.
fn entry_stream(
    arc: &zf::Archive,
    data: &[u8],
    i: usize,
    pw: Option<&[u8]>,
) -> Result<EntryStream, ZErr> {
    let d = arc.decode(data, i, pw)?;
    let e = &arc.entries[i];
    Ok(EntryStream {
        data: d.data,
        pos: 0,
        seekable: e.real_method() == cm::STORE && !e.is_encrypted(),
        crc_bad: !d.crc_ok,
        failed: false,
        alive: None,
        uri: None,
    })
}

fn get_stream(vm: &mut Vm, a: &Args, by_name: bool, flags: i64) -> R<Value> {
    on_entry(
        vm,
        a,
        by_name,
        flags & (FL_UNCHANGED | FL_NOCASE | FL_NODIR),
        |_, op, i| {
            let unchanged = flags & FL_UNCHANGED != 0;
            let s = &op.slots[i];
            let name = s.name.clone();
            let r = match (&s.src, s.orig) {
                (Some(src), _) if !unchanged => read_src(src).map(|d| EntryStream {
                    data: d,
                    pos: 0,
                    seekable: true,
                    crc_bad: false,
                    failed: false,
                    alive: None,
                    uri: None,
                }),
                (_, Some(o)) => entry_stream(&op.arc, &op.data, o, op.password.as_deref()),
                _ => Err(ZErr::new(er::CHANGED)),
            };
            match r {
                Ok(mut st) => {
                    st.alive = Some(op.alive.clone());
                    st.uri = Some(name.clone());
                    let s = Stream::new(Backing::Native(Box::new(st)), b"rb", &name, "", "zip");
                    Ok(Value::Resource(new_stream_res(s)))
                }
                Err(e) => {
                    op.err = e;
                    Ok(Value::Bool(false))
                }
            }
        },
    )
}

fn m_get_stream(vm: &mut Vm, a: &mut Args) -> R<Value> {
    not_empty(vm, a, 0, "name")?;
    get_stream(vm, a, true, 0)
}

fn m_get_stream_name(vm: &mut Vm, a: &mut Args) -> R<Value> {
    not_empty(vm, a, 0, "name")?;
    let f = a.int_or(1, 0);
    get_stream(vm, a, true, f)
}

fn m_get_stream_index(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let f = a.int_or(1, 0);
    get_stream(vm, a, false, f)
}

/// The stream context passed to the function opening the URL.
fn context_arg(a: &Args) -> Value {
    let name = a.fname().to_ascii_lowercase();
    let idx = match name.as_str() {
        "fopen" => Some(3),
        "file_get_contents" | "file" | "readfile" | "copy" => Some(2),
        _ => None,
    };
    if let Some(i) = idx {
        let v = a.val(i);
        if super::sockets::with_context(&v, |_| ()).is_some() {
            return v;
        }
    }
    super::sockets::default_context()
}

/// `zip://archive#entry` (PHP manual: Zip context options: `password`).
fn wrapper_open(vm: &mut Vm, a: &Args, url: &[u8], mode: &[u8], quiet: bool) -> R<Option<Stream>> {
    let fail = |vm: &mut Vm| -> R<Option<Stream>> {
        if !quiet {
            vm.warn(&format!(
                "{}({}): Failed to open stream: operation failed",
                a.fname(),
                String::from_utf8_lossy(url)
            ))?;
        }
        Ok(None)
    };
    let rest = &url[b"zip://".len()..];
    let Some(h) = rest.iter().rposition(|&c| c == b'#') else {
        return fail(vm);
    };
    if !mode.starts_with(b"r") || mode.contains(&b'+') {
        return fail(vm);
    }
    let (path, entry) = (&rest[..h], &rest[h + 1..]);
    let Ok(data) = std::fs::read(resolve(vm, path)) else {
        return fail(vm);
    };
    let Ok(arc) = zf::parse(&data, zf::ReadOptions::default()) else {
        return fail(vm);
    };
    let Some(i) = arc.locate(entry) else {
        return fail(vm);
    };
    let pw =
        super::sockets::context_option(&context_arg(a), "zip", "password").and_then(|v| match v {
            Value::Str(s) => Some(s.to_vec()),
            Value::Int(n) => Some(n.to_string().into_bytes()),
            _ => None,
        });
    match entry_stream(&arc, &data, i, pw.as_deref()) {
        Ok(st) => Ok(Some(Stream::new(
            Backing::Native(Box::new(st)),
            mode,
            url,
            "zip wrapper",
            "zip",
        ))),
        Err(_) => fail(vm),
    }
}

// ------------------------------------------------------------------ extract

/// An entry name as a relative path: no root, no `.`/`..` (tests
/// bug70350.phpt, bug81420.phpt).
fn safe_relative(name: &[u8]) -> Vec<u8> {
    let mut parts: Vec<&[u8]> = Vec::new();
    for p in name.split(|&c| c == b'/') {
        match p {
            b"" | b"." => {}
            b".." => {
                parts.pop();
            }
            _ => parts.push(p),
        }
    }
    parts.join(&b'/')
}

fn m_extract_to(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let dest = a.str(0).to_vec();
    let files = a.val(1);
    let o = a.this_obj().clone();
    let Some(op) = take_open(&o) else {
        return vm.throw("ValueError", "Invalid or uninitialized Zip object");
    };
    let r = extract(vm, a, &op, &dest, &files);
    if with_obj(&o, |z| z.open.is_none()).unwrap_or(false) {
        put_open(&o, op);
    }
    r
}

fn extract(vm: &mut Vm, a: &Args, op: &Open, dest: &[u8], files: &Value) -> R<Value> {
    let is_url = dest.windows(3).any(|w| w == b"://");
    if !is_url {
        let _ = std::fs::create_dir_all(resolve(vm, dest));
    }
    let idx: Vec<usize> = match files {
        Value::Null => (0..op.slots.len())
            .filter(|&i| !op.slots[i].deleted)
            .collect(),
        Value::Array(arr) => {
            let mut v = Vec::new();
            for (_, x) in arr.iter() {
                let n = vm.to_php_str(&x.deref())?;
                match op.locate(&n, 0) {
                    Some(i) => v.push(i),
                    None => return Ok(Value::Bool(false)),
                }
            }
            v
        }
        other => {
            let n = vm.to_php_str(other)?;
            match op.locate(&n, 0) {
                Some(i) => vec![i],
                None => return Ok(Value::Bool(false)),
            }
        }
    };
    for i in idx {
        let s = &op.slots[i];
        let rel = safe_relative(&s.name);
        if rel.is_empty() {
            continue;
        }
        let mut target = dest.to_vec();
        if target.last() != Some(&b'/') {
            target.push(b'/');
        }
        target.extend_from_slice(&rel);
        if s.name.last() == Some(&b'/') {
            if !is_url {
                let _ = std::fs::create_dir_all(resolve(vm, &target));
            }
            continue;
        }
        if !is_url && let Some(p) = target.iter().rposition(|&c| c == b'/') {
            let _ = std::fs::create_dir_all(resolve(vm, &target[..p]));
        }
        let data = match op.read_data(i, false, false) {
            Ok(d) => d,
            Err(_) => return Ok(Value::Bool(false)),
        };
        let Some(mut st) = open_stream(vm, a, &target, b"wb", false)? else {
            return Ok(Value::Bool(false));
        };
        let ok = st.write_all(vm, &data).is_ok();
        super::streams::close_stream(vm, &mut st)?;
        if !ok {
            return Ok(Value::Bool(false));
        }
        // the entry's mtime (test bug50678.phpt)
        if !is_url {
            let mtime = s.mtime.or_else(|| op.orig(i).map(|e| e.mtime()));
            if let Some(t) = mtime
                && let Ok(f) = std::fs::OpenOptions::new()
                    .write(true)
                    .open(resolve(vm, &target))
            {
                let _ = f.set_modified(
                    std::time::UNIX_EPOCH + std::time::Duration::from_secs(t.max(0) as u64),
                );
            }
        }
    }
    Ok(Value::Bool(true))
}

// ------------------------------------------------------------------ zip_* functions

/// A `zip_open()` resource ("Zip Directory").
struct ZipDir {
    arc: Rc<zf::Archive>,
    data: Rc<Vec<u8>>,
    next: usize,
}

/// A `zip_read()` resource ("Zip Entry"): it doesn't keep the directory
/// alive (test gh17319.phpt).
struct ZipEntryRes {
    dir: WeakRes,
    idx: usize,
    entry: zf::Entry,
    buf: Option<Vec<u8>>,
    pos: usize,
}

fn deprecated(vm: &mut Vm, name: &str, alt: Option<&str>) -> R<()> {
    match alt {
        Some(alt) => vm.deprecated(&format!(
            "Function {name}() is deprecated since 8.0, use {alt} instead"
        )),
        None => vm.deprecated(&format!("Function {name}() is deprecated since 8.0")),
    }
}

/// The `T` behind resource argument `i` ("supplied resource is not a valid
/// `kind` resource" otherwise).
fn res_arg<T: 'static>(
    vm: &mut Vm,
    a: &Args,
    i: usize,
    param: &str,
    kind: &str,
) -> R<(ResRef, *const RefCell<T>)> {
    let v = a.val(i);
    let Value::Resource(r) = &v else {
        return vm.fthrow(
            a,
            "TypeError",
            &format!(
                "Argument #{} (${param}) must be of type resource, {} given",
                i + 1,
                Vm::type_name_given(&v)
            ),
        );
    };
    let p = {
        let data = r.res().data.borrow();
        data.as_ref()
            .and_then(|d| d.downcast_ref::<RefCell<T>>())
            .map(|c| c as *const RefCell<T>)
    };
    match p {
        Some(p) => Ok((r.clone(), p)),
        None => vm.fthrow(
            a,
            "TypeError",
            &format!("supplied resource is not a valid {kind} resource"),
        ),
    }
}

fn zip_open(vm: &mut Vm, a: &mut Args) -> R<Value> {
    deprecated(vm, "zip_open", Some("ZipArchive::open()"))?;
    let f = a.str(0);
    if f.is_empty() {
        return vm.fthrow(a, "ValueError", "Argument #1 ($filename) must not be empty");
    }
    match open_archive(vm, &f, 0)? {
        Ok(op) => Ok(Value::Resource(ResRef::new(
            PhpStr::intern(b"Zip Directory"),
            Box::new(RefCell::new(ZipDir {
                arc: op.arc,
                data: op.data,
                next: 0,
            })),
        ))),
        Err(e) => Ok(Value::Int(e.code as i64)),
    }
}

fn zip_close(vm: &mut Vm, a: &mut Args) -> R<Value> {
    deprecated(vm, "zip_close", Some("ZipArchive::close()"))?;
    let (r, _) = res_arg::<ZipDir>(vm, a, 0, "zip", "Zip Directory")?;
    r.close();
    Ok(Value::Null)
}

fn zip_read(vm: &mut Vm, a: &mut Args) -> R<Value> {
    deprecated(vm, "zip_read", Some("ZipArchive::statIndex()"))?;
    let (r, p) = res_arg::<ZipDir>(vm, a, 0, "zip", "Zip Directory")?;
    // SAFETY: the resource is held by `r` for the call.
    let mut d = unsafe { (*p).borrow_mut() };
    if d.next >= d.arc.entries.len() {
        return Ok(Value::Bool(false));
    }
    let idx = d.next;
    d.next += 1;
    let entry = d.arc.entries[idx].clone();
    drop(d);
    Ok(Value::Resource(ResRef::new(
        PhpStr::intern(b"Zip Entry"),
        Box::new(RefCell::new(ZipEntryRes {
            dir: r.downgrade(),
            idx,
            entry,
            buf: None,
            pos: 0,
        })),
    )))
}

/// The entry resource, None when its directory is gone.
fn entry_arg(vm: &mut Vm, a: &Args, i: usize) -> R<Option<(ResRef, *const RefCell<ZipEntryRes>)>> {
    let (r, p) = res_arg::<ZipEntryRes>(vm, a, i, "zip_entry", "Zip Entry")?;
    // SAFETY: held by `r`.
    let alive = unsafe { (*p).borrow().dir.upgrade() }.is_some_and(|d| !d.is_closed());
    Ok(if alive { Some((r, p)) } else { None })
}

fn load_entry(e: &mut ZipEntryRes) -> bool {
    if e.buf.is_some() {
        return true;
    }
    let Some(d) = e.dir.upgrade() else {
        return false;
    };
    let data = d.res().data.borrow();
    let Some(dir) = data
        .as_ref()
        .and_then(|x| x.downcast_ref::<RefCell<ZipDir>>())
    else {
        return false;
    };
    let dir = dir.borrow();
    match dir.arc.read(&dir.data, e.idx, None) {
        Ok(b) => {
            e.buf = Some(b);
            true
        }
        Err(_) => false,
    }
}

fn zip_entry_open(vm: &mut Vm, a: &mut Args) -> R<Value> {
    deprecated(vm, "zip_entry_open", None)?;
    res_arg::<ZipDir>(vm, a, 0, "zip_dp", "Zip Directory")?;
    let Some((_r, p)) = entry_arg(vm, a, 1)? else {
        return Ok(Value::Bool(false));
    };
    // SAFETY: held by `_r`.
    let mut e = unsafe { (*p).borrow_mut() };
    Ok(Value::Bool(load_entry(&mut e)))
}

fn zip_entry_close(vm: &mut Vm, a: &mut Args) -> R<Value> {
    deprecated(vm, "zip_entry_close", None)?;
    let (r, _) = res_arg::<ZipEntryRes>(vm, a, 0, "zip_entry", "Zip Entry")?;
    r.close();
    Ok(Value::Bool(true))
}

fn zip_entry_read(vm: &mut Vm, a: &mut Args) -> R<Value> {
    deprecated(vm, "zip_entry_read", Some("ZipArchive::getFromIndex()"))?;
    let len = a.int_or(1, 1024);
    if len <= 0 {
        return vm.fthrow(a, "ValueError", "Argument #2 ($len) must be greater than 0");
    }
    let Some((_r, p)) = entry_arg(vm, a, 0)? else {
        return Ok(Value::Bool(false));
    };
    // SAFETY: held by `_r`.
    let mut e = unsafe { (*p).borrow_mut() };
    if !load_entry(&mut e) {
        return Ok(Value::Bool(false));
    }
    let buf = e.buf.as_ref().expect("loaded");
    let start = e.pos.min(buf.len());
    let end = (start + len as usize).min(buf.len());
    let out = buf[start..end].to_vec();
    e.pos = end;
    Ok(Value::Str(PhpStr::from(out)))
}

fn entry_info(vm: &mut Vm, a: &Args, f: impl FnOnce(&zf::Entry) -> Value) -> R<Value> {
    let Some((_r, p)) = entry_arg(vm, a, 0)? else {
        return Ok(Value::Bool(false));
    };
    // SAFETY: held by `_r`.
    let e = unsafe { (*p).borrow() };
    Ok(f(&e.entry))
}

fn zip_entry_name(vm: &mut Vm, a: &mut Args) -> R<Value> {
    deprecated(vm, "zip_entry_name", Some("ZipArchive::statIndex()"))?;
    entry_info(vm, a, |e| {
        Value::from(zf::text::decode(&e.name, e.utf8(), Decode::Guess).as_slice())
    })
}

fn zip_entry_compressedsize(vm: &mut Vm, a: &mut Args) -> R<Value> {
    deprecated(
        vm,
        "zip_entry_compressedsize",
        Some("ZipArchive::statIndex()"),
    )?;
    entry_info(vm, a, |e| Value::Int(e.comp_size as i64))
}

fn zip_entry_filesize(vm: &mut Vm, a: &mut Args) -> R<Value> {
    deprecated(vm, "zip_entry_filesize", Some("ZipArchive::statIndex()"))?;
    entry_info(vm, a, |e| Value::Int(e.size as i64))
}

fn zip_entry_compressionmethod(vm: &mut Vm, a: &mut Args) -> R<Value> {
    deprecated(
        vm,
        "zip_entry_compressionmethod",
        Some("ZipArchive::statIndex()"),
    )?;
    entry_info(vm, a, |e| {
        Value::from(match e.real_method() {
            0 => "stored",
            1 => "shrunk",
            2..=5 => "reduced",
            6 => "imploded",
            7 => "tokenized",
            8 => "deflated",
            9 => "deflatedX",
            10 => "implodedX",
            _ => "unknown",
        })
    })
}
