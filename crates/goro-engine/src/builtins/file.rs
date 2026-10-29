//! Files, directories and streams (PHP manual: Filesystem functions,
//! Directory functions, Streams; php:// wrappers). Plain files use the
//! real filesystem; php://memory, php://temp, php://stdin/stdout/stderr,
//! php://output and php://input are supported.

use std::io::{Read, Seek, SeekFrom, Write};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};

use goro_runtime::{Key, PhpArray, PhpStr, ResRef, Value};

use crate::func::Args;
use crate::registry::{ConstVal, Module};
use crate::vm::{R, Vm};

pub static MODULE: Module = Module {
    name: "standard",
    functions: &[
        (
            "function fopen(string $filename, string $mode, bool $use_include_path = false, $context = null)",
            fopen,
        ),
        ("function fclose($stream): bool", fclose),
        ("function fread($stream, int $length): string|false", fread),
        (
            "function fwrite($stream, string $data, ?int $length = null): int|false",
            fwrite,
        ),
        (
            "function fputs($stream, string $data, ?int $length = null): int|false",
            fwrite,
        ),
        (
            "function fgets($stream, ?int $length = null): string|false",
            fgets,
        ),
        ("function fgetc($stream): string|false", fgetc),
        ("function feof($stream): bool", feof),
        ("function fflush($stream): bool", fflush),
        ("function ftell($stream): int|false", ftell),
        (
            "function fseek($stream, int $offset, int $whence = SEEK_SET): int",
            fseek,
        ),
        ("function rewind($stream): bool", rewind),
        ("function ftruncate($stream, int $size): bool", ftruncate),
        ("function fstat($stream): array|false", fstat),
        (
            "function flock($stream, int $operation, &$would_block = null): bool",
            flock,
        ),
        ("function fpassthru($stream): int", fpassthru),
        (
            "function stream_copy_to_stream($from, $to, ?int $length = null, int $offset = 0): int|false",
            stream_copy_to_stream,
        ),
        (
            "function stream_get_line($stream, int $length, string $ending = \"\"): string|false",
            stream_get_line,
        ),
        (
            "function stream_get_contents($stream, ?int $length = null, int $offset = -1): string|false",
            stream_get_contents,
        ),
        (
            "function stream_get_meta_data($stream): array",
            stream_get_meta_data,
        ),
        ("function stream_isatty($stream): bool", stream_isatty),
        (
            "function posix_isatty($file_descriptor): bool",
            stream_isatty,
        ),
        // stream_set_blocking() and the stream_context_*() functions are in
        // builtins/sockets.rs.
        (
            "function stream_set_write_buffer($stream, int $size): int",
            set_buffer,
        ),
        (
            "function stream_set_read_buffer($stream, int $size): int",
            set_buffer,
        ),
        (
            "function set_file_buffer($stream, int $size): int",
            set_buffer,
        ),
        ("function stream_get_wrappers(): array", stream_get_wrappers),
        ("function stream_is_local($stream): bool", ret_true),
        (
            "function stream_resolve_include_path(string $filename): string|false",
            stream_resolve_include_path,
        ),
        (
            "function stream_supports_lock($stream): bool",
            stream_supports_lock,
        ),
        (
            "function file_get_contents(string $filename, bool $use_include_path = false, $context = null, int $offset = 0, ?int $length = null): string|false",
            file_get_contents,
        ),
        (
            "function file_put_contents(string $filename, mixed $data, int $flags = 0, $context = null): int|false",
            file_put_contents,
        ),
        (
            "function file(string $filename, int $flags = 0, $context = null): array|false",
            file,
        ),
        (
            "function readfile(string $filename, bool $use_include_path = false, $context = null): int|false",
            readfile,
        ),
        (
            "function unlink(string $filename, $context = null): bool",
            unlink,
        ),
        (
            "function rename(string $from, string $to, $context = null): bool",
            rename,
        ),
        (
            "function copy(string $from, string $to, $context = null): bool",
            copy,
        ),
        (
            "function mkdir(string $directory, int $permissions = 0777, bool $recursive = false, $context = null): bool",
            mkdir,
        ),
        (
            "function rmdir(string $directory, $context = null): bool",
            rmdir,
        ),
        ("function is_file(string $filename): bool", is_file),
        ("function is_dir(string $filename): bool", is_dir),
        ("function is_link(string $filename): bool", is_link),
        ("function file_exists(string $filename): bool", file_exists),
        ("function is_readable(string $filename): bool", is_readable),
        ("function is_writable(string $filename): bool", is_writable),
        ("function is_writeable(string $filename): bool", is_writable),
        (
            "function is_executable(string $filename): bool",
            is_executable,
        ),
        ("function filesize(string $filename): int|false", filesize),
        ("function filemtime(string $filename): int|false", filemtime),
        ("function fileatime(string $filename): int|false", fileatime),
        ("function filectime(string $filename): int|false", filectime),
        ("function fileperms(string $filename): int|false", fileperms),
        ("function fileinode(string $filename): int|false", fileinode),
        ("function fileowner(string $filename): int|false", fileowner),
        ("function filegroup(string $filename): int|false", filegroup),
        (
            "function filetype(string $filename): string|false",
            filetype,
        ),
        (
            "function touch(string $filename, ?int $mtime = null, ?int $atime = null): bool",
            touch,
        ),
        (
            "function chmod(string $filename, int $permissions): bool",
            chmod,
        ),
        (
            "function chown(string $filename, string|int $user): bool",
            chown,
        ),
        (
            "function chgrp(string $filename, string|int $group): bool",
            chgrp,
        ),
        (
            "function lchown(string $filename, string|int $user): bool",
            lchown,
        ),
        (
            "function lchgrp(string $filename, string|int $group): bool",
            lchgrp,
        ),
        ("function stat(string $filename): array|false", stat),
        ("function lstat(string $filename): array|false", lstat),
        (
            "function clearstatcache(bool $clear_realpath_cache = false, string $filename = \"\"): void",
            clearstatcache,
        ),
        ("function realpath(string $path): string|false", realpath),
        (
            "function dirname(string $path, int $levels = 1): string",
            dirname,
        ),
        (
            "function basename(string $path, string $suffix = \"\"): string",
            basename,
        ),
        (
            "function pathinfo(string $path, int $flags = PATHINFO_ALL): array|string",
            pathinfo,
        ),
        (
            "function fnmatch(string $pattern, string $filename, int $flags = 0): bool",
            fnmatch_fn,
        ),
        (
            "function glob(string $pattern, int $flags = 0): array|false",
            glob,
        ),
        (
            "function scandir(string $directory, int $sorting_order = SCANDIR_SORT_ASCENDING, $context = null): array|false",
            scandir,
        ),
        (
            "function opendir(string $directory, $context = null)",
            opendir,
        ),
        (
            "function readdir($dir_handle = null): string|false",
            readdir,
        ),
        ("function rewinddir($dir_handle = null): void", rewinddir),
        ("function closedir($dir_handle = null): void", closedir),
        (
            "function tempnam(string $directory, string $prefix): string|false",
            tempnam,
        ),
        ("function tmpfile()", tmpfile),
        ("function getcwd(): string|false", getcwd),
        ("function chdir(string $directory): bool", chdir),
        ("function umask(?int $mask = null): int", umask),
        ("function readlink(string $path): string|false", readlink),
        (
            "function symlink(string $target, string $link): bool",
            symlink,
        ),
        ("function link(string $target, string $link): bool", link),
        ("function linkinfo(string $path): int|false", linkinfo),
        (
            "function disk_free_space(string $directory): float|false",
            disk_free_space,
        ),
        (
            "function disk_total_space(string $directory): float|false",
            disk_total_space,
        ),
        (
            "function diskfreespace(string $directory): float|false",
            disk_free_space,
        ),
        (
            "function fputcsv($stream, array $fields, string $separator = \",\", string $enclosure = \"\\\"\", string $escape = \"\\\\\", string $eol = \"\\n\"): int|false",
            fputcsv,
        ),
        (
            "function fprintf($stream, string $format, mixed ...$values): int",
            fprintf,
        ),
        (
            "function vfprintf($stream, string $format, array $values): int",
            vfprintf,
        ),
        (
            "function fscanf($stream, string $format, mixed &...$vars): array|int|false|null",
            fscanf,
        ),
        (
            "function fgetcsv($stream, ?int $length = null, string $separator = \",\", string $enclosure = \"\\\"\", string $escape = \"\\\\\"): array|false",
            fgetcsv,
        ),
        (
            "function str_getcsv(string $string, string $separator = \",\", string $enclosure = \"\\\"\", string $escape = \"\\\\\"): array",
            str_getcsv,
        ),
        ("function fsync($stream): bool", fsync),
        ("function fdatasync($stream): bool", fsync),
    ],
    constants: &[
        ("SEEK_SET", ConstVal::Int(0)),
        ("SEEK_CUR", ConstVal::Int(1)),
        ("SEEK_END", ConstVal::Int(2)),
        ("LOCK_SH", ConstVal::Int(1)),
        ("LOCK_EX", ConstVal::Int(2)),
        ("LOCK_UN", ConstVal::Int(3)),
        ("LOCK_NB", ConstVal::Int(4)),
        ("FILE_USE_INCLUDE_PATH", ConstVal::Int(1)),
        ("FILE_IGNORE_NEW_LINES", ConstVal::Int(2)),
        ("FILE_SKIP_EMPTY_LINES", ConstVal::Int(4)),
        ("FILE_APPEND", ConstVal::Int(8)),
        ("FILE_NO_DEFAULT_CONTEXT", ConstVal::Int(16)),
        ("FILE_TEXT", ConstVal::Int(0)),
        ("FILE_BINARY", ConstVal::Int(0)),
        ("PATHINFO_DIRNAME", ConstVal::Int(1)),
        ("PATHINFO_BASENAME", ConstVal::Int(2)),
        ("PATHINFO_EXTENSION", ConstVal::Int(4)),
        ("PATHINFO_FILENAME", ConstVal::Int(8)),
        ("PATHINFO_ALL", ConstVal::Int(15)),
        ("GLOB_BRACE", ConstVal::Int(1024)),
        ("GLOB_MARK", ConstVal::Int(2)),
        ("GLOB_NOSORT", ConstVal::Int(4)),
        ("GLOB_NOCHECK", ConstVal::Int(16)),
        ("GLOB_NOESCAPE", ConstVal::Int(64)),
        ("GLOB_ERR", ConstVal::Int(1)),
        ("GLOB_ONLYDIR", ConstVal::Int(1 << 30)),
        ("GLOB_AVAILABLE_FLAGS", ConstVal::Int(1073741911)),
        ("DIRECTORY_SEPARATOR", ConstVal::Str("/")),
        ("PATH_SEPARATOR", ConstVal::Str(":")),
        ("SCANDIR_SORT_ASCENDING", ConstVal::Int(0)),
        ("SCANDIR_SORT_DESCENDING", ConstVal::Int(1)),
        ("SCANDIR_SORT_NONE", ConstVal::Int(2)),
        ("FNM_NOESCAPE", ConstVal::Int(2)),
        ("FNM_PATHNAME", ConstVal::Int(1)),
        ("FNM_PERIOD", ConstVal::Int(4)),
        ("FNM_CASEFOLD", ConstVal::Int(16)),
        ("STREAM_REPORT_ERRORS", ConstVal::Int(8)),
        ("STREAM_USE_PATH", ConstVal::Int(1)),
        ("STREAM_URL_STAT_LINK", ConstVal::Int(1)),
        ("STREAM_URL_STAT_QUIET", ConstVal::Int(2)),
        ("STREAM_MKDIR_RECURSIVE", ConstVal::Int(1)),
        ("STREAM_IS_URL", ConstVal::Int(1)),
        ("STREAM_OPTION_BLOCKING", ConstVal::Int(1)),
        ("STREAM_OPTION_READ_TIMEOUT", ConstVal::Int(4)),
        ("STREAM_OPTION_READ_BUFFER", ConstVal::Int(2)),
        ("STREAM_OPTION_WRITE_BUFFER", ConstVal::Int(3)),
        ("STREAM_BUFFER_NONE", ConstVal::Int(0)),
        ("STREAM_BUFFER_LINE", ConstVal::Int(1)),
        ("STREAM_BUFFER_FULL", ConstVal::Int(2)),
        ("STREAM_CAST_AS_STREAM", ConstVal::Int(0)),
        ("STREAM_CAST_FOR_SELECT", ConstVal::Int(3)),
        ("STREAM_META_TOUCH", ConstVal::Int(1)),
        ("STREAM_META_OWNER", ConstVal::Int(3)),
        ("STREAM_META_OWNER_NAME", ConstVal::Int(2)),
        ("STREAM_META_GROUP", ConstVal::Int(5)),
        ("STREAM_META_GROUP_NAME", ConstVal::Int(4)),
        ("STREAM_META_ACCESS", ConstVal::Int(6)),
    ],
    init: Some(init),
    on_ini_set: Some(on_ini_set),
    listed: false,
    ..Module::EMPTY
};

/// ini_set("auto_detect_line_endings", on): deprecated (observed on PHP
/// 8.5; test auto_detect_line_endings_2.phpt).
fn on_ini_set(vm: &mut Vm, k: &[u8], v: &[u8]) -> R<bool> {
    if k == b"auto_detect_line_endings" && super::misc::assert_ini_truthy(v) {
        vm.deprecated("auto_detect_line_endings is deprecated")?;
    }
    Ok(true)
}

/// The backing of a stream.
pub enum Backing {
    File(std::fs::File),
    Memory {
        data: Vec<u8>,
        pos: usize,
    },
    Stdin,
    Stdout,
    Stderr,
    /// php://output: through the output buffers.
    Output,
    Dir {
        entries: Vec<Vec<u8>>,
        pos: usize,
    },
    /// A user stream wrapper instance (stream_wrapper_register).
    User {
        obj: goro_runtime::ObjRef,
    },
    /// A directory opened through a user wrapper.
    UserDir {
        obj: goro_runtime::ObjRef,
    },
    /// A stream implemented by a Rust module (see builtins/streams.rs).
    Native(Box<dyn super::streams::NativeStream>),
}

/// A stream resource's data.
pub struct Stream {
    pub backing: Backing,
    pub readable: bool,
    pub writable: bool,
    pub append: bool,
    pub eof: bool,
    pub uri: PhpStr,
    pub mode: PhpStr,
    pub wrapper: &'static str,
    /// data: streams: their media type, parameters and base64 flag, as
    /// stream_get_meta_data() reports them first.
    pub data_meta: Option<PhpArray>,
    pub stream_type: &'static str,
    /// Bytes read ahead (stdin line reads, user stream chunks).
    pub pending: Vec<u8>,
    /// The position seen by the caller (user streams).
    pub upos: i64,
    /// Filters on reads and writes (stream_filter_append()).
    pub read_filters: Vec<super::streams::AttachedFilter>,
    pub write_filters: Vec<super::streams::AttachedFilter>,
    /// Filtered bytes not read yet; whether the read filters were closed.
    pub filtered: Vec<u8>,
    pub filters_closed: bool,
}

impl Stream {
    pub fn new(
        backing: Backing,
        mode: &[u8],
        uri: &[u8],
        wrapper: &'static str,
        stream_type: &'static str,
    ) -> Stream {
        let m = mode;
        let plus = m.contains(&b'+');
        let first = m.first().copied().unwrap_or(b'r');
        Stream {
            backing,
            readable: first == b'r' || plus,
            writable: first != b'r' || plus,
            append: first == b'a',
            eof: false,
            uri: PhpStr::new(uri),
            mode: PhpStr::new(mode),
            wrapper,
            stream_type,
            pending: Vec::new(),
            upos: 0,
            read_filters: Vec::new(),
            write_filters: Vec::new(),
            filtered: Vec::new(),
            filters_closed: false,
            data_meta: None,
        }
    }

    /// A pipe or socket from proc_open()/popen() (made without a wrapper):
    /// reads return what is available, it is not seekable.
    pub fn is_pipe(&self) -> bool {
        match &self.backing {
            Backing::File(_) => self.wrapper.is_empty(),
            Backing::Native(n) => n.is_socket(),
            _ => false,
        }
    }

    pub fn read_some(&mut self, vm: &mut Vm, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.read_filters.is_empty() {
            return self.read_raw(vm, buf);
        }
        // Filtered reads: raw chunks through the chain until output.
        while self.filtered.is_empty() && !self.filters_closed {
            let mut raw = vec![0u8; 8192];
            let n = self.read_raw(vm, &mut raw)?;
            raw.truncate(n);
            // a short read of a file or memory stream is its end: the
            // filters see the last data with $closing set (test
            // streams/bug79984.phpt)
            let closing = n == 0
                || (n < 8192
                    && (matches!(self.backing, Backing::Memory { .. })
                        || matches!(self.backing, Backing::File(_)) && !self.is_pipe()));
            let mut chain = std::mem::take(&mut self.read_filters);
            let prev = FILTER_EOF.with(|c| c.replace(Some(closing)));
            let r = super::streams::run_chain(vm, &mut chain, &raw, closing);
            FILTER_EOF.with(|c| c.set(prev));
            // A filter may have been removed meanwhile; keep the chain.
            if self.read_filters.is_empty() {
                self.read_filters = chain;
            }
            match r {
                Ok(Some(out)) => self.filtered.extend_from_slice(&out),
                // a failed filter ends the stream: feof() is then true
                // (test ext/zlib/tests/bug71417.phpt)
                Ok(None) => {
                    self.filters_closed = true;
                    self.eof = true;
                    return Err(std::io::Error::other("stream filter failed"));
                }
                Err(e) => {
                    if vm.err.deferred.is_none() {
                        vm.err.deferred = Some(e);
                    }
                    return Err(std::io::Error::other("stream filter error"));
                }
            }
            if closing {
                self.filters_closed = true;
            }
        }
        let n = buf.len().min(self.filtered.len());
        buf[..n].copy_from_slice(&self.filtered[..n]);
        self.filtered.drain(..n);
        Ok(n)
    }

    /// Reads from the backing, before the read filters.
    pub fn read_raw(&mut self, vm: &mut Vm, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.pending.is_empty()
            && matches!(self.backing, Backing::User { .. })
            && let Err(e) = super::userstream::fill(vm, self)
        {
            if vm.err.deferred.is_none() {
                vm.err.deferred = Some(e);
            }
            return Err(std::io::Error::other("user stream error"));
        }
        if !self.pending.is_empty() {
            let n = buf.len().min(self.pending.len());
            buf[..n].copy_from_slice(&self.pending[..n]);
            self.pending.drain(..n);
            self.upos += n as i64;
            return Ok(n);
        }
        if !self.pending.is_empty() {
            let n = buf.len().min(self.pending.len());
            buf[..n].copy_from_slice(&self.pending[..n]);
            self.pending.drain(..n);
            return Ok(n);
        }
        match &mut self.backing {
            Backing::File(f) => {
                let n = f.read(buf)?;
                if self.append {
                    self.upos += n as i64;
                }
                Ok(n)
            }
            Backing::Memory { data, pos } => {
                // a position past the end (after fseek) reads nothing
                // (test bug52335.phpt)
                let n = buf.len().min(data.len().saturating_sub(*pos));
                if n > 0 {
                    buf[..n].copy_from_slice(&data[*pos..*pos + n]);
                }
                *pos += n;
                // a short read sets EOF (test stream_rfc2397_007.phpt)
                if n < buf.len() {
                    self.eof = true;
                }
                Ok(n)
            }
            Backing::Stdin => std::io::stdin().read(buf),
            Backing::Native(n) => n.read(vm, buf),
            _ => Ok(0),
        }
    }

    pub fn write_all(&mut self, vm: &mut Vm, b: &[u8]) -> std::io::Result<usize> {
        if self.write_filters.is_empty() {
            return self.write_raw(vm, b);
        }
        let mut chain = std::mem::take(&mut self.write_filters);
        let r = super::streams::run_chain(vm, &mut chain, b, false);
        if self.write_filters.is_empty() {
            self.write_filters = chain;
        }
        match r {
            Ok(Some(out)) => {
                if !out.is_empty() {
                    self.write_raw(vm, &out)?;
                }
                Ok(b.len())
            }
            Ok(None) => Err(std::io::Error::other("stream filter failed")),
            Err(e) => {
                if vm.err.deferred.is_none() {
                    vm.err.deferred = Some(e);
                }
                Err(std::io::Error::other("stream filter error"))
            }
        }
    }

    /// Writes to the backing, after the write filters.
    pub fn write_raw(&mut self, vm: &mut Vm, b: &[u8]) -> std::io::Result<usize> {
        if matches!(self.backing, Backing::User { .. }) {
            return match super::userstream::write(vm, self, b) {
                Ok(n) => {
                    self.upos += n as i64;
                    Ok(n)
                }
                Err(e) => {
                    if vm.err.deferred.is_none() {
                        vm.err.deferred = Some(e);
                    }
                    Err(std::io::Error::other("user stream error"))
                }
            };
        }
        match &mut self.backing {
            Backing::File(f) => {
                if self.append {
                    f.seek(SeekFrom::End(0))?;
                }
                f.write_all(b)?;
                if self.append {
                    self.upos += b.len() as i64;
                }
                Ok(b.len())
            }
            Backing::Memory { data, pos } => {
                if self.append {
                    *pos = data.len();
                }
                let end = *pos + b.len();
                if data.len() < end {
                    data.resize(end, 0);
                }
                data[*pos..end].copy_from_slice(b);
                *pos = end;
                Ok(b.len())
            }
            Backing::Stdout => {
                vm.out.flush();
                let mut so = std::io::stdout().lock();
                so.write_all(b)?;
                so.flush()?;
                Ok(b.len())
            }
            Backing::Stderr => {
                vm.out.flush();
                std::io::stderr().write_all(b)?;
                Ok(b.len())
            }
            Backing::Output => {
                vm.echo(b);
                Ok(b.len())
            }
            Backing::Native(n) => n.write(vm, b),
            _ => Ok(0),
        }
    }
}

/// Whether a resource is a (possibly closed) stream.
pub(crate) fn is_stream_res(r: &ResRef) -> bool {
    let data = r.res().data.borrow();
    data.as_ref()
        .is_some_and(|d| d.downcast_ref::<std::cell::RefCell<Stream>>().is_some())
}

pub fn new_stream_res(s: Stream) -> ResRef {
    let r = ResRef::new(
        PhpStr::intern(b"stream"),
        Box::new(std::cell::RefCell::new(s)),
    );
    OPEN_STREAMS.with(|l| {
        let mut l = l.borrow_mut();
        if l.len() >= 64 && l.len().is_power_of_two() {
            l.retain(|w| !w.is_dead());
        }
        l.push(r.downgrade());
    });
    r
}

thread_local! {
    /// While read filters run: whether the stream reached its end (what
    /// feof() on it reports from inside the filter; test
    /// streams/bug79984.phpt).
    static FILTER_EOF: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) };
    /// The streams opened, to close them at the end of the request.
    static OPEN_STREAMS: std::cell::RefCell<Vec<goro_runtime::object::WeakRes>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// Closes the streams still open at the end of the request, so that
/// their write filters finish (observed on PHP 8.5: zlib.deflate output
/// is complete without fclose()).
pub fn close_open_streams(vm: &mut Vm) {
    let list = OPEN_STREAMS.with(|l| std::mem::take(&mut *l.borrow_mut()));
    for w in list {
        let Some(r) = w.upgrade() else {
            continue;
        };
        let data = r.res().data.borrow();
        let Some(d) = data.as_ref() else {
            continue;
        };
        let Some(cell) = d.downcast_ref::<std::cell::RefCell<Stream>>() else {
            continue;
        };
        let Ok(mut s) = cell.try_borrow_mut() else {
            continue;
        };
        if s.write_filters.is_empty() {
            continue;
        }
        let r = super::streams::close_stream(vm, &mut s);
        if let Err(e) = r {
            drop(s);
            drop(data);
            vm.handle_unwind(Err(e));
        }
    }
}

/// STDIN, STDOUT and STDERR (created first: resource ids 1, 2, 3).
pub fn stdio_resources() -> (ResRef, ResRef, ResRef) {
    let stdin = new_stream_res(Stream::new(
        Backing::Stdin,
        b"r",
        b"php://stdin",
        "PHP",
        "STDIO",
    ));
    let stdout = new_stream_res(Stream::new(
        Backing::Stdout,
        b"w",
        b"php://stdout",
        "PHP",
        "STDIO",
    ));
    let stderr = new_stream_res(Stream::new(
        Backing::Stderr,
        b"w",
        b"php://stderr",
        "PHP",
        "STDIO",
    ));
    (stdin, stdout, stderr)
}

fn init(vm: &mut Vm) {
    let r = &vm.resources;
    let (i, o, e) = (r.stdin.clone(), r.stdout.clone(), r.stderr.clone());
    vm.constants
        .insert(PhpStr::intern(b"STDIN"), Value::Resource(i.unwrap()));
    vm.constants
        .insert(PhpStr::intern(b"STDOUT"), Value::Resource(o.unwrap()));
    vm.constants
        .insert(PhpStr::intern(b"STDERR"), Value::Resource(e.unwrap()));
}

/// The stream of a resource argument (TypeError otherwise).
pub fn with_stream<T>(
    vm: &mut Vm,
    a: &Args,
    i: usize,
    f: impl FnOnce(&mut Vm, &mut Stream) -> R<T>,
) -> R<T> {
    let v = a.val(i);
    let Value::Resource(r) = &v else {
        return vm.fthrow(
            a,
            "TypeError",
            &format!(
                "Argument #{} ($stream) must be of type resource, {} given",
                i + 1,
                crate::vm::Vm::type_name_given(&v)
            ),
        );
    };
    let data = r.res().data.borrow();
    let Some(d) = data.as_ref() else {
        drop(data);
        return vm.fthrow(
            a,
            "TypeError",
            &format!(
                "Argument #{} ($stream) must be an open stream resource",
                i + 1
            ),
        );
    };
    let Some(cell) = d.downcast_ref::<std::cell::RefCell<Stream>>() else {
        drop(data);
        return vm.fthrow(
            a,
            "TypeError",
            &format!(
                "Argument #{} ($stream) must be an open stream resource",
                i + 1
            ),
        );
    };
    // SAFETY: the resource stays alive (held by `v`) for the call.
    let cell: *const std::cell::RefCell<Stream> = cell;
    drop(data);
    let Ok(mut s) = (unsafe { (*cell).try_borrow_mut() }) else {
        return vm.fthrow(a, "Error", "Cannot use a stream while it is being filtered");
    };
    // Output is written before blocking on standard input (PHP's CLI
    // output is unbuffered; a parent may wait for it).
    if matches!(s.backing, Backing::Stdin) {
        vm.out.flush();
    }
    f(vm, &mut s)
}

fn os(b: &[u8]) -> &Path {
    Path::new(std::ffi::OsStr::from_bytes(b))
}

impl Vm {
    /// A path for filesystem calls: relative paths resolve against the
    /// current directory (chdir changes it).
    pub fn fs_path(&self, p: &[u8]) -> PathBuf {
        let p = os(p);
        // An empty path names nothing (ENOENT), not the working directory.
        if p.as_os_str().is_empty() || p.is_absolute() {
            p.to_path_buf()
        } else {
            self.cwd.join(p)
        }
    }

    /// A path for opening a plain file stream: ".." after a directory that
    /// doesn't exist removes it, as PHP resolves these paths itself (test
    /// file_variation5.phpt: "dir/bad_dir/../../file" opens "file"; other
    /// filesystem calls such as chmod() fail, test chmod_variation2.phpt).
    pub fn stream_path(&self, p: &[u8]) -> PathBuf {
        let full = self.fs_path(p);
        use std::path::Component;
        if !os(p).components().any(|c| c == Component::ParentDir) {
            return full;
        }
        let mut out = PathBuf::new();
        for c in full.components() {
            if c == Component::ParentDir && !out.exists() && out.parent().is_some() {
                out.pop();
            } else {
                out.push(c.as_os_str());
            }
        }
        out
    }
}

/// The C library's text of an I/O error (as PHP reports them).
fn io_err_msg(e: &std::io::Error) -> String {
    match e.raw_os_error() {
        Some(n) => crate::builtins::exec::strerror(n),
        None => match e.kind() {
            std::io::ErrorKind::NotFound => "No such file or directory".into(),
            std::io::ErrorKind::PermissionDenied => "Permission denied".into(),
            std::io::ErrorKind::AlreadyExists => "File exists".into(),
            _ => "Operation failed".into(),
        },
    }
}

/// A URL whose protocol has no wrapper is opened as a local path, with a
/// warning (test ext/standard/tests/streams/bug76857.phpt).
fn unknown_wrapper_warn(vm: &mut Vm, a: &Args, path: &[u8]) -> R<()> {
    let Some(sch) = super::userstream::scheme(path) else {
        return Ok(());
    };
    if matches!(&sch[..], b"php" | b"file" | b"data" | b"glob")
        || super::streams::wrapper_for(vm, path).is_some()
        || super::userstream::wrapper_for(vm, path).is_some()
    {
        return Ok(());
    }
    let p = path.windows(3).position(|w| w == b"://").unwrap_or(0);
    vm.warn(&format!(
        "{}(): Unable to find the wrapper \"{}\" - did you forget to enable it when you configured PHP?",
        a.fname(),
        String::from_utf8_lossy(&path[..p])
    ))
}

/// Opens `filename` with PHP's fopen modes.
pub fn open_stream(
    vm: &mut Vm,
    a: &Args,
    filename: &[u8],
    mode: &[u8],
    quiet: bool,
) -> R<Option<Stream>> {
    let fname = a.fname();
    if filename.is_empty() {
        return vm.throw("ValueError", "Path must not be empty");
    }
    if let Some(cls) = super::userstream::wrapper_for(vm, filename) {
        return super::userstream::open(vm, a, &cls, filename, mode, quiet);
    }
    if let Some(w) = super::streams::wrapper_for(vm, filename) {
        return (w.open)(vm, a, filename, mode, quiet);
    }
    if super::userstream::disabled(vm, filename) {
        if !quiet {
            let scheme = filename
                .windows(3)
                .position(|w| w == b"://")
                .map(|p| String::from_utf8_lossy(&filename[..p]).into_owned())
                .unwrap_or_else(|| "file".into());
            vm.warn(&format!(
                "{}(): Unable to find the wrapper \"{}\" - did you forget to enable it when you configured PHP?",
                fname, scheme
            ))?;
            vm.warn(&format!(
                "{}({}): Failed to open stream: No such file or directory",
                fname,
                String::from_utf8_lossy(filename)
            ))?;
        }
        return Ok(None);
    }
    if filename.len() >= 5 && filename[..5].eq_ignore_ascii_case(b"data:") {
        return open_data_url(vm, a, filename, mode, quiet);
    }
    if !quiet {
        unknown_wrapper_warn(vm, a, filename)?;
    }
    if let Some(rest) = filename.strip_prefix(b"php://") {
        if rest.len() >= 7 && rest[..7].eq_ignore_ascii_case(b"filter/") {
            return super::streams::open_php_filter(vm, a, &rest[7..], mode, quiet);
        }
        let lower = rest.to_ascii_lowercase();
        let s = match &lower[..] {
            b"stdin" => Stream::new(Backing::Stdin, mode, filename, "PHP", "STDIO"),
            b"stdout" => Stream::new(Backing::Stdout, mode, filename, "PHP", "STDIO"),
            b"stderr" => Stream::new(Backing::Stderr, mode, filename, "PHP", "STDIO"),
            b"output" => Stream::new(Backing::Output, b"wb", filename, "PHP", "Output"),
            l if l.starts_with(b"fd/") => {
                // php://fd/N: a duplicate of descriptor N (PHP manual:
                // php:// wrappers); messages observed on PHP 8.5.
                let n = std::str::from_utf8(&l[3..])
                    .ok()
                    .filter(|d| !d.is_empty() && d.bytes().all(|c| c.is_ascii_digit()))
                    .and_then(|d| d.parse::<i32>().ok());
                let neg = l[3..]
                    .strip_prefix(b"-")
                    .is_some_and(|d| !d.is_empty() && d.iter().all(|c| c.is_ascii_digit()));
                if neg {
                    if !quiet {
                        let mut rl = libc::rlimit {
                            rlim_cur: 0,
                            rlim_max: 0,
                        };
                        unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut rl) };
                        vm.warn(&format!(
                            "{}({}): Failed to open stream: The file descriptors must be non-negative numbers smaller than {}",
                            fname,
                            String::from_utf8_lossy(filename),
                            rl.rlim_cur
                        ))?;
                    }
                    return Ok(None);
                }
                let Some(n) = n else {
                    if !quiet {
                        vm.warn(&format!(
                            "{}({}): Failed to open stream: php://fd/ stream must be specified in the form php://fd/<orig fd>",
                            fname,
                            String::from_utf8_lossy(filename)
                        ))?;
                    }
                    return Ok(None);
                };
                // SAFETY: dup() of a plain descriptor number.
                let fd = unsafe { libc::dup(n) };
                if fd < 0 {
                    if !quiet {
                        let e = std::io::Error::last_os_error();
                        vm.warn(&format!(
                            "{}({}): Failed to open stream: Error duping file descriptor {}; possibly it doesn't exist: [{}]: {}",
                            fname,
                            String::from_utf8_lossy(filename),
                            n,
                            e.raw_os_error().unwrap_or(0),
                            crate::builtins::exec::strerror(e.raw_os_error().unwrap_or(0))
                        ))?;
                    }
                    return Ok(None);
                }
                use std::os::fd::FromRawFd;
                // SAFETY: fd is a fresh descriptor owned by the File.
                let f = unsafe { std::fs::File::from_raw_fd(fd) };
                let mut s = Stream::new(Backing::File(f), mode, filename, "PHP", "STDIO");
                // the mode is ignored (test php_fd_wrapper_02.phpt)
                s.readable = true;
                s.writable = true;
                s
            }
            b"input" => Stream::new(
                Backing::Memory {
                    data: vm.out.sapi.input.clone(),
                    pos: 0,
                },
                b"rb",
                filename,
                "PHP",
                "Input",
            ),
            l if l == b"memory" || l.starts_with(b"temp") => {
                let st = if l == b"memory" { "MEMORY" } else { "TEMP" };
                let mut m = mode.to_vec();
                if !m.contains(&b'b') {
                    m.push(b'b');
                }
                let mut s = Stream::new(
                    Backing::Memory {
                        data: Vec::new(),
                        pos: 0,
                    },
                    &m,
                    filename,
                    "PHP",
                    st,
                );
                s.readable = true;
                s.writable = !mode.iter().all(|c| matches!(c, b'r' | b'b' | b't'));
                s
            }
            _ => {
                if !quiet {
                    vm.warn(&format!("{}(): Invalid php:// URL specified", fname))?;
                    vm.warn(&format!(
                        "{}({}): Failed to open stream: operation failed",
                        fname,
                        String::from_utf8_lossy(filename)
                    ))?;
                }
                return Ok(None);
            }
        };
        return Ok(Some(s));
    }
    if filename.contains(&0) {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #1 ($filename) must not contain any null bytes",
        );
    }
    // the scheme is case-insensitive (test stream_get_meta_data_file_variation4.phpt)
    let path = if filename.len() >= 7 && filename[..7].eq_ignore_ascii_case(b"file://") {
        &filename[7..]
    } else {
        filename
    };
    if path.len() != filename.len() && !path.starts_with(b"/") {
        if !quiet {
            vm.warn(&format!(
                "{}(): Remote host file access not supported, {}",
                fname,
                String::from_utf8_lossy(filename)
            ))?;
            vm.warn(&format!(
                "{}({}): Failed to open stream: no suitable wrapper could be found",
                fname,
                String::from_utf8_lossy(filename)
            ))?;
        }
        return Ok(None);
    }
    if !vm.basedir_allows(path) {
        if !quiet {
            vm.warn(&format!(
                "{}(): open_basedir restriction in effect. File({}) is not within the allowed path(s): ({})",
                fname,
                String::from_utf8_lossy(path),
                vm.get_ini(b"open_basedir").map(|v| v.to_string_lossy()).unwrap_or_default()
            ))?;
            vm.warn(&format!(
                "{}({}): Failed to open stream: Operation not permitted",
                fname,
                String::from_utf8_lossy(filename)
            ))?;
        }
        return Ok(None);
    }
    // The first letter is the mode, a '+' anywhere adds the other
    // direction, other letters are ignored (observed on PHP 8.5: "rw"
    // opens for reading, "Q" warns).
    let plus = mode.contains(&b'+');
    let m: Vec<u8> = match mode.first() {
        Some(&c @ (b'r' | b'w' | b'a' | b'x' | b'c')) if plus => vec![c, b'+'],
        Some(&c @ (b'r' | b'w' | b'a' | b'x' | b'c')) => vec![c],
        _ => {
            if !quiet {
                vm.warn(&format!(
                    "{}({}): Failed to open stream: `{}' is not a valid mode for fopen",
                    fname,
                    String::from_utf8_lossy(filename),
                    String::from_utf8_lossy(mode)
                ))?;
            }
            return Ok(None);
        }
    };
    let mut o = std::fs::OpenOptions::new();
    match &m[..] {
        b"r" => o.read(true),
        b"r+" => o.read(true).write(true),
        b"w" => o.write(true).create(true).truncate(true),
        b"w+" => o.read(true).write(true).create(true).truncate(true),
        b"a" => o.append(true).create(true),
        b"a+" => o.read(true).append(true).create(true),
        b"x" => o.write(true).create_new(true),
        b"x+" => o.read(true).write(true).create_new(true),
        b"c" => o.write(true).create(true),
        _ => o.read(true).write(true).create(true),
    };
    let p = vm.stream_path(path);
    if p.is_dir() && m.first() != Some(&b'r') {
        if !quiet {
            vm.warn(&format!(
                "{}({}): Failed to open stream: Is a directory",
                fname,
                String::from_utf8_lossy(filename)
            ))?;
        }
        return Ok(None);
    }
    match o.open(&p) {
        Ok(f) => {
            let mut s = Stream::new(Backing::File(f), mode, filename, "plainfile", "STDIO");
            s.append = m.first() == Some(&b'a');
            Ok(Some(s))
        }
        Err(e) => {
            if !quiet {
                vm.warn(&format!(
                    "{}({}): Failed to open stream: {}",
                    fname,
                    String::from_utf8_lossy(filename),
                    io_err_msg(&e)
                ))?;
            }
            Ok(None)
        }
    }
}

/// A file looked up through include_path (`$use_include_path`): the
/// path to open (PHP manual: fopen(), file_get_contents()).
fn with_include_path(vm: &Vm, f: &PhpStr, use_it: bool) -> PhpStr {
    if !use_it || f.windows(3).any(|w| w == b"://") {
        return f.clone();
    }
    match vm.resolve_include(f) {
        Some(p) => PhpStr::new(p.as_os_str().as_bytes()),
        None => f.clone(),
    }
}

fn fopen(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let m = a.str(1);
    let f = with_include_path(
        vm,
        &a.str(0),
        a.bool_or(2, false) && m.first() == Some(&b'r'),
    );
    match open_stream(vm, a, &f, &m, false)? {
        Some(s) => Ok(Value::Resource(new_stream_res(s))),
        None => Ok(Value::Bool(false)),
    }
}

fn fclose(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let v = a.val(0);
    let Value::Resource(r) = &v else {
        return vm.fthrow(
            a,
            "TypeError",
            &format!(
                "Argument #1 ($stream) must be of type resource, {} given",
                crate::vm::Vm::type_name_given(&v)
            ),
        );
    };
    if r.is_closed() {
        return vm.fthrow(
            a,
            "TypeError",
            "Argument #1 ($stream) must be an open stream resource",
        );
    }
    let is_std = [
        &vm.resources.stdin,
        &vm.resources.stdout,
        &vm.resources.stderr,
    ]
    .iter()
    .any(|x| x.as_ref().is_some_and(|x| ResRef::ptr_eq(x, r)));
    let _ = is_std;
    let user = {
        let data = r.res().data.borrow();
        data.as_ref()
            .and_then(|d| d.downcast_ref::<std::cell::RefCell<Stream>>())
            .and_then(|c| match c.try_borrow().ok()?.backing {
                Backing::User { ref obj } => Some(obj.clone()),
                _ => None,
            })
    };
    if let Some(o) = user {
        super::userstream::call(vm, &o, b"stream_flush", Vec::new())?;
        super::userstream::call(vm, &o, b"stream_close", Vec::new())?;
    }
    let cell = {
        let data = r.res().data.borrow();
        data.as_ref()
            .and_then(|d| d.downcast_ref::<std::cell::RefCell<Stream>>())
            .map(|c| c as *const std::cell::RefCell<Stream>)
    };
    if let Some(cell) = cell {
        // SAFETY: `v` holds the resource for the call.
        let Ok(mut s) = (unsafe { (*cell).try_borrow_mut() }) else {
            // In use (a filter of the stream closing it, test
            // ext/standard/tests/filters/bug54350.phpt).
            vm.fwarn(
                a,
                "cannot close the provided stream, as it must not be manually closed",
            )?;
            return Ok(Value::Bool(false));
        };
        super::streams::close_stream(vm, &mut s)?;
    }
    r.close();
    Ok(Value::Bool(true))
}

fn fread(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let n = a.int(1);
    if n <= 0 {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #2 ($length) must be greater than 0",
        );
    }
    with_stream(vm, a, 0, |vm, s| {
        if !s.readable {
            vm.notice_msg(&format!(
                "fread(): Read of {} bytes failed with errno=9 Bad file descriptor",
                8192
            ))?;
            return Ok(Value::Bool(false));
        }
        let mut buf = vec![0u8; n as usize];
        let mut got = 0;
        while got < buf.len() {
            match s.read_some(vm, &mut buf[got..]) {
                Ok(0) => {
                    s.eof = true;
                    break;
                }
                Ok(k) => {
                    got += k;
                    if matches!(s.backing, Backing::Stdin) || s.is_pipe() {
                        break;
                    }
                }
                // nothing read and a failed filter: false (test
                // ext/zlib/tests/bug71417.phpt)
                Err(e) if got == 0 && e.to_string() == "stream filter failed" => {
                    return Ok(Value::Bool(false));
                }
                // a socket read timing out with nothing read (observed on
                // PHP 8.5)
                Err(e) if got == 0 && e.kind() == std::io::ErrorKind::TimedOut => {
                    return Ok(Value::Bool(false));
                }
                // a failed socket read (a refused UDP datagram; test
                // ext/standard/tests/network/bug20134.phpt)
                Err(e)
                    if got == 0
                        && matches!(&s.backing, Backing::Native(n) if n.is_socket())
                        && e.kind() != std::io::ErrorKind::WouldBlock =>
                {
                    return Ok(Value::Bool(false));
                }
                Err(_) => break,
            }
        }
        buf.truncate(got);
        Ok(Value::Str(PhpStr::from(buf)))
    })
}

fn fwrite(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let data = a.str(1);
    let len = match a.opt_int(2) {
        Some(l) if l >= 0 => (l as usize).min(data.len()),
        Some(_) => 0,
        None => data.len(),
    };
    with_stream(vm, a, 0, |vm, s| {
        if !s.writable {
            if !matches!(s.backing, Backing::Memory { .. }) {
                vm.notice_msg(&format!(
                    "fwrite(): Write of {} bytes failed with errno=9 Bad file descriptor",
                    len
                ))?;
            }
            return Ok(Value::Bool(false));
        }
        match s.write_all(vm, &data[..len]) {
            Ok(n) => Ok(Value::Int(n as i64)),
            Err(_) => Ok(Value::Bool(false)),
        }
    })
}

fn read_line(vm: &mut Vm, s: &mut Stream, max: Option<usize>) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut b = [0u8; 1];
    // auto_detect_line_endings: a lone CR ends a line too (PHP manual:
    // Filesystem runtime configuration)
    let cr = vm
        .get_ini(b"auto_detect_line_endings")
        .is_some_and(|v| super::misc::assert_ini_truthy(&v));
    loop {
        if let Some(m) = max
            && out.len() >= m
        {
            break;
        }
        match s.read_some(vm, &mut b) {
            Ok(1) => {
                out.push(b[0]);
                if b[0] == b'\n' {
                    break;
                }
                if cr && b[0] == b'\r' {
                    let mut n = [0u8; 1];
                    if let Ok(1) = s.read_some(vm, &mut n) {
                        if n[0] == b'\n' {
                            out.push(b'\n');
                        } else if !s.read_filters.is_empty() {
                            s.filtered.insert(0, n[0]);
                        } else {
                            s.pending.insert(0, n[0]);
                            s.upos -= 1;
                        }
                    }
                    break;
                }
            }
            // a non-blocking or timed-out socket read is not the end
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                break;
            }
            _ => {
                s.eof = true;
                break;
            }
        }
    }
    if out.is_empty() { None } else { Some(out) }
}

fn fgets(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let max = a.opt_int(1);
    if let Some(m) = max
        && m <= 0
    {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #2 ($length) must be greater than 0",
        );
    }
    with_stream(vm, a, 0, |vm, s| {
        if !s.readable {
            vm.notice_msg("fgets(): Read of 8192 bytes failed with errno=9 Bad file descriptor")?;
            return Ok(Value::Bool(false));
        }
        if max == Some(1) {
            return Ok(Value::Bool(false));
        }
        let r = read_line(vm, s, max.map(|m| (m as usize).saturating_sub(1)));
        // Detect EOF right after the last line for memory/file streams.
        if let Backing::Memory { data, pos } = &s.backing
            && *pos >= data.len()
        {
            s.eof = true;
        }
        Ok(match r {
            Some(l) => Value::Str(PhpStr::from(l)),
            None => Value::Bool(false),
        })
    })
}

fn fgetc(vm: &mut Vm, a: &mut Args) -> R<Value> {
    with_stream(vm, a, 0, |vm, s| {
        if !s.readable {
            vm.notice_msg("fgetc(): Read of 8192 bytes failed with errno=9 Bad file descriptor")?;
            return Ok(Value::Bool(false));
        }
        let mut b = [0u8; 1];
        match s.read_some(vm, &mut b) {
            Ok(1) => Ok(Value::Str(PhpStr::from_byte(b[0]))),
            _ => {
                s.eof = true;
                Ok(Value::Bool(false))
            }
        }
    })
}

fn feof(vm: &mut Vm, a: &mut Args) -> R<Value> {
    if let Value::Resource(r) = a.val(0)
        && let Some(eof) = FILTER_EOF.with(|c| c.get())
    {
        let data = r.res().data.borrow();
        let busy = data
            .as_ref()
            .and_then(|d| d.downcast_ref::<std::cell::RefCell<Stream>>())
            .is_some_and(|c| c.try_borrow_mut().is_err());
        if busy {
            return Ok(Value::Bool(eof));
        }
    }
    with_stream(vm, a, 0, |_vm, s| {
        if matches!(s.backing, Backing::User { .. }) {
            return Ok(Value::Bool(s.eof && s.pending.is_empty()));
        }
        let at_end = match &mut s.backing {
            Backing::Memory { .. } => s.eof,
            Backing::File(f) => {
                if s.eof {
                    true
                } else {
                    let cur = f.stream_position().unwrap_or(0);
                    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
                    cur >= len && s.eof
                }
            }
            Backing::Native(n) => s.eof || n.eof(),
            _ => s.eof,
        };
        if !s.read_filters.is_empty() {
            return Ok(Value::Bool(
                s.filtered.is_empty() && (s.filters_closed || s.eof),
            ));
        }
        Ok(Value::Bool(
            at_end && s.eof
                || matches!(s.backing, Backing::Native(_)) && at_end && s.pending.is_empty(),
        ))
    })
}

/// fsync(), fdatasync(): plain files and descriptors only.
fn fsync(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let data_only = a.fname() == "fdatasync";
    with_stream(vm, a, 0, |vm, s| {
        let _ = s.write_all(vm, b"");
        let fd = match &s.backing {
            Backing::File(f) => {
                use std::os::fd::AsRawFd;
                f.as_raw_fd()
            }
            Backing::Stdin => 0,
            Backing::Stdout => 1,
            Backing::Stderr => 2,
            _ => {
                vm.fwarn(a, "Can't fsync this stream!")?;
                return Ok(Value::Bool(false));
            }
        };
        vm.out.flush();
        let rc = unsafe {
            if data_only {
                libc::fdatasync(fd)
            } else {
                libc::fsync(fd)
            }
        };
        Ok(Value::Bool(rc == 0))
    })
}

fn fflush(vm: &mut Vm, a: &mut Args) -> R<Value> {
    with_stream(vm, a, 0, |vm, s| {
        super::streams::flush_write_filters(vm, s)?;
        match &mut s.backing {
            Backing::File(f) => {
                let _ = f.flush();
            }
            Backing::Stdout | Backing::Output => vm.out.flush(),
            Backing::Native(n) => {
                return Ok(Value::Bool(n.flush(vm).is_ok()));
            }
            Backing::User { .. } => {
                let r = super::userstream::simple(vm, s, b"stream_flush", Vec::new())?;
                return Ok(Value::Bool(r.is_some_and(|v| vm.to_bool(&v))));
            }
            _ => {}
        }
        Ok(Value::Bool(true))
    })
}

fn ftell(vm: &mut Vm, a: &mut Args) -> R<Value> {
    with_stream(vm, a, 0, |_vm, s| {
        Ok(match &mut s.backing {
            Backing::File(_) if s.append => Value::Int(s.upos),
            Backing::File(f) => Value::Int(f.stream_position().map(|p| p as i64).unwrap_or(0)),
            Backing::Memory { pos, .. } => Value::Int(*pos as i64),
            Backing::User { .. } => Value::Int(s.upos),
            Backing::Native(n) => match n.tell() {
                Some(p) => Value::Int(p as i64),
                None => Value::Bool(false),
            },
            _ => Value::Bool(false),
        })
    })
}

fn do_seek(vm: &mut Vm, s: &mut Stream, off: i64, whence: i64) -> bool {
    if matches!(s.backing, Backing::User { .. }) {
        return match super::userstream::seek(vm, s, off, whence) {
            Ok(b) => b,
            Err(e) => {
                if vm.err.deferred.is_none() {
                    vm.err.deferred = Some(e);
                }
                false
            }
        };
    }
    if !(0..=2).contains(&whence) {
        return false;
    }
    // a directory stream rewinds, whatever the offset (test
    // fseek_dir_basic.phpt)
    if let Backing::Dir { pos, .. } = &mut s.backing {
        *pos = 0;
        return true;
    }
    s.eof = false;
    s.pending.clear();
    // Seeking flushes the write filters (observed on PHP 8.5).
    if !s.write_filters.is_empty()
        && let Err(e) = super::streams::flush_write_filters(vm, s)
    {
        if vm.err.deferred.is_none() {
            vm.err.deferred = Some(e);
        }
        return false;
    }
    match &mut s.backing {
        Backing::File(f) => {
            let w = match whence {
                1 => SeekFrom::Current(off),
                2 => SeekFrom::End(off),
                _ => {
                    if off < 0 {
                        return false;
                    }
                    SeekFrom::Start(off as u64)
                }
            };
            match f.seek(w) {
                Ok(p) => {
                    s.upos = p as i64;
                    true
                }
                Err(_) => false,
            }
        }
        Backing::Memory { data, pos } => {
            let base = match whence {
                1 => *pos as i64,
                2 => data.len() as i64,
                _ => 0,
            };
            let np = base + off;
            if np < 0 {
                return false;
            }
            *pos = np as usize;
            true
        }
        Backing::Native(n) => {
            let w = match whence {
                1 => SeekFrom::Current(off),
                2 => SeekFrom::End(off),
                _ => {
                    if off < 0 {
                        return false;
                    }
                    SeekFrom::Start(off as u64)
                }
            };
            matches!(n.seek(vm, w), Some(Ok(_)))
        }
        _ => false,
    }
}

fn fseek(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let off = a.int(1);
    let wh = a.int_or(2, 0);
    with_stream(vm, a, 0, |vm, s| {
        if matches!(
            s.backing,
            Backing::Stdin | Backing::Stdout | Backing::Stderr | Backing::Output
        ) || s.is_pipe()
        {
            vm.warn("fseek(): Stream does not support seeking")?;
            return Ok(Value::Int(-1));
        }
        Ok(Value::Int(if do_seek(vm, s, off, wh) { 0 } else { -1 }))
    })
}

fn rewind(vm: &mut Vm, a: &mut Args) -> R<Value> {
    with_stream(vm, a, 0, |vm, s| Ok(Value::Bool(do_seek(vm, s, 0, 0))))
}

fn ftruncate(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let size = a.int(1);
    if size < 0 {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #2 ($size) must be greater than or equal to 0",
        );
    }
    with_stream(vm, a, 0, |vm, s| {
        Ok(Value::Bool(match &mut s.backing {
            Backing::File(f) => f.set_len(size as u64).is_ok(),
            Backing::Memory { data, pos } => {
                data.resize(size as usize, 0);
                *pos = (*pos).min(data.len());
                true
            }
            Backing::User { .. } => {
                let r =
                    super::userstream::simple(vm, s, b"stream_truncate", vec![Value::Int(size)])?;
                match r {
                    None => {
                        vm.warn("ftruncate(): Can't truncate this stream!")?;
                        false
                    }
                    Some(Value::Bool(b)) => b,
                    Some(v) => {
                        let c = super::userstream::class_of_stream(vm, s);
                        vm.warn(&format!(
                            "ftruncate(): {}::stream_truncate value must be of type bool, {} given",
                            c,
                            crate::class::TypeName::tname(&v)
                        ))?;
                        false
                    }
                }
            }
            _ => false,
        }))
    })
}

fn stat_array(m: &std::fs::Metadata) -> PhpArray {
    let vals: [(&str, i64); 13] = [
        ("dev", m.dev() as i64),
        ("ino", m.ino() as i64),
        ("mode", m.mode() as i64),
        ("nlink", m.nlink() as i64),
        ("uid", m.uid() as i64),
        ("gid", m.gid() as i64),
        ("rdev", m.rdev() as i64),
        ("size", m.size() as i64),
        ("atime", m.atime()),
        ("mtime", m.mtime()),
        ("ctime", m.ctime()),
        ("blksize", m.blksize() as i64),
        ("blocks", m.blocks() as i64),
    ];
    let mut a = PhpArray::new();
    for (_, v) in vals.iter() {
        let _ = a.push(Value::Int(*v));
    }
    for (k, v) in vals.iter() {
        a.insert(Key::from(*k), Value::Int(*v));
    }
    a
}

fn fstat(vm: &mut Vm, a: &mut Args) -> R<Value> {
    with_stream(vm, a, 0, |vm, s| {
        if matches!(s.backing, Backing::User { .. }) {
            return Ok(
                match super::userstream::simple(vm, s, b"stream_stat", Vec::new())? {
                    Some(Value::Array(st)) => Value::Array(stat_from_user(&st)),
                    _ => Value::Bool(false),
                },
            );
        }
        Ok(match &s.backing {
            Backing::File(f) => match f.metadata() {
                Ok(m) => Value::Array(stat_array(&m)),
                Err(_) => Value::Bool(false),
            },
            Backing::Memory { data, .. } => {
                let mut arr = PhpArray::new();
                let vals: [(&str, i64); 13] = [
                    ("dev", 12),
                    ("ino", 0),
                    ("mode", 33206),
                    ("nlink", 1),
                    ("uid", 0),
                    ("gid", 0),
                    ("rdev", -1),
                    ("size", data.len() as i64),
                    ("atime", 0),
                    ("mtime", 0),
                    ("ctime", 0),
                    ("blksize", -1),
                    ("blocks", -1),
                ];
                for (_, v) in vals.iter() {
                    let _ = arr.push(Value::Int(*v));
                }
                for (k, v) in vals.iter() {
                    arr.insert(Key::from(*k), Value::Int(*v));
                }
                Value::Array(arr)
            }
            Backing::Stdin | Backing::Stdout | Backing::Stderr => {
                let fd = match s.backing {
                    Backing::Stdin => 0,
                    Backing::Stdout => 1,
                    _ => 2,
                };
                let mut st: libc::stat = unsafe { std::mem::zeroed() };
                if unsafe { libc::fstat(fd, &mut st) } == 0 {
                    let mut arr = PhpArray::new();
                    let vals: [(&str, i64); 13] = [
                        ("dev", st.st_dev as i64),
                        ("ino", st.st_ino as i64),
                        ("mode", st.st_mode as i64),
                        ("nlink", st.st_nlink as i64),
                        ("uid", st.st_uid as i64),
                        ("gid", st.st_gid as i64),
                        ("rdev", st.st_rdev as i64),
                        ("size", st.st_size),
                        ("atime", st.st_atime),
                        ("mtime", st.st_mtime),
                        ("ctime", st.st_ctime),
                        ("blksize", st.st_blksize),
                        ("blocks", st.st_blocks),
                    ];
                    for (_, v) in vals.iter() {
                        let _ = arr.push(Value::Int(*v));
                    }
                    for (k, v) in vals.iter() {
                        arr.insert(Key::from(*k), Value::Int(*v));
                    }
                    Value::Array(arr)
                } else {
                    Value::Bool(false)
                }
            }
            _ => Value::Bool(false),
        })
    })
}

fn flock(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let op = a.int(1);
    with_stream(vm, a, 0, |_, _| Ok(()))?;
    if op & 3 == 0 {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #2 ($operation) must be one of LOCK_SH, LOCK_EX, or LOCK_UN",
        );
    }
    if a.has(2) {
        a.set_ref(2, Value::Int(0));
    }
    with_stream(vm, a, 0, |vm, s| {
        if matches!(s.backing, Backing::User { .. }) {
            let r = super::userstream::simple(vm, s, b"stream_lock", vec![Value::Int(op)])?;
            return Ok(Value::Bool(match r {
                Some(v) => vm.to_bool(&v),
                None => {
                    let c = super::userstream::class_of_stream(vm, s);
                    vm.warn(&format!("flock(): {}::stream_lock is not implemented!", c))?;
                    false
                }
            }));
        }
        // native streams over a file (compress.zlib://) lock it (test
        // ext/zlib/tests/zlib_wrapper_flock_basic.phpt)
        if matches!(s.backing, Backing::Native(_)) {
            return Ok(Value::Bool(true));
        }
        let Backing::File(f) = &s.backing else {
            return Ok(Value::Bool(false));
        };
        use std::os::fd::AsRawFd;
        let mut how = match op & 3 {
            1 => libc::LOCK_SH,
            2 => libc::LOCK_EX,
            _ => libc::LOCK_UN,
        };
        if op & 4 != 0 {
            how |= libc::LOCK_NB;
        }
        if unsafe { libc::flock(f.as_raw_fd(), how) } == 0 {
            return Ok(Value::Bool(true));
        }
        if std::io::Error::last_os_error().raw_os_error() == Some(libc::EWOULDBLOCK) && a.has(2) {
            a.set_ref(2, Value::Int(1));
        }
        Ok(Value::Bool(false))
    })
}

fn stream_resolve_include_path(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let f = a.str(0);
    if f.is_empty() {
        return vm.fthrow(a, "ValueError", "Argument #1 ($filename) cannot be empty");
    }
    Ok(match vm.resolve_include(&f) {
        Some(p) => Value::Str(PhpStr::new(p.as_os_str().as_bytes())),
        None => Value::Bool(false),
    })
}

fn stream_supports_lock(vm: &mut Vm, a: &mut Args) -> R<Value> {
    with_stream(vm, a, 0, |vm, s| {
        if matches!(s.backing, Backing::User { .. }) {
            let r = super::userstream::simple(vm, s, b"stream_lock", vec![Value::Int(0)])?;
            return Ok(Value::Bool(r.is_some_and(|v| vm.to_bool(&v))));
        }
        Ok(Value::Bool(
            matches!(s.backing, Backing::File(_)) && !s.wrapper.is_empty(),
        ))
    })
}

pub(crate) fn read_rest(vm: &mut Vm, s: &mut Stream) -> Vec<u8> {
    let mut out = Vec::new();
    let mut buf = [0u8; 8192];
    loop {
        match s.read_some(vm, &mut buf) {
            // a non-blocking or timed-out socket has more to come
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                return out;
            }
            Ok(0) | Err(_) => break,
            Ok(n) => out.extend_from_slice(&buf[..n]),
        }
    }
    s.eof = true;
    out
}

fn fpassthru(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let data = with_stream(vm, a, 0, |vm, s| Ok(read_rest(vm, s)))?;
    vm.echo(&data);
    Ok(Value::Int(data.len() as i64))
}

fn stream_get_contents(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let len = a.opt_int(1);
    let off = a.int_or(2, -1);
    if len.is_some_and(|l| l < -1) {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #2 ($length) must be greater than or equal to -1",
        );
    }
    with_stream(vm, a, 0, |vm, s| {
        // test streams/bug54946.phpt
        if !s.readable && matches!(s.backing, Backing::File(_)) {
            vm.notice_msg(
                "stream_get_contents(): Read of 8192 bytes failed with errno=9 Bad file descriptor",
            )?;
            return Ok(Value::Str(PhpStr::empty()));
        }
        if off >= 0 {
            do_seek(vm, s, off, 0);
        }
        // Sockets and pipes: read up to the length only, never past it
        // (test ext/openssl/tests/gh10495.phpt).
        if let Some(l) = len
            && l >= 0
            && s.is_pipe()
        {
            let mut data = vec![0u8; l as usize];
            let mut got = 0;
            while got < data.len() {
                match s.read_some(vm, &mut data[got..]) {
                    Ok(0) => {
                        s.eof = true;
                        break;
                    }
                    Ok(n) => got += n,
                    Err(_) => break,
                }
            }
            data.truncate(got);
            return Ok(Value::Str(PhpStr::from(data)));
        }
        let mut data = read_rest(vm, s);
        if let Some(l) = len
            && l >= 0
            && (l as usize) < data.len()
        {
            // Put back what wasn't asked for.
            let rest = data.split_off(l as usize);
            if let Backing::Memory { pos, .. } = &mut s.backing {
                *pos -= rest.len();
            } else if let Backing::File(f) = &mut s.backing {
                let _ = f.seek(SeekFrom::Current(-(rest.len() as i64)));
            }
            s.eof = false;
        }
        Ok(Value::Str(PhpStr::from(data)))
    })
}

pub(crate) fn stream_get_meta_data(vm: &mut Vm, a: &mut Args) -> R<Value> {
    with_stream(vm, a, 0, |_vm, s| {
        let mut m = PhpArray::new();
        // data: streams have no timed_out/blocked/eof entries (test
        // stream_rfc2397_002.phpt)
        if let Some(dm) = &s.data_meta {
            let mut m = dm.clone();
            m.insert(Key::from("wrapper_type"), Value::from(s.wrapper));
            m.insert(Key::from("stream_type"), Value::from(s.stream_type));
            m.insert(Key::from("mode"), Value::Str(s.mode.clone()));
            m.insert(Key::from("unread_bytes"), Value::Int(0));
            m.insert(Key::from("seekable"), Value::Bool(true));
            m.insert(Key::from("uri"), Value::Str(s.uri.clone()));
            return Ok(Value::Array(m));
        }
        m.insert(Key::from("timed_out"), Value::Bool(false));
        // a pipe set non-blocking by stream_set_blocking() (test
        // ext/standard/tests/streams/bug72853.phpt)
        let blocked = match &s.backing {
            Backing::File(f) if s.is_pipe() => {
                use std::os::unix::io::AsRawFd;
                // SAFETY: F_GETFL on an open descriptor.
                let fl = unsafe { libc::fcntl(f.as_raw_fd(), libc::F_GETFL) };
                fl < 0 || fl & libc::O_NONBLOCK == 0
            }
            _ => true,
        };
        m.insert(Key::from("blocked"), Value::Bool(blocked));
        let eof = match &s.backing {
            Backing::Memory { data, pos } => *pos >= data.len() && s.eof || *pos >= data.len(),
            _ => s.eof,
        };
        m.insert(Key::from("eof"), Value::Bool(eof));
        if let Backing::User { obj } = &s.backing {
            m.insert(Key::from("wrapper_data"), Value::Object(obj.clone()));
        }
        if let Backing::Native(n) = &s.backing
            && let Some(w) = n.wrapper_data()
        {
            m.insert(Key::from("wrapper_data"), w);
        }
        let pipe = s.is_pipe();
        // no wrapper: gzopen() streams (test zlib_wrapper_meta_data_basic.phpt)
        let nowrap = pipe || s.wrapper.is_empty();
        if !nowrap {
            m.insert(Key::from("wrapper_type"), Value::from(s.wrapper));
        }
        m.insert(Key::from("stream_type"), Value::from(s.stream_type));
        m.insert(Key::from("mode"), Value::Str(s.mode.clone()));
        m.insert(Key::from("unread_bytes"), Value::Int(0));
        m.insert(
            Key::from("seekable"),
            Value::Bool(
                !pipe
                    && match &mut s.backing {
                        Backing::File(_) | Backing::Memory { .. } | Backing::Dir { .. } => true,
                        Backing::Native(n) => n.tell().is_some(),
                        _ => false,
                    },
            ),
        );
        if !nowrap && !matches!(s.backing, Backing::Dir { .. }) {
            m.insert(Key::from("uri"), Value::Str(s.uri.clone()));
        }
        if let Backing::Native(n) = &s.backing {
            n.meta(&mut m);
        }
        Ok(Value::Array(m))
    })
}

fn stream_isatty(vm: &mut Vm, a: &mut Args) -> R<Value> {
    if let Value::Int(fd) = a.val(0) {
        return Ok(Value::Bool(unsafe { libc::isatty(fd as i32) } == 1));
    }
    with_stream(vm, a, 0, |_vm, s| {
        let fd = match &s.backing {
            Backing::Stdin => 0,
            Backing::Stdout => 1,
            Backing::Stderr => 2,
            Backing::File(f) => {
                use std::os::unix::io::AsRawFd;
                f.as_raw_fd()
            }
            _ => return Ok(Value::Bool(false)),
        };
        Ok(Value::Bool(unsafe { libc::isatty(fd) } == 1))
    })
}

fn stream_get_wrappers(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    let mut out: Vec<Value> = ["php", "file", "glob", "data"]
        .iter()
        .filter(|s| {
            !vm.disabled_wrappers
                .iter()
                .any(|d| d.as_slice() == s.as_bytes())
        })
        .map(|s| Value::from(*s))
        .collect();
    for n in super::streams::wrapper_names(vm) {
        out.push(Value::Str(PhpStr::from(n)));
    }
    let mut users: Vec<&Vec<u8>> = vm.user_wrappers.keys().collect();
    users.sort();
    for u in users {
        out.push(Value::Str(PhpStr::new(u)));
    }
    Ok(Value::Array(PhpArray::from_values(out)))
}

/// A stat array from a user wrapper's stream_stat/url_stat result: the 13
/// fields by number and name (missing ones 0).
pub fn stat_from_user(st: &PhpArray) -> PhpArray {
    const NAMES: [&str; 13] = [
        "dev", "ino", "mode", "nlink", "uid", "gid", "rdev", "size", "atime", "mtime", "ctime",
        "blksize", "blocks",
    ];
    let get = |i: usize| -> i64 {
        let v = st
            .get_str(&PhpStr::from(NAMES[i]))
            .or_else(|| st.get_int(i as i64))
            .map(|v| v.deref());
        match v {
            Some(Value::Int(n)) => n,
            Some(Value::Float(f)) => f as i64,
            Some(Value::Bool(b)) => b as i64,
            Some(Value::Str(s)) => goro_runtime::convert::str_to_int(&s),
            _ => {
                if i >= 11 {
                    -1
                } else {
                    0
                }
            }
        }
    };
    let mut a = PhpArray::new();
    for i in 0..13 {
        let _ = a.push(Value::Int(get(i)));
    }
    for (i, n) in NAMES.iter().enumerate() {
        a.insert(Key::from(*n), Value::Int(get(i)));
    }
    a
}

fn file_get_contents(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let f = with_include_path(vm, &a.str(0), a.bool_or(1, false));
    let off = a.int_or(3, 0);
    let len = a.opt_int(4);
    if let Some(l) = len
        && l < 0
    {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #5 ($length) must be greater than or equal to 0",
        );
    }
    let Some(mut s) = open_stream(vm, a, &f, b"rb", false)? else {
        return Ok(Value::Bool(false));
    };
    if let Backing::File(file) = &s.backing
        && file.metadata().is_ok_and(|m| m.is_dir())
    {
        vm.fnotice(a, "Read of 8192 bytes failed with errno=21 Is a directory")?;
        return Ok(Value::Str(PhpStr::empty()));
    }
    if off != 0 && !do_seek(vm, &mut s, off, if off < 0 { 2 } else { 0 }) {
        vm.fwarn(
            a,
            &format!("Failed to seek to position {} in the stream", off),
        )?;
        return Ok(Value::Bool(false));
    }
    let mut data = read_rest(vm, &mut s);
    if let Some(l) = len {
        data.truncate(l as usize);
    }
    Ok(Value::Str(PhpStr::from(data)))
}

fn file_put_contents(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let f = a.str(0);
    let flags = a.int_or(2, 0);
    let data = a.val(1);
    if let Value::Resource(r) = a.val(3)
        && r.res().kind.borrow().as_bytes() != b"stream-context"
    {
        return vm.fthrow(
            a,
            "TypeError",
            "supplied resource is not a valid Stream-Context resource",
        );
    }
    let bytes: Vec<u8> = match &data {
        Value::Array(arr) => {
            let mut out = Vec::new();
            for (_, v) in arr.iter() {
                vm.append_to_string(&mut out, &v.deref())?;
            }
            out
        }
        Value::Resource(r) if !is_stream_res(r) => {
            return vm.fthrow(
                a,
                "TypeError",
                "supplied resource is not a valid stream resource",
            );
        }
        Value::Resource(_) => {
            let tmp_args = Args {
                this: None,
                vals: &mut vec![data.clone()],
                func: a.func,
                static_class: std::ptr::null(),
                named: None,
            };
            with_stream(vm, &tmp_args, 0, |vm, st| Ok(read_rest(vm, st)))?
        }
        // an object without __toString() writes nothing (observed on
        // PHP 8.5; test file_put_contents.phpt)
        Value::Object(o) if vm.class_of(o).find_method(b"__tostring").is_none() => {
            // the file is opened (truncated) first
            let mode: &[u8] = if flags & 8 != 0 { b"ab" } else { b"wb" };
            if let Some(mut s) = open_stream(vm, a, &f, mode, false)? {
                super::streams::close_stream(vm, &mut s)?;
            }
            return Ok(Value::Bool(false));
        }
        Value::Object(_) => {
            let s = vm.to_php_str(&data)?;
            s.to_vec()
        }
        other => {
            let mut out = Vec::new();
            vm.append_to_string(&mut out, other)?;
            out
        }
    };
    let mode: &[u8] = if flags & 8 != 0 { b"ab" } else { b"wb" };
    let Some(mut s) = open_stream(vm, a, &f, mode, false)? else {
        return Ok(Value::Bool(false));
    };
    if !s.writable {
        vm.fnotice(a, "Stream is not writable")?;
        return Ok(Value::Bool(false));
    }
    let r = s.write_all(vm, &bytes);
    // native streams (compress.zlib://) finish their output on close
    super::streams::close_stream(vm, &mut s)?;
    match r {
        Ok(n) if n < bytes.len() => {
            vm.fwarn(
                a,
                &format!(
                    "Only {} of {} bytes written, possibly out of free disk space",
                    n,
                    bytes.len()
                ),
            )?;
            Ok(Value::Bool(false))
        }
        Ok(n) => Ok(Value::Int(n as i64)),
        Err(_) => Ok(Value::Bool(false)),
    }
}

fn file(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let flags = a.int_or(1, 0);
    let f = with_include_path(vm, &a.str(0), flags & 1 != 0);
    // FILE_USE_INCLUDE_PATH, FILE_IGNORE_NEW_LINES, FILE_SKIP_EMPTY_LINES,
    // FILE_NO_DEFAULT_CONTEXT
    if flags < 0 || flags & !(1 | 2 | 4 | 16) != 0 {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #2 ($flags) must be a valid flag value",
        );
    }
    let Some(mut s) = open_stream(vm, a, &f, b"rb", false)? else {
        return Ok(Value::Bool(false));
    };
    let data = read_rest(vm, &mut s);
    let mut out = PhpArray::new();
    let mut start = 0;
    let ignore_nl = flags & 2 != 0;
    let skip_empty = flags & 4 != 0;
    let mut i = 0;
    while i < data.len() {
        if data[i] == b'\n' {
            // a CR before the LF goes too (observed on PHP 8.5)
            let end = if !ignore_nl {
                i + 1
            } else if i > start && data[i - 1] == b'\r' {
                i - 1
            } else {
                i
            };
            let line = &data[start..end];
            if !(skip_empty && ignore_nl && line.is_empty()) {
                let _ = out.push(Value::Str(PhpStr::new(line)));
            }
            start = i + 1;
        }
        i += 1;
    }
    if start < data.len() {
        let _ = out.push(Value::Str(PhpStr::new(&data[start..])));
    }
    Ok(Value::Array(out))
}

fn readfile(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let f = with_include_path(vm, &a.str(0), a.bool_or(1, false));
    let Some(mut s) = open_stream(vm, a, &f, b"rb", false)? else {
        return Ok(Value::Bool(false));
    };
    // In chunks: the file may be larger than memory_limit (test
    // ext/standard/tests/file/bug22414).
    let mut buf = vec![0u8; 64 << 10];
    let mut total = 0usize;
    loop {
        match s.read_some(vm, &mut buf) {
            Ok(0) => break,
            Ok(n) => {
                vm.echo(&buf[..n]);
                total += n;
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(_) => break,
        }
    }
    Ok(Value::Int(total as i64))
}

fn fs_warn(vm: &mut Vm, a: &Args, what: &[u8], e: &std::io::Error) -> R<Value> {
    pwarn(vm, a, what, &io_err_msg(e))?;
    Ok(Value::Bool(false))
}

/// A warning naming the path: `"f(path): msg"`.
fn pwarn(vm: &mut Vm, a: &Args, path: &[u8], msg: &str) -> R<()> {
    vm.warn(&format!(
        "{}({}): {}",
        a.fname(),
        String::from_utf8_lossy(path),
        msg
    ))
}

/// ValueError for a path argument holding a NUL byte.
pub(crate) fn nul_arg(vm: &mut Vm, a: &Args, i: usize, name: &str) -> R<bool> {
    if a.get(i)
        .is_some_and(|v| matches!(v.deref(), Value::Str(s) if s.contains(&0)))
    {
        vm.fthrow::<()>(
            a,
            "ValueError",
            &format!(
                "Argument #{} (${}) must not contain any null bytes",
                i + 1,
                name
            ),
        )?;
        return Ok(false);
    }
    Ok(true)
}

fn unlink(vm: &mut Vm, a: &mut Args) -> R<Value> {
    if !nul_arg(vm, a, 0, "filename")? {
        return Ok(Value::Null);
    }
    if !basedir_check(vm, a, 0)? {
        return Ok(Value::Bool(false));
    }
    let f = a.str(0);
    if let Some(cls) = super::userstream::wrapper_for(vm, &f) {
        return super::userstream::path_op(vm, a, &cls, "unlink", vec![Value::Str(f)]);
    }
    let p = vm.fs_path(&f);
    // a symbolic link to a directory is removed
    if std::fs::symlink_metadata(&p).is_ok_and(|m| m.is_dir()) {
        pwarn(vm, a, &f, "Is a directory")?;
        return Ok(Value::Bool(false));
    }
    match std::fs::remove_file(&p) {
        Ok(()) => Ok(Value::Bool(true)),
        Err(e) => fs_warn(vm, a, &f, &e),
    }
}

fn rename(vm: &mut Vm, a: &mut Args) -> R<Value> {
    if !nul_arg(vm, a, 0, "from")? {
        return Ok(Value::Null);
    }
    if !nul_arg(vm, a, 1, "to")? {
        return Ok(Value::Null);
    }
    if !basedir_check(vm, a, 0)? || !basedir_check(vm, a, 1)? {
        return Ok(Value::Bool(false));
    }
    let from = a.str(0);
    let to = a.str(1);
    let sch = |p: &[u8]| super::userstream::scheme(p).unwrap_or_else(|| b"file".to_vec());
    if sch(&from) != sch(&to) {
        vm.fwarn(a, "Cannot rename a file across wrapper types")?;
        return Ok(Value::Bool(false));
    }
    if let Some(cls) = super::userstream::wrapper_for(vm, &from) {
        return super::userstream::path_op(
            vm,
            a,
            &cls,
            "rename",
            vec![Value::Str(from), Value::Str(to)],
        );
    }
    match std::fs::rename(vm.fs_path(&from), vm.fs_path(&to)) {
        Ok(()) => Ok(Value::Bool(true)),
        Err(e) => {
            let mut both = from.to_vec();
            both.push(b',');
            both.extend_from_slice(&to);
            pwarn(vm, a, &both, &io_err_msg(&e))?;
            Ok(Value::Bool(false))
        }
    }
}

fn copy(vm: &mut Vm, a: &mut Args) -> R<Value> {
    if !nul_arg(vm, a, 0, "from")? {
        return Ok(Value::Null);
    }
    if !nul_arg(vm, a, 1, "to")? {
        return Ok(Value::Null);
    }
    if !basedir_check(vm, a, 0)? || !basedir_check(vm, a, 1)? {
        return Ok(Value::Bool(false));
    }
    let from = a.str(0);
    let to = a.str(1);
    let local = |p: &[u8]| !p.windows(3).any(|w| w == b"://") || p.starts_with(b"file://");
    if local(&from) && local(&to) {
        let fp = vm.fs_path(from.strip_prefix(b"file://").unwrap_or(&from));
        let tp = vm.fs_path(to.strip_prefix(b"file://").unwrap_or(&to));
        if let Ok(m) = std::fs::metadata(&fp) {
            if m.is_dir() {
                vm.fwarn(
                    a,
                    "The first argument to copy() function cannot be a directory",
                )?;
                return Ok(Value::Bool(false));
            }
            if let Ok(t) = std::fs::metadata(&tp) {
                if t.is_dir() {
                    vm.fwarn(
                        a,
                        "The second argument to copy() function cannot be a directory",
                    )?;
                    return Ok(Value::Bool(false));
                }
                if t.dev() == m.dev() && t.ino() == m.ino() {
                    return Ok(Value::Bool(false));
                }
            }
        }
    }
    let Some(mut s) = open_stream(vm, a, &from, b"rb", false)? else {
        return Ok(Value::Bool(false));
    };
    let Some(mut d) = open_stream(vm, a, &to, b"wb", false)? else {
        return Ok(Value::Bool(false));
    };
    // In chunks: files may be larger than memory (test
    // ext/standard/tests/file/bug81145).
    let mut ok = true;
    let mut buf = vec![0u8; 1 << 20];
    loop {
        match s.read_some(vm, &mut buf) {
            Ok(0) => break,
            Ok(n) => {
                if d.write_all(vm, &buf[..n]).is_err() {
                    ok = false;
                    break;
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(_) => break,
        }
    }
    super::streams::close_stream(vm, &mut s)?;
    super::streams::close_stream(vm, &mut d)?;
    Ok(Value::Bool(ok))
}

fn mkdir(vm: &mut Vm, a: &mut Args) -> R<Value> {
    if !nul_arg(vm, a, 0, "directory")? {
        return Ok(Value::Null);
    }
    if !basedir_check(vm, a, 0)? {
        return Ok(Value::Bool(false));
    }
    let d = a.str(0);
    let perms = a.int_or(1, 0o777);
    let rec = a.bool_or(2, false);
    if let Some(cls) = super::userstream::wrapper_for(vm, &d) {
        let opts = if rec { 1 | 8 } else { 8 };
        return super::userstream::path_op(
            vm,
            a,
            &cls,
            "mkdir",
            vec![Value::Str(d), Value::Int(perms), Value::Int(opts)],
        );
    }
    let p = vm.fs_path(&d);
    let r = if rec {
        if p.is_dir() {
            Err(std::io::Error::from(std::io::ErrorKind::AlreadyExists))
        } else {
            std::fs::create_dir_all(&p)
        }
    } else {
        std::fs::create_dir(&p)
    };
    match r {
        Ok(()) => {
            let _ = std::fs::set_permissions(
                &p,
                std::fs::Permissions::from_mode(perms as u32 & !current_umask()),
            );
            Ok(Value::Bool(true))
        }
        Err(e) => {
            vm.fwarn(a, &io_err_msg(&e))?;
            Ok(Value::Bool(false))
        }
    }
}

fn current_umask() -> u32 {
    unsafe {
        let m = libc::umask(0o022);
        libc::umask(m);
        m
    }
}

fn rmdir(vm: &mut Vm, a: &mut Args) -> R<Value> {
    if !nul_arg(vm, a, 0, "directory")? {
        return Ok(Value::Null);
    }
    if !basedir_check(vm, a, 0)? {
        return Ok(Value::Bool(false));
    }
    let d = a.str(0);
    if let Some(cls) = super::userstream::wrapper_for(vm, &d) {
        return super::userstream::path_op(
            vm,
            a,
            &cls,
            "rmdir",
            vec![Value::Str(d), Value::Int(8)],
        );
    }
    match std::fs::remove_dir(vm.fs_path(&d)) {
        Ok(()) => Ok(Value::Bool(true)),
        Err(e) => fs_warn(vm, a, &d, &e),
    }
}

/// url_stat() through a user wrapper for the path argument: Some(None)
/// when the wrapper reports no file.
fn user_stat(vm: &mut Vm, a: &Args, flags: i64) -> R<Option<Option<PhpArray>>> {
    let f = a.str(0);
    let Some(cls) = super::userstream::wrapper_for(vm, &f) else {
        return Ok(None);
    };
    Ok(Some(
        super::userstream::url_stat(vm, &cls, &f, flags)?.map(|st| stat_from_user(&st)),
    ))
}

fn user_mode(st: &PhpArray) -> u32 {
    match st.get_str(&PhpStr::from("mode")) {
        Some(Value::Int(m)) => *m as u32,
        _ => 0,
    }
}

fn meta(vm: &Vm, a: &Args) -> Option<std::fs::Metadata> {
    let f = a.str(0);
    if f.is_empty() || f.contains(&0) {
        return None;
    }
    std::fs::metadata(vm.stream_path(f.strip_prefix(b"file://").unwrap_or(&f))).ok()
}

fn is_file(vm: &mut Vm, a: &mut Args) -> R<Value> {
    if !basedir_check(vm, a, 0)? {
        return Ok(Value::Bool(false));
    }
    if let Some(st) = user_stat(vm, a, 2)? {
        return Ok(Value::Bool(
            st.is_some_and(|st| user_mode(&st) & 0o170000 == 0o100000),
        ));
    }
    Ok(Value::Bool(meta(vm, a).is_some_and(|m| m.is_file())))
}
fn is_dir(vm: &mut Vm, a: &mut Args) -> R<Value> {
    if !basedir_check(vm, a, 0)? {
        return Ok(Value::Bool(false));
    }
    if let Some(st) = user_stat(vm, a, 2)? {
        return Ok(Value::Bool(
            st.is_some_and(|st| user_mode(&st) & 0o170000 == 0o040000),
        ));
    }
    Ok(Value::Bool(meta(vm, a).is_some_and(|m| m.is_dir())))
}
fn is_link(vm: &mut Vm, a: &mut Args) -> R<Value> {
    if !basedir_check(vm, a, 0)? {
        return Ok(Value::Bool(false));
    }
    let f = a.str(0);
    Ok(Value::Bool(
        std::fs::symlink_metadata(vm.fs_path(&f)).is_ok_and(|m| m.file_type().is_symlink()),
    ))
}
fn file_exists(vm: &mut Vm, a: &mut Args) -> R<Value> {
    if !basedir_check(vm, a, 0)? {
        return Ok(Value::Bool(false));
    }
    unknown_wrapper_warn(vm, a, &a.str(0))?;
    if let Some(st) = user_stat(vm, a, 2)? {
        return Ok(Value::Bool(st.is_some()));
    }
    Ok(Value::Bool(meta(vm, a).is_some()))
}

fn access(vm: &Vm, a: &Args, mode: i32) -> bool {
    let f = a.str(0);
    if f.is_empty() || f.contains(&0) {
        return false;
    }
    let p = vm.fs_path(&f);
    let c = std::ffi::CString::new(p.as_os_str().as_bytes()).unwrap();
    unsafe { libc::access(c.as_ptr(), mode) == 0 }
}

fn is_readable(vm: &mut Vm, a: &mut Args) -> R<Value> {
    if !basedir_check(vm, a, 0)? {
        return Ok(Value::Bool(false));
    }
    Ok(Value::Bool(access(vm, a, libc::R_OK)))
}
fn is_writable(vm: &mut Vm, a: &mut Args) -> R<Value> {
    if !basedir_check(vm, a, 0)? {
        return Ok(Value::Bool(false));
    }
    Ok(Value::Bool(access(vm, a, libc::W_OK)))
}
fn is_executable(vm: &mut Vm, a: &mut Args) -> R<Value> {
    if !basedir_check(vm, a, 0)? {
        return Ok(Value::Bool(false));
    }
    // directories count (test is_executable_variation1.phpt)
    Ok(Value::Bool(
        meta(vm, a).is_some() && access(vm, a, libc::X_OK),
    ))
}

fn stat_fail(vm: &mut Vm, a: &Args, what: &str) -> R<Value> {
    let f = a.str(0);
    if f.is_empty() {
        return Ok(Value::Bool(false));
    }
    if f.contains(&0) {
        vm.fwarn(a, "Filename contains null byte")?;
        return Ok(Value::Bool(false));
    }
    vm.fwarn(
        a,
        &format!("{} failed for {}", what, a.str(0).to_string_lossy()),
    )?;
    Ok(Value::Bool(false))
}

macro_rules! stat_fn {
    ($name:ident, $label:literal, $m:ident => $e:expr) => {
        fn $name(vm: &mut Vm, a: &mut Args) -> R<Value> {
            if !basedir_check(vm, a, 0)? {
                return Ok(Value::Bool(false));
            }
            if let Some(st) = user_stat(vm, a, 0)? {
                let key = match stringify!($name) {
                    "filesize" => "size",
                    "filemtime" => "mtime",
                    "fileatime" => "atime",
                    "filectime" => "ctime",
                    "fileperms" => "mode",
                    "fileinode" => "ino",
                    "fileowner" => "uid",
                    _ => "gid",
                };
                return match st {
                    Some(st) => Ok(st.get_str(&PhpStr::from(key)).cloned().unwrap_or_default()),
                    None => stat_fail(vm, a, $label),
                };
            }
            match meta(vm, a) {
                Some($m) => Ok(Value::Int($e)),
                None => stat_fail(vm, a, $label),
            }
        }
    };
}

stat_fn!(filesize, "stat", m => m.len() as i64);
stat_fn!(filemtime, "stat", m => m.mtime());
stat_fn!(fileatime, "stat", m => m.atime());
stat_fn!(filectime, "stat", m => m.ctime());
stat_fn!(fileperms, "stat", m => m.mode() as i64);
stat_fn!(fileinode, "stat", m => m.ino() as i64);
stat_fn!(fileowner, "stat", m => m.uid() as i64);
stat_fn!(filegroup, "stat", m => m.gid() as i64);

fn filetype(vm: &mut Vm, a: &mut Args) -> R<Value> {
    if !basedir_check(vm, a, 0)? {
        return Ok(Value::Bool(false));
    }
    let f = a.str(0);
    match std::fs::symlink_metadata(vm.fs_path(&f)) {
        Ok(m) => {
            let t = m.file_type();
            use std::os::unix::fs::FileTypeExt;
            let s = if t.is_symlink() {
                "link"
            } else if t.is_dir() {
                "dir"
            } else if t.is_file() {
                "file"
            } else if t.is_fifo() {
                "fifo"
            } else if t.is_char_device() {
                "char"
            } else if t.is_block_device() {
                "block"
            } else {
                "unknown"
            };
            Ok(Value::from(s))
        }
        Err(_) => stat_fail(vm, a, "Lstat"),
    }
}

fn touch(vm: &mut Vm, a: &mut Args) -> R<Value> {
    if !nul_arg(vm, a, 0, "filename")? {
        return Ok(Value::Null);
    }
    if !basedir_check(vm, a, 0)? {
        return Ok(Value::Bool(false));
    }
    let f = a.str(0);
    if a.opt_int(1).is_none() && a.opt_int(2).is_some() {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #2 ($mtime) cannot be null when argument #3 ($atime) is an integer",
        );
    }
    if f.is_empty() {
        return Ok(Value::Bool(false));
    }
    if let Some(cls) = super::userstream::wrapper_for(vm, &f) {
        let times = match (a.opt_int(1), a.opt_int(2)) {
            (None, _) => PhpArray::new(),
            (Some(m), at) => PhpArray::from_values([Value::Int(m), Value::Int(at.unwrap_or(m))]),
        };
        return super::userstream::path_op(
            vm,
            a,
            &cls,
            "stream_metadata",
            vec![Value::Str(f), Value::Int(1), Value::Array(times)],
        );
    }
    let p = vm.fs_path(&f);
    if !p.exists() && std::fs::File::create(&p).is_err() {
        vm.fwarn(
            a,
            &format!(
                "Unable to create file {} because No such file or directory",
                f.to_string_lossy()
            ),
        )?;
        return Ok(Value::Bool(false));
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let mtime = a.opt_int(1).unwrap_or(now);
    let atime = a.opt_int(2).unwrap_or(mtime);
    let c = std::ffi::CString::new(p.as_os_str().as_bytes()).unwrap();
    let times = [
        libc::timespec {
            tv_sec: atime as _,
            tv_nsec: 0,
        },
        libc::timespec {
            tv_sec: mtime as _,
            tv_nsec: 0,
        },
    ];
    unsafe { libc::utimensat(libc::AT_FDCWD, c.as_ptr(), times.as_ptr(), 0) };
    Ok(Value::Bool(true))
}

fn chmod(vm: &mut Vm, a: &mut Args) -> R<Value> {
    if !nul_arg(vm, a, 0, "filename")? {
        return Ok(Value::Null);
    }
    if !basedir_check(vm, a, 0)? {
        return Ok(Value::Bool(false));
    }
    let f = a.str(0);
    let m = a.int(1);
    if let Some(cls) = super::userstream::wrapper_for(vm, &f) {
        return super::userstream::path_op(
            vm,
            a,
            &cls,
            "stream_metadata",
            vec![Value::Str(f), Value::Int(6), Value::Int(m)],
        );
    }
    match std::fs::set_permissions(vm.fs_path(&f), std::fs::Permissions::from_mode(m as u32)) {
        Ok(()) => Ok(Value::Bool(true)),
        Err(e) => {
            vm.fwarn(a, &io_err_msg(&e))?;
            Ok(Value::Bool(false))
        }
    }
}

fn stat(vm: &mut Vm, a: &mut Args) -> R<Value> {
    if !basedir_check(vm, a, 0)? {
        return Ok(Value::Bool(false));
    }
    if let Some(st) = user_stat(vm, a, 0)? {
        return match st {
            Some(st) => Ok(Value::Array(st)),
            None => stat_fail(vm, a, "stat"),
        };
    }
    match meta(vm, a) {
        Some(m) => Ok(Value::Array(stat_array(&m))),
        None => stat_fail(vm, a, "stat"),
    }
}

fn lstat(vm: &mut Vm, a: &mut Args) -> R<Value> {
    if !basedir_check(vm, a, 0)? {
        return Ok(Value::Bool(false));
    }
    if let Some(st) = user_stat(vm, a, 1)? {
        return match st {
            Some(st) => Ok(Value::Array(st)),
            None => stat_fail(vm, a, "Lstat"),
        };
    }
    let f = a.str(0);
    match std::fs::symlink_metadata(vm.fs_path(&f)) {
        Ok(m) => Ok(Value::Array(stat_array(&m))),
        Err(_) => stat_fail(vm, a, "Lstat"),
    }
}

fn clearstatcache(_vm: &mut Vm, _a: &mut Args) -> R<Value> {
    super::userstream::clear_stat_cache();
    Ok(Value::Null)
}

fn ret_true(_vm: &mut Vm, _a: &mut Args) -> R<Value> {
    Ok(Value::Bool(true))
}
/// stream_set_write_buffer() & co: buffering is not emulated, user
/// streams get the option (STREAM_BUFFER_NONE or STREAM_BUFFER_FULL with
/// the size).
fn set_buffer(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let size = a.int(1);
    let fname = a.fname();
    let option = if fname == "stream_set_read_buffer" {
        2
    } else {
        3
    };
    with_stream(vm, a, 0, |vm, s| {
        let (value, ptr) = if size == 0 { (0, 8192) } else { (2, size) };
        Ok(Value::Int(
            match super::userstream::set_option(vm, s, &fname, option, value, Value::Int(ptr))? {
                Some(true) | None => 0,
                Some(false) => -1,
            },
        ))
    })
}

fn realpath(vm: &mut Vm, a: &mut Args) -> R<Value> {
    if !nul_arg(vm, a, 0, "path")? {
        return Ok(Value::Null);
    }
    if !basedir_check(vm, a, 0)? {
        return Ok(Value::Bool(false));
    }
    let f = a.str(0);
    let p = if f.is_empty() {
        vm.cwd.clone()
    } else {
        vm.fs_path(&f)
    };
    match std::fs::canonicalize(&p) {
        Ok(c) => Ok(Value::Str(PhpStr::new(c.as_os_str().as_bytes()))),
        Err(_) => Ok(Value::Bool(false)),
    }
}

/// `dirname()` (PHP manual: dirname): parent directory, trailing slashes
/// ignored, "." for no directory.
pub fn dirname_bytes(p: &[u8]) -> Vec<u8> {
    if p.is_empty() {
        return Vec::new();
    }
    let mut end = p.len();
    while end > 1 && p[end - 1] == b'/' {
        end -= 1;
    }
    if end == 1 && p[0] == b'/' {
        return b"/".to_vec();
    }
    match p[..end].iter().rposition(|&c| c == b'/') {
        None => b".".to_vec(),
        Some(i) => {
            let mut j = i;
            while j > 0 && p[j - 1] == b'/' {
                j -= 1;
            }
            if j == 0 {
                b"/".to_vec()
            } else {
                p[..j].to_vec()
            }
        }
    }
}

fn dirname(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let p = a.str(0);
    let levels = a.int_or(1, 1);
    if levels < 1 {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #2 ($levels) must be greater than or equal to 1",
        );
    }
    let mut cur = p.to_vec();
    for _ in 0..levels {
        let n = dirname_bytes(&cur);
        if n == cur {
            break;
        }
        cur = n;
    }
    Ok(Value::Str(PhpStr::from(cur)))
}

/// `basename()`: last path component, with a suffix removed.
pub fn basename_bytes(p: &[u8], suffix: &[u8]) -> Vec<u8> {
    let mut end = p.len();
    while end > 0 && p[end - 1] == b'/' {
        end -= 1;
    }
    let start = p[..end]
        .iter()
        .rposition(|&c| c == b'/')
        .map(|i| i + 1)
        .unwrap_or(0);
    let mut b = &p[start..end];
    if !suffix.is_empty() && b.len() > suffix.len() && b.ends_with(suffix) {
        b = &b[..b.len() - suffix.len()];
    }
    b.to_vec()
}

fn basename(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let p = a.str(0);
    let s = a.str(1);
    Ok(Value::Str(PhpStr::from(basename_bytes(&p, &s))))
}

fn pathinfo(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let p = a.str(0);
    let flags = a.int_or(1, 15);
    let mut out = PhpArray::new();
    let base = basename_bytes(&p, b"");
    if flags & 1 != 0 {
        let has_dir = p.contains(&b'/');
        if has_dir || flags == 1 {
            let d = dirname_bytes(&p);
            out.insert(Key::from("dirname"), Value::Str(PhpStr::from(d)));
        } else if !p.is_empty() {
            out.insert(Key::from("dirname"), Value::from("."));
        }
    }
    if flags & 2 != 0 {
        out.insert(Key::from("basename"), Value::Str(PhpStr::new(&base)));
    }
    let dot = base.iter().rposition(|&c| c == b'.');
    if flags & 4 != 0
        && let Some(d) = dot
    {
        out.insert(
            Key::from("extension"),
            Value::Str(PhpStr::new(&base[d + 1..])),
        );
    }
    if flags & 8 != 0 {
        let f = match dot {
            Some(d) => &base[..d],
            None => &base[..],
        };
        out.insert(Key::from("filename"), Value::Str(PhpStr::new(f)));
    }
    // Any other selection than PATHINFO_ALL gives the first element
    // (observed on PHP 8.5: DIRNAME|BASENAME is the dirname).
    if flags != 15 {
        return Ok(out
            .iter()
            .next()
            .map(|(_, v)| v.clone())
            .unwrap_or(Value::Str(PhpStr::empty())));
    }
    Ok(Value::Array(out))
}

/// Shell wildcard matching (fnmatch/glob): `*`, `?`, `[...]`, `\`.
pub fn fnmatch(pat: &[u8], s: &[u8], flags: i64) -> bool {
    let pathname = flags & 1 != 0;
    let noescape = flags & 2 != 0;
    let period = flags & 4 != 0;
    let casefold = flags & 16 != 0;
    fn eq(a: u8, b: u8, cf: bool) -> bool {
        if cf {
            a.eq_ignore_ascii_case(&b)
        } else {
            a == b
        }
    }
    fn rec(
        p: &[u8],
        s: &[u8],
        pathname: bool,
        noescape: bool,
        period: bool,
        cf: bool,
        at_start: bool,
    ) -> bool {
        if p.is_empty() {
            return s.is_empty();
        }
        if period && at_start && s.first() == Some(&b'.') && p[0] != b'.' {
            return false;
        }
        match p[0] {
            b'*' => {
                let rest = &p[1..];
                for i in 0..=s.len() {
                    if i > 0 && pathname && s[i - 1] == b'/' {
                        break;
                    }
                    if rec(rest, &s[i..], pathname, noescape, period, cf, false) {
                        return true;
                    }
                }
                false
            }
            b'?' => {
                if s.is_empty() || (pathname && s[0] == b'/') {
                    return false;
                }
                rec(&p[1..], &s[1..], pathname, noescape, period, cf, false)
            }
            b'[' => {
                if s.is_empty() {
                    return false;
                }
                let mut i = 1;
                let neg = i < p.len() && (p[i] == b'!' || p[i] == b'^');
                if neg {
                    i += 1;
                }
                let mut matched = false;
                let mut first = true;
                while i < p.len() && (p[i] != b']' || first) {
                    first = false;
                    let lo = p[i];
                    if i + 2 < p.len() && p[i + 1] == b'-' && p[i + 2] != b']' {
                        let hi = p[i + 2];
                        if s[0] >= lo && s[0] <= hi {
                            matched = true;
                        }
                        i += 3;
                    } else {
                        if eq(lo, s[0], cf) {
                            matched = true;
                        }
                        i += 1;
                    }
                }
                if i >= p.len() {
                    return eq(b'[', s[0], cf)
                        && rec(&p[1..], &s[1..], pathname, noescape, period, cf, false);
                }
                if matched == neg {
                    return false;
                }
                rec(&p[i + 1..], &s[1..], pathname, noescape, period, cf, false)
            }
            b'\\' if !noescape && p.len() > 1 => {
                if s.is_empty() || !eq(p[1], s[0], cf) {
                    return false;
                }
                rec(&p[2..], &s[1..], pathname, noescape, period, cf, false)
            }
            c => {
                if s.is_empty() || !eq(c, s[0], cf) {
                    return false;
                }
                let next_start = pathname && c == b'/';
                rec(&p[1..], &s[1..], pathname, noescape, period, cf, next_start)
            }
        }
    }
    rec(pat, s, pathname, noescape, period, casefold, true)
}

fn fnmatch_fn(vm: &mut Vm, a: &mut Args) -> R<Value> {
    // PHP_MAXPATHLEN (test fnmatch_maxpathlen.phpt)
    for (i, what) in [(1, "Filename"), (0, "Pattern")] {
        if a.bytes(i).len() >= 4096 {
            vm.fwarn(
                a,
                &format!("{what} exceeds the maximum allowed length of 4096 characters"),
            )?;
            return Ok(Value::Bool(false));
        }
    }
    if !nul_arg(vm, a, 0, "pattern")? {
        return Ok(Value::Null);
    }
    if !nul_arg(vm, a, 1, "filename")? {
        return Ok(Value::Null);
    }
    Ok(Value::Bool(fnmatch(&a.str(0), &a.str(1), a.int_or(2, 0))))
}

fn expand_braces(p: &[u8]) -> Vec<Vec<u8>> {
    if let Some(open) = p.iter().position(|&c| c == b'{') {
        let mut depth = 0;
        let mut close = None;
        for (i, &c) in p.iter().enumerate().skip(open) {
            if c == b'{' {
                depth += 1;
            } else if c == b'}' {
                depth -= 1;
                if depth == 0 {
                    close = Some(i);
                    break;
                }
            }
        }
        if let Some(close) = close {
            let inner = &p[open + 1..close];
            let mut alts = Vec::new();
            let mut start = 0;
            let mut d = 0;
            for (i, &c) in inner.iter().enumerate() {
                match c {
                    b'{' => d += 1,
                    b'}' => d -= 1,
                    b',' if d == 0 => {
                        alts.push(&inner[start..i]);
                        start = i + 1;
                    }
                    _ => {}
                }
            }
            alts.push(&inner[start..]);
            let mut out = Vec::new();
            for alt in alts {
                let mut s = p[..open].to_vec();
                s.extend_from_slice(alt);
                s.extend_from_slice(&p[close + 1..]);
                out.extend(expand_braces(&s));
            }
            return out;
        }
    }
    vec![p.to_vec()]
}

fn glob_one(vm: &Vm, pat: &[u8], flags: i64, out: &mut Vec<Vec<u8>>) {
    let has_meta = |s: &[u8]| s.iter().any(|&c| matches!(c, b'*' | b'?' | b'['));
    let abs = pat.starts_with(b"/");
    let comps: Vec<&[u8]> = pat.split(|&c| c == b'/').collect();
    let mut cur: Vec<Vec<u8>> = vec![if abs { b"/".to_vec() } else { Vec::new() }];
    for (ci, comp) in comps.iter().enumerate() {
        if comp.is_empty() {
            continue;
        }
        let last = ci + 1 == comps.len();
        let mut next = Vec::new();
        for base in &cur {
            let join = |n: &[u8]| {
                let mut v = base.clone();
                if !v.is_empty() && !v.ends_with(b"/") {
                    v.push(b'/');
                }
                v.extend_from_slice(n);
                v
            };
            if !has_meta(comp) {
                let cand = join(comp);
                if vm.fs_path(&cand).exists() || (last && false) {
                    next.push(cand);
                }
                continue;
            }
            let dir = if base.is_empty() {
                vm.cwd.clone()
            } else {
                vm.fs_path(base)
            };
            let Ok(rd) = std::fs::read_dir(&dir) else {
                continue;
            };
            let mut names: Vec<Vec<u8>> = rd
                .filter_map(|e| e.ok())
                .map(|e| e.file_name().into_vec())
                .collect();
            names.sort();
            for n in names {
                if n.starts_with(b".") && !comp.starts_with(b".") {
                    continue;
                }
                if fnmatch(comp, &n, if flags & 64 != 0 { 2 } else { 0 }) {
                    next.push(join(&n));
                }
            }
        }
        cur = next;
    }
    for c in cur {
        if c.is_empty() {
            continue;
        }
        let is_dir = vm.fs_path(&c).is_dir();
        if flags & (1 << 30) != 0 && !is_dir {
            continue;
        }
        let mut c = c;
        if flags & 2 != 0 && is_dir {
            c.push(b'/');
        }
        out.push(c);
    }
}

fn glob(vm: &mut Vm, a: &mut Args) -> R<Value> {
    if !nul_arg(vm, a, 0, "pattern")? {
        return Ok(Value::Null);
    }
    let pat = a.str(0);
    let flags = a.int_or(1, 0);
    let pats = if flags & 1024 != 0 {
        expand_braces(&pat)
    } else {
        vec![pat.to_vec()]
    };
    let mut out = Vec::new();
    for p in pats {
        let before = out.len();
        glob_one(vm, &p, flags, &mut out);
        if flags & 4 == 0 {
            out[before..].sort();
        }
    }
    // open_basedir: matches outside the allowed directories are dropped,
    // and false comes back when nothing allowed is left (tests
    // tests/security/open_basedir_glob.phpt, open_basedir_glob_variation.phpt).
    let found = out.len();
    out.retain(|p| vm.basedir_allows(p));
    if found > 0 && out.is_empty() {
        return Ok(Value::Bool(false));
    }
    if out.is_empty() && flags & 16 != 0 {
        out.push(pat.to_vec());
    }
    Ok(Value::Array(PhpArray::from_values(
        out.into_iter().map(|s| Value::Str(PhpStr::from(s))),
    )))
}

fn list_dir(vm: &Vm, d: &[u8]) -> std::io::Result<Vec<Vec<u8>>> {
    // glob://PATTERN lists the names matched, inside open_basedir (PHP
    // manual: glob:// wrapper; tests streams/gh17067.phpt,
    // streams/glob-wrapper.phpt)
    if d.len() >= 7 && d[..7].eq_ignore_ascii_case(b"glob://") {
        let mut found = Vec::new();
        glob_one(vm, &d[7..], 0, &mut found);
        return Ok(found
            .into_iter()
            .filter(|p| vm.basedir_allows(p))
            .map(|p| basename_bytes(&p, b""))
            .collect());
    }
    let mut names: Vec<Vec<u8>> = vec![b".".to_vec(), b"..".to_vec()];
    for e in std::fs::read_dir(vm.fs_path(d))? {
        names.push(e?.file_name().into_vec());
    }
    Ok(names)
}

fn scandir(vm: &mut Vm, a: &mut Args) -> R<Value> {
    if !nul_arg(vm, a, 0, "directory")? {
        return Ok(Value::Null);
    }
    if !basedir_check(vm, a, 0)? {
        return Ok(Value::Bool(false));
    }
    let d = a.str(0);
    if d.is_empty() {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #1 ($directory) must not be empty",
        );
    }
    let order = a.int_or(1, 0);
    let listed = match super::userstream::wrapper_for(vm, &d) {
        Some(cls) => match super::userstream::list(vm, a, &cls, &d)? {
            Some(n) => Ok(n),
            None => return Ok(Value::Bool(false)),
        },
        None => list_dir(vm, &d),
    };
    match listed {
        Ok(mut names) => {
            // SCANDIR_SORT_NONE (2) keeps the directory order, any other
            // non-zero value sorts descending (test scandir_variation9)
            match order {
                0 => names.sort(),
                2 => {}
                _ => {
                    names.sort();
                    names.reverse();
                }
            }
            Ok(Value::Array(PhpArray::from_values(
                names.into_iter().map(|s| Value::Str(PhpStr::from(s))),
            )))
        }
        Err(e) => {
            pwarn(
                vm,
                a,
                &d,
                &format!("Failed to open directory: {}", &io_err_msg(&e)),
            )?;
            vm.fwarn(a, "(errno 2): No such file or directory")?;
            Ok(Value::Bool(false))
        }
    }
}

pub(crate) fn opendir(vm: &mut Vm, a: &mut Args) -> R<Value> {
    if !nul_arg(vm, a, 0, "directory")? {
        return Ok(Value::Null);
    }
    if !basedir_check(vm, a, 0)? {
        pwarn(
            vm,
            a,
            &a.str(0),
            "Failed to open directory: Operation not permitted",
        )?;
        return Ok(Value::Bool(false));
    }
    let d = a.str(0);
    if d.is_empty() {
        return Ok(Value::Bool(false));
    }
    if let Some(cls) = super::userstream::wrapper_for(vm, &d) {
        return Ok(match super::userstream::opendir(vm, a, &cls, &d)? {
            Some(r) => {
                vm.last_dir = Some(r.clone());
                Value::Resource(r)
            }
            None => Value::Bool(false),
        });
    }
    match list_dir(vm, &d) {
        Ok(entries) => {
            let s = Stream::new(
                Backing::Dir { entries, pos: 0 },
                b"r",
                &d,
                "plainfile",
                "dir",
            );
            let r = new_stream_res(s);
            vm.last_dir = Some(r.clone());
            Ok(Value::Resource(r))
        }
        Err(e) => {
            pwarn(
                vm,
                a,
                &d,
                &format!("Failed to open directory: {}", &io_err_msg(&e)),
            )?;
            Ok(Value::Bool(false))
        }
    }
}

/// The directory handle argument: the last opened directory when null
/// (deprecated), TypeErrors for closed or non-directory resources
/// (observed on PHP 8.5).
fn dir_arg(vm: &mut Vm, a: &Args) -> R<ResRef> {
    let r = match a.val(0) {
        Value::Resource(r) => r,
        _ => {
            vm.fdeprecated(
                a,
                "Passing null is deprecated, instead the last opened directory stream should be provided",
            )?;
            match vm.last_dir.clone() {
                Some(r) => r,
                None => return vm.throw("TypeError", "No resource supplied"),
            }
        }
    };
    if r.is_closed() {
        return vm.fthrow(
            a,
            "TypeError",
            "Argument #1 ($dir_handle) must be an open stream resource",
        );
    }
    let is_dir = {
        let data = r.res().data.borrow();
        data.as_ref()
            .and_then(|d| d.downcast_ref::<std::cell::RefCell<Stream>>())
            .is_some_and(|c| {
                c.try_borrow().is_ok_and(|s| {
                    matches!(s.backing, Backing::Dir { .. } | Backing::UserDir { .. })
                })
            })
    };
    if !is_dir {
        return vm.fthrow(
            a,
            "TypeError",
            "Argument #1 ($dir_handle) must be a valid Directory resource",
        );
    }
    Ok(r)
}

pub(crate) fn readdir(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let r = dir_arg(vm, a)?;
    let mut tmp = vec![Value::Resource(r)];
    let args = Args {
        this: None,
        vals: &mut tmp,
        func: a.func,
        static_class: std::ptr::null(),
        named: None,
    };
    with_stream(vm, &args, 0, |vm, s| {
        if let Backing::UserDir { obj } = &s.backing {
            let o = obj.clone();
            return Ok(
                match super::userstream::call(vm, &o, b"dir_readdir", Vec::new())? {
                    Some(Value::Str(n)) => Value::Str(n),
                    Some(Value::Bool(false)) | None => Value::Bool(false),
                    Some(v) => Value::Str(vm.to_php_str(&v)?),
                },
            );
        }
        if let Backing::Dir { entries, pos } = &mut s.backing
            && *pos < entries.len()
        {
            *pos += 1;
            return Ok(Value::Str(PhpStr::new(&entries[*pos - 1])));
        }
        Ok(Value::Bool(false))
    })
}

pub(crate) fn rewinddir(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let r = dir_arg(vm, a)?;
    let mut tmp = vec![Value::Resource(r)];
    let args = Args {
        this: None,
        vals: &mut tmp,
        func: a.func,
        static_class: std::ptr::null(),
        named: None,
    };
    with_stream(vm, &args, 0, |vm, s| {
        if let Backing::UserDir { obj } = &s.backing {
            let o = obj.clone();
            super::userstream::call(vm, &o, b"dir_rewinddir", Vec::new())?;
        }
        if let Backing::Dir { pos, .. } = &mut s.backing {
            *pos = 0;
        }
        Ok(Value::Null)
    })
}

pub(crate) fn closedir(vm: &mut Vm, a: &mut Args) -> R<Value> {
    {
        let r = dir_arg(vm, a)?;
        let user = {
            let data = r.res().data.borrow();
            data.as_ref()
                .and_then(|d| d.downcast_ref::<std::cell::RefCell<Stream>>())
                .and_then(|c| match &c.borrow().backing {
                    Backing::UserDir { obj } => Some(obj.clone()),
                    _ => None,
                })
        };
        if let Some(o) = user {
            super::userstream::call(vm, &o, b"dir_closedir", Vec::new())?;
        }
        r.close();
    }
    Ok(Value::Null)
}

fn tempnam(vm: &mut Vm, a: &mut Args) -> R<Value> {
    if !nul_arg(vm, a, 0, "directory")? {
        return Ok(Value::Null);
    }
    if !nul_arg(vm, a, 1, "prefix")? {
        return Ok(Value::Null);
    }
    if !basedir_check(vm, a, 0)? {
        return Ok(Value::Bool(false));
    }
    let d = a.str(0);
    let prefix = a.str(1);
    // the prefix is a file name, 63 bytes at most (tests
    // tempnam_variation3.phpt, tempnam_variation9.phpt)
    let prefix: Vec<u8> = basename_bytes(&prefix, b"").into_iter().take(63).collect();
    let mut dir = if d.is_empty() {
        PathBuf::from("/tmp")
    } else {
        vm.fs_path(&d)
    };
    if !d.is_empty() && !dir.is_dir() {
        vm.notice_msg("tempnam(): file created in the system's temporary directory")?;
        dir = PathBuf::from("/tmp");
    }
    // the name is made in the resolved directory (test
    // tempnam_variation2.phpt)
    if let Ok(c) = std::fs::canonicalize(&dir) {
        dir = c;
    }
    let tmp = PathBuf::from("/tmp");
    // a directory we can't write to falls back to the temporary one (test
    // tempnam_variation4-0.phpt)
    if dir != tmp {
        let c = std::ffi::CString::new(dir.as_os_str().as_bytes()).unwrap_or_default();
        if unsafe { libc::access(c.as_ptr(), libc::W_OK | libc::X_OK) } != 0 {
            vm.notice_msg("tempnam(): file created in the system's temporary directory")?;
            dir = tmp;
        }
    }
    for n in 0..1000u32 {
        // 19 random letters and digits follow (observed on PHP 8.5)
        let name = {
            let mut v = prefix.clone();
            let t = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default();
            let mut x = (t.as_nanos() as u64)
                ^ ((std::process::id() as u64) << 32)
                ^ (n as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
            const ALNUM: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ";
            for _ in 0..19 {
                // xorshift64*
                x ^= x >> 12;
                x ^= x << 25;
                x ^= x >> 27;
                let r = x.wrapping_mul(0x2545_F491_4F6C_DD1D);
                v.push(ALNUM[(r >> 33) as usize % ALNUM.len()]);
            }
            v
        };
        let p = dir.join(std::ffi::OsStr::from_bytes(&name));
        if std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&p)
            .is_ok()
        {
            let _ = std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600));
            return Ok(Value::Str(PhpStr::new(p.as_os_str().as_bytes())));
        }
    }
    Ok(Value::Bool(false))
}

fn tmpfile(_vm: &mut Vm, _a: &mut Args) -> R<Value> {
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let p = PathBuf::from(format!(
        "/tmp/php{:x}{:x}",
        std::process::id(),
        t.subsec_nanos()
    ));
    match std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(&p)
    {
        Ok(f) => {
            let _ = std::fs::remove_file(&p);
            Ok(Value::Resource(new_stream_res(Stream::new(
                Backing::File(f),
                b"r+b",
                p.as_os_str().as_bytes(),
                "plainfile",
                "STDIO",
            ))))
        }
        Err(_) => Ok(Value::Bool(false)),
    }
}

fn getcwd(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    Ok(Value::Str(PhpStr::new(vm.cwd.as_os_str().as_bytes())))
}

fn chdir(vm: &mut Vm, a: &mut Args) -> R<Value> {
    if !nul_arg(vm, a, 0, "directory")? {
        return Ok(Value::Null);
    }
    if !basedir_check(vm, a, 0)? {
        return Ok(Value::Bool(false));
    }
    let d = a.str(0);
    let p = vm.fs_path(&d);
    if p.is_dir() {
        let c = std::fs::canonicalize(&p).unwrap_or(p);
        let _ = std::env::set_current_dir(&c);
        vm.cwd = c;
        Ok(Value::Bool(true))
    } else {
        let msg = if p.exists() {
            "Not a directory (errno 20)"
        } else {
            "No such file or directory (errno 2)"
        };
        vm.fwarn(a, msg)?;
        Ok(Value::Bool(false))
    }
}

fn umask(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let old = current_umask();
    if let Some(m) = a.opt_int(0) {
        unsafe { libc::umask(m as libc::mode_t) };
    }
    Ok(Value::Int(old as i64))
}

fn readlink(vm: &mut Vm, a: &mut Args) -> R<Value> {
    if !nul_arg(vm, a, 0, "path")? {
        return Ok(Value::Null);
    }
    if !basedir_check(vm, a, 0)? {
        return Ok(Value::Bool(false));
    }
    let f = a.str(0);
    match std::fs::read_link(vm.fs_path(&f)) {
        Ok(p) => Ok(Value::Str(PhpStr::new(p.as_os_str().as_bytes()))),
        Err(e) => {
            vm.fwarn(a, &io_err_msg(&e))?;
            Ok(Value::Bool(false))
        }
    }
}

fn symlink(vm: &mut Vm, a: &mut Args) -> R<Value> {
    if !nul_arg(vm, a, 0, "target")? {
        return Ok(Value::Null);
    }
    if !nul_arg(vm, a, 1, "link")? {
        return Ok(Value::Null);
    }
    if !basedir_check(vm, a, 0)? || !basedir_check(vm, a, 1)? {
        return Ok(Value::Bool(false));
    }
    let t = a.str(0);
    let l = a.str(1);
    match std::os::unix::fs::symlink(os(&t), vm.fs_path(&l)) {
        Ok(()) => Ok(Value::Bool(true)),
        Err(e) => {
            vm.fwarn(a, &io_err_msg(&e))?;
            Ok(Value::Bool(false))
        }
    }
}

fn link(vm: &mut Vm, a: &mut Args) -> R<Value> {
    if !nul_arg(vm, a, 0, "target")? {
        return Ok(Value::Null);
    }
    if !nul_arg(vm, a, 1, "link")? {
        return Ok(Value::Null);
    }
    if !basedir_check(vm, a, 0)? || !basedir_check(vm, a, 1)? {
        return Ok(Value::Bool(false));
    }
    let t = a.str(0);
    let l = a.str(1);
    match std::fs::hard_link(vm.fs_path(&t), vm.fs_path(&l)) {
        Ok(()) => Ok(Value::Bool(true)),
        Err(e) => {
            vm.fwarn(a, &io_err_msg(&e))?;
            Ok(Value::Bool(false))
        }
    }
}

fn linkinfo(vm: &mut Vm, a: &mut Args) -> R<Value> {
    if !nul_arg(vm, a, 0, "path")? {
        return Ok(Value::Null);
    }
    if !basedir_check(vm, a, 0)? {
        return Ok(Value::Bool(false));
    }
    let f = a.str(0);
    match std::fs::symlink_metadata(vm.fs_path(&f)) {
        Ok(m) => Ok(Value::Int(m.dev() as i64)),
        Err(e) => {
            vm.fwarn(a, &io_err_msg(&e))?;
            Ok(Value::Int(-1))
        }
    }
}

fn disk_space(vm: &mut Vm, a: &mut Args, total: bool) -> R<Value> {
    if !nul_arg(vm, a, 0, "directory")? {
        return Ok(Value::Null);
    }
    if !basedir_check(vm, a, 0)? {
        return Ok(Value::Bool(false));
    }
    let d = a.str(0);
    if d.is_empty() {
        return Ok(Value::Bool(false));
    }
    let p = vm.fs_path(&d);
    let Ok(c) = std::ffi::CString::new(p.as_os_str().as_bytes()) else {
        return Ok(Value::Bool(false));
    };
    let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(c.as_ptr(), &mut st) } != 0 {
        let e = std::io::Error::last_os_error();
        vm.fwarn(a, &io_err_msg(&e))?;
        return Ok(Value::Bool(false));
    }
    let blocks = if total { st.f_blocks } else { st.f_bavail };
    Ok(Value::Float(blocks as f64 * st.f_frsize as f64))
}

fn disk_free_space(vm: &mut Vm, a: &mut Args) -> R<Value> {
    disk_space(vm, a, false)
}

fn disk_total_space(vm: &mut Vm, a: &mut Args) -> R<Value> {
    disk_space(vm, a, true)
}

/// chown(), chgrp(), lchown(), lchgrp(): a user or group name or id.
fn change_owner(vm: &mut Vm, a: &mut Args, group: bool, link: bool) -> R<Value> {
    if !nul_arg(vm, a, 0, "filename")? {
        return Ok(Value::Null);
    }
    if !basedir_check(vm, a, 0)? {
        return Ok(Value::Bool(false));
    }
    let f = a.str(0);
    if let Some(cls) = super::userstream::wrapper_for(vm, &f) {
        let opt = match (group, a.val(1)) {
            (false, Value::Int(_)) => 3,
            (false, _) => 2,
            (true, Value::Int(_)) => 5,
            (true, _) => 4,
        };
        return super::userstream::path_op(
            vm,
            a,
            &cls,
            "stream_metadata",
            vec![Value::Str(f), Value::Int(opt), a.val(1)],
        );
    }
    let id: u32 = match a.val(1) {
        Value::Int(n) => n as u32,
        v => {
            let name = vm.to_php_str(&v)?;
            let Ok(cn) = std::ffi::CString::new(name.to_vec()) else {
                return Ok(Value::Bool(false));
            };
            let found = if group {
                let g = unsafe { libc::getgrnam(cn.as_ptr()) };
                (!g.is_null()).then(|| unsafe { (*g).gr_gid })
            } else {
                let u = unsafe { libc::getpwnam(cn.as_ptr()) };
                (!u.is_null()).then(|| unsafe { (*u).pw_uid })
            };
            match found {
                Some(id) => id,
                None => {
                    let what = if group { "group" } else { "user" };
                    vm.fwarn(
                        a,
                        &format!("Unable to find {what} ID for {}", name.to_string_lossy()),
                    )?;
                    return Ok(Value::Bool(false));
                }
            }
        }
    };
    let p = vm.fs_path(&f);
    let Ok(c) = std::ffi::CString::new(p.as_os_str().as_bytes()) else {
        return Ok(Value::Bool(false));
    };
    let (uid, gid) = if group {
        (u32::MAX, id)
    } else {
        (id, u32::MAX)
    };
    let rc = unsafe {
        if link {
            libc::lchown(c.as_ptr(), uid, gid)
        } else {
            libc::chown(c.as_ptr(), uid, gid)
        }
    };
    if rc != 0 {
        let e = std::io::Error::last_os_error();
        vm.fwarn(a, &io_err_msg(&e))?;
        return Ok(Value::Bool(false));
    }
    Ok(Value::Bool(true))
}

fn chown(vm: &mut Vm, a: &mut Args) -> R<Value> {
    change_owner(vm, a, false, false)
}
fn chgrp(vm: &mut Vm, a: &mut Args) -> R<Value> {
    change_owner(vm, a, true, false)
}
fn lchown(vm: &mut Vm, a: &mut Args) -> R<Value> {
    change_owner(vm, a, false, true)
}
fn lchgrp(vm: &mut Vm, a: &mut Args) -> R<Value> {
    change_owner(vm, a, true, true)
}

// ---------------------------------------------------------------- CSV

/// One CSV record as fputcsv writes it.
pub fn csv_line(
    vm: &mut Vm,
    fields: &PhpArray,
    sep: u8,
    enc: Option<u8>,
    esc: Option<u8>,
    eol: &[u8],
) -> R<Vec<u8>> {
    let mut out = Vec::new();
    let mut first = true;
    for (_, v) in fields.iter() {
        if !first {
            out.push(sep);
        }
        first = false;
        let s = vm.to_php_str(&v.deref())?;
        let needs = enc.is_some()
            && s.iter().any(|&c| {
                c == sep
                    || Some(c) == enc
                    || c == b'\n'
                    || c == b'\r'
                    || c == b'\t'
                    || c == b' '
                    || Some(c) == esc
            });
        if needs {
            let e = enc.unwrap();
            out.push(e);
            let mut escaped = false;
            for &c in s.iter() {
                if escaped {
                    escaped = false;
                } else if Some(c) == esc {
                    escaped = true;
                } else if c == e {
                    out.push(e);
                }
                out.push(c);
            }
            out.push(e);
        } else {
            out.extend_from_slice(&s);
        }
    }
    out.extend_from_slice(eol);
    Ok(out)
}

/// Parses one CSV record from `data` starting at `*pos`.
pub fn csv_parse(
    data: &[u8],
    pos: &mut usize,
    sep: u8,
    enc: Option<u8>,
    esc: Option<u8>,
) -> Vec<Vec<u8>> {
    let mut fields = Vec::new();
    let mut field = Vec::new();
    let mut i = *pos;
    let n = data.len();
    loop {
        // Leading whitespace before an enclosure is skipped.
        let mut j = i;
        while j < n && (data[j] == b' ' || data[j] == b'\t') && Some(data[j]) != enc {
            j += 1;
        }
        if let Some(e) = enc
            && j < n
            && data[j] == e
        {
            i = j + 1;
            loop {
                if i >= n {
                    break;
                }
                let c = data[i];
                if Some(c) == esc && esc != Some(e) && i + 1 < n {
                    field.push(c);
                    field.push(data[i + 1]);
                    i += 2;
                    continue;
                }
                if c == e {
                    if i + 1 < n && data[i + 1] == e {
                        field.push(e);
                        i += 2;
                        continue;
                    }
                    i += 1;
                    break;
                }
                field.push(c);
                i += 1;
            }
            // Anything until the separator is appended.
            while i < n && data[i] != sep && data[i] != b'\n' && data[i] != b'\r' {
                field.push(data[i]);
                i += 1;
            }
        } else {
            while i < n && data[i] != sep && data[i] != b'\n' && data[i] != b'\r' {
                field.push(data[i]);
                i += 1;
            }
        }
        fields.push(std::mem::take(&mut field));
        if i < n && data[i] == sep {
            i += 1;
            continue;
        }
        if i < n && data[i] == b'\r' {
            i += 1;
        }
        if i < n && data[i] == b'\n' {
            i += 1;
        }
        break;
    }
    *pos = i;
    fields
}

/// Whether a record ends inside an enclosure (it goes on on the next
/// line), following csv_parse().
fn csv_open(data: &[u8], sep: u8, enc: Option<u8>, esc: Option<u8>) -> bool {
    let Some(e) = enc else {
        return false;
    };
    let n = data.len();
    let mut i = 0;
    loop {
        let mut j = i;
        while j < n && (data[j] == b' ' || data[j] == b'\t') && data[j] != e {
            j += 1;
        }
        if j < n && data[j] == e {
            i = j + 1;
            loop {
                if i >= n {
                    return true;
                }
                let c = data[i];
                if Some(c) == esc && esc != Some(e) && i + 1 < n {
                    i += 2;
                    continue;
                }
                if c == e {
                    if i + 1 < n && data[i + 1] == e {
                        i += 2;
                        continue;
                    }
                    i += 1;
                    break;
                }
                i += 1;
            }
        }
        while i < n && data[i] != sep && data[i] != b'\n' && data[i] != b'\r' {
            i += 1;
        }
        if i < n && data[i] == sep {
            i += 1;
            continue;
        }
        return false;
    }
}

fn csv_opts(vm: &mut Vm, a: &Args, base: usize) -> R<(u8, Option<u8>, Option<u8>)> {
    let sep = a.opt_str(base).unwrap_or_else(|| PhpStr::from(","));
    let enc = a.opt_str(base + 1).unwrap_or_else(|| PhpStr::from("\""));
    let esc = a.opt_str(base + 2).unwrap_or_else(|| PhpStr::from("\\"));
    if sep.len() != 1 {
        return vm.fthrow(
            a,
            "ValueError",
            &format!(
                "Argument #{} ($separator) must be a single character",
                base + 1
            ),
        );
    }
    if enc.len() != 1 {
        return vm.fthrow(
            a,
            "ValueError",
            &format!(
                "Argument #{} ($enclosure) must be a single character",
                base + 2
            ),
        );
    }
    if esc.len() > 1 {
        return vm.fthrow(
            a,
            "ValueError",
            &format!(
                "Argument #{} ($escape) must be empty or a single character",
                base + 3
            ),
        );
    }
    if !a.has(base + 2) {
        vm.fdeprecated(
            a,
            "the $escape parameter must be provided as its default value will change",
        )?;
    }
    Ok((sep[0], Some(enc[0]), esc.first().copied()))
}

fn fputcsv(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let fields = a.array(1);
    let (sep, enc, esc) = csv_opts(vm, a, 2)?;
    let eol = a.opt_str(5).unwrap_or_else(|| PhpStr::from("\n"));
    let line = csv_line(vm, &fields, sep, enc, esc, &eol)?;
    with_stream(vm, a, 0, |vm, s| match s.write_all(vm, &line) {
        Ok(n) => Ok(Value::Int(n as i64)),
        Err(_) => Ok(Value::Bool(false)),
    })
}

fn fgetcsv(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let len = a.opt_int(1).unwrap_or(0);
    if !(0..i64::MAX).contains(&len) {
        return vm.fthrow(
            a,
            "ValueError",
            &format!(
                "Argument #2 ($length) must be between 0 and {}",
                i64::MAX - 1
            ),
        );
    }
    let (sep, enc, esc) = csv_opts(vm, a, 2)?;
    with_stream(vm, a, 0, |vm, s| {
        // Read a logical line (enclosures may span lines), at most
        // $length bytes when given.
        let mut buf = Vec::new();
        let mut in_enc = false;
        let max = (len > 0).then_some(len as usize);
        // (a line cut inside an enclosure goes on unlimited, test
        // bug12556.phpt)
        while let Some(line) = read_line(vm, s, if buf.is_empty() { max } else { None }) {
            buf.extend_from_slice(&line);
            in_enc = csv_open(&buf, sep, enc, esc);
            if !in_enc {
                break;
            }
        }
        let _ = in_enc;
        if buf.is_empty() {
            return Ok(Value::Bool(false));
        }
        if buf == b"\n" || buf == b"\r\n" {
            return Ok(Value::Array(PhpArray::from_values([Value::Null])));
        }
        let mut pos = 0;
        let fields = csv_parse(&buf, &mut pos, sep, enc, esc);
        Ok(Value::Array(PhpArray::from_values(
            fields.into_iter().map(|f| Value::Str(PhpStr::from(f))),
        )))
    })
}

fn str_getcsv(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let s = a.str(0);
    let (sep, enc, esc) = csv_opts(vm, a, 1)?;
    if s.is_empty() {
        return Ok(Value::Array(PhpArray::from_values([Value::Null])));
    }
    let mut pos = 0;
    let mut data = s.to_vec();
    // A string is one record; embedded newlines stay in the last field.
    let fields = {
        let mut all = Vec::new();
        loop {
            let f = csv_parse(&data, &mut pos, sep, enc, esc);
            if all.is_empty() {
                all = f;
            } else {
                // Rejoin: str_getcsv doesn't split records.
                let last = all.pop().unwrap_or_default();
                let mut l = last;
                l.push(b'\n');
                let mut it = f.into_iter();
                if let Some(first) = it.next() {
                    l.extend_from_slice(&first);
                }
                all.push(l);
                all.extend(it);
            }
            if pos >= data.len() {
                break;
            }
        }
        all
    };
    data.clear();
    Ok(Value::Array(PhpArray::from_values(
        fields.into_iter().map(|f| Value::Str(PhpStr::from(f))),
    )))
}

/// Writes formatted output to a stream (fprintf, vfprintf).
fn write_formatted(vm: &mut Vm, a: &mut Args, out: Vec<u8>) -> R<Value> {
    with_stream(vm, a, 0, |vm, s| {
        if !s.writable {
            vm.notice_msg(&format!(
                "{}(): Write of {} bytes failed with errno=9 Bad file descriptor",
                a.fname(),
                out.len()
            ))?;
            return Ok(Value::Int(0));
        }
        let _ = s.write_all(vm, &out);
        Ok(Value::Int(out.len() as i64))
    })
}

fn fprintf(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let fmt = a.str(1);
    let vals: Vec<Value> = a.vals[2..].to_vec();
    let out = super::string::format_values(
        vm,
        a,
        &fmt,
        &vals,
        goro_ext_standard::printf::ArgsKind::Variadic,
    )?;
    write_formatted(vm, a, out)
}

fn vfprintf(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let fmt = a.str(1);
    let vals: Vec<Value> = a.array(2).iter().map(|(_, v)| v.deref()).collect();
    let out = super::string::format_values(
        vm,
        a,
        &fmt,
        &vals,
        goro_ext_standard::printf::ArgsKind::Array,
    )?;
    write_formatted(vm, a, out)
}

fn fscanf(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let fmt = a.str(1);
    if let Value::Resource(r) = a.val(0)
        && (r.is_closed() || !is_stream_res(&r))
    {
        return vm.fthrow(
            a,
            "TypeError",
            "supplied resource is not a valid File-Handle resource",
        );
    }
    let line = with_stream(vm, a, 0, |vm, s| {
        if !s.readable {
            vm.notice_msg("fscanf(): Read of 8192 bytes failed with errno=9 Bad file descriptor")?;
            return Ok(Value::Bool(false));
        }
        let r = read_line(vm, s, None);
        if let Backing::Memory { data, pos } = &s.backing
            && *pos >= data.len()
        {
            s.eof = true;
        }
        Ok(match r {
            Some(l) => Value::Str(PhpStr::from(l)),
            None => Value::Bool(false),
        })
    })?;
    match line {
        Value::Str(l) => super::strmisc::scan_into(vm, a, &l, &fmt, 2),
        other => Ok(other),
    }
}

/// open_basedir (PHP manual: Description of core php.ini directives): the
/// path argument `i` must be inside one of the allowed directories; warns
/// and gives false otherwise.
fn basedir_check(vm: &mut Vm, a: &Args, i: usize) -> R<bool> {
    let Some(Value::Str(p)) = a.get(i).map(|v| v.deref()) else {
        return Ok(true);
    };
    if p.is_empty() || vm.basedir_allows(&p) {
        return Ok(true);
    }
    let bd = vm
        .get_ini(b"open_basedir")
        .map(|v| v.to_string_lossy())
        .unwrap_or_default();
    vm.fwarn(
        a,
        &format!(
            "open_basedir restriction in effect. File({}) is not within the allowed path(s): ({})",
            p.to_string_lossy(),
            bd
        ),
    )?;
    Ok(false)
}

impl Vm {
    /// Whether `path` is inside the open_basedir directories (always true
    /// when it is not set). Paths resolve like realpath, with a missing
    /// last part resolved through its directory.
    pub fn basedir_allows(&self, path: &[u8]) -> bool {
        let Some(bd) = self.get_ini(b"open_basedir") else {
            return true;
        };
        if bd.is_empty() {
            return true;
        }
        if path.windows(3).any(|w| w == b"://") && !path.starts_with(b"file://") {
            return true;
        }
        let target = self.basedir_resolve(path);
        for d in bd.split(|&c| c == b':') {
            if d.is_empty() {
                continue;
            }
            let dir = self.basedir_resolve(d);
            let dir_b = dir.as_os_str().as_bytes();
            let t_b = target.as_os_str().as_bytes();
            // A trailing slash requires the directory itself; otherwise the
            // value is a prefix (manual: "/dir/incl" also allows
            // "/dir/include").
            if d.ends_with(b"/") {
                // ("/" allows everything, test bug41655_2.phpt)
                if t_b == dir_b
                    || (t_b.starts_with(dir_b)
                        && (dir_b.ends_with(b"/") || t_b.get(dir_b.len()) == Some(&b'/')))
                {
                    return true;
                }
            } else if t_b.starts_with(dir_b)
                && (t_b.len() == dir_b.len()
                    || t_b[dir_b.len()] == b'/'
                    // an existing directory is a directory, not a name
                    // prefix (test ext/openssl/tests/gh9310.phpt: basedir
                    // ".../gh9310" refuses ".../gh9310.crt")
                    || !dir.is_dir())
            {
                return true;
            }
        }
        false
    }

    fn basedir_resolve(&self, p: &[u8]) -> PathBuf {
        let full = self.fs_path(p);
        if let Ok(c) = std::fs::canonicalize(&full) {
            return c;
        }
        // Lexical normalization, then the existing prefix canonicalized.
        let mut out = PathBuf::from("/");
        for comp in full.components() {
            match comp {
                std::path::Component::ParentDir => {
                    out.pop();
                }
                std::path::Component::Normal(n) => out.push(n),
                _ => {}
            }
        }
        if let (Some(parent), Some(name)) = (out.parent(), out.file_name())
            && let Ok(c) = std::fs::canonicalize(parent)
        {
            return c.join(name);
        }
        out
    }
}

/// `data:` URLs (RFC 2397; PHP manual: data://): the decoded data as a
/// read-only memory stream. Messages observed on PHP 8.5.
fn open_data_url(vm: &mut Vm, a: &Args, url: &[u8], mode: &[u8], quiet: bool) -> R<Option<Stream>> {
    if url.contains(&0) {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #1 ($filename) must not contain any null bytes",
        );
    }
    let mut rest = &url[5..];
    if let Some(r) = rest.strip_prefix(b"//") {
        rest = r;
    }
    let fail = |vm: &mut Vm, why: &str| -> R<Option<Stream>> {
        if !quiet {
            vm.warn(&format!(
                "{}({}): Failed to open stream: rfc2397: {}",
                a.fname(),
                String::from_utf8_lossy(url),
                why
            ))?;
        }
        Ok(None)
    };
    let Some(comma) = rest.iter().position(|&c| c == b',') else {
        return fail(vm, "no comma in URL");
    };
    let meta = &rest[..comma];
    let payload = &rest[comma + 1..];
    // [mediatype][;param=value]*[;base64] (RFC 2397; messages from test
    // stream_rfc2397_002.phpt)
    let mut parts = meta.split(|&c| c == b';');
    let media = parts.next().unwrap_or_default();
    let params: Vec<&[u8]> = parts.collect();
    let mut dm = PhpArray::new();
    if !media.is_empty() {
        if !media.contains(&b'/') || media.contains(&b'=') {
            return fail(vm, "illegal media type");
        }
        dm.insert(Key::from("mediatype"), Value::Str(PhpStr::new(media)));
    }
    let mut base64 = false;
    for (i, prm) in params.iter().enumerate() {
        if prm.eq_ignore_ascii_case(b"base64") && i + 1 == params.len() {
            base64 = true;
            break;
        }
        // parameters need a media type
        let Some(eq) = prm.iter().position(|&c| c == b'=') else {
            return fail(vm, "illegal parameter");
        };
        if media.is_empty() {
            return fail(vm, "illegal media type");
        }
        if &prm[..eq] == b"mediatype" {
            continue;
        }
        dm.insert(
            Key::from_bytes(&prm[..eq]),
            Value::Str(PhpStr::new(&prm[eq + 1..])),
        );
    }
    dm.insert(Key::from("base64"), Value::Bool(base64));
    let data = if base64 {
        match goro_ext_standard::url::base64_decode(payload, true) {
            Some(d) => d,
            None => return fail(vm, "unable to decode"),
        }
    } else {
        goro_ext_standard::url::urldecode(payload)
    };
    let mut s = Stream::new(
        Backing::Memory { data, pos: 0 },
        mode,
        url,
        "RFC2397",
        "RFC2397",
    );
    s.writable = false;
    s.readable = true;
    s.data_meta = Some(dm);
    Ok(Some(s))
}

/// stream_copy_to_stream() (PHP manual): what is read from `$from` (from
/// `$offset`, at most `$length` bytes) is written to `$to`.
fn stream_copy_to_stream(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let len = a.opt_int(2);
    let off = a.int_or(3, 0);
    let data = with_stream(vm, a, 0, |vm, s| {
        if off > 0 && !do_seek(vm, s, off, 0) {
            vm.warn(&format!(
                "stream_copy_to_stream(): Failed to seek to position {} in the stream",
                off
            ))?;
            return Ok(None);
        }
        let mut out = Vec::new();
        let mut buf = vec![0u8; 8192];
        loop {
            let want = match len {
                Some(l) if l >= 0 => (l as usize - out.len()).min(buf.len()),
                _ => buf.len(),
            };
            if want == 0 {
                break;
            }
            match s.read_some(vm, &mut buf[..want]) {
                Ok(0) | Err(_) => {
                    s.eof = true;
                    break;
                }
                Ok(n) => out.extend_from_slice(&buf[..n]),
            }
        }
        Ok(Some(out))
    })?;
    vm.check_deferred()?;
    let Some(data) = data else {
        return Ok(Value::Bool(false));
    };
    with_stream(vm, a, 1, |vm, s| match s.write_all(vm, &data) {
        Ok(n) => Ok(Value::Int(n as i64)),
        Err(_) => Ok(Value::Bool(false)),
    })
}

/// stream_get_line() (PHP manual): up to `$length` bytes, stopping
/// before `$ending` (which is consumed).
fn stream_get_line(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let len = a.int(1);
    if len < 0 {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #2 ($length) must be greater than or equal to 0",
        );
    }
    let max = if len == 0 { 8192 } else { len as usize };
    let ending = a.str(2).to_vec();
    let r = with_stream(vm, a, 0, |vm, s| {
        let mut out = Vec::new();
        let mut b = [0u8; 1];
        while out.len() < max + ending.len().saturating_sub(1) {
            match s.read_some(vm, &mut b) {
                Ok(1) => {
                    out.push(b[0]);
                    if !ending.is_empty() && out.ends_with(&ending) {
                        out.truncate(out.len() - ending.len());
                        return Ok(Some(out));
                    }
                    if ending.is_empty() && out.len() >= max {
                        break;
                    }
                }
                _ => {
                    s.eof = true;
                    break;
                }
            }
            if !ending.is_empty() && out.len() >= max {
                break;
            }
        }
        Ok(if out.is_empty() { None } else { Some(out) })
    })?;
    vm.check_deferred()?;
    Ok(match r {
        Some(v) => Value::Str(PhpStr::from(v)),
        None => Value::Bool(false),
    })
}
