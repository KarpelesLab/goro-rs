//! Variable handling: var_dump, print_r, var_export, debug_zval_dump,
//! gettype/settype, is_*, intval & co (PHP manual: Variable handling
//! functions; output formats observed on PHP 8.5).

use crate::class::{DisplayName, TypeName};
use goro_runtime::{Key, KeyRef, ObjRef, PhpArray, PhpStr, Value};

use crate::func::Args;
use crate::registry::Module;
use crate::vm::{R, Vm};

pub static MODULE: Module = Module {
    name: "var",
    functions: &[
        (
            "function var_dump(mixed $value, mixed ...$values): void",
            var_dump,
        ),
        (
            "function print_r(mixed $value, bool $return = false): string|true",
            print_r,
        ),
        (
            "function var_export(mixed $value, bool $return = false): ?string",
            var_export,
        ),
        (
            "function debug_zval_dump(mixed $value, mixed ...$values): void",
            debug_zval_dump,
        ),
        ("function gettype(mixed $value): string", gettype),
        (
            "function get_debug_type(mixed $value): string",
            get_debug_type,
        ),
        ("function settype(mixed &$var, string $type): bool", settype),
        ("function is_null(mixed $value): bool", is_null),
        ("function is_bool(mixed $value): bool", is_bool),
        ("function is_int(mixed $value): bool", is_int),
        ("function is_integer(mixed $value): bool", is_int),
        ("function is_long(mixed $value): bool", is_int),
        ("function is_float(mixed $value): bool", is_float),
        ("function is_double(mixed $value): bool", is_float),
        ("function is_string(mixed $value): bool", is_string),
        ("function is_array(mixed $value): bool", is_array),
        ("function is_object(mixed $value): bool", is_object),
        ("function is_resource(mixed $value): bool", is_resource),
        ("function is_scalar(mixed $value): bool", is_scalar),
        ("function is_numeric(mixed $value): bool", is_numeric),
        ("function is_iterable(mixed $value): bool", is_iterable),
        ("function is_countable(mixed $value): bool", is_countable),
        (
            "function is_callable(mixed $value, bool $syntax_only = false, &$callable_name = null): bool",
            is_callable,
        ),
        ("function intval(mixed $value, int $base = 10): int", intval),
        ("function floatval(mixed $value): float", floatval),
        ("function doubleval(mixed $value): float", floatval),
        ("function strval(mixed $value): string", strval),
        ("function boolval(mixed $value): bool", boolval),
        (
            "function get_resource_type($resource): string",
            get_resource_type,
        ),
        ("function get_resource_id($resource): int", get_resource_id),
    ],
    listed: false,
    ..Module::EMPTY
};

impl Vm {
    /// Formats a float as var_dump / var_export / json do
    /// (serialize_precision).
    pub fn repr_float(&self, f: f64, out: &mut Vec<u8>) {
        goro_runtime::float_fmt::format_php_repr(f, self.ini.serialize_precision, b'E', out);
    }

    // ------------------------------------------------------------ var_dump

    pub fn var_dump_into(
        &mut self,
        out: &mut Vec<u8>,
        v: &Value,
        indent: usize,
        is_ref: bool,
    ) -> R<()> {
        let pad = " ".repeat(indent);
        let amp = if is_ref { "&" } else { "" };
        match v {
            Value::Null => {
                out.extend_from_slice(format!("{}{}NULL\n", pad, amp).as_bytes());
            }
            Value::Bool(b) => {
                out.extend_from_slice(format!("{}{}bool({})\n", pad, amp, b).as_bytes())
            }
            Value::Int(i) => {
                out.extend_from_slice(format!("{}{}int({})\n", pad, amp, i).as_bytes())
            }
            Value::Float(f) => {
                out.extend_from_slice(format!("{}{}float(", pad, amp).as_bytes());
                self.repr_float(*f, out);
                out.extend_from_slice(b")\n");
            }
            Value::Str(s) => {
                out.extend_from_slice(format!("{}{}string({}) \"", pad, amp, s.len()).as_bytes());
                out.extend_from_slice(s);
                out.extend_from_slice(b"\"\n");
            }
            Value::Array(a) => {
                // An array can only contain itself through a reference.
                let id = a.data() as *const _ as usize;
                if is_ref && self.dump_stack.contains(&id) {
                    out.extend_from_slice(format!("{}*RECURSION*\n", pad).as_bytes());
                    return Ok(());
                }
                out.extend_from_slice(format!("{}{}array({}) {{\n", pad, amp, a.len()).as_bytes());
                self.dump_stack.push(id);
                for (k, x) in a.iter() {
                    match k {
                        KeyRef::Int(i) => {
                            out.extend_from_slice(format!("{}  [{}]=>\n", pad, i).as_bytes())
                        }
                        KeyRef::Str(s) => {
                            out.extend_from_slice(format!("{}  [\"", pad).as_bytes());
                            out.extend_from_slice(s);
                            out.extend_from_slice(b"\"]=>\n");
                        }
                    }
                    let (val, r, via_ref) = match x {
                        Value::Ref(rf) => (rf.get(), rf.refcount() > 1, true),
                        v => (v.clone(), false, false),
                    };
                    let res = if via_ref && !r {
                        // A reference only this array holds: still checked
                        // for recursion.
                        self.var_dump_ref_elem(out, &val, indent + 2)
                    } else {
                        self.var_dump_into(out, &val, indent + 2, r)
                    };
                    if res.is_err() {
                        self.dump_stack.pop();
                        return res;
                    }
                }
                self.dump_stack.pop();
                out.extend_from_slice(format!("{}}}\n", pad).as_bytes());
            }
            Value::Object(o) => return self.var_dump_object(out, o, indent, amp),
            Value::Resource(r) => {
                if r.is_closed() {
                    out.extend_from_slice(
                        format!("{}{}resource({}) of type (Unknown)\n", pad, amp, r.id())
                            .as_bytes(),
                    );
                } else {
                    out.extend_from_slice(
                        format!(
                            "{}{}resource({}) of type ({})\n",
                            pad,
                            amp,
                            r.id(),
                            r.res().kind.borrow().to_string_lossy()
                        )
                        .as_bytes(),
                    );
                }
            }
            Value::Ref(r) => {
                let v = r.get();
                return self.var_dump_into(out, &v, indent, is_ref);
            }
        }
        Ok(())
    }

    fn var_dump_ref_elem(&mut self, out: &mut Vec<u8>, v: &Value, indent: usize) -> R<()> {
        if let Value::Array(a) = v {
            let id = a.data() as *const _ as usize;
            if self.dump_stack.contains(&id) {
                out.extend_from_slice(format!("{}*RECURSION*\n", " ".repeat(indent)).as_bytes());
                return Ok(());
            }
        }
        self.var_dump_into(out, v, indent, false)
    }

    fn var_dump_object(
        &mut self,
        out: &mut Vec<u8>,
        o: &ObjRef,
        indent: usize,
        amp: &str,
    ) -> R<()> {
        let pad = " ".repeat(indent);
        let cls = self.class_of(o);
        if cls.is_enum() {
            let name = self.get_prop_raw(o, b"name");
            out.extend_from_slice(format!("{}{}enum({}::", pad, amp, cls.name.dname()).as_bytes());
            out.extend_from_slice(name.as_str().map(|s| s.as_bytes()).unwrap_or(b""));
            out.extend_from_slice(b")\n");
            return Ok(());
        }
        let id = o.handle() as usize | (1 << 62);
        if self.dump_stack.contains(&id) {
            out.extend_from_slice(format!("{}*RECURSION*\n", pad).as_bytes());
            return Ok(());
        }
        let mut lazy_prefix = "";
        let entries = if o.is_lazy() {
            let inst = crate::vm::lazy::with_lazy(o, |d| (d.proxy, d.instance.clone()));
            match inst {
                Some((_, Some(i))) => {
                    lazy_prefix = "lazy proxy ";
                    vec![(b"\"instance\"".to_vec(), String::new(), Some(Value::Object(i)))]
                }
                Some((proxy, None)) => {
                    lazy_prefix = if proxy { "lazy proxy " } else { "lazy ghost " };
                    self.debug_entries(o)?
                }
                None => self.debug_entries(o)?,
            }
        } else {
            self.debug_entries(o)?
        };
        let count = entries.iter().filter(|e| e.2.is_some()).count();
        out.extend_from_slice(format!("{}{}{}object(", pad, amp, lazy_prefix).as_bytes());
        out.extend_from_slice(cls.name.dname().as_bytes());
        out.extend_from_slice(format!(")#{} ({}) {{\n", o.handle(), count).as_bytes());
        self.dump_stack.push(id);
        for (label, ty, val) in entries {
            out.extend_from_slice(format!("{}  [", pad).as_bytes());
            out.extend_from_slice(&label);
            out.extend_from_slice(b"]=>\n");
            match val {
                Some(v) => {
                    let (val, r) = match &v {
                        Value::Ref(rf) => (rf.get(), rf.refcount() > 1),
                        v => (v.clone(), false),
                    };
                    if let Err(e) = self.var_dump_into(out, &val, indent + 2, r) {
                        self.dump_stack.pop();
                        return Err(e);
                    }
                }
                None => {
                    out.extend_from_slice(format!("{}  uninitialized({})\n", pad, ty).as_bytes());
                }
            }
        }
        self.dump_stack.pop();
        out.extend_from_slice(format!("{}}}\n", pad).as_bytes());
        Ok(())
    }

    /// Entries var_dump shows for an object: (label like `"name":protected`,
    /// type for uninitialized, value).
    pub fn debug_entries(&mut self, o: &ObjRef) -> R<Vec<(Vec<u8>, String, Option<Value>)>> {
        let cls = self.class_of(o);
        let mut out = Vec::new();
        let di = cls.magic.borrow().debug_info.clone();
        if let Some(f) = di {
            let r = self.call_func_this(&f, Some(o.clone()), Vec::new())?;
            match r {
                Value::Array(a) => {
                    for (k, v) in a.iter() {
                        let key = match k {
                            KeyRef::Str(s) => mangled_label(s),
                            k => dump_key(k),
                        };
                        out.push((key, String::new(), Some(v.clone())));
                    }
                    return Ok(out);
                }
                Value::Null => return Ok(out),
                _ => {
                    return Err(self.fatal("__debuginfo() must return an array"));
                }
            }
        }
        if let Some(h) = cls.native.debug_props
            && let Some(a) = h(self, o)
        {
            for (k, v) in a.iter() {
                let key = match k {
                    KeyRef::Str(s) => mangled_label(s),
                    k => dump_key(k),
                };
                out.push((key, String::new(), Some(v.clone())));
            }
            return Ok(out);
        }
        let props = cls.props.borrow().clone();
        let slots = o.obj().slots.borrow().clone();
        for p in props.iter() {
            if p.flags & (crate::class::mflags::VIRTUAL | crate::class::mflags::HIDDEN) != 0 {
                continue;
            }
            let v = &slots[p.slot as usize];
            let mut label = b"\"".to_vec();
            label.extend_from_slice(&p.name);
            label.push(b'"');
            if p.flags & crate::class::mflags::PROTECTED != 0 {
                label.extend_from_slice(b":protected");
            } else if p.flags & crate::class::mflags::PRIVATE != 0 {
                label.extend_from_slice(b":\"");
                label.extend_from_slice(unsafe { &(*p.class).name });
                label.extend_from_slice(b"\":private");
            }
            if self.is_uninit(v) {
                if self.is_unset_mark(v) || p.ty.is_none() {
                    continue;
                }
                out.push((label, p.ty.as_ref().unwrap().display(), None));
                continue;
            }
            out.push((label, String::new(), Some(v.clone())));
        }
        for (k, v) in o.obj().props.borrow().iter() {
            out.push((dump_key(k), String::new(), Some(v.clone())));
        }
        Ok(out)
    }

    // ------------------------------------------------------------ print_r

    pub fn print_r_into(&mut self, out: &mut Vec<u8>, v: &Value, indent: usize) -> R<()> {
        match v {
            Value::Array(a) => {
                let id = a.data() as *const _ as usize;
                if self.dump_stack.contains(&id) {
                    out.extend_from_slice(b"Array\n *RECURSION*");
                    return Ok(());
                }
                let pad = " ".repeat(indent);
                out.extend_from_slice(format!("Array\n{}(\n", pad).as_bytes());
                self.dump_stack.push(id);
                for (k, x) in a.iter() {
                    out.extend_from_slice(format!("{}    [", pad).as_bytes());
                    match k {
                        KeyRef::Int(i) => out.extend_from_slice(i.to_string().as_bytes()),
                        KeyRef::Str(s) => out.extend_from_slice(s),
                    }
                    out.extend_from_slice(b"] => ");
                    let x = x.deref();
                    if let Err(e) = self.print_r_into(out, &x, indent + 8) {
                        self.dump_stack.pop();
                        return Err(e);
                    }
                    out.push(b'\n');
                }
                self.dump_stack.pop();
                out.extend_from_slice(format!("{})\n", pad).as_bytes());
            }
            Value::Object(o) => {
                let cls = self.class_of(o);
                let id = o.handle() as usize | (1 << 62);
                if self.dump_stack.contains(&id) {
                    out.extend_from_slice(cls.name.dname().as_bytes());
                    out.extend_from_slice(b" Object\n *RECURSION*");
                    return Ok(());
                }
                let pad = " ".repeat(indent);
                out.extend_from_slice(cls.name.dname().as_bytes());
                out.extend_from_slice(format!(" Object\n{}(\n", pad).as_bytes());
                let entries = if cls.is_enum() {
                    let mut e = vec![(b"name".to_vec(), Some(self.get_prop_raw(o, b"name")))];
                    if cls.enum_backing.is_some() {
                        e.push((b"value".to_vec(), Some(self.get_prop_raw(o, b"value"))));
                    }
                    e
                } else {
                    self.debug_entries(o)?
                        .into_iter()
                        .filter(|e| e.2.is_some())
                        .map(|(l, _, v)| (print_r_label(&l), v))
                        .collect()
                };
                self.dump_stack.push(id);
                for (label, v) in entries {
                    out.extend_from_slice(format!("{}    [", pad).as_bytes());
                    out.extend_from_slice(&label);
                    out.extend_from_slice(b"] => ");
                    let v = v.unwrap().deref();
                    if let Err(e) = self.print_r_into(out, &v, indent + 8) {
                        self.dump_stack.pop();
                        return Err(e);
                    }
                    out.push(b'\n');
                }
                self.dump_stack.pop();
                out.extend_from_slice(format!("{})\n", pad).as_bytes());
            }
            Value::Ref(r) => {
                let v = r.get();
                return self.print_r_into(out, &v, indent);
            }
            other => self.append_to_string(out, other)?,
        }
        Ok(())
    }

    // ------------------------------------------------------------ var_export

    pub fn var_export_into(&mut self, out: &mut Vec<u8>, v: &Value, indent: usize) -> R<()> {
        let pad = " ".repeat(indent);
        match v {
            Value::Null => out.extend_from_slice(b"NULL"),
            Value::Bool(b) => out.extend_from_slice(if *b { b"true" } else { b"false" }),
            Value::Int(i) => {
                if *i == i64::MIN {
                    out.extend_from_slice(b"-9223372036854775807-1");
                } else {
                    out.extend_from_slice(i.to_string().as_bytes());
                }
            }
            Value::Float(f) => {
                let mut b = Vec::new();
                goro_runtime::float_fmt::format_php_repr(
                    *f,
                    self.ini.serialize_precision.max(-1),
                    b'E',
                    &mut b,
                );
                if f.is_finite() && !b.iter().any(|c| matches!(c, b'.' | b'E' | b'N' | b'I')) {
                    b.extend_from_slice(b".0");
                }
                out.extend_from_slice(&b);
            }
            Value::Str(s) => export_string(out, s),
            Value::Array(a) => {
                let id = a.data() as *const _ as usize;
                if self.dump_stack.contains(&id) {
                    self.warn("var_export does not handle circular references")?;
                    out.extend_from_slice(b"NULL");
                    return Ok(());
                }
                out.extend_from_slice(b"array (\n");
                self.dump_stack.push(id);
                for (k, x) in a.iter() {
                    out.extend_from_slice(format!("{}  ", pad).as_bytes());
                    match k {
                        KeyRef::Int(i) => out.extend_from_slice(i.to_string().as_bytes()),
                        KeyRef::Str(s) => export_string(out, s),
                    }
                    out.extend_from_slice(b" => ");
                    let x = x.deref();
                    if matches!(x, Value::Array(_) | Value::Object(_)) {
                        out.push(b'\n');
                        out.extend_from_slice(format!("{}  ", pad).as_bytes());
                    }
                    if let Err(e) = self.var_export_into(out, &x, indent + 2) {
                        self.dump_stack.pop();
                        return Err(e);
                    }
                    out.extend_from_slice(b",\n");
                }
                self.dump_stack.pop();
                out.extend_from_slice(format!("{})", pad).as_bytes());
            }
            Value::Object(o) => {
                let cls = self.class_of(o);
                let o = &self.lazy_realize(o)?;
                let id = o.handle() as usize | (1 << 62);
                if self.dump_stack.contains(&id) {
                    self.warn("var_export does not handle circular references")?;
                    out.extend_from_slice(b"NULL");
                    return Ok(());
                }
                if cls.is_enum() {
                    out.push(b'\\');
                    out.extend_from_slice(cls.name.dname().as_bytes());
                    out.extend_from_slice(b"::");
                    let n = self.get_prop_raw(o, b"name");
                    out.extend_from_slice(n.as_str().map(|s| s.as_bytes()).unwrap_or(b""));
                    return Ok(());
                }
                let is_std = cls.lname.as_bytes() == b"stdclass";
                if is_std {
                    out.extend_from_slice(b"(object) array(\n");
                } else {
                    out.push(b'\\');
                    out.extend_from_slice(cls.name.dname().as_bytes());
                    out.extend_from_slice(b"::__set_state(array(\n");
                }
                let props = self.object_props_array(o, true);
                self.dump_stack.push(id);
                for (k, x) in props.iter() {
                    out.extend_from_slice(format!("{}   ", pad).as_bytes());
                    match k {
                        KeyRef::Int(i) => out.extend_from_slice(i.to_string().as_bytes()),
                        KeyRef::Str(s) => {
                            // Mangled names: the bare property name.
                            let bare = match s.iter().rposition(|&c| c == 0) {
                                Some(p) if s.first() == Some(&0) => &s[p + 1..],
                                _ => &s[..],
                            };
                            export_string(out, bare);
                        }
                    }
                    out.extend_from_slice(b" => ");
                    let x = x.deref();
                    if matches!(x, Value::Array(_) | Value::Object(_)) {
                        out.push(b'\n');
                        out.extend_from_slice(format!("{}  ", pad).as_bytes());
                    }
                    if let Err(e) = self.var_export_into(out, &x, indent + 2) {
                        self.dump_stack.pop();
                        return Err(e);
                    }
                    out.extend_from_slice(b",\n");
                }
                self.dump_stack.pop();
                if is_std {
                    out.extend_from_slice(format!("{})", pad).as_bytes());
                } else {
                    out.extend_from_slice(format!("{}))", pad).as_bytes());
                }
            }
            Value::Resource(_) => {
                self.warn("var_export does not handle resources")?;
                out.extend_from_slice(b"NULL");
            }
            Value::Ref(r) => {
                let v = r.get();
                return self.var_export_into(out, &v, indent);
            }
        }
        Ok(())
    }

    // ------------------------------------------------------------ debug_zval_dump

    fn zval_dump_into(
        &mut self,
        out: &mut Vec<u8>,
        v: &Value,
        indent: usize,
        refcount: Option<usize>,
        is_ref: bool,
    ) -> R<()> {
        let pad = " ".repeat(indent);
        let amp = if is_ref {
            "reference refcount(2) {\n"
        } else {
            ""
        };
        let _ = amp;
        match v {
            Value::Str(s) => {
                out.extend_from_slice(format!("{}string({}) \"", pad, s.len()).as_bytes());
                out.extend_from_slice(s);
                if s.is_immortal() {
                    out.extend_from_slice(b"\" interned\n");
                } else {
                    out.extend_from_slice(
                        format!("\" refcount({})\n", refcount.unwrap_or(s.refcount())).as_bytes(),
                    );
                }
            }
            Value::Array(a) => {
                let rc = if a.is_empty() || a.data().is_literal() {
                    None
                } else {
                    Some(refcount.unwrap_or(a.refcount()))
                };
                let packed = if a.data().is_packed() { " packed" } else { "" };
                match rc {
                    None => out.extend_from_slice(
                        format!("{}array({}){} interned {{\n", pad, a.len(), packed).as_bytes(),
                    ),
                    Some(r) => out.extend_from_slice(
                        format!("{}array({}){} refcount({}){{\n", pad, a.len(), packed, r)
                            .as_bytes(),
                    ),
                }
                for (k, x) in a.iter() {
                    match k {
                        KeyRef::Int(i) => {
                            out.extend_from_slice(format!("{}  [{}]=>\n", pad, i).as_bytes())
                        }
                        KeyRef::Str(s) => {
                            out.extend_from_slice(format!("{}  [\"", pad).as_bytes());
                            out.extend_from_slice(s);
                            out.extend_from_slice(b"\"]=>\n");
                        }
                    }
                    match x {
                        Value::Ref(r) => {
                            out.extend_from_slice(
                                format!("{}  reference refcount({}) {{\n", pad, r.refcount())
                                    .as_bytes(),
                            );
                            let inner = r.get();
                            self.zval_dump_into(out, &inner, indent + 4, None, false)?;
                            out.extend_from_slice(format!("{}  }}\n", pad).as_bytes());
                        }
                        x => self.zval_dump_into(out, x, indent + 2, None, false)?,
                    }
                }
                out.extend_from_slice(format!("{}}}\n", pad).as_bytes());
            }
            Value::Object(o) => {
                let cls = self.class_of(o);
                let entries = self.debug_entries(o)?;
                let lazy = if self.is_lazy_uninit(o) {
                    if crate::vm::lazy::with_lazy(o, |d| d.proxy).unwrap_or(false) {
                        "lazy proxy "
                    } else {
                        "lazy ghost "
                    }
                } else {
                    ""
                };
                out.extend_from_slice(format!("{}{}object(", pad, lazy).as_bytes());
                out.extend_from_slice(cls.name.dname().as_bytes());
                out.extend_from_slice(
                    format!(
                        ")#{} ({}) refcount({}){{\n",
                        o.handle(),
                        entries.iter().filter(|e| e.2.is_some()).count(),
                        refcount.unwrap_or(o.refcount())
                    )
                    .as_bytes(),
                );
                for (label, ty, val) in entries {
                    out.extend_from_slice(format!("{}  [", pad).as_bytes());
                    out.extend_from_slice(&label);
                    out.extend_from_slice(b"]=>\n");
                    match val {
                        Some(Value::Ref(r)) => {
                            out.extend_from_slice(
                                format!("{}  reference refcount({}) {{\n", pad, r.refcount())
                                    .as_bytes(),
                            );
                            let inner = r.get();
                            self.zval_dump_into(out, &inner, indent + 4, None, false)?;
                            out.extend_from_slice(format!("{}  }}\n", pad).as_bytes());
                        }
                        Some(v) => self.zval_dump_into(out, &v, indent + 2, None, false)?,
                        None => out.extend_from_slice(
                            format!("{}  uninitialized({})\n", pad, ty).as_bytes(),
                        ),
                    }
                }
                out.extend_from_slice(format!("{}}}\n", pad).as_bytes());
            }
            Value::Ref(r) => {
                let v = r.get();
                self.zval_dump_into(out, &v, indent, None, false)?;
            }
            other => self.var_dump_into(out, other, indent, false)?,
        }
        Ok(())
    }
}

fn dump_key(k: KeyRef<'_>) -> Vec<u8> {
    match k {
        KeyRef::Int(i) => i.to_string().into_bytes(),
        KeyRef::Str(s) => {
            let mut v = b"\"".to_vec();
            v.extend_from_slice(s);
            v.push(b'"');
            v
        }
    }
}

/// A mangled property name (`\0*\0x`, `\0A\0x`) as var_dump shows it.
fn mangled_label(s: &[u8]) -> Vec<u8> {
    if s.first() == Some(&0)
        && let Some(p) = s[1..].iter().position(|&c| c == 0)
    {
        let cls = &s[1..p + 1];
        let name = &s[p + 2..];
        let mut v = b"\"".to_vec();
        v.extend_from_slice(name);
        v.push(b'"');
        if cls == b"*" {
            v.extend_from_slice(b":protected");
        } else {
            v.extend_from_slice(b":\"");
            v.extend_from_slice(cls);
            v.extend_from_slice(b"\":private");
        }
        return v;
    }
    dump_key(KeyRef::Str(&PhpStr::new(s)))
}

/// `"name":protected` → `name:protected`; `"x":"A":private` →
/// `x:A:private`.
fn print_r_label(l: &[u8]) -> Vec<u8> {
    l.iter().copied().filter(|&c| c != b'"').collect()
}

pub fn export_string(out: &mut Vec<u8>, s: &[u8]) {
    // NUL bytes are written as '' . "\0" . ''.
    out.push(b'\'');
    for &c in s {
        match c {
            b'\'' => out.extend_from_slice(b"\\'"),
            b'\\' => out.extend_from_slice(b"\\\\"),
            0 => out.extend_from_slice(b"' . \"\\0\" . '"),
            c => out.push(c),
        }
    }
    out.push(b'\'');
}

fn var_dump(vm: &mut Vm, a: &mut Args) -> R<Value> {
    for i in 0..a.len() {
        let v = a.vals[i].clone();
        let mut out = Vec::new();
        let r = vm.var_dump_into(&mut out, &v, 0, false);
        vm.dump_stack.clear();
        r?;
        vm.echo(&out);
    }
    Ok(Value::Null)
}

fn print_r(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let v = a.val(0);
    let mut out = Vec::new();
    let r = vm.print_r_into(&mut out, &v, 0);
    vm.dump_stack.clear();
    r?;
    if a.bool_or(1, false) {
        return Ok(Value::Str(PhpStr::from(out)));
    }
    vm.echo(&out);
    Ok(Value::Bool(true))
}

fn var_export(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let v = a.val(0);
    let mut out = Vec::new();
    let r = vm.var_export_into(&mut out, &v, 0);
    vm.dump_stack.clear();
    r?;
    if a.bool_or(1, false) {
        return Ok(Value::Str(PhpStr::from(out)));
    }
    vm.echo(&out);
    Ok(Value::Null)
}

fn debug_zval_dump(vm: &mut Vm, a: &mut Args) -> R<Value> {
    for i in 0..a.len() {
        let v = a.vals[i].clone();
        let mut out = Vec::new();
        // The argument copy adds one reference.
        let rc = match &v {
            Value::Array(arr) => Some(arr.refcount()),
            Value::Object(o) => Some(o.refcount()),
            Value::Str(s) if !s.is_immortal() => Some(s.refcount()),
            _ => None,
        };
        vm.zval_dump_into(&mut out, &v, 0, rc, false)?;
        vm.echo(&out);
    }
    Ok(Value::Null)
}

pub fn type_name(v: &Value) -> &'static str {
    match v {
        Value::Null => "NULL",
        Value::Bool(_) => "boolean",
        Value::Int(_) => "integer",
        Value::Float(_) => "double",
        Value::Str(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
        Value::Resource(r) => {
            if r.is_closed() {
                "resource (closed)"
            } else {
                "resource"
            }
        }
        Value::Ref(r) => type_name(&r.get()),
    }
}

fn gettype(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(Value::Str(PhpStr::intern(type_name(&a.val(0)).as_bytes())))
}

fn get_debug_type(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let v = a.val(0);
    let s = match &v {
        Value::Object(o) => {
            let c = vm.class_of(o);
            if c.has_flag(crate::class::cflags::ANON) {
                let n = c.name.as_bytes();
                let cut = n.iter().position(|&c| c == 0).unwrap_or(n.len());
                String::from_utf8_lossy(&n[..cut]).into_owned()
            } else {
                c.name.dname()
            }
        }
        v => v.debug_type(),
    };
    Ok(Value::Str(PhpStr::from(s)))
}

fn settype(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let Some(r) = a.ref_at(0) else {
        return Ok(Value::Bool(false));
    };
    let t = a.str(1).to_ascii_lowercase();
    let v = r.get();
    let nv = match &t[..] {
        b"int" | b"integer" => vm.cast(crate::bytecode::CastTo::Int, v)?,
        b"float" | b"double" => vm.cast(crate::bytecode::CastTo::Float, v)?,
        b"string" => vm.cast(crate::bytecode::CastTo::String, v)?,
        b"bool" | b"boolean" => vm.cast(crate::bytecode::CastTo::Bool, v)?,
        b"array" => vm.cast(crate::bytecode::CastTo::Array, v)?,
        b"object" => vm.cast(crate::bytecode::CastTo::Object, v)?,
        b"null" => Value::Null,
        b"resource" => return vm.value_error("Cannot convert to resource type"),
        _ => {
            return vm.value_error("settype(): Argument #2 ($type) must be a valid type");
        }
    };
    let old = r.set(nv);
    drop(old);
    Ok(Value::Bool(true))
}

fn is_null(_: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(Value::Bool(a.val(0).is_null()))
}
fn is_bool(_: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(Value::Bool(matches!(a.val(0), Value::Bool(_))))
}
fn is_int(_: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(Value::Bool(matches!(a.val(0), Value::Int(_))))
}
fn is_float(_: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(Value::Bool(matches!(a.val(0), Value::Float(_))))
}
fn is_string(_: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(Value::Bool(matches!(a.val(0), Value::Str(_))))
}
fn is_array(_: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(Value::Bool(matches!(a.val(0), Value::Array(_))))
}
fn is_object(_: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(Value::Bool(matches!(a.val(0), Value::Object(_))))
}
fn is_resource(_: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(Value::Bool(
        matches!(a.val(0), Value::Resource(r) if !r.is_closed()),
    ))
}
fn is_scalar(_: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(Value::Bool(matches!(
        a.val(0),
        Value::Bool(_) | Value::Int(_) | Value::Float(_) | Value::Str(_)
    )))
}
fn is_numeric(_: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(Value::Bool(match a.val(0) {
        Value::Int(_) | Value::Float(_) => true,
        Value::Str(s) => goro_runtime::numeric::is_numeric_str(&s),
        _ => false,
    }))
}
fn is_iterable(vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(Value::Bool(match a.val(0) {
        Value::Array(_) => true,
        Value::Object(o) => {
            let c = vm.class_of(&o);
            vm.is_traversable(&c)
        }
        _ => false,
    }))
}
fn is_countable(vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(Value::Bool(match a.val(0) {
        Value::Array(_) => true,
        Value::Object(o) => {
            let c = vm.class_of(&o);
            vm.core
                .countable
                .as_ref()
                .is_some_and(|k| c.is_subclass_of(k))
                || c.native.count.is_some()
        }
        _ => false,
    }))
}

fn is_callable(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let v = a.val(0);
    let syntax_only = a.bool_or(1, false);
    let ok = if syntax_only {
        match &v {
            Value::Str(_) => true,
            Value::Array(arr) => {
                arr.len() == 2
                    && matches!(
                        arr.get_int(0).map(|x| x.deref()),
                        Some(Value::Str(_)) | Some(Value::Object(_))
                    )
                    && matches!(arr.get_int(1).map(|x| x.deref()), Some(Value::Str(_)))
            }
            Value::Object(o) => {
                vm.class_of(o).magic.borrow().invoke.is_some() || vm.closure_data(o).is_some()
            }
            _ => false,
        }
    } else {
        let ok = vm.is_callable_value(&v, false);
        vm.callable_deprecation(&v)?;
        ok
    };
    if a.len() > 2 {
        let name = vm.callable_name(&v);
        a.set_ref(2, Value::Str(name));
    }
    Ok(Value::Bool(ok))
}

impl Vm {
    /// Whether a value is callable from the current scope.
    pub fn is_callable_value(&mut self, v: &Value, _syntax: bool) -> bool {
        let saved = self.err.deferred.take();
        let r = match v {
            Value::Str(_) | Value::Array(_) | Value::Object(_) => {
                // Resolve without autoload noise.
                let ok = self.resolve_callable_quiet(v);
                ok
            }
            _ => false,
        };
        self.err.deferred = saved;
        r
    }

    fn resolve_callable_quiet(&mut self, v: &Value) -> bool {
        match v {
            Value::Str(s) => {
                if let Some(p) = s.windows(2).position(|w| w == b"::") {
                    let c = &s[..p];
                    let m = &s[p + 2..];
                    let Ok(Some(cls)) = self.find_class_for_call(c) else {
                        return false;
                    };
                    return self.method_callable_static(&cls, m);
                }
                let name = s.strip_prefix(b"\\").unwrap_or(s);
                self.functions.contains_key(&crate::class::lower(name))
            }
            Value::Array(a) => {
                if a.len() != 2 {
                    return false;
                }
                let (Some(t), Some(m)) = (
                    a.get_int(0).map(|x| x.deref()),
                    a.get_int(1).map(|x| x.deref()),
                ) else {
                    return false;
                };
                let Value::Str(m) = m else { return false };
                match t {
                    Value::Object(o) => {
                        let cls = self.class_of(&o);
                        let mut mm = m.as_bytes();
                        let mut c = cls.clone();
                        match self.qualified_method(&cls, m.as_bytes()) {
                            Some(Ok((q, rest))) => {
                                c = q;
                                mm = rest;
                            }
                            Some(Err(_)) => return false,
                            None => {}
                        }
                        match c.find_method(&crate::class::lower(mm)) {
                            Some(f) => {
                                let scope = self.scope_ptr();
                                self.method_visible(&f, scope)
                                    && f.flags & crate::class::mflags::ABSTRACT == 0
                                    || cls.magic.borrow().call.is_some()
                            }
                            None => cls.magic.borrow().call.is_some(),
                        }
                    }
                    Value::Str(cn) => {
                        let Ok(Some(mut cls)) = self.find_class_for_call(&cn) else {
                            return false;
                        };
                        let mut m = m.as_bytes();
                        match self.qualified_method(&cls, m) {
                            Some(Ok((q, rest))) => {
                                cls = q;
                                m = rest;
                            }
                            Some(Err(_)) => return false,
                            None => {}
                        }
                        self.method_callable_static(&cls, m)
                    }
                    _ => false,
                }
            }
            Value::Object(o) => {
                self.closure_data(o).is_some() || self.class_of(o).magic.borrow().invoke.is_some()
            }
            _ => false,
        }
    }

    fn find_class_for_call(&mut self, name: &[u8]) -> R<Option<std::rc::Rc<crate::class::Class>>> {
        let l = crate::class::lower(name);
        match l.as_bytes() {
            b"self" | b"static" => {
                let p = if l.as_bytes() == b"self" {
                    self.scope_ptr()
                } else {
                    self.static_class_ptr()
                };
                Ok((!p.is_null()).then(|| self.rc_class(p)))
            }
            b"parent" => {
                let p = self.scope_ptr();
                Ok((!p.is_null())
                    .then(|| unsafe { (*p).parent.clone() })
                    .flatten())
            }
            _ => self.find_class(name, true),
        }
    }

    fn method_callable_static(&mut self, cls: &crate::class::Class, m: &[u8]) -> bool {
        match cls.find_method(&crate::class::lower(m)) {
            Some(f) => {
                let scope = self.scope_ptr();
                if !self.method_visible(&f, scope) {
                    return cls.magic.borrow().call_static.is_some();
                }
                if f.flags & crate::class::mflags::ABSTRACT != 0 {
                    return false;
                }
                if f.flags & crate::class::mflags::STATIC != 0 {
                    return true;
                }
                // Non-static: callable with a compatible $this.
                match self.this_obj() {
                    Some(t) => self.class_of(&t).is_subclass_of(cls),
                    None => false,
                }
            }
            None => cls.magic.borrow().call_static.is_some(),
        }
    }

    /// The name `is_callable` reports.
    pub fn callable_name(&mut self, v: &Value) -> PhpStr {
        match v {
            Value::Str(s) => s.clone(),
            Value::Array(a) if a.len() == 2 => {
                let t = a.get_int(0).map(|x| x.deref()).unwrap_or_default();
                let m = a.get_int(1).map(|x| x.deref()).unwrap_or_default();
                let cn = match &t {
                    Value::Object(o) => self.class_of(o).name.dname(),
                    Value::Str(s) => s.to_string_lossy(),
                    _ => return PhpStr::from("Array"),
                };
                let mn = match &m {
                    Value::Str(s) => s.to_string_lossy(),
                    _ => return PhpStr::from("Array"),
                };
                PhpStr::from(format!("{}::{}", cn, mn))
            }
            Value::Object(o) => {
                let c = self.class_of(o);
                PhpStr::from(format!("{}::__invoke", c.name.dname()))
            }
            Value::Array(_) => PhpStr::from("Array"),
            other => {
                let mut out = Vec::new();
                let _ = goro_runtime::convert::append_str(
                    &mut out,
                    other,
                    &mut goro_runtime::Diagnostics::new(),
                );
                PhpStr::from(out)
            }
        }
    }
}

fn intval(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let v = a.val(0);
    let base = a.int_or(1, 10);
    if base != 10
        && let Value::Str(s) = &v
    {
        return Ok(Value::Int(parse_int_base(s, base)));
    }
    let r = vm.cast(crate::bytecode::CastTo::Int, v)?;
    Ok(r)
}

/// `intval($s, $base)`: like C's strtol (base 0 detects 0x/0/0b prefixes).
pub fn parse_int_base(s: &[u8], base: i64) -> i64 {
    let mut i = 0;
    while i < s.len() && matches!(s[i], b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c) {
        i += 1;
    }
    let mut neg = false;
    if i < s.len() && (s[i] == b'-' || s[i] == b'+') {
        neg = s[i] == b'-';
        i += 1;
    }
    let mut base = base as u32;
    let rest = &s[i..];
    if (base == 16 || base == 0) && rest.len() >= 2 && rest[0] == b'0' && (rest[1] | 0x20) == b'x' {
        i += 2;
        base = 16;
    } else if (base == 2 || base == 0)
        && rest.len() >= 2
        && rest[0] == b'0'
        && (rest[1] | 0x20) == b'b'
    {
        i += 2;
        base = 2;
    } else if (base == 8 || base == 0)
        && rest.len() >= 2
        && rest[0] == b'0'
        && (rest[1] | 0x20) == b'o'
    {
        i += 2;
        base = 8;
    } else if base == 0 {
        base = if rest.first() == Some(&b'0') { 8 } else { 10 };
    }
    if !(2..=36).contains(&base) {
        return 0;
    }
    let mut v: i64 = 0;
    let mut overflow = false;
    while i < s.len() {
        let d = match s[i] {
            c @ b'0'..=b'9' => (c - b'0') as u32,
            c @ b'a'..=b'z' => (c - b'a') as u32 + 10,
            c @ b'A'..=b'Z' => (c - b'A') as u32 + 10,
            _ => break,
        };
        if d >= base {
            break;
        }
        match v
            .checked_mul(base as i64)
            .and_then(|x| x.checked_add(d as i64))
        {
            Some(n) => v = n,
            None => overflow = true,
        }
        i += 1;
    }
    if overflow {
        return if neg { i64::MIN } else { i64::MAX };
    }
    if neg { -v } else { v }
}

fn floatval(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let v = a.val(0);
    vm.cast(crate::bytecode::CastTo::Float, v)
}

fn strval(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let v = a.val(0);
    Ok(Value::Str(vm.to_php_str(&v)?))
}

fn boolval(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let v = a.val(0);
    Ok(Value::Bool(vm.to_bool(&v)))
}

fn get_resource_type(vm: &mut Vm, a: &mut Args) -> R<Value> {
    match a.val(0) {
        Value::Resource(r) => Ok(Value::Str(r.res().kind.borrow().clone())),
        v => vm.type_error(&format!(
            "get_resource_type(): Argument #1 ($resource) must be of type resource, {} given",
            v.tname()
        )),
    }
}

fn get_resource_id(vm: &mut Vm, a: &mut Args) -> R<Value> {
    match a.val(0) {
        Value::Resource(r) => Ok(Value::Int(r.id())),
        v => vm.type_error(&format!(
            "get_resource_id(): Argument #1 ($resource) must be of type resource, {} given",
            v.tname()
        )),
    }
}

#[allow(dead_code)]
fn key_unused(_: Key, _: PhpArray) {}
