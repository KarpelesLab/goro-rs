//! iterators.c: ZendTest\Iterators\TraversableTest, whose internal
//! iterator (get_iterator) yields 0 => 0 ... 3 => 3 and reports each call.

use goro_core::internal_iterator::ObjectIterator;
use goro_core::object::ClassEntry;
use goro_core::value::Value;
use goro_core::vm::{NativeClassHandler, Vm};

const CLASS: &[u8] = b"ZendTest\\Iterators\\TraversableTest";
const CLASS_LOWER: &[u8] = b"zendtest\\iterators\\traversabletest";

pub(crate) fn register(vm: &mut Vm) {
    let mut ce = goro_core::stubs::self_declared_class_entry(vm, CLASS)
        .unwrap_or_else(|| ClassEntry::new(CLASS.to_vec()));
    // (methods kept for Reflection; calls go to the native handler)
    ce.name = CLASS.to_vec();
    vm.classes.insert(CLASS_LOWER.to_vec(), ce);
    vm.native_class_handlers.insert(
        CLASS_LOWER.to_vec(),
        NativeClassHandler {
            has_method,
            call,
            prop_added: None,
            prop_write: None,
            compare_check: None,
            prop_read: None,
            param_names: Some(|_, _| Some(&[])),
        },
    );
}

fn has_method(_class: &[u8], method: &[u8], _is_static: bool) -> bool {
    method.eq_ignore_ascii_case(b"__construct") || method.eq_ignore_ascii_case(b"getIterator")
}

fn call(
    vm: &mut Vm,
    _class: &[u8],
    method: &[u8],
    _called: &[u8],
    this: Option<&Value>,
    args: &[Value],
) -> Value {
    let name = if method.eq_ignore_ascii_case(b"getIterator") {
        "getIterator"
    } else {
        "__construct"
    };
    if !args.is_empty() {
        // ZEND_PARSE_PARAMETERS_NONE()
        let msg = format!(
            "ZendTest\\Iterators\\TraversableTest::{}() expects exactly 0 arguments, {} given",
            name,
            args.len()
        );
        let exc = vm.create_exception(b"ArgumentCountError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Value::Null;
    }
    match (name, this) {
        ("getIterator", Some(obj)) => vm.create_internal_iterator(Box::new(TestIterator {
            _data: obj.deref(),
            current: 0,
        })),
        _ => Value::Null,
    }
}

struct TestIterator {
    /// intern.data: the object iterated over
    _data: Value,
    current: i64,
}

impl ObjectIterator for TestIterator {
    fn valid(&mut self, vm: &mut Vm) -> bool {
        vm.write_output(b"TraversableTest::valid\n");
        self.current < 4
    }

    fn current(&mut self, vm: &mut Vm) -> Value {
        vm.write_output(b"TraversableTest::current\n");
        Value::Long(self.current)
    }

    fn key(&mut self, vm: &mut Vm) -> Option<Value> {
        vm.write_output(b"TraversableTest::key\n");
        Some(Value::Long(self.current))
    }

    fn move_forward(&mut self, vm: &mut Vm) {
        vm.write_output(b"TraversableTest::next\n");
        self.current += 1;
    }

    fn rewind(&mut self, vm: &mut Vm) -> bool {
        vm.write_output(b"TraversableTest::rewind\n");
        self.current = 0;
        true
    }

    fn dtor(&mut self, vm: &mut Vm) {
        vm.write_output(b"TraversableTest::drop\n");
    }
}
