//! The PDO core data structures (php_pdo_driver.h), the driver registry
//! (pdo.c), error reporting (pdo_dbh.c's pdo_raise_impl_error /
//! pdo_handle_error / pdo_throw_exception) and small helpers shared by the
//! PDO and PDOStatement methods.

use std::any::Any;
use std::cell::RefCell;
use std::rc::{Rc, Weak};

use goro_core::array::{ArrayKey, PhpArray};
use goro_core::object::PhpObject;
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::Vm;

pub type Obj = Rc<RefCell<PhpObject>>;
pub type Arr = Rc<RefCell<PhpArray>>;
pub type DbhRef = Rc<RefCell<Dbh>>;
pub type StmtRef = Rc<RefCell<Stmt>>;

// ── php_pdo_driver.h ──────────────────────────────────────────────────

pub const PDO_PARAM_NULL: i64 = 0;
pub const PDO_PARAM_INT: i64 = 1;
pub const PDO_PARAM_STR: i64 = 2;
pub const PDO_PARAM_LOB: i64 = 3;
pub const PDO_PARAM_STMT: i64 = 4;
pub const PDO_PARAM_BOOL: i64 = 5;
pub const PDO_PARAM_INPUT_OUTPUT: i64 = 0x8000_0000;
pub const PDO_PARAM_STR_NATL: i64 = 0x4000_0000;
pub const PDO_PARAM_STR_CHAR: i64 = 0x2000_0000;
pub const PDO_PARAM_FLAGS: i64 = 0xFFFF_0000;

/// PDO_PARAM_TYPE(x): the C enum is an int, the flags mask an unsigned int.
pub fn param_type(x: i64) -> i64 {
    ((x as i32 as u32) & !(PDO_PARAM_FLAGS as u32)) as i64
}

pub const PDO_PARAM_EVT_ALLOC: u8 = 0;
pub const PDO_PARAM_EVT_FREE: u8 = 1;
pub const PDO_PARAM_EVT_EXEC_PRE: u8 = 2;
pub const PDO_PARAM_EVT_EXEC_POST: u8 = 3;
pub const PDO_PARAM_EVT_FETCH_PRE: u8 = 4;
pub const PDO_PARAM_EVT_FETCH_POST: u8 = 5;
pub const PDO_PARAM_EVT_NORMALIZE: u8 = 6;

pub const PDO_FETCH_USE_DEFAULT: i64 = 0;
pub const PDO_FETCH_LAZY: i64 = 1;
pub const PDO_FETCH_ASSOC: i64 = 2;
pub const PDO_FETCH_NUM: i64 = 3;
pub const PDO_FETCH_BOTH: i64 = 4;
pub const PDO_FETCH_OBJ: i64 = 5;
pub const PDO_FETCH_BOUND: i64 = 6;
pub const PDO_FETCH_COLUMN: i64 = 7;
pub const PDO_FETCH_CLASS: i64 = 8;
pub const PDO_FETCH_INTO: i64 = 9;
pub const PDO_FETCH_FUNC: i64 = 10;
pub const PDO_FETCH_NAMED: i64 = 11;
pub const PDO_FETCH_KEY_PAIR: i64 = 12;

pub const PDO_FETCH_FLAGS: i64 = 0xFFFF_FFF0;
pub const PDO_FETCH_GROUP: i64 = 1 << 5;
pub const PDO_FETCH_UNIQUE: i64 = 1 << 6;
pub const PDO_FETCH_CLASSTYPE: i64 = 1 << 7;
pub const PDO_FETCH_PROPS_LATE: i64 = 1 << 8;
pub const PDO_FETCH_SERIALIZE: i64 = 1 << 9;
pub const PDO_FIRST_INVALID_FLAG: i64 = 1 << 10;

pub const PDO_FETCH_ORI_NEXT: i64 = 0;
pub const PDO_FETCH_ORI_PRIOR: i64 = 1;
pub const PDO_FETCH_ORI_FIRST: i64 = 2;
pub const PDO_FETCH_ORI_LAST: i64 = 3;
pub const PDO_FETCH_ORI_ABS: i64 = 4;
pub const PDO_FETCH_ORI_REL: i64 = 5;

pub const PDO_ATTR_AUTOCOMMIT: i64 = 0;
pub const PDO_ATTR_PREFETCH: i64 = 1;
pub const PDO_ATTR_TIMEOUT: i64 = 2;
pub const PDO_ATTR_ERRMODE: i64 = 3;
pub const PDO_ATTR_SERVER_VERSION: i64 = 4;
pub const PDO_ATTR_CLIENT_VERSION: i64 = 5;
pub const PDO_ATTR_SERVER_INFO: i64 = 6;
pub const PDO_ATTR_CONNECTION_STATUS: i64 = 7;
pub const PDO_ATTR_CASE: i64 = 8;
pub const PDO_ATTR_CURSOR_NAME: i64 = 9;
pub const PDO_ATTR_CURSOR: i64 = 10;
pub const PDO_ATTR_ORACLE_NULLS: i64 = 11;
pub const PDO_ATTR_PERSISTENT: i64 = 12;
pub const PDO_ATTR_STATEMENT_CLASS: i64 = 13;
pub const PDO_ATTR_FETCH_TABLE_NAMES: i64 = 14;
pub const PDO_ATTR_FETCH_CATALOG_NAMES: i64 = 15;
pub const PDO_ATTR_DRIVER_NAME: i64 = 16;
pub const PDO_ATTR_STRINGIFY_FETCHES: i64 = 17;
pub const PDO_ATTR_MAX_COLUMN_LEN: i64 = 18;
pub const PDO_ATTR_DEFAULT_FETCH_MODE: i64 = 19;
pub const PDO_ATTR_EMULATE_PREPARES: i64 = 20;
pub const PDO_ATTR_DEFAULT_STR_PARAM: i64 = 21;
pub const PDO_ATTR_DRIVER_SPECIFIC: i64 = 1000;

pub const PDO_CURSOR_FWDONLY: i64 = 0;
pub const PDO_CURSOR_SCROLL: i64 = 1;

pub const PDO_ERRMODE_SILENT: u8 = 0;
pub const PDO_ERRMODE_WARNING: u8 = 1;
pub const PDO_ERRMODE_EXCEPTION: u8 = 2;

pub const PDO_CASE_NATURAL: u8 = 0;
pub const PDO_CASE_UPPER: u8 = 1;
pub const PDO_CASE_LOWER: u8 = 2;

pub const PDO_NULL_NATURAL: u8 = 0;
pub const PDO_NULL_EMPTY_STRING: u8 = 1;
pub const PDO_NULL_TO_STRING: u8 = 2;

pub const PDO_ERR_NONE: &[u8] = b"00000";

pub const PDO_PLACEHOLDER_NONE: u8 = 0;
pub const PDO_PLACEHOLDER_NAMED: u8 = 1;
pub const PDO_PLACEHOLDER_POSITIONAL: u8 = 2;

// ── driver interface ──────────────────────────────────────────────────

/// What a get_attribute handler reports (1 / 0 / -1 in C).
pub enum AttrResult {
    Value(Value),
    Unsupported,
    Error,
}

/// struct pdo_dbh_methods
pub struct DbhMethods {
    pub closer: fn(&DbhRef),
    pub preparer: fn(&mut Vm, &DbhRef, &[u8], &StmtRef, Option<&Arr>) -> bool,
    pub doer: fn(&mut Vm, &DbhRef, &[u8]) -> i64,
    pub quoter: Option<fn(&mut Vm, &DbhRef, &[u8], i64) -> Option<Vec<u8>>>,
    pub begin: Option<fn(&DbhRef) -> bool>,
    pub commit: Option<fn(&DbhRef) -> bool>,
    pub rollback: Option<fn(&DbhRef) -> bool>,
    pub set_attribute: Option<fn(&mut Vm, &DbhRef, i64, &Value) -> bool>,
    pub last_id: Option<fn(&mut Vm, &DbhRef, Option<&[u8]>) -> Option<Vec<u8>>>,
    pub fetch_err: Option<fn(&mut Vm, &DbhRef, Option<&StmtRef>, &mut PhpArray)>,
    pub get_attribute: Option<fn(&mut Vm, &DbhRef, i64) -> AttrResult>,
    pub check_liveness: Option<fn(&mut Vm, &DbhRef) -> bool>,
    pub persistent_shutdown: Option<fn(&DbhRef)>,
    pub in_transaction: Option<fn(&DbhRef) -> bool>,
    pub scanner: Option<fn(&[u8], usize) -> (i32, usize)>,
}

/// struct pdo_stmt_methods
pub struct StmtMethods {
    /// Runs when the statement is freed (no VM at hand).
    pub dtor: fn(&mut Stmt),
    pub executer: fn(&mut Vm, &StmtRef) -> bool,
    pub fetcher: fn(&mut Vm, &StmtRef, i64, i64) -> bool,
    pub describer: fn(&mut Vm, &StmtRef, usize) -> bool,
    /// The column's value (NULL when the driver has none).
    pub get_col: fn(&mut Vm, &StmtRef, usize) -> Value,
    /// get_col with the type the column is fetched as (bound columns),
    /// for drivers that look at it
    pub get_col_typed: Option<fn(&mut Vm, &StmtRef, usize, Option<i64>) -> Value>,
    pub param_hook: Option<fn(&mut Vm, &StmtRef, &mut BoundParam, u8) -> bool>,
    pub set_attribute: Option<fn(&mut Vm, &StmtRef, i64, &Value) -> bool>,
    pub get_attribute: Option<fn(&mut Vm, &StmtRef, i64) -> AttrResult>,
    pub get_column_meta: Option<fn(&mut Vm, &StmtRef, i64) -> Option<PhpArray>>,
    pub next_rowset: Option<fn(&mut Vm, &StmtRef) -> bool>,
    pub cursor_closer: Option<fn(&mut Vm, &StmtRef) -> bool>,
}

/// pdo_driver_t
pub struct Driver {
    pub name: &'static str,
    pub factory: fn(&mut Vm, &DbhRef, Option<&Arr>) -> bool,
}

// ── pdo_dbh_t ─────────────────────────────────────────────────────────

pub struct Dbh {
    pub methods: Option<&'static DbhMethods>,
    pub driver_data: Option<Box<dyn Any>>,
    pub username: Option<Vec<u8>>,
    pub password: Option<Vec<u8>>,
    pub is_persistent: bool,
    pub auto_commit: bool,
    pub is_closed: bool,
    pub alloc_own_columns: bool,
    pub in_txn: bool,
    pub stringify: bool,
    pub skip_param_evt: u8,
    pub error_mode: u8,
    pub oracle_nulls: u8,
    pub native_case: u8,
    pub desired_case: u8,
    pub max_escaped_char_length: u8,
    pub data_source: Option<Vec<u8>>,
    pub error_code: Vec<u8>,
    pub default_fetch_type: u16,
    pub persistent_id: Option<Vec<u8>>,
    pub refcount: u32,
    pub driver: Option<&'static Driver>,
    /// Declared name of the statement class (def_stmt_ce)
    pub def_stmt_ce: Vec<u8>,
    pub def_stmt_ctor_args: Option<Value>,
    /// The statement of a failed PDO::query() (query_stmt / query_stmt_obj)
    pub query_stmt: Option<(StmtRef, Obj)>,
}

impl Dbh {
    pub fn new() -> Dbh {
        Dbh {
            methods: None,
            driver_data: None,
            username: None,
            password: None,
            is_persistent: false,
            auto_commit: false,
            is_closed: false,
            alloc_own_columns: false,
            in_txn: false,
            stringify: false,
            skip_param_evt: 0,
            error_mode: 0,
            oracle_nulls: 0,
            native_case: 0,
            desired_case: 0,
            max_escaped_char_length: 0,
            data_source: None,
            error_code: Vec::new(),
            default_fetch_type: 0,
            persistent_id: None,
            refcount: 0,
            driver: None,
            def_stmt_ce: b"PDOStatement".to_vec(),
            def_stmt_ctor_args: None,
            query_stmt: None,
        }
    }

    pub fn has_error(&self) -> bool {
        self.error_code.as_slice() != PDO_ERR_NONE
    }
}

impl Default for Dbh {
    fn default() -> Self {
        Dbh::new()
    }
}

// ── pdo_stmt_t ────────────────────────────────────────────────────────

/// struct pdo_column_data
#[derive(Clone, Debug, Default)]
pub struct Column {
    pub name: Option<Vec<u8>>,
    pub maxlen: usize,
    pub precision: u64,
}

impl Column {
    pub fn name(&self) -> &[u8] {
        self.name.as_deref().unwrap_or(b"")
    }
}

/// struct pdo_bound_param_data
#[derive(Clone, Debug)]
pub struct BoundParam {
    /// The variable itself (a Value::Reference when bound by reference)
    pub parameter: Value,
    pub driver_params: Value,
    pub paramno: i64,
    pub name: Option<Vec<u8>>,
    pub max_value_len: i64,
    pub param_type: i64,
    pub is_param: bool,
}

impl BoundParam {
    pub fn new() -> BoundParam {
        BoundParam {
            parameter: Value::Undef,
            driver_params: Value::Undef,
            paramno: 0,
            name: None,
            max_value_len: 0,
            param_type: 0,
            is_param: false,
        }
    }

    /// The value (dereferenced)
    pub fn value(&self) -> Value {
        self.parameter.deref()
    }

    /// Replace the value, through the reference when bound by reference.
    pub fn set_value(&mut self, v: Value) {
        match &self.parameter {
            Value::Reference(r) => *r.borrow_mut() = v,
            _ => self.parameter = v,
        }
    }
}

impl Default for BoundParam {
    fn default() -> Self {
        BoundParam::new()
    }
}

/// A HashTable key
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HKey {
    Int(i64),
    Str(Vec<u8>),
}

/// An insertion-ordered hash (zend_hash_update / index_update semantics).
#[derive(Clone, Debug)]
pub struct Table<T> {
    pub entries: Vec<(HKey, T)>,
}

impl<T> Table<T> {
    pub fn new() -> Self {
        Table {
            entries: Vec::new(),
        }
    }
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    pub fn get(&self, k: &HKey) -> Option<&T> {
        self.entries.iter().find(|(x, _)| x == k).map(|(_, v)| v)
    }
    pub fn get_mut(&mut self, k: &HKey) -> Option<&mut T> {
        self.entries
            .iter_mut()
            .find(|(x, _)| x == k)
            .map(|(_, v)| v)
    }
    pub fn update(&mut self, k: HKey, v: T) {
        match self.entries.iter_mut().find(|(x, _)| *x == k) {
            Some(e) => e.1 = v,
            None => self.entries.push((k, v)),
        }
    }
    pub fn remove(&mut self, k: &HKey) -> Option<T> {
        let i = self.entries.iter().position(|(x, _)| x == k)?;
        Some(self.entries.remove(i).1)
    }
}

impl<T> Default for Table<T> {
    fn default() -> Self {
        Table::new()
    }
}

/// The fetch-mode union of pdo_stmt_t
#[derive(Clone, Default)]
pub struct FetchData {
    pub column: i64,
    /// Declared name of the class (fetch.cls.ce)
    pub cls_ce: Option<Vec<u8>>,
    pub cls_ctor_args: Option<Arr>,
    pub func: Option<Value>,
    pub into: Option<Obj>,
}

pub struct Stmt {
    pub methods: Option<&'static StmtMethods>,
    pub driver_data: Option<Box<dyn Any>>,
    pub error_code: Vec<u8>,
    pub executed: bool,
    pub in_fetch: bool,
    pub supports_placeholders: u8,
    pub bound_params: Option<Table<BoundParam>>,
    pub bound_param_map: Option<Table<Vec<u8>>>,
    pub bound_columns: Option<Table<BoundParam>>,
    pub columns: Option<Vec<Column>>,
    pub column_count: i32,
    pub default_fetch_type: i64,
    pub fetch: FetchData,
    pub lazy_object_ref: Option<Weak<RefCell<PhpObject>>>,
    pub dbh: Option<DbhRef>,
    pub database_object_handle: Option<Obj>,
    pub row_count: i64,
    pub query_string: Option<Vec<u8>>,
    pub active_query_string: Option<Vec<u8>>,
    /// active_query_string is the query string itself (same zend_string)
    pub active_is_query: bool,
    pub named_rewrite_template: Option<&'static str>,
    /// The PDOStatement object (for PDORow and the iterator)
    pub self_obj: Option<Weak<RefCell<PhpObject>>>,
}

impl Stmt {
    pub fn new() -> Stmt {
        Stmt {
            methods: None,
            driver_data: None,
            error_code: Vec::new(),
            executed: false,
            in_fetch: false,
            supports_placeholders: 0,
            bound_params: None,
            bound_param_map: None,
            bound_columns: None,
            columns: None,
            column_count: 0,
            default_fetch_type: 0,
            fetch: FetchData::default(),
            lazy_object_ref: None,
            dbh: None,
            database_object_handle: None,
            row_count: 0,
            query_string: None,
            active_query_string: None,
            active_is_query: false,
            named_rewrite_template: None,
            self_obj: None,
        }
    }

    /// PDO_STMT_CLEAR_ERR()
    pub fn clear_err(&mut self) {
        self.error_code = PDO_ERR_NONE.to_vec();
    }

    pub fn has_error(&self) -> bool {
        self.error_code.as_slice() != PDO_ERR_NONE
    }

    pub fn query(&self) -> &[u8] {
        self.query_string.as_deref().unwrap_or(b"")
    }
}

impl Default for Stmt {
    fn default() -> Self {
        Stmt::new()
    }
}

/// PDO_DBH_CLEAR_ERR() (the released query statement is dropped once the
/// handle is no longer borrowed)
pub fn dbh_clear_err(dbh: &DbhRef) {
    let old = {
        let mut d = dbh.borrow_mut();
        d.error_code = PDO_ERR_NONE.to_vec();
        d.query_stmt.take()
    };
    drop(old);
}

/// strcpy() of a SQLSTATE into an error_code buffer (5 characters).
pub fn sqlstate_buf(s: &[u8]) -> Vec<u8> {
    let s = match s.iter().position(|&c| c == 0) {
        Some(p) => &s[..p],
        None => s,
    };
    s[..s.len().min(5)].to_vec()
}

// ── driver registry (pdo.c) ───────────────────────────────────────────

thread_local! {
    static DRIVERS: RefCell<Vec<&'static Driver>> = const { RefCell::new(Vec::new()) };
    /// pdo_driver_specific_ce_hash: driver name -> lowercase class name
    static DRIVER_CES: RefCell<Vec<(&'static str, Vec<u8>)>> = const { RefCell::new(Vec::new()) };
}

/// php_pdo_register_driver
pub fn register_driver(d: &'static Driver) {
    DRIVERS.with(|r| {
        let mut r = r.borrow_mut();
        if !r.iter().any(|x| x.name == d.name) {
            r.push(d);
        }
    });
}

/// php_pdo_register_driver_specific_ce
pub fn register_driver_specific_ce(d: &'static Driver, class_lower: &[u8]) {
    DRIVER_CES.with(|r| {
        let mut r = r.borrow_mut();
        if !r.iter().any(|(n, _)| *n == d.name) {
            r.push((d.name, class_lower.to_vec()));
        }
    });
}

pub fn drivers() -> Vec<&'static Driver> {
    DRIVERS.with(|r| r.borrow().clone())
}

pub fn driver_specific_ces() -> Vec<(&'static str, Vec<u8>)> {
    DRIVER_CES.with(|r| r.borrow().clone())
}

/// pdo_find_driver
pub fn find_driver(name: &[u8]) -> Option<&'static Driver> {
    DRIVERS.with(|r| {
        r.borrow()
            .iter()
            .find(|d| d.name.as_bytes() == name)
            .copied()
    })
}

/// php_pdo_parse_data_source: `vars` are (option name, default); returns
/// the parsed values and the number of matches.
pub fn parse_data_source(data_source: &[u8], vars: &mut [(&str, Option<Vec<u8>>)]) -> usize {
    let len = data_source.len();
    let mut i = 0usize;
    let mut optstart = 0usize;
    let mut n_matches = 0;
    while i < len {
        if data_source[i] == 0 {
            break;
        }
        if data_source[i] != b'=' {
            i += 1;
            continue;
        }
        i += 1;
        let valstart = i;
        let mut semi: Option<usize> = None;
        let mut n_semicolumns = 0;
        while i < len {
            if data_source[i] == 0 {
                semi = Some(i);
                i += 1;
                break;
            }
            if data_source[i] == b';' {
                if i + 1 >= len || data_source[i + 1] != b';' {
                    semi = Some(i);
                    i += 1;
                    break;
                } else {
                    n_semicolumns += 1;
                    i += 2;
                    continue;
                }
            }
            i += 1;
        }
        let semi = semi.unwrap_or(i);
        let nlen = valstart - optstart - 1;
        let optname = &data_source[optstart..optstart + nlen];
        for v in vars.iter_mut() {
            if v.0.as_bytes() == optname {
                let raw = &data_source[valstart..semi.max(valstart)];
                let val = if n_semicolumns == 0 {
                    raw.to_vec()
                } else {
                    let mut out = Vec::new();
                    let mut k = 0;
                    while k < raw.len() && raw[k] != 0 {
                        out.push(raw[k]);
                        if raw[k] == b';' {
                            k += 2;
                        } else {
                            k += 1;
                        }
                    }
                    out
                };
                // estrndup stops at the first NUL
                let val = match val.iter().position(|&c| c == 0) {
                    Some(p) => val[..p].to_vec(),
                    None => val,
                };
                v.1 = Some(val);
                n_matches += 1;
                break;
            }
        }
        while i < len && data_source[i].is_ascii_whitespace() {
            i += 1;
        }
        optstart = i;
    }
    n_matches
}

// ── attribute helpers ─────────────────────────────────────────────────

/// zval_get_long
pub fn zval_get_long(v: &Value) -> i64 {
    v.deref().to_long()
}

/// pdo_attr_lval
pub fn attr_lval(options: Option<&Arr>, option: i64, defval: i64) -> i64 {
    match options.and_then(|o| o.borrow().get_int(option).cloned()) {
        Some(v) => zval_get_long(&v),
        None => defval,
    }
}

/// pdo_attr_strval: Err when the conversion threw.
pub fn attr_strval(vm: &mut Vm, options: Option<&Arr>, option: i64) -> Result<Option<Vec<u8>>, ()> {
    match options.and_then(|o| o.borrow().get_int(option).cloned()) {
        Some(v) => match vm.value_to_string_checked(&v.deref()) {
            Ok(s) => Ok(Some(s.as_bytes().to_vec())),
            Err(_) => Err(()),
        },
        None => Ok(None),
    }
}

pub fn throw(vm: &mut Vm, class: &[u8], msg: &str) {
    let e = vm.create_exception(class, msg, vm.current_line);
    vm.current_exception = Some(e);
}

/// zend_argument_value_error / zend_argument_type_error for an argument
/// number of the active method (variadic positions have no name).
pub fn arg_error(vm: &mut Vm, class: &[u8], argn: u32, msg: &str) {
    let fname = active_function_name(vm);
    let mut parts = fname.splitn(2, "::");
    let c = parts.next().unwrap_or("");
    let m = parts.next();
    let sig = match m {
        Some(m) => goro_core::stubs::find_method(c.as_bytes(), m.as_bytes()).map(|(_, f)| f),
        None => goro_core::stubs::find_function(c.as_bytes()),
    };
    let pname = sig.and_then(|f| {
        let a = f.args.get(argn as usize - 1)?;
        if a.flags & goro_core::stubs::A_VARIADIC != 0 {
            None
        } else {
            Some(a.name)
        }
    });
    let m = match pname {
        Some(p) => format!("{}(): Argument #{} (${}) {}", fname, argn, p, msg),
        None => format!("{}(): Argument #{} {}", fname, argn, msg),
    };
    throw(vm, class, &m);
}

/// zend_argument_count_error
pub fn count_error(vm: &mut Vm, msg: &str) {
    throw(vm, b"ArgumentCountError", msg);
}

/// is_numeric_str_function(): Some(long) when the string is an integer
/// numeric string.
pub fn numeric_str_long(s: &[u8]) -> Option<i64> {
    match Vm::value_to_array_key(Value::String(PhpString::from_bytes(s))) {
        ArrayKey::Int(n) => Some(n),
        _ => {
            let t = String::from_utf8_lossy(s);
            let t = t.trim_start_matches([' ', '\t', '\n', '\r', '\x0b', '\x0c']);
            let t = t.trim_end_matches([' ', '\t', '\n', '\r', '\x0b', '\x0c']);
            let digits = t.strip_prefix(['+', '-']).unwrap_or(t);
            if !digits.is_empty() && digits.bytes().all(|c| c.is_ascii_digit()) {
                t.parse::<i64>().ok()
            } else {
                None
            }
        }
    }
}

/// pdo_get_long_param
pub fn get_long_param(vm: &mut Vm, value: &Value) -> Option<i64> {
    let v = value.deref();
    match &v {
        Value::Long(n) => Some(*n),
        Value::True => Some(1),
        Value::False => Some(0),
        Value::String(s) => match numeric_str_long(s.as_bytes()) {
            Some(n) => Some(n),
            None => {
                type_error_attr(vm, "int", &v);
                None
            }
        },
        _ => {
            type_error_attr(vm, "int", &v);
            None
        }
    }
}

/// pdo_get_bool_param
pub fn get_bool_param(vm: &mut Vm, value: &Value) -> Option<bool> {
    let v = value.deref();
    match &v {
        Value::True => Some(true),
        Value::False => Some(false),
        Value::Long(n) => Some(*n != 0),
        _ => {
            type_error_attr(vm, "bool", &v);
            None
        }
    }
}

fn type_error_attr(vm: &mut Vm, ty: &str, v: &Value) {
    let m = format!(
        "Attribute value must be of type {} for selected attribute, {} given",
        ty,
        Vm::value_type_name(v)
    );
    throw(vm, b"TypeError", &m);
}

// ── diagnostics ───────────────────────────────────────────────────────

thread_local! {
    /// The internal function being run (for php_error_docref's prefix).
    static ACTIVE_FN: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

pub fn push_active(name: &str) {
    ACTIVE_FN.with(|a| a.borrow_mut().push(name.to_string()));
}

pub fn pop_active() {
    ACTIVE_FN.with(|a| {
        a.borrow_mut().pop();
    });
}

/// get_active_function_or_method_name()
pub fn active_function_name(vm: &Vm) -> String {
    if let Some(n) = ACTIVE_FN.with(|a| a.borrow().last().cloned()) {
        return n;
    }
    match vm.call_stack.last() {
        Some(f) if !f.0.is_empty() => f.0.replace("->", "::"),
        _ => "main".to_string(),
    }
}

/// php_error_docref(NULL, E_WARNING, ...)
pub fn docref_warning(vm: &mut Vm, msg: &str) {
    let m = format!("{}(): {}", active_function_name(vm), msg);
    vm.emit_warning(&m);
}

/// php_error_docref(NULL, E_DEPRECATED, ...)
pub fn docref_deprecated(vm: &mut Vm, msg: &str) {
    let m = format!("{}(): {}", active_function_name(vm), msg);
    vm.emit_deprecated(&m);
}

/// Emit the diagnostics mysqlnd queued.
pub fn flush_mysqlnd(vm: &mut Vm) {
    use goro_ext_mysqli::mysqlnd::{E_DEPRECATED, E_NOTICE};
    // (and what libpq queued for the pgsql driver)
    crate::pgsql::flush(vm);
    for w in goro_ext_mysqli::mysqlnd::take_warnings() {
        let msg = if w.docref {
            format!(
                "{}({}): {}",
                active_function_name(vm),
                w.param.unwrap_or_default(),
                w.msg
            )
        } else {
            w.msg
        };
        match w.level {
            E_NOTICE => {
                let line = vm.current_line;
                vm.emit_notice_at(&msg, line)
            }
            E_DEPRECATED => vm.emit_deprecated(&msg),
            _ => vm.emit_warning(&msg),
        }
    }
}

fn vstr(s: impl AsRef<[u8]>) -> Value {
    Value::String(PhpString::from_bytes(s.as_ref()))
}

/// A new PDOException with message, code and (optionally) errorInfo.
fn pdo_exception(vm: &mut Vm, msg: &[u8], code: Value, info: Option<PhpArray>) -> Value {
    let e = vm.create_exception(b"PDOException", "", vm.current_line);
    if let Value::Object(o) = &e {
        let mut ob = o.borrow_mut();
        ob.set_property(b"message".to_vec(), vstr(msg));
        ob.set_property(b"code".to_vec(), code);
        if let Some(i) = info {
            ob.set_property(
                b"errorInfo".to_vec(),
                Value::Array(Rc::new(RefCell::new(i))),
            );
        }
    }
    e
}

/// zend_throw_exception(pdo_exception_ce, msg, 0)
pub fn throw_pdo(vm: &mut Vm, msg: &str) {
    let e = pdo_exception(vm, msg.as_bytes(), Value::Long(0), None);
    vm.current_exception = Some(e);
}

/// pdo_throw_exception
pub fn throw_exception(vm: &mut Vm, driver_errcode: u32, driver_errmsg: &[u8], sqlstate: &[u8]) {
    let mut info = PhpArray::new();
    info.push(vstr(sqlstate));
    info.push(Value::Long(driver_errcode as i64));
    info.push(vstr(driver_errmsg));
    let mut msg = format!(
        "SQLSTATE[{}] [{}] ",
        String::from_utf8_lossy(sqlstate),
        driver_errcode
    )
    .into_bytes();
    msg.extend_from_slice(driver_errmsg);
    let e = pdo_exception(vm, &msg, Value::Long(driver_errcode as i64), Some(info));
    vm.current_exception = Some(e);
}

/// pdo_raise_impl_error
pub fn raise_impl_error(
    vm: &mut Vm,
    dbh: &DbhRef,
    stmt: Option<&StmtRef>,
    sqlstate: &str,
    supp: Option<&str>,
) {
    let state = sqlstate.as_bytes().to_vec();
    match stmt {
        Some(s) => s.borrow_mut().error_code = state.clone(),
        None => dbh.borrow_mut().error_code = state.clone(),
    }
    let desc = crate::sqlstate::state_to_description(&state).unwrap_or("<<Unknown error>>");
    let message = match supp {
        Some(s) => format!("SQLSTATE[{}]: {}: {}", sqlstate, desc, s),
        None => format!("SQLSTATE[{}]: {}", sqlstate, desc),
    };
    let mode = dbh.borrow().error_mode;
    if mode != PDO_ERRMODE_EXCEPTION {
        docref_warning(vm, &message);
    } else {
        let mut info = PhpArray::new();
        info.push(vstr(&state));
        info.push(Value::Long(0));
        let e = pdo_exception(vm, message.as_bytes(), vstr(&state), Some(info));
        vm.current_exception = Some(e);
    }
}

/// pdo_handle_error
pub fn handle_error(vm: &mut Vm, dbh: &DbhRef, stmt: Option<&StmtRef>) {
    let (mode, methods) = {
        let d = dbh.borrow();
        (d.error_mode, d.methods)
    };
    if mode == PDO_ERRMODE_SILENT {
        return;
    }
    let err = match stmt {
        Some(s) => s.borrow().error_code.clone(),
        None => dbh.borrow().error_code.clone(),
    };
    let desc = crate::sqlstate::state_to_description(&err).unwrap_or("<<Unknown error>>");
    let mut info: Option<PhpArray> = None;
    let mut native_code = 0i64;
    let mut supp: Option<Vec<u8>> = None;
    if let Some(fe) = methods.and_then(|m| m.fetch_err) {
        let mut a = PhpArray::new();
        a.push(vstr(&err));
        fe(vm, dbh, stmt, &mut a);
        if let Some(Value::Long(n)) = a.get_int(1) {
            native_code = *n;
        }
        if let Some(v) = a.get_int(2) {
            supp = Some(v.to_php_string().as_bytes().to_vec());
        }
        info = Some(a);
    }
    let state = String::from_utf8_lossy(&err).into_owned();
    let message: Vec<u8> = match (&supp, native_code) {
        (Some(s), n) if n != 0 => {
            let mut m = format!("SQLSTATE[{}]: {}: {} ", state, desc, n).into_bytes();
            m.extend_from_slice(s);
            m
        }
        (Some(s), _) => {
            let mut m = format!("SQLSTATE[{}]: {}: ", state, desc).into_bytes();
            m.extend_from_slice(s);
            m
        }
        (None, _) => format!("SQLSTATE[{}]: {}", state, desc).into_bytes(),
    };
    if mode == PDO_ERRMODE_WARNING {
        docref_warning(vm, &String::from_utf8_lossy(&message));
    } else if vm.current_exception.is_none() {
        let e = pdo_exception(vm, &message, vstr(&err), info);
        vm.current_exception = Some(e);
    }
}

/// PDO_HANDLE_DBH_ERR()
pub fn handle_dbh_err(vm: &mut Vm, dbh: &DbhRef) {
    if dbh.borrow().has_error() {
        handle_error(vm, dbh, None);
    }
}

/// PDO_HANDLE_STMT_ERR()
pub fn handle_stmt_err(vm: &mut Vm, stmt: &StmtRef) {
    if stmt.borrow().has_error() {
        let dbh = stmt.borrow().dbh.clone();
        if let Some(dbh) = dbh {
            handle_error(vm, &dbh, Some(stmt));
        }
    }
}

// ── running PHP code in a class scope ─────────────────────────────────

thread_local! {
    static SNIPPETS: RefCell<std::collections::HashMap<&'static str, Rc<goro_core::opcode::OpArray>>> =
        RefCell::new(std::collections::HashMap::new());
}

fn compile_snippet(src: &'static str) -> Option<Rc<goro_core::opcode::OpArray>> {
    if let Some(op) = SNIPPETS.with(|s| s.borrow().get(src).cloned()) {
        return Some(op);
    }
    let source = format!("<?php {}", src).into_bytes();
    let mut lexer = goro_parser::Lexer::new(&source);
    let tokens = lexer.tokenize();
    let mut parser = goro_parser::Parser::new(tokens);
    let program = parser.parse().ok()?;
    let compiler = goro_core::compiler::Compiler::new();
    let (op_array, _, _) = compiler.compile(&program).ok()?;
    let op = Rc::new(op_array);
    SNIPPETS.with(|s| s.borrow_mut().insert(src, op.clone()));
    Some(op)
}

/// Run a PHP snippet with the given variables as if it were code of the
/// class `scope` (lowercase; empty for the global scope), with errors
/// reported at the current line. None when it threw.
pub fn run_in_scope(
    vm: &mut Vm,
    src: &'static str,
    vars: &[(&str, Value)],
    scope: &[u8],
) -> Option<Value> {
    let op = compile_snippet(src)?;
    let mut op_array = (*op).clone();
    let line = vm.current_line;
    for o in op_array.ops.iter_mut() {
        o.line = line;
    }
    op_array.filename = vm.current_file.clone().into_bytes();
    let mut cvs = vec![Value::Undef; op_array.cv_names.len()];
    for (name, val) in vars {
        if let Some(i) = op_array
            .cv_names
            .iter()
            .position(|n| n.as_slice() == name.as_bytes())
        {
            cvs[i] = val.clone();
        }
    }
    // An empty scope keeps the scope of the calling code.
    let scoped = !scope.is_empty();
    if scoped {
        vm.called_class_stack.push(scope.to_vec());
        vm.push_class_scope(scope.to_vec());
    }
    let was_global = std::mem::replace(&mut vm.is_global_scope, false);
    let r = vm.execute_op_array_pub(&op_array, cvs);
    vm.is_global_scope = was_global;
    if scoped {
        vm.called_class_stack.pop();
        vm.pop_class_scope();
    }
    vm.current_line = line;
    if vm.current_exception.is_some() {
        return None;
    }
    r.ok()
}

/// zend_update_property_ex(scope, obj, name, value)
pub fn update_property(vm: &mut Vm, scope: &[u8], obj: &Obj, name: &[u8], value: Value) -> bool {
    // An empty scope is the scope of the calling code.
    let eff_scope: Option<Vec<u8>> = if scope.is_empty() {
        vm.current_class_scope().map(|s| s.to_ascii_lowercase())
    } else {
        Some(scope.to_vec())
    };
    let cl = obj.borrow().class_name.to_ascii_lowercase();
    if let Some(vis) = inaccessible_property(vm, &cl, eff_scope.as_deref(), name) {
        let has_set = vm
            .classes
            .get(&cl)
            .is_some_and(|c| c.get_method(b"__set").is_some());
        if !has_set {
            let m = format!(
                "Cannot access {} property {}::${}",
                vis,
                goro_core::value::display_class_name(&obj.borrow().class_name),
                String::from_utf8_lossy(name)
            );
            throw(vm, b"Error", &m);
            return false;
        }
        return run_in_scope(
            vm,
            "$__o->__set($__n, $__v);",
            &[
                ("__o", Value::Object(obj.clone())),
                ("__n", vstr(name)),
                ("__v", value),
            ],
            scope,
        )
        .is_some();
    }
    run_in_scope(
        vm,
        "$__o->{$__n} = $__v;",
        &[
            ("__o", Value::Object(obj.clone())),
            ("__n", vstr(name)),
            ("__v", value),
        ],
        scope,
    )
    .is_some()
}

/// The visibility ("protected" / "private") of a declared property the
/// scope may not access (zend_get_property_offset's wrong cases).
fn inaccessible_property(
    vm: &Vm,
    class_lower: &[u8],
    scope: Option<&[u8]>,
    name: &[u8],
) -> Option<&'static str> {
    use goro_core::object::Visibility;
    let mut cur = class_lower.to_vec();
    for _ in 0..64 {
        let c = vm.classes.get(&cur)?;
        if let Some(p) = c.properties.iter().find(|p| p.name == name && !p.is_static) {
            let decl = p.declaring_class.to_ascii_lowercase();
            return match p.visibility {
                Visibility::Public => None,
                Visibility::Protected => {
                    let ok = scope.is_some_and(|s| {
                        s == decl.as_slice() || instanceof(vm, s, &decl) || instanceof(vm, &decl, s)
                    });
                    if ok { None } else { Some("protected") }
                }
                Visibility::Private => {
                    if scope == Some(decl.as_slice()) || decl.as_slice() != class_lower {
                        None
                    } else {
                        Some("private")
                    }
                }
            };
        }
        cur = c.parent.as_ref()?.to_ascii_lowercase();
    }
    None
}

/// The declaring class (lowercase) of a class's constructor, if any.
pub fn constructor_of(vm: &Vm, class_lower: &[u8]) -> Option<Vec<u8>> {
    let stub_ctor = |cl: &[u8]| {
        goro_core::stubs::find_method(cl, b"__construct")
            .map(|(sc, _)| sc.name.to_ascii_lowercase().into_bytes())
    };
    match vm.classes.get(class_lower) {
        Some(c) => match c.get_method(b"__construct") {
            Some(m) => Some(m.declaring_class.to_ascii_lowercase()),
            None => match c.parent.as_ref() {
                // an ancestor's constructor
                Some(p) => {
                    let p = p.to_ascii_lowercase();
                    if vm.classes.contains_key(&p) {
                        constructor_of(vm, &p)
                    } else {
                        stub_ctor(&p)
                    }
                }
                // an internal class registered by its extension
                None if c.filename.is_none() => stub_ctor(class_lower),
                None => None,
            },
        },
        None => stub_ctor(class_lower),
    }
}

/// zend_call_known_function(ce->constructor, obj, ..., named_params = args)
/// Returns false when the call failed (threw).
pub fn call_constructor(vm: &mut Vm, obj: &Obj, args: Option<&Arr>) -> bool {
    let cl = obj.borrow().class_name.to_ascii_lowercase();
    let scope = match constructor_of(vm, &cl) {
        Some(s) => s,
        None => return true,
    };
    let method = vm
        .classes
        .get(&scope)
        .and_then(|c| c.get_method(b"__construct"))
        .cloned();
    if let Some(m) = method {
        // zend_call_function() with the array as named_params
        let mut positional = Vec::new();
        let mut named: Vec<(Vec<u8>, Value)> = Vec::new();
        if let Some(a) = args {
            for (k, v) in a.borrow().iter() {
                match k {
                    ArrayKey::Int(_) => {
                        if !named.is_empty() {
                            throw(
                                vm,
                                b"Error",
                                "Cannot use positional argument after named argument",
                            );
                            return false;
                        }
                        positional.push(v.clone());
                    }
                    ArrayKey::String(s) => named.push((s.as_bytes().to_vec(), v.clone())),
                }
            }
        }
        // Too few arguments (zend_missing_arg_error for an internal caller)
        let op = &m.op_array;
        let required = op.required_param_count as usize;
        if named.is_empty() && positional.len() < required {
            let implicit = usize::from(op.cv_names.first().map(|n| n.as_slice()) == Some(b"this"));
            let total = (op.param_count as usize).saturating_sub(implicit);
            let q = if op.variadic_param.is_some() || total > required {
                "at least"
            } else {
                "exactly"
            };
            let cname = vm
                .classes
                .get(&scope)
                .map(|c| String::from_utf8_lossy(&c.name).into_owned())
                .unwrap_or_default();
            let msg = format!(
                "Too few arguments to function {}::__construct(), {} passed and {} {} expected",
                cname,
                positional.len(),
                q,
                required
            );
            throw(vm, b"ArgumentCountError", &msg);
            return false;
        }
        let had = vm.current_exception.is_some();
        vm.push_class_scope(m.declaring_class.to_ascii_lowercase());
        vm.called_class_stack.push(obj.borrow().class_name.clone());
        let was_global = std::mem::replace(&mut vm.is_global_scope, false);
        let line = vm.current_line;
        let r = vm.execute_fn_with_named_args(
            &m.op_array,
            positional,
            named,
            Some(Value::Object(obj.clone())),
        );
        vm.current_line = line;
        vm.is_global_scope = was_global;
        vm.called_class_stack.pop();
        vm.pop_class_scope();
        return r.is_ok() && (had || vm.current_exception.is_none());
    }
    let a = match args {
        Some(a) => Value::Array(Rc::new(RefCell::new(a.borrow().clone()))),
        None => Value::Array(Rc::new(RefCell::new(PhpArray::new()))),
    };
    run_in_scope(
        vm,
        "$__o->__construct(...$__a);",
        &[("__o", Value::Object(obj.clone())), ("__a", a)],
        &scope,
    )
    .is_some()
}

/// object_init_ex(): a new object of a (lowercase) class, without running
/// its constructor. None after throwing.
pub fn object_init_ex(vm: &mut Vm, class_lower: &[u8]) -> Option<Obj> {
    let name = vm
        .classes
        .get(class_lower)
        .map(|c| c.name.clone())
        .unwrap_or_else(|| vm.builtin_canonical_name(class_lower).into_bytes());
    if let Some(c) = vm.classes.get(class_lower) {
        let kind = if c.is_interface {
            Some("interface")
        } else if c.is_trait {
            Some("trait")
        } else if c.is_enum {
            Some("enum")
        } else if c.is_abstract {
            Some("abstract class")
        } else {
            None
        };
        if let Some(k) = kind {
            let m = format!(
                "Cannot instantiate {} {}",
                k,
                String::from_utf8_lossy(&c.name)
            );
            throw(vm, b"Error", &m);
            return None;
        }
    }
    let id = vm.alloc_object_id();
    let mut obj = PhpObject::new(name, id);
    vm.apply_class_property_defaults(&mut obj, class_lower);
    if !vm.classes.contains_key(class_lower) {
        // object_properties_init of an internal class: typed properties
        // without a default start uninitialized
        if let Some(sc) = goro_core::stubs::find_class(class_lower) {
            for p in sc.props {
                if p.flags & goro_core::stubs::F_STATIC != 0 || obj.has_property(p.name.as_bytes())
                {
                    continue;
                }
                let v = p
                    .default
                    .and_then(goro_core::stubs::literal_value)
                    .unwrap_or(Value::Undef);
                obj.set_property(p.name.as_bytes().to_vec(), v);
            }
        }
    }
    let o = Rc::new(RefCell::new(obj));
    goro_core::gc::track_object(&o);
    let has_dtor = vm
        .classes
        .get(class_lower)
        .is_some_and(|c| c.get_method(b"__destruct").is_some());
    if has_dtor {
        vm.track_destructible(Value::Object(o.clone()));
    }
    vm.run_object_create_hook(&o);
    if vm.current_exception.is_some() {
        return None;
    }
    Some(o)
}

/// zend_lookup_class (with autoloading): the lowercase name when found.
pub fn lookup_class(vm: &mut Vm, name: &[u8]) -> Option<Vec<u8>> {
    let lower = name
        .strip_prefix(b"\\")
        .unwrap_or(name)
        .to_ascii_lowercase();
    if lower.is_empty() {
        return None;
    }
    if vm.lookup_class(&lower).is_some() {
        return Some(lower);
    }
    let _ = vm.try_autoload_class(name);
    if vm.current_exception.is_some() {
        return None;
    }
    vm.lookup_class(&lower).map(|_| lower)
}

/// The declared name of a (lowercase) class
pub fn class_display(vm: &Vm, lower: &[u8]) -> Vec<u8> {
    vm.lookup_class(lower)
        .map(|c| c.name.to_vec())
        .unwrap_or_else(|| lower.to_vec())
}

/// instanceof_function(class, target) over user and internal ancestors
pub fn instanceof(vm: &Vm, class_lower: &[u8], target_lower: &[u8]) -> bool {
    if class_lower == target_lower {
        return true;
    }
    let mut cur = class_lower.to_vec();
    for _ in 0..64 {
        let parent = match vm.classes.get(&cur) {
            Some(c) => c.parent.clone(),
            None => {
                // internal class: its stub hierarchy
                return match goro_core::stubs::class_hierarchy(&cur) {
                    Some((_, h)) => {
                        h.parents_lower.iter().any(|p| p.as_slice() == target_lower)
                            || h.interfaces_lower
                                .iter()
                                .any(|p| p.as_slice() == target_lower)
                    }
                    None => false,
                };
            }
        };
        match parent {
            Some(p) => {
                cur = p.to_ascii_lowercase();
                if cur == target_lower {
                    return true;
                }
            }
            None => return false,
        }
    }
    false
}

pub fn vbool(b: bool) -> Value {
    if b { Value::True } else { Value::False }
}
