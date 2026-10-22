//! Streams implemented by extensions (php_stream_alloc() with an
//! extension's own php_stream_ops), e.g. pdo_pgsql's large object streams.
//! The stream layer of the standard extension installs `ALLOC`.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::vm::Vm;

/// php_stream_ops of an extension stream
pub trait ExtStream {
    /// ops->read(): None == -1. Sets `*eof` like stream->eof.
    fn read(&mut self, vm: &mut Vm, count: usize, eof: &mut bool) -> Option<Vec<u8>>;
    /// ops->write(): bytes written, < 0 on error.
    fn write(&mut self, vm: &mut Vm, buf: &[u8]) -> i64;
    /// ops->close()
    fn close(&mut self, vm: &mut Vm);
    /// ops->flush(): 0 on success.
    fn flush(&mut self, _vm: &mut Vm) -> i32 {
        0
    }
    /// ops->seek(): the new offset, None on failure (or no seek op).
    fn seek(&mut self, _vm: &mut Vm, _offset: i64, _whence: i64) -> Option<i64> {
        None
    }
    /// ops->label
    fn label(&self) -> &'static str;
}

/// php_stream_alloc(ops, abstract, NULL, mode): the stream resource id.
pub type AllocFn = fn(&mut Vm, Rc<RefCell<dyn ExtStream>>, &str) -> i64;

thread_local! {
    pub static ALLOC: Cell<Option<AllocFn>> = const { Cell::new(None) };
}

/// Allocate a stream served by `s`; None without a stream layer.
pub fn alloc(vm: &mut Vm, s: Rc<RefCell<dyn ExtStream>>, mode: &str) -> Option<i64> {
    let f = ALLOC.with(|a| a.get())?;
    Some(f(vm, s, mode))
}
