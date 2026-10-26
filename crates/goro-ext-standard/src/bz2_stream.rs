//! compress.bzip2:// streams and bzopen() & co.: ext/bz2/bz2.c on top of a
//! port of libbz2's BZFILE layer (BZ2_bzRead / BZ2_bzWrite, bzlib.c).
//!
//! The streams are served by the generic stream layer of the user-space
//! wrappers (the parent module) through its `NativeStream` ops.

use super::*;
use crate::bz2codec;
use std::io::{Read, Write};

const BZ_OK: i32 = 0;
const BZ_STREAM_END: i32 = 4;
const BZ_SEQUENCE_ERROR: i32 = -1;
const BZ_PARAM_ERROR: i32 = -2;
const BZ_DATA_ERROR: i32 = -4;
const BZ_DATA_ERROR_MAGIC: i32 = -5;
const BZ_IO_ERROR: i32 = -6;
const BZ_UNEXPECTED_EOF: i32 = -7;
/// BZ_MAX_UNUSED
const BUF_SIZE: usize = 5000;

/// bzlib.c's bzFile.
struct BzFile {
    file: std::fs::File,
    dec: Option<bz2codec::Decompress>,
    enc: Option<bz2codec::Compress>,
    buf: Vec<u8>,
    pos: usize,
    at_eof: bool,
    last_err: i32,
}

fn map_err(e: bz2codec::Error) -> i32 {
    match e {
        bz2codec::Error::Sequence => BZ_SEQUENCE_ERROR,
        bz2codec::Error::Data => BZ_DATA_ERROR,
        bz2codec::Error::DataMagic => BZ_DATA_ERROR_MAGIC,
        bz2codec::Error::Param => BZ_PARAM_ERROR,
    }
}

impl BzFile {
    /// BZ2_bzdopen() / BZ2_bzopen(): `mode` holds 'r' or 'w' and an optional
    /// block size digit (default 9).
    fn open(file: std::fs::File, mode: &str) -> Self {
        let writing = mode.contains('w');
        let block = mode
            .bytes()
            .find(|c| (b'1'..=b'9').contains(c))
            .map(|c| (c - b'0') as u32)
            .unwrap_or(9);
        BzFile {
            file,
            dec: (!writing).then(|| bz2codec::Decompress::new(false)),
            enc: writing.then(|| bz2codec::Compress::new(block, 0)),
            buf: Vec::new(),
            pos: 0,
            at_eof: false,
            last_err: BZ_OK,
        }
    }

    /// BZ2_bzread(): bytes read, 0 at the end of the stream, -1 on error.
    fn read(&mut self, out: &mut [u8]) -> i64 {
        if self.last_err == BZ_STREAM_END {
            return 0;
        }
        let Some(dec) = self.dec.as_mut() else {
            self.last_err = BZ_SEQUENCE_ERROR;
            return -1;
        };
        if out.is_empty() {
            self.last_err = BZ_OK;
            return 0;
        }
        let mut produced = 0usize;
        loop {
            if self.pos >= self.buf.len() && !self.at_eof {
                self.buf.resize(BUF_SIZE, 0);
                match self.file.read(&mut self.buf) {
                    Ok(n) => {
                        self.buf.truncate(n);
                        if n == 0 {
                            self.at_eof = true;
                        }
                    }
                    Err(_) => {
                        self.buf.clear();
                        self.last_err = BZ_IO_ERROR;
                        return -1;
                    }
                }
                self.pos = 0;
            }
            let (in0, out0) = (dec.total_in(), dec.total_out());
            let r = dec.decompress(&self.buf[self.pos..], &mut out[produced..]);
            self.pos += (dec.total_in() - in0) as usize;
            produced += (dec.total_out() - out0) as usize;
            match r {
                Err(e) => {
                    self.last_err = map_err(e);
                    return -1;
                }
                Ok(bz2codec::Status::StreamEnd) => {
                    self.last_err = BZ_STREAM_END;
                    return produced as i64;
                }
                Ok(_) => {}
            }
            if self.at_eof && self.pos >= self.buf.len() && produced < out.len() {
                self.last_err = BZ_UNEXPECTED_EOF;
                return -1;
            }
            if produced == out.len() {
                self.last_err = BZ_OK;
                return produced as i64;
            }
        }
    }

    /// Run the compressor over `input` with `action`, writing its output.
    fn compress(&mut self, input: &[u8], action: bz2codec::Action) -> bool {
        let Some(enc) = self.enc.as_mut() else {
            self.last_err = BZ_SEQUENCE_ERROR;
            return false;
        };
        let mut done = 0usize;
        let mut out = vec![0u8; BUF_SIZE];
        loop {
            let (in0, out0) = (enc.total_in(), enc.total_out());
            let r = enc.compress(&input[done..], &mut out, action);
            done += (enc.total_in() - in0) as usize;
            let n = (enc.total_out() - out0) as usize;
            if n > 0 && self.file.write_all(&out[..n]).is_err() {
                self.last_err = BZ_IO_ERROR;
                return false;
            }
            match r {
                Err(e) => {
                    self.last_err = map_err(e);
                    return false;
                }
                Ok(bz2codec::Status::StreamEnd) => return true,
                Ok(_) => {}
            }
            // BZ2_bzWrite() returns once all the input is taken (output
            // still pending comes with the next write or the close)
            if matches!(action, bz2codec::Action::Run) && done >= input.len() {
                return true;
            }
        }
    }

    /// BZ2_bzwrite()
    fn write(&mut self, data: &[u8]) -> i64 {
        if data.is_empty() {
            self.last_err = BZ_OK;
            return 0;
        }
        if self.compress(data, bz2codec::Action::Run) {
            self.last_err = BZ_OK;
            data.len() as i64
        } else {
            -1
        }
    }

    /// BZ2_bzclose()
    fn close(&mut self) {
        if self.enc.is_some() && self.last_err == BZ_OK {
            self.compress(&[], bz2codec::Action::Finish);
            let _ = self.file.flush();
        }
        self.enc = None;
        self.dec = None;
    }

    /// BZ2_bzerror()
    fn error(&self) -> (i32, &'static str) {
        const STRINGS: [&str; 10] = [
            "OK",
            "SEQUENCE_ERROR",
            "PARAM_ERROR",
            "MEM_ERROR",
            "DATA_ERROR",
            "DATA_ERROR_MAGIC",
            "IO_ERROR",
            "UNEXPECTED_EOF",
            "OUTBUFF_FULL",
            "CONFIG_ERROR",
        ];
        let err = self.last_err.min(0);
        (err, STRINGS.get((-err) as usize).copied().unwrap_or("???"))
    }
}

type Bz = Rc<RefCell<BzFile>>;

thread_local! {
    /// The BZFILE of each open bz2 stream (php_stream_is(PHP_STREAM_IS_BZIP2)).
    static BZ_STREAMS: RefCell<HashMap<i64, Bz>> = RefCell::new(HashMap::new());
}

/// php_bz2_stream_data_t
struct Bz2Stream {
    bz: Bz,
    fid: Rc<std::cell::Cell<i64>>,
    /// The inner stream (bzopen() of a stream resource), closed with us.
    inner: i64,
}

impl NativeStream for Bz2Stream {
    /// php_bz2iop_read()
    fn read(&mut self, _vm: &mut Vm, count: usize, eof: &mut bool) -> Option<Vec<u8>> {
        let mut out = vec![0u8; count];
        let mut ret = 0usize;
        loop {
            let to_read = (count - ret).min(i32::MAX as usize);
            let just_read = self.bz.borrow_mut().read(&mut out[ret..ret + to_read]);
            if just_read < 1 {
                // it is not safe to keep reading after an error, see #72613
                *eof = true;
                if just_read < 0 {
                    if ret > 0 {
                        break;
                    }
                    return None;
                }
                break;
            }
            ret += just_read as usize;
            if ret >= count {
                break;
            }
        }
        out.truncate(ret);
        Some(out)
    }

    /// php_bz2iop_write()
    fn write(&mut self, _vm: &mut Vm, buf: &[u8]) -> i64 {
        let mut wrote = 0i64;
        for chunk in buf.chunks(i32::MAX as usize) {
            let n = self.bz.borrow_mut().write(chunk);
            if n < 0 {
                if wrote == 0 {
                    return n;
                }
                break;
            }
            wrote += n;
        }
        wrote
    }

    /// php_bz2iop_close()
    fn close(&mut self, vm: &mut Vm) {
        self.bz.borrow_mut().close();
        let fid = self.fid.get();
        BZ_STREAMS.with(|m| m.borrow_mut().remove(&fid));
        if self.inner != 0 {
            let inner = std::mem::take(&mut self.inner);
            let _ = any_close(vm, inner);
        }
    }

    /// php_bz2iop_flush(): BZ2_bzflush() does nothing.
    fn flush(&mut self, _vm: &mut Vm) -> i32 {
        0
    }

    fn seek(&mut self, _vm: &mut Vm, _offset: i64, _whence: i64) -> Option<i64> {
        None
    }

    fn lock(&mut self, _vm: &mut Vm, _value: i64) -> i32 {
        -1
    }

    fn label(&self) -> &'static str {
        "BZip2"
    }
}

/// php_stream_bz2open_from_BZFILE()
fn from_bzfile(bz: BzFile, mode: &str, inner: i64, uri: Option<&[u8]>) -> (i64, Us) {
    let bz = Rc::new(RefCell::new(bz));
    let fid_cell = Rc::new(std::cell::Cell::new(0));
    let native: Rc<RefCell<dyn NativeStream>> = Rc::new(RefCell::new(Bz2Stream {
        bz: bz.clone(),
        fid: fid_cell.clone(),
        inner,
    }));
    let (fid, us) = match uri {
        Some(u) => alloc_native_stream(native, mode, Some(u), Some("BZip2"), false),
        None => alloc_native_stream(native, mode, None, None, false),
    };
    // ops->seek is NULL
    us.borrow_mut().flags |= FLAG_NO_SEEK;
    fid_cell.set(fid);
    BZ_STREAMS.with(|m| m.borrow_mut().insert(fid, bz));
    (fid, us)
}

/// php_stream_cast(PHP_STREAM_AS_FD): a duplicate of the stream's file.
fn stream_file(vm: &mut Vm, fid: i64) -> Option<std::fs::File> {
    let cast = if get_us(fid).is_some() {
        Err("user-space")
    } else {
        FILE_HANDLES.with(|h| match h.borrow_mut().get_mut(&fid) {
            None => Err("unknown"),
            Some(fh) => match fh.special {
                SpecialStream::Memory => Err("MEMORY"),
                // (php_stream_temp_cast(): the data moves to a temporary file)
                SpecialStream::Temp => {
                    crate::misc::temp_stream_to_file(&mut fh.temp_max);
                    fh.file.try_clone().map_err(|_| "TEMP")
                }
                SpecialStream::Output => Err("Output"),
                _ => fh.file.try_clone().map_err(|_| "STDIO"),
            },
        })
    };
    match cast {
        Ok(f) => Some(f),
        Err(label) => {
            warn(
                vm,
                &format!(
                    "Cannot represent a stream of type {} as a File Descriptor",
                    label
                ),
            );
            None
        }
    }
}

/// _php_stream_bz2open(): `via_wrapper` when opened through the
/// compress.bzip2:// wrapper.
pub(crate) fn bz2open(
    vm: &mut Vm,
    path: &[u8],
    mode: &str,
    options: i64,
    context: Option<i64>,
    via_wrapper: bool,
) -> Result<Option<(i64, Us)>, VmError> {
    let full = path;
    let mut path = path;
    if path.len() >= 17 && path[..17].eq_ignore_ascii_case(b"compress.bzip2://") {
        path = &path[17..];
    }
    let m = mode.as_bytes();
    if m.is_empty() || (m[0] != b'w' && m[0] != b'r' && m.len() > 1) {
        return Ok(None);
    }
    let uri = via_wrapper.then_some(full);
    if !goro_core::basedir::check_open_basedir_ex(vm, cur_fn(), &lossy(path), true) {
        return Ok(None);
    }
    // try and open it directly first (BZ2_bzopen(): fopen())
    {
        use std::os::unix::ffi::OsStrExt;
        let p = std::path::Path::new(std::ffi::OsStr::from_bytes(path));
        let direct = if m[0] == b'w' {
            std::fs::File::create(p)
        } else {
            std::fs::File::open(p)
        };
        if let Ok(f) = direct {
            return Ok(Some(from_bzfile(BzFile::open(f, mode), mode, 0, uri)));
        }
    }
    // that didn't work, so try and get something from the network/wrapper
    let Some(inner) = any_open(vm, path, mode, options, context)? else {
        return Ok(None);
    };
    let Some(file) = stream_file(vm, inner) else {
        any_close(vm, inner)?;
        return Ok(None);
    };
    Ok(Some(from_bzfile(
        BzFile::open(file, mode),
        mode,
        inner,
        uri,
    )))
}

fn type_error(vm: &mut Vm, msg: String) -> VmError {
    throw(vm, b"TypeError", &msg)
}

/// PHP_Z_PARAM_STREAM for `fname`'s $bz.
fn stream_param(vm: &mut Vm, fname: &str, v: Option<&Value>) -> Result<i64, VmError> {
    let v = v.map(|v| v.deref()).unwrap_or(Value::Null);
    match v {
        Value::Resource(id) if goro_core::value::is_open_resource(id.id()) => Ok(id.id()),
        Value::Resource(_) => Err(type_error(
            vm,
            format!(
                "{}(): Argument #1 ($bz) must be an open stream resource",
                fname
            ),
        )),
        other => Err(type_error(
            vm,
            format!(
                "{}(): Argument #1 ($bz) must be of type resource, {} given",
                fname,
                Vm::value_type_name(&other)
            ),
        )),
    }
}

/// PHP_FUNCTION(bzopen)
pub(crate) fn h_bzopen(vm: &mut Vm, args: &[Value]) -> Result<Option<Value>, VmError> {
    let file = args.first().map(|v| v.deref()).unwrap_or(Value::Null);
    let mode = args
        .get(1)
        .map(|v| v.deref().to_php_string().to_string_lossy())
        .unwrap_or_default();
    if mode != "r" && mode != "w" {
        return Err(throw(
            vm,
            b"ValueError",
            "bzopen(): Argument #2 ($mode) must be either \"r\" or \"w\"",
        ));
    }
    match file {
        Value::String(s) => {
            if s.is_empty() {
                return Err(throw(
                    vm,
                    b"ValueError",
                    "bzopen(): Argument #1 ($file) must not be empty",
                ));
            }
            if s.as_bytes().contains(&0) {
                return Err(type_error(
                    vm,
                    "bzopen(): Argument #1 ($file) must not contain null bytes".to_string(),
                ));
            }
            let ctx = default_context();
            Ok(Some(
                match bz2open(vm, s.as_bytes(), &mode, REPORT_ERRORS, Some(ctx), false)? {
                    Some((fid, _)) => Value::resource(fid),
                    None => Value::False,
                },
            ))
        }
        Value::Resource(id) => {
            let fid = stream_param(vm, "bzopen", Some(&Value::resource(&id)))?;
            let smode = stream_mode_of(fid);
            let sm = smode.as_bytes();
            let primary = match sm.len() {
                1 => Some(sm[0]),
                2 => {
                    let (p, sec) = if sm[0] != b'b' {
                        (sm[0], sm[1])
                    } else {
                        (sm[1], sm[0])
                    };
                    (sec == b'b').then_some(p)
                }
                _ => None,
            };
            let Some(primary) = primary else {
                warn(vm, &format!("Cannot use stream opened in mode '{}'", smode));
                return Ok(Some(Value::False));
            };
            if mode == "r" && primary != b'r' {
                warn(vm, "Cannot read from a stream opened in write only mode");
                return Ok(Some(Value::False));
            }
            if mode == "w" && !matches!(primary, b'w' | b'a' | b'x') {
                warn(vm, "cannot write to a stream opened in read only mode");
                return Ok(Some(Value::False));
            }
            let Some(f) = stream_file(vm, fid) else {
                return Ok(Some(Value::False));
            };
            // The stream keeps its own handle: closing ours leaves it open.
            let (nfid, _) = from_bzfile(BzFile::open(f, &mode), &mode, 0, None);
            Ok(Some(Value::resource(nfid)))
        }
        other => Err(type_error(
            vm,
            format!(
                "bzopen(): Argument #1 ($file) must be of type string or file-resource, {} given",
                Vm::value_type_name(&other)
            ),
        )),
    }
}

/// PHP_FUNCTION(bzread)
pub(crate) fn h_bzread(vm: &mut Vm, args: &[Value]) -> Result<Option<Value>, VmError> {
    let fid = stream_param(vm, "bzread", args.first())?;
    let len = int_arg(args, 1).unwrap_or(1024);
    if len < 0 {
        return Err(throw(
            vm,
            b"ValueError",
            "bzread(): Argument #2 ($length) must be greater than or equal to 0",
        ));
    }
    if len == 0 {
        return Ok(Some(s(b"")));
    }
    match get_us(fid) {
        // php_stream_read_to_str()
        Some(us) => Ok(Some(match stream_read(vm, fid, &us, len as usize)? {
            Some(d) => s(&d),
            None => Value::False,
        })),
        None => Ok(Some(call_orig(
            vm,
            "fread",
            &[Value::resource(fid), Value::Long(len)],
        )?)),
    }
}

/// php_bz2_error(): errno (0), errstr (1) or both (2).
fn bz_error(vm: &mut Vm, fname: &str, args: &[Value], opt: u8) -> Result<Option<Value>, VmError> {
    let fid = stream_param(vm, fname, args.first())?;
    let Some(bz) = BZ_STREAMS.with(|m| m.borrow().get(&fid).cloned()) else {
        return Err(type_error(
            vm,
            format!("{}(): Argument #1 ($bz) must be a bz2 stream", fname),
        ));
    };
    let (errnum, errstr) = bz.borrow().error();
    Ok(Some(match opt {
        0 => Value::Long(errnum as i64),
        1 => s(errstr.as_bytes()),
        _ => {
            let mut a = new_arr();
            a.set(
                goro_core::array::ArrayKey::String(PhpString::from_bytes(b"errno")),
                Value::Long(errnum as i64),
            );
            a.set(
                goro_core::array::ArrayKey::String(PhpString::from_bytes(b"errstr")),
                s(errstr.as_bytes()),
            );
            arr_val(a)
        }
    }))
}

pub(crate) fn h_bzerrno(vm: &mut Vm, args: &[Value]) -> Result<Option<Value>, VmError> {
    bz_error(vm, "bzerrno", args, 0)
}

pub(crate) fn h_bzerrstr(vm: &mut Vm, args: &[Value]) -> Result<Option<Value>, VmError> {
    bz_error(vm, "bzerrstr", args, 1)
}

pub(crate) fn h_bzerror(vm: &mut Vm, args: &[Value]) -> Result<Option<Value>, VmError> {
    bz_error(vm, "bzerror", args, 2)
}
