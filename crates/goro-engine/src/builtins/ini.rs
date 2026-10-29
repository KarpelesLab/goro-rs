//! parse_ini_string() and parse_ini_file() over goro_ext_standard::ini
//! (PHP manual: parse_ini_file).

use goro_ext_standard::ini::{self, Event, IniValue, Mode};
use goro_runtime::{Key, PhpArray, PhpStr, Value};

use super::string::sv;
use crate::func::Args;
use crate::registry::{ConstVal, Module};
use crate::vm::{R, Vm};

pub static MODULE: Module = Module {
    name: "standard",
    functions: &[
        (
            "function parse_ini_string(string $ini_string, bool $process_sections = false, int $scanner_mode = INI_SCANNER_NORMAL): array|false",
            parse_ini_string,
        ),
        (
            "function parse_ini_file(string $filename, bool $process_sections = false, int $scanner_mode = INI_SCANNER_NORMAL): array|false",
            parse_ini_file,
        ),
    ],
    constants: &[
        ("INI_SCANNER_NORMAL", ConstVal::Int(0)),
        ("INI_SCANNER_RAW", ConstVal::Int(1)),
        ("INI_SCANNER_TYPED", ConstVal::Int(2)),
    ],
    listed: false,
    ..Module::EMPTY
};

struct VmEnv<'a> {
    vm: &'a mut Vm,
}

impl ini::Env for VmEnv<'_> {
    fn constant(&mut self, name: &[u8]) -> Option<Vec<u8>> {
        let v = self.vm.constants.get(&PhpStr::new(name))?.clone();
        let mut out = Vec::new();
        self.vm.append_to_string(&mut out, &v).ok()?;
        Some(out)
    }
    fn var(&mut self, name: &[u8]) -> Option<Vec<u8>> {
        if let Some(v) = self.vm.get_ini(name) {
            return Some(v.to_vec());
        }
        let n = std::str::from_utf8(name).ok()?;
        std::env::var_os(n).map(|v| {
            use std::os::unix::ffi::OsStrExt;
            v.as_bytes().to_vec()
        })
    }
}

fn to_value(v: IniValue) -> Value {
    match v {
        IniValue::Str(s) => sv(s),
        IniValue::Int(i) => Value::Int(i),
        IniValue::Float(f) => Value::Float(f),
        IniValue::Bool(b) => Value::Bool(b),
        IniValue::Null => Value::Null,
    }
}

fn put(target: &mut PhpArray, key: &[u8], offset: Option<Option<Vec<u8>>>, v: Value) {
    let k = Key::from_bytes(key);
    match offset {
        None => {
            target.insert(k, v);
        }
        Some(off) => {
            let e = target.entry(k);
            if !matches!(e, Value::Array(_)) {
                *e = Value::Array(PhpArray::new());
            }
            if let Value::Array(arr) = e {
                match off {
                    None => {
                        let _ = arr.push(v);
                    }
                    Some(o) => {
                        arr.insert(Key::from_bytes(&o), v);
                    }
                }
            }
        }
    }
}

fn parse_common(vm: &mut Vm, a: &Args, src: &[u8], origin: &str) -> R<Value> {
    let sections = a.bool_or(1, false);
    let Some(mode) = Mode::from_int(a.int_or(2, 0)) else {
        // A plain warning (observed on PHP 8.5; test Zend/tests/bug61087).
        vm.warn("Invalid scanner mode")?;
        return Ok(Value::Bool(false));
    };
    let events = {
        let mut env = VmEnv { vm };
        ini::parse(src, mode, &mut env)
    };
    let events = match events {
        Ok(e) => e,
        Err(e) => {
            // Observed on PHP 8.5: the message names the source and line,
            // ends with a newline, and is raised as a plain warning.
            vm.warn(&format!("{} in {} on line {}\n", e.message, origin, e.line))?;
            return Ok(Value::Bool(false));
        }
    };
    let mut out = PhpArray::new();
    let mut cur: Option<Key> = None;
    for ev in events {
        match ev {
            Event::Section(name) => {
                if sections {
                    let k = Key::from_bytes(&name);
                    out.insert(k.clone(), Value::Array(PhpArray::new()));
                    cur = Some(k);
                }
            }
            Event::Entry { key, offset, value } => {
                let v = to_value(value);
                match &cur {
                    Some(k) => {
                        let e = out.entry(k.clone());
                        if let Value::Array(arr) = e {
                            put(arr, &key, offset, v);
                        }
                    }
                    None => put(&mut out, &key, offset, v),
                }
            }
        }
    }
    Ok(Value::Array(out))
}

fn parse_ini_string(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let s = a.str(0);
    parse_common(vm, a, &s, "Unknown")
}

fn parse_ini_file(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let name = a.str(0);
    if name.is_empty() {
        return vm.fthrow(a, "ValueError", "Argument #1 ($filename) must not be empty");
    }
    if name.contains(&0) {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #1 ($filename) must not contain any null bytes",
        );
    }
    use std::os::unix::ffi::OsStrExt;
    let p = std::path::Path::new(std::ffi::OsStr::from_bytes(&name));
    let mut candidates = Vec::new();
    // include_path first (test parse_ini_file_variation3.phpt)
    if let Some(found) = vm.resolve_include(&name) {
        candidates.push(found);
    }
    candidates.push(if p.is_absolute() {
        p.to_path_buf()
    } else {
        vm.cwd.join(p)
    });
    if !p.is_absolute() {
        // Relative names are also looked up next to the running script
        // (test general_functions/bug49692.phpt).
        let script = std::path::PathBuf::from(std::ffi::OsStr::from_bytes(&vm.cur_file()));
        if let Some(dir) = script.parent() {
            candidates.push(dir.join(p));
        }
    }
    let candidates: Vec<std::path::PathBuf> = candidates;
    let Some(data) = candidates.iter().find_map(|c| std::fs::read(c).ok()) else {
        vm.warn(&format!(
            "parse_ini_file({}): Failed to open stream: No such file or directory",
            name.to_string_lossy()
        ))?;
        return Ok(Value::Bool(false));
    };
    let origin = name.to_string_lossy();
    parse_common(vm, a, &data, &origin)
}
