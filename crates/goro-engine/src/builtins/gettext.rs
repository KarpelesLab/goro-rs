//! ext/gettext (PHP manual: Gettext) reading GNU MO catalogs directly
//! (GNU gettext manual, "The Format of GNU MO Files"; plural forms from
//! the "Plural-Forms" header, "Translating plural forms").
//!
//! Lookup follows glibc as PHP on Linux observes it: the catalog is
//! `<dir>/<locale>/<category>/<domain>.mo`, the locale being the one set
//! for the category (setlocale(), else LC_ALL / LC_xxx / LANG), with
//! LANGUAGE's list taking precedence unless the locale is C; locale names
//! fall back from `ll_CC.codeset@mod` to `ll`. Argument checks and
//! messages from the ext/gettext tests and PHP 8.5.

use std::collections::HashMap;
use std::rc::Rc;

use goro_runtime::{PhpStr, Value};

use crate::func::Args;
use crate::registry::Module;
use crate::vm::{R, Vm};

pub static MODULE: Module = Module {
    name: "gettext",
    functions: &[
        ("function textdomain(?string $domain): string", textdomain),
        ("function gettext(string $message): string", gettext),
        ("function _(string $message): string", gettext),
        (
            "function dgettext(string $domain, string $message): string",
            dgettext,
        ),
        (
            "function dcgettext(string $domain, string $message, int $category): string",
            dcgettext,
        ),
        (
            "function bindtextdomain(string $domain, ?string $directory): string|false",
            bindtextdomain,
        ),
        (
            "function ngettext(string $singular, string $plural, int $count): string",
            ngettext,
        ),
        (
            "function dngettext(string $domain, string $singular, string $plural, int $count): string",
            dngettext,
        ),
        (
            "function dcngettext(string $domain, string $singular, string $plural, int $count, int $category): string",
            dcngettext,
        ),
        (
            "function bind_textdomain_codeset(string $domain, ?string $codeset): string|false",
            bind_textdomain_codeset,
        ),
    ],
    listed: true,
    ..Module::EMPTY
};

/// glibc's limits as PHP 8.5 enforces them (observed: 1024-byte domains
/// and 4096-byte messages pass, one more is "too long").
const DOMAIN_MAX: usize = 1024;
const MSGID_MAX: usize = 4096;

const LC_MESSAGES: i64 = 5;
const LC_ALL: i64 = 6;
const CATEGORY_NAMES: [&str; 6] = [
    "LC_CTYPE",
    "LC_NUMERIC",
    "LC_TIME",
    "LC_COLLATE",
    "LC_MONETARY",
    "LC_MESSAGES",
];

#[derive(Default)]
struct State {
    domain: Option<Vec<u8>>,
    bindings: HashMap<Vec<u8>, Vec<u8>>,
    codesets: HashMap<Vec<u8>, Vec<u8>>,
    /// Loaded catalogs by path (None: missing or unreadable).
    catalogs: HashMap<std::path::PathBuf, Option<Rc<Catalog>>>,
}

thread_local! {
    /// Locales set through setlocale(), by category (LC_ALL = 6).
    static LOCALES: std::cell::RefCell<HashMap<i64, Vec<u8>>> = std::cell::RefCell::new(HashMap::new());
}

/// Called by setlocale() when it accepts a locale.
pub fn note_setlocale(category: i64, name: &[u8]) {
    LOCALES.with(|l| {
        let mut l = l.borrow_mut();
        if category == LC_ALL {
            l.clear();
        }
        l.insert(category, name.to_vec());
    });
}

/// The locale name gettext uses for a category.
fn category_locale(cat: i64) -> Vec<u8> {
    let set = LOCALES.with(|l| {
        let l = l.borrow();
        l.get(&cat).or_else(|| l.get(&LC_ALL)).cloned()
    });
    if let Some(s) = set {
        return s;
    }
    let env = |n: &str| std::env::var_os(n).filter(|v| !v.is_empty());
    use std::os::unix::ffi::OsStringExt;
    env("LC_ALL")
        .or_else(|| env(CATEGORY_NAMES[cat as usize]))
        .or_else(|| env("LANG"))
        .map(|v| v.into_vec())
        .unwrap_or_else(|| b"C".to_vec())
}

// ---- MO catalogs ----

struct Catalog {
    map: HashMap<Vec<u8>, Vec<u8>>,
    plural: Option<Plural>,
    nplurals: u64,
}

fn read_u32(b: &[u8], off: usize, le: bool) -> Option<u32> {
    let s: [u8; 4] = b.get(off..off + 4)?.try_into().ok()?;
    Some(if le {
        u32::from_le_bytes(s)
    } else {
        u32::from_be_bytes(s)
    })
}

fn parse_mo(b: &[u8]) -> Option<Catalog> {
    let le = match read_u32(b, 0, true)? {
        0x9504_12de => true,
        0xde12_0495 => false,
        _ => return None,
    };
    let n = read_u32(b, 8, le)? as usize;
    let orig = read_u32(b, 12, le)? as usize;
    let trans = read_u32(b, 16, le)? as usize;
    let entry = |table: usize, i: usize| -> Option<&[u8]> {
        let len = read_u32(b, table + i * 8, le)? as usize;
        let off = read_u32(b, table + i * 8 + 4, le)? as usize;
        b.get(off..off.checked_add(len)?)
    };
    let mut map = HashMap::with_capacity(n);
    for i in 0..n {
        let k = entry(orig, i)?;
        let v = entry(trans, i)?;
        // The key is the singular msgid (the plural follows a NUL).
        let k = match k.iter().position(|&c| c == 0) {
            Some(p) => &k[..p],
            None => k,
        };
        map.insert(k.to_vec(), v.to_vec());
    }
    let mut plural = None;
    let mut nplurals = 2;
    if let Some(h) = map.get(&b""[..]) {
        for line in h.split(|&c| c == b'\n') {
            if let Some(rest) = line.strip_prefix(b"Plural-Forms:") {
                let rest = String::from_utf8_lossy(rest);
                for part in rest.split(';') {
                    let part = part.trim();
                    if let Some(v) = part.strip_prefix("nplurals=") {
                        nplurals = v.trim().parse().unwrap_or(2);
                    } else if let Some(v) = part.strip_prefix("plural=") {
                        plural = Plural::parse(v.trim());
                    }
                }
            }
        }
    }
    Some(Catalog {
        map,
        plural,
        nplurals,
    })
}

// ---- plural expressions (C syntax over n) ----

#[derive(Debug)]
enum Plural {
    N,
    Num(u64),
    Not(Box<Plural>),
    Bin(u8, Box<Plural>, Box<Plural>),
    Cond(Box<Plural>, Box<Plural>, Box<Plural>),
}

struct PParser<'a> {
    s: &'a [u8],
    i: usize,
}

impl Plural {
    fn parse(s: &str) -> Option<Plural> {
        let mut p = PParser { s: s.as_bytes(), i: 0 };
        let e = p.cond()?;
        p.ws();
        (p.i == p.s.len()).then_some(e)
    }

    fn eval(&self, n: u64) -> u64 {
        match self {
            Plural::N => n,
            Plural::Num(v) => *v,
            Plural::Not(e) => (e.eval(n) == 0) as u64,
            Plural::Cond(c, a, b) => {
                if c.eval(n) != 0 {
                    a.eval(n)
                } else {
                    b.eval(n)
                }
            }
            Plural::Bin(op, a, b) => {
                let x = a.eval(n);
                if *op == b'&' {
                    return (x != 0 && b.eval(n) != 0) as u64;
                }
                if *op == b'|' {
                    return (x != 0 || b.eval(n) != 0) as u64;
                }
                let y = b.eval(n);
                match op {
                    b'*' => x.wrapping_mul(y),
                    b'/' => x.checked_div(y).unwrap_or(0),
                    b'%' => x.checked_rem(y).unwrap_or(0),
                    b'+' => x.wrapping_add(y),
                    b'-' => x.wrapping_sub(y),
                    b'<' => (x < y) as u64,
                    b'>' => (x > y) as u64,
                    b'l' => (x <= y) as u64,
                    b'g' => (x >= y) as u64,
                    b'=' => (x == y) as u64,
                    b'!' => (x != y) as u64,
                    _ => 0,
                }
            }
        }
    }
}

impl PParser<'_> {
    fn ws(&mut self) {
        while self.i < self.s.len() && self.s[self.i].is_ascii_whitespace() {
            self.i += 1;
        }
    }
    fn eat(&mut self, t: &[u8]) -> bool {
        self.ws();
        if self.s[self.i..].starts_with(t) {
            self.i += t.len();
            true
        } else {
            false
        }
    }
    fn cond(&mut self) -> Option<Plural> {
        let c = self.bin(0)?;
        if self.eat(b"?") {
            let a = self.cond()?;
            if !self.eat(b":") {
                return None;
            }
            let b = self.cond()?;
            return Some(Plural::Cond(Box::new(c), Box::new(a), Box::new(b)));
        }
        Some(c)
    }
    /// Binary operators by precedence level (|| && ==/!= relational +- */%).
    fn bin(&mut self, level: usize) -> Option<Plural> {
        const LEVELS: &[&[(&[u8], u8)]] = &[
            &[(b"||", b'|')],
            &[(b"&&", b'&')],
            &[(b"==", b'='), (b"!=", b'!')],
            &[(b"<=", b'l'), (b">=", b'g'), (b"<", b'<'), (b">", b'>')],
            &[(b"+", b'+'), (b"-", b'-')],
            &[(b"*", b'*'), (b"/", b'/'), (b"%", b'%')],
        ];
        if level == LEVELS.len() {
            return self.unary();
        }
        let mut l = self.bin(level + 1)?;
        'outer: loop {
            for (tok, op) in LEVELS[level] {
                if self.eat(tok) {
                    let r = self.bin(level + 1)?;
                    l = Plural::Bin(*op, Box::new(l), Box::new(r));
                    continue 'outer;
                }
            }
            return Some(l);
        }
    }
    fn unary(&mut self) -> Option<Plural> {
        if self.eat(b"!") {
            return Some(Plural::Not(Box::new(self.unary()?)));
        }
        if self.eat(b"(") {
            let e = self.cond()?;
            return self.eat(b")").then_some(e);
        }
        self.ws();
        if self.eat(b"n") {
            return Some(Plural::N);
        }
        let st = self.i;
        while self.i < self.s.len() && self.s[self.i].is_ascii_digit() {
            self.i += 1;
        }
        if st == self.i {
            return None;
        }
        std::str::from_utf8(&self.s[st..self.i])
            .ok()?
            .parse()
            .ok()
            .map(Plural::Num)
    }
}

// ---- lookup ----

/// Locale names to try, most specific first: ll_CC.codeset@mod, with the
/// codeset normalised (lower case, alphanumerics only), then without
/// codeset, territory and modifier.
fn locale_variants(name: &[u8]) -> Vec<Vec<u8>> {
    let (rest, modifier) = match name.iter().position(|&c| c == b'@') {
        Some(p) => (&name[..p], Some(&name[p..])),
        None => (name, None),
    };
    let (rest, codeset) = match rest.iter().position(|&c| c == b'.') {
        Some(p) => (&rest[..p], Some(&rest[p + 1..])),
        None => (rest, None),
    };
    let (lang, terr) = match rest.iter().position(|&c| c == b'_') {
        Some(p) => (&rest[..p], Some(&rest[p..])),
        None => (rest, None),
    };
    let norm: Option<Vec<u8>> = codeset.map(|c| {
        let mut v: Vec<u8> = c
            .iter()
            .filter(|b| b.is_ascii_alphanumeric())
            .map(|b| b.to_ascii_lowercase())
            .collect();
        if v.iter().all(|b| b.is_ascii_digit()) {
            v.splice(0..0, b"iso".iter().copied());
        }
        v
    });
    let mut out: Vec<Vec<u8>> = Vec::new();
    let mut push = |v: Vec<u8>| {
        if !out.contains(&v) {
            out.push(v);
        }
    };
    for m in [modifier, None] {
        for t in [terr, None] {
            let mut cs: Vec<Option<Vec<u8>>> = Vec::new();
            if let Some(c) = codeset {
                cs.push(Some(c.to_vec()));
            }
            if let Some(n) = &norm {
                cs.push(Some(n.clone()));
            }
            cs.push(None);
            for c in cs {
                let mut v = lang.to_vec();
                if let Some(t) = t {
                    v.extend_from_slice(t);
                }
                if let Some(c) = &c {
                    v.push(b'.');
                    v.extend_from_slice(c);
                }
                if let Some(m) = m {
                    v.extend_from_slice(m);
                }
                push(v);
            }
        }
    }
    out
}

const DEFAULT_DIR: &[u8] = b"/usr/share/locale";

fn catalog(vm: &mut Vm, path: std::path::PathBuf) -> Option<Rc<Catalog>> {
    if let Some(c) = vm.ext::<State>().catalogs.get(&path) {
        return c.clone();
    }
    let c = std::fs::read(&path)
        .ok()
        .and_then(|b| parse_mo(&b))
        .map(Rc::new);
    vm.ext::<State>().catalogs.insert(path, c.clone());
    c
}

/// The translation of `msgid` (and its plural form for `n`), if any.
fn lookup(vm: &mut Vm, domain: &[u8], msgid: &[u8], plural: Option<(&[u8], u64)>, cat: i64) -> Option<Vec<u8>> {
    if !(0..CATEGORY_NAMES.len() as i64).contains(&cat) || msgid.is_empty() {
        return None;
    }
    let loc = category_locale(cat);
    if loc == b"C" || loc == b"POSIX" || loc.starts_with(b"C.") {
        return None;
    }
    let mut names: Vec<Vec<u8>> = Vec::new();
    if let Some(l) = std::env::var_os("LANGUAGE").filter(|v| !v.is_empty()) {
        use std::os::unix::ffi::OsStrExt;
        for part in l.as_bytes().split(|&c| c == b':') {
            if !part.is_empty() {
                names.push(part.to_vec());
            }
        }
    }
    names.push(loc);
    let dir = vm
        .ext::<State>()
        .bindings
        .get(domain)
        .cloned()
        .unwrap_or_else(|| DEFAULT_DIR.to_vec());
    let cat_name = CATEGORY_NAMES[cat as usize];
    let key = match msgid.iter().position(|&c| c == 0) {
        Some(p) => &msgid[..p],
        None => msgid,
    };
    for name in names {
        for v in locale_variants(&name) {
            let mut p = dir.clone();
            p.push(b'/');
            p.extend_from_slice(&v);
            p.push(b'/');
            p.extend_from_slice(cat_name.as_bytes());
            p.push(b'/');
            p.extend_from_slice(domain);
            p.extend_from_slice(b".mo");
            use std::os::unix::ffi::OsStrExt;
            let path = std::path::PathBuf::from(std::ffi::OsStr::from_bytes(&p));
            let Some(c) = catalog(vm, path) else { continue };
            let Some(t) = c.map.get(key) else { continue };
            let Some((_, n)) = plural else {
                let end = t.iter().position(|&c| c == 0).unwrap_or(t.len());
                return Some(t[..end].to_vec());
            };
            let idx = match &c.plural {
                Some(e) => e.eval(n),
                None => (n != 1) as u64,
            };
            let idx = if idx >= c.nplurals { 0 } else { idx };
            let form = t.split(|&c| c == 0).nth(idx as usize).unwrap_or(&t[..]);
            return Some(form.to_vec());
        }
    }
    None
}

// ---- argument checks ----

fn check_domain(vm: &mut Vm, a: &Args, i: usize, d: &[u8]) -> R<()> {
    if d.len() > DOMAIN_MAX {
        return vm.fthrow(
            a,
            "ValueError",
            &format!("Argument #{} ($domain) is too long", i + 1),
        );
    }
    if d.is_empty() {
        return vm.fthrow(
            a,
            "ValueError",
            &format!("Argument #{} ($domain) must not be empty", i + 1),
        );
    }
    Ok(())
}

fn check_msg(vm: &mut Vm, a: &Args, i: usize, name: &str, m: &[u8]) -> R<()> {
    if m.len() > MSGID_MAX {
        return vm.fthrow(
            a,
            "ValueError",
            &format!("Argument #{} (${}) is too long", i + 1, name),
        );
    }
    Ok(())
}

fn check_category(vm: &mut Vm, a: &Args, i: usize, c: i64) -> R<()> {
    if c == LC_ALL {
        return vm.fthrow(
            a,
            "ValueError",
            &format!("Argument #{} ($category) cannot be LC_ALL", i + 1),
        );
    }
    Ok(())
}

/// The domain as the C library sees it (up to a NUL byte).
fn c_str(b: &[u8]) -> &[u8] {
    match b.iter().position(|&c| c == 0) {
        Some(p) => &b[..p],
        None => b,
    }
}

fn current_domain(vm: &mut Vm) -> Vec<u8> {
    vm.ext::<State>()
        .domain
        .clone()
        .unwrap_or_else(|| b"messages".to_vec())
}

fn result(found: Option<Vec<u8>>, orig: PhpStr) -> Value {
    match found {
        Some(t) => Value::Str(PhpStr::from(t)),
        None => Value::Str(orig),
    }
}

fn plural_result(found: Option<Vec<u8>>, s: PhpStr, p: PhpStr, n: i64) -> Value {
    match found {
        Some(t) => Value::Str(PhpStr::from(t)),
        None if n == 1 => Value::Str(s),
        None => Value::Str(p),
    }
}

// ---- functions ----

fn textdomain(vm: &mut Vm, a: &mut Args) -> R<Value> {
    if let Some(d) = a.opt_str(0) {
        if d.len() > DOMAIN_MAX {
            return vm.fthrow(a, "ValueError", "Argument #1 ($domain) is too long");
        }
        if d.is_empty() {
            return vm.fthrow(a, "ValueError", "Argument #1 ($domain) must not be empty");
        }
        if d.as_bytes() == b"0" {
            return vm.fthrow(a, "ValueError", "Argument #1 ($domain) cannot be zero");
        }
        vm.ext::<State>().domain = Some(c_str(&d).to_vec());
    }
    let d = current_domain(vm);
    Ok(Value::Str(PhpStr::from(d)))
}

fn gettext(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let m = a.str(0);
    check_msg(vm, a, 0, "message", &m)?;
    let d = current_domain(vm);
    let t = lookup(vm, &d, c_str(&m), None, LC_MESSAGES);
    Ok(result(t, m))
}

fn dgettext(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let d = a.str(0);
    let m = a.str(1);
    check_domain(vm, a, 0, &d)?;
    check_msg(vm, a, 1, "message", &m)?;
    let t = lookup(vm, c_str(&d), c_str(&m), None, LC_MESSAGES);
    Ok(result(t, m))
}

fn dcgettext(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let d = a.str(0);
    let m = a.str(1);
    let c = a.int(2);
    check_domain(vm, a, 0, &d)?;
    check_msg(vm, a, 1, "message", &m)?;
    check_category(vm, a, 2, c)?;
    let t = lookup(vm, c_str(&d), c_str(&m), None, c);
    Ok(result(t, m))
}

fn ngettext(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let s = a.str(0);
    let p = a.str(1);
    let n = a.int(2);
    check_msg(vm, a, 0, "singular", &s)?;
    check_msg(vm, a, 1, "plural", &p)?;
    let d = current_domain(vm);
    let t = lookup(vm, &d, c_str(&s), Some((&p, n as u64)), LC_MESSAGES);
    Ok(plural_result(t, s, p, n))
}

fn dngettext(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let d = a.str(0);
    let s = a.str(1);
    let p = a.str(2);
    let n = a.int(3);
    check_domain(vm, a, 0, &d)?;
    check_msg(vm, a, 1, "singular", &s)?;
    check_msg(vm, a, 2, "plural", &p)?;
    let t = lookup(vm, c_str(&d), c_str(&s), Some((&p, n as u64)), LC_MESSAGES);
    Ok(plural_result(t, s, p, n))
}

fn dcngettext(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let d = a.str(0);
    let s = a.str(1);
    let p = a.str(2);
    let n = a.int(3);
    let c = a.int(4);
    check_domain(vm, a, 0, &d)?;
    check_msg(vm, a, 1, "singular", &s)?;
    check_msg(vm, a, 2, "plural", &p)?;
    check_category(vm, a, 4, c)?;
    let t = lookup(vm, c_str(&d), c_str(&s), Some((&p, n as u64)), c);
    Ok(plural_result(t, s, p, n))
}

fn bindtextdomain(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let d = a.str(0);
    if d.len() > DOMAIN_MAX {
        return vm.fthrow(a, "ValueError", "Argument #1 ($domain) is too long");
    }
    if d.is_empty() {
        return vm.fthrow(a, "ValueError", "Argument #1 ($domain) must not be empty");
    }
    if d.as_bytes().contains(&0) {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #1 ($domain) must not contain any null bytes",
        );
    }
    let key = d.to_vec();
    let Some(dir) = a.opt_str(1) else {
        let cur = vm
            .ext::<State>()
            .bindings
            .get(&key)
            .cloned()
            .unwrap_or_else(|| DEFAULT_DIR.to_vec());
        return Ok(Value::Str(PhpStr::from(cur)));
    };
    if dir.as_bytes().contains(&0) {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #2 ($directory) must not contain any null bytes",
        );
    }
    use std::os::unix::ffi::{OsStrExt, OsStringExt};
    let path = if dir.is_empty() {
        vm.cwd.clone()
    } else {
        let p = std::path::Path::new(std::ffi::OsStr::from_bytes(&dir));
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            vm.cwd.join(p)
        }
    };
    let Ok(real) = std::fs::canonicalize(&path) else {
        return Ok(Value::Bool(false));
    };
    let real = real.into_os_string().into_vec();
    let st = vm.ext::<State>();
    st.bindings.insert(key, real.clone());
    st.catalogs.clear();
    Ok(Value::Str(PhpStr::from(real)))
}

fn bind_textdomain_codeset(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let d = a.str(0);
    if d.is_empty() {
        return vm.fthrow(a, "ValueError", "Argument #1 ($domain) must not be empty");
    }
    let key = d.to_vec();
    let st = vm.ext::<State>();
    if let Some(cs) = a.opt_str(1) {
        st.codesets.insert(key.clone(), c_str(&cs).to_vec());
    }
    Ok(match st.codesets.get(&key) {
        Some(cs) => Value::Str(PhpStr::from(cs.clone())),
        None => Value::Bool(false),
    })
}
