//! The observer API test hooks of ext/zend_test (observer.c): with
//! `zend_test.observer.enabled`, every function's first call prints an
//! "init" line and observed calls print `<name>` ... `</name>` around their
//! execution; `zend_test.observer.execute_internal` reports the internal
//! functions zend_execute_internal runs.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

use goro_core::object::PhpObject;
use goro_core::opcode::OpArray;
use goro_core::value::Value;
use goro_core::vm::{FiberEvent, FiberSwitch, ObservedFrame, ObserverHooks, Vm};

use crate::ini::ini_bool;

#[derive(Default)]
struct Config {
    enabled: bool,
    show_output: bool,
    observe_all: bool,
    observe_includes: bool,
    observe_functions: bool,
    show_return_type: bool,
    show_return_value: bool,
    show_init_backtrace: bool,
    execute_internal: bool,
    fiber_init: bool,
    fiber_switch: bool,
    fiber_destroy: bool,
    function_names: HashSet<Vec<u8>>,
}

#[derive(Default)]
struct State {
    cfg: Config,
    /// ZT_G(observer_nesting_depth)
    depth: usize,
    /// Functions whose fcall_init ran: key -> whether they are observed.
    inited: HashMap<String, bool>,
    /// Per running call: whether its begin handler printed (end must too).
    stack: Vec<bool>,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

pub(crate) fn register(vm: &mut Vm) {
    STATE.with(|s| *s.borrow_mut() = State::default());
    vm.startup_hooks.push(startup);
}

/// zend_test_observer_init(): the INI settings are in place.
fn startup(vm: &mut Vm) {
    let cfg = Config {
        enabled: ini_bool(vm, "zend_test.observer.enabled"),
        show_output: ini_bool(vm, "zend_test.observer.show_output"),
        observe_all: ini_bool(vm, "zend_test.observer.observe_all"),
        observe_includes: ini_bool(vm, "zend_test.observer.observe_includes"),
        observe_functions: ini_bool(vm, "zend_test.observer.observe_functions"),
        show_return_type: ini_bool(vm, "zend_test.observer.show_return_type"),
        show_return_value: ini_bool(vm, "zend_test.observer.show_return_value"),
        show_init_backtrace: ini_bool(vm, "zend_test.observer.show_init_backtrace"),
        execute_internal: ini_bool(vm, "zend_test.observer.execute_internal"),
        fiber_init: ini_bool(vm, "zend_test.observer.fiber_init"),
        fiber_switch: ini_bool(vm, "zend_test.observer.fiber_switch"),
        fiber_destroy: ini_bool(vm, "zend_test.observer.fiber_destroy"),
        function_names: parse_names(&crate::ini::ini_str(
            vm,
            "zend_test.observer.observe_function_names",
        )),
    };
    let install = cfg.enabled || cfg.execute_internal;
    let fibers = cfg.enabled;
    STATE.with(|s| s.borrow_mut().cfg = cfg);
    if install {
        vm.observer = Some(ObserverHooks { begin, end });
    }
    if fibers {
        vm.fiber_observer = Some(fiber_event);
    }
}

/// A fiber context, as observer.c prints it (%p of the context).
fn ctx(id: u64) -> String {
    format!("{:#x}", id)
}

/// fiber_init_observer(), fiber_address_observer() + fiber_enter_observer()
/// + fiber_suspend_observer(), fiber_destroy_observer().
fn fiber_event(vm: &mut Vm, ev: FiberEvent) {
    let (init, switch, destroy) = STATE.with(|s| {
        let c = &s.borrow().cfg;
        (c.fiber_init, c.fiber_switch, c.fiber_destroy)
    });
    let out = match ev {
        FiberEvent::Init(id) if init => format!("<!-- alloc: {} -->\n", ctx(id)),
        FiberEvent::Destroy(id) if destroy => format!("<!-- destroy: {} -->\n", ctx(id)),
        FiberEvent::Switch { from, to, kind } if switch => {
            let mut o = format!(
                "<!-- switching from fiber {} to {} -->\n",
                ctx(from),
                ctx(to)
            );
            let line = match kind {
                FiberSwitch::Start => format!("<init '{}'>", ctx(to)),
                FiberSwitch::Resume => format!("<resume '{}'>", ctx(to)),
                FiberSwitch::Destroying => format!("<destroying '{}'>", ctx(to)),
                FiberSwitch::Suspend => format!("<suspend '{}'>", ctx(from)),
                FiberSwitch::Returned => format!("<returned '{}'>", ctx(from)),
                FiberSwitch::Threw => format!("<threw '{}'>", ctx(from)),
                FiberSwitch::Destroyed => format!("<destroyed '{}'>", ctx(from)),
            };
            o.push_str(&line);
            o.push('\n');
            o
        }
        _ => return,
    };
    vm.write_output(out.as_bytes());
}

fn parse_names(v: &[u8]) -> HashSet<Vec<u8>> {
    if v.is_empty() {
        return HashSet::new();
    }
    v.split(|&b| b == b',').map(|n| n.to_vec()).collect()
}

/// zend_test_observer_OnUpdateCommaList: only while the observer is enabled;
/// at runtime the observed functions change.
pub(crate) fn on_update_function_names(vm: &mut Vm, _name: &[u8], value: &Value) -> bool {
    if !ini_bool(vm, "zend_test.observer.enabled") {
        return false;
    }
    let names = parse_names(value.to_php_string().as_bytes());
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        // zend_observer_add/remove_begin_handler() on the functions named.
        let affected: Vec<Vec<u8>> = s
            .cfg
            .function_names
            .iter()
            .chain(names.iter())
            .cloned()
            .collect();
        for n in affected {
            let prefix = format!("{}@", String::from_utf8_lossy(&n).to_ascii_lowercase());
            let observed = names.contains(&n);
            for (k, v) in s.inited.iter_mut() {
                if k.starts_with(&prefix) || k.as_bytes() == n.to_ascii_lowercase() {
                    *v = observed;
                }
            }
        }
        s.cfg.function_names = names;
    });
    true
}

fn indent(depth: usize) -> String {
    " ".repeat(2 * depth)
}

/// What the observer knows of a function: its display name ("foo",
/// "Foo::bar"), the file of top-level code, and the fcall_init key.
struct Func {
    name: Option<String>,
    file: String,
    key: Option<String>,
    internal: bool,
    /// A builtin (native) function: its frame is not on the call stack.
    builtin: bool,
}

fn describe(vm: &Vm, frame: &ObservedFrame) -> Func {
    match frame {
        ObservedFrame::Internal(name) => Func {
            name: Some(name.to_string()),
            file: String::new(),
            key: Some(name.to_ascii_lowercase()),
            internal: true,
            builtin: true,
        },
        ObservedFrame::User(op) => user_func(vm, op),
    }
}

fn user_func(vm: &Vm, op: &OpArray) -> Func {
    let file = if op.filename.is_empty() {
        vm.current_file.clone()
    } else {
        String::from_utf8_lossy(&op.filename).into_owned()
    };
    if op.name.is_empty() {
        // Top-level code: compiled anew for every include / eval.
        return Func {
            name: None,
            file,
            key: None,
            internal: false,
            builtin: false,
        };
    }
    let fname = if op.name.starts_with(b"{closure") || op.name.starts_with(b"__closure") {
        if !op.display_name.is_empty() {
            String::from_utf8_lossy(&op.display_name).into_owned()
        } else {
            String::from_utf8_lossy(&op.name).into_owned()
        }
    } else {
        String::from_utf8_lossy(&op.name).into_owned()
    };
    // The function's scope (the class declaring the method).
    let name = match &op.scope_class {
        Some(scope) if !fname.contains("::") || fname.starts_with("{closure") => {
            let class = vm
                .classes
                .get(scope.as_slice())
                .map(|c| String::from_utf8_lossy(&c.name).into_owned())
                .unwrap_or_else(|| String::from_utf8_lossy(scope).into_owned());
            format!("{}::{}", class, fname)
        }
        _ => fname,
    };
    let key = format!("{}@{}:{}", name.to_ascii_lowercase(), file, op.decl_line);
    Func {
        name: Some(name),
        file,
        key: Some(key),
        internal: op.internal_sig.is_some(),
        builtin: false,
    }
}

fn begin(vm: &mut Vm, frame: &ObservedFrame) {
    let (enabled, execute_internal) = STATE.with(|s| {
        let s = s.borrow();
        (s.cfg.enabled, s.cfg.execute_internal)
    });
    let f = describe(vm, frame);
    // zend_test_execute_internal(): an internal function about to run.
    if execute_internal && matches!(frame, ObservedFrame::Internal(_)) {
        let depth = STATE.with(|s| s.borrow().depth);
        let line = format!(
            "{}<!-- internal enter {}() -->\n",
            indent(depth),
            f.name.as_deref().unwrap_or_default()
        );
        vm.write_output(line.as_bytes());
    }
    if !enabled {
        STATE.with(|s| s.borrow_mut().stack.push(false));
        return;
    }
    let cached = f
        .key
        .as_ref()
        .and_then(|k| STATE.with(|s| s.borrow().inited.get(k).copied()));
    let observed = match cached {
        Some(o) => o,
        None => fcall_init(vm, &f),
    };
    let show = observed && STATE.with(|s| s.borrow().cfg.show_output);
    if show {
        let depth = STATE.with(|s| s.borrow().depth);
        let line = match &f.name {
            Some(n) => format!("{}<{}>\n", indent(depth), n),
            None => format!("{}<file '{}'>\n", indent(depth), f.file),
        };
        vm.write_output(line.as_bytes());
        STATE.with(|s| s.borrow_mut().depth += 1);
    }
    STATE.with(|s| s.borrow_mut().stack.push(show));
}

/// observer_fcall_init(): the first call of a function. Returns whether
/// its calls are observed.
fn fcall_init(vm: &mut Vm, f: &Func) -> bool {
    let (show_output, backtrace, depth) = STATE.with(|s| {
        let s = s.borrow();
        (s.cfg.show_output, s.cfg.show_init_backtrace, s.depth)
    });
    if show_output {
        let line = match &f.name {
            Some(n) => format!("{}<!-- init {}() -->\n", indent(depth), n),
            None => format!(
                "{}<!-- init '{}' -->\n",
                indent(depth),
                if f.file.is_empty() {
                    "[no active file]"
                } else {
                    &f.file
                }
            ),
        };
        vm.write_output(line.as_bytes());
        if backtrace {
            init_backtrace(vm, f, depth);
        }
    }
    let observed = STATE.with(|s| {
        let s = s.borrow();
        let c = &s.cfg;
        if c.observe_all {
            true
        } else if let Some(n) = &f.name {
            c.observe_functions
                || c.function_names.contains(n.to_ascii_lowercase().as_bytes())
                || c.function_names.contains(n.as_bytes())
        } else {
            c.observe_includes
        }
    });
    if let Some(k) = &f.key {
        STATE.with(|s| s.borrow_mut().inited.insert(k.clone(), observed));
    }
    let _ = f.internal;
    observed
}

/// observer_show_init_backtrace(): the function being initialized and its
/// callers, innermost first.
fn init_backtrace(vm: &mut Vm, f: &Func, depth: usize) {
    // (not the builtin being initialized: its frame is not shown twice
    // when it prints a backtrace itself)
    if !f.builtin {
        vm.sync_builtin_frames();
    }
    let pad = indent(depth);
    let inner = " ".repeat(2 * depth + 4);
    let mut out = format!("{}<!--\n", pad);
    if let Some(n) = &f.name {
        out.push_str(&format!("{}{}()\n", inner, n));
    }
    // Frames are named by their function's scope and name: a closure's
    // frame by the class of its object.
    let objects = vm.frame_objects();
    let names: Vec<String> = vm
        .call_stack
        .iter()
        .enumerate()
        .map(|(i, (name, _, _, _, _))| {
            let n = name.replace("->", "::");
            match objects.get(i) {
                Some(Some(Value::Object(o))) if n.starts_with("{closure") => {
                    format!("{}::{}", String::from_utf8_lossy(&o.borrow().class_name), n)
                }
                _ => n,
            }
        })
        .collect();
    // The function's own frame may be on the call stack already.
    let skip = usize::from(f.name.as_ref().is_some_and(|n| {
        names
            .last()
            .is_some_and(|top| top == n || n.ends_with(&format!("::{top}")))
    }));
    for n in names.iter().rev().skip(skip) {
        match n.as_str() {
            "include" | "include_once" | "require" | "require_once" | "eval" => continue,
            _ => out.push_str(&format!("{}{}()\n", inner, n)),
        }
    }
    let main = if f.name.is_none() {
        f.file.clone()
    } else {
        main_script(vm)
    };
    out.push_str(&format!("{}{{main}} {}\n", inner, main));
    out.push_str(&format!("{}-->\n", pad));
    vm.write_output(out.as_bytes());
}

fn main_script(vm: &Vm) -> String {
    vm.constants
        .get(b"__zend_test_main_script".as_slice())
        .map(|v| v.to_php_string().to_string_lossy())
        .unwrap_or_else(|| vm.current_file.clone())
}

fn end(vm: &mut Vm, frame: &ObservedFrame, retval: Option<&Value>) {
    let Some(shown) = STATE.with(|s| s.borrow_mut().stack.pop()) else {
        return;
    };
    if !shown {
        return;
    }
    let f = describe(vm, frame);
    let depth = STATE.with(|s| s.borrow().depth);
    let mut out = String::new();
    if let Some(Value::Object(e)) = &vm.current_exception {
        out.push_str(&format!(
            "{}<!-- Exception: {} -->\n",
            indent(depth),
            String::from_utf8_lossy(&e.borrow().class_name)
        ));
    }
    let depth = depth.saturating_sub(1);
    STATE.with(|s| s.borrow_mut().depth = depth);
    match &f.name {
        Some(n) => {
            let info = retval_info(vm, retval, depth);
            out.push_str(&format!("{}</{}{}>\n", indent(depth), n, info));
        }
        None => out.push_str(&format!("{}</file '{}'>\n", indent(depth), f.file)),
    }
    vm.write_output(out.as_bytes());
}

/// get_retval_info()
fn retval_info(vm: &mut Vm, retval: Option<&Value>, depth: usize) -> String {
    let (show_type, show_value) = STATE.with(|s| {
        let s = s.borrow();
        (s.cfg.show_return_type, s.cfg.show_return_value)
    });
    if !show_type && !show_value {
        return String::new();
    }
    let Some(v) = retval else {
        return ":NULL".to_string();
    };
    if show_value {
        if let Value::Object(o) = v {
            let o: std::cell::Ref<PhpObject> = o.borrow();
            return format!(
                ":object({})#{}",
                String::from_utf8_lossy(&o.class_name),
                o.object_id
            );
        }
        return format!(":{}", var_export(vm, v, 2 * depth + 3));
    }
    format!(":{}", zval_type_name(v))
}

/// zend_zval_type_name()
fn zval_type_name(v: &Value) -> &'static str {
    match v {
        Value::Null | Value::Undef => "null",
        Value::True | Value::False => "bool",
        Value::Long(_) => "int",
        Value::Double(_) => "float",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) | Value::Generator(_) => "object",
        Value::Resource(_) => "resource",
        Value::Reference(r) => zval_type_name(&r.borrow()),
    }
}

/// php_var_export_ex() at nesting `level`.
fn var_export(vm: &mut Vm, v: &Value, level: usize) -> String {
    let Some(f) = vm.functions.get(b"var_export".as_slice()).copied() else {
        return String::new();
    };
    let r = f(vm, &[v.clone(), Value::True]).unwrap_or(Value::Null);
    let s = r.to_php_string().to_string_lossy();
    if !matches!(v, Value::Array(_)) || level <= 1 {
        return s;
    }
    // Nested lines are indented by the export level.
    let pad = " ".repeat(level - 1);
    let mut out = String::new();
    for (i, line) in s.split('\n').enumerate() {
        if i > 0 {
            out.push('\n');
            out.push_str(&pad);
        }
        out.push_str(line);
    }
    out
}
