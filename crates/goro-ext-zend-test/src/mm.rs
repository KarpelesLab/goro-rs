//! zend_mm_custom_handlers.c: while zend_test.zend_mm_custom_handlers.enabled
//! is on, a heap whose custom handlers report every allocation, free and
//! zend_mm_gc() is installed (they print to the C stdout, unbuffered by the
//! output layer).

use std::io::Write;

use goro_core::value::{MmEvent, Value};
use goro_core::vm::Vm;

use std::cell::Cell;

pub(crate) const ENABLED: &str = "zend_test.zend_mm_custom_handlers.enabled";

/// Stand-ins for the heap pointers printed (there is a single heap).
const ORIGINAL_HEAP: usize = 0x7f00_0000_0040;
const OBSERVED_HEAP: usize = 0x7f00_0020_0040;

thread_local! {
    static INSTALLED: Cell<bool> = const { Cell::new(false) };
    /// A stand-in for the address of the next block.
    static NEXT_BLOCK: Cell<usize> = const { Cell::new(0x7f00_0020_1000) };
}

fn print(s: &str) {
    let mut out = std::io::stdout();
    let _ = out.write_all(s.as_bytes());
    let _ = out.flush();
}

pub(crate) fn register(vm: &mut Vm) {
    vm.ini_set_hooks
        .insert(ENABLED.as_bytes().to_vec(), on_update_enabled);
    vm.request_shutdown_hooks.push(|_| shutdown());
    // zend_test_mm_custom_handlers_rinit()
    if crate::ini::ini_bool(vm, ENABLED) {
        init();
    }
}

/// OnUpdateZendTestMMCustomHandlersEnabled
fn on_update_enabled(_vm: &mut Vm, _name: &[u8], value: &Value) -> bool {
    if crate::ini::parse_bool(value.to_php_string().as_bytes()) {
        init();
    } else {
        shutdown();
    }
    true
}

fn observe(ev: MmEvent) {
    let line = match ev {
        MmEvent::Alloc(size) => {
            let at = NEXT_BLOCK.with(|n| {
                let at = n.get();
                n.set(at + size.next_multiple_of(8).max(8));
                at
            });
            format!("Allocated {} bytes at {:#x}\n", size, at)
        }
        MmEvent::Free(_) => {
            let at = NEXT_BLOCK.with(|n| n.get());
            format!("Freed memory at {:#x}\n", at)
        }
        MmEvent::Gc(size) => format!("ZendMM GC freed {} bytes", size),
    };
    print(&line);
}

/// zend_test_mm_custom_handlers_init()
fn init() {
    if INSTALLED.with(|i| i.replace(true)) {
        return;
    }
    print("Prev handlers at (nil), (nil), (nil), (nil), (nil)\n");
    print(&format!(
        "Heap at {:#x} installed in ZendMM (orig at {:#x})\n",
        OBSERVED_HEAP, ORIGINAL_HEAP
    ));
    goro_core::value::set_mm_observer(Some(observe));
}

/// zend_test_mm_custom_handlers_shutdown()
fn shutdown() {
    if !INSTALLED.with(|i| i.replace(false)) {
        return;
    }
    goro_core::value::set_mm_observer(None);
    print(&format!(
        "Prev heap at {:#x} restored in ZendMM\n",
        ORIGINAL_HEAP
    ));
}
