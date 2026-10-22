//! The classes of ext/zend_test.
//!
//! They are declared by running prelude.php, then turned into internal
//! classes: no declaring file or lines (ReflectionClass::isInternal(),
//! "<internal:zend_test>"), methods carrying their stub arginfo
//! (`OpArray::internal_sig`), which also makes the VM run them like
//! internal functions (errors are reported at the caller's line, functions
//! they call appear as called from "[internal function]").

use std::rc::Rc;

use goro_core::object::ClassEntry;
use goro_core::opcode::OpArray;
use goro_core::stubs::StubClass;
use goro_core::vm::Vm;

const PRELUDE: &[u8] = include_bytes!("prelude.php");

pub(crate) fn register(vm: &mut Vm) {
    let Some((op_array, classes)) =
        goro_core::compiler::without_internal_attributes(compile_prelude)
    else {
        return;
    };
    let names: Vec<Vec<u8>> = classes
        .iter()
        .map(|c| c.name.to_ascii_lowercase())
        .collect();
    let saved_file = std::mem::take(&mut vm.current_file);
    let saved_line = vm.current_line;
    let user_consts = vm.user_constant_names.len();
    let _ = vm.run_compiled(&op_array, classes);
    // Its constants are the extension's.
    for (k, _) in vm.user_constant_names.split_off(user_consts) {
        vm.constant_files.remove(&k);
    }
    vm.current_file = saved_file;
    vm.current_line = saved_line;
    vm.current_exception = None;
    for lower in &names {
        make_internal(vm, lower);
    }
    // The declarations of builtins (attributes, doc comments).
    const DECLS: &[u8] = b"__zend_test_function_decls\\";
    let decls: Vec<Vec<u8>> = vm
        .user_functions
        .keys()
        .filter(|k| k.starts_with(DECLS))
        .cloned()
        .collect();
    for k in decls {
        if let Some(mut op) = vm.user_functions.remove(&k) {
            // Named like the function (not its declaration's namespace).
            let o = Rc::make_mut(&mut op);
            if o.name.len() >= DECLS.len() {
                o.name.drain(..DECLS.len());
            }
            vm.internal_function_decls
                .insert(k[DECLS.len()..].to_vec(), op);
        }
    }
    // _ZendTestClass objects have a method "test" provided by their
    // get_method / get_static_method handlers (a trampoline returning its
    // name).
    vm.native_class_handlers.insert(
        b"_zendtestclass".to_vec(),
        goro_core::vm::NativeClassHandler {
            has_method: test_class_has_method,
            call: test_class_call,
            prop_added: None,
            prop_write: None,
            compare_check: None,
            prop_read: None,
            param_names: Some(test_class_param_names),
        },
    );
    // zend_forbid_dynamic_call()
    for m in ["call", "callstatic"] {
        vm.forbid_dynamic_call_methods
            .push((b"zendtestforbiddynamiccall".to_vec(), m.as_bytes().to_vec()));
    }
    // _ZendTestClass is also registered as _ZendTestClassAlias (@alias).
    if let Some(ce) = vm.classes.get(b"_zendtestclass".as_slice()).cloned() {
        let alias = b"_zendtestclassalias".to_vec();
        vm.classes.insert(alias.clone(), ce);
        vm.class_aliases
            .insert(alias.clone(), b"_zendtestclass".to_vec());
        vm.class_aliases.insert(b"_zendtestclass".to_vec(), alias);
    }
}

/// zend_mark_internal_attribute() of the extension's attributes (known
/// to the compiler from module startup).
pub(crate) fn mark_internal_attributes() {
    use goro_core::compiler::{
        ATTRIBUTE_IS_REPEATABLE, ATTRIBUTE_TARGET_ALL, InternalAttribute,
        register_internal_attribute,
    };
    for (name, flags, validator) in [
        (
            "ZendTestAttribute",
            ATTRIBUTE_TARGET_ALL,
            Some(validate_zend_test_attribute as fn(u32) -> Option<String>),
        ),
        (
            "ZendTestRepeatableAttribute",
            ATTRIBUTE_TARGET_ALL | ATTRIBUTE_IS_REPEATABLE,
            None,
        ),
        ("ZendTestParameterAttribute", 32, None),
        ("ZendTestPropertyAttribute", 8, None),
        ("ZendTestAttributeWithArguments", ATTRIBUTE_TARGET_ALL, None),
    ] {
        register_internal_attribute(name.as_bytes(), InternalAttribute { flags, validator });
    }
}

/// zend_attribute_validate_zendtestattribute()
fn validate_zend_test_attribute(target: u32) -> Option<String> {
    (target != 1).then(|| "Only classes can be marked with #[ZendTestAttribute]".to_string())
}

fn compile_prelude() -> Option<(OpArray, Vec<ClassEntry>)> {
    let mut lexer = goro_parser::Lexer::new(PRELUDE);
    let tokens = lexer.tokenize();
    let _ = goro_parser::take_lexer_deprecations();
    let _ = goro_parser::take_lexer_warnings();
    let mut parser = goro_parser::Parser::new(tokens)
        .with_source(PRELUDE)
        .trusted();
    let program = parser.parse().ok()?;
    let compiler = goro_core::compiler::Compiler::new();
    let (mut op_array, mut classes, _) = compiler.compile(&program).ok()?;
    let _ = goro_core::compiler::take_pending_compile_warnings();
    clear_lines(&mut op_array);
    for c in &mut classes {
        for m in c.methods.values_mut() {
            clear_lines(Rc::make_mut(&mut m.op_array));
        }
    }
    Some((op_array, classes))
}

/// Internal code has no lines: the VM keeps reporting the caller's.
fn clear_lines(op: &mut OpArray) {
    for o in &mut op.ops {
        o.line = 0;
    }
    op.decl_line = 0;
    op.end_line = 0;
    op.filename.clear();
    for c in &mut op.child_functions {
        clear_lines(c);
    }
}

fn stub_class(lower: &[u8]) -> Option<&'static StubClass> {
    goro_core::stubs::find_self_declared_class(lower)
}

fn make_internal(vm: &mut Vm, lower: &[u8]) {
    let Some(ce) = vm.classes.get_mut(lower) else {
        return;
    };
    ce.filename = None;
    ce.start_line = 0;
    ce.end_line = 0;
    for m in ce.methods.values_mut() {
        let decl = m.declaring_class.clone();
        let Some(sc) = stub_class(&decl) else {
            continue;
        };
        let Some(f) = sc
            .methods
            .iter()
            .find(|f| f.name.as_bytes().eq_ignore_ascii_case(&m.name))
        else {
            continue;
        };
        let oa = Rc::make_mut(&mut m.op_array);
        oa.internal_sig = Some(f);
        oa.doc_comment = None;
        m.doc_comment = None;
    }
}

fn test_class_has_method(_class: &[u8], method: &[u8], _is_static: bool) -> bool {
    method.eq_ignore_ascii_case(b"test")
}

/// zend_test_func(): the trampoline returns the name it was called by.
fn test_class_call(
    _vm: &mut Vm,
    _class: &[u8],
    method: &[u8],
    _called: &[u8],
    _this: Option<&goro_core::value::Value>,
    _args: &[goro_core::value::Value],
) -> goro_core::value::Value {
    goro_core::value::Value::String(goro_core::string::PhpString::from_vec(method.to_vec()))
}

/// The trampoline declares no parameters: named arguments are unknown.
fn test_class_param_names(_class: &[u8], _method: &[u8]) -> Option<&'static [&'static str]> {
    Some(&[])
}
