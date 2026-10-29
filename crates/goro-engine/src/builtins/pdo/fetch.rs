//! Fetching (PHP manual: PDOStatement::fetch, fetchAll, fetchColumn,
//! fetchObject, setFetchMode; the PDO::FETCH_* constants), the
//! statement's iterator and `PDORow`.

use std::rc::Rc;

use goro_runtime::{Key, ObjRef, PhpArray, PhpStr, Value};

use super::driver::DrvError;
use super::driver::ParamKey;
use super::stmt::{Bound, check_init, dbh_view, stmt_error, stmt_fail, with_drv, with_st};
use crate::builtins::spl::ExtIterOp;
use crate::class::{Class, NativeHooks, ObjAccess};
use crate::func::Args;
use crate::vm::{R, Vm};

pub const FETCH_DEFAULT: i64 = 0;
pub const FETCH_LAZY: i64 = 1;
pub const FETCH_ASSOC: i64 = 2;
pub const FETCH_NUM: i64 = 3;
pub const FETCH_BOTH: i64 = 4;
pub const FETCH_OBJ: i64 = 5;
pub const FETCH_BOUND: i64 = 6;
pub const FETCH_COLUMN: i64 = 7;
pub const FETCH_CLASS: i64 = 8;
pub const FETCH_INTO: i64 = 9;
pub const FETCH_FUNC: i64 = 10;
pub const FETCH_NAMED: i64 = 11;
pub const FETCH_KEY_PAIR: i64 = 12;
pub const FETCH_GROUP: i64 = 32;
pub const FETCH_UNIQUE: i64 = 64;
pub const FETCH_CLASSTYPE: i64 = 128;
pub const FETCH_PROPS_LATE: i64 = 256;
pub const FETCH_SERIALIZE: i64 = 512;
const FLAGS_MASK: i64 = 0x3f0;

/// A statement's default fetch mode and its arguments.
#[derive(Clone, Default)]
pub struct FetchMode {
    pub mode: i64,
    pub class: Option<Rc<Class>>,
    pub ctor_args: Option<PhpArray>,
    pub into: Option<ObjRef>,
    pub column: i64,
    pub func: Option<Value>,
}

impl FetchMode {
    pub fn with_mode(mode: i64) -> FetchMode {
        FetchMode {
            mode,
            ..FetchMode::default()
        }
    }

    pub fn trace(&self, visit: &mut dyn FnMut(&Value)) {
        if let Some(a) = &self.ctor_args {
            visit(&Value::Array(a.clone()));
        }
        if let Some(o) = &self.into {
            visit(&Value::Object(o.clone()));
        }
        if let Some(f) = &self.func {
            visit(f);
        }
    }
}

fn base(mode: i64) -> i64 {
    mode & 0xf
}

/// The "must be a bitmask" and flag checks shared by the fetch methods.
fn check_mode_bits(vm: &mut Vm, fname: &str, mode: i64, argn: usize) -> R<()> {
    let pname = if argn == 2 { "$fetchMode" } else { "$mode" };
    if mode & !0x3ff != 0 || base(mode) > FETCH_KEY_PAIR {
        return vm.value_error(&format!(
            "{fname}(): Argument #{argn} ({pname}) must be a bitmask of PDO::FETCH_* constants"
        ));
    }
    let b = base(mode);
    if mode & (FETCH_CLASSTYPE | FETCH_PROPS_LATE | FETCH_SERIALIZE) != 0
        && b != FETCH_CLASS
        && b != FETCH_DEFAULT
    {
        return vm.value_error(&format!(
            "{fname}(): Argument #{argn} ({pname}) cannot use PDO::FETCH_CLASSTYPE, PDO::FETCH_PROPS_LATE, or PDO::FETCH_SERIALIZE fetch flags with a fetch mode other than PDO::FETCH_CLASS"
        ));
    }
    Ok(())
}

fn serialize_deprecation(vm: &mut Vm, fname: &str, mode: i64) -> R<()> {
    if mode & FETCH_SERIALIZE != 0 {
        vm.deprecated(&format!(
            "{fname}(): The PDO::FETCH_SERIALIZE mode is deprecated"
        ))?;
    }
    Ok(())
}

fn class_has_ctor(cls: &Class) -> bool {
    cls.find_method(b"__construct").is_some()
}

/// setFetchMode() and the fetch mode of PDO::query(): validates `mode`
/// and its arguments and makes them the statement's default. `offset` is
/// the number of arguments before the mode (1 for query()).
pub fn apply_fetch_mode(
    vm: &mut Vm,
    st: &ObjRef,
    fname: &str,
    mode: i64,
    args: &[Value],
    offset: usize,
) -> R<()> {
    let argn = offset + 1;
    check_mode_bits(vm, fname, mode, argn)?;
    if with_st(st, |s| s.fetching).unwrap_or(false) {
        return vm.throw("Error", "Cannot change default fetch mode while fetching");
    }
    let total = args.len() + 1 + offset;
    let expects = |vm: &mut Vm, how: &str, n: usize| -> R<()> {
        vm.throw(
            "ArgumentCountError",
            &format!(
                "{fname}() expects {how} {} arguments for the fetch mode provided, {total} given",
                n + offset
            ),
        )
    };
    let b = base(mode);
    serialize_deprecation(vm, fname, mode)?;
    let mut fm = FetchMode::with_mode(mode);
    match b {
        FETCH_COLUMN => {
            if args.len() != 1 {
                expects(vm, "exactly", 2)?;
            }
            fm.column = match &args[0] {
                Value::Int(i) => *i,
                other => {
                    return vm.type_error(&format!(
                        "{fname}(): Argument #{} must be of type int, {} given",
                        argn + 1,
                        Vm::type_name_given(other)
                    ));
                }
            };
        }
        FETCH_CLASS => {
            if mode & FETCH_CLASSTYPE != 0 {
                if args.len() > 1 {
                    expects(vm, "at most", 2)?;
                }
            } else {
                if args.is_empty() {
                    expects(vm, "at least", 2)?;
                }
                if args.len() > 2 {
                    expects(vm, "at most", 3)?;
                }
                let cls = match &args[0] {
                    Value::Str(s) => vm.find_class(s.as_bytes(), true)?,
                    other => {
                        return vm.type_error(&format!(
                            "{fname}(): Argument #{} must be of type string, {} given",
                            argn + 1,
                            Vm::type_name_given(other)
                        ));
                    }
                };
                let Some(cls) = cls else {
                    return vm.type_error(&format!(
                        "{fname}(): Argument #{} must be a valid class",
                        argn + 1
                    ));
                };
                let ctor = match args.get(1) {
                    None | Some(Value::Null) => None,
                    Some(Value::Array(a)) => Some(a.clone()),
                    Some(other) => {
                        return vm.type_error(&format!(
                            "{fname}(): Argument #{} must be of type ?array, {} given",
                            argn + 2,
                            Vm::type_name_given(other)
                        ));
                    }
                };
                if let Some(c) = &ctor
                    && !c.is_empty()
                    && !class_has_ctor(&cls)
                {
                    return vm.value_error(&format!(
                        "{fname}(): Argument #{} must be empty when class provided in argument #{} ($class) does not have a constructor",
                        argn + 2,
                        argn + 1
                    ));
                }
                fm.class = Some(cls);
                fm.ctor_args = ctor;
            }
        }
        FETCH_INTO => {
            if args.len() != 1 {
                expects(vm, "exactly", 2)?;
            }
            match &args[0] {
                Value::Object(o) => fm.into = Some(o.clone()),
                other => {
                    return vm.type_error(&format!(
                        "{fname}(): Argument #{} must be of type object, {} given",
                        argn + 1,
                        Vm::type_name_given(other)
                    ));
                }
            }
        }
        FETCH_FUNC => {
            return vm.value_error(&format!(
                "{fname}(): Argument #{argn} ($mode) PDO::FETCH_FUNC can only be used with PDOStatement::fetchAll()"
            ));
        }
        _ => {
            if !args.is_empty() {
                expects(vm, "exactly", 1)?;
            }
        }
    }
    if b == FETCH_DEFAULT {
        // The handle's default.
        let d = super::stmt::dbh_of(st)
            .and_then(|d| super::core::with_dbh(&d, |x| x.default_fetch))
            .unwrap_or(FETCH_BOTH);
        fm.mode = d | (mode & FLAGS_MASK);
    }
    with_st(st, |s| s.fetch = fm);
    Ok(())
}

pub fn set_fetch_mode(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    check_init(vm, &o)?;
    let mode = a.int(0);
    let args: Vec<Value> = a.vals.iter().skip(1).map(|v| v.deref()).collect();
    apply_fetch_mode(vm, &o, "PDOStatement::setFetchMode", mode, &args, 0)?;
    Ok(Value::Bool(true))
}

/// The next row in its column types (after ATTR_STRINGIFY_FETCHES and
/// ATTR_ORACLE_NULLS), with the bound columns updated; `None` at the end
/// (or before execute).
fn next_row(vm: &mut Vm, st: &ObjRef, fname: &str) -> R<Option<Vec<Value>>> {
    if !with_st(st, |s| s.executed).unwrap_or(false) {
        return Ok(None);
    }
    let r = with_drv(st, |d| d.fetch(vm)).unwrap_or(Ok(None));
    let row = match r {
        Ok(Some(r)) => r,
        Ok(None) => return Ok(None),
        Err(f) => {
            stmt_fail(vm, st, fname, f)?;
            return Ok(None);
        }
    };
    let view = dbh_view(st);
    let mut out = Vec::with_capacity(row.len());
    for v in row {
        let v = match v {
            Value::Int(_) | Value::Float(_) if view.stringify => Value::Str(vm.to_php_str(&v)?),
            Value::Bool(b) if view.stringify => Value::Str(PhpStr::from(if b { "1" } else { "" })),
            other => other,
        };
        let v = match (&v, view.oracle_nulls) {
            (Value::Str(s), 1) if s.is_empty() => Value::Null,
            (Value::Null, 2) => Value::Str(PhpStr::empty()),
            _ => v,
        };
        let _ = out.push(v);
    }
    // Bound columns (bindColumn) take every fetched row.
    let cols: Vec<Bound> = with_st(st, |s| s.cols.clone()).unwrap_or_default();
    if !cols.is_empty() {
        let names = with_st(st, |s| s.columns.clone())
            .flatten()
            .unwrap_or_default();
        for b in &cols {
            let idx = match &b.key {
                ParamKey::Pos(i) => Some(*i),
                ParamKey::Name(n) => names.iter().position(|c| c.as_bytes() == n.as_slice()),
            };
            if let Some(i) = idx
                && let Some(v) = out.get(i)
                && let Value::Ref(r) = &b.var
            {
                let v = convert_bound(vm, v, b.ptype)?;
                let old = r.set(v);
                drop(old);
            }
        }
    }
    Ok(Some(out))
}

/// A column value for a bound column of type `ptype`.
fn convert_bound(vm: &mut Vm, v: &Value, ptype: i64) -> R<Value> {
    Ok(match ptype & 0xffff {
        super::stmt::PARAM_INT if !matches!(v, Value::Null) => Value::Int(vm.to_int(v)?),
        super::stmt::PARAM_BOOL if !matches!(v, Value::Null) => Value::Bool(vm.to_bool(v)),
        _ => v.clone(),
    })
}

/// A column value as an array key (PHP's array key rules).
fn to_key(vm: &mut Vm, v: &Value) -> R<Key> {
    Ok(match v {
        Value::Int(i) => Key::Int(*i),
        Value::Str(s) => Key::from_bytes(s.as_bytes()),
        Value::Bool(b) => Key::Int(*b as i64),
        Value::Float(f) => Key::Int(*f as i64),
        Value::Null => Key::from_bytes(b""),
        other => {
            let s = vm.to_php_str(other)?;
            Key::from_bytes(s.as_bytes())
        }
    })
}

/// Writes a property as code of class `scope` would (FETCH_CLASS sets
/// private and protected properties of the class).
fn write_scoped(vm: &mut Vm, o: &ObjRef, scope: *const Class, name: &PhpStr, v: Value) -> R<()> {
    let saved = vm.ex.frames.last().map(|f| f.scope);
    if let Some(f) = vm.ex.frames.last_mut() {
        f.scope = scope;
    }
    let r = vm.write_prop(o, name, v, None);
    if let (Some(f), Some(s)) = (vm.ex.frames.last_mut(), saved) {
        f.scope = s;
    }
    r
}

fn call_ctor(vm: &mut Vm, o: &ObjRef, cls: &Rc<Class>, args: Option<&PhpArray>) -> R<()> {
    if let Some(ctor) = cls.find_method(b"__construct") {
        let c = crate::vm::calls::Callee {
            func: ctor.clone(),
            this: Some(o.clone()),
            static_class: &**cls as *const _,
            scope: ctor.scope.get(),
            closure: None,
            magic_name: None,
        };
        let args = args.cloned().unwrap_or_default();
        vm.call_callee_named(c, args)?;
    }
    Ok(())
}

/// What a fetch call builds.
struct Spec {
    mode: i64,
    class: Option<Rc<Class>>,
    ctor_args: Option<PhpArray>,
    into: Option<ObjRef>,
    column: i64,
    func: Option<Value>,
}

impl From<FetchMode> for Spec {
    fn from(f: FetchMode) -> Spec {
        Spec {
            mode: f.mode,
            class: f.class,
            ctor_args: f.ctor_args,
            into: f.into,
            column: f.column,
            func: f.func,
        }
    }
}

/// Builds the fetched value of a row by the mode (without GROUP and
/// UNIQUE, which fetchAll() handles). `Ok(None)` after an error was
/// reported.
fn build(
    vm: &mut Vm,
    st: &ObjRef,
    fname: &str,
    spec: &Spec,
    mut row: Vec<Value>,
    mut names: Vec<PhpStr>,
) -> R<Option<Value>> {
    let b = base(spec.mode);
    match b {
        FETCH_ASSOC | FETCH_NUM | FETCH_BOTH | FETCH_NAMED => {
            let mut arr = PhpArray::new();
            for (i, v) in row.into_iter().enumerate() {
                let name = names.get(i).cloned().unwrap_or_default();
                match b {
                    FETCH_NUM => {
                        arr.insert(Key::Int(i as i64), v);
                    }
                    FETCH_ASSOC => {
                        arr.insert(Key::from_bytes(name.as_bytes()), v);
                    }
                    FETCH_BOTH => {
                        arr.insert(Key::from_bytes(name.as_bytes()), v.clone());
                        // A numeric index doesn't replace a column named
                        // like it (test bug_79106_collision.phpt).
                        if arr.get_int(i as i64).is_none() {
                            arr.insert(Key::Int(i as i64), v);
                        }
                    }
                    _ => {
                        let k = Key::from_bytes(name.as_bytes());
                        match arr.get(k.as_ref()).cloned() {
                            Some(Value::Array(mut prev))
                                if names[..i].iter().filter(|n| **n == name).count() > 1 =>
                            {
                                let _ = prev.push(v);
                                arr.insert(k, Value::Array(prev));
                            }
                            Some(prev) => {
                                let list = PhpArray::from_values([prev, v]);
                                arr.insert(k, Value::Array(list));
                            }
                            None => {
                                arr.insert(k, v);
                            }
                        }
                    }
                }
            }
            Ok(Some(Value::Array(arr)))
        }
        FETCH_KEY_PAIR => {
            if row.len() != 2 {
                let e = DrvError::pdo(
                    "HY000",
                    "PDO::FETCH_KEY_PAIR fetch mode requires the result set to contain exactly 2 columns.",
                );
                stmt_error(vm, st, fname, &e)?;
                return Ok(None);
            }
            let k = to_key(vm, &row[0])?;
            let mut arr = PhpArray::new();
            arr.insert(k, row.pop().unwrap());
            Ok(Some(Value::Array(arr)))
        }
        FETCH_COLUMN => {
            let c = spec.column.max(0) as usize;
            if c >= row.len() {
                return vm.value_error("Invalid column index");
            }
            Ok(Some(row.swap_remove(c)))
        }
        FETCH_OBJ => {
            let o = vm.new_std_object()?;
            for (i, v) in row.into_iter().enumerate() {
                let name = names.get(i).cloned().unwrap_or_default();
                vm.write_prop(&o, &name, v, None)?;
            }
            Ok(Some(Value::Object(o)))
        }
        FETCH_CLASS => {
            let mut cls = spec.class.clone();
            if spec.mode & FETCH_CLASSTYPE != 0 {
                let cname = if row.is_empty() {
                    Value::Null
                } else {
                    row.remove(0)
                };
                if !names.is_empty() {
                    names.remove(0);
                }
                cls = match cname {
                    Value::Str(s) => match vm.find_class(s.as_bytes(), true)? {
                        Some(c) => Some(c),
                        None => Some(vm.core.std_class.clone().unwrap()),
                    },
                    _ => cls.or_else(|| vm.core.std_class.clone()),
                };
            }
            let Some(cls) = cls else {
                let e = DrvError::pdo("HY000", "No fetch class specified");
                stmt_error(vm, st, fname, &e)?;
                return Ok(None);
            };
            if spec.mode & FETCH_SERIALIZE != 0 {
                // The (next) column is the object's serialized form, given
                // to Serializable::unserialize() of an instance made
                // without its constructor (tests bug_44409.phpt,
                // pdo_fetch_class_with_serialize_flag.phpt).
                let ser = vm.find_class(b"Serializable", false)?;
                if !ser.is_some_and(|s| cls.is_subclass_of(&s)) {
                    let e = DrvError::pdo("HY000", "cannot unserialize class");
                    stmt_error(vm, st, fname, &e)?;
                    return Ok(None);
                }
                let o = vm.instantiate(&cls)?;
                let data = if row.is_empty() {
                    Value::Null
                } else {
                    row.remove(0)
                };
                vm.call_method(&o, b"unserialize", vec![data])?;
                return Ok(Some(Value::Object(o)));
            }
            let o = vm.instantiate(&cls)?;
            with_st(st, |s| s.fetching = true);
            let r = (|| -> R<()> {
                let late = spec.mode & FETCH_PROPS_LATE != 0;
                if late {
                    call_ctor(vm, &o, &cls, spec.ctor_args.as_ref())?;
                }
                for (i, v) in row.into_iter().enumerate() {
                    let name = names.get(i).cloned().unwrap_or_default();
                    write_scoped(vm, &o, &*cls as *const _, &name, v)?;
                }
                if !late {
                    call_ctor(vm, &o, &cls, spec.ctor_args.as_ref())?;
                }
                Ok(())
            })();
            with_st(st, |s| s.fetching = false);
            r?;
            Ok(Some(Value::Object(o)))
        }
        FETCH_INTO => {
            let Some(o) = spec.into.clone() else {
                let e = DrvError::pdo("HY000", "No fetch-into object specified.");
                stmt_error(vm, st, fname, &e)?;
                return Ok(None);
            };
            for (i, v) in row.into_iter().enumerate() {
                let name = names.get(i).cloned().unwrap_or_default();
                vm.write_prop(&o, &name, v, None)?;
            }
            Ok(Some(Value::Object(o)))
        }
        FETCH_FUNC => {
            let Some(f) = spec.func.clone() else {
                let e = DrvError::pdo("HY000", "No fetch function specified");
                stmt_error(vm, st, fname, &e)?;
                return Ok(None);
            };
            Ok(Some(vm.call_value(&f, row)?))
        }
        FETCH_BOUND => Ok(Some(Value::Bool(true))),
        FETCH_LAZY => {
            let o = lazy_row(vm, st, &row, &names)?;
            Ok(Some(Value::Object(o)))
        }
        _ => Ok(Some(Value::Bool(false))),
    }
}

/// The statement's PDORow, holding the current row (FETCH_LAZY).
fn lazy_row(vm: &mut Vm, st: &ObjRef, row: &[Value], names: &[PhpStr]) -> R<ObjRef> {
    let existing = with_st(st, |s| s.lazy.clone()).flatten();
    let o = match existing {
        Some(o) => o,
        None => {
            let cls = vm.find_class(b"PDORow", false)?.unwrap();
            let o = vm.instantiate_raw(&cls)?;
            *o.obj().native.borrow_mut() = Some(Box::new(RowObj {
                stmt: Some(st.clone()),
                names: Vec::new(),
                values: Vec::new(),
            }));
            with_st(st, |s| s.lazy = Some(o.clone()));
            o
        }
    };
    let q = vm.get_prop_raw(st, b"queryString");
    vm.set_prop_raw(&o, b"queryString", q);
    let old = with_row(&o, |r| {
        r.names = names.to_vec();
        std::mem::replace(&mut r.values, row.to_vec())
    });
    drop(old);
    Ok(o)
}

fn with_row<T>(o: &ObjRef, f: impl FnOnce(&mut RowObj) -> T) -> Option<T> {
    let mut n = o.obj().native.borrow_mut();
    let any: &mut dyn std::any::Any = &mut **n.as_mut()?;
    any.downcast_mut::<RowObj>().map(f)
}

/// A PDORow column by name or position (a numeric name is a position).
fn row_column(o: &ObjRef, key: &Value) -> Option<Value> {
    with_row(o, |r| {
        let idx = match key {
            Value::Int(i) => Some(*i as usize),
            Value::Str(s) => match Key::from_bytes(s.as_bytes()) {
                Key::Int(i) => Some(i as usize),
                Key::Str(_) => r.names.iter().position(|n| n == s),
            },
            _ => None,
        }?;
        r.values.get(idx).cloned()
    })
    .flatten()
}

/// The fetch arguments common to fetch() and the iterator.
fn fetch_one(vm: &mut Vm, st: &ObjRef, fname: &str, spec: &Spec) -> R<Value> {
    let Some(row) = next_row(vm, st, fname)? else {
        return Ok(Value::Bool(false));
    };
    let names = with_st(st, |s| s.columns.clone())
        .flatten()
        .unwrap_or_default();
    match build(vm, st, fname, spec, row, names)? {
        Some(v) => Ok(v),
        None => {
            fetch_failed(vm, st, fname)?;
            Ok(Value::Bool(false))
        }
    }
}

/// After a fetch error PDO reports the failed fetch itself too: a second
/// "SQLSTATE[HY000]: General error" (tests bug_38253.phpt, pdo_034.phpt).
fn fetch_failed(vm: &mut Vm, st: &ObjRef, fname: &str) -> R<()> {
    stmt_error(vm, st, fname, &DrvError::new("HY000", None, None))
}

pub fn fetch(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    check_init(vm, &o)?;
    let mode = a.int_or(0, 0);
    check_mode_bits(vm, "PDOStatement::fetch", mode, 1)?;
    if mode & (FETCH_GROUP | FETCH_UNIQUE) != 0 {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #1 ($mode) cannot use PDO::FETCH_GROUP or PDO::FETCH_UNIQUE with PDOStatement::fetch()",
        );
    }
    let fm = with_st(&o, |s| s.fetch.clone()).unwrap_or_default();
    let mut spec: Spec = fm.clone().into();
    if base(mode) != FETCH_DEFAULT || mode != 0 {
        spec.mode = if base(mode) == FETCH_DEFAULT {
            fm.mode | (mode & FLAGS_MASK)
        } else {
            mode
        };
        if base(spec.mode) == FETCH_FUNC {
            return vm.fthrow(
                a,
                "ValueError",
                "Argument #1 ($mode) PDO::FETCH_FUNC can only be used with PDOStatement::fetchAll()",
            );
        }
        if base(mode) == FETCH_CLASS && base(fm.mode) != FETCH_CLASS {
            spec.class = None;
            spec.ctor_args = None;
        }
    }
    serialize_deprecation(vm, "PDOStatement::fetch", spec.mode)?;
    if base(mode) == FETCH_CLASS && spec.class.is_none() && spec.mode & FETCH_CLASSTYPE == 0 {
        spec.class = vm.core.std_class.clone();
    }
    fetch_one(vm, &o, "PDOStatement::fetch", &spec)
}

pub fn fetch_object(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    check_init(vm, &o)?;
    let cname = a.opt_str(0).unwrap_or_else(|| PhpStr::from("stdClass"));
    let Some(cls) = vm.find_class(cname.as_bytes(), true)? else {
        return vm.fthrow(
            a,
            "TypeError",
            "Argument #1 ($class) must be a valid class name, ",
        );
    };
    let args = match a.get(1).map(|v| v.deref()) {
        Some(Value::Array(x)) => Some(x),
        _ => None,
    };
    if let Some(x) = &args
        && !x.is_empty()
        && !class_has_ctor(&cls)
    {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #2 ($constructorArgs) must be empty when class provided in argument #1 ($class) does not have a constructor",
        );
    }
    let spec = Spec {
        mode: FETCH_CLASS,
        class: Some(cls),
        ctor_args: args,
        into: None,
        column: 0,
        func: None,
    };
    fetch_one(vm, &o, "PDOStatement::fetchObject", &spec)
}

pub fn fetch_column(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    check_init(vm, &o)?;
    let col = a.int_or(0, 0);
    if col < 0 {
        return vm.value_error("Column index must be greater than or equal to 0");
    }
    let Some(row) = next_row(vm, &o, "PDOStatement::fetchColumn")? else {
        return Ok(Value::Bool(false));
    };
    if col as usize >= row.len() {
        return vm.value_error("Invalid column index");
    }
    Ok(row.into_iter().nth(col as usize).unwrap_or_default())
}

pub fn fetch_all(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    check_init(vm, &o)?;
    let fname = "PDOStatement::fetchAll";
    let mode = a.int_or(0, 0);
    let args: Vec<Value> = a.vals.iter().skip(1).map(|v| v.deref()).collect();
    check_mode_bits(vm, fname, mode, 1)?;
    let fm = with_st(&o, |s| s.fetch.clone()).unwrap_or_default();
    let mut spec: Spec = fm.clone().into();
    let total = args.len() + 1;
    let expects = |vm: &mut Vm, how: &str, n: usize| -> R<()> {
        vm.throw(
            "ArgumentCountError",
            &format!(
                "{fname}() expects {how} {n} arguments for the fetch mode provided, {total} given"
            ),
        )
    };
    let b = base(mode);
    if mode != 0 {
        spec.mode = if b == FETCH_DEFAULT {
            fm.mode | (mode & FLAGS_MASK)
        } else {
            mode
        };
    }
    serialize_deprecation(vm, fname, spec.mode)?;
    match base(spec.mode) {
        FETCH_CLASS if b == FETCH_CLASS => {
            if args.len() > 2 {
                expects(vm, "at most", 3)?;
            }
            if spec.mode & FETCH_CLASSTYPE != 0 && args.is_empty() {
                spec.class = None;
            } else {
                let cls = match args.first() {
                    None => vm.core.std_class.clone(),
                    Some(Value::Str(s)) => match vm.find_class(s.as_bytes(), true)? {
                        Some(c) => Some(c),
                        None => {
                            return vm.fthrow(a, "TypeError", "Argument #2 must be a valid class");
                        }
                    },
                    Some(other) => {
                        let m = format!(
                            "Argument #2 must be of type string, {} given",
                            Vm::type_name_given(other)
                        );
                        return vm.fthrow(a, "TypeError", &m);
                    }
                };
                spec.class = cls;
            }
            spec.ctor_args = match args.get(1) {
                None | Some(Value::Null) => None,
                Some(Value::Array(x)) => Some(x.clone()),
                Some(other) => {
                    let m = format!(
                        "Argument #3 must be of type ?array, {} given",
                        Vm::type_name_given(other)
                    );
                    return vm.fthrow(a, "TypeError", &m);
                }
            };
            if let (Some(c), Some(x)) = (&spec.class, &spec.ctor_args)
                && !x.is_empty()
                && !class_has_ctor(c)
            {
                return vm.fthrow(
                    a,
                    "ValueError",
                    "Argument #3 must be empty when class provided in argument #2 ($class) does not have a constructor",
                );
            }
        }
        // The default mode without its class or function: the fetch
        // reports it (test bug_38253.phpt).
        FETCH_CLASS => {}
        FETCH_FUNC if b != FETCH_FUNC => {}
        FETCH_COLUMN if b == FETCH_COLUMN => {
            if args.len() > 1 {
                expects(vm, "at most", 2)?;
            }
            spec.column = match args.first() {
                None => {
                    if spec.mode & (FETCH_GROUP | FETCH_UNIQUE) != 0 {
                        1
                    } else {
                        0
                    }
                }
                Some(Value::Int(i)) => *i,
                Some(other) => {
                    let m = format!(
                        "Argument #2 must be of type int, {} given",
                        Vm::type_name_given(other)
                    );
                    return vm.fthrow(a, "TypeError", &m);
                }
            };
        }
        FETCH_FUNC => {
            if args.len() != 1 {
                expects(vm, "exactly", 2)?;
            }
            let f = args[0].clone();
            if !vm.is_callable_value(&f, false) {
                // Only null gets the argument message; other values the
                // reason they aren't callable (test
                // pdo_fetch_function_incorrect_callables.phpt).
                if matches!(f, Value::Null) {
                    return vm.fthrow(a, "TypeError", "Argument #2 must be a callable, null given");
                }
                let why = vm.callable_error(&f);
                return vm.type_error(&why);
            }
            spec.func = Some(f);
        }
        FETCH_LAZY => {
            return vm.fthrow(
                a,
                "ValueError",
                "Argument #1 ($mode) cannot be PDO::FETCH_LAZY in PDOStatement::fetchAll()",
            );
        }
        _ => {
            if b != FETCH_DEFAULT && !args.is_empty() {
                expects(vm, "exactly", 1)?;
            }
        }
    }
    let group = spec.mode & FETCH_GROUP != 0;
    let unique = spec.mode & FETCH_UNIQUE != 0;
    let mut out = PhpArray::new();
    let names_all = with_st(&o, |s| s.columns.clone())
        .flatten()
        .unwrap_or_default();
    loop {
        let Some(mut row) = next_row(vm, &o, fname)? else {
            break;
        };
        let mut names = names_all.clone();
        let bm = base(spec.mode);
        if bm == FETCH_KEY_PAIR {
            if row.len() != 2 {
                let e = DrvError::pdo(
                    "HY000",
                    "PDO::FETCH_KEY_PAIR fetch mode requires the result set to contain exactly 2 columns.",
                );
                stmt_error(vm, &o, fname, &e)?;
                fetch_failed(vm, &o, fname)?;
                return Ok(Value::Array(PhpArray::new()));
            }
            let k = to_key(vm, &row[0])?;
            out.insert(k, row.pop().unwrap());
            continue;
        }
        if !(group || unique) {
            match build(vm, &o, fname, &spec, row, names)? {
                Some(v) => {
                    let _ = out.push(v);
                }
                None => {
                    fetch_failed(vm, &o, fname)?;
                    return Ok(Value::Array(PhpArray::new()));
                }
            }
            continue;
        }
        let key;
        let val;
        if bm == FETCH_COLUMN {
            key = to_key(vm, row.first().unwrap_or(&Value::Null))?;
            let c = spec.column.max(0) as usize;
            if c >= row.len() {
                return vm.value_error("Invalid column index");
            }
            val = row.swap_remove(c);
        } else {
            // CLASSTYPE takes the first column; the group key the next.
            let mut ct = None;
            if bm == FETCH_CLASS && spec.mode & FETCH_CLASSTYPE != 0 && !row.is_empty() {
                ct = Some(row.remove(0));
                names.remove(0);
            }
            if row.is_empty() {
                break;
            }
            key = to_key(vm, &row.remove(0))?;
            names.remove(0);
            if let Some(c) = ct {
                row.insert(0, c);
                names.insert(0, PhpStr::empty());
            }
            val = match build(vm, &o, fname, &spec, row, names)? {
                Some(v) => v,
                None => {
                    fetch_failed(vm, &o, fname)?;
                    return Ok(Value::Array(PhpArray::new()));
                }
            };
        }
        if unique {
            out.insert(key, val);
        } else {
            let mut list = match out.get(key.as_ref()) {
                Some(Value::Array(l)) => l.clone(),
                _ => PhpArray::new(),
            };
            let _ = list.push(val);
            out.insert(key, Value::Array(list));
        }
    }
    Ok(Value::Array(out))
}

// ------------------------------------------------------------ iterator

/// The step of the statement's InternalIterator: rows are fetched in the
/// default fetch mode, the first when iteration starts.
fn iter_step(vm: &mut Vm, st: &ObjRef, pos: usize, op: ExtIterOp, state: &mut Value) -> R<Value> {
    // `state` holds [fetched, current].
    let fetched = matches!(state, Value::Array(_));
    if !fetched {
        let v = iter_fetch(vm, st)?;
        *state = Value::Array(PhpArray::from_values([v]));
    }
    let cur = match state {
        Value::Array(a) => a.get_int(0).cloned().unwrap_or_default(),
        _ => Value::Bool(false),
    };
    let valid = !matches!(cur, Value::Bool(false));
    match op {
        ExtIterOp::Valid => Ok(Value::Bool(valid)),
        ExtIterOp::Current => Ok(if valid { cur } else { Value::Null }),
        ExtIterOp::Key => Ok(if valid {
            Value::Int(pos as i64)
        } else {
            Value::Null
        }),
        ExtIterOp::Next => {
            if valid {
                let v = iter_fetch(vm, st)?;
                *state = Value::Array(PhpArray::from_values([v]));
                Ok(Value::Int(pos as i64 + 1))
            } else {
                Ok(Value::Int(pos as i64))
            }
        }
    }
}

fn iter_fetch(vm: &mut Vm, st: &ObjRef) -> R<Value> {
    let fm = with_st(st, |s| s.fetch.clone()).unwrap_or_default();
    let spec: Spec = fm.into();
    fetch_one(vm, st, "PDOStatement::getIterator", &spec)
}

thread_local! {
    /// Statements whose user getIterator() is running.
    static USER_ITER: std::cell::RefCell<Vec<u32>> = const { std::cell::RefCell::new(Vec::new()) };
}

pub fn iterator_hook(vm: &mut Vm, o: &ObjRef, _by_ref: bool) -> R<ObjRef> {
    let cls = vm.class_of(o);
    let id = o.handle();
    // A user getIterator() is used by foreach, but iterating the statement
    // from inside it (IteratorIterator($this)) gets the internal iterator
    // (test pdo_014.phpt).
    let inside = USER_ITER.with(|u| u.borrow().contains(&id));
    if !inside
        && let Some(f) = cls.find_method(b"getiterator")
        && !f.is_native()
    {
        USER_ITER.with(|u| u.borrow_mut().push(id));
        let r = vm.call_method(o, b"getiterator", Vec::new());
        USER_ITER.with(|u| {
            let mut u = u.borrow_mut();
            if let Some(p) = u.iter().rposition(|&x| x == id) {
                u.remove(p);
            }
        });
        return match r? {
            Value::Object(it) => Ok(it),
            _ => vm.throw(
                "Exception",
                &format!(
                    "Objects returned by {}::getIterator() must be traversable or implement interface Iterator",
                    cls.name.to_string_lossy()
                ),
            ),
        };
    }
    check_init(vm, o)?;
    crate::builtins::spl::new_ext_iter(vm, o, iter_step)
}

pub fn get_iterator(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    check_init(vm, &o)?;
    Ok(Value::Object(crate::builtins::spl::new_ext_iter(
        vm, &o, iter_step,
    )?))
}

// ------------------------------------------------------------ PDORow

/// A PDORow: the row of the statement's last FETCH_LAZY fetch.
pub struct RowObj {
    pub stmt: Option<ObjRef>,
    pub names: Vec<PhpStr>,
    pub values: Vec<Value>,
}

impl goro_runtime::NativeData for RowObj {
    fn trace(&self, visit: &mut dyn FnMut(&Value)) {
        if let Some(s) = &self.stmt {
            visit(&Value::Object(s.clone()));
        }
        for v in &self.values {
            visit(v);
        }
    }
    fn clear(&mut self) {
        self.stmt = None;
        self.values.clear();
    }
}

pub fn row_construct(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    Err(super::core::pdo_exception(
        vm,
        "You may not create a PDORow manually",
        None,
        "",
    ))
}

/// Read/isset/empty of a column; the PDORow can't be modified (test
/// ext/pdo/tests/pdo_035.phpt).
fn row_access(found: Option<Value>, acc: &ObjAccess) -> Option<R<Value>> {
    match acc {
        ObjAccess::Read | ObjAccess::Fetch => Some(Ok(found.unwrap_or_default())),
        ObjAccess::Isset => Some(Ok(Value::Bool(
            found.is_some_and(|v| !matches!(v, Value::Null)),
        ))),
        ObjAccess::Empty => Some(Ok(Value::Bool(
            found.is_none_or(|v| !goro_runtime::convert::to_bool_quiet(&v)),
        ))),
        _ => None,
    }
}

pub fn row_hooks() -> NativeHooks {
    NativeHooks {
        dim: Some(|vm, o, key, acc| {
            let Some(key) = key else {
                return Some(vm.throw("Error", "Cannot append to PDORow offset"));
            };
            match acc {
                ObjAccess::Write(_) => {
                    return Some(vm.throw("Error", "Cannot write to PDORow offset"));
                }
                ObjAccess::Unset => return Some(vm.throw("Error", "Cannot unset PDORow offset")),
                _ => {}
            }
            let key = key.deref();
            let key = match key {
                Value::Int(_) | Value::Str(_) => key,
                other => match vm.to_php_str(&other) {
                    Ok(s) => Value::Str(s),
                    Err(e) => return Some(Err(e)),
                },
            };
            row_access(row_column(o, &key), &acc)
        }),
        prop: Some(|vm, o, name, acc| {
            match acc {
                ObjAccess::Write(_) => {
                    return Some(vm.throw("Error", "Cannot write to PDORow property"));
                }
                ObjAccess::Unset => {
                    return Some(vm.throw("Error", "Cannot unset PDORow property"));
                }
                _ => {}
            }
            row_access(row_column(o, &Value::Str(name.clone())), &acc)
        }),
        // (array) and foreach see no properties (test gh18114.phpt).
        props_for: Some(|_, _| Some(PhpArray::new())),
        debug_props: Some(|vm, o| {
            let mut a = PhpArray::new();
            a.insert(
                Key::from_bytes(b"queryString"),
                vm.get_prop_raw(o, b"queryString"),
            );
            with_row(o, |r| {
                for (n, v) in r.names.iter().zip(r.values.iter()) {
                    if n.as_bytes() == b"queryString" {
                        continue;
                    }
                    a.insert(Key::Str(n.clone()), v.clone());
                }
            });
            Some(a)
        }),
        ..Default::default()
    }
}
