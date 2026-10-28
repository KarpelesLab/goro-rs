//! `IntlBreakIterator`, `IntlRuleBasedBreakIterator` and
//! `IntlCodePointBreakIterator` (PHP manual). Offsets are UTF-8 byte
//! offsets of the text.

use goro_ext_intl::breakiter::{self as bk, Kind, RuleSet};
use goro_ext_intl::error as ue;
use goro_runtime::{ObjRef, PhpArray, Value};

use super::{
    already_constructed, fail_obj, has_obj, obj_err, reset, reset_obj, static_class, store,
    with_obj,
};
use crate::class::NativeHooks;
use crate::func::Args;
use crate::registry::Module;
use crate::vm::{R, Vm};

pub static MODULE: Module = Module {
    name: "intl",
    classes: &[
        ("intlbreakiterator", BI_CLASS),
        ("intlrulebasedbreakiterator", RBBI_CLASS),
        ("intlcodepointbreakiterator", CPBI_CLASS),
    ],
    methods: &[
        (
            "intlbreakiterator::createcharacterinstance",
            create_character,
        ),
        (
            "intlbreakiterator::createcodepointinstance",
            create_code_point,
        ),
        ("intlbreakiterator::createlineinstance", create_line),
        ("intlbreakiterator::createsentenceinstance", create_sentence),
        ("intlbreakiterator::createtitleinstance", create_title),
        ("intlbreakiterator::createwordinstance", create_word),
        ("intlbreakiterator::current", b_current),
        ("intlbreakiterator::first", b_first),
        ("intlbreakiterator::following", b_following),
        ("intlbreakiterator::geterrorcode", b_error_code),
        ("intlbreakiterator::geterrormessage", b_error_message),
        ("intlbreakiterator::getlocale", b_get_locale),
        ("intlbreakiterator::getpartsiterator", b_parts),
        ("intlbreakiterator::gettext", b_get_text),
        ("intlbreakiterator::isboundary", b_is_boundary),
        ("intlbreakiterator::last", b_last),
        ("intlbreakiterator::next", b_next),
        ("intlbreakiterator::preceding", b_preceding),
        ("intlbreakiterator::previous", b_previous),
        ("intlbreakiterator::settext", b_set_text),
        ("intlbreakiterator::getiterator", b_get_iterator),
        ("intlrulebasedbreakiterator::__construct", rb_construct),
        ("intlrulebasedbreakiterator::getrules", rb_rules),
        ("intlrulebasedbreakiterator::getrulestatus", rb_status),
        (
            "intlrulebasedbreakiterator::getrulestatusvec",
            rb_status_vec,
        ),
        ("intlrulebasedbreakiterator::getbinaryrules", rb_binary),
        ("intlcodepointbreakiterator::getlastcodepoint", cp_last),
    ],
    hooks: &[("intlbreakiterator", hooks)],
    ..Module::EMPTY
};

const BI_CLASS: &str = r#"<?php class IntlBreakIterator implements IteratorAggregate {
    public const int DONE = -1;
    public const int WORD_NONE = 0;
    public const int WORD_NONE_LIMIT = 100;
    public const int WORD_NUMBER = 100;
    public const int WORD_NUMBER_LIMIT = 200;
    public const int WORD_LETTER = 200;
    public const int WORD_LETTER_LIMIT = 300;
    public const int WORD_KANA = 300;
    public const int WORD_KANA_LIMIT = 400;
    public const int WORD_IDEO = 400;
    public const int WORD_IDEO_LIMIT = 500;
    public const int LINE_SOFT = 0;
    public const int LINE_SOFT_LIMIT = 100;
    public const int LINE_HARD = 100;
    public const int LINE_HARD_LIMIT = 200;
    public const int SENTENCE_TERM = 0;
    public const int SENTENCE_TERM_LIMIT = 100;
    public const int SENTENCE_SEP = 100;
    public const int SENTENCE_SEP_LIMIT = 200;
    public static function createCharacterInstance(?string $locale = null): ?IntlBreakIterator {}
    public static function createCodePointInstance(): IntlCodePointBreakIterator {}
    public static function createLineInstance(?string $locale = null): ?IntlBreakIterator {}
    public static function createSentenceInstance(?string $locale = null): ?IntlBreakIterator {}
    public static function createTitleInstance(?string $locale = null): ?IntlBreakIterator {}
    public static function createWordInstance(?string $locale = null): ?IntlBreakIterator {}
    private function __construct() {}
    public function current(): int {}
    public function first(): int {}
    public function following(int $offset): int {}
    public function getErrorCode(): int {}
    public function getErrorMessage(): string {}
    public function getLocale(int $type): string|false {}
    public function getPartsIterator(int $type = IntlPartsIterator::KEY_SEQUENTIAL): IntlPartsIterator {}
    public function getText(): ?string {}
    public function isBoundary(int $offset): bool {}
    public function last(): int {}
    public function next(?int $offset = null): int {}
    public function preceding(int $offset): int {}
    public function previous(): int {}
    public function setText(string $text): bool {}
    public function getIterator(): Iterator {}
}"#;

const RBBI_CLASS: &str = r#"<?php class IntlRuleBasedBreakIterator extends IntlBreakIterator {
    public function __construct(string $rules, bool $compiled = false) {}
    public function getBinaryRules(): string|false {}
    public function getRules(): string|false {}
    public function getRuleStatus(): int {}
    public function getRuleStatusVec(): array|false {}
}"#;

const CPBI_CLASS: &str = r#"<?php class IntlCodePointBreakIterator extends IntlBreakIterator {
    public function getLastCodePoint(): int {}
}"#;

/// A break iterator's state.
#[derive(Clone)]
pub(crate) struct Brk {
    kind: Kind,
    locale: String,
    text: Option<String>,
    bounds: Vec<(usize, Vec<i32>)>,
    cur: usize,
    last_cp: i64,
}

impl Brk {
    fn new(kind: Kind, locale: String) -> Brk {
        Brk {
            kind,
            locale,
            text: None,
            bounds: vec![(0, vec![0])],
            cur: 0,
            last_cp: -1,
        }
    }

    fn len(&self) -> usize {
        self.text.as_ref().map_or(0, String::len)
    }

    fn pos(&self) -> i64 {
        self.bounds[self.cur].0 as i64
    }
}

fn hooks() -> NativeHooks {
    NativeHooks {
        create: Some(super::numfmt::on_create),
        clone: Some(
            |vm, from, to| match with_obj::<Brk, Brk>(from, |x| x.v.clone()) {
                Some(b) => store(to, b),
                None => super::timezone::clone_error(
                    vm,
                    &format!(
                        "Cannot clone uninitialized {}",
                        vm.class_of(from).name.to_string_lossy()
                    ),
                ),
            },
        ),
        debug_props: Some(|vm, o| {
            let mut arr = PhpArray::new();
            let st = with_obj::<Brk, (Option<String>, String)>(o, |s| {
                (
                    s.v.text.clone(),
                    match s.v.kind {
                        Kind::CodePoint => "CodePointBreakIterator".to_string(),
                        _ => "icu::RuleBasedBreakIterator".to_string(),
                    },
                )
            });
            let _ = vm;
            match st {
                Some((text, ty)) => {
                    arr.insert(goro_runtime::Key::from_bytes(b"valid"), Value::Bool(true));
                    arr.insert(
                        goro_runtime::Key::from_bytes(b"text"),
                        text.map_or(Value::Null, |t| Value::from(t.as_str())),
                    );
                    arr.insert(
                        goro_runtime::Key::from_bytes(b"type"),
                        Value::from(ty.as_str()),
                    );
                }
                None => {
                    arr.insert(goro_runtime::Key::from_bytes(b"valid"), Value::Bool(false));
                }
            }
            Some(arr)
        }),
        compare: Some(|_, x, y| {
            let (Value::Object(a), Value::Object(b)) = (x, y) else {
                return None;
            };
            let p = with_obj::<Brk, (String, String, Option<String>)>(a, |s| {
                (kind_name(&s.v.kind), s.v.locale.clone(), s.v.text.clone())
            })?;
            let q = with_obj::<Brk, (String, String, Option<String>)>(b, |s| {
                (kind_name(&s.v.kind), s.v.locale.clone(), s.v.text.clone())
            })?;
            Some(Ok(if p.0 == q.0 && p.2 == q.2 { 0 } else { 1 }))
        }),
        ..Default::default()
    }
}

fn kind_name(k: &Kind) -> String {
    match k {
        Kind::Rules(r) => r.source.clone(),
        other => format!("{other:?}"),
    }
}

fn make(vm: &mut Vm, a: &Args, class: &str, kind: Kind) -> R<Value> {
    reset(vm);
    let l = match a.opt_str(0) {
        Some(s) if !s.as_bytes().is_empty() => s.to_string_lossy(),
        _ => super::default_locale(vm),
    };
    let c = super::class(vm, class)?;
    let o = vm.instantiate_raw(&c)?;
    store(&o, Brk::new(kind, l));
    Ok(Value::Object(o))
}

fn create_character(vm: &mut Vm, a: &mut Args) -> R<Value> {
    make(vm, a, "IntlRuleBasedBreakIterator", Kind::Character)
}

fn create_code_point(vm: &mut Vm, a: &mut Args) -> R<Value> {
    make(vm, a, "IntlCodePointBreakIterator", Kind::CodePoint)
}

fn create_line(vm: &mut Vm, a: &mut Args) -> R<Value> {
    make(vm, a, "IntlRuleBasedBreakIterator", Kind::Line)
}

fn create_sentence(vm: &mut Vm, a: &mut Args) -> R<Value> {
    make(vm, a, "IntlRuleBasedBreakIterator", Kind::Sentence)
}

fn create_title(vm: &mut Vm, a: &mut Args) -> R<Value> {
    make(vm, a, "IntlRuleBasedBreakIterator", Kind::Title)
}

fn create_word(vm: &mut Vm, a: &mut Args) -> R<Value> {
    make(vm, a, "IntlRuleBasedBreakIterator", Kind::Word)
}

fn this(vm: &mut Vm, a: &Args) -> R<ObjRef> {
    let o = a.this_obj().clone();
    if !has_obj::<Brk>(&o) {
        let name = vm.class_of(&o).name.to_string_lossy();
        return vm.throw("Error", &format!("Found unconstructed {name}"));
    }
    Ok(o)
}

fn with<U>(o: &ObjRef, f: impl FnOnce(&mut Brk) -> U) -> U {
    with_obj::<Brk, U>(o, |x| f(&mut x.v)).expect("break iterator state")
}

const DONE: i64 = -1;

fn b_current(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = this(vm, a)?;
    Ok(Value::Int(with(&o, |b| b.pos())))
}

fn b_first(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = this(vm, a)?;
    Ok(Value::Int(with(&o, |b| {
        b.cur = 0;
        b.last_cp = -1;
        b.pos()
    })))
}

fn b_last(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = this(vm, a)?;
    Ok(Value::Int(with(&o, |b| {
        b.cur = b.bounds.len() - 1;
        b.last_cp = -1;
        b.pos()
    })))
}

/// The code point between two boundaries of a code point iterator.
fn cp_between(b: &Brk, from: usize, to: usize) -> i64 {
    let (x, y) = (b.bounds[from.min(to)].0, b.bounds[from.max(to)].0);
    b.text
        .as_ref()
        .and_then(|t| t[x..y].chars().next())
        .map_or(-1, |c| c as i64)
}

fn step(b: &mut Brk, forward: bool) -> i64 {
    if forward {
        if b.cur + 1 < b.bounds.len() {
            b.last_cp = cp_between(b, b.cur, b.cur + 1);
            b.cur += 1;
            b.pos()
        } else {
            DONE
        }
    } else if b.cur > 0 {
        b.last_cp = cp_between(b, b.cur - 1, b.cur);
        b.cur -= 1;
        b.pos()
    } else {
        DONE
    }
}

fn b_next(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = this(vm, a)?;
    let n = a.opt_int(0);
    Ok(Value::Int(with(&o, |b| match n {
        None => step(b, true),
        Some(0) => b.pos(),
        Some(k) => {
            let mut r = b.pos();
            for _ in 0..k.unsigned_abs() {
                r = step(b, k > 0);
                if r == DONE {
                    break;
                }
            }
            r
        }
    })))
}

fn b_previous(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = this(vm, a)?;
    Ok(Value::Int(with(&o, |b| step(b, false))))
}

fn b_following(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = this(vm, a)?;
    let off = a.int(0);
    Ok(Value::Int(with(&o, |b| {
        b.last_cp = -1;
        if off < 0 {
            b.cur = 0;
            return b.pos();
        }
        match b.bounds.iter().position(|(p, _)| *p as i64 > off) {
            Some(i) => {
                b.cur = i;
                b.pos()
            }
            None => {
                b.cur = b.bounds.len() - 1;
                DONE
            }
        }
    })))
}

fn b_preceding(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = this(vm, a)?;
    let off = a.int(0);
    Ok(Value::Int(with(&o, |b| {
        b.last_cp = -1;
        if off > b.len() as i64 {
            b.cur = b.bounds.len() - 1;
            return b.pos();
        }
        if off < 0 {
            b.cur = 0;
            return b.pos();
        }
        match b.bounds.iter().rposition(|(p, _)| (*p as i64) < off) {
            Some(i) => {
                b.cur = i;
                b.pos()
            }
            None => {
                b.cur = 0;
                DONE
            }
        }
    })))
}

fn b_is_boundary(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = this(vm, a)?;
    let off = a.int(0);
    Ok(Value::Bool(with(&o, |b| {
        if off < 0 || off > b.len() as i64 {
            return false;
        }
        match b.bounds.iter().position(|(p, _)| *p as i64 == off) {
            Some(i) => {
                b.cur = i;
                true
            }
            None => {
                if let Some(i) = b.bounds.iter().position(|(p, _)| *p as i64 > off) {
                    b.cur = i;
                }
                false
            }
        }
    })))
}

fn b_get_text(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = this(vm, a)?;
    Ok(with(&o, |b| {
        b.text
            .as_ref()
            .map_or(Value::Null, |t| Value::from(t.as_str()))
    }))
}

fn b_set_text(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = this(vm, a)?;
    reset_obj::<Brk>(vm, &o);
    let Ok(t) = std::str::from_utf8(a.bytes(0)).map(str::to_string) else {
        fail_obj::<Brk>(
            vm,
            a,
            &o,
            ue::U_INVALID_CHAR_FOUND,
            "The text must be valid UTF-8",
        )?;
        return Ok(Value::Bool(false));
    };
    with(&o, |b| {
        b.bounds = bk::boundaries(&b.kind, &t);
        b.text = Some(t);
        b.cur = 0;
        b.last_cp = -1;
    });
    Ok(Value::Bool(true))
}

fn b_get_locale(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = this(vm, a)?;
    match a.int(0) {
        0 => Ok(Value::from("")),
        1 => Ok(Value::from(with(&o, |b| b.locale.clone()).as_str())),
        _ => {
            fail_obj::<Brk>(
                vm,
                a,
                &o,
                ue::U_ILLEGAL_ARGUMENT_ERROR,
                "invalid locale type",
            )?;
            Ok(Value::Bool(false))
        }
    }
}

fn b_parts(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = this(vm, a)?;
    let ty = a.int_or(0, 0);
    if !(0..=2).contains(&ty) {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #1 ($type) must be one of IntlPartsIterator::KEY_SEQUENTIAL, IntlPartsIterator::KEY_LEFT, or IntlPartsIterator::KEY_RIGHT",
        );
    }
    let it = super::iter::new_parts_iterator(vm, o, ty)?;
    Ok(Value::Object(it))
}

/// The text between two byte offsets of a break iterator.
pub(crate) fn text_slice(o: &ObjRef, l: usize, r: usize) -> Value {
    with(o, |b| {
        let t = b.text.as_deref().unwrap_or("");
        Value::from(t.get(l..r).unwrap_or(""))
    })
}

/// Moves a break iterator to its first boundary (`first`) or on, giving
/// the part it stepped over: (left, right, rule status).
pub(crate) fn advance(o: &ObjRef, first: bool) -> Option<(usize, usize, i64)> {
    with(o, |b| {
        if first {
            b.cur = 0;
            b.last_cp = -1;
        }
        let l = b.bounds[b.cur].0;
        if step(b, true) == DONE {
            return None;
        }
        let st = b.bounds[b.cur].1.iter().copied().max().unwrap_or(0) as i64;
        Some((l, b.bounds[b.cur].0, st))
    })
}

fn b_get_iterator(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = this(vm, a)?;
    let items = with(&o, |b| {
        b.bounds
            .iter()
            .enumerate()
            .map(|(i, (p, _))| (Value::Int(i as i64), Value::Int(*p as i64)))
            .collect::<Vec<_>>()
    });
    let it = super::iter::new_iterator(vm, "IntlIterator", items, Some(o), Vec::new())?;
    Ok(Value::Object(it))
}

fn b_error_code(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    let _ = vm;
    Ok(Value::Int(obj_err::<Brk>(&o).code))
}

fn b_error_message(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    let _ = vm;
    Ok(Value::from(obj_err::<Brk>(&o).full_message().as_str()))
}

// ---------------------------------------------------------------- rule based

/// The marker of goro's "compiled" rules (the rules text behind it).
const BINARY_MAGIC: &str = "\u{0}goro-rbbi\u{0}";

fn rb_construct(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    if has_obj::<Brk>(&o) {
        return already_constructed(vm, "IntlRuleBasedBreakIterator");
    }
    reset(vm);
    let compiled = a.bool_or(1, false);
    let src = String::from_utf8_lossy(a.bytes(0)).into_owned();
    let rules = if compiled {
        match src.strip_prefix(BINARY_MAGIC) {
            Some(r) => r.to_string(),
            None => {
                let msg = format!(
                    "{}(): unable to create instance from compiled rules",
                    a.fname()
                );
                super::set_error_quiet(vm, ue::U_ILLEGAL_ARGUMENT_ERROR, msg.clone());
                return vm.throw("IntlException", &msg);
            }
        }
    } else {
        src
    };
    match RuleSet::parse(&rules) {
        Ok(r) => {
            let locale = super::default_locale(vm);
            store(&o, Brk::new(Kind::Rules(Box::new(r)), locale));
            let _ = static_class;
            Ok(Value::Null)
        }
        Err(e) => {
            let msg = format!(
                "{}(): unable to create RuleBasedBreakIterator from rules (parse error on line {}, offset {})",
                a.fname(),
                e.line,
                e.offset
            );
            super::set_error_quiet(vm, ue::U_BRK_RULE_SYNTAX, msg.clone());
            vm.throw("IntlException", &msg)
        }
    }
}

fn rules_of(b: &Brk) -> Option<String> {
    match &b.kind {
        Kind::Rules(r) => Some(r.rules_text()),
        _ => None,
    }
}

fn rb_rules(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = this(vm, a)?;
    Ok(match with(&o, |b| rules_of(b)) {
        Some(r) => Value::from(r.as_str()),
        None => Value::from(""),
    })
}

fn rb_binary(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = this(vm, a)?;
    let r = with(&o, |b| match &b.kind {
        Kind::Rules(r) => r.source.clone(),
        _ => String::new(),
    });
    Ok(Value::from(format!("{BINARY_MAGIC}{r}").as_str()))
}

fn rb_status(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = this(vm, a)?;
    Ok(Value::Int(with(&o, |b| {
        b.bounds[b.cur].1.iter().copied().max().unwrap_or(0) as i64
    })))
}

fn rb_status_vec(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = this(vm, a)?;
    let v = with(&o, |b| b.bounds[b.cur].1.clone());
    Ok(Value::Array(PhpArray::from_values(
        v.into_iter().map(|x| Value::Int(x as i64)),
    )))
}

fn cp_last(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = this(vm, a)?;
    Ok(Value::Int(with(&o, |b| b.last_cp)))
}
