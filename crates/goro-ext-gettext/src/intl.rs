//! An emulation of GNU libintl as glibc implements it (dcigettext.c,
//! finddomain.c, l10nflist.c, loadmsgcat.c, plural.y): text domains and
//! their bindings, the catalog search by locale, .mo files and plural
//! forms.

use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::CStr;
use std::path::PathBuf;
use std::rc::Rc;

/// _nl_default_default_domain
pub const DEFAULT_DOMAIN: &[u8] = b"messages";
/// _nl_default_dirname (LOCALEDIR)
pub const DEFAULT_DIRNAME: &[u8] = b"/usr/share/locale";

#[derive(Default)]
struct Binding {
    dirname: Option<Vec<u8>>,
    codeset: Option<Vec<u8>>,
}

/// A loaded .mo catalog (struct loaded_domain).
pub struct Catalog {
    /// original (up to its first NUL) -> translation (plural forms
    /// separated by NULs)
    msgs: HashMap<Vec<u8>, Vec<u8>>,
    charset: Option<Vec<u8>>,
    nplurals: u64,
    plural: Expr,
}

thread_local! {
    static CURRENT_DOMAIN: RefCell<Vec<u8>> = RefCell::new(DEFAULT_DOMAIN.to_vec());
    static BINDINGS: RefCell<HashMap<Vec<u8>, Binding>> = RefCell::new(HashMap::new());
    /// _nl_loaded_domains: catalogs by file (None: not there / invalid)
    static LOADED: RefCell<HashMap<PathBuf, Option<Rc<Catalog>>>> = RefCell::new(HashMap::new());
    /// The known translations (dcigettext.c's search tree): by domain,
    /// category, locale name and msgid, valid while the catalog counter
    /// is unchanged.
    static KNOWN: RefCell<HashMap<(Vec<u8>, i64, Vec<u8>, Vec<u8>), (u64, Vec<u8>, Rc<Catalog>)>> =
        RefCell::new(HashMap::new());
    /// Our part of _nl_msg_cat_cntr (bumped by textdomain() & co).
    static CAT_CNTR: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

fn bump_counter() {
    CAT_CNTR.with(|c| c.set(c.get() + 1));
}

/// _nl_msg_cat_cntr: ours plus the C library's (which setlocale()
/// increments), when it has one.
fn msg_cat_cntr() -> u64 {
    thread_local! {
        static LIBC_CNTR: std::cell::Cell<Option<usize>> = const { std::cell::Cell::new(None) };
    }
    let p = LIBC_CNTR.with(|c| {
        if c.get().is_none() {
            let p = unsafe { libc::dlsym(libc::RTLD_DEFAULT, c"_nl_msg_cat_cntr".as_ptr()) };
            c.set(Some(p as usize));
        }
        c.get().unwrap_or(0)
    });
    let libc_part = if p == 0 {
        0
    } else {
        unsafe { std::ptr::read_volatile(p as *const libc::c_int) as u32 as u64 }
    };
    CAT_CNTR.with(|c| c.get()).wrapping_add(libc_part << 32)
}

/// textdomain()
pub fn textdomain(domain: Option<&[u8]>) -> Vec<u8> {
    CURRENT_DOMAIN.with(|c| {
        let mut c = c.borrow_mut();
        if let Some(d) = domain {
            *c = if d.is_empty() {
                DEFAULT_DOMAIN.to_vec()
            } else {
                d.to_vec()
            };
            bump_counter();
        }
        c.clone()
    })
}

/// bindtextdomain()
pub fn bindtextdomain(domain: &[u8], dirname: Option<&[u8]>) -> Vec<u8> {
    BINDINGS.with(|b| {
        let mut b = b.borrow_mut();
        let new = !b.contains_key(domain);
        let e = b.entry(domain.to_vec()).or_default();
        if let Some(d) = dirname {
            if new || e.dirname.as_deref() != Some(d) {
                bump_counter();
            }
            e.dirname = Some(d.to_vec());
        }
        e.dirname
            .clone()
            .unwrap_or_else(|| DEFAULT_DIRNAME.to_vec())
    })
}

/// bind_textdomain_codeset()
pub fn bind_textdomain_codeset(domain: &[u8], codeset: Option<&[u8]>) -> Option<Vec<u8>> {
    BINDINGS.with(|b| {
        let mut b = b.borrow_mut();
        let e = b.entry(domain.to_vec()).or_default();
        if let Some(c) = codeset {
            if e.codeset.as_deref() != Some(c) {
                bump_counter();
            }
            e.codeset = Some(c.to_vec());
        }
        e.codeset.clone()
    })
}

pub const LC_ALL: i64 = libc::LC_ALL as i64;

/// The name of a locale category.
fn category_name(category: i64) -> &'static str {
    match category as i32 {
        libc::LC_CTYPE => "LC_CTYPE",
        libc::LC_NUMERIC => "LC_NUMERIC",
        libc::LC_TIME => "LC_TIME",
        libc::LC_COLLATE => "LC_COLLATE",
        libc::LC_MONETARY => "LC_MONETARY",
        libc::LC_MESSAGES => "LC_MESSAGES",
        libc::LC_ALL => "LC_ALL",
        libc::LC_PAPER => "LC_PAPER",
        libc::LC_NAME => "LC_NAME",
        libc::LC_ADDRESS => "LC_ADDRESS",
        libc::LC_TELEPHONE => "LC_TELEPHONE",
        libc::LC_MEASUREMENT => "LC_MEASUREMENT",
        libc::LC_IDENTIFICATION => "LC_IDENTIFICATION",
        _ => "LC_XXX",
    }
}

/// __current_locale_name(category)
fn current_locale_name(category: i64) -> Option<Vec<u8>> {
    let p = unsafe { libc::setlocale(category as libc::c_int, std::ptr::null()) };
    if p.is_null() {
        return None;
    }
    Some(unsafe { CStr::from_ptr(p) }.to_bytes().to_vec())
}

/// guess_category_value(): LANGUAGE unless the locale is "C".
fn category_value(category: i64) -> Vec<u8> {
    let locale = current_locale_name(category).unwrap_or_else(|| b"C".to_vec());
    if locale == b"C" {
        return locale;
    }
    match std::env::var_os("LANGUAGE") {
        Some(l) if !l.is_empty() => {
            use std::os::unix::ffi::OsStrExt;
            l.as_bytes().to_vec()
        }
        _ => locale,
    }
}

const XPG_NORM_CODESET: u32 = 1;
const XPG_CODESET: u32 = 2;
const XPG_TERRITORY: u32 = 4;
const XPG_MODIFIER: u32 = 8;

/// _nl_normalize_codeset(): lower case letters and digits ("iso" before
/// an all-digit name).
fn normalize_codeset(c: &[u8]) -> Vec<u8> {
    let mut out: Vec<u8> = c
        .iter()
        .filter(|b| b.is_ascii_alphanumeric())
        .map(|b| b.to_ascii_lowercase())
        .collect();
    if out.iter().all(|b| b.is_ascii_digit()) {
        let mut v = b"iso".to_vec();
        v.append(&mut out);
        out = v;
    }
    out
}

/// _nl_explode_name() + _nl_make_l10nflist(): the locale directory names
/// to try, most specific first.
fn locale_variants(name: &[u8]) -> Vec<Vec<u8>> {
    let lang_end = name
        .iter()
        .position(|&c| c == b'_' || c == b'.' || c == b'@')
        .unwrap_or(name.len());
    let language = &name[..lang_end];
    let mut rest = &name[lang_end..];
    let mut mask = 0;
    let mut territory: &[u8] = b"";
    let mut codeset: &[u8] = b"";
    let mut modifier: &[u8] = b"";
    if rest.first() == Some(&b'_') {
        let end = rest[1..]
            .iter()
            .position(|&c| c == b'.' || c == b'@')
            .map(|p| p + 1)
            .unwrap_or(rest.len());
        territory = &rest[1..end];
        mask |= XPG_TERRITORY;
        rest = &rest[end..];
    }
    if rest.first() == Some(&b'.') {
        let end = rest[1..]
            .iter()
            .position(|&c| c == b'@')
            .map(|p| p + 1)
            .unwrap_or(rest.len());
        codeset = &rest[1..end];
        mask |= XPG_CODESET;
        rest = &rest[end..];
    }
    let norm = normalize_codeset(codeset);
    if mask & XPG_CODESET != 0 && !codeset.is_empty() && norm != codeset {
        mask |= XPG_NORM_CODESET;
    }
    if rest.first() == Some(&b'@') {
        modifier = &rest[1..];
        mask |= XPG_MODIFIER;
    }
    let mut out = Vec::new();
    let mut cnt = mask as i32;
    while cnt >= 0 {
        let c = cnt as u32;
        if c & !mask == 0 && !(c & XPG_CODESET != 0 && c & XPG_NORM_CODESET != 0) {
            let mut n = language.to_vec();
            if c & XPG_TERRITORY != 0 {
                n.push(b'_');
                n.extend_from_slice(territory);
            }
            if c & XPG_CODESET != 0 {
                n.push(b'.');
                n.extend_from_slice(codeset);
            }
            if c & XPG_NORM_CODESET != 0 {
                n.push(b'.');
                n.extend_from_slice(&norm);
            }
            if c & XPG_MODIFIER != 0 {
                n.push(b'@');
                n.extend_from_slice(modifier);
            }
            out.push(n);
        }
        cnt -= 1;
    }
    out
}

fn load_catalog(path: &PathBuf) -> Option<Rc<Catalog>> {
    if let Some(c) = LOADED.with(|l| l.borrow().get(path).cloned()) {
        return c;
    }
    let c = std::fs::read(path)
        .ok()
        .and_then(|d| parse_mo(&d))
        .map(Rc::new);
    LOADED.with(|l| l.borrow_mut().insert(path.clone(), c.clone()));
    c
}

/// _nl_load_domain(): parse a .mo file.
fn parse_mo(d: &[u8]) -> Option<Catalog> {
    if d.len() < 28 {
        return None;
    }
    let magic = u32::from_le_bytes(d[0..4].try_into().ok()?);
    let le = match magic {
        0x950412de => true,
        0xde120495 => false,
        _ => return None,
    };
    let rd = |off: usize| -> Option<u32> {
        let b: [u8; 4] = d.get(off..off + 4)?.try_into().ok()?;
        Some(if le {
            u32::from_le_bytes(b)
        } else {
            u32::from_be_bytes(b)
        })
    };
    let revision = rd(4)?;
    if revision >> 16 > 1 {
        return None;
    }
    let n = rd(8)? as usize;
    let orig = rd(12)? as usize;
    let trans = rd(16)? as usize;
    let mut msgs = HashMap::with_capacity(n);
    for i in 0..n {
        let ol = rd(orig + i * 8)? as usize;
        let oo = rd(orig + i * 8 + 4)? as usize;
        let tl = rd(trans + i * 8)? as usize;
        let to = rd(trans + i * 8 + 4)? as usize;
        let o = d.get(oo..oo + ol)?;
        let t = d.get(to..to + tl)?;
        let key = match o.iter().position(|&c| c == 0) {
            Some(p) => &o[..p],
            None => o,
        };
        msgs.entry(key.to_vec()).or_insert_with(|| t.to_vec());
    }
    let mut cat = Catalog {
        msgs,
        charset: None,
        nplurals: 2,
        plural: Expr::default_plural(),
    };
    if let Some(header) = cat.msgs.get(b"".as_slice()).cloned() {
        if let Some(cs) = header_field(&header, b"charset=") {
            let end = cs
                .iter()
                .position(|&c| c == b' ' || c == b'\t' || c == b'\n' || c == b';')
                .unwrap_or(cs.len());
            cat.charset = Some(cs[..end].to_vec());
        }
        if let Some(pf) = header_line(&header, b"Plural-Forms:") {
            if let Some((np, expr)) = parse_plural_forms(pf) {
                cat.nplurals = np;
                cat.plural = expr;
            }
        }
    }
    Some(cat)
}

/// The text after `key` in the header.
fn header_field<'a>(h: &'a [u8], key: &[u8]) -> Option<&'a [u8]> {
    let p = h.windows(key.len()).position(|w| w == key)?;
    Some(&h[p + key.len()..])
}

/// The rest of the header line starting with `key`.
fn header_line<'a>(h: &'a [u8], key: &[u8]) -> Option<&'a [u8]> {
    let mut start = 0;
    while start < h.len() {
        let end = h[start..]
            .iter()
            .position(|&c| c == b'\n')
            .map(|p| p + start)
            .unwrap_or(h.len());
        let line = &h[start..end];
        if line.starts_with(key) {
            return Some(&line[key.len()..]);
        }
        start = end + 1;
    }
    None
}

/// EXTRACT_PLURAL_EXPRESSION(): "nplurals=N; plural=EXPR;"
fn parse_plural_forms(s: &[u8]) -> Option<(u64, Expr)> {
    let np = header_field(s, b"nplurals=")?;
    let digits: Vec<u8> = np
        .iter()
        .skip_while(|c| c.is_ascii_whitespace())
        .take_while(|c| c.is_ascii_digit())
        .copied()
        .collect();
    let nplurals: u64 = std::str::from_utf8(&digits).ok()?.parse().ok()?;
    let pl = header_field(s, b"plural=")?;
    let expr = Parser { s: pl, i: 0 }.parse()?;
    Some((nplurals, expr))
}

// ---------------------------------------------------------------------------
// Plural expressions (plural.y)
// ---------------------------------------------------------------------------

pub enum Expr {
    Num(u64),
    Var,
    Not(Box<Expr>),
    Bin(u8, Box<Expr>, Box<Expr>),
    Cond(Box<Expr>, Box<Expr>, Box<Expr>),
}

impl Expr {
    /// GERMANIC_PLURAL: n != 1
    fn default_plural() -> Expr {
        Expr::Bin(b'N', Box::new(Expr::Var), Box::new(Expr::Num(1)))
    }

    /// plural_eval() in unsigned long arithmetic.
    fn eval(&self, n: u64) -> u64 {
        match self {
            Expr::Num(v) => *v,
            Expr::Var => n,
            Expr::Not(e) => (e.eval(n) == 0) as u64,
            Expr::Cond(c, a, b) => {
                if c.eval(n) != 0 {
                    a.eval(n)
                } else {
                    b.eval(n)
                }
            }
            Expr::Bin(op, a, b) => {
                let x = a.eval(n);
                match op {
                    b'|' => (x != 0 || b.eval(n) != 0) as u64,
                    b'&' => (x != 0 && b.eval(n) != 0) as u64,
                    _ => {
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
                            b'N' => (x != y) as u64,
                            _ => 0,
                        }
                    }
                }
            }
        }
    }
}

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
}

impl Parser<'_> {
    fn ws(&mut self) {
        while self.i < self.s.len() && matches!(self.s[self.i], b' ' | b'\t') {
            self.i += 1;
        }
    }

    fn peek(&mut self) -> Option<u8> {
        self.ws();
        self.s.get(self.i).copied()
    }

    fn eat(&mut self, tok: &[u8]) -> bool {
        self.ws();
        if self.s[self.i..].starts_with(tok) {
            self.i += tok.len();
            true
        } else {
            false
        }
    }

    fn parse(mut self) -> Option<Expr> {
        let e = self.cond()?;
        // the expression ends at ';', a newline or the end
        match self.peek() {
            None | Some(b';') | Some(b'\n') | Some(b'\r') | Some(0) => Some(e),
            _ => None,
        }
    }

    fn cond(&mut self) -> Option<Expr> {
        let c = self.or()?;
        if self.eat(b"?") {
            let a = self.cond()?;
            if !self.eat(b":") {
                return None;
            }
            let b = self.cond()?;
            return Some(Expr::Cond(Box::new(c), Box::new(a), Box::new(b)));
        }
        Some(c)
    }

    fn or(&mut self) -> Option<Expr> {
        let mut l = self.and()?;
        while self.eat(b"||") {
            let r = self.and()?;
            l = Expr::Bin(b'|', Box::new(l), Box::new(r));
        }
        Some(l)
    }

    fn and(&mut self) -> Option<Expr> {
        let mut l = self.eq()?;
        while self.eat(b"&&") {
            let r = self.eq()?;
            l = Expr::Bin(b'&', Box::new(l), Box::new(r));
        }
        Some(l)
    }

    fn eq(&mut self) -> Option<Expr> {
        let mut l = self.rel()?;
        loop {
            let op = if self.eat(b"==") {
                b'='
            } else if self.eat(b"!=") {
                b'N'
            } else {
                return Some(l);
            };
            let r = self.rel()?;
            l = Expr::Bin(op, Box::new(l), Box::new(r));
        }
    }

    fn rel(&mut self) -> Option<Expr> {
        let mut l = self.add()?;
        loop {
            let op = if self.eat(b"<=") {
                b'l'
            } else if self.eat(b">=") {
                b'g'
            } else if self.eat(b"<") {
                b'<'
            } else if self.eat(b">") {
                b'>'
            } else {
                return Some(l);
            };
            let r = self.add()?;
            l = Expr::Bin(op, Box::new(l), Box::new(r));
        }
    }

    fn add(&mut self) -> Option<Expr> {
        let mut l = self.mul()?;
        loop {
            let op = match self.peek() {
                Some(b'+') => b'+',
                Some(b'-') => b'-',
                _ => return Some(l),
            };
            self.i += 1;
            let r = self.mul()?;
            l = Expr::Bin(op, Box::new(l), Box::new(r));
        }
    }

    fn mul(&mut self) -> Option<Expr> {
        let mut l = self.unary()?;
        loop {
            let op = match self.peek() {
                Some(b'*') => b'*',
                Some(b'/') => b'/',
                Some(b'%') => b'%',
                _ => return Some(l),
            };
            self.i += 1;
            let r = self.unary()?;
            l = Expr::Bin(op, Box::new(l), Box::new(r));
        }
    }

    fn unary(&mut self) -> Option<Expr> {
        match self.peek()? {
            b'!' => {
                self.i += 1;
                Some(Expr::Not(Box::new(self.unary()?)))
            }
            b'(' => {
                self.i += 1;
                let e = self.cond()?;
                if !self.eat(b")") {
                    return None;
                }
                Some(e)
            }
            b'n' => {
                self.i += 1;
                Some(Expr::Var)
            }
            c if c.is_ascii_digit() => {
                let mut v: u64 = 0;
                while let Some(&d) = self.s.get(self.i) {
                    if !d.is_ascii_digit() {
                        break;
                    }
                    v = v.wrapping_mul(10).wrapping_add((d - b'0') as u64);
                    self.i += 1;
                }
                Some(Expr::Num(v))
            }
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Lookup (dcigettext)
// ---------------------------------------------------------------------------

/// The catalog of `domain` for `category`, searched by the locale.
fn find_catalog(domain: &[u8], category: i64) -> Option<Rc<Catalog>> {
    use std::os::unix::ffi::OsStrExt;
    let dirname = BINDINGS
        .with(|b| b.borrow().get(domain).and_then(|e| e.dirname.clone()))
        .unwrap_or_else(|| DEFAULT_DIRNAME.to_vec());
    let mut dir = PathBuf::from(std::ffi::OsStr::from_bytes(&dirname));
    if !dir.is_absolute() {
        if let Ok(cwd) = std::env::current_dir() {
            dir = cwd.join(dir);
        }
    }
    let value = category_value(category);
    let mut file = domain.to_vec();
    file.extend_from_slice(b".mo");
    for single in value.split(|&c| c == b':') {
        if single.is_empty() {
            continue;
        }
        if single == b"C" || single == b"POSIX" {
            break;
        }
        for v in locale_variants(single) {
            let p = dir
                .join(std::ffi::OsStr::from_bytes(&v))
                .join(category_name(category))
                .join(std::ffi::OsStr::from_bytes(&file));
            if let Some(c) = load_catalog(&p) {
                return Some(c);
            }
        }
    }
    None
}

/// The translation converted to the output codeset (the binding's, else
/// the LC_CTYPE one), transliterating what it lacks.
fn convert(domain: &[u8], cat: &Catalog, s: &[u8]) -> Vec<u8> {
    let Some(from) = &cat.charset else {
        return s.to_vec();
    };
    let to = BINDINGS
        .with(|b| b.borrow().get(domain).and_then(|e| e.codeset.clone()))
        .unwrap_or_else(|| {
            let p = unsafe { libc::nl_langinfo(libc::CODESET) };
            if p.is_null() {
                b"ANSI_X3.4-1968".to_vec()
            } else {
                unsafe { CStr::from_ptr(p) }.to_bytes().to_vec()
            }
        });
    if normalize_codeset(from) == normalize_codeset(&to) || s.is_ascii() {
        return s.to_vec();
    }
    let mut spec = to.clone();
    spec.extend_from_slice(b"//TRANSLIT");
    let Some(mut cd) = goro_ext_iconv::conv::Iconv::open(&spec, from) else {
        return s.to_vec();
    };
    let mut out = Vec::new();
    let mut pos = 0;
    // (what cannot be converted is cut off)
    let _ = cd.conv(s, &mut pos, &mut out, None);
    let _ = cd.flush(&mut out, None);
    out
}

/// dcigettext(): Some(translation) or None for "not translated".
pub fn dcigettext(
    domain: Option<&[u8]>,
    msgid1: &[u8],
    plural: Option<(&[u8], u64)>,
    category: i64,
) -> Option<Vec<u8>> {
    let domain = match domain {
        Some(d) => d.to_vec(),
        None => CURRENT_DOMAIN.with(|c| c.borrow().clone()),
    };
    let msgid1 = super_cstr(msgid1);
    let localename = current_locale_name(category).unwrap_or_default();
    let key = (domain.clone(), category, localename, msgid1.to_vec());
    let cntr = msg_cat_cntr();
    let known = KNOWN.with(|k| {
        k.borrow()
            .get(&key)
            .filter(|e| e.0 == cntr)
            .map(|e| (e.1.clone(), e.2.clone()))
    });
    let (t, cat) = match known {
        Some(x) => x,
        None => {
            let cat = find_catalog(&domain, category)?;
            let t = cat.msgs.get(msgid1)?.clone();
            KNOWN.with(|k| k.borrow_mut().insert(key, (cntr, t.clone(), cat.clone())));
            (t, cat)
        }
    };
    let t = &t;
    let mut r: &[u8] = t;
    if let Some((_, n)) = plural {
        let mut index = cat.plural.eval(n);
        if index >= cat.nplurals {
            index = 0;
        }
        let mut p = 0;
        while index > 0 {
            match t[p..].iter().position(|&c| c == 0) {
                Some(z) => p += z + 1,
                None => p = t.len(),
            }
            if p >= t.len() {
                p = 0;
                break;
            }
            index -= 1;
        }
        r = &t[p..];
    }
    let r = super_cstr(r);
    Some(convert(&domain, &cat, r))
}

/// A C string view.
fn super_cstr(b: &[u8]) -> &[u8] {
    match b.iter().position(|&c| c == 0) {
        Some(p) => &b[..p],
        None => b,
    }
}
