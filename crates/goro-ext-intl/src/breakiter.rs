//! ext/intl/breakiterator: IntlBreakIterator, IntlRuleBasedBreakIterator,
//! IntlCodePointBreakIterator, IntlPartsIterator.
//!
//! Offsets are UTF-8 byte offsets into the text (php-src iterates a UText
//! over the UTF-8 string): ill-formed sequences count as U+FFFD, an offset
//! inside a character snaps back to its start. The boundaries of a text are
//! computed when it is set (intl's UAX #29 / UAX #14 segmenters, or the
//! rules of an IntlRuleBasedBreakIterator), each with its rule status
//! values.

use std::cell::RefCell;
use std::rc::Rc;

use goro_core::array::{ArrayKey, PhpArray};
use goro_core::value::Value;
use goro_core::vm::{NativeObjectHandlers, Vm};
use intl::unicode as un;

use crate::breakiter_rules::{self, Rules};
use crate::error::{self, IntlError};
use crate::iterator::{self, Source};
use crate::zpp::Obj;
use crate::{Call, obj, s, sv};

pub const DONE: i64 = -1;

#[derive(Clone)]
pub enum Kind {
    Char,
    Word,
    Line,
    Sentence,
    Title,
    CodePoint,
    Rules(Rc<Rules>),
}

impl Kind {
    fn same(&self, o: &Kind) -> bool {
        match (self, o) {
            (Kind::Rules(a), Kind::Rules(b)) => Rc::ptr_eq(a, b) || a.source == b.source,
            (Kind::Char, Kind::Char)
            | (Kind::Word, Kind::Word)
            | (Kind::Line, Kind::Line)
            | (Kind::Sentence, Kind::Sentence)
            | (Kind::Title, Kind::Title)
            | (Kind::CodePoint, Kind::CodePoint) => true,
            _ => false,
        }
    }
}

/// BreakIterator_object
#[derive(Clone, Default)]
pub struct Bi {
    /// None: not constructed.
    pub kind: Option<Kind>,
    /// The text set (getText()).
    pub text: Option<Vec<u8>>,
    /// The code points of the text with their offsets.
    chars: Vec<(usize, u32)>,
    /// The boundaries and their rule statuses (rule-based kinds).
    bounds: Vec<(usize, Vec<i32>)>,
    /// Index of the current boundary in `bounds`.
    idx: usize,
    /// The position of a code point iterator.
    pos: usize,
    last_cp: i64,
    pub err: IntlError,
    locale: String,
}

impl Bi {
    fn len(&self) -> usize {
        self.text.as_ref().map_or(0, |t| t.len())
    }

    fn is_cp(&self) -> bool {
        matches!(self.kind, Some(Kind::CodePoint))
    }

    /// utext_setNativeIndex(): pinned to the text, snapped back to a code
    /// point start.
    fn snap(&self, off: i64) -> usize {
        if off <= 0 {
            return 0;
        }
        let len = self.len();
        if off as usize >= len {
            return len;
        }
        let off = off as usize;
        match self.chars.binary_search_by(|c| c.0.cmp(&off)) {
            Ok(_) => off,
            Err(i) => self.chars[i - 1].0,
        }
    }

    fn set_text(&mut self, t: Vec<u8>) {
        self.chars = decode_native(&t);
        self.text = Some(t);
        self.compute();
        self.idx = 0;
        self.pos = 0;
        self.last_cp = -1;
    }

    fn compute(&mut self) {
        let len = self.len();
        let kind = self.kind.clone();
        self.bounds = match kind {
            None | Some(Kind::CodePoint) => vec![(0, vec![0])],
            Some(Kind::Rules(r)) => r.boundaries(&self.chars, len),
            Some(k) => builtin_bounds(&k, &self.chars, len),
        };
    }

    fn cp_at(&self, off: usize) -> Option<(u32, usize)> {
        let i = self.chars.binary_search_by(|c| c.0.cmp(&off)).ok()?;
        let end = self.chars.get(i + 1).map_or(self.len(), |c| c.0);
        Some((self.chars[i].1, end))
    }

    fn cp_before(&self, off: usize) -> Option<(u32, usize)> {
        if off == 0 {
            return None;
        }
        let i = match self.chars.binary_search_by(|c| c.0.cmp(&off)) {
            Ok(i) => i,
            Err(i) => i,
        };
        let c = self.chars.get(i.checked_sub(1)?)?;
        Some((c.1, c.0))
    }

    pub fn current(&self) -> i64 {
        if self.is_cp() {
            return self.pos as i64;
        }
        self.bounds.get(self.idx).map_or(0, |b| b.0 as i64)
    }

    pub fn first(&mut self) -> i64 {
        if self.is_cp() {
            self.pos = 0;
            self.last_cp = -1;
            return 0;
        }
        self.idx = 0;
        self.current()
    }

    pub fn last(&mut self) -> i64 {
        if self.is_cp() {
            self.pos = self.len();
            self.last_cp = -1;
            return self.pos as i64;
        }
        self.idx = self.bounds.len().saturating_sub(1);
        self.current()
    }

    pub fn next_boundary(&mut self) -> i64 {
        if self.is_cp() {
            return match self.cp_at(self.pos) {
                Some((c, end)) => {
                    self.last_cp = c as i64;
                    self.pos = end;
                    end as i64
                }
                None => {
                    self.last_cp = -1;
                    DONE
                }
            };
        }
        if self.idx + 1 < self.bounds.len() {
            self.idx += 1;
            self.current()
        } else {
            DONE
        }
    }

    pub fn previous(&mut self) -> i64 {
        if self.is_cp() {
            return match self.cp_before(self.pos) {
                Some((c, start)) => {
                    self.last_cp = c as i64;
                    self.pos = start;
                    start as i64
                }
                None => {
                    self.last_cp = -1;
                    DONE
                }
            };
        }
        if self.idx > 0 {
            self.idx -= 1;
            self.current()
        } else {
            DONE
        }
    }

    fn following(&mut self, off: i64) -> i64 {
        if self.is_cp() {
            self.pos = self.snap(off);
            return self.next_boundary();
        }
        if off < 0 {
            return self.first();
        }
        let p = self.snap(off);
        match self.bounds.iter().position(|b| b.0 > p) {
            Some(i) => {
                self.idx = i;
                self.current()
            }
            None => {
                self.last();
                DONE
            }
        }
    }

    fn preceding(&mut self, off: i64) -> i64 {
        if self.is_cp() {
            self.pos = self.snap(off);
            return self.previous();
        }
        if off > self.len() as i64 {
            return self.last();
        }
        let p = self.snap(off);
        match self.bounds.iter().rposition(|b| b.0 < p) {
            Some(i) => {
                self.idx = i;
                self.current()
            }
            None => {
                self.first();
                DONE
            }
        }
    }

    fn is_boundary(&mut self, off: i64) -> bool {
        if self.is_cp() {
            self.pos = self.snap(off);
            return off == self.pos as i64;
        }
        if off < 0 {
            self.first();
            return false;
        }
        let p = self.snap(off);
        match self.bounds.iter().position(|b| b.0 == p) {
            Some(i) => {
                self.idx = i;
                if p as i64 == off {
                    return true;
                }
                self.next_boundary();
                false
            }
            None => {
                let i = self.bounds.iter().position(|b| b.0 > p);
                self.idx = i.unwrap_or(self.bounds.len().saturating_sub(1));
                false
            }
        }
    }

    fn next_n(&mut self, n: i64) -> i64 {
        if self.is_cp() {
            // utext_moveIndex32()
            let mut ok = true;
            if n > 0 {
                for _ in 0..n {
                    match self.cp_at(self.pos) {
                        Some((_, end)) => self.pos = end,
                        None => {
                            ok = false;
                            break;
                        }
                    }
                }
            } else {
                for _ in 0..(-n) {
                    match self.cp_before(self.pos) {
                        Some((_, st)) => self.pos = st,
                        None => {
                            ok = false;
                            break;
                        }
                    }
                }
            }
            if ok {
                self.last_cp = self.cp_at(self.pos).map_or(-1, |c| c.0 as i64);
                return self.pos as i64;
            }
            self.last_cp = -1;
            return DONE;
        }
        let mut r = 0;
        if n > 0 {
            let mut n = n;
            while n > 0 && r != DONE {
                r = self.next_boundary();
                n -= 1;
            }
        } else if n < 0 {
            let mut n = n;
            while n < 0 && r != DONE {
                r = self.previous();
                n += 1;
            }
        } else {
            r = self.current();
        }
        r
    }

    fn statuses(&self) -> Vec<i32> {
        if self.is_cp() {
            return vec![0];
        }
        self.bounds
            .get(self.idx)
            .map_or_else(|| vec![0], |b| b.1.clone())
    }

    pub fn rule_status(&self) -> i64 {
        self.statuses().iter().copied().max().unwrap_or(0) as i64
    }
}

/// The code points of `t` with their offsets (U+FFFD for each maximal
/// ill-formed subsequence).
pub fn decode_native(t: &[u8]) -> Vec<(usize, u32)> {
    let mut out = Vec::with_capacity(t.len());
    let mut i = 0;
    while i < t.len() {
        let (cp, n) = u8_next(&t[i..]);
        out.push((i, cp));
        i += n;
    }
    out
}

/// U8_NEXT_OR_FFFD
fn u8_next(s: &[u8]) -> (u32, usize) {
    let b = s[0];
    if b < 0x80 {
        return (b as u32, 1);
    }
    let (n, init) = match b {
        0xC2..=0xDF => (2, (b & 0x1F) as u32),
        0xE0..=0xEF => (3, (b & 0x0F) as u32),
        0xF0..=0xF4 => (4, (b & 0x07) as u32),
        _ => return (0xFFFD, 1),
    };
    let mut c = init;
    for i in 1..n {
        let Some(&t) = s.get(i) else {
            return (0xFFFD, i);
        };
        let ok = match (i, b) {
            (1, 0xE0) => (0xA0..=0xBF).contains(&t),
            (1, 0xED) => (0x80..=0x9F).contains(&t),
            (1, 0xF0) => (0x90..=0xBF).contains(&t),
            (1, 0xF4) => (0x80..=0x8F).contains(&t),
            _ => t & 0xC0 == 0x80,
        };
        if !ok {
            return (0xFFFD, i);
        }
        c = (c << 6) | (t & 0x3F) as u32;
    }
    (c, n)
}

/// The boundaries of the built-in iterators.
fn builtin_bounds(kind: &Kind, chars: &[(usize, u32)], len: usize) -> Vec<(usize, Vec<i32>)> {
    // The text as a String, with the native offset of each byte position
    // at a character start.
    let mut st = String::with_capacity(len);
    let mut map: Vec<usize> = Vec::with_capacity(len + 1);
    for &(off, cp) in chars {
        let c = char::from_u32(cp).unwrap_or('\u{FFFD}');
        for _ in 0..c.len_utf8() {
            map.push(off);
        }
        st.push(c);
    }
    map.push(len);
    let mut out = vec![(0usize, vec![0])];
    let mut at = 0usize;
    let mut push = |seg: &str, status: i32, at: &mut usize| {
        *at += seg.len();
        out.push((map[*at], vec![status]));
    };
    match kind {
        Kind::Char => {
            for g in un::graphemes(&st) {
                push(g, 0, &mut at);
            }
        }
        Kind::Word | Kind::Title => {
            for w in un::words(&st) {
                push(w, word_status(w), &mut at);
            }
        }
        Kind::Line => {
            for l in un::line_breaks(&st) {
                let hard = l.mandatory
                    || l.text.ends_with([
                        '\n', '\r', '\u{0B}', '\u{0C}', '\u{85}', '\u{2028}', '\u{2029}',
                    ]);
                push(l.text, if hard { 100 } else { 0 }, &mut at);
            }
        }
        Kind::Sentence => {
            for s in un::sentences(&st) {
                let term = s.chars().any(|c| {
                    matches!(c, '.' | '!' | '?' | '\u{3002}' | '\u{FF01}' | '\u{FF1F}' | '\u{2026}')
                });
                let sep = s
                    .trim_end_matches([' ', '\t'])
                    .ends_with(['\n', '\r', '\u{85}', '\u{2028}', '\u{2029}']);
                push(s, if !term && sep { 100 } else { 0 }, &mut at);
            }
        }
        _ => {}
    }
    out
}

/// The rule status of a word: UBRK_WORD_*.
fn word_status(w: &str) -> i32 {
    let mut letter = false;
    let mut number = false;
    for c in w.chars() {
        match un::script(c) {
            un::Script::Hiragana | un::Script::Katakana => return 300,
            un::Script::Han => return 400,
            _ => {}
        }
        if crate::uset::is_ideographic(c) {
            return 400;
        }
        if un::is_alphabetic(c) {
            letter = true;
        } else if un::is_numeric(c) {
            number = true;
        }
    }
    if letter {
        200
    } else if number {
        100
    } else {
        0
    }
}

// ---------------------------------------------------------------------------
// Object handlers

pub static HANDLERS: NativeObjectHandlers = NativeObjectHandlers {
    clone_obj: Some(clone_obj),
    compare: Some(compare),
    get_properties: Some(debug_info),
    ..NativeObjectHandlers::EMPTY
};

fn create(_vm: &mut Vm, o: &Obj) {
    obj::attach(
        o,
        Bi {
            last_cp: -1,
            bounds: vec![(0, vec![0])],
            ..Bi::default()
        },
        &HANDLERS,
    );
}

fn clone_obj(vm: &mut Vm, src: &Obj, dst: &Obj) {
    let Some(st) = obj::state::<Bi>(src) else {
        return;
    };
    if st.borrow().kind.is_none() {
        crate::zpp::throw(vm, b"Error", "Cannot clone uninitialized BreakIterator");
    }
    obj::clone_state::<Bi>(src, dst, |b| {
        let mut c = b.clone();
        c.err = IntlError::default();
        c
    });
}

fn compare(a: &Value, b: &Value) -> Option<i64> {
    let (Value::Object(x), Value::Object(y)) = (a.deref(), b.deref()) else {
        return None;
    };
    let sx = obj::state::<Bi>(&x)?;
    let sy = obj::state::<Bi>(&y)?;
    if Rc::ptr_eq(&sx, &sy) {
        return Some(0);
    }
    let (p, q) = (sx.borrow(), sy.borrow());
    let eq = match (&p.kind, &q.kind) {
        (None, None) => true,
        (Some(k1), Some(k2)) => {
            k1.same(k2) && p.text == q.text && p.current() == q.current()
        }
        _ => false,
    };
    Some(if eq { 0 } else { 1 })
}

fn debug_info(_vm: &mut Vm, o: &Obj, purpose: u8) -> Option<PhpArray> {
    if purpose != 0 {
        return None;
    }
    let st = obj::state::<Bi>(o)?;
    let b = st.borrow();
    let mut a = PhpArray::new();
    let k = |n: &str| ArrayKey::String(goro_core::string::PhpString::from_bytes(n.as_bytes()));
    let Some(kind) = &b.kind else {
        a.set(k("valid"), Value::False);
        return Some(a);
    };
    a.set(k("valid"), Value::True);
    a.set(
        k("text"),
        match &b.text {
            Some(t) => sv(t.clone()),
            None => Value::Null,
        },
    );
    let ty = match kind {
        Kind::CodePoint => "N3PHP22CodePointBreakIteratorE",
        _ => "N6icu_7822RuleBasedBreakIteratorE",
    };
    a.set(k("type"), s(ty));
    Some(a)
}

// ---------------------------------------------------------------------------
// Methods

crate::intl_functions! { pub FUNCTIONS: }

pub const METHODS: &[(&str, crate::Imp)] = &[
    ("IntlBreakIterator::__construct", ctor_private),
    ("IntlBreakIterator::createWordInstance", create_word),
    ("IntlBreakIterator::createLineInstance", create_line),
    ("IntlBreakIterator::createCharacterInstance", create_char),
    ("IntlBreakIterator::createSentenceInstance", create_sentence),
    ("IntlBreakIterator::createTitleInstance", create_title),
    ("IntlBreakIterator::createCodePointInstance", create_codepoint),
    ("IntlBreakIterator::getText", get_text),
    ("IntlBreakIterator::setText", set_text),
    ("IntlBreakIterator::first", first),
    ("IntlBreakIterator::last", last),
    ("IntlBreakIterator::previous", previous),
    ("IntlBreakIterator::next", next),
    ("IntlBreakIterator::current", current),
    ("IntlBreakIterator::following", following),
    ("IntlBreakIterator::preceding", preceding),
    ("IntlBreakIterator::isBoundary", is_boundary),
    ("IntlBreakIterator::getLocale", get_locale),
    ("IntlBreakIterator::getPartsIterator", get_parts_iterator),
    ("IntlBreakIterator::getErrorCode", get_error_code),
    ("IntlBreakIterator::getErrorMessage", get_error_message),
    ("IntlBreakIterator::getIterator", get_iterator),
    ("IntlRuleBasedBreakIterator::__construct", rbbi_construct),
    ("IntlRuleBasedBreakIterator::getRules", get_rules),
    ("IntlRuleBasedBreakIterator::getRuleStatus", get_rule_status),
    ("IntlRuleBasedBreakIterator::getRuleStatusVec", get_rule_status_vec),
    ("IntlRuleBasedBreakIterator::getBinaryRules", get_binary_rules),
    ("IntlCodePointBreakIterator::getLastCodePoint", get_last_code_point),
    ("IntlPartsIterator::getBreakIterator", parts_get_break_iterator),
    ("IntlPartsIterator::getRuleStatus", parts_get_rule_status),
];

pub const CLASSES: &[(&str, Option<crate::obj::CreateFn>)] = &[
    ("IntlBreakIterator", Some(create)),
    ("IntlRuleBasedBreakIterator", Some(create)),
    ("IntlCodePointBreakIterator", Some(create)),
];

/// BREAKITER_METHOD_FETCH_OBJECT
fn fetch(vm: &mut Vm, c: &Call) -> Option<Rc<RefCell<Bi>>> {
    let o = c.obj();
    let st = obj::state::<Bi>(&o)?;
    st.borrow_mut().err.reset();
    if st.borrow().kind.is_none() {
        crate::zpp::throw(vm, b"Error", "Found unconstructed BreakIterator");
        return None;
    }
    Some(st)
}

/// The break iterator state of an object.
pub fn state_of(o: &Obj) -> Option<Rc<RefCell<Bi>>> {
    obj::state::<Bi>(o)
}

/// The private IntlBreakIterator::__construct(): only reachable by `new`.
fn ctor_private(vm: &mut Vm, _c: &mut Call) -> Value {
    let scope = match vm.current_class_scope() {
        Some(c) => format!(
            "scope {}",
            goro_core::value::display_class_name(&vm.classes.get(&c).map(|d| d.name.clone()).unwrap_or(c))
        ),
        None => "global scope".to_string(),
    };
    let msg = format!("Call to private IntlBreakIterator::__construct() from {}", scope);
    crate::zpp::throw(vm, b"Error", &msg);
    Value::Null
}

fn factory(vm: &mut Vm, c: &mut Call, kind: Kind) -> Value {
    error::reset_global();
    let loc = c
        .opt_str(0)
        .map(|l| String::from_utf8_lossy(&l).into_owned())
        .unwrap_or_else(|| crate::common::default_locale(vm));
    let cls: &[u8] = match kind {
        Kind::CodePoint => b"intlcodepointbreakiterator",
        _ => b"intlrulebasedbreakiterator",
    };
    let Some(o) = obj::object_init_ex(vm, cls) else {
        return Value::Null;
    };
    if let Some(st) = obj::state::<Bi>(&o) {
        let mut b = st.borrow_mut();
        b.kind = Some(kind);
        b.locale = valid_locale(&loc);
        b.compute();
    }
    Value::Object(o)
}

/// The valid locale of a break iterator for the requested one.
fn valid_locale(loc: &str) -> String {
    let base = loc.split('@').next().unwrap_or("").replace('-', "_");
    let parts: Vec<&str> = base.split('_').filter(|p| !p.is_empty()).collect();
    match parts.first() {
        None => String::new(),
        Some(l) => {
            let mut out = l.to_ascii_lowercase();
            for p in &parts[1..] {
                out.push('_');
                if p.len() == 4 {
                    let mut t = p.to_ascii_lowercase();
                    t[..1].make_ascii_uppercase();
                    out.push_str(&t);
                } else {
                    out.push_str(&p.to_ascii_uppercase());
                }
            }
            out
        }
    }
}

fn create_word(vm: &mut Vm, c: &mut Call) -> Value {
    factory(vm, c, Kind::Word)
}
fn create_line(vm: &mut Vm, c: &mut Call) -> Value {
    factory(vm, c, Kind::Line)
}
fn create_char(vm: &mut Vm, c: &mut Call) -> Value {
    factory(vm, c, Kind::Char)
}
fn create_sentence(vm: &mut Vm, c: &mut Call) -> Value {
    factory(vm, c, Kind::Sentence)
}
fn create_title(vm: &mut Vm, c: &mut Call) -> Value {
    factory(vm, c, Kind::Title)
}
fn create_codepoint(vm: &mut Vm, c: &mut Call) -> Value {
    factory(vm, c, Kind::CodePoint)
}

fn get_text(vm: &mut Vm, c: &mut Call) -> Value {
    let Some(st) = fetch(vm, c) else {
        return Value::Null;
    };
    match &st.borrow().text {
        Some(t) => sv(t.clone()),
        None => Value::Null,
    }
}

fn set_text(vm: &mut Vm, c: &mut Call) -> Value {
    let t = c.str(0);
    let Some(st) = fetch(vm, c) else {
        return Value::Null;
    };
    st.borrow_mut().set_text(t);
    Value::True
}

fn int32_arg(vm: &mut Vm, c: &Call, i: usize) -> Option<i64> {
    let v = c.long(i);
    if v < i32::MIN as i64 || v > i32::MAX as i64 {
        let msg = format!("must be between {} and {}", i32::MIN, i32::MAX);
        let pname = match c.name.as_str() {
            n if n.ends_with("isBoundary") || n.ends_with("following") || n.ends_with("preceding") => {
                "offset"
            }
            _ => "offset",
        };
        crate::zpp::value_error(vm, &c.name, 1, pname, &msg);
        return None;
    }
    Some(v)
}

fn first(vm: &mut Vm, c: &mut Call) -> Value {
    match fetch(vm, c) {
        Some(st) => Value::Long(st.borrow_mut().first()),
        None => Value::Null,
    }
}

fn last(vm: &mut Vm, c: &mut Call) -> Value {
    match fetch(vm, c) {
        Some(st) => Value::Long(st.borrow_mut().last()),
        None => Value::Null,
    }
}

fn previous(vm: &mut Vm, c: &mut Call) -> Value {
    match fetch(vm, c) {
        Some(st) => Value::Long(st.borrow_mut().previous()),
        None => Value::Null,
    }
}

fn next(vm: &mut Vm, c: &mut Call) -> Value {
    let n = c.opt_long(0);
    if n.is_some() {
        int32_arg(vm, c, 0);
        if vm.current_exception.is_some() {
            return Value::Null;
        }
    }
    let Some(st) = fetch(vm, c) else {
        return Value::Null;
    };
    let r = match n {
        None => st.borrow_mut().next_boundary(),
        Some(n) => st.borrow_mut().next_n(n),
    };
    Value::Long(r)
}

fn current(vm: &mut Vm, c: &mut Call) -> Value {
    match fetch(vm, c) {
        Some(st) => Value::Long(st.borrow().current()),
        None => Value::Null,
    }
}

fn following(vm: &mut Vm, c: &mut Call) -> Value {
    let Some(st) = fetch(vm, c) else {
        return Value::Null;
    };
    let Some(off) = int32_arg(vm, c, 0) else {
        return Value::Null;
    };
    Value::Long(st.borrow_mut().following(off))
}

fn preceding(vm: &mut Vm, c: &mut Call) -> Value {
    let Some(st) = fetch(vm, c) else {
        return Value::Null;
    };
    let Some(off) = int32_arg(vm, c, 0) else {
        return Value::Null;
    };
    Value::Long(st.borrow_mut().preceding(off))
}

fn is_boundary(vm: &mut Vm, c: &mut Call) -> Value {
    let Some(off) = int32_arg(vm, c, 0) else {
        return Value::Null;
    };
    let Some(st) = fetch(vm, c) else {
        return Value::Null;
    };
    crate::bool_v(st.borrow_mut().is_boundary(off))
}

fn get_locale(vm: &mut Vm, c: &mut Call) -> Value {
    let t = c.long(0);
    if t != 0 && t != 1 {
        error::set_global(vm, &c.name, error::U_ILLEGAL_ARGUMENT_ERROR, "invalid locale type");
        return Value::False;
    }
    let Some(st) = fetch(vm, c) else {
        return Value::Null;
    };
    if t == 0 {
        return s("");
    }
    s(st.borrow().locale.clone())
}

/// The parts of a break iterator's text (IntlPartsIterator).
struct Parts {
    bi: Obj,
    key_type: i64,
}

impl Source for Parts {
    fn reset(&mut self, _vm: &mut Vm) {
        if let Some(st) = obj::state::<Bi>(&self.bi) {
            st.borrow_mut().first();
        }
    }
    fn next(&mut self, _vm: &mut Vm) -> Option<(Option<i64>, Value)> {
        let st = obj::state::<Bi>(&self.bi)?;
        let mut b = st.borrow_mut();
        let cur = b.current();
        if cur == DONE {
            return None;
        }
        let nx = b.next_boundary();
        if nx == DONE {
            return None;
        }
        let key = match self.key_type {
            1 => Some(cur),
            2 => Some(nx),
            _ => None,
        };
        let t = b.text.as_deref().unwrap_or(b"");
        let (a, z) = (cur as usize, (nx as usize).min(t.len()));
        let part = t.get(a..z).unwrap_or(b"").to_vec();
        Some((key, sv(part)))
    }
    fn object(&self) -> Option<Obj> {
        Some(self.bi.clone())
    }
}

/// The boundaries of a break iterator (its foreach iterator).
struct Boundaries {
    bi: Obj,
    fresh: bool,
}

impl Source for Boundaries {
    fn reset(&mut self, _vm: &mut Vm) {
        self.fresh = true;
    }
    fn next(&mut self, _vm: &mut Vm) -> Option<(Option<i64>, Value)> {
        let st = obj::state::<Bi>(&self.bi)?;
        let mut b = st.borrow_mut();
        let p = if std::mem::take(&mut self.fresh) {
            b.first()
        } else {
            b.next_boundary()
        };
        (p != DONE).then_some((None, Value::Long(p)))
    }
    fn object(&self) -> Option<Obj> {
        Some(self.bi.clone())
    }
}

fn get_parts_iterator(vm: &mut Vm, c: &mut Call) -> Value {
    let kt = c.long(0);
    if !(0..=2).contains(&kt) {
        crate::zpp::value_error(
            vm,
            &c.name,
            1,
            "type",
            "must be one of IntlPartsIterator::KEY_SEQUENTIAL, IntlPartsIterator::KEY_LEFT, or IntlPartsIterator::KEY_RIGHT",
        );
        return Value::Null;
    }
    if fetch(vm, c).is_none() {
        return Value::Null;
    }
    let src = Parts {
        bi: c.obj(),
        key_type: kt,
    };
    match iterator::new_iterator(vm, b"intlpartsiterator", Box::new(src)) {
        Some(o) => Value::Object(o),
        None => Value::Null,
    }
}

fn get_iterator(vm: &mut Vm, c: &mut Call) -> Value {
    let o = c.obj();
    let constructed = obj::state::<Bi>(&o).is_some_and(|s| s.borrow().kind.is_some());
    if !constructed {
        crate::zpp::throw(vm, b"Exception", "The BreakIterator is not properly constructed");
        return Value::Null;
    }
    let src = Boundaries { bi: o, fresh: true };
    match iterator::new_iterator(vm, b"intliterator", Box::new(src)) {
        Some(o) => Value::Object(o),
        None => Value::Null,
    }
}

fn get_error_code(_vm: &mut Vm, c: &mut Call) -> Value {
    let o = c.obj();
    match obj::state::<Bi>(&o) {
        Some(st) => Value::Long(st.borrow().err.code as i64),
        None => Value::Long(0),
    }
}

fn get_error_message(_vm: &mut Vm, c: &mut Call) -> Value {
    let o = c.obj();
    match obj::state::<Bi>(&o) {
        Some(st) => s(st.borrow().err.message()),
        None => s("U_ZERO_ERROR"),
    }
}

fn rbbi_construct(vm: &mut Vm, c: &mut Call) -> Value {
    let rules = c.str(0);
    let compiled = c.bool(1);
    let o = c.obj();
    let Some(st) = obj::state::<Bi>(&o) else {
        return Value::Null;
    };
    if st.borrow().kind.is_some() {
        crate::zpp::throw(
            vm,
            b"Error",
            "IntlRuleBasedBreakIterator object is already constructed",
        );
        return Value::Null;
    }
    let r = if !compiled {
        let Ok(src) = std::str::from_utf8(&rules) else {
            crate::zpp::throw(
                vm,
                b"IntlException",
                "IntlRuleBasedBreakIterator::__construct(): rules were not a valid UTF-8 string",
            );
            return Value::Null;
        };
        match Rules::compile(src) {
            Ok(r) => {
                error::set_global_code(0);
                r
            }
            Err(e) => {
                error::set_global_code(e.code);
                let pe = error::parse_error_string(e.line, e.offset, "", "");
                let msg = format!(
                    "IntlRuleBasedBreakIterator::__construct(): unable to create RuleBasedBreakIterator from rules ({})",
                    pe
                );
                crate::zpp::throw(vm, b"IntlException", &msg);
                return Value::Null;
            }
        }
    } else {
        match breakiter_rules::from_binary(&rules) {
            Some(r) => r,
            None => {
                crate::zpp::throw(
                    vm,
                    b"IntlException",
                    "IntlRuleBasedBreakIterator::__construct(): unable to create instance from compiled rules",
                );
                return Value::Null;
            }
        }
    };
    let mut b = st.borrow_mut();
    b.kind = Some(Kind::Rules(Rc::new(r)));
    b.compute();
    Value::Null
}

fn get_rules(vm: &mut Vm, c: &mut Call) -> Value {
    let Some(st) = fetch(vm, c) else {
        return Value::Null;
    };
    let b = st.borrow();
    match &b.kind {
        Some(Kind::Rules(r)) => s(breakiter_rules::strip_rules(&r.source)),
        _ => s(""),
    }
}

fn get_rule_status(vm: &mut Vm, c: &mut Call) -> Value {
    match fetch(vm, c) {
        Some(st) => Value::Long(st.borrow().rule_status()),
        None => Value::Null,
    }
}

fn get_rule_status_vec(vm: &mut Vm, c: &mut Call) -> Value {
    let Some(st) = fetch(vm, c) else {
        return Value::Null;
    };
    let mut a = PhpArray::new();
    for v in st.borrow().statuses() {
        a.push(Value::Long(v as i64));
    }
    crate::arr(a)
}

fn get_binary_rules(vm: &mut Vm, c: &mut Call) -> Value {
    let Some(st) = fetch(vm, c) else {
        return Value::Null;
    };
    let b = st.borrow();
    match &b.kind {
        Some(Kind::Rules(r)) => sv(breakiter_rules::binary(r)),
        _ => {
            let src = match &b.kind {
                Some(Kind::Word) => "word",
                Some(Kind::Line) => "line",
                Some(Kind::Sentence) => "sentence",
                Some(Kind::Title) => "title",
                _ => "char",
            };
            sv(format!("GORO-BUILTIN-{}", src).into_bytes())
        }
    }
}

fn get_last_code_point(vm: &mut Vm, c: &mut Call) -> Value {
    match fetch(vm, c) {
        Some(st) => Value::Long(st.borrow().last_cp),
        None => Value::Null,
    }
}

fn parts_get_break_iterator(vm: &mut Vm, c: &mut Call) -> Value {
    if iterator::fetch(vm, c).is_none() {
        return Value::Null;
    }
    match iterator::source_object(&c.obj()) {
        Some(o) => Value::Object(o),
        None => Value::Null,
    }
}

fn parts_get_rule_status(vm: &mut Vm, c: &mut Call) -> Value {
    if iterator::fetch(vm, c).is_none() {
        return Value::Null;
    }
    match iterator::source_object(&c.obj()) {
        Some(o) => vm
            .call_object_method(&Value::Object(o), b"getRuleStatus", &[])
            .unwrap_or(Value::Null),
        None => Value::Null,
    }
}
