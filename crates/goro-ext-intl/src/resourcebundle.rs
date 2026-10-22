//! ext/intl/resourcebundle: ResourceBundle and the resourcebundle_*()
//! functions, over a reader of ICU's binary resource bundles (.res,
//! formatVersion 1 to 3) and ures_open()'s locale fallback.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use goro_core::array::{ArrayKey, PhpArray};
use goro_core::value::Value;
use goro_core::vm::{NativeObjectHandlers, Vm};

use crate::error::{
    self, IntlError, U_ILLEGAL_ARGUMENT_ERROR, U_INDEX_OUTOFBOUNDS_ERROR, U_MISSING_RESOURCE_ERROR,
    U_RESOURCE_TYPE_MISMATCH, U_USING_DEFAULT_WARNING, U_USING_FALLBACK_WARNING, U_ZERO_ERROR,
};
use crate::locale::uloc;
use crate::{Call, Obj, arr, s, sv};

crate::intl_functions! { pub FUNCTIONS:
    "resourcebundle_create" => create,
    "resourcebundle_get" => get_fn,
    "resourcebundle_count" => count,
    "resourcebundle_locales" => locales,
    "resourcebundle_get_error_code" => get_error_code,
    "resourcebundle_get_error_message" => get_error_message,
}

pub const METHODS: &[(&str, crate::Imp)] = &[
    ("ResourceBundle::__construct", construct),
    ("ResourceBundle::get", get_method),
    ("ResourceBundle::getIterator", get_iterator),
];

pub const CLASSES: &[(&str, Option<crate::obj::CreateFn>)] = &[("ResourceBundle", Some(create_object))];

// ---------------------------------------------------------------------------
// The .res reader (uresdata.cpp)

/// A resource item.
#[derive(Debug, Clone)]
pub enum Res {
    Str(Vec<u16>),
    Bin(Vec<u8>),
    Int(i32),
    IntVec(Vec<i32>),
    /// (key, value) in key order
    Table(Vec<(String, Rc<Res>)>),
    Array(Vec<Rc<Res>>),
    Alias(Vec<u16>),
}

/// UResType of an item.
fn res_type(r: &Res) -> i32 {
    match r {
        Res::Str(_) => 0,
        Res::Bin(_) => 1,
        Res::Table(_) => 2,
        Res::Alias(_) => 3,
        Res::Int(_) => 7,
        Res::Array(_) => 8,
        Res::IntVec(_) => 14,
    }
}

/// A loaded bundle file.
#[derive(Debug)]
pub struct Bundle {
    pub root: Rc<Res>,
    pub no_fallback: bool,
}

struct Reader<'a> {
    d: &'a [u8],
    be: bool,
    /// byte offset of the 16-bit units array
    units16: usize,
    /// local key limit (bytes) for 16-bit key offsets
    local_key_limit: usize,
    pool_string_limit: u32,
    depth: u32,
}

impl Reader<'_> {
    fn u16(&self, off: usize) -> Option<u16> {
        let b = self.d.get(off..off + 2)?;
        Some(if self.be {
            u16::from_be_bytes([b[0], b[1]])
        } else {
            u16::from_le_bytes([b[0], b[1]])
        })
    }
    fn u32(&self, off: usize) -> Option<u32> {
        let b = self.d.get(off..off + 4)?;
        Some(if self.be {
            u32::from_be_bytes([b[0], b[1], b[2], b[3]])
        } else {
            u32::from_le_bytes([b[0], b[1], b[2], b[3]])
        })
    }
    fn key(&self, off: usize) -> Option<String> {
        let rest = self.d.get(off..)?;
        let end = rest.iter().position(|&b| b == 0)?;
        Some(String::from_utf8_lossy(&rest[..end]).into_owned())
    }
    fn key16(&self, k: u16) -> Option<String> {
        let k = k as usize;
        if k < self.local_key_limit {
            self.key(k)
        } else {
            None
        }
    }
    fn string_v2(&self, off: usize) -> Option<Vec<u16>> {
        let p = self.units16 + off * 2;
        let first = self.u16(p)?;
        let (len, start) = if (first & 0xfc00) != 0xdc00 {
            // implicit length: NUL-terminated
            let mut n = 0;
            while self.u16(p + n * 2)? != 0 {
                n += 1;
            }
            (n, p)
        } else if first < 0xdfef {
            ((first & 0x3ff) as usize, p + 2)
        } else if first < 0xdfff {
            ((((first - 0xdfef) as usize) << 16) | self.u16(p + 2)? as usize, p + 4)
        } else {
            (((self.u16(p + 2)? as usize) << 16) | self.u16(p + 4)? as usize, p + 6)
        };
        (0..len).map(|i| self.u16(start + i * 2)).collect()
    }
    fn res16(&self, r: u16) -> Option<Res> {
        if (r as u32) < self.pool_string_limit || self.pool_string_limit == 0 {
            Some(Res::Str(self.string_v2(r as usize)?))
        } else {
            None
        }
    }
    fn read(&mut self, res: u32) -> Option<Res> {
        self.depth += 1;
        if self.depth > 64 {
            return None;
        }
        let ty = res >> 28;
        let off = (res & 0x0fff_ffff) as usize;
        let p = off * 4;
        let r = match ty {
            0 | 3 => {
                let v = if off == 0 {
                    Vec::new()
                } else {
                    let len = self.u32(p)? as usize;
                    (0..len).map(|i| self.u16(p + 4 + i * 2)).collect::<Option<Vec<u16>>>()?
                };
                if ty == 0 { Res::Str(v) } else { Res::Alias(v) }
            }
            6 => Res::Str(self.string_v2(off)?),
            1 => {
                if off == 0 {
                    Res::Bin(Vec::new())
                } else {
                    let len = self.u32(p)? as usize;
                    Res::Bin(self.d.get(p + 4..p + 4 + len)?.to_vec())
                }
            }
            7 => Res::Int((((res & 0x0fff_ffff) << 4) as i32) >> 4),
            14 => {
                if off == 0 {
                    Res::IntVec(Vec::new())
                } else {
                    let len = self.u32(p)? as usize;
                    Res::IntVec((0..len).map(|i| self.u32(p + 4 + i * 4).map(|x| x as i32)).collect::<Option<_>>()?)
                }
            }
            2 => {
                let mut items = Vec::new();
                if off != 0 {
                    let n = self.u16(p)? as usize;
                    let vals = p + 2 + n * 2 + if n.is_multiple_of(2) { 2 } else { 0 };
                    for i in 0..n {
                        let k = self.key(self.u16(p + 2 + i * 2)? as usize)?;
                        let v = self.u32(vals + i * 4)?;
                        items.push((k, Rc::new(self.read(v)?)));
                    }
                }
                Res::Table(items)
            }
            4 => {
                let mut items = Vec::new();
                if off != 0 {
                    let n = self.u32(p)? as usize;
                    for i in 0..n {
                        let ko = self.u32(p + 4 + i * 4)? as i32;
                        let k = if ko >= 0 { self.key(ko as usize)? } else { String::new() };
                        let v = self.u32(p + 4 + n * 4 + i * 4)?;
                        items.push((k, Rc::new(self.read(v)?)));
                    }
                }
                Res::Table(items)
            }
            5 => {
                let mut items = Vec::new();
                let q = self.units16 + off * 2;
                if off != 0 {
                    let n = self.u16(q)? as usize;
                    for i in 0..n {
                        let k = self.key16(self.u16(q + 2 + i * 2)?)?;
                        let v = self.u16(q + 2 + n * 2 + i * 2)?;
                        items.push((k, Rc::new(self.res16(v)?)));
                    }
                }
                Res::Table(items)
            }
            8 => {
                let mut items = Vec::new();
                if off != 0 {
                    let n = self.u32(p)? as usize;
                    for i in 0..n {
                        let v = self.u32(p + 4 + i * 4)?;
                        items.push(Rc::new(self.read(v)?));
                    }
                }
                Res::Array(items)
            }
            9 => {
                let mut items = Vec::new();
                let q = self.units16 + off * 2;
                if off != 0 {
                    let n = self.u16(q)? as usize;
                    for i in 0..n {
                        let v = self.u16(q + 2 + i * 2)?;
                        items.push(Rc::new(self.res16(v)?));
                    }
                }
                Res::Array(items)
            }
            _ => return None,
        };
        self.depth -= 1;
        Some(r)
    }
}

/// res_read(): parse a .res file.
pub fn parse_res(file: &[u8]) -> Option<Bundle> {
    // DataHeader: headerSize, magic, UDataInfo
    if file.len() < 32 || file[2] != 0xda || file[3] != 0x27 {
        return None;
    }
    let be = file[8] != 0;
    let hs = if be {
        u16::from_be_bytes([file[0], file[1]])
    } else {
        u16::from_le_bytes([file[0], file[1]])
    } as usize;
    if &file[12..16] != b"ResB" {
        return None;
    }
    let fv = file[16];
    let d = file.get(hs..)?;
    let mut r = Reader {
        d,
        be,
        units16: 0,
        local_key_limit: 0,
        pool_string_limit: 0,
        depth: 0,
    };
    let root = r.u32(0)?;
    let mut no_fallback = false;
    if fv >= 1 {
        let idx_len = (r.u32(4)? & 0xff) as usize;
        let idx: Vec<Option<u32>> = (0..8)
            .map(|i| if i < idx_len { r.u32(4 + i * 4) } else { None })
            .collect();
        let keys_top = idx[1].unwrap_or(0) as usize;
        r.local_key_limit = keys_top * 4;
        if let Some(att) = idx[5] {
            no_fallback = att & 1 != 0;
            if fv >= 3 {
                r.pool_string_limit = att >> 8;
            }
        }
        if fv >= 2 {
            r.units16 = keys_top * 4;
        }
    }
    let root = r.read(root)?;
    Some(Bundle {
        root: Rc::new(root),
        no_fallback,
    })
}

thread_local! {
    static CACHE: RefCell<HashMap<String, Option<Rc<Bundle>>>> = RefCell::new(HashMap::new());
}

#[path = "resourcebundle_data.rs"]
mod data;

fn ustr(s: &str) -> Rc<Res> {
    Rc::new(Res::Str(s.encode_utf16().collect()))
}

fn table(mut items: Vec<(String, Rc<Res>)>) -> Rc<Res> {
    items.sort_by(|a, b| a.0.cmp(&b.0));
    Rc::new(Res::Table(items))
}

type DispTable = &'static [(&'static str, &'static str, &'static str)];

/// The entries of a display-name table for `locale`.
fn disp_entries(t: DispTable, locale: &str) -> Vec<(String, Rc<Res>)> {
    let start = t.partition_point(|(l, _, _)| *l < locale);
    t[start..]
        .iter()
        .take_while(|(l, _, _)| *l == locale)
        .map(|(_, k, v)| (k.to_string(), ustr(v)))
        .collect()
}

/// The ICU data packages ("ICUDATA", "ICUDATA-lang", ...; "" for the
/// default locale data) as far as their data is available here.
fn icu_package(path: &str) -> Option<&str> {
    if path.is_empty() || path == "ICUDATA" {
        return Some("");
    }
    path.strip_prefix("ICUDATA-")
}

/// The locales of an ICU data package.
fn package_locales(pkg: &str) -> Vec<String> {
    match pkg {
        "" => uloc::data::AVAILABLE.iter().map(|s| s.to_string()).collect(),
        "lang" | "region" | "curr" => uloc::data::LOCALES
            .iter()
            .filter(|l| **l != "root")
            .map(|s| s.to_string())
            .collect(),
        "coll" => crate::collator::collation_locale_list(),
        _ => Vec::new(),
    }
}

/// A bundle of an ICU data package, built from the data at hand.
fn icu_bundle(pkg: &str, locale: &str) -> Option<Bundle> {
    let known = locale == "root" || package_locales(pkg).iter().any(|l| l == locale);
    let root = match pkg {
        "" => match locale {
            "icuver" => table(vec![
                ("DataVersion".into(), ustr("78.1.0.0")),
                ("ICUVersion".into(), ustr("78.1.0.0")),
            ]),
            "supplementalData" => {
                return Some(Bundle {
                    root: table(vec![("cldrVersion".into(), ustr("48"))]),
                    no_fallback: true,
                });
            }
            _ if known => {
                let mut items = Vec::new();
                if let Ok(i) = data::DATE_TIME_PATTERNS.binary_search_by(|(l, _)| (*l).cmp(locale)) {
                    let p = data::DATE_TIME_PATTERNS[i].1;
                    let pats = Rc::new(Res::Array(p.iter().map(|x| ustr(x)).collect()));
                    let greg = table(vec![("DateTimePatterns".into(), pats)]);
                    items.push(("calendar".into(), table(vec![("gregorian".into(), greg)])));
                }
                table(items)
            }
            _ => return None,
        },
        "region" if known => table(vec![(
            "Countries".into(),
            table(disp_entries(uloc::data::REGIONS, locale)),
        )]),
        "lang" if known => {
            let mut types: Vec<(String, Vec<(String, Rc<Res>)>)> = Vec::new();
            for (k, v) in disp_entries(uloc::data::TYPES, locale) {
                let (key, ty) = k.split_once('/').unwrap_or((&k, ""));
                match types.iter_mut().find(|(kk, _)| kk == key) {
                    Some((_, l)) => l.push((ty.to_string(), v)),
                    None => types.push((key.to_string(), vec![(ty.to_string(), v)])),
                }
            }
            let mut items = vec![
                ("Languages".into(), table(disp_entries(uloc::data::LANGUAGES, locale))),
                ("Scripts".into(), table(disp_entries(uloc::data::SCRIPTS, locale))),
                (
                    "Scripts%stand-alone".into(),
                    table(disp_entries(uloc::data::SCRIPTS_STANDALONE, locale)),
                ),
                ("Variants".into(), table(disp_entries(uloc::data::VARIANTS, locale))),
                ("Keys".into(), table(disp_entries(uloc::data::KEYS, locale))),
                (
                    "Types".into(),
                    table(types.into_iter().map(|(k, l)| (k, table(l))).collect()),
                ),
            ];
            let pats = disp_entries(uloc::data::PATTERNS, locale);
            if !pats.is_empty() {
                items.push(("localeDisplayPattern".into(), table(pats)));
            }
            items.retain(|(_, v)| !matches!(&**v, Res::Table(t) if t.is_empty()));
            table(items)
        }
        "curr" if known => {
            let cur: Vec<(String, Rc<Res>)> = disp_entries(uloc::data::CURRENCIES, locale)
                .into_iter()
                .map(|(k, v)| {
                    let a = Rc::new(Res::Array(vec![ustr(&k), v]));
                    (k, a)
                })
                .collect();
            table(vec![("Currencies".into(), table(cur))])
        }
        _ => return None,
    };
    Some(Bundle {
        root,
        no_fallback: false,
    })
}

/// A bundle of package `path` for `locale`, loaded from `path/locale.res`
/// (or built for an ICU data package).
fn load(path: &str, locale: &str) -> Option<Rc<Bundle>> {
    if locale.is_empty() {
        return None;
    }
    let pkg = icu_package(path);
    let file = match pkg {
        Some(p) => format!("\0{}\0{}", p, locale),
        None => format!("{}/{}.res", path.trim_end_matches('/'), locale),
    };
    if let Some(b) = CACHE.with(|c| c.borrow().get(&file).cloned()) {
        return b;
    }
    let b = match pkg {
        Some(p) => icu_bundle(p, locale).map(Rc::new),
        None => std::fs::read(&file)
            .ok()
            .and_then(|d| parse_res(&d))
            .map(Rc::new),
    };
    CACHE.with(|c| c.borrow_mut().insert(file, b.clone()));
    b
}

/// The parent of a locale ID for bundle fallback (uloc_getParent()).
fn parent(loc: &str) -> String {
    match loc.rfind('_') {
        Some(p) => loc[..p].to_string(),
        None => String::new(),
    }
}

/// An opened bundle: the bundle and its fallback chain (the bundle first).
#[derive(Clone)]
pub struct Opened {
    pub chain: Vec<(String, Rc<Bundle>)>,
}

/// ures_open() / ures_openDirect(): Ok((bundle, warning code)).
fn open(path: &str, locale: &str, fallback: bool) -> Result<(Opened, i32), i32> {
    let base = uloc::get_base_name(locale.as_bytes()).unwrap_or_else(|_| locale.to_string());
    let base = if base.is_empty() { "root".to_string() } else { base };
    if !fallback {
        return match load(path, &base) {
            Some(b) => Ok((
                Opened {
                    chain: vec![(base, b)],
                },
                U_ZERO_ERROR,
            )),
            None => Err(U_MISSING_RESOURCE_ERROR),
        };
    }
    let mut code = U_ZERO_ERROR;
    let mut found: Option<(String, Rc<Bundle>)> = None;
    let mut cur = base.clone();
    while !cur.is_empty() {
        if let Some(b) = load(path, &cur) {
            found = Some((cur.clone(), b));
            break;
        }
        cur = parent(&cur);
        code = U_USING_FALLBACK_WARNING;
    }
    if found.is_none() {
        let mut cur = crate::common::icu_default_locale();
        while !cur.is_empty() {
            if let Some(b) = load(path, &cur) {
                found = Some((cur.clone(), b));
                break;
            }
            cur = parent(&cur);
        }
        code = U_USING_DEFAULT_WARNING;
    }
    if found.is_none() {
        if let Some(b) = load(path, "root") {
            found = Some(("root".to_string(), b));
        }
        code = U_USING_DEFAULT_WARNING;
    }
    let Some((loc, b)) = found else {
        return Err(U_MISSING_RESOURCE_ERROR);
    };
    if loc == "root" && base == "root" {
        code = U_ZERO_ERROR;
    }
    let mut chain = vec![(loc.clone(), b.clone())];
    let mut cur = loc;
    let mut nf = b.no_fallback;
    while cur != "root" && !nf {
        cur = parent(&cur);
        if cur.is_empty() {
            cur = "root".to_string();
        }
        if let Some(b) = load(path, &cur) {
            nf = b.no_fallback;
            chain.push((cur.clone(), b));
        }
    }
    Ok((Opened { chain }, code))
}

/// The locales of res_index.res (ures_openAvailableLocales()).
fn available_locales(path: &str) -> Option<Vec<String>> {
    if let Some(pkg) = icu_package(path) {
        let l = package_locales(pkg);
        return if l.is_empty() { None } else { Some(l) };
    }
    let b = load(path, "res_index")?;
    let Res::Table(t) = &*b.root else {
        return None;
    };
    let (_, inst) = t.iter().find(|(k, _)| k == "InstalledLocales")?;
    match &**inst {
        Res::Table(l) => Some(l.iter().map(|(k, _)| k.clone()).collect()),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// ResourceBundle objects

/// A resource of an opened bundle (UResourceBundle).
#[derive(Clone)]
pub struct Node {
    /// The bundle (and fallback chain) it comes from.
    pub opened: Rc<Opened>,
    pub res: Rc<Res>,
    /// Whether this is the top-level bundle (which falls back to its
    /// parents for missing keys).
    pub top: bool,
}

#[derive(Default)]
pub struct RbObject {
    pub err: IntlError,
    pub me: Option<Node>,
}

static HANDLERS: NativeObjectHandlers = NativeObjectHandlers {
    read_dimension: Some(read_dimension),
    has_dimension: Some(has_dimension),
    count_elements: Some(count_elements),
    clone_obj: Some(clone_obj),
    ..NativeObjectHandlers::EMPTY
};

fn clone_obj(vm: &mut Vm, src: &Obj, _dst: &Obj) {
    crate::obj::uncloneable(vm, src);
}

fn create_object(_vm: &mut Vm, o: &Obj) {
    crate::obj::attach(o, RbObject::default(), &HANDLERS);
}

fn state(o: &Obj) -> Rc<RefCell<RbObject>> {
    crate::obj::state::<RbObject>(o).unwrap_or_else(|| Rc::new(RefCell::new(RbObject::default())))
}

/// The name get_active_function_name() reports.
fn active_function(vm: &Vm) -> String {
    match vm.call_stack.last() {
        Some((f, ..)) => {
            let f = f.rsplit("->").next().unwrap_or(f);
            f.rsplit("::").next().unwrap_or(f).to_string()
        }
        None => "main".to_string(),
    }
}

/// resourcebundle_ctor(): false on failure.
fn ctor(vm: &mut Vm, c: &Call, o: &Obj) -> bool {
    error::reset_global();
    let st = state(o);
    if st.borrow().me.is_some() {
        crate::zpp::throw(vm, b"Error", "ResourceBundle object is already constructed");
        return false;
    }
    let loc = c.opt_str(0);
    let bundle = c.opt_str(1);
    let fallback = !c.passed(2) || c.bool(2);
    if loc.as_ref().is_some_and(|l| l.len() > crate::common::MAX_LOCALE_LEN) {
        error::set_global(
            vm,
            &c.name,
            U_ILLEGAL_ARGUMENT_ERROR,
            &format!(
                "Locale string too long, should be no longer than {} characters",
                crate::common::MAX_LOCALE_LEN
            ),
        );
        return false;
    }
    let locale = match loc {
        Some(l) => String::from_utf8_lossy(&l).into_owned(),
        None => crate::common::default_locale(vm),
    };
    if bundle.as_ref().is_some_and(|b| b.len() >= 4096) {
        crate::zpp::value_error(vm, &c.name, c.argn(1), "bundle", "is too long");
        return false;
    }
    let path = bundle.map(|b| String::from_utf8_lossy(&b).into_owned());
    let r = open(path.as_deref().unwrap_or(""), &locale, fallback);
    let mut stb = st.borrow_mut();
    match r {
        Ok((opened, code)) => {
            let root = opened.chain[0].1.root.clone();
            stb.me = Some(Node {
                opened: Rc::new(opened),
                res: root,
                top: true,
            });
            stb.err.code = code;
            error::set_global_code(code);
            true
        }
        Err(code) => {
            error::set_both(vm, &c.name, &mut stb.err, code, "Cannot load libICU resource bundle");
            false
        }
    }
}

/// resourcebundle_create()
fn create(vm: &mut Vm, c: &mut Call) -> Value {
    let Some(o) = crate::obj::object_init_ex(vm, b"resourcebundle") else {
        return Value::Null;
    };
    if ctor(vm, c, &o) {
        Value::Object(o)
    } else {
        Value::Null
    }
}

/// ResourceBundle::__construct()
fn construct(vm: &mut Vm, c: &mut Call) -> Value {
    let o = c.obj();
    crate::collator::with_ctor_errors(vm, |vm| ctor(vm, c, &o));
    Value::Null
}

/// ures_getByKey() / ures_getByIndex() on `node`: the child and its
/// warning code.
fn child(node: &Node, key: Option<&str>, index: i64) -> Result<(Node, i32), i32> {
    match (&*node.res, key) {
        (Res::Table(t), Some(k)) => {
            if let Some((_, v)) = t.iter().find(|(kk, _)| kk == k) {
                return Ok((
                    Node {
                        opened: node.opened.clone(),
                        res: v.clone(),
                        top: false,
                    },
                    U_ZERO_ERROR,
                ));
            }
            if node.top {
                for (i, (loc, b)) in node.opened.chain.iter().enumerate().skip(1) {
                    if let Res::Table(t) = &*b.root {
                        if let Some((_, v)) = t.iter().find(|(kk, _)| kk == k) {
                            let code = if loc == "root" || i == node.opened.chain.len() - 1 && loc == "root" {
                                U_USING_DEFAULT_WARNING
                            } else {
                                U_USING_FALLBACK_WARNING
                            };
                            return Ok((
                                Node {
                                    opened: node.opened.clone(),
                                    res: v.clone(),
                                    top: false,
                                },
                                code,
                            ));
                        }
                    }
                }
            }
            Err(U_MISSING_RESOURCE_ERROR)
        }
        (Res::Table(t), None) => match t.get(index as usize).filter(|_| index >= 0) {
            Some((_, v)) => Ok((
                Node {
                    opened: node.opened.clone(),
                    res: v.clone(),
                    top: false,
                },
                U_ZERO_ERROR,
            )),
            None => Err(U_MISSING_RESOURCE_ERROR),
        },
        (Res::Array(a), None) => match a.get(index as usize).filter(|_| index >= 0) {
            Some(v) => Ok((
                Node {
                    opened: node.opened.clone(),
                    res: v.clone(),
                    top: false,
                },
                U_ZERO_ERROR,
            )),
            None => Err(U_MISSING_RESOURCE_ERROR),
        },
        (Res::Array(_), Some(_)) => Err(U_RESOURCE_TYPE_MISMATCH),
        (_, None) if index == 0 => Ok((node.clone(), U_ZERO_ERROR)),
        (_, None) => Err(U_INDEX_OUTOFBOUNDS_ERROR),
        (_, Some(_)) => Err(U_RESOURCE_TYPE_MISMATCH),
    }
}

/// ures_getSize()
fn size(r: &Res) -> i64 {
    match r {
        Res::Table(t) => t.len() as i64,
        Res::Array(a) => a.len() as i64,
        _ => 1,
    }
}

/// resourcebundle_extract_value()
fn extract_value(vm: &mut Vm, node: Node) -> Value {
    match &*node.res {
        Res::Str(u) | Res::Alias(u) => sv(crate::common::from_utf16_lossy(u)),
        Res::Bin(b) => sv(b.clone()),
        Res::Int(i) => Value::Long(*i as i64),
        Res::IntVec(v) => {
            let mut a = PhpArray::new();
            for x in v {
                a.push(Value::Long(*x as i64));
            }
            arr(a)
        }
        Res::Table(_) | Res::Array(_) => {
            let Some(o) = crate::obj::object_init_ex(vm, b"resourcebundle") else {
                return Value::Null;
            };
            state(&o).borrow_mut().me = Some(node);
            Value::Object(o)
        }
    }
}

/// resource_bundle_array_fetch(): None when an exception was thrown.
fn array_fetch(
    vm: &mut Vm,
    o: &Obj,
    func: &str,
    key: Option<&[u8]>,
    index: i64,
    fallback: bool,
    arg_num: usize,
) -> Option<Value> {
    let st = state(o);
    error::reset_global();
    st.borrow_mut().err.reset();
    if let Some(k) = key {
        if k.is_empty() {
            if arg_num > 0 {
                crate::zpp::value_error(vm, func, arg_num, "index", "must not be empty");
            } else {
                crate::zpp::throw(vm, b"ValueError", "Offset must not be empty");
            }
            return None;
        }
    } else if index < i32::MIN as i64 || index > i32::MAX as i64 {
        if arg_num > 0 {
            crate::zpp::value_error(
                vm,
                func,
                arg_num,
                "index",
                &format!("index must be between {} and {}", i32::MIN, i32::MAX),
            );
        } else {
            crate::zpp::throw(
                vm,
                b"ValueError",
                &format!("Index must be between {} and {}", i32::MIN, i32::MAX),
            );
        }
        return None;
    }
    let Some(me) = st.borrow().me.clone() else {
        crate::zpp::throw(vm, b"Error", "Found unconstructed ResourceBundle");
        return None;
    };
    let ks = key.map(|k| String::from_utf8_lossy(k).into_owned());
    let r = child(&me, ks.as_deref(), index);
    let mut stb = st.borrow_mut();
    match r {
        Err(code) => {
            let msg = match &ks {
                Some(k) => format!("Cannot load resource element '{}'", k),
                None => format!("Cannot load resource element {}", index),
            };
            error::set_both(vm, func, &mut stb.err, code, &msg);
            Some(Value::Null)
        }
        Ok((node, code)) => {
            stb.err.code = code;
            error::set_global_code(code);
            if !fallback && (code == U_USING_FALLBACK_WARNING || code == U_USING_DEFAULT_WARNING) {
                let loc = me.opened.chain[0].0.clone();
                let msg = match &ks {
                    Some(k) => format!("Cannot load element '{}' without fallback from to {}", k, loc),
                    None => format!("Cannot load element {} without fallback from to {}", index, loc),
                };
                error::set_both(vm, func, &mut stb.err, code, &msg);
                return Some(Value::Null);
            }
            drop(stb);
            Some(extract_value(vm, node))
        }
    }
}

/// resourcebundle_array_get(): the read_dimension handler.
fn read_dimension(vm: &mut Vm, o: &Obj, offset: Option<&Value>, _mode: u8) -> Option<Value> {
    let Some(off) = offset else {
        crate::zpp::throw(vm, b"Error", "Cannot apply [] to ResourceBundle object");
        return Some(Value::Null);
    };
    let func = active_function(vm);
    let r = match off.deref() {
        Value::Long(i) => array_fetch(vm, o, &func, None, i, true, 0),
        Value::String(k) => array_fetch(vm, o, &func, Some(k.as_bytes()), 0, true, 0),
        v => {
            let cn = goro_core::value::display_class_name(&o.borrow().class_name);
            let ty = match &v {
                Value::Object(x) => goro_core::value::display_class_name(&x.borrow().class_name),
                v => Vm::value_type_name(v).to_string(),
            };
            crate::zpp::throw(vm, b"TypeError", &format!("Cannot access offset of type {} on {}", ty, cn));
            None
        }
    };
    Some(r.unwrap_or(Value::Null))
}

/// zend_std_has_dimension() on an object without ArrayAccess.
fn has_dimension(vm: &mut Vm, o: &Obj, _offset: &Value, _check_empty: u8) -> Option<bool> {
    let cn = goro_core::value::display_class_name(&o.borrow().class_name);
    crate::zpp::throw(vm, b"Error", &format!("Cannot use object of type {} as array", cn));
    Some(false)
}

/// resourcebundle_array_count()
fn count_elements(vm: &mut Vm, o: &Obj) -> Option<i64> {
    let st = state(o);
    let mut stb = st.borrow_mut();
    match &stb.me {
        Some(n) => Some(size(&n.res)),
        None => {
            let f = active_function(vm);
            error::set_both(vm, &f, &mut stb.err, U_ILLEGAL_ARGUMENT_ERROR, "Found unconstructed ResourceBundle");
            Some(0)
        }
    }
}

/// resourcebundle_get()
fn get_fn(vm: &mut Vm, c: &mut Call) -> Value {
    let Value::Object(o) = c.arg(0) else {
        return Value::Null;
    };
    get_common(vm, c, &o, 1, 2)
}

/// ResourceBundle::get()
fn get_method(vm: &mut Vm, c: &mut Call) -> Value {
    let o = c.obj();
    get_common(vm, c, &o, 0, 1)
}

/// The index argument is argument `i` (argument number `arg_num`).
fn get_common(vm: &mut Vm, c: &mut Call, o: &Obj, i: usize, arg_num: usize) -> Value {
    let fallback = !c.passed(i + 1) || c.bool(i + 1);
    let r = match c.arg(i) {
        Value::Long(n) => array_fetch(vm, o, &c.name, None, n, fallback, arg_num),
        v => {
            let k = v.to_php_string();
            array_fetch(vm, o, &c.name, Some(k.as_bytes()), 0, fallback, arg_num)
        }
    };
    r.unwrap_or(Value::Null)
}

/// RESOURCEBUNDLE_METHOD_FETCH_OBJECT
fn fetch(vm: &mut Vm, c: &Call) -> Option<Rc<RefCell<RbObject>>> {
    error::reset_global();
    let st = state(&c.obj());
    st.borrow_mut().err.reset();
    if st.borrow().me.is_none() {
        crate::zpp::throw(vm, b"Error", "Found unconstructed ResourceBundle");
        return None;
    }
    Some(st)
}

/// resourcebundle_count()
fn count(vm: &mut Vm, c: &mut Call) -> Value {
    let Some(st) = fetch(vm, c) else {
        return Value::Null;
    };
    let n = st.borrow().me.as_ref().map(|n| size(&n.res)).unwrap_or(0);
    Value::Long(n)
}

/// resourcebundle_locales()
fn locales(vm: &mut Vm, c: &mut Call) -> Value {
    error::reset_global();
    let b = c.str(0);
    if b.len() >= 4096 {
        crate::zpp::value_error(vm, &c.name, 1, "bundle", "is too long");
        return Value::Null;
    }
    let list = available_locales(&String::from_utf8_lossy(&b));
    match list {
        Some(l) => {
            let mut a = PhpArray::new();
            for x in l {
                a.push(s(x));
            }
            arr(a)
        }
        None => {
            error::set_global(vm, &c.name, U_MISSING_RESOURCE_ERROR, "Cannot fetch locales list");
            Value::False
        }
    }
}

/// resourcebundle_get_error_code()
fn get_error_code(_vm: &mut Vm, c: &mut Call) -> Value {
    Value::Long(state(&c.obj()).borrow().err.code as i64)
}

/// resourcebundle_get_error_message()
fn get_error_message(_vm: &mut Vm, c: &mut Call) -> Value {
    s(state(&c.obj()).borrow().err.message())
}

/// ResourceBundle::getIterator(): an iterator over the items (keys for a
/// table, indexes for an array).
fn get_iterator(vm: &mut Vm, c: &mut Call) -> Value {
    let o = c.obj();
    let me = state(&o).borrow().me.clone();
    let mut a = PhpArray::new();
    if let Some(me) = me {
        let n = size(&me.res);
        let is_table = res_type(&me.res) == 2;
        for i in 0..n {
            let Ok((node, _)) = child(&me, None, i) else {
                break;
            };
            let key = match &*me.res {
                Res::Table(t) if is_table => Some(t[i as usize].0.clone()),
                _ => None,
            };
            let v = extract_value(vm, node);
            match key {
                Some(k) => a.set(ArrayKey::String(goro_core::string::PhpString::from_string(k)), v),
                None => a.set(ArrayKey::Int(i), v),
            }
        }
    }
    let it = vm.spl_instantiate(b"ArrayIterator");
    vm.ao_construct(&it, &[arr(a)]);
    Value::Object(it)
}
