//! ext/tokenizer (PHP manual: Tokenizer; RFC "Object-based token_get_all()
//! alternative" for PhpToken) over goro-syntax's lexer, whose token stream
//! is token_get_all()'s. TOKEN_PARSE runs the parser too: its parse error
//! becomes a ParseError, its deprecations are raised, and keywords used as
//! names come out as T_STRING.

use goro_runtime::{PhpArray, PhpStr, Value};
use goro_syntax::{T, Token};

use crate::func::Args;
use crate::registry::{ConstVal, Module};
use crate::vm::{R, Vm};

pub static MODULE: Module = Module {
    name: "tokenizer",
    functions: &[
        (
            "function token_get_all(string $code, int $flags = 0): array",
            token_get_all,
        ),
        ("function token_name(int $id): string", token_name),
    ],
    classes: &[(
        "phptoken",
        "<?php class PhpToken implements Stringable {
            public int $id;
            public string $text;
            public int $line;
            public int $pos;
            final public function __construct(int $id, string $text, int $line = -1, int $pos = -1) {
                $this->id = $id;
                $this->text = $text;
                $this->line = $line;
                $this->pos = $pos;
            }
            public static function tokenize(string $code, int $flags = 0): array {}
            public function is($kind): bool {
                if (is_int($kind)) { return $this->id === $kind; }
                if (is_string($kind)) { return $this->text === $kind; }
                if (is_array($kind)) {
                    foreach ($kind as $k) {
                        if (is_int($k)) {
                            if ($this->id === $k) { return true; }
                        } elseif (is_string($k)) {
                            if ($this->text === $k) { return true; }
                        } else {
                            throw new TypeError('PhpToken::is(): Argument #1 ($kind) must only have elements of type string|int, ' . get_debug_type($k) . ' given');
                        }
                    }
                    return false;
                }
                throw new TypeError('PhpToken::is(): Argument #1 ($kind) must be of type string|int|array, ' . get_debug_type($kind) . ' given');
            }
            public function isIgnorable(): bool {
                $id = $this->id;
                return $id === T_WHITESPACE || $id === T_COMMENT || $id === T_DOC_COMMENT || $id === T_OPEN_TAG;
            }
            public function getTokenName(): ?string {
                $id = $this->id;
                if ($id < 256) { return chr($id); }
                $n = token_name($id);
                return $n === 'UNKNOWN' ? null : $n;
            }
            public function __toString(): string { return $this->text; }
        }",
    )],
    methods: &[("phptoken::tokenize", phptoken_tokenize)],
    // The T_* ids of PHP 8.5 (goro-syntax's token table), the old
    // spelling of T_DOUBLE_COLON (PHP manual: List of Parser Tokens).
    constants: &[
        ("T_LNUMBER", ConstVal::Int(260)),
        ("T_DNUMBER", ConstVal::Int(261)),
        ("T_STRING", ConstVal::Int(262)),
        ("T_NAME_FULLY_QUALIFIED", ConstVal::Int(263)),
        ("T_NAME_RELATIVE", ConstVal::Int(264)),
        ("T_NAME_QUALIFIED", ConstVal::Int(265)),
        ("T_VARIABLE", ConstVal::Int(266)),
        ("T_INLINE_HTML", ConstVal::Int(267)),
        ("T_ENCAPSED_AND_WHITESPACE", ConstVal::Int(268)),
        ("T_CONSTANT_ENCAPSED_STRING", ConstVal::Int(269)),
        ("T_STRING_VARNAME", ConstVal::Int(270)),
        ("T_NUM_STRING", ConstVal::Int(271)),
        ("T_INCLUDE", ConstVal::Int(272)),
        ("T_INCLUDE_ONCE", ConstVal::Int(273)),
        ("T_EVAL", ConstVal::Int(274)),
        ("T_REQUIRE", ConstVal::Int(275)),
        ("T_REQUIRE_ONCE", ConstVal::Int(276)),
        ("T_LOGICAL_OR", ConstVal::Int(277)),
        ("T_LOGICAL_XOR", ConstVal::Int(278)),
        ("T_LOGICAL_AND", ConstVal::Int(279)),
        ("T_PRINT", ConstVal::Int(280)),
        ("T_YIELD", ConstVal::Int(281)),
        ("T_YIELD_FROM", ConstVal::Int(282)),
        ("T_INSTANCEOF", ConstVal::Int(283)),
        ("T_NEW", ConstVal::Int(284)),
        ("T_CLONE", ConstVal::Int(285)),
        ("T_EXIT", ConstVal::Int(286)),
        ("T_IF", ConstVal::Int(287)),
        ("T_ELSEIF", ConstVal::Int(288)),
        ("T_ELSE", ConstVal::Int(289)),
        ("T_ENDIF", ConstVal::Int(290)),
        ("T_ECHO", ConstVal::Int(291)),
        ("T_DO", ConstVal::Int(292)),
        ("T_WHILE", ConstVal::Int(293)),
        ("T_ENDWHILE", ConstVal::Int(294)),
        ("T_FOR", ConstVal::Int(295)),
        ("T_ENDFOR", ConstVal::Int(296)),
        ("T_FOREACH", ConstVal::Int(297)),
        ("T_ENDFOREACH", ConstVal::Int(298)),
        ("T_DECLARE", ConstVal::Int(299)),
        ("T_ENDDECLARE", ConstVal::Int(300)),
        ("T_AS", ConstVal::Int(301)),
        ("T_SWITCH", ConstVal::Int(302)),
        ("T_ENDSWITCH", ConstVal::Int(303)),
        ("T_CASE", ConstVal::Int(304)),
        ("T_DEFAULT", ConstVal::Int(305)),
        ("T_MATCH", ConstVal::Int(306)),
        ("T_BREAK", ConstVal::Int(307)),
        ("T_CONTINUE", ConstVal::Int(308)),
        ("T_GOTO", ConstVal::Int(309)),
        ("T_FUNCTION", ConstVal::Int(310)),
        ("T_FN", ConstVal::Int(311)),
        ("T_CONST", ConstVal::Int(312)),
        ("T_RETURN", ConstVal::Int(313)),
        ("T_TRY", ConstVal::Int(314)),
        ("T_CATCH", ConstVal::Int(315)),
        ("T_FINALLY", ConstVal::Int(316)),
        ("T_THROW", ConstVal::Int(317)),
        ("T_USE", ConstVal::Int(318)),
        ("T_INSTEADOF", ConstVal::Int(319)),
        ("T_GLOBAL", ConstVal::Int(320)),
        ("T_STATIC", ConstVal::Int(321)),
        ("T_ABSTRACT", ConstVal::Int(322)),
        ("T_FINAL", ConstVal::Int(323)),
        ("T_PRIVATE", ConstVal::Int(324)),
        ("T_PROTECTED", ConstVal::Int(325)),
        ("T_PUBLIC", ConstVal::Int(326)),
        ("T_PRIVATE_SET", ConstVal::Int(327)),
        ("T_PROTECTED_SET", ConstVal::Int(328)),
        ("T_PUBLIC_SET", ConstVal::Int(329)),
        ("T_READONLY", ConstVal::Int(330)),
        ("T_VAR", ConstVal::Int(331)),
        ("T_UNSET", ConstVal::Int(332)),
        ("T_ISSET", ConstVal::Int(333)),
        ("T_EMPTY", ConstVal::Int(334)),
        ("T_HALT_COMPILER", ConstVal::Int(335)),
        ("T_CLASS", ConstVal::Int(336)),
        ("T_TRAIT", ConstVal::Int(337)),
        ("T_INTERFACE", ConstVal::Int(338)),
        ("T_ENUM", ConstVal::Int(339)),
        ("T_EXTENDS", ConstVal::Int(340)),
        ("T_IMPLEMENTS", ConstVal::Int(341)),
        ("T_NAMESPACE", ConstVal::Int(342)),
        ("T_LIST", ConstVal::Int(343)),
        ("T_ARRAY", ConstVal::Int(344)),
        ("T_CALLABLE", ConstVal::Int(345)),
        ("T_LINE", ConstVal::Int(346)),
        ("T_FILE", ConstVal::Int(347)),
        ("T_DIR", ConstVal::Int(348)),
        ("T_CLASS_C", ConstVal::Int(349)),
        ("T_TRAIT_C", ConstVal::Int(350)),
        ("T_METHOD_C", ConstVal::Int(351)),
        ("T_FUNC_C", ConstVal::Int(352)),
        ("T_PROPERTY_C", ConstVal::Int(353)),
        ("T_NS_C", ConstVal::Int(354)),
        ("T_ATTRIBUTE", ConstVal::Int(355)),
        ("T_PLUS_EQUAL", ConstVal::Int(356)),
        ("T_MINUS_EQUAL", ConstVal::Int(357)),
        ("T_MUL_EQUAL", ConstVal::Int(358)),
        ("T_DIV_EQUAL", ConstVal::Int(359)),
        ("T_CONCAT_EQUAL", ConstVal::Int(360)),
        ("T_MOD_EQUAL", ConstVal::Int(361)),
        ("T_AND_EQUAL", ConstVal::Int(362)),
        ("T_OR_EQUAL", ConstVal::Int(363)),
        ("T_XOR_EQUAL", ConstVal::Int(364)),
        ("T_SL_EQUAL", ConstVal::Int(365)),
        ("T_SR_EQUAL", ConstVal::Int(366)),
        ("T_COALESCE_EQUAL", ConstVal::Int(367)),
        ("T_BOOLEAN_OR", ConstVal::Int(368)),
        ("T_BOOLEAN_AND", ConstVal::Int(369)),
        ("T_IS_EQUAL", ConstVal::Int(370)),
        ("T_IS_NOT_EQUAL", ConstVal::Int(371)),
        ("T_IS_IDENTICAL", ConstVal::Int(372)),
        ("T_IS_NOT_IDENTICAL", ConstVal::Int(373)),
        ("T_IS_SMALLER_OR_EQUAL", ConstVal::Int(374)),
        ("T_IS_GREATER_OR_EQUAL", ConstVal::Int(375)),
        ("T_SPACESHIP", ConstVal::Int(376)),
        ("T_SL", ConstVal::Int(377)),
        ("T_SR", ConstVal::Int(378)),
        ("T_INC", ConstVal::Int(379)),
        ("T_DEC", ConstVal::Int(380)),
        ("T_INT_CAST", ConstVal::Int(381)),
        ("T_DOUBLE_CAST", ConstVal::Int(382)),
        ("T_STRING_CAST", ConstVal::Int(383)),
        ("T_ARRAY_CAST", ConstVal::Int(384)),
        ("T_OBJECT_CAST", ConstVal::Int(385)),
        ("T_BOOL_CAST", ConstVal::Int(386)),
        ("T_UNSET_CAST", ConstVal::Int(387)),
        ("T_VOID_CAST", ConstVal::Int(388)),
        ("T_OBJECT_OPERATOR", ConstVal::Int(389)),
        ("T_NULLSAFE_OBJECT_OPERATOR", ConstVal::Int(390)),
        ("T_DOUBLE_ARROW", ConstVal::Int(391)),
        ("T_COMMENT", ConstVal::Int(392)),
        ("T_DOC_COMMENT", ConstVal::Int(393)),
        ("T_OPEN_TAG", ConstVal::Int(394)),
        ("T_OPEN_TAG_WITH_ECHO", ConstVal::Int(395)),
        ("T_CLOSE_TAG", ConstVal::Int(396)),
        ("T_WHITESPACE", ConstVal::Int(397)),
        ("T_START_HEREDOC", ConstVal::Int(398)),
        ("T_END_HEREDOC", ConstVal::Int(399)),
        ("T_DOLLAR_OPEN_CURLY_BRACES", ConstVal::Int(400)),
        ("T_CURLY_OPEN", ConstVal::Int(401)),
        ("T_DOUBLE_COLON", ConstVal::Int(402)),
        ("T_NS_SEPARATOR", ConstVal::Int(403)),
        ("T_ELLIPSIS", ConstVal::Int(404)),
        ("T_COALESCE", ConstVal::Int(405)),
        ("T_POW", ConstVal::Int(406)),
        ("T_POW_EQUAL", ConstVal::Int(407)),
        ("T_PIPE", ConstVal::Int(408)),
        ("T_AMPERSAND_FOLLOWED_BY_VAR_OR_VARARG", ConstVal::Int(409)),
        ("T_AMPERSAND_NOT_FOLLOWED_BY_VAR_OR_VARARG", ConstVal::Int(410)),
        ("T_BAD_CHARACTER", ConstVal::Int(411)),
        ("T_PAAMAYIM_NEKUDOTAYIM", ConstVal::Int(402)),
        ("TOKEN_PARSE", ConstVal::Int(1)),
    ],
    listed: true,
    ..Module::EMPTY
};

const TOKEN_PARSE: i64 = 1;

/// Lexes (and with TOKEN_PARSE parses) `src`.
fn tokens(vm: &mut Vm, src: &[u8], flags: i64) -> R<Vec<Token>> {
    let short = vm.ini_bool(b"short_open_tag", true);
    if flags & TOKEN_PARSE == 0 {
        return Ok(goro_syntax::tokenize(src, short));
    }
    let opts = goro_syntax::ParseOptions {
        short_open_tag: short,
        ..Default::default()
    };
    let arena = goro_syntax::Arena::new();
    let prog = match goro_syntax::parse_with(&arena, src, opts) {
        Ok(p) => p,
        Err(e) => {
            let o = vm.create_exception("ParseError", &e.message, 0);
            return match o {
                Ok(o) => {
                    vm.set_prop_raw(&o, b"file", Value::Str(PhpStr::empty()));
                    vm.set_prop_raw(&o, b"line", Value::Int(e.line as i64));
                    Err(crate::vm::Unwind::Throw(o))
                }
                Err(u) => Err(u),
            };
        }
    };
    // Deprecations of the parse (non-canonical casts) are raised with no
    // file, as PHP 8.5 does (test gh19507_throw.phpt).
    for n in &prog.notices {
        let level = match n.kind {
            goro_syntax::ast::NoticeKind::Deprecated => crate::vm::errors::E_DEPRECATED,
            goro_syntax::ast::NoticeKind::Warning => crate::vm::errors::E_WARNING,
        };
        vm.raise_at(level, &n.message, PhpStr::empty(), n.line)?;
    }
    let mut toks = goro_syntax::tokenize(src, short);
    let mut ids = prog.reserved_identifiers.clone();
    ids.sort_unstable();
    for t in &mut toks {
        if !t.kind.is_trivia() && ids.binary_search(&t.start).is_ok() {
            t.kind = T::String;
        }
    }
    Ok(toks)
}

fn token_get_all(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let src = a.str(0);
    let toks = tokens(vm, &src, a.int_or(1, 0))?;
    let mut out = Vec::with_capacity(toks.len());
    for t in &toks {
        let text = PhpStr::from(t.text(&src));
        if t.kind.is_char() {
            out.push(Value::Str(text));
        } else {
            out.push(Value::Array(PhpArray::from_values(vec![
                Value::Int(t.kind.id() as i64),
                Value::Str(text),
                Value::Int(t.line as i64),
            ])));
        }
    }
    Ok(Value::Array(PhpArray::from_values(out)))
}

fn token_name(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(Value::Str(PhpStr::from(T::token_name(a.int(0)))))
}

/// PhpToken::tokenize(): objects of the called class (`static`), made
/// without running the constructor.
fn phptoken_tokenize(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let src = a.str(0);
    let p = if a.static_class.is_null() {
        a.func.scope.get()
    } else {
        a.static_class
    };
    let cls = vm.rc_class(p);
    let toks = tokens(vm, &src, a.int_or(1, 0))?;
    let mut out = Vec::with_capacity(toks.len());
    for t in &toks {
        let o = vm.instantiate(&cls)?;
        vm.set_prop_raw(&o, b"id", Value::Int(t.kind.id() as i64));
        vm.set_prop_raw(&o, b"text", Value::Str(PhpStr::from(t.text(&src))));
        vm.set_prop_raw(&o, b"line", Value::Int(t.line as i64));
        vm.set_prop_raw(&o, b"pos", Value::Int(t.start as i64));
        out.push(Value::Object(o));
    }
    Ok(Value::Array(PhpArray::from_values(out)))
}
