//! Files, directories and streams (PHP manual: Filesystem functions,
//! Directory functions, Streams; php:// wrappers). Plain files use the
//! real filesystem; php://memory, php://temp, php://stdin/stdout/stderr,
//! php://output and php://input are supported.

use std::io::{Read, Seek, SeekFrom, Write};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};

use goro_runtime::{Key, PhpArray, PhpStr, ResRef, Value};

use crate::class::TypeName;
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
        (
            "function stream_set_blocking($stream, bool $enable): bool",
            ret_true,
        ),
        (
            "function stream_set_write_buffer($stream, int $size): int",
            ret_zero,
        ),
        (
            "function stream_set_read_buffer($stream, int $size): int",
            ret_zero,
        ),
        (
            "function set_file_buffer($stream, int $size): int",
            ret_zero,
        ),
        (
            "function stream_context_create(?array $options = null, ?array $params = null)",
            stream_context_create,
        ),
        (
            "function stream_context_get_default(?array $options = null)",
            stream_context_create,
        ),
        (
            "function stream_context_set_option($context, array|string $wrapper_or_options, ?string $option_name = null, mixed $value = UNKNOWN): bool",
            ret_true,
        ),
        (
            "function stream_context_get_options($stream_or_context): array",
            empty_array,
        ),
        ("function stream_get_wrappers(): array", stream_get_wrappers),
        ("function stream_is_local($stream): bool", ret_true),
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
            ret_true,
        ),
        (
            "function chgrp(string $filename, string|int $group): bool",
            ret_true,
        ),
        ("function stat(string $filename): array|false", stat),
        ("function lstat(string $filename): array|false", lstat),
        (
            "function clearstatcache(bool $clear_realpath_cache = false, string $filename = \"\"): void",
            noop,
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
            disk_free_space,
        ),
        (
            "function is_uploaded_file(string $filename): bool",
            ret_false,
        ),
        (
            "function move_uploaded_file(string $from, string $to): bool",
            ret_false,
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
        ("function fsync($stream): bool", fflush),
        ("function fdatasync($stream): bool", fflush),
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
    listed: false,
    ..Module::EMPTY
};

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
    pub stream_type: &'static str,
    /// Bytes read ahead (stdin line reads, user stream chunks).
    pub pending: Vec<u8>,
    /// The position seen by the caller (user streams).
    pub upos: i64,
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
        }
    }

    pub fn read_some(&mut self, vm: &mut Vm, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.pending.is_empty() && matches!(self.backing, Backing::User { .. }) {
            if let Err(e) = super::userstream::fill(vm, self) {
                if vm.err.deferred.is_none() {
                    vm.err.deferred = Some(e);
                }
                return Err(std::io::Error::other("user stream error"));
            }
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
            Backing::File(f) => f.read(buf),
            Backing::Memory { data, pos } => {
                let n = buf.len().min(data.len().saturating_sub(*pos));
                buf[..n].copy_from_slice(&data[*pos..*pos + n]);
                *pos += n;
                Ok(n)
            }
            Backing::Stdin => std::io::stdin().read(buf),
            _ => Ok(0),
        }
    }

    pub fn write_all(&mut self, vm: &mut Vm, b: &[u8]) -> std::io::Result<usize> {
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
            _ => Ok(0),
        }
    }
}

pub fn new_stream_res(s: Stream) -> ResRef {
    ResRef::new(
        PhpStr::intern(b"stream"),
        Box::new(std::cell::RefCell::new(s)),
    )
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
                v.tname()
            ),
        );
    };
    let data = r.res().data.borrow();
    let Some(d) = data.as_ref() else {
        drop(data);
        return vm.fthrow(
            a,
            "TypeError",
            "supplied resource is not a valid stream resource",
        );
    };
    let Some(cell) = d.downcast_ref::<std::cell::RefCell<Stream>>() else {
        drop(data);
        return vm.fthrow(
            a,
            "TypeError",
            "supplied resource is not a valid stream resource",
        );
    };
    // SAFETY: the resource stays alive (held by `v`) for the call.
    let cell: *const std::cell::RefCell<Stream> = cell;
    drop(data);
    let mut s = unsafe { (*cell).borrow_mut() };
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
}

fn io_err_msg(e: &std::io::Error) -> &'static str {
    match e.kind() {
        std::io::ErrorKind::NotFound => "No such file or directory",
        std::io::ErrorKind::PermissionDenied => "Permission denied",
        std::io::ErrorKind::AlreadyExists => "File exists",
        _ => match e.raw_os_error() {
            Some(libc::EISDIR) => "Is a directory",
            Some(libc::ENOTDIR) => "Not a directory",
            Some(libc::ENOTEMPTY) => "Directory not empty",
            _ => "Operation failed",
        },
    }
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
    if let Some(cls) = super::userstream::wrapper_for(vm, filename) {
        return super::userstream::open(vm, a, &cls, filename, mode, quiet);
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
    if let Some(rest) = filename.strip_prefix(b"php://") {
        let lower = rest.to_ascii_lowercase();
        let s = match &lower[..] {
            b"stdin" => Stream::new(Backing::Stdin, mode, filename, "PHP", "STDIO"),
            b"stdout" => Stream::new(Backing::Stdout, mode, filename, "PHP", "STDIO"),
            b"stderr" => Stream::new(Backing::Stderr, mode, filename, "PHP", "STDIO"),
            b"output" => Stream::new(Backing::Output, b"wb", filename, "PHP", "Output"),
            b"input" => Stream::new(
                Backing::Memory {
                    data: Vec::new(),
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
                s.writable = true;
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
    let path = filename.strip_prefix(b"file://").unwrap_or(filename);
    let m = mode
        .iter()
        .filter(|&&c| c != b'b' && c != b't')
        .copied()
        .collect::<Vec<u8>>();
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
        b"c+" => o.read(true).write(true).create(true),
        _ => {
            return vm.fthrow(a, "ValueError", "Argument #2 ($mode) must be a valid mode");
        }
    };
    let p = vm.fs_path(path);
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

fn fopen(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let f = a.str(0);
    let m = a.str(1);
    if f.is_empty() {
        return vm.fthrow(a, "ValueError", "Argument #1 ($filename) cannot be empty");
    }
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
                v.tname()
            ),
        );
    };
    if r.is_closed() {
        return vm.fthrow(
            a,
            "TypeError",
            "supplied resource is not a valid stream resource",
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
            .and_then(|c| match &c.borrow().backing {
                Backing::User { obj } => Some(obj.clone()),
                _ => None,
            })
    };
    if let Some(o) = user {
        super::userstream::call(vm, &o, b"stream_flush", Vec::new())?;
        super::userstream::call(vm, &o, b"stream_close", Vec::new())?;
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
                n.min(8192)
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
                    if matches!(s.backing, Backing::Stdin) {
                        break;
                    }
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
            vm.notice_msg(&format!(
                "fwrite(): Write of {} bytes failed with errno=9 Bad file descriptor",
                len
            ))?;
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
        let r = read_line(vm, s, max.map(|m| (m as usize).saturating_sub(1).max(1)));
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
    with_stream(vm, a, 0, |_vm, s| {
        if matches!(s.backing, Backing::User { .. }) {
            return Ok(Value::Bool(s.eof && s.pending.is_empty()));
        }
        let at_end = match &mut s.backing {
            Backing::Memory { data, pos } => *pos >= data.len() && (s.eof || *pos >= data.len()),
            Backing::File(f) => {
                if s.eof {
                    true
                } else {
                    let cur = f.stream_position().unwrap_or(0);
                    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
                    cur >= len && s.eof
                }
            }
            _ => s.eof,
        };
        Ok(Value::Bool(
            at_end && s.eof || matches!(s.backing, Backing::Memory { .. }) && at_end,
        ))
    })
}

fn fflush(vm: &mut Vm, a: &mut Args) -> R<Value> {
    with_stream(vm, a, 0, |vm, s| {
        match &mut s.backing {
            Backing::File(f) => {
                let _ = f.flush();
            }
            Backing::Stdout | Backing::Output => vm.out.flush(),
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
            Backing::File(f) => Value::Int(f.stream_position().map(|p| p as i64).unwrap_or(0)),
            Backing::Memory { pos, .. } => Value::Int(*pos as i64),
            Backing::User { .. } => Value::Int(s.upos),
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
    s.eof = false;
    s.pending.clear();
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
            f.seek(w).is_ok()
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
        _ => false,
    }
}

fn fseek(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let off = a.int(1);
    let wh = a.int_or(2, 0);
    with_stream(vm, a, 0, |vm, s| {
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
            Backing::Memory { data, .. } => {
                data.resize(size as usize, 0);
                true
            }
            Backing::User { .. } => {
                let r = super::userstream::simple(vm, s, b"stream_truncate", vec![Value::Int(size)])?;
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
            return Ok(match super::userstream::simple(vm, s, b"stream_stat", Vec::new())? {
                Some(Value::Array(st)) => Value::Array(stat_from_user(&st)),
                _ => Value::Bool(false),
            });
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
        Ok(Value::Bool(true))
    })
}

fn read_rest(vm: &mut Vm, s: &mut Stream) -> Vec<u8> {
    let mut out = Vec::new();
    let mut buf = [0u8; 8192];
    loop {
        match s.read_some(vm, &mut buf) {
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
    with_stream(vm, a, 0, |vm, s| {
        if off >= 0 {
            do_seek(vm, s, off, 0);
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

fn stream_get_meta_data(vm: &mut Vm, a: &mut Args) -> R<Value> {
    with_stream(vm, a, 0, |_vm, s| {
        let mut m = PhpArray::new();
        m.insert(Key::from("timed_out"), Value::Bool(false));
        m.insert(Key::from("blocked"), Value::Bool(true));
        let eof = match &s.backing {
            Backing::Memory { data, pos } => *pos >= data.len() && s.eof || *pos >= data.len(),
            _ => s.eof,
        };
        m.insert(Key::from("eof"), Value::Bool(eof));
        if let Backing::User { obj } = &s.backing {
            m.insert(Key::from("wrapper_data"), Value::Object(obj.clone()));
        }
        m.insert(Key::from("wrapper_type"), Value::from(s.wrapper));
        m.insert(Key::from("stream_type"), Value::from(s.stream_type));
        m.insert(Key::from("mode"), Value::Str(s.mode.clone()));
        m.insert(Key::from("unread_bytes"), Value::Int(0));
        m.insert(
            Key::from("seekable"),
            Value::Bool(matches!(
                s.backing,
                Backing::File(_) | Backing::Memory { .. }
            )),
        );
        m.insert(Key::from("uri"), Value::Str(s.uri.clone()));
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

fn stream_context_create(_vm: &mut Vm, _a: &mut Args) -> R<Value> {
    Ok(Value::Resource(ResRef::new(
        PhpStr::intern(b"stream-context"),
        Box::new(()),
    )))
}

fn stream_get_wrappers(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    let mut out: Vec<Value> = ["php", "file", "glob", "data"]
        .iter()
        .filter(|s| !vm.disabled_wrappers.iter().any(|d| d.as_slice() == s.as_bytes()))
        .map(|s| Value::from(*s))
        .collect();
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
                if i >= 11 { -1 } else { 0 }
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
    let f = a.str(0);
    if f.is_empty() {
        return vm.fthrow(a, "ValueError", "Argument #1 ($filename) cannot be empty");
    }
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
    let bytes: Vec<u8> = match &data {
        Value::Array(arr) => {
            let mut out = Vec::new();
            for (_, v) in arr.iter() {
                vm.append_to_string(&mut out, &v.deref())?;
            }
            out
        }
        Value::Resource(_) => {
            let mut s = a.vals.clone();
            let _ = &mut s;
            let tmp_args = Args {
                this: None,
                vals: &mut vec![data.clone()],
                func: a.func,
                static_class: std::ptr::null(),
                named: None,
            };
            with_stream(vm, &tmp_args, 0, |vm, st| Ok(read_rest(vm, st)))?
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
    match s.write_all(vm, &bytes) {
        Ok(n) => Ok(Value::Int(n as i64)),
        Err(_) => Ok(Value::Bool(false)),
    }
}

fn file(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let f = a.str(0);
    let flags = a.int_or(1, 0);
    if f.is_empty() {
        return vm.fthrow(a, "ValueError", "Argument #1 ($filename) cannot be empty");
    }
    if !(0..=31).contains(&flags) {
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
            let end = if ignore_nl { i } else { i + 1 };
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
    let f = a.str(0);
    let Some(mut s) = open_stream(vm, a, &f, b"rb", false)? else {
        return Ok(Value::Bool(false));
    };
    let data = read_rest(vm, &mut s);
    vm.echo(&data);
    Ok(Value::Int(data.len() as i64))
}

fn fs_warn(vm: &mut Vm, a: &Args, what: &[u8], e: &std::io::Error) -> R<Value> {
    vm.fwarn(
        a,
        &format!("{}: {}", String::from_utf8_lossy(what), io_err_msg(e)),
    )?;
    Ok(Value::Bool(false))
}

fn unlink(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let f = a.str(0);
    if let Some(cls) = super::userstream::wrapper_for(vm, &f) {
        return super::userstream::path_op(vm, a, &cls, "unlink", vec![Value::Str(f)]);
    }
    let p = vm.fs_path(&f);
    if p.is_dir() {
        vm.fwarn(a, &format!("{}: Is a directory", f.to_string_lossy()))?;
        return Ok(Value::Bool(false));
    }
    match std::fs::remove_file(&p) {
        Ok(()) => Ok(Value::Bool(true)),
        Err(e) => fs_warn(vm, a, &f, &e),
    }
}

fn rename(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let from = a.str(0);
    let to = a.str(1);
    if let Some(cls) = super::userstream::wrapper_for(vm, &from) {
        return super::userstream::path_op(vm, a, &cls, "rename", vec![Value::Str(from), Value::Str(to)]);
    }
    match std::fs::rename(vm.fs_path(&from), vm.fs_path(&to)) {
        Ok(()) => Ok(Value::Bool(true)),
        Err(e) => {
            vm.fwarn(
                a,
                &format!(
                    "{},{}: {}",
                    from.to_string_lossy(),
                    to.to_string_lossy(),
                    io_err_msg(&e)
                ),
            )?;
            Ok(Value::Bool(false))
        }
    }
}

fn copy(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let from = a.str(0);
    let to = a.str(1);
    let Some(mut s) = open_stream(vm, a, &from, b"rb", false)? else {
        return Ok(Value::Bool(false));
    };
    let data = read_rest(vm, &mut s);
    let Some(mut d) = open_stream(vm, a, &to, b"wb", false)? else {
        return Ok(Value::Bool(false));
    };
    Ok(Value::Bool(d.write_all(vm, &data).is_ok()))
}

fn mkdir(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let d = a.str(0);
    let perms = a.int_or(1, 0o777);
    let rec = a.bool_or(2, false);
    if let Some(cls) = super::userstream::wrapper_for(vm, &d) {
        let opts = if rec { 1 | 8 } else { 8 };
        return super::userstream::path_op(vm, a, &cls, "mkdir", vec![Value::Str(d), Value::Int(perms), Value::Int(opts)]);
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
            vm.fwarn(a, io_err_msg(&e))?;
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
    let d = a.str(0);
    if let Some(cls) = super::userstream::wrapper_for(vm, &d) {
        return super::userstream::path_op(vm, a, &cls, "rmdir", vec![Value::Str(d), Value::Int(8)]);
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
    Ok(Some(super::userstream::url_stat(vm, &cls, &f, flags)?.map(|st| stat_from_user(&st))))
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
    std::fs::metadata(vm.fs_path(&f)).ok()
}

fn is_file(vm: &mut Vm, a: &mut Args) -> R<Value> {
    if let Some(st) = user_stat(vm, a, 2)? {
        return Ok(Value::Bool(st.is_some_and(|st| user_mode(&st) & 0o170000 == 0o100000)));
    }
    Ok(Value::Bool(meta(vm, a).is_some_and(|m| m.is_file())))
}
fn is_dir(vm: &mut Vm, a: &mut Args) -> R<Value> {
    if let Some(st) = user_stat(vm, a, 2)? {
        return Ok(Value::Bool(st.is_some_and(|st| user_mode(&st) & 0o170000 == 0o040000)));
    }
    Ok(Value::Bool(meta(vm, a).is_some_and(|m| m.is_dir())))
}
fn is_link(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let f = a.str(0);
    Ok(Value::Bool(
        std::fs::symlink_metadata(vm.fs_path(&f)).is_ok_and(|m| m.file_type().is_symlink()),
    ))
}
fn file_exists(vm: &mut Vm, a: &mut Args) -> R<Value> {
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
    Ok(Value::Bool(access(vm, a, libc::R_OK)))
}
fn is_writable(vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(Value::Bool(access(vm, a, libc::W_OK)))
}
fn is_executable(vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(Value::Bool(
        meta(vm, a).is_some_and(|m| m.is_file()) && access(vm, a, libc::X_OK),
    ))
}

fn stat_fail(vm: &mut Vm, a: &Args, what: &str) -> R<Value> {
    vm.fwarn(
        a,
        &format!("{} failed for {}", what, a.str(0).to_string_lossy()),
    )?;
    Ok(Value::Bool(false))
}

macro_rules! stat_fn {
    ($name:ident, $label:literal, $m:ident => $e:expr) => {
        fn $name(vm: &mut Vm, a: &mut Args) -> R<Value> {
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
    let f = a.str(0);
    if let Some(cls) = super::userstream::wrapper_for(vm, &f) {
        let times = match (a.opt_int(1), a.opt_int(2)) {
            (None, _) => PhpArray::new(),
            (Some(m), at) => PhpArray::from_values([Value::Int(m), Value::Int(at.unwrap_or(m))]),
        };
        return super::userstream::path_op(vm, a, &cls, "stream_metadata", vec![Value::Str(f), Value::Int(1), Value::Array(times)]);
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
    let f = a.str(0);
    let m = a.int(1);
    if let Some(cls) = super::userstream::wrapper_for(vm, &f) {
        return super::userstream::path_op(vm, a, &cls, "stream_metadata", vec![Value::Str(f), Value::Int(6), Value::Int(m)]);
    }
    match std::fs::set_permissions(vm.fs_path(&f), std::fs::Permissions::from_mode(m as u32)) {
        Ok(()) => Ok(Value::Bool(true)),
        Err(e) => {
            vm.fwarn(a, io_err_msg(&e))?;
            Ok(Value::Bool(false))
        }
    }
}

fn stat(vm: &mut Vm, a: &mut Args) -> R<Value> {
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

fn noop(_vm: &mut Vm, _a: &mut Args) -> R<Value> {
    Ok(Value::Null)
}
fn ret_true(_vm: &mut Vm, _a: &mut Args) -> R<Value> {
    Ok(Value::Bool(true))
}
fn ret_false(_vm: &mut Vm, _a: &mut Args) -> R<Value> {
    Ok(Value::Bool(false))
}
fn ret_zero(_vm: &mut Vm, _a: &mut Args) -> R<Value> {
    Ok(Value::Int(0))
}
fn empty_array(_vm: &mut Vm, _a: &mut Args) -> R<Value> {
    Ok(Value::Array(PhpArray::new()))
}

fn realpath(vm: &mut Vm, a: &mut Args) -> R<Value> {
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
    if flags != 15 && flags.count_ones() == 1 {
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

fn fnmatch_fn(_vm: &mut Vm, a: &mut Args) -> R<Value> {
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
    if out.is_empty() && flags & 16 != 0 {
        out.push(pat.to_vec());
    }
    Ok(Value::Array(PhpArray::from_values(
        out.into_iter().map(|s| Value::Str(PhpStr::from(s))),
    )))
}

fn list_dir(vm: &Vm, d: &[u8]) -> std::io::Result<Vec<Vec<u8>>> {
    let mut names: Vec<Vec<u8>> = vec![b".".to_vec(), b"..".to_vec()];
    for e in std::fs::read_dir(vm.fs_path(d))? {
        names.push(e?.file_name().into_vec());
    }
    Ok(names)
}

fn scandir(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let d = a.str(0);
    if d.is_empty() {
        return vm.fthrow(a, "ValueError", "Argument #1 ($directory) cannot be empty");
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
            match order {
                0 => names.sort(),
                1 => {
                    names.sort();
                    names.reverse();
                }
                _ => {}
            }
            Ok(Value::Array(PhpArray::from_values(
                names.into_iter().map(|s| Value::Str(PhpStr::from(s))),
            )))
        }
        Err(e) => {
            vm.fwarn(
                a,
                &format!(
                    "{}: Failed to open directory: {}",
                    d.to_string_lossy(),
                    io_err_msg(&e)
                ),
            )?;
            vm.fwarn(a, "(errno 2): No such file or directory")?;
            Ok(Value::Bool(false))
        }
    }
}

fn opendir(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let d = a.str(0);
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
            vm.fwarn(
                a,
                &format!(
                    "{}: Failed to open directory: {}",
                    d.to_string_lossy(),
                    io_err_msg(&e)
                ),
            )?;
            Ok(Value::Bool(false))
        }
    }
}

fn dir_arg(vm: &mut Vm, a: &Args) -> Option<ResRef> {
    match a.val(0) {
        Value::Resource(r) => Some(r),
        _ => vm.last_dir.clone(),
    }
}

fn readdir(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let Some(r) = dir_arg(vm, a) else {
        return Ok(Value::Bool(false));
    };
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
            return Ok(match super::userstream::call(vm, &o, b"dir_readdir", Vec::new())? {
                Some(Value::Str(n)) => Value::Str(n),
                Some(Value::Bool(false)) | None => Value::Bool(false),
                Some(v) => Value::Str(vm.to_php_str(&v)?),
            });
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

fn rewinddir(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let Some(r) = dir_arg(vm, a) else {
        return Ok(Value::Null);
    };
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

fn closedir(vm: &mut Vm, a: &mut Args) -> R<Value> {
    if let Some(r) = dir_arg(vm, a) {
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
    let d = a.str(0);
    let prefix = a.str(1);
    let prefix: Vec<u8> = prefix.iter().take(63).copied().collect();
    let mut dir = if d.is_empty() {
        PathBuf::from("/tmp")
    } else {
        vm.fs_path(&d)
    };
    if !dir.is_dir() {
        vm.notice_msg("tempnam(): file created in the system's temporary directory")?;
        dir = PathBuf::from("/tmp");
    }
    for n in 0..1000u32 {
        let name = {
            let mut v = prefix.clone();
            let t = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default();
            v.extend_from_slice(
                format!("{:x}{:x}{}", std::process::id(), t.subsec_nanos(), n).as_bytes(),
            );
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
    let f = a.str(0);
    match std::fs::read_link(vm.fs_path(&f)) {
        Ok(p) => Ok(Value::Str(PhpStr::new(p.as_os_str().as_bytes()))),
        Err(e) => {
            vm.fwarn(a, io_err_msg(&e))?;
            Ok(Value::Bool(false))
        }
    }
}

fn symlink(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let t = a.str(0);
    let l = a.str(1);
    match std::os::unix::fs::symlink(os(&t), vm.fs_path(&l)) {
        Ok(()) => Ok(Value::Bool(true)),
        Err(e) => {
            vm.fwarn(a, io_err_msg(&e))?;
            Ok(Value::Bool(false))
        }
    }
}

fn link(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let t = a.str(0);
    let l = a.str(1);
    match std::fs::hard_link(vm.fs_path(&t), vm.fs_path(&l)) {
        Ok(()) => Ok(Value::Bool(true)),
        Err(e) => {
            vm.fwarn(a, io_err_msg(&e))?;
            Ok(Value::Bool(false))
        }
    }
}

fn linkinfo(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let f = a.str(0);
    match std::fs::symlink_metadata(vm.fs_path(&f)) {
        Ok(m) => Ok(Value::Int(m.dev() as i64)),
        Err(e) => {
            vm.fwarn(a, io_err_msg(&e))?;
            Ok(Value::Int(-1))
        }
    }
}

fn disk_free_space(_vm: &mut Vm, _a: &mut Args) -> R<Value> {
    Ok(Value::Float(1e12))
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
        if enc.is_some() && j < n && Some(data[j]) == enc {
            let e = enc.unwrap();
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
    let (sep, enc, esc) = csv_opts(vm, a, 2)?;
    with_stream(vm, a, 0, |vm, s| {
        // Read a logical line (enclosures may span lines).
        let mut buf = Vec::new();
        let mut in_enc = false;
        loop {
            let Some(line) = read_line(vm, s, None) else {
                break;
            };
            for &c in &line {
                if Some(c) == enc {
                    in_enc = !in_enc;
                }
            }
            buf.extend_from_slice(&line);
            if !in_enc {
                break;
            }
        }
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
    let line = with_stream(vm, a, 0, |vm, s| {
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
