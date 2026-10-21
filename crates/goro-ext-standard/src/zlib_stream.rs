//! compress.zlib:// streams: ext/zlib/zlib_fopen_wrapper.c on top of the
//! gzio port (goro_zlib::gz), plus gzopen(), gzfile() and readgzfile().
//!
//! The streams are served by the generic stream layer of the user-space
//! wrappers (the parent module) through its `NativeStream` ops.

use super::*;
use goro_zlib::gz::GzFile;
use std::os::unix::io::AsRawFd;

/// php_gz_stream_data_t
struct GzStream {
    gz: Option<GzFile>,
    /// The inner (file) stream.
    inner: i64,
}

/// php_gziop_report_errors()
fn report_errors(vm: &mut Vm, gz: &GzFile, count: usize, verb: &str) {
    let (err, _) = gz.error();
    if err == goro_zlib::Z_ERRNO {
        let e = std::io::Error::last_os_error();
        let errno = e.raw_os_error().unwrap_or(0);
        let msg = e.to_string();
        let msg = match msg.find(" (os error") {
            Some(i) => msg[..i].to_string(),
            None => msg,
        };
        notice(
            vm,
            &format!(
                "{} of {} bytes failed with errno={} {}",
                verb, count, errno, msg
            ),
        );
    }
}

impl NativeStream for GzStream {
    fn read(&mut self, vm: &mut Vm, count: usize, eof: &mut bool) -> Option<Vec<u8>> {
        let gz = self.gz.as_mut()?;
        let chunk = count.min(i32::MAX as usize);
        let mut buf = vec![0u8; chunk];
        let read = gz.read(&mut buf);
        if gz.eof() {
            *eof = true;
        }
        if read < 0 {
            if let Some(gz) = self.gz.as_ref() {
                report_errors(vm, gz, chunk, "Read");
            }
            return None;
        }
        buf.truncate(read as usize);
        Some(buf)
    }

    fn write(&mut self, vm: &mut Vm, buf: &[u8]) -> i64 {
        let Some(gz) = self.gz.as_mut() else {
            return -1;
        };
        let mut total = 0i64;
        for chunk in buf.chunks(i32::MAX as usize) {
            let written = gz.write(chunk);
            if written < 0 {
                if let Some(gz) = self.gz.as_ref() {
                    report_errors(vm, gz, chunk.len(), "Write");
                }
                return written;
            }
            total += written;
        }
        total
    }

    fn close(&mut self, vm: &mut Vm) {
        if let Some(gz) = self.gz.take() {
            gz.close();
        }
        if self.inner != 0 {
            let inner = std::mem::take(&mut self.inner);
            let _ = any_close(vm, inner);
        }
    }

    fn flush(&mut self, _vm: &mut Vm) -> i32 {
        match self.gz.as_mut() {
            Some(gz) => gz.flush(goro_zlib::Z_SYNC_FLUSH),
            None => -1,
        }
    }

    fn seek(&mut self, vm: &mut Vm, offset: i64, whence: i64) -> Option<i64> {
        if whence == SEEK_END {
            warn(vm, "SEEK_END is not supported");
            return None;
        }
        let gz = self.gz.as_mut()?;
        let n = gz.seek(offset, whence as i32);
        if n < 0 { None } else { Some(n) }
    }

    fn lock(&mut self, _vm: &mut Vm, value: i64) -> i32 {
        // set_option(PHP_STREAM_OPTION_LOCKING) of the inner (plain) stream
        let fd = FILE_HANDLES.with(|h| h.borrow().get(&self.inner).map(|fh| fh.file.as_raw_fd()));
        let Some(fd) = fd else {
            return -1;
        };
        if value == 0 {
            return 0;
        }
        if unsafe { libc::flock(fd, value as libc::c_int) } == 0 {
            0
        } else {
            -1
        }
    }

    fn label(&self) -> &'static str {
        "ZLIB"
    }
}

/// zlib.level of a stream context.
fn context_level(context: Option<i64>) -> Option<i64> {
    let id = context?;
    STREAM_CONTEXTS.with(|c| {
        c.borrow()
            .get(&id)
            .and_then(|ctx| ctx.options.get(b"zlib".as_slice()))
            .and_then(|o| o.get(b"level".as_slice()))
            .map(|v| v.deref().to_long())
    })
}

/// php_stream_gzopen(): `via_wrapper` when opened through the
/// compress.zlib:// wrapper (fopen() & co.) rather than gzopen().
pub(crate) fn gzopen(
    vm: &mut Vm,
    path: &[u8],
    mode: &str,
    options: i64,
    context: Option<i64>,
    via_wrapper: bool,
) -> Result<Option<(i64, Us)>, VmError> {
    if mode.contains('+') {
        if options & REPORT_ERRORS != 0 {
            warn(
                vm,
                "Cannot open a zlib stream for reading and writing at the same time!",
            );
        }
        return Ok(None);
    }
    let full = path;
    let mut path = path;
    if path.len() >= 16 && path[..16].eq_ignore_ascii_case(b"compress.zlib://") {
        path = &path[16..];
    } else if path.len() >= 5 && path[..5].eq_ignore_ascii_case(b"zlib:") {
        path = &path[5..];
    }
    // STREAM_MUST_SEEK on the output-only php:// streams.
    for p in [
        &b"php://output"[..],
        b"php://stdout",
        b"php://stderr",
        b"php://input",
    ] {
        if path.len() >= p.len() && path[..p.len()].eq_ignore_ascii_case(p) {
            let f = cur_fn();
            vm.emit_warning(&format!(
                "{}({}): could not make seekable - {}",
                f,
                lossy(path),
                lossy(path)
            ));
            return Ok(None);
        }
    }
    let inner = if options & REPORT_ERRORS != 0 {
        any_open(vm, path, mode, options, context)?
    } else {
        // Without REPORT_ERRORS the plain wrapper does not complain.
        let saved = vm.warning_capture.replace(Vec::new());
        let r = any_open(vm, path, mode, options, context);
        vm.warning_capture = saved;
        r?
    };
    let Some(inner) = inner else {
        return Ok(None);
    };
    // php_stream_cast(innerstream, PHP_STREAM_AS_FD, ..., REPORT_ERRORS)
    let cast = if get_us(inner).is_some() {
        Err("user-space")
    } else {
        FILE_HANDLES.with(|h| match h.borrow().get(&inner) {
            None => Err("unknown"),
            Some(fh) => match fh.special {
                SpecialStream::Memory => Err("MEMORY"),
                SpecialStream::Temp => Err("TEMP"),
                SpecialStream::Output => Err("Output"),
                _ => fh.file.try_clone().map_err(|_| "STDIO"),
            },
        })
    };
    let file = match cast {
        Ok(f) => f,
        Err(label) => {
            warn(
                vm,
                &format!(
                    "Cannot represent a stream of type {} as a File Descriptor",
                    label
                ),
            );
            any_close(vm, inner)?;
            return Ok(None);
        }
    };
    let fd = file.as_raw_fd();
    let Some(mut gz) = GzFile::dopen(file, fd, mode.as_bytes()) else {
        if options & REPORT_ERRORS != 0 {
            warn(vm, "gzopen failed");
        }
        any_close(vm, inner)?;
        return Ok(None);
    };
    let ctx = context.or_else(|| Some(default_context()));
    if let Some(level) = context_level(ctx) {
        if gz.set_params(level as i32, goro_zlib::Z_DEFAULT_STRATEGY) != goro_zlib::Z_OK {
            vm.emit_warning("failed setting compression level");
        }
    }
    let native: Rc<RefCell<dyn NativeStream>> = Rc::new(RefCell::new(GzStream {
        gz: Some(gz),
        inner,
    }));
    let (fid, us) = if via_wrapper {
        alloc_native_stream(native, mode, Some(full), Some("ZLIB"), true)
    } else {
        alloc_native_stream(native, mode, None, None, true)
    };
    Ok(Some((fid, us)))
}

fn path_arg(vm: &mut Vm, args: &[Value], fname: &str) -> Result<Vec<u8>, VmError> {
    let v = args.first().map(|v| v.deref()).unwrap_or(Value::Null);
    // zpp "p": a string (objects with __toString, scalars), never null
    // bytes.
    let p = match &v {
        Value::Array(_) | Value::Resource(_) => {
            return Err(throw(
                vm,
                b"TypeError",
                &format!(
                    "{}(): Argument #1 ($filename) must be of type string, {} given",
                    fname,
                    Vm::value_type_name(&v)
                ),
            ));
        }
        Value::Null | Value::Undef => {
            vm.emit_deprecated(&format!(
                "{}(): Passing null to parameter #1 ($filename) of type string is deprecated",
                fname
            ));
            Vec::new()
        }
        _ => vm.value_to_string_checked(&v)?.as_bytes().to_vec(),
    };
    if p.contains(&0) {
        return Err(throw(
            vm,
            b"ValueError",
            &format!(
                "{}(): Argument #1 ($filename) must not contain any null bytes",
                fname
            ),
        ));
    }
    Ok(p)
}

fn use_path_arg(args: &[Value], i: usize) -> i64 {
    if args.get(i).map(|v| v.deref().is_truthy()).unwrap_or(false) {
        USE_PATH
    } else {
        0
    }
}

/// PHP_FUNCTION(gzopen)
pub(crate) fn h_gzopen(vm: &mut Vm, args: &[Value]) -> Result<Option<Value>, VmError> {
    let p = path_arg(vm, args, "gzopen")?;
    let mode = args
        .get(1)
        .map(|v| v.deref().to_php_string().to_string_lossy())
        .unwrap_or_default();
    let options = REPORT_ERRORS | use_path_arg(args, 2);
    let ctx = default_context();
    Ok(Some(
        match gzopen(vm, &p, &mode, options, Some(ctx), false)? {
            Some((fid, _)) => Value::Resource(fid),
            None => Value::False,
        },
    ))
}

/// PHP_FUNCTION(gzfile)
pub(crate) fn h_gzfile(vm: &mut Vm, args: &[Value]) -> Result<Option<Value>, VmError> {
    let p = path_arg(vm, args, "gzfile")?;
    let options = REPORT_ERRORS | use_path_arg(args, 1);
    let ctx = default_context();
    let Some((fid, us)) = gzopen(vm, &p, "rb", options, Some(ctx), false)? else {
        return Ok(Some(Value::False));
    };
    let mut out = new_arr();
    // php_stream_gets(stream, buf, sizeof(buf) - 1) with an 8K buffer
    while let Some(line) = stream_get_line(vm, fid, &us, Some(8191))? {
        // add_index_string(): the line as a C string
        let end = line.iter().position(|&c| c == 0).unwrap_or(line.len());
        out.push(s(&line[..end]));
    }
    stream_free(vm, fid, &us)?;
    Ok(Some(arr_val(out)))
}

/// PHP_FUNCTION(readgzfile)
pub(crate) fn h_readgzfile(vm: &mut Vm, args: &[Value]) -> Result<Option<Value>, VmError> {
    let p = path_arg(vm, args, "readgzfile")?;
    let options = REPORT_ERRORS | use_path_arg(args, 1);
    let ctx = default_context();
    let Some((fid, us)) = gzopen(vm, &p, "rb", options, Some(ctx), false)? else {
        return Ok(Some(Value::False));
    };
    let n = passthru(vm, fid, &us)?;
    stream_free(vm, fid, &us)?;
    Ok(Some(Value::Long(n)))
}
