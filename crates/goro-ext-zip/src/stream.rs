//! zip_stream.c: the streams of ZipArchive::getStream*() and the zip://
//! wrapper.

use std::cell::RefCell;
use std::rc::Rc;

use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};
use goro_ext_standard::ext_io::{self, NativeOpened, NativeStream, NativeWrapperOps, StatBuf};

use crate::zipint::*;

/// php_zip_stream_data_t
struct ZipStream {
    /// the archive opened by the wrapper (closed with the stream)
    za: Option<Archive>,
    zf: Option<ZipFile>,
    cursor: usize,
    /// php_stream_zipio_seek_ops (zip_file_is_seekable())
    seekable: bool,
    /// the entry of a zip:// path (php_zip_ops_stat() uses orig_path)
    entry: Option<Vec<u8>>,
}

impl NativeStream for ZipStream {
    /// php_zip_ops_read()
    fn read(&mut self, vm: &mut Vm, count: usize, eof: &mut bool) -> Option<Vec<u8>> {
        let zf = self.zf.as_mut()?;
        match zf.read(count) {
            Ok(b) => {
                if b.is_empty() || b.len() < count {
                    *eof = true;
                } else {
                    self.cursor += b.len();
                }
                Some(b)
            }
            Err(e) => {
                *eof = true;
                let f = vm.current_builtin_name().unwrap_or_default();
                let line = vm.current_line;
                vm.emit_warning_at(
                    &format!("{}(): Zip stream error: {}", f, e.strerror()),
                    line,
                );
                None
            }
        }
    }

    fn write(&mut self, _vm: &mut Vm, buf: &[u8]) -> i64 {
        buf.len() as i64
    }

    fn close(&mut self, _vm: &mut Vm) {
        self.zf = None;
        if let Some(mut za) = self.za.take() {
            let _ = za.close(None);
        }
    }

    fn flush(&mut self, _vm: &mut Vm) -> i32 {
        0
    }

    /// php_zip_ops_seek()
    fn seek(&mut self, _vm: &mut Vm, offset: i64, whence: i64) -> Option<i64> {
        if !self.seekable {
            return None;
        }
        let zf = self.zf.as_mut()?;
        if zf.seek(offset, whence) {
            Some(zf.tell())
        } else {
            None
        }
    }

    fn lock(&mut self, _vm: &mut Vm, _value: i64) -> i32 {
        -1
    }

    fn label(&self) -> &'static str {
        "zip"
    }

    fn seekable(&self) -> bool {
        self.seekable
    }

    /// php_zip_ops_stat()
    fn stat(&mut self, _vm: &mut Vm) -> Option<StatBuf> {
        let name = self.entry.clone()?;
        let za = self.za.as_mut()?;
        let idx = za.name_locate(&name, FL_NOCASE);
        if idx < 0 {
            return None;
        }
        let sb = za.stat_index(idx as u64, 0)?;
        let mut st = StatBuf::default();
        if name.last() != Some(&b'/') {
            st.size = sb.size as i64;
            st.mode = 0o100000;
        } else {
            st.size = 0;
            st.mode = 0o040000;
        }
        st.mtime = sb.mtime;
        st.atime = sb.mtime;
        st.ctime = sb.mtime;
        st.nlink = 1;
        st.rdev = -1;
        st.blksize = -1;
        st.blocks = -1;
        st.ino = sb.index as i64;
        Some(st)
    }
}

/// php_stream_zip_open(): the resource id of a stream on an opened entry.
pub fn alloc(zf: ZipFile, za: Option<Archive>, name: &[u8]) -> i64 {
    let seekable = zf.seekable;
    let s = Rc::new(RefCell::new(ZipStream {
        za,
        zf: Some(zf),
        cursor: 0,
        seekable,
        entry: None,
    }));
    ext_io::ext_alloc_stream(s, "rb", Some(c_str(name)))
}

/// php_stream_zip_opener()
fn opener(
    vm: &mut Vm,
    path: &[u8],
    mode: &str,
    context: Option<i64>,
) -> Result<Option<NativeOpened>, VmError> {
    let Some(frag_pos) = path.iter().position(|&c| c == b'#') else {
        return Ok(None);
    };
    let fragment_len = path.len() - frag_pos;
    let mut p = path;
    if p.len() >= 6 && p[..6].eq_ignore_ascii_case(b"zip://") {
        p = &p[6..];
    }
    if fragment_len < 1 {
        return Ok(None);
    }
    if p.len() >= 4096 || !mode.starts_with('r') {
        return Ok(None);
    }
    let file_dirname = &p[..p.len() - fragment_len];
    let fragment = &path[frag_pos + 1..];
    let f = vm.current_builtin_name().unwrap_or_default();
    if crate::basedir_refused(vm, &f, file_dirname) {
        return Ok(None);
    }
    let dn = String::from_utf8_lossy(file_dirname).into_owned();
    let Ok(mut za) = Archive::open(&dn, ZIP_CREATE) else {
        return Ok(None);
    };
    if let Some(pw) = ext_io::context_option(context, b"zip", b"password") {
        match pw.deref() {
            Value::String(s) => za.set_default_password(Some(s.as_bytes())),
            _ => {
                let line = vm.current_line;
                vm.emit_warning_at(&format!("{}(): Can't set zip password", f), line);
            }
        }
    }
    match za.fopen(fragment, 0) {
        Some(zf) => {
            let seekable = zf.seekable;
            Ok(Some(NativeOpened {
                stream: Rc::new(RefCell::new(ZipStream {
                    za: Some(za),
                    zf: Some(zf),
                    cursor: 0,
                    seekable,
                    entry: Some(fragment.to_vec()),
                })),
                opened_path: None,
            }))
        }
        None => {
            let _ = za.close(None);
            Ok(None)
        }
    }
}

/// php_stream_zip_wrapper: only wops->stream_opener is set
struct ZipWrapper;

fn warn(vm: &mut Vm, msg: &str) {
    let f = vm.current_builtin_name().unwrap_or_default();
    let line = vm.current_line;
    vm.emit_warning_at(&format!("{}(): {}", f, msg), line);
}

impl NativeWrapperOps for ZipWrapper {
    fn label(&self) -> &'static str {
        "zip wrapper"
    }

    fn open(
        &self,
        vm: &mut Vm,
        path: &[u8],
        mode: &str,
        _options: i64,
        context: Option<i64>,
    ) -> Result<Option<NativeOpened>, VmError> {
        opener(vm, path, mode, context)
    }

    fn opendir(
        &self,
        _vm: &mut Vm,
        _path: &[u8],
        _options: i64,
        _context: Option<i64>,
    ) -> Result<Option<Rc<RefCell<dyn NativeStream>>>, VmError> {
        Ok(None)
    }

    fn url_stat(&self, _vm: &mut Vm, _url: &[u8], _flags: i64) -> Result<Option<StatBuf>, VmError> {
        Ok(None)
    }

    fn unlink(
        &self,
        vm: &mut Vm,
        _url: &[u8],
        _options: i64,
        _context: Option<i64>,
    ) -> Result<bool, VmError> {
        warn(vm, "zip wrapper does not allow unlinking");
        Ok(false)
    }

    fn rename(
        &self,
        vm: &mut Vm,
        _from: &[u8],
        _to: &[u8],
        _options: i64,
        _context: Option<i64>,
    ) -> Result<bool, VmError> {
        warn(vm, "zip wrapper wrapper does not support renaming");
        Ok(false)
    }

    fn mkdir(
        &self,
        _vm: &mut Vm,
        _url: &[u8],
        _mode: i64,
        _options: i64,
        _context: Option<i64>,
    ) -> Result<bool, VmError> {
        Ok(false)
    }

    fn rmdir(
        &self,
        _vm: &mut Vm,
        _url: &[u8],
        _options: i64,
        _context: Option<i64>,
    ) -> Result<bool, VmError> {
        Ok(false)
    }
}

pub fn register(_vm: &mut Vm) {
    ext_io::register_native_wrapper(b"zip", Rc::new(ZipWrapper));
}
