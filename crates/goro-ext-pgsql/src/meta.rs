//! pg_meta_data, pg_convert and the pg_insert / pg_update / pg_delete /
//! pg_select helpers of pgsql.c.

use std::cell::RefCell;
use std::rc::Rc;

use goro_core::array::{ArrayKey, PhpArray};
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::Vm;

use crate::handles::*;
use crate::libpq::ExecStatus;
use crate::link::{arr_value, bval, skey, sval};
use crate::zpp::{self, parse, throw, value_error};

const PGSQL_ASSOC: i64 = 1;
const PGSQL_BOTH: i64 = 3;
const PGSQL_CONV_IGNORE_DEFAULT: i64 = 1 << 1;
const PGSQL_CONV_FORCE_NULL: i64 = 1 << 2;
const PGSQL_CONV_IGNORE_NOT_NULL: i64 = 1 << 3;
const PGSQL_CONV_OPTS: i64 =
    PGSQL_CONV_IGNORE_DEFAULT | PGSQL_CONV_FORCE_NULL | PGSQL_CONV_IGNORE_NOT_NULL;
const PGSQL_DML_NO_CONV: i64 = 1 << 8;
const PGSQL_DML_EXEC: i64 = 1 << 9;
const PGSQL_DML_ASYNC: i64 = 1 << 10;
const PGSQL_DML_STRING: i64 = 1 << 11;
const PGSQL_DML_ESCAPE: i64 = 1 << 12;

/// PGG(regexes): (pattern, caseless); all compiled with the 'n' modifier
const REGEXES: &[(&str, bool)] = &[
    ("^([+-]{0,1}[0-9]+)$", false),
    ("^[-+]?[0-9]*\\.?[0-9]+([eE][-+]?[0-9]+)?$", false),
    ("^[+-]{0,1}(inf)(inity){0,1}$", true),
    ("^[0-9]+$", false),
    (
        "^((25[0-5]|(2[0-4]|1{0,1}[0-9]){0,1}[0-9])\\.){3,3}(25[0-5]|(2[0-4]|1{0,1}[0-9]){0,1}[0-9])(\\/[0-9]{1,3})?$",
        false,
    ),
    (
        "^(([0-9a-fA-F]{1,4}:){7,7}[0-9a-fA-F]{1,4}|([0-9a-fA-F]{1,4}:){1,7}:|([0-9a-fA-F]{1,4}:){1,6}:[0-9a-fA-F]{1,4}|([0-9a-fA-F]{1,4}:){1,5}(:[0-9a-fA-F]{1,4}){1,2}|([0-9a-fA-F]{1,4}:){1,4}(:[0-9a-fA-F]{1,4}){1,3}|([0-9a-fA-F]{1,4}:){1,3}(:[0-9a-fA-F]{1,4}){1,4}|([0-9a-fA-F]{1,4}:){1,2}(:[0-9a-fA-F]{1,4}){1,5}|[0-9a-fA-F]{1,4}:((:[0-9a-fA-F]{1,4}){1,6})|:((:[0-9a-fA-F]{1,4}){1,7}|:)|fe80:(:[0-9a-fA-F]{0,4}){0,4}%[0-9a-zA-Z]{1,}|::(ffff(:0{1,4}){0,1}:){0,1}((25[0-5]|(2[0-4]|1{0,1}[0-9]){0,1}[0-9])\\.){3,3}(25[0-5]|(2[0-4]|1{0,1}[0-9]){0,1}[0-9])|([0-9a-fA-F]{1,4}:){1,4}:((25[0-5]|(2[0-4]|1{0,1}[0-9]){0,1}[0-9])\\.){3,3}(25[0-5]|(2[0-4]|1{0,1}[0-9]){0,1}[0-9]))(\\/[0-9]{1,3})?$",
        false,
    ),
    (
        "^([0-9]{4}[/-][0-9]{1,2}[/-][0-9]{1,2})(([ \\t]+|T)(([0-9]{1,2}:[0-9]{1,2}){1}(:[0-9]{1,2}){0,1}(\\.[0-9]+){0,1}([ \\t]*([+-][0-9]{1,4}(:[0-9]{1,2}){0,1}|[-a-zA-Z_/+]{1,50})){0,1})){0,1}$",
        true,
    ),
    ("^([0-9]{4}[/-][0-9]{1,2}[/-][0-9]{1,2})$", true),
    (
        "^(([0-9]{1,2}:[0-9]{1,2}){1}(:[0-9]{1,2}){0,1}){0,1}$",
        true,
    ),
    (
        "^(@?[ \\t]+)?((([-+]?[ \\t]+)?[0-9]+(\\.[0-9]*)?[ \\t]*(millenniums|millennia|millennium|mil|mils|centuries|century|cent|c|decades|decade|dec|decs|years|year|y|months|month|mon|weeks|week|w|days|day|d|hours|hour|hr|hrs|h|minutes|minute|mins|min|m|seconds|second|secs|sec|s))+|((([-+]?[ \\t]+)?[0-9]+(\\.[0-9]*)?[ \\t]*(millenniums|millennia|millennium|mil|mils|centuries|century|cent|c|decades|decade|dec|decs|years|year|y|months|month|mon|weeks|week|w|days|day|d))+([-+]?[ \\t]+([0-9]+[ \\t]+)+(([0-9]{1,2}:){0,2}[0-9]{0,2}))?))([ \\t]+ago)?$",
        true,
    ),
    ("^([0-9a-f]{2,2}:){5,5}[0-9a-f]{2,2}$", true),
];

/// php_pgsql_convert_match
fn convert_match(vm: &mut Vm, s: &[u8], regex: usize) -> bool {
    if s.iter().any(|&c| c == b'\n' || c == b'\r' || c == 0) {
        return false;
    }
    let (pat, ci) = REGEXES[regex];
    let mut opts = goro_pcre::constants::PCRE2_NO_AUTO_CAPTURE;
    if ci {
        opts |= goro_pcre::constants::PCRE2_CASELESS;
    }
    let cctx = goro_pcre::CompileContext::default();
    let Ok(re) = goro_pcre::Code::compile(pat.as_bytes(), opts, &cctx) else {
        return false;
    };
    let mut md = goro_pcre::MatchData::new(1);
    let r = re.match_at(s, 0, 0, &mut md, &goro_pcre::MatchContext::default());
    if r == goro_pcre::constants::PCRE2_ERROR_NOMATCH {
        return false;
    }
    if r < 0 {
        warning(vm, "Cannot exec regex");
        return false;
    }
    true
}

/// _php_pgsql_identifier_is_escaped
pub fn identifier_is_escaped(id: &[u8]) -> bool {
    let len = id.len();
    if len <= 2 {
        return false;
    }
    if id[0] == b'"' && id[len - 1] == b'"' {
        let mut i = 1;
        while i < len - 1 {
            if id[i] == b'"' {
                i += 1;
                if id[i] != b'"' || i == len - 1 {
                    return false;
                }
            }
            i += 1;
        }
        true
    } else {
        false
    }
}

/// One pg_attribute row of php_pgsql_meta_data
struct Column {
    name: Vec<u8>,
    num: i64,
    typ: Vec<u8>,
    len: i64,
    not_null: bool,
    has_default: bool,
    array_dims: i64,
    typtype: Vec<u8>,
    description: Vec<u8>,
}

/// php_strtok_r(src, ".", &rest): (first token, rest)
fn strtok_dot(s: &[u8]) -> Option<(&[u8], &[u8])> {
    let mut i = 0;
    while i < s.len() && s[i] == b'.' {
        i += 1;
    }
    if i >= s.len() {
        return None;
    }
    let start = i;
    while i < s.len() && s[i] != b'.' {
        i += 1;
    }
    let tok = &s[start..i];
    let rest = if i < s.len() { &s[i + 1..] } else { &s[i..] };
    Some((tok, rest))
}

/// php_pgsql_meta_data: None after a warning / exception
fn meta_data(vm: &mut Vm, conn: &ConnRef, table: &[u8], extended: bool) -> Option<Vec<Column>> {
    let n = table.iter().position(|&c| c == 0).unwrap_or(table.len());
    let src = &table[..n];
    let tname = String::from_utf8_lossy(table).into_owned();
    let Some((tok, rest)) = strtok_dot(src) else {
        value_error(vm, active(), 2, &format!("must be specified ({})", tname));
        return None;
    };
    let (schema, name): (&[u8], &[u8]) = if rest.is_empty() {
        (b"public", tok)
    } else {
        (tok, rest)
    };
    let mut q: Vec<u8> = if extended {
        b"SELECT a.attname, a.attnum, t.typname, a.attlen, a.attnotNULL, a.atthasdef, a.attndims, t.typtype, d.description FROM pg_class as c  JOIN pg_attribute a ON (a.attrelid = c.oid)  JOIN pg_type t ON (a.atttypid = t.oid)  JOIN pg_namespace n ON (c.relnamespace = n.oid)  LEFT JOIN pg_description d ON (d.objoid=a.attrelid AND d.objsubid=a.attnum AND c.oid=d.objoid) WHERE a.attnum > 0  AND c.relname = '".to_vec()
    } else {
        b"SELECT a.attname, a.attnum, t.typname, a.attlen, a.attnotnull, a.atthasdef, a.attndims, t.typtype FROM pg_class as c  JOIN pg_attribute a ON (a.attrelid = c.oid)  JOIN pg_type t ON (a.atttypid = t.oid)  JOIN pg_namespace n ON (c.relnamespace = n.oid) WHERE a.attnum > 0 AND c.relname = '".to_vec()
    };
    let (esc, err) = conn.borrow_mut().escape_string_conn(name);
    if err {
        warning(vm, &format!("Escaping table name '{}' failed", tname));
        return None;
    }
    q.extend_from_slice(&esc);
    q.extend_from_slice(b"' AND n.nspname = '");
    let (esc, err) = conn.borrow_mut().escape_string_conn(schema);
    if err {
        warning(vm, &format!("Escaping table namespace '{}' failed", tname));
        return None;
    }
    q.extend_from_slice(&esc);
    q.extend_from_slice(b"' ORDER BY a.attnum;");
    let res = conn.borrow_mut().exec(&q);
    let res = match res {
        Some(r) if r.status == ExecStatus::TuplesOk && r.ntuples() > 0 => r,
        _ => {
            warning(vm, &format!("Table '{}' doesn't exists", tname));
            return None;
        }
    };
    let mut cols = Vec::new();
    for i in 0..res.ntuples() {
        cols.push(Column {
            name: res.value(i, 0).to_vec(),
            num: crate::fetch::atoi(res.value(i, 1)),
            typ: res.value(i, 2).to_vec(),
            len: crate::fetch::atoi(res.value(i, 3)),
            not_null: res.value(i, 4) == b"t",
            has_default: res.value(i, 5) == b"t",
            array_dims: crate::fetch::atoi(res.value(i, 6)),
            typtype: res.value(i, 7).to_vec(),
            description: if extended {
                res.value(i, 8).to_vec()
            } else {
                Vec::new()
            },
        });
    }
    Some(cols)
}

pub fn pg_meta_data(vm: &mut Vm, f: &'static str, args: &[Value]) -> Value {
    let Ok(a) = parse(vm, f, "Cp|b", args) else {
        return Value::Null;
    };
    let Some((_, c)) = link_conn(vm, &a[0].obj().unwrap()) else {
        return Value::Null;
    };
    let table = a[1].str().to_vec();
    if table.is_empty() {
        zpp::must_not_be_empty(vm, f, 2);
        return Value::Null;
    }
    let extended = a[2].bool_or(false);
    let Some(cols) = meta_data(vm, &c, &table, extended) else {
        return Value::False;
    };
    let mut out = PhpArray::new();
    for col in cols {
        let mut e = PhpArray::new();
        e.set(skey(b"num"), Value::Long(col.num));
        e.set(skey(b"type"), sval(&col.typ));
        e.set(skey(b"len"), Value::Long(col.len));
        e.set(skey(b"not null"), bval(col.not_null));
        e.set(skey(b"has default"), bval(col.has_default));
        e.set(skey(b"array dims"), Value::Long(col.array_dims));
        e.set(skey(b"is enum"), bval(col.typtype == b"e"));
        if extended {
            e.set(skey(b"is base"), bval(col.typtype == b"b"));
            e.set(skey(b"is composite"), bval(col.typtype == b"c"));
            e.set(skey(b"is pseudo"), bval(col.typtype == b"p"));
            e.set(skey(b"description"), sval(&col.description));
        }
        out.set(skey(&col.name), arr_value(e));
    }
    arr_value(out)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DataType {
    Bool,
    Oid,
    Int2,
    Int4,
    Int8,
    Float4,
    Float8,
    Numeric,
    Money,
    Text,
    Char,
    Varchar,
    UnixTime,
    UnixTimeInterval,
    Date,
    Time,
    TimeWithTimezone,
    Timestamp,
    TimestampWithTimezone,
    Interval,
    Bytea,
    Cidr,
    Inet,
    Macaddr,
    Bit,
    Varbit,
    Line,
    Lseg,
    Box,
    Path,
    Point,
    Polygon,
    Circle,
    Unknown,
}

/// php_pgsql_get_data_type
fn get_data_type(t: &[u8]) -> DataType {
    use DataType::*;
    match t {
        b"bool" | b"boolean" => Bool,
        b"oid" => Oid,
        b"int2" | b"smallint" => Int2,
        b"int4" | b"integer" => Int4,
        b"int8" | b"bigint" => Int8,
        b"float4" | b"real" => Float4,
        b"float8" | b"double precision" => Float8,
        b"numeric" => Numeric,
        b"money" => Money,
        b"text" => Text,
        b"bpchar" | b"character" => Char,
        b"varchar" | b"character varying" => Varchar,
        b"abstime" => UnixTime,
        b"reltime" | b"tinterval" => UnixTimeInterval,
        b"date" => Date,
        b"time" => Time,
        b"time with time zone" | b"timetz" => TimeWithTimezone,
        b"timestamp without time zone" | b"timestamp" => Timestamp,
        b"timestamp with time zone" | b"timestamptz" => TimestampWithTimezone,
        b"interval" => Interval,
        b"bytea" => Bytea,
        b"cidr" => Cidr,
        b"inet" => Inet,
        b"macaddr" => Macaddr,
        b"bit" => Bit,
        b"bit varying" => Varbit,
        b"line" => Line,
        b"lseg" => Lseg,
        b"box" => Box,
        b"path" => Path,
        b"point" => Point,
        b"polygon" => Polygon,
        b"circle" => Circle,
        _ => Unknown,
    }
}

/// php_pgsql_add_quotes
fn add_quotes(s: &[u8]) -> Value {
    let mut v = b"E'".to_vec();
    v.extend_from_slice(s);
    v.push(b'\'');
    sval(&v)
}

fn null_str() -> Value {
    sval(b"NULL")
}

fn lossy(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

/// convert_to_long of a numeric string
fn str_to_long(vm: &mut Vm, s: &[u8]) -> Value {
    let _ = vm;
    Value::Long(Value::String(PhpString::from_bytes(s)).to_long())
}

/// php_pgsql_convert: the escaped/converted values, or None on failure
fn convert(
    vm: &mut Vm,
    conn: &ConnRef,
    table: &[u8],
    values: &PhpArray,
    opt: i64,
) -> Option<PhpArray> {
    let meta = meta_data(vm, conn, table, false)?;
    let mut result = PhpArray::new();
    let fname = active();
    let entries: Vec<(ArrayKey, Value)> =
        values.iter().map(|(k, v)| (k.clone(), v.deref())).collect();
    for (k, val) in entries {
        let field = match &k {
            ArrayKey::String(s) => s.as_bytes().to_vec(),
            ArrayKey::Int(_) => {
                throw(
                    vm,
                    b"ValueError",
                    "Array of values must be an associative array with string keys",
                );
                return None;
            }
        };
        let fs = lossy(&field);
        let Some(def) = meta.iter().find(|c| c.name == field) else {
            notice(vm, &format!("Invalid field name ({}) in values", fs));
            return None;
        };
        if matches!(
            val,
            Value::Array(_) | Value::Object(_) | Value::Resource(_) | Value::Generator(_)
        ) {
            let m = format!(
                "Values must be of type string|int|float|bool|null, {} given",
                Vm::value_type_name(&val)
            );
            throw(vm, b"TypeError", &m);
            return None;
        }
        let ty = lossy(&def.typ);
        let data_type = if def.typtype == b"e" {
            DataType::Text
        } else {
            get_data_type(&def.typ)
        };
        let mut err = 0;
        let mut escape_err = false;
        let mut new_val = Value::Null;
        let s_val = |v: &Value| -> Vec<u8> {
            match v {
                Value::String(s) => s.as_bytes().to_vec(),
                _ => Vec::new(),
            }
        };
        use DataType::*;
        match data_type {
            Bool => {
                match &val {
                    Value::String(s) => {
                        let s = s.as_bytes();
                        if s.is_empty() {
                            new_val = null_str();
                        } else if matches!(
                            s,
                            b"t" | b"T" | b"y" | b"Y" | b"true" | b"True" | b"yes" | b"Yes" | b"1"
                        ) {
                            new_val = sval(b"'t'");
                        } else if matches!(
                            s,
                            b"f" | b"F" | b"n" | b"N" | b"false" | b"False" | b"no" | b"No" | b"0"
                        ) {
                            new_val = sval(b"'f'");
                        } else {
                            let m = format!(
                                "{}(): Field \"{}\" must be of type bool, invalid PostgreSQL string boolean value \"{}\" given",
                                fname,
                                fs,
                                lossy(s)
                            );
                            throw(vm, b"ValueError", &m);
                            err = 1;
                        }
                    }
                    Value::Long(n) => new_val = sval(if *n != 0 { b"'t'" } else { b"'f'" }),
                    Value::True => new_val = sval(b"'t'"),
                    Value::False => new_val = sval(b"'f'"),
                    Value::Null => new_val = null_str(),
                    _ => err = 1,
                }
                if err != 0 && vm.current_exception.is_none() {
                    let m = format!(
                        "{}(): Field \"{}\" must be of type string|null|int|bool, {} given",
                        fname, fs, ty
                    );
                    throw(vm, b"TypeError", &m);
                }
            }
            Oid | Int2 | Int4 | Int8 => {
                match &val {
                    Value::String(s) => {
                        if s.is_empty() {
                            new_val = null_str();
                        } else if !convert_match(vm, s.as_bytes(), 0) {
                            err = 1;
                        } else {
                            new_val = val.clone();
                        }
                    }
                    Value::Double(d) => new_val = Value::Long(Value::Double(*d).to_long()),
                    Value::Long(n) => new_val = Value::Long(*n),
                    Value::Null => new_val = null_str(),
                    _ => err = 1,
                }
                if err != 0 {
                    let m = format!(
                        "{}(): Field \"{}\" must be of type int|null, {} given",
                        fname, fs, ty
                    );
                    throw(vm, b"TypeError", &m);
                }
            }
            Numeric | Money | Float4 | Float8 => {
                match &val {
                    Value::String(s) => {
                        if s.is_empty() {
                            new_val = null_str();
                        } else if !convert_match(vm, s.as_bytes(), 1) {
                            if !convert_match(vm, s.as_bytes(), 2) {
                                err = 1;
                            } else {
                                new_val = add_quotes(s.as_bytes());
                            }
                        } else {
                            new_val = val.clone();
                        }
                    }
                    Value::Long(n) => new_val = Value::Long(*n),
                    Value::Double(d) => new_val = Value::Double(*d),
                    Value::Null => new_val = null_str(),
                    _ => err = 1,
                }
                if err != 0 {
                    // (the arguments are out of order in php-src)
                    let m = format!(
                        "{}(): Field \"{}\" must be of type {}|int|null, {} given",
                        fname,
                        if data_type == Money { "money" } else { "float" },
                        fs,
                        ty
                    );
                    throw(vm, b"TypeError", &m);
                }
            }
            Text | Char | Varchar | Bit | Varbit | Line | Lseg | Point | Box | Path | Polygon
            | Circle | Unknown => {
                match &val {
                    Value::String(s) => {
                        if s.is_empty() {
                            new_val = if opt & PGSQL_CONV_FORCE_NULL != 0 {
                                null_str()
                            } else {
                                sval(b"''")
                            };
                        } else {
                            let (e, bad) = conn.borrow_mut().escape_string_conn(s.as_bytes());
                            if bad {
                                err = 1;
                                escape_err = true;
                            } else {
                                new_val = add_quotes(&e);
                            }
                        }
                    }
                    Value::Long(n) => new_val = sval(n.to_string().as_bytes()),
                    Value::Double(_) => new_val = Value::String(vm.value_to_string(&val)),
                    Value::Null => new_val = null_str(),
                    _ => err = 1,
                }
                if err != 0 {
                    if escape_err {
                        notice(
                            vm,
                            &format!(
                                "String value escaping failed for PostgreSQL '{}' ({})",
                                ty, fs
                            ),
                        );
                    } else {
                        let m = format!(
                            "{}(): Field \"{}\" must be of type string|null, {} given",
                            fname, fs, ty
                        );
                        throw(vm, b"TypeError", &m);
                    }
                }
            }
            UnixTime | UnixTimeInterval => {
                match &val {
                    Value::String(s) => {
                        if s.is_empty() {
                            new_val = null_str();
                        } else if !convert_match(vm, s.as_bytes(), 3) {
                            err = 1;
                        } else {
                            new_val = str_to_long(vm, s.as_bytes());
                        }
                    }
                    Value::Double(d) => new_val = Value::Long(Value::Double(*d).to_long()),
                    Value::Long(n) => new_val = Value::Long(*n),
                    Value::Null => new_val = null_str(),
                    _ => err = 1,
                }
                if err != 0 {
                    let m = format!(
                        "{}(): Field \"{}\" must be of type int|null, {} given",
                        fname,
                        fs,
                        Vm::value_type_name(&val)
                    );
                    throw(vm, b"TypeError", &m);
                }
            }
            Cidr | Inet => {
                match &val {
                    Value::String(s) => {
                        if s.is_empty() {
                            new_val = null_str();
                        } else if !convert_match(vm, s.as_bytes(), 4)
                            && !convert_match(vm, s.as_bytes(), 5)
                        {
                            err = 2;
                        } else {
                            new_val = add_quotes(s.as_bytes());
                        }
                    }
                    Value::Null => new_val = null_str(),
                    _ => err = 1,
                }
                if err == 2 {
                    let m = format!(
                        "{}(): Field \"{}\" must be a valid IPv4 or IPv6 address string, \"{}\" given",
                        fname,
                        fs,
                        lossy(&s_val(&val))
                    );
                    throw(vm, b"ValueError", &m);
                } else if err != 0 {
                    let m = format!(
                        "{}(): Field \"{}\" must be of type string|null, given {}",
                        fname,
                        fs,
                        Vm::value_type_name(&val)
                    );
                    throw(vm, b"TypeError", &m);
                }
            }
            TimeWithTimezone
            | Timestamp
            | TimestampWithTimezone
            | Date
            | Time
            | Interval
            | Macaddr => {
                let regex = match data_type {
                    Date => 7,
                    Time => 8,
                    Interval => 9,
                    Macaddr => 10,
                    _ => 6,
                };
                let is_ts = regex == 6;
                match &val {
                    Value::String(s) => {
                        let b = s.as_bytes();
                        if b.is_empty() {
                            new_val = null_str();
                        } else if is_ts && b.eq_ignore_ascii_case(b"now()") {
                            new_val = sval(b"NOW()");
                        } else if !convert_match(vm, b, regex) {
                            err = 1;
                        } else {
                            new_val = add_quotes(b);
                        }
                    }
                    Value::Null => new_val = null_str(),
                    _ => err = 1,
                }
                if err != 0 {
                    let m = format!(
                        "{}(): Field \"{}\" must be of type string|null, {} given",
                        fname, fs, ty
                    );
                    throw(vm, b"TypeError", &m);
                }
            }
            Bytea => {
                match &val {
                    Value::String(s) => {
                        if s.is_empty() {
                            new_val = null_str();
                        } else {
                            let e = conn.borrow_mut().escape_bytea_conn(s.as_bytes());
                            new_val = add_quotes(&e);
                        }
                    }
                    Value::Long(n) => new_val = sval(n.to_string().as_bytes()),
                    Value::Double(_) => new_val = Value::String(vm.value_to_string(&val)),
                    Value::Null => new_val = null_str(),
                    _ => err = 1,
                }
                if err != 0 {
                    let m = format!(
                        "{}(): Field \"{}\" must be of type string|null, {} given",
                        fname, fs, ty
                    );
                    throw(vm, b"TypeError", &m);
                }
            }
        }
        // PGSQL_CONV_CHECK_IGNORE() (the exception, if any, stays)
        let mut skip_field = false;
        if err == 0 {
            if let Value::String(s) = &new_val {
                if s.as_bytes() == b"NULL" {
                    if opt & PGSQL_CONV_IGNORE_DEFAULT == 0 && def.has_default {
                        skip_field = true;
                    } else if opt & PGSQL_CONV_IGNORE_NOT_NULL == 0 && def.not_null {
                        notice(vm, &format!("Detected NULL for 'NOT NULL' field '{}'", fs));
                        err = 1;
                    }
                }
            }
        }
        if err != 0 {
            return None;
        }
        if !skip_field {
            if identifier_is_escaped(&field) {
                result.set(skey(&field), new_val);
            } else {
                let Some(esc) = conn.borrow_mut().escape_identifier(&field) else {
                    notice(vm, &format!("Escaping field '{}' failed", fs));
                    return None;
                };
                result.set(skey(&esc), new_val);
            }
        }
    }
    Some(result)
}

/// php_pgsql_flush_query: the count of leftover results
fn flush_query(vm: &mut Vm, c: &ConnRef) -> i64 {
    if c.borrow_mut().set_nonblocking(true) != 0 {
        notice(vm, "Cannot set connection to nonblocking mode");
        return -1;
    }
    let mut n = 0;
    while c.borrow_mut().get_result().is_some() {
        n += 1;
    }
    let _ = c.borrow_mut().set_nonblocking(false);
    n
}

fn check_flush(vm: &mut Vm, c: &ConnRef) {
    if flush_query(vm, c) != 0 {
        notice(vm, "Detected unhandled result(s) in connection");
    }
}

pub fn pg_convert(vm: &mut Vm, f: &'static str, args: &[Value]) -> Value {
    let Ok(a) = parse(vm, f, "Cpa|l", args) else {
        return Value::Null;
    };
    let table = a[1].str().to_vec();
    if table.is_empty() {
        zpp::must_not_be_empty(vm, f, 2);
        return Value::Null;
    }
    let opt = a[3].long_or(0);
    if opt & !PGSQL_CONV_OPTS != 0 {
        value_error(
            vm,
            f,
            4,
            "must be a valid bit mask of PGSQL_CONV_IGNORE_DEFAULT, PGSQL_CONV_FORCE_NULL, and PGSQL_CONV_IGNORE_NOT_NULL",
        );
        return Value::Null;
    }
    let Some((_, c)) = link_conn(vm, &a[0].obj().unwrap()) else {
        return Value::Null;
    };
    check_flush(vm, &c);
    let values = a[2].arr().unwrap().borrow().clone();
    match convert(vm, &c, &table, &values, opt) {
        Some(r) => arr_value(r),
        None => {
            if vm.current_exception.is_some() {
                Value::Null
            } else {
                Value::False
            }
        }
    }
}

/// do_exec
fn do_exec(vm: &mut Vm, q: &[u8], c: &ConnRef, opt: i64) -> bool {
    if opt & PGSQL_DML_ASYNC != 0 {
        if c.borrow_mut().send_query(q) {
            return true;
        }
    } else {
        let r = c.borrow_mut().exec(q);
        match r {
            Some(r) if r.status == ExecStatus::CommandOk => return true,
            r => {
                let m = r.map(|r| r.error_message().to_vec()).unwrap_or_default();
                warning(vm, &lossy(&m));
            }
        }
    }
    false
}

/// build_tablename
fn build_tablename(vm: &mut Vm, q: &mut Vec<u8>, c: &ConnRef, table: &[u8]) -> bool {
    let dot = table.iter().position(|&x| x == b'.');
    let first = &table[..dot.unwrap_or(table.len())];
    let fail = |vm: &mut Vm| {
        notice(
            vm,
            &format!("Failed to escape table name '{}'", lossy(table)),
        );
        false
    };
    if identifier_is_escaped(first) {
        q.extend_from_slice(first);
    } else {
        match c.borrow_mut().escape_identifier(first) {
            Some(e) => q.extend_from_slice(&e),
            None => return fail(vm),
        }
    }
    if let Some(d) = dot {
        let after = &table[d + 1..];
        q.push(b'.');
        if identifier_is_escaped(after) {
            q.extend_from_slice(after);
        } else {
            match c.borrow_mut().escape_identifier(after) {
                Some(e) => q.extend_from_slice(&e),
                None => return fail(vm),
            }
        }
    }
    true
}

/// snprintf("%F")
fn fmt_f(d: f64) -> String {
    if d.is_nan() {
        return "NAN".into();
    }
    if d.is_infinite() {
        return if d > 0.0 { "INF".into() } else { "-INF".into() };
    }
    format!("{:.6}", d)
}

/// A value of the VALUES / SET / WHERE lists; Err after an exception or
/// notice
fn append_value(
    vm: &mut Vm,
    q: &mut Vec<u8>,
    c: &ConnRef,
    fld: &[u8],
    val: &Value,
    opt: i64,
) -> Result<(), ()> {
    match val {
        Value::String(s) => {
            if opt & PGSQL_DML_ESCAPE != 0 {
                let (e, bad) = c.borrow_mut().escape_string_conn(s.as_bytes());
                if bad {
                    notice(
                        vm,
                        &format!("Failed to escape field '{}' value", lossy(fld)),
                    );
                    return Err(());
                }
                q.push(b'\'');
                q.extend_from_slice(&e);
                q.push(b'\'');
            } else {
                q.extend_from_slice(s.as_bytes());
            }
        }
        Value::Long(n) => q.extend_from_slice(n.to_string().as_bytes()),
        Value::Double(d) => q.extend_from_slice(fmt_f(*d).as_bytes()),
        Value::Null => q.extend_from_slice(b"NULL"),
        other => {
            let m = format!(
                "Value must be of type string|int|float|null, {} given",
                Vm::value_type_name(other)
            );
            throw(vm, b"TypeError", &m);
            return Err(());
        }
    }
    Ok(())
}

fn append_field(vm: &mut Vm, q: &mut Vec<u8>, c: &ConnRef, fld: &[u8], opt: i64) -> Result<(), ()> {
    if opt & PGSQL_DML_ESCAPE != 0 {
        match c.borrow_mut().escape_identifier(fld) {
            Some(e) => q.extend_from_slice(&e),
            None => {
                notice(vm, &format!("Failed to escape field '{}'", lossy(fld)));
                return Err(());
            }
        }
    } else {
        q.extend_from_slice(fld);
    }
    Ok(())
}

fn string_key(vm: &mut Vm, k: &ArrayKey) -> Result<Vec<u8>, ()> {
    match k {
        ArrayKey::String(s) => Ok(s.as_bytes().to_vec()),
        ArrayKey::Int(_) => {
            throw(
                vm,
                b"ValueError",
                "Array of values must be an associative array with string keys",
            );
            Err(())
        }
    }
}

/// build_assignment_string
fn build_assignment(
    vm: &mut Vm,
    q: &mut Vec<u8>,
    c: &ConnRef,
    ht: &PhpArray,
    where_cond: bool,
    pad: &[u8],
    opt: i64,
) -> Result<(), ()> {
    let entries: Vec<(ArrayKey, Value)> = ht.iter().map(|(k, v)| (k.clone(), v.deref())).collect();
    let any = !entries.is_empty();
    for (k, val) in entries {
        let fld = string_key(vm, &k)?;
        append_field(vm, q, c, &fld, opt)?;
        let is_null_kw = matches!(&val, Value::String(s) if s.as_bytes() == b"NULL");
        if where_cond && (matches!(val, Value::True | Value::False) || is_null_kw) {
            q.extend_from_slice(b" IS ");
        } else {
            q.push(b'=');
        }
        append_value(vm, q, c, &fld, &val, opt)?;
        q.extend_from_slice(pad);
    }
    if any {
        q.truncate(q.len() - pad.len());
    }
    Ok(())
}

/// The converted array (or the array itself with NO_CONV / ESCAPE)
fn maybe_convert(
    vm: &mut Vm,
    c: &ConnRef,
    table: &[u8],
    arr: &PhpArray,
    opt: i64,
) -> Option<PhpArray> {
    if opt & (PGSQL_DML_NO_CONV | PGSQL_DML_ESCAPE) == 0 {
        convert(vm, c, table, arr, opt & PGSQL_CONV_OPTS)
    } else {
        Some(arr.clone())
    }
}

/// php_pgsql_insert: the SQL on success
fn insert(vm: &mut Vm, c: &ConnRef, table: &[u8], values: &PhpArray, opt: i64) -> Option<Vec<u8>> {
    let mut q = b"INSERT INTO ".to_vec();
    if values.is_empty() {
        if !build_tablename(vm, &mut q, c, table) {
            return None;
        }
        q.extend_from_slice(b" DEFAULT VALUES");
    } else {
        let vals = maybe_convert(vm, c, table, values, opt)?;
        if !build_tablename(vm, &mut q, c, table) {
            return None;
        }
        q.extend_from_slice(b" (");
        let entries: Vec<(ArrayKey, Value)> =
            vals.iter().map(|(k, v)| (k.clone(), v.deref())).collect();
        for (k, _) in &entries {
            let fld = string_key(vm, k).ok()?;
            append_field(vm, &mut q, c, &fld, opt).ok()?;
            q.push(b',');
        }
        q.pop();
        q.extend_from_slice(b") VALUES (");
        for (k, v) in &entries {
            let fld = match k {
                ArrayKey::String(s) => s.as_bytes().to_vec(),
                ArrayKey::Int(i) => i.to_string().into_bytes(),
            };
            append_value(vm, &mut q, c, &fld, v, opt).ok()?;
            q.push(b',');
        }
        q.pop();
        q.extend_from_slice(b");");
    }
    if (opt & (PGSQL_DML_EXEC | PGSQL_DML_ASYNC)) != 0 && do_exec(vm, &q, c, opt & PGSQL_CONV_OPTS)
    {
        return Some(q);
    }
    if opt & PGSQL_DML_STRING != 0 {
        return Some(q);
    }
    None
}

fn arr_arg(a: &zpp::Arg) -> PhpArray {
    a.arr().map(|x| x.borrow().clone()).unwrap_or_default()
}

pub fn pg_insert(vm: &mut Vm, f: &'static str, args: &[Value]) -> Value {
    let Ok(a) = parse(vm, f, "Cpa|l", args) else {
        return Value::Null;
    };
    let table = a[1].str().to_vec();
    if table.is_empty() {
        zpp::must_not_be_empty(vm, f, 2);
        return Value::Null;
    }
    let mut option = a[3].long_or(PGSQL_DML_EXEC);
    if option
        & !(PGSQL_CONV_OPTS
            | PGSQL_DML_NO_CONV
            | PGSQL_DML_EXEC
            | PGSQL_DML_ASYNC
            | PGSQL_DML_STRING
            | PGSQL_DML_ESCAPE)
        != 0
    {
        value_error(
            vm,
            f,
            4,
            "must be a valid bit mask of PGSQL_CONV_FORCE_NULL, PGSQL_DML_NO_CONV, PGSQL_DML_ESCAPE, PGSQL_DML_EXEC, PGSQL_DML_ASYNC, and PGSQL_DML_STRING",
        );
        return Value::Null;
    }
    let Some((_, c)) = link_conn(vm, &a[0].obj().unwrap()) else {
        return Value::Null;
    };
    check_flush(vm, &c);
    let values = arr_arg(&a[2]);
    let return_sql = option & PGSQL_DML_STRING != 0;
    if option & PGSQL_DML_EXEC != 0 {
        option &= !PGSQL_DML_EXEC;
        let Some(sql) = insert(vm, &c, &table, &values, option | PGSQL_DML_STRING) else {
            return if vm.current_exception.is_some() {
                Value::Null
            } else {
                Value::False
            };
        };
        let mut res = c.borrow_mut().exec(&sql);
        let auto_reset = vm
            .constants
            .get(b"pgsql.auto_reset_persistent".as_slice())
            .map(|v| v.to_long())
            .unwrap_or(0)
            & 2
            != 0;
        if auto_reset && c.borrow().status != crate::libpq::ConnStatus::Ok {
            c.borrow_mut().reset();
            res = c.borrow_mut().exec(&sql);
        }
        let status = match &res {
            Some(r) => r.status as i32,
            None => c.borrow().status as i32,
        };
        return match status {
            0 | 5 | 6 | 7 => {
                pq_error(vm, "Query failed: %s", &c);
                Value::False
            }
            _ => match res {
                Some(r) => new_result(vm, &c, r),
                None => Value::False,
            },
        };
    }
    match insert(vm, &c, &table, &values, option) {
        None => {
            if vm.current_exception.is_some() {
                Value::Null
            } else {
                Value::False
            }
        }
        Some(sql) => {
            if return_sql {
                sval(&sql)
            } else {
                Value::True
            }
        }
    }
}

fn fail_value(vm: &Vm) -> Value {
    if vm.current_exception.is_some() {
        Value::Null
    } else {
        Value::False
    }
}

pub fn pg_update(vm: &mut Vm, f: &'static str, args: &[Value]) -> Value {
    let Ok(a) = parse(vm, f, "Cpaa|l", args) else {
        return Value::Null;
    };
    let table = a[1].str().to_vec();
    if table.is_empty() {
        zpp::must_not_be_empty(vm, f, 2);
        return Value::Null;
    }
    let opt = a[4].long_or(PGSQL_DML_EXEC);
    if opt
        & !(PGSQL_CONV_OPTS
            | PGSQL_DML_NO_CONV
            | PGSQL_DML_EXEC
            | PGSQL_DML_STRING
            | PGSQL_DML_ESCAPE)
        != 0
    {
        value_error(
            vm,
            f,
            5,
            "must be a valid bit mask of PGSQL_CONV_FORCE_NULL, PGSQL_DML_NO_CONV, PGSQL_DML_ESCAPE, PGSQL_DML_EXEC, PGSQL_DML_ASYNC, and PGSQL_DML_STRING",
        );
        return Value::Null;
    }
    let Some((_, c)) = link_conn(vm, &a[0].obj().unwrap()) else {
        return Value::Null;
    };
    check_flush(vm, &c);
    let values = arr_arg(&a[2]);
    let ids = arr_arg(&a[3]);
    if values.is_empty() || ids.is_empty() {
        return Value::False;
    }
    let Some(vals) = maybe_convert(vm, &c, &table, &values, opt) else {
        return fail_value(vm);
    };
    let Some(ids) = maybe_convert(vm, &c, &table, &ids, opt) else {
        return fail_value(vm);
    };
    let mut q = b"UPDATE ".to_vec();
    if !build_tablename(vm, &mut q, &c, &table) {
        return fail_value(vm);
    }
    q.extend_from_slice(b" SET ");
    if build_assignment(vm, &mut q, &c, &vals, false, b",", opt).is_err() {
        return fail_value(vm);
    }
    q.extend_from_slice(b" WHERE ");
    if build_assignment(vm, &mut q, &c, &ids, true, b" AND ", opt).is_err() {
        return fail_value(vm);
    }
    q.push(b';');
    finish_dml(vm, &c, q, opt)
}

/// The tail of php_pgsql_update / php_pgsql_delete and their callers
fn finish_dml(vm: &mut Vm, c: &ConnRef, q: Vec<u8>, opt: i64) -> Value {
    let ok = (opt & PGSQL_DML_EXEC != 0 && do_exec(vm, &q, c, opt)) || opt & PGSQL_DML_STRING != 0;
    if !ok {
        return fail_value(vm);
    }
    if opt & PGSQL_DML_STRING != 0 {
        return sval(&q);
    }
    Value::True
}

pub fn pg_delete(vm: &mut Vm, f: &'static str, args: &[Value]) -> Value {
    let Ok(a) = parse(vm, f, "Cpa|l", args) else {
        return Value::Null;
    };
    let table = a[1].str().to_vec();
    if table.is_empty() {
        zpp::must_not_be_empty(vm, f, 2);
        return Value::Null;
    }
    let opt = a[3].long_or(PGSQL_DML_EXEC);
    if opt
        & !(PGSQL_CONV_FORCE_NULL
            | PGSQL_DML_NO_CONV
            | PGSQL_DML_EXEC
            | PGSQL_DML_STRING
            | PGSQL_DML_ESCAPE)
        != 0
    {
        value_error(
            vm,
            f,
            4,
            "must be a valid bit mask of PGSQL_CONV_FORCE_NULL, PGSQL_DML_NO_CONV, PGSQL_DML_ESCAPE, PGSQL_DML_EXEC, PGSQL_DML_ASYNC, and PGSQL_DML_STRING",
        );
        return Value::Null;
    }
    let Some((_, c)) = link_conn(vm, &a[0].obj().unwrap()) else {
        return Value::Null;
    };
    check_flush(vm, &c);
    let ids = arr_arg(&a[2]);
    if ids.is_empty() {
        return Value::False;
    }
    let Some(ids) = maybe_convert(vm, &c, &table, &ids, opt) else {
        return fail_value(vm);
    };
    let mut q = b"DELETE FROM ".to_vec();
    if !build_tablename(vm, &mut q, &c, &table) {
        return fail_value(vm);
    }
    q.extend_from_slice(b" WHERE ");
    if build_assignment(vm, &mut q, &c, &ids, true, b" AND ", opt).is_err() {
        return fail_value(vm);
    }
    q.push(b';');
    finish_dml(vm, &c, q, opt)
}

pub fn pg_select(vm: &mut Vm, f: &'static str, args: &[Value]) -> Value {
    let Ok(a) = parse(vm, f, "Cp|all", args) else {
        return Value::Null;
    };
    let table = a[1].str().to_vec();
    if table.is_empty() {
        zpp::must_not_be_empty(vm, f, 2);
        return Value::Null;
    }
    let opt = a[3].long_or(PGSQL_DML_EXEC);
    let result_type = a[4].long_or(PGSQL_ASSOC);
    if opt
        & !(PGSQL_CONV_FORCE_NULL
            | PGSQL_DML_NO_CONV
            | PGSQL_DML_EXEC
            | PGSQL_DML_ASYNC
            | PGSQL_DML_STRING
            | PGSQL_DML_ESCAPE)
        != 0
    {
        value_error(
            vm,
            f,
            4,
            "must be a valid bit mask of PGSQL_CONV_FORCE_NULL, PGSQL_DML_NO_CONV, PGSQL_DML_ESCAPE, PGSQL_DML_EXEC, PGSQL_DML_ASYNC, and PGSQL_DML_STRING",
        );
        return Value::Null;
    }
    if result_type & PGSQL_BOTH == 0 {
        value_error(
            vm,
            f,
            5,
            "must be one of PGSQL_ASSOC, PGSQL_NUM, or PGSQL_BOTH",
        );
        return Value::Null;
    }
    let Some((_, c)) = link_conn(vm, &a[0].obj().unwrap()) else {
        return Value::Null;
    };
    check_flush(vm, &c);
    let ids = a[2].arr().map(|x| x.borrow().clone());
    let ids = match ids {
        Some(i) if !i.is_empty() => match maybe_convert(vm, &c, &table, &i, opt) {
            Some(i) => Some(i),
            None => return fail_value(vm),
        },
        _ => None,
    };
    let mut q = b"SELECT * FROM ".to_vec();
    if !build_tablename(vm, &mut q, &c, &table) {
        return fail_value(vm);
    }
    if let Some(ids) = &ids {
        q.extend_from_slice(b" WHERE ");
        if build_assignment(vm, &mut q, &c, ids, true, b" AND ", opt).is_err() {
            return fail_value(vm);
        }
    }
    q.push(b';');
    let res = c.borrow_mut().exec(&q);
    let arr = match res {
        Some(r) if r.status == ExecStatus::TuplesOk => crate::fetch::result2array(&r, result_type),
        _ => {
            notice(vm, &format!("Failed to execute '{}'", lossy(&q)));
            return fail_value(vm);
        }
    };
    if opt & PGSQL_DML_STRING != 0 {
        return sval(&q);
    }
    Value::Array(Rc::new(RefCell::new(arr)))
}
