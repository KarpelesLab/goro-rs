//! SplFileInfo (name handling) / SplFileObject / SplTempFileObject.
//!
//! SplFileObject is implemented on top of a real stream resource opened via
//! the standard `fopen()` builtin, mirroring php-src's spl_directory.c
//! (current line caching, READ_AHEAD/SKIP_EMPTY/DROP_NEW_LINE/READ_CSV
//! flags, line numbering, csv control).
//!
//! Hidden state (all `__spl_*` properties):
//!   __spl_file_path   file_name (pathName)
//!   __spl_path        path (directory part), when known
//!   __spl_stream      Resource (SplFileObject only)
//!   __spl_open_mode   open mode string
//!   __spl_flags       SplFileObject flags
//!   __spl_max_line_len
//!   __spl_cur_line    current line (String) or absent/Null
//!   __spl_cur_zval    current csv row (Array) or absent/Null
//!   __spl_line_num    current line number
//!   __spl_csv_delimiter / __spl_csv_enclosure / __spl_csv_escape ("" = none)
//!   __spl_escape_set  True once setCsvControl() got an explicit escape

use std::cell::RefCell;
use std::rc::Rc;

use crate::array::PhpArray;
use crate::object::PhpObject;
use crate::spl::{obj_class_lower, val_arr};
use crate::string::PhpString;
use crate::value::Value;
use crate::vm::Vm;

const DROP_NEW_LINE: i64 = 1;
const READ_AHEAD: i64 = 2;
const SKIP_EMPTY: i64 = 4;
const READ_CSV: i64 = 8;

fn prop(obj: &Rc<RefCell<PhpObject>>, name: &[u8]) -> Value {
    obj.borrow().get_property(name)
}

fn set_prop(obj: &Rc<RefCell<PhpObject>>, name: &[u8], v: Value) {
    obj.borrow_mut().set_property_slice(name, v);
}

fn sv(s: &[u8]) -> Value {
    Value::String(PhpString::from_bytes(s))
}

fn is_slash(c: u8) -> bool {
    c == b'/'
}

/// php-src spl_filesystem_info_set_filename(): returns (file_name, path).
pub(crate) fn split_file_name(path: &[u8]) -> (Vec<u8>, Vec<u8>) {
    let mut path_len = path.len();
    let file_name = if path_len > 1 && is_slash(path[path_len - 1]) {
        loop {
            path_len -= 1;
            if !(path_len > 1 && is_slash(path[path_len - 1])) {
                break;
            }
        }
        path[..path_len].to_vec()
    } else {
        path.to_vec()
    };
    while path_len > 1 && !is_slash(path[path_len - 1]) {
        path_len -= 1;
    }
    path_len = path_len.saturating_sub(1);
    (file_name, path[..path_len].to_vec())
}

/// php_basename() with optional suffix stripping.
pub(crate) fn php_basename(s: &[u8], suffix: &[u8]) -> Vec<u8> {
    let mut end = s.len();
    while end > 0 && s[end - 1] == b'/' {
        end -= 1;
    }
    let mut start = end;
    while start > 0 && s[start - 1] != b'/' {
        start -= 1;
    }
    let mut base = s[start..end].to_vec();
    if !suffix.is_empty() && base.len() > suffix.len() && base.ends_with(suffix) {
        base.truncate(base.len() - suffix.len());
    }
    base
}

impl Vm {
    /// Call a registered builtin function by name. Returns None when the
    /// function failed (an exception may be set).
    pub(crate) fn spl_call_builtin(&mut self, name: &[u8], args: &[Value]) -> Option<Value> {
        // (the stream layer itself: a disabled function still works here)
        let f = self
            .functions
            .get(name)
            .or_else(|| self.disabled_function_impls.get(name))
            .copied()?;
        f(self, args).ok()
    }

    fn sfo_class_disp(&self, obj: &Rc<RefCell<PhpObject>>) -> String {
        crate::value::display_class_name(&obj.borrow().class_name).to_string()
    }

    fn sfi_not_initialized(&mut self) -> Option<Value> {
        self.spl_throw(b"Error", "Object not initialized")
    }

    pub(crate) fn sfi_set_filename(obj: &Rc<RefCell<PhpObject>>, path: &[u8]) {
        let (f, p) = split_file_name(path);
        set_prop(
            obj,
            b"__spl_file_path",
            Value::String(PhpString::from_vec(f)),
        );
        set_prop(obj, b"__spl_path", Value::String(PhpString::from_vec(p)));
    }

    fn sfo_csv_char(obj: &Rc<RefCell<PhpObject>>, name: &[u8], default: &[u8]) -> Value {
        match prop(obj, name) {
            Value::String(s) => Value::String(s),
            _ => sv(default),
        }
    }

    // ----- SplFileObject internals -----

    fn sfo_flags(obj: &Rc<RefCell<PhpObject>>) -> i64 {
        match prop(obj, b"__spl_flags") {
            Value::Long(f) => f,
            _ => 0,
        }
    }

    fn sfo_has_line(obj: &Rc<RefCell<PhpObject>>) -> bool {
        matches!(prop(obj, b"__spl_cur_line"), Value::String(_))
    }

    fn sfo_has_zval(obj: &Rc<RefCell<PhpObject>>) -> bool {
        matches!(prop(obj, b"__spl_cur_zval"), Value::Array(_))
    }

    fn sfo_line_num(obj: &Rc<RefCell<PhpObject>>) -> i64 {
        match prop(obj, b"__spl_line_num") {
            Value::Long(n) => n,
            _ => 0,
        }
    }

    fn sfo_add_line_num(obj: &Rc<RefCell<PhpObject>>, n: i64) {
        let cur = Self::sfo_line_num(obj);
        set_prop(obj, b"__spl_line_num", Value::Long(cur + n));
    }

    fn sfo_free_line(obj: &Rc<RefCell<PhpObject>>) {
        set_prop(obj, b"__spl_cur_line", Value::Null);
        set_prop(obj, b"__spl_cur_zval", Value::Null);
    }

    fn sfo_stream(obj: &Rc<RefCell<PhpObject>>) -> Option<Value> {
        match prop(obj, b"__spl_stream") {
            v @ Value::Resource(_) => Some(v),
            _ => None,
        }
    }

    fn sfo_file_name_str(obj: &Rc<RefCell<PhpObject>>) -> String {
        prop(obj, b"__spl_file_path")
            .to_php_string()
            .to_string_lossy()
    }

    fn sfo_eof(&mut self, stream: &Value) -> bool {
        self.spl_call_builtin(b"feof", std::slice::from_ref(stream))
            .map(|v| v.is_truthy())
            .unwrap_or(true)
    }

    fn sfo_cannot_read(&mut self, obj: &Rc<RefCell<PhpObject>>) {
        let msg = format!("Cannot read from file {}", Self::sfo_file_name_str(obj));
        self.spl_throw(b"RuntimeException", &msg);
    }

    fn sfo_read_ex(
        &mut self,
        obj: &Rc<RefCell<PhpObject>>,
        silent: bool,
        line_add: i64,
        csv: bool,
    ) -> bool {
        Self::sfo_free_line(obj);
        let stream = match Self::sfo_stream(obj) {
            Some(s) => s,
            None => return false,
        };
        if self.sfo_eof(&stream) {
            if !silent {
                self.sfo_cannot_read(obj);
            }
            return false;
        }
        let max = match prop(obj, b"__spl_max_line_len") {
            Value::Long(n) => n,
            _ => 0,
        };
        let buf = if max > 0 {
            self.spl_call_builtin(b"fgets", &[stream.clone(), Value::Long(max + 1)])
        } else {
            self.spl_call_builtin(b"fgets", std::slice::from_ref(&stream))
        };
        let line: Vec<u8> = match buf {
            Some(Value::String(s)) => {
                let mut b = s.as_bytes().to_vec();
                if !csv && Self::sfo_flags(obj) & DROP_NEW_LINE != 0 && b.last() == Some(&b'\n') {
                    b.pop();
                    if b.last() == Some(&b'\r') {
                        b.pop();
                    }
                }
                b
            }
            _ => Vec::new(),
        };
        set_prop(
            obj,
            b"__spl_cur_line",
            Value::String(PhpString::from_vec(line)),
        );
        Self::sfo_add_line_num(obj, line_add);
        true
    }

    fn sfo_read(&mut self, obj: &Rc<RefCell<PhpObject>>, silent: bool, csv: bool) -> bool {
        let add = if Self::sfo_has_line(obj) { 1 } else { 0 };
        self.sfo_read_ex(obj, silent, add, csv)
    }

    fn sfo_is_line_empty(obj: &Rc<RefCell<PhpObject>>) -> bool {
        let line = match prop(obj, b"__spl_cur_line") {
            Value::String(s) => s.as_bytes().to_vec(),
            _ => return true,
        };
        let flags = Self::sfo_flags(obj);
        line.is_empty()
            || (flags & READ_CSV != 0
                && flags & DROP_NEW_LINE != 0
                && (line == b"\n" || line == b"\r\n"))
    }

    fn sfo_read_csv(
        &mut self,
        obj: &Rc<RefCell<PhpObject>>,
        delim: Value,
        encl: Value,
        esc: Value,
        silent: bool,
    ) -> Option<Value> {
        let stream = Self::sfo_stream(obj)?;
        let mut start;
        loop {
            start = self
                .spl_call_builtin(b"ftell", std::slice::from_ref(&stream))
                .unwrap_or(Value::False);
            if !self.sfo_read(obj, silent, true) {
                return None;
            }
            if !(Self::sfo_is_line_empty(obj) && Self::sfo_flags(obj) & SKIP_EMPTY != 0) {
                break;
            }
        }
        let line = prop(obj, b"__spl_cur_line").to_php_string();
        // Re-parse from the start of the line so multi-line enclosures can
        // continue reading from the stream, as php_fgetcsv() does.
        let mut values: Option<Value> = None;
        if let Value::Long(pos) = start {
            if !line.as_bytes().is_empty() {
                self.spl_call_builtin(b"fseek", &[stream.clone(), Value::Long(pos)]);
                let r = self
                    .spl_call_builtin(b"fgetcsv", &[stream.clone(), Value::Null, delim, encl, esc]);
                if let Some(Value::Array(a)) = r {
                    values = Some(Value::Array(a));
                }
            }
        }
        let values = values.unwrap_or_else(|| {
            let mut a = PhpArray::new();
            a.push(Value::Null);
            val_arr(a)
        });
        set_prop(obj, b"__spl_cur_zval", values.clone());
        Some(values)
    }

    fn sfo_user_get_current_line(&self, obj: &Rc<RefCell<PhpObject>>) -> bool {
        let cl = obj_class_lower(obj);
        self.classes
            .get(&cl)
            .map(|c| c.get_method(b"getcurrentline").is_some())
            .unwrap_or(false)
    }

    fn sfo_read_line_ex(&mut self, obj: &Rc<RefCell<PhpObject>>, silent: bool) -> bool {
        if Self::sfo_flags(obj) & READ_CSV != 0 {
            let (d, e, x) = self.sfo_csv_controls(obj);
            return self.sfo_read_csv(obj, d, e, x, silent).is_some();
        }
        if self.sfo_user_get_current_line(obj) {
            Self::sfo_free_line(obj);
            let stream = match Self::sfo_stream(obj) {
                Some(s) => s,
                None => return false,
            };
            if self.sfo_eof(&stream) {
                if !silent {
                    self.sfo_cannot_read(obj);
                }
                return false;
            }
            let r = self.call_object_method(&Value::Object(obj.clone()), b"getCurrentLine", &[]);
            if self.current_exception.is_some() {
                return false;
            }
            let r = match r {
                Some(v) => v.deref(),
                None => return false,
            };
            let s = match r {
                Value::String(s) => s,
                other => {
                    let msg = format!(
                        "{}::getCurrentLine(): Return value must be of type string, {} returned",
                        self.sfo_class_disp(obj),
                        Self::value_type_name(&other)
                    );
                    self.spl_throw(b"TypeError", &msg);
                    return false;
                }
            };
            // (a line the method itself read, e.g. with fgetcsv())
            if Self::sfo_has_line(obj) || Self::sfo_has_zval(obj) {
                Self::sfo_add_line_num(obj, 1);
            }
            Self::sfo_free_line(obj);
            set_prop(obj, b"__spl_cur_line", Value::String(s));
            return true;
        }
        self.sfo_read(obj, silent, false)
    }

    fn sfo_read_line(&mut self, obj: &Rc<RefCell<PhpObject>>, silent: bool) -> bool {
        let mut ret = self.sfo_read_line_ex(obj, silent);
        while Self::sfo_flags(obj) & SKIP_EMPTY != 0 && ret && Self::sfo_is_line_empty(obj) {
            Self::sfo_free_line(obj);
            ret = self.sfo_read_line_ex(obj, silent);
        }
        ret
    }

    fn sfo_rewind(&mut self, obj: &Rc<RefCell<PhpObject>>) {
        let stream = match Self::sfo_stream(obj) {
            Some(s) => s,
            None => {
                self.sfi_not_initialized();
                return;
            }
        };
        let ok = self
            .spl_call_builtin(b"rewind", &[stream])
            .map(|v| v.is_truthy())
            .unwrap_or(false);
        if !ok {
            let msg = format!("Cannot rewind file {}", Self::sfo_file_name_str(obj));
            self.spl_throw(b"RuntimeException", &msg);
            return;
        }
        Self::sfo_free_line(obj);
        set_prop(obj, b"__spl_line_num", Value::Long(0));
        if Self::sfo_flags(obj) & READ_AHEAD != 0 {
            self.sfo_read_line(obj, true);
        }
    }

    fn sfo_csv_controls(&self, obj: &Rc<RefCell<PhpObject>>) -> (Value, Value, Value) {
        (
            Self::sfo_csv_char(obj, b"__spl_csv_delimiter", b","),
            Self::sfo_csv_char(obj, b"__spl_csv_enclosure", b"\""),
            Self::sfo_csv_char(obj, b"__spl_csv_escape", b"\\"),
        )
    }

    /// Validate a single-character csv argument. Err(()) => exception set.
    fn sfo_csv_arg(
        &mut self,
        func: &str,
        v: Option<&Value>,
        argn: usize,
        name: &str,
    ) -> Result<Option<Value>, ()> {
        match v.map(|v| v.deref()) {
            None | Some(Value::Undef) => Ok(None),
            Some(v) => {
                let s = match self.spl_arg_string(func, argn, name, &v) {
                    Some(s) => s,
                    None => return Err(()),
                };
                if s.len() != 1 {
                    let msg = format!(
                        "{}(): Argument #{} (${}) must be a single character",
                        func, argn, name
                    );
                    self.spl_throw(b"ValueError", &msg);
                    return Err(());
                }
                Ok(Some(Value::String(s)))
            }
        }
    }

    /// Escape argument for fgetcsv/fputcsv (spl_csv_enclosure_param_handling).
    fn sfo_escape_arg(
        &mut self,
        obj: &Rc<RefCell<PhpObject>>,
        func: &str,
        v: Option<&Value>,
        argn: usize,
    ) -> Result<Value, ()> {
        match v.map(|v| v.deref()) {
            None | Some(Value::Undef) => {
                if !matches!(prop(obj, b"__spl_escape_set"), Value::True) {
                    let msg = format!(
                        "{}(): the $escape parameter must be provided, as its default value will change, either explicitly or via SplFileObject::setCsvControl()",
                        func
                    );
                    self.emit_deprecated(&msg);
                    if self.current_exception.is_some() {
                        return Err(());
                    }
                }
                Ok(Self::sfo_csv_char(obj, b"__spl_csv_escape", b"\\"))
            }
            Some(v) => {
                let s = match self.spl_arg_string(func, argn, "escape", &v) {
                    Some(s) => s,
                    None => return Err(()),
                };
                if s.len() > 1 {
                    let msg = format!(
                        "{}(): Argument #{} ($escape) must be empty or a single character",
                        func, argn
                    );
                    self.spl_throw(b"ValueError", &msg);
                    return Err(());
                }
                Ok(Value::String(s))
            }
        }
    }

    /// Coerce an argument to string (non-strict mode). None => TypeError set.
    pub(crate) fn spl_arg_string(
        &mut self,
        func: &str,
        argn: usize,
        name: &str,
        v: &Value,
    ) -> Option<PhpString> {
        match v.deref() {
            Value::String(s) => Some(s),
            Value::Long(_) | Value::Double(_) | Value::True | Value::False => {
                Some(v.to_php_string())
            }
            Value::Null | Value::Undef => {
                let msg = format!(
                    "{}(): Passing null to parameter #{} (${}) of type string is deprecated",
                    func, argn, name
                );
                self.emit_deprecated(&msg);
                Some(PhpString::empty())
            }
            Value::Object(o) => {
                let cl = obj_class_lower(&o);
                let has_ts = self
                    .classes
                    .get(&cl)
                    .map(|c| c.get_method(b"__tostring").is_some())
                    .unwrap_or(false)
                    || self.spl_has_builtin_tostring(&cl);
                if has_ts {
                    let line = self.current_line;
                    let s = self.value_to_string(v);
                    self.current_line = line;
                    if self.current_exception.is_some() {
                        return None;
                    }
                    return Some(s);
                }
                let msg = format!(
                    "{}(): Argument #{} (${}) must be of type string, {} given",
                    func,
                    argn,
                    name,
                    Self::value_type_name(v)
                );
                self.spl_throw(b"TypeError", &msg);
                None
            }
            other => {
                let msg = format!(
                    "{}(): Argument #{} (${}) must be of type string, {} given",
                    func,
                    argn,
                    name,
                    Self::value_type_name(&other)
                );
                self.spl_throw(b"TypeError", &msg);
                None
            }
        }
    }

    /// Coerce an argument to int (non-strict mode). None => TypeError set.
    pub(crate) fn spl_arg_long(
        &mut self,
        func: &str,
        argn: usize,
        name: &str,
        v: &Value,
    ) -> Option<i64> {
        self.spl_arg_long_as(func, argn, name, v, "int")
    }

    /// spl_arg_long() of a parameter declared as `tname` (int or ?int, for
    /// the TypeError message; null is handled by the caller for ?int).
    #[allow(clippy::if_same_then_else)] // branches mirror php-src
    pub(crate) fn spl_arg_long_as(
        &mut self,
        func: &str,
        argn: usize,
        name: &str,
        v: &Value,
        tname: &str,
    ) -> Option<i64> {
        match v.deref() {
            Value::Long(n) => Some(n),
            Value::True => Some(1),
            Value::False => Some(0),
            Value::Double(d) => {
                if d.is_finite() && d.fract() == 0.0 && d.abs() < 9.2e18 {
                    Some(d as i64)
                } else if d.is_finite() && d.abs() < 9.2e18 {
                    let msg = format!(
                        "Implicit conversion from float {} to int loses precision",
                        crate::value::format_php_float_repr(d)
                    );
                    self.emit_deprecated(&msg);
                    Some(d as i64)
                } else {
                    let msg = format!(
                        "{}(): Argument #{} (${}) must be of type {}, float given",
                        func, argn, name, tname
                    );
                    self.spl_throw(b"TypeError", &msg);
                    None
                }
            }
            Value::Null | Value::Undef => {
                let msg = format!(
                    "{}(): Passing null to parameter #{} (${}) of type int is deprecated",
                    func, argn, name
                );
                self.emit_deprecated(&msg);
                Some(0)
            }
            Value::String(s) => {
                let t = s.to_string_lossy();
                let tt = t.trim_start();
                if let Ok(n) = tt.trim_end().parse::<i64>() {
                    Some(n)
                } else if let Ok(d) = tt.trim_end().parse::<f64>() {
                    if d.fract() == 0.0 {
                        Some(d as i64)
                    } else {
                        Some(d as i64)
                    }
                } else {
                    let msg = format!(
                        "{}(): Argument #{} (${}) must be of type {}, string given",
                        func, argn, name, tname
                    );
                    self.spl_throw(b"TypeError", &msg);
                    None
                }
            }
            other => {
                let msg = format!(
                    "{}(): Argument #{} (${}) must be of type {}, {} given",
                    func,
                    argn,
                    name,
                    tname,
                    Self::value_type_name(&other)
                );
                self.spl_throw(b"TypeError", &msg);
                None
            }
        }
    }

    fn sfo_expects_none(&mut self, func: &str, args: Option<&[Value]>) -> bool {
        if let Some(a) = args {
            if !a.is_empty() {
                let msg = format!("{}() expects exactly 0 arguments, {} given", func, a.len());
                self.spl_throw(b"ArgumentCountError", &msg);
                return false;
            }
        }
        true
    }

    /// Open the stream for an SplFileObject (spl_filesystem_file_open).
    /// `ctor` is the active function name used for promoted warnings.
    fn sfo_open(
        &mut self,
        obj: &Rc<RefCell<PhpObject>>,
        file_name: &[u8],
        mode: &[u8],
        use_include_path: bool,
        context: Value,
        ctor: &str,
    ) -> bool {
        let fname_str = String::from_utf8_lossy(file_name).to_string();
        // php_stat(FS_IS_DIR): stream wrappers answer through url_stat
        let is_dir = if fname_str.contains("://") {
            let saved = self.warning_capture.replace(Vec::new());
            let r = self.spl_call_builtin(
                b"is_dir",
                &[Value::String(PhpString::from_vec(file_name.to_vec()))],
            );
            self.warning_capture = saved;
            matches!(r, Some(Value::True))
        } else {
            std::path::Path::new(&fname_str).is_dir()
        };
        if is_dir {
            self.spl_throw(
                b"LogicException",
                "Cannot use SplFileObject with directories",
            );
            return false;
        }
        let prev = self.warning_capture.replace(Vec::new());
        let mut fargs = vec![
            Value::String(PhpString::from_vec(file_name.to_vec())),
            Value::String(PhpString::from_vec(mode.to_vec())),
            if use_include_path {
                Value::True
            } else {
                Value::False
            },
        ];
        if !matches!(context, Value::Null | Value::Undef) {
            fargs.push(context);
        }
        let r = self.spl_call_builtin(b"fopen", &fargs);
        let captured = std::mem::replace(&mut self.warning_capture, prev).unwrap_or_default();
        let res = match r {
            Some(v @ Value::Resource(_)) if !file_name.is_empty() => v,
            _ => {
                if self.current_exception.is_none() {
                    if let Some(w) = captured.first() {
                        // "fopen(x): msg" -> "SplFileObject::__construct(x): msg"
                        let msg = match w.find('(') {
                            Some(i)
                                if w[..i]
                                    .chars()
                                    .all(|c| c.is_ascii_alphanumeric() || c == '_') =>
                            {
                                format!("{}{}", ctor, &w[i..])
                            }
                            _ => format!("{}(): {}", ctor, w),
                        };
                        self.spl_throw(b"RuntimeException", &msg);
                    } else {
                        let msg = format!("Cannot open file '{}'", fname_str);
                        self.spl_throw(b"RuntimeException", &msg);
                    }
                }
                return false;
            }
        };
        let (fname, _) = split_file_name(file_name);
        let fname = if file_name.len() > 1 && file_name[file_name.len() - 1] == b'/' {
            file_name[..file_name.len() - 1].to_vec()
        } else {
            let _ = fname;
            file_name.to_vec()
        };
        // intern->path / orig_path come from the opened stream's orig_path:
        // the file found through the include path.
        let resolved = if use_include_path
            && !fname_str.contains("://")
            && !fname_str.starts_with('/')
            && !std::path::Path::new(&fname_str).exists()
        {
            // php_resolve_path(): the include path, then the directory of
            // the executing script.
            let inc = self
                .constants
                .get(b"include_path".as_slice())
                .map(|v| v.to_php_string().to_string_lossy())
                .unwrap_or_else(|| ".".to_string());
            let script_dir = std::path::Path::new(&self.current_file)
                .parent()
                .map(|p| p.to_string_lossy().into_owned());
            inc.split(':')
                .filter(|d| !d.is_empty())
                .map(|d| d.to_string())
                .chain(script_dir)
                .map(|d| format!("{}/{}", d, fname_str))
                .find(|c| std::path::Path::new(c).is_file())
                .map(|c| {
                    std::fs::canonicalize(&c)
                        .map(|p| p.to_string_lossy().into_owned())
                        .unwrap_or(c)
                        .into_bytes()
                })
        } else {
            None
        };
        // The object owns its stream: freed with it, not fclose()-able.
        let rid = res.resource_id();
        crate::value::adopt_new_resource(rid);
        crate::value::set_resource_no_fclose(rid, true);
        // (its dtor_obj closes the stream in the shutdown destructor pass)
        if !obj.borrow().dtor_listed.get() {
            self.list_destructible(Value::Object(obj.clone()));
        }
        let mut ob = obj.borrow_mut();
        ob.set_property_slice(b"__spl_stream", res);
        ob.set_property_slice(
            b"__spl_file_path",
            Value::String(PhpString::from_vec(fname)),
        );
        if let Some(r) = resolved {
            ob.set_property(
                b"__spl_orig_path".to_vec(),
                Value::String(PhpString::from_vec(r)),
            );
        }
        ob.set_property(
            b"__spl_open_mode".to_vec(),
            Value::String(PhpString::from_vec(mode.to_vec())),
        );
        ob.set_property_slice(b"__spl_csv_delimiter", sv(b","));
        ob.set_property_slice(b"__spl_csv_enclosure", sv(b"\""));
        ob.set_property_slice(b"__spl_csv_escape", sv(b"\\"));
        ob.set_property_slice(b"__spl_escape_set", Value::False);
        ob.set_property_slice(b"__spl_line_num", Value::Long(0));
        if !matches!(ob.get_property(b"__spl_flags"), Value::Long(_)) {
            ob.set_property_slice(b"__spl_flags", Value::Long(0));
        }
        if !matches!(ob.get_property(b"__spl_max_line_len"), Value::Long(_)) {
            ob.set_property_slice(b"__spl_max_line_len", Value::Long(0));
        }
        true
    }

    /// SplFileObject::__construct / SplTempFileObject::__construct.
    /// `args` excludes $this.
    pub fn spl_file_object_construct(
        &mut self,
        obj: &Rc<RefCell<PhpObject>>,
        args: &[Value],
    ) -> bool {
        self.spl_file_object_construct_in(obj, args, "SplFileObject::__construct")
    }

    /// spl_file_object_construct(), with the stream opened (and its
    /// failure reported) by `open_func` (SplFileInfo::openFile()).
    pub(crate) fn spl_file_object_construct_in(
        &mut self,
        obj: &Rc<RefCell<PhpObject>>,
        args: &[Value],
        open_func: &str,
    ) -> bool {
        let cl = obj_class_lower(obj);
        let is_temp = self.spl_find_base(&cl, &[b"spltempfileobject", b"splfileobject"])
            == Some(b"spltempfileobject");
        if is_temp {
            let func = "SplTempFileObject::__construct";
            if args.len() > 1 {
                let msg = format!(
                    "{}() expects at most 1 argument, {} given",
                    func,
                    args.len()
                );
                self.spl_throw(b"ArgumentCountError", &msg);
                return false;
            }
            let max_memory = match args.first() {
                Some(v) => match self.spl_arg_long(func, 1, "maxMemory", v) {
                    Some(n) => Some(n),
                    None => return false,
                },
                None => None,
            };
            if Self::sfo_stream(obj).is_some() {
                self.spl_throw(b"Error", "Cannot call constructor twice");
                return false;
            }
            let file_name: Vec<u8> = match max_memory {
                Some(n) if n < 0 => b"php://memory".to_vec(),
                Some(n) => format!("php://temp/maxmemory:{}", n).into_bytes(),
                None => b"php://temp".to_vec(),
            };
            if self.sfo_open(obj, &file_name, b"wb", false, Value::Null, func) {
                set_prop(obj, b"__spl_path", sv(b""));
                return true;
            }
            return false;
        }
        let func = "SplFileObject::__construct";
        if args.is_empty() {
            let msg = format!("{}() expects at least 1 argument, 0 given", func);
            self.spl_throw(b"ArgumentCountError", &msg);
            return false;
        }
        if args.len() > 4 {
            let msg = format!(
                "{}() expects at most 4 arguments, {} given",
                func,
                args.len()
            );
            self.spl_throw(b"ArgumentCountError", &msg);
            return false;
        }
        let file_name = match self.spl_arg_string(func, 1, "filename", &args[0]) {
            Some(s) => s.as_bytes().to_vec(),
            None => return false,
        };
        if file_name.contains(&0) {
            let msg = format!(
                "{}(): Argument #1 ($filename) must not contain any null bytes",
                func
            );
            self.spl_throw(b"ValueError", &msg);
            return false;
        }
        let mode = match args.get(1) {
            Some(v) => match self.spl_arg_string(func, 2, "mode", v) {
                Some(s) => s.as_bytes().to_vec(),
                None => return false,
            },
            None => b"r".to_vec(),
        };
        let use_include_path = args.get(2).map(|v| v.is_truthy()).unwrap_or(false);
        let context = args.get(3).cloned().unwrap_or(Value::Null).deref();
        if !matches!(context, Value::Null | Value::Resource(_)) {
            let msg = format!(
                "{}(): Argument #4 ($context) must be of type resource or null, {} given",
                func,
                Self::value_type_name(&context)
            );
            self.spl_throw(b"TypeError", &msg);
            return false;
        }
        if Self::sfo_stream(obj).is_some() {
            self.spl_throw(b"Error", "Cannot call constructor twice");
            return false;
        }
        if !self.sfo_open(obj, &file_name, &mode, use_include_path, context, open_func) {
            return false;
        }
        // path: directory part of the opened path (stream->orig_path)
        let orig = match prop(obj, b"__spl_orig_path") {
            Value::String(s) => s.as_bytes().to_vec(),
            _ => prop(obj, b"__spl_file_path")
                .to_php_string()
                .as_bytes()
                .to_vec(),
        };
        let mut path_len = orig.len();
        if path_len > 1 && orig[path_len - 1] == b'/' {
            path_len -= 1;
        }
        while path_len > 1 && orig[path_len - 1] != b'/' {
            path_len -= 1;
        }
        path_len = path_len.saturating_sub(1);
        set_prop(
            obj,
            b"__spl_path",
            Value::String(PhpString::from_vec(orig[..path_len].to_vec())),
        );
        true
    }

    /// Dispatch an SplFileObject method. `args` excludes $this; None on the
    /// no-argument dispatch path (methods needing args then return None).
    pub(crate) fn spl_file_object_call(
        &mut self,
        method: &[u8],
        obj: &Rc<RefCell<PhpObject>>,
        args: Option<&[Value]>,
    ) -> Option<Value> {
        // (the stream functions called report as the method)
        let name = format!(
            "SplFileObject::{}",
            match method {
                b"getcsvcontrol" => "getCsvControl",
                b"setcsvcontrol" => "setCsvControl",
                b"getflags" => "getFlags",
                b"setflags" => "setFlags",
                b"getmaxlinelen" => "getMaxLineLen",
                b"setmaxlinelen" => "setMaxLineLen",
                b"getcurrentline" => "getCurrentLine",
                b"haschildren" => "hasChildren",
                b"getchildren" => "getChildren",
                b"__tostring" => "__toString",
                m => std::str::from_utf8(m).unwrap_or(""),
            }
        );
        let saved = self
            .spl_active_fn
            .replace((name, self.builtin_frames.len()));
        let r = self.spl_file_object_call_inner(method, obj, args);
        self.spl_active_fn = saved;
        r
    }

    fn spl_file_object_call_inner(
        &mut self,
        method: &[u8],
        obj: &Rc<RefCell<PhpObject>>,
        args: Option<&[Value]>,
    ) -> Option<Value> {
        let needs_args = matches!(
            method,
            b"setflags"
                | b"setmaxlinelen"
                | b"fgetcsv"
                | b"fputcsv"
                | b"setcsvcontrol"
                | b"flock"
                | b"fseek"
                | b"fscanf"
                | b"fwrite"
                | b"fread"
                | b"ftruncate"
                | b"seek"
        );
        if needs_args && args.is_none() {
            return None;
        }
        let a: &[Value] = args.unwrap_or(&[]);
        macro_rules! stream_or_throw {
            () => {
                match Self::sfo_stream(obj) {
                    Some(s) => s,
                    None => return self.sfi_not_initialized(),
                }
            };
        }
        match method {
            b"rewind" => {
                self.sfo_rewind(obj);
                Some(Value::Null)
            }
            b"eof" => {
                let s = stream_or_throw!();
                Some(if self.sfo_eof(&s) {
                    Value::True
                } else {
                    Value::False
                })
            }
            b"valid" => {
                if Self::sfo_flags(obj) & READ_AHEAD != 0 {
                    return Some(if Self::sfo_has_line(obj) || Self::sfo_has_zval(obj) {
                        Value::True
                    } else {
                        Value::False
                    });
                }
                match Self::sfo_stream(obj) {
                    None => Some(Value::False),
                    Some(s) => Some(if self.sfo_eof(&s) {
                        Value::False
                    } else {
                        Value::True
                    }),
                }
            }
            b"fgets" | b"getcurrentline" => {
                if !self.sfo_expects_none(
                    if method == b"fgets" {
                        "SplFileObject::fgets"
                    } else {
                        "SplFileObject::getCurrentLine"
                    },
                    args,
                ) {
                    return Some(Value::Null);
                }
                let _ = stream_or_throw!();
                if !self.sfo_read_ex(obj, false, 1, false) {
                    return Some(Value::Null);
                }
                Some(prop(obj, b"__spl_cur_line"))
            }
            b"current" => {
                let _ = stream_or_throw!();
                if !Self::sfo_has_line(obj) && !Self::sfo_has_zval(obj) {
                    self.sfo_read_line(obj, true);
                    if self.current_exception.is_some() {
                        return Some(Value::Null);
                    }
                }
                let csv = Self::sfo_flags(obj) & READ_CSV != 0;
                if Self::sfo_has_line(obj) && (!csv || !Self::sfo_has_zval(obj)) {
                    return Some(prop(obj, b"__spl_cur_line"));
                } else if Self::sfo_has_zval(obj) {
                    return Some(prop(obj, b"__spl_cur_zval"));
                }
                Some(Value::False)
            }
            b"key" => Some(Value::Long(Self::sfo_line_num(obj))),
            b"next" => {
                Self::sfo_free_line(obj);
                if Self::sfo_flags(obj) & READ_AHEAD != 0 {
                    self.sfo_read_line(obj, true);
                }
                Self::sfo_add_line_num(obj, 1);
                Some(Value::Null)
            }
            b"setflags" => {
                let f = match a.first() {
                    Some(v) => match self.spl_arg_long("SplFileObject::setFlags", 1, "flags", v) {
                        Some(n) => n,
                        None => return Some(Value::Null),
                    },
                    None => {
                        return self.spl_throw(
                            b"ArgumentCountError",
                            "SplFileObject::setFlags() expects exactly 1 argument, 0 given",
                        );
                    }
                };
                set_prop(obj, b"__spl_flags", Value::Long(f));
                Some(Value::Null)
            }
            b"getflags" => Some(Value::Long(Self::sfo_flags(obj) & 0xF)),
            b"setmaxlinelen" => {
                let n = match a.first() {
                    Some(v) => {
                        match self.spl_arg_long("SplFileObject::setMaxLineLen", 1, "maxLength", v) {
                            Some(n) => n,
                            None => return Some(Value::Null),
                        }
                    }
                    None => {
                        return self.spl_throw(
                            b"ArgumentCountError",
                            "SplFileObject::setMaxLineLen() expects exactly 1 argument, 0 given",
                        );
                    }
                };
                if n < 0 {
                    return self.spl_throw(b"ValueError", "SplFileObject::setMaxLineLen(): Argument #1 ($maxLength) must be greater than or equal to 0");
                }
                set_prop(obj, b"__spl_max_line_len", Value::Long(n));
                Some(Value::Null)
            }
            b"getmaxlinelen" => Some(match prop(obj, b"__spl_max_line_len") {
                Value::Long(n) => Value::Long(n),
                _ => Value::Long(0),
            }),
            b"haschildren" => Some(Value::False),
            b"getchildren" => Some(Value::Null),
            b"fgetcsv" => {
                let func = "SplFileObject::fgetcsv";
                let (cd, ce, _) = self.sfo_csv_controls(obj);
                let d = match self.sfo_csv_arg(func, a.first(), 1, "separator") {
                    Ok(v) => v.unwrap_or(cd),
                    Err(()) => return Some(Value::Null),
                };
                let e = match self.sfo_csv_arg(func, a.get(1), 2, "enclosure") {
                    Ok(v) => v.unwrap_or(ce),
                    Err(()) => return Some(Value::Null),
                };
                let _ = stream_or_throw!();
                let x = match self.sfo_escape_arg(obj, func, a.get(2), 3) {
                    Ok(v) => v,
                    Err(()) => return Some(Value::Null),
                };
                match self.sfo_read_csv(obj, d, e, x, true) {
                    Some(v) => Some(v),
                    None => Some(Value::False),
                }
            }
            b"fputcsv" => {
                let func = "SplFileObject::fputcsv";
                let fields = match a.first().map(|v| v.deref()) {
                    Some(v @ Value::Array(_)) => v,
                    Some(other) => {
                        let msg = format!(
                            "{}(): Argument #1 ($fields) must be of type array, {} given",
                            func,
                            Self::value_type_name(&other)
                        );
                        return self.spl_throw(b"TypeError", &msg);
                    }
                    None => {
                        return self.spl_throw(
                            b"ArgumentCountError",
                            "SplFileObject::fputcsv() expects at least 1 argument, 0 given",
                        );
                    }
                };
                let (cd, ce, _) = self.sfo_csv_controls(obj);
                let d = match self.sfo_csv_arg(func, a.get(1), 2, "separator") {
                    Ok(v) => v.unwrap_or(cd),
                    Err(()) => return Some(Value::Null),
                };
                let e = match self.sfo_csv_arg(func, a.get(2), 3, "enclosure") {
                    Ok(v) => v.unwrap_or(ce),
                    Err(()) => return Some(Value::Null),
                };
                let x = match self.sfo_escape_arg(obj, func, a.get(3), 4) {
                    Ok(v) => v,
                    Err(()) => return Some(Value::Null),
                };
                let s = stream_or_throw!();
                let mut fargs = vec![s, fields, d, e, x];
                if let Some(eol) = a.get(4) {
                    fargs.push(eol.clone());
                }
                Some(
                    self.spl_call_builtin(b"fputcsv", &fargs)
                        .unwrap_or(Value::False),
                )
            }
            b"setcsvcontrol" => {
                let func = "SplFileObject::setCsvControl";
                let d = match self.sfo_csv_arg(func, a.first(), 1, "separator") {
                    Ok(v) => v.unwrap_or_else(|| sv(b",")),
                    Err(()) => return Some(Value::Null),
                };
                let e = match self.sfo_csv_arg(func, a.get(1), 2, "enclosure") {
                    Ok(v) => v.unwrap_or_else(|| sv(b"\"")),
                    Err(()) => return Some(Value::Null),
                };
                let x = match a.get(2).map(|v| v.deref()) {
                    None | Some(Value::Undef) => {
                        let msg = format!(
                            "{}(): the $escape parameter must be provided as its default value will change",
                            func
                        );
                        self.emit_deprecated(&msg);
                        if self.current_exception.is_some() {
                            return Some(Value::Null);
                        }
                        sv(b"\\")
                    }
                    Some(v) => {
                        let s = match self.spl_arg_string(func, 3, "escape", &v) {
                            Some(s) => s,
                            None => return Some(Value::Null),
                        };
                        if s.len() > 1 {
                            return self.spl_throw(b"ValueError", "SplFileObject::setCsvControl(): Argument #3 ($escape) must be empty or a single character");
                        }
                        set_prop(obj, b"__spl_escape_set", Value::True);
                        Value::String(s)
                    }
                };
                set_prop(obj, b"__spl_csv_delimiter", d);
                set_prop(obj, b"__spl_csv_enclosure", e);
                set_prop(obj, b"__spl_csv_escape", x);
                Some(Value::Null)
            }
            b"getcsvcontrol" => {
                let (d, e, x) = self.sfo_csv_controls(obj);
                let mut arr = PhpArray::new();
                arr.push(d);
                arr.push(e);
                arr.push(x);
                Some(val_arr(arr))
            }
            b"flock" => {
                let s = stream_or_throw!();
                let mut fargs = vec![s];
                fargs.extend_from_slice(a);
                Some(
                    self.spl_call_builtin(b"flock", &fargs)
                        .unwrap_or(Value::False),
                )
            }
            b"fflush" => {
                let s = stream_or_throw!();
                Some(
                    self.spl_call_builtin(b"fflush", &[s])
                        .unwrap_or(Value::False),
                )
            }
            b"ftell" => {
                let s = stream_or_throw!();
                Some(
                    self.spl_call_builtin(b"ftell", &[s])
                        .unwrap_or(Value::False),
                )
            }
            b"fseek" => {
                let func = "SplFileObject::fseek";
                let pos = match a.first() {
                    Some(v) => match self.spl_arg_long(func, 1, "offset", v) {
                        Some(n) => n,
                        None => return Some(Value::Null),
                    },
                    None => {
                        return self.spl_throw(
                            b"ArgumentCountError",
                            "SplFileObject::fseek() expects at least 1 argument, 0 given",
                        );
                    }
                };
                let whence = match a.get(1) {
                    Some(v) => match self.spl_arg_long(func, 2, "whence", v) {
                        Some(n) => n,
                        None => return Some(Value::Null),
                    },
                    None => 0,
                };
                let s = stream_or_throw!();
                Self::sfo_free_line(obj);
                Some(
                    self.spl_call_builtin(b"fseek", &[s, Value::Long(pos), Value::Long(whence)])
                        .unwrap_or(Value::Long(-1)),
                )
            }
            b"fgetc" => {
                let s = stream_or_throw!();
                Self::sfo_free_line(obj);
                match self.spl_call_builtin(b"fgetc", &[s]) {
                    Some(Value::String(c)) => {
                        if c.as_bytes() == b"\n" {
                            Self::sfo_add_line_num(obj, 1);
                        }
                        Some(Value::String(c))
                    }
                    _ => Some(Value::False),
                }
            }
            b"fpassthru" => {
                let s = stream_or_throw!();
                Some(
                    self.spl_call_builtin(b"fpassthru", &[s])
                        .unwrap_or(Value::False),
                )
            }
            b"fscanf" => {
                let format = match a.first() {
                    // (zpp of the method's own string $format)
                    Some(v) => match self.spl_arg_string("SplFileObject::fscanf", 1, "format", v) {
                        Some(s) => Value::String(s),
                        None => return Some(Value::Null),
                    },
                    None => {
                        return self.spl_throw(
                            b"ArgumentCountError",
                            "SplFileObject::fscanf() expects at least 1 argument, 0 given",
                        );
                    }
                };
                let _ = stream_or_throw!();
                if !self.sfo_read(obj, false, false) {
                    return Some(Value::Null);
                }
                let mut sargs = vec![prop(obj, b"__spl_cur_line"), format];
                sargs.extend_from_slice(&a[1..]);
                Some(
                    self.spl_call_builtin(b"sscanf", &sargs)
                        .unwrap_or(Value::Null),
                )
            }
            b"fwrite" => {
                let func = "SplFileObject::fwrite";
                let data = match a.first() {
                    Some(v) => match self.spl_arg_string(func, 1, "data", v) {
                        Some(s) => s,
                        None => return Some(Value::Null),
                    },
                    None => {
                        return self.spl_throw(
                            b"ArgumentCountError",
                            "SplFileObject::fwrite() expects at least 1 argument, 0 given",
                        );
                    }
                };
                let len = match a.get(1).map(|v| v.deref()) {
                    None | Some(Value::Null) | Some(Value::Undef) => None,
                    Some(v) => match self.spl_arg_long(func, 2, "length", &v) {
                        Some(n) => Some(n),
                        None => return Some(Value::Null),
                    },
                };
                let s = stream_or_throw!();
                let mut bytes = data.as_bytes().to_vec();
                if let Some(n) = len {
                    if n >= 0 {
                        bytes.truncate(n as usize);
                    } else {
                        bytes.clear();
                    }
                }
                if bytes.is_empty() {
                    return Some(Value::Long(0));
                }
                Some(
                    self.spl_call_builtin(
                        b"fwrite",
                        &[s, Value::String(PhpString::from_vec(bytes))],
                    )
                    .unwrap_or(Value::False),
                )
            }
            b"fread" => {
                let func = "SplFileObject::fread";
                let n = match a.first() {
                    Some(v) => match self.spl_arg_long(func, 1, "length", v) {
                        Some(n) => n,
                        None => return Some(Value::Null),
                    },
                    None => {
                        return self.spl_throw(
                            b"ArgumentCountError",
                            "SplFileObject::fread() expects exactly 1 argument, 0 given",
                        );
                    }
                };
                let s = stream_or_throw!();
                if n <= 0 {
                    return self.spl_throw(
                        b"ValueError",
                        "SplFileObject::fread(): Argument #1 ($length) must be greater than 0",
                    );
                }
                Some(
                    self.spl_call_builtin(b"fread", &[s, Value::Long(n)])
                        .unwrap_or(Value::False),
                )
            }
            b"fstat" => {
                let s = stream_or_throw!();
                Some(
                    self.spl_call_builtin(b"fstat", &[s])
                        .unwrap_or(Value::False),
                )
            }
            b"ftruncate" => {
                let n = match a.first() {
                    Some(v) => match self.spl_arg_long("SplFileObject::ftruncate", 1, "size", v) {
                        Some(n) => n,
                        None => return Some(Value::Null),
                    },
                    None => {
                        return self.spl_throw(
                            b"ArgumentCountError",
                            "SplFileObject::ftruncate() expects exactly 1 argument, 0 given",
                        );
                    }
                };
                let s = stream_or_throw!();
                if n < 0 {
                    return self.spl_throw(b"ValueError", "SplFileObject::ftruncate(): Argument #1 ($size) must be greater than or equal to 0");
                }
                let prev = self.warning_capture.replace(Vec::new());
                let r = self.spl_call_builtin(b"ftruncate", &[s, Value::Long(n)]);
                let captured =
                    std::mem::replace(&mut self.warning_capture, prev).unwrap_or_default();
                if !captured.is_empty() && !matches!(r, Some(Value::True)) {
                    let msg = format!("Can't truncate file {}", Self::sfo_file_name_str(obj));
                    return self.spl_throw(b"LogicException", &msg);
                }
                Some(r.unwrap_or(Value::False))
            }
            b"seek" => {
                let n = match a.first() {
                    Some(v) => match self.spl_arg_long("SplFileObject::seek", 1, "line", v) {
                        Some(n) => n,
                        None => return Some(Value::Null),
                    },
                    None => {
                        return self.spl_throw(
                            b"ArgumentCountError",
                            "SplFileObject::seek() expects exactly 1 argument, 0 given",
                        );
                    }
                };
                let _ = stream_or_throw!();
                if n < 0 {
                    return self.spl_throw(b"ValueError", "SplFileObject::seek(): Argument #1 ($line) must be greater than or equal to 0");
                }
                self.sfo_rewind(obj);
                if self.current_exception.is_some() {
                    return Some(Value::Null);
                }
                for _ in 0..n {
                    if !self.sfo_read_line(obj, true) {
                        return Some(Value::Null);
                    }
                }
                if n > 0 && Self::sfo_flags(obj) & READ_AHEAD == 0 {
                    Self::sfo_add_line_num(obj, 1);
                    Self::sfo_free_line(obj);
                }
                Some(Value::Null)
            }
            b"__tostring" => {
                let _ = stream_or_throw!();
                if !Self::sfo_has_line(obj) {
                    if !self.sfo_read_line(obj, false) {
                        return Some(Value::Null);
                    }
                }
                Some(prop(obj, b"__spl_cur_line"))
            }
            b"__debuginfo" => Some(val_arr(self.fs_debug_info(obj))),
            _ => None,
        }
    }
}

/// Native constructor handler for SplFileObject / SplTempFileObject
/// (registered in object_construct_handlers).
pub fn spl_file_object_construct_native(
    vm: &mut Vm,
    args: &[Value],
    _line: u32,
) -> Result<(), String> {
    let obj = match args.first() {
        Some(Value::Object(o)) => o.clone(),
        _ => return Ok(()),
    };
    vm.spl_push_ctor_frame(&obj, &args[1..]);
    let ok = vm.spl_file_object_construct(&obj, &args[1..]);
    vm.call_stack.pop();
    if ok {
        Ok(())
    } else {
        let msg = match &vm.current_exception {
            Some(Value::Object(e)) => e
                .borrow()
                .get_property(b"message")
                .to_php_string()
                .to_msg_string(),
            _ => String::new(),
        };
        Err(msg)
    }
}

/// Native constructor handler for SplFileInfo.
pub fn spl_file_info_construct_native(
    vm: &mut Vm,
    args: &[Value],
    _line: u32,
) -> Result<(), String> {
    let obj = match args.first() {
        Some(Value::Object(o)) => o.clone(),
        _ => return Ok(()),
    };
    let func = "SplFileInfo::__construct";
    vm.spl_push_ctor_frame(&obj, &args[1..]);
    let r = spl_file_info_construct_inner(vm, &obj, args, func);
    vm.call_stack.pop();
    r
}

fn spl_file_info_construct_inner(
    vm: &mut Vm,
    obj: &Rc<RefCell<PhpObject>>,
    args: &[Value],
    func: &str,
) -> Result<(), String> {
    let fail = |vm: &mut Vm| -> Result<(), String> {
        let msg = match &vm.current_exception {
            Some(Value::Object(e)) => e
                .borrow()
                .get_property(b"message")
                .to_php_string()
                .to_msg_string(),
            _ => String::new(),
        };
        Err(msg)
    };
    if args.len() != 2 {
        let msg = format!(
            "{}() expects exactly 1 argument, {} given",
            func,
            args.len() - 1
        );
        vm.spl_throw(b"ArgumentCountError", &msg);
        return fail(vm);
    }
    let name = match vm.spl_arg_string(func, 1, "filename", &args[1]) {
        Some(s) => s,
        None => return fail(vm),
    };
    if name.as_bytes().contains(&0) {
        let msg = format!(
            "{}(): Argument #1 ($filename) must not contain any null bytes",
            func
        );
        vm.spl_throw(b"ValueError", &msg);
        return fail(vm);
    }
    Vm::sfi_set_filename(obj, name.as_bytes());
    Ok(())
}
