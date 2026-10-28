//! AST shape tests: operator precedence and associativity (PHP manual:
//! "Operator Precedence"), literals, strings and declarations, through the
//! S-expression dump.

use goro_syntax::{dump, parse};

fn d(code: &str) -> String {
    let src = format!("<?php {}", code);
    let arena = goro_syntax::Arena::new();
    match parse(&arena, src.as_bytes()) {
        Ok(p) => dump::program(&p),
        Err(e) => format!("ERROR {}", e),
    }
}

macro_rules! check {
    ($($code:expr => $exp:expr,)*) => {{
        let mut fails = Vec::new();
        $(
            let got = d($code);
            if got != $exp {
                fails.push(format!("{}\n    got: {}\n    exp: {}", $code, got, $exp));
            }
        )*
        for f in &fails {
            eprintln!("{}", f);
        }
        assert!(fails.is_empty(), "{} cases differ", fails.len());
    }};
}

#[test]
fn precedence() {
    check! {
        "1 + 2 * 3;" => "(+ 1 (* 2 3));",
        "1 - 2 - 3;" => "(- (- 1 2) 3);",
        "2 ** 3 ** 2;" => "(** 2 (** 3 2));",
        "-2 ** 2;" => "(neg (** 2 2));",
        "2 ** -1;" => "(** 2 (neg 1));",
        "!$a instanceof B;" => "(! (instanceof $a B));",
        "-$a instanceof B;" => "(instanceof (neg $a) B);",
        "!$a + 1;" => "(+ (! $a) 1);",
        "$a = $b = 1;" => "(= $a (= $b 1));",
        "$a + $b = 5;" => "(+ $a (= $b 5));",
        "!$a = f();" => "(! (= $a (call f)));",
        "$a ?? $b ?? $c;" => "(?? $a (?? $b $c));",
        "$a ? $b : $c;" => "(? $a $b $c);",
        "$a ?: $b ?: $c;" => "(?: (?: $a $b) $c);",
        "$a ? $b : ($c ? $d : $e);" => "(? $a $b (? $c $d $e));",
        "$a ?? $b ? 1 : 2;" => "(? (?? $a $b) 1 2);",
        "\"a\" . 1 + 2;" => "(. \"a\" (+ 1 2));",
        "1 << 2 . 3;" => "(. (<< 1 2) 3);",
        "1 . 2 == 3;" => "(== (. 1 2) 3);",
        "$x |> f(...) |> g(...);" => "(|> (|> $x (call f ...)) (call g ...));",
        "1 + 2 |> f(...);" => "(|> (+ 1 2) (call f ...));",
        "1 |> f(...) == 2;" => "(== (|> 1 (call f ...)) 2);",
        "$a and $b or $c;" => "(|| (&& $a $b) $c);",
        "$a or $b xor $c and $d;" => "(|| $a (xor $b (&& $c $d)));",
        "$a = 1 and 2;" => "(&& (= $a 1) 2);",
        "print 1 and 2;" => "(&& (print 1) 2);",
        "$a && $b || $c && $d;" => "(|| (&& $a $b) (&& $c $d));",
        "$a | $b ^ $c & $d;" => "(| $a (^ $b (& $c $d)));",
        "$a == $b < $c;" => "(== $a (< $b $c));",
        "$a <=> $b;" => "(<=> $a $b);",
        "@$a['x'] + 1;" => "(+ (@ ([] $a \"x\")) 1);",
        "(int)$a + 1;" => "(+ (cast:int $a) 1);",
        "(string)(float)$a;" => "(cast:string (cast:float $a));",
        "-$a ** 2;" => "(neg (** $a 2));",
        "~$a++;" => "(~ (post++ $a));",
        "++$a ** 2;" => "(** (++pre $a) 2);",
        "$a = &$b + 1;" => "(+ (=& $a $b) 1);",
        "$a .= $b . $c;" => "(.= $a (. $b $c));",
        "$a ??= $b ?: $c;" => "(??= $a (?: $b $c));",
        "yield $a + 1;" => "(yield (+ $a 1));",
        "$x = yield $k => $v;" => "(= $x (yield $k => $v));",
        "$x = yield;" => "(= $x (yield));",
        "yield from f() + 1;" => "(yield-from (+ (call f) 1));",
        "throw $e ?? new E;" => "(throw (?? $e (new E)));",
        "$f = fn($x) => $x and 1;" => "(= $f (fn($x) => (&& $x 1)));",
        "include 'a' . 'b';" => "(include (. \"a\" \"b\"));",
        "clone $a->b;" => "(clone (-> $a b));",
        "clone($a)->b;" => "(clone (-> $a b));",
        "clone($a, ['x' => 1]);" => "(clone-with $a ([] \"x\"=>1));",
        "new A()->b();" => "(->call (new A) b);",
        "new class {}::C;" => "(:: (new (class Class { })) C);",
        "$a instanceof $b;" => "(instanceof $a $b);",
        "$a instanceof ('A' . 'B');" => "(instanceof $a (. \"A\" \"B\"));",
    }
}

#[test]
fn variables_and_access() {
    check! {
        "$a->b->c();" => "(->call (-> $a b) c);",
        "$a?->b?->c;" => "(?-> (?-> $a b) c);",
        "$a->$b;" => "(-> $a {$b});",
        "$a->{'x' . 1};" => "(-> $a {(. \"x\" 1)});",
        "$$a;" => "($$a);",
        "${'a'};" => "($\"a\");",
        "$$$a[0];" => "([] ($($$a)) 0);",
        "A::$b;" => "(:: A $b);",
        "A::$$b;" => "(:: A ${$b});",
        "A::B;" => "(:: A B);",
        "A::class;" => "(:: A class);",
        "$a::class;" => "(:: $a class);",
        "A::{'B'};" => "(:: A {\"B\"});",
        "static::f();" => "(::call static f);",
        "A::$b();" => "(::call A {$b});",
        "A::b()();" => "(call (::call A b));",
        "$a[0][1];" => "([] ([] $a 0) 1);",
        "$a[] = 1;" => "(= ([] $a) 1);",
        "FOO[0];" => "([] FOO 0);",
        "\"abc\"[1];" => "([] \"abc\" 1);",
        "[1, 2][0];" => "([] ([] 1 2) 0);",
        "(function () {})();" => "(call (function() {}));",
        "f(...)();" => "(call (call f ...));",
        "\\Foo\\bar();" => "(call \\Foo\\bar);",
        "namespace\\f();" => "(call namespace\\f);",
        "a\\b\\C::D;" => "(:: a\\b\\C D);",
        "new $a;" => "(new $a);",
        "new $a->b['c'];" => "(new ([] (-> $a b) \"c\"));",
        "new (trim('A'));" => "(new (call trim \"A\"));",
        "new static;" => "(new static);",
        "new A::$b;" => "(new (:: A $b));",
        "f(a: 1, ...$b);" => "(call f a:1 ...$b);",
        "f(array: 1, class: 2);" => "(call f array:1 class:2);",
        "readonly(1);" => "(call readonly 1);",
        "exit;" => "(exit);",
        "die(1);" => "(exit 1);",
        "exit(...);" => "(exit ...);",
        "isset($a, $b[0]);" => "(isset $a ([] $b 0));",
        "list($a, , $b) = $c;" => "(= (list $a _ $b) $c);",
        "[$a, [$b, $c]] = $d;" => "(= (list[] $a (list[] $b $c)) $d);",
        "['k' => $a, 'l' => list($b)] = $c;" => "(= (list[] \"k\"=>$a \"l\"=>(list $b)) $c);",
        "[1, 2, ...$a, 'k' => &$b];" => "([] 1 2 ...$a \"k\"=>&$b);",
        "array(1, 2,);" => "(array 1 2);",
    }
}

#[test]
fn literals() {
    check! {
        "0x1F; 0b101; 0o17; 017; 1_000; 0;" => "31;\n5;\n15;\n15;\n1000;\n0;",
        "9223372036854775807; 9223372036854775808;" => "9223372036854775807;\n9.223372036854776e18;",
        "0x7FFFFFFFFFFFFFFF; 0xFFFFFFFFFFFFFFFF;" => "9223372036854775807;\n1.8446744073709552e19;",
        "1.5; .5; 1.; 1e3; 1_0.5e-1_0;" => "1.5;\n0.5;\n1.0;\n1000.0;\n1.05e-9;",
        "'a\\'b\\\\c\\n';" => "\"a'b\\\\c\\\\n\";",
        "\"a\\tb\\n\\x41\\101\\u{1F600}\\e\\$\\\"\\q\";" => "\"a\\tb\\nAA\\xf0\\x9f\\x98\\x80\\x1b$\\\"\\\\q\";",
        "\"$a $b[1] $c->d ${e} {$f['g']} ${h[1]}\";" => "(interp $a \" \" ([] $b 1) \" \" (-> $c d) \" \" $e \" \" ([] $f \"g\") \" \" ([] $h 1));",
        "\"$a[-1] $a[01] $a[x] $a[$i] $a[-0]\";" => "(interp ([] $a -1) \" \" ([] $a \"01\") \" \" ([] $a \"x\") \" \" ([] $a $i) \" \" ([] $a \"-0\"));",
        "b'x'; b\"y\\n\";" => "\"x\";\n\"y\\n\";",
        "`ls $d`;" => "(shell \"ls \" $d);",
        "__LINE__; __CLASS__; __DIR__;" => "Line;\nClass;\nDir;",
        "true; null; \\false;" => "true;\nnull;\n\\false;",
    }
}

#[test]
fn heredoc() {
    check! {
        "<<<EOT\n  a\n   b\n  EOT;" => "\"a\\n b\";",
        "<<<EOT\n  a $x\n    b\n  EOT;" => "(interp \"a \" $x \"\\n  b\");",
        "<<<'EOT'\n    a\\n\n  b\n  EOT;" => "\"  a\\\\n\\nb\";",
        "<<<EOT\nEOT;" => "\"\";",
        "<<<EOT\n\nEOT;" => "\"\";",
        "<<<EOT\n\n\nEOT;" => "\"\\n\";",
        "<<<\"EOT\"\na\\tb\nEOT;" => "\"a\\tb\";",
        "<<<EOT\n  {$a}\n  EOT;" => "(interp $a);",
        "<<<EOT\n\ta\n\tEOT;" => "\"a\";",
        "[<<<A\n  x\n  A, 1];" => "([] \"x\" 1);",
    }
}

#[test]
fn statements() {
    check! {
        "if ($a) b(); elseif ($c) d(); else e();" => "(if $a {(call b);} elseif $c {(call d);} else {(call e);})",
        "if ($a): b(); elseif ($c): d(); else: e(); endif;" => "(if $a {(call b);} elseif $c {(call d);} else {(call e);})",
        "if ($a) { } else if ($b) { }" => "(if $a {{}} else {(if $b {{}})})",
        "while ($a) { b(); }" => "(while $a {{(call b);}})",
        "while ($a): b(); endwhile;" => "(while $a {(call b);})",
        "do { a(); } while ($b);" => "(do {{(call a);}} $b)",
        "for ($i = 0, $j = 1; $i < 3; $i++, (void)f()) ;" => "(for [(= $i 0),(= $j 1)] [(< $i 3)] [(post++ $i),(cast:void (call f))] {;})",
        "for (;;): endfor;" => "(for [] [] [] {})",
        "foreach ($a as $k => &$v) {}" => "(foreach $a as $k => &$v {{}})",
        "foreach ($a as [$x, $y]) {}" => "(foreach $a as (list[] $x $y) {{}})",
        "foreach ($a as list('k' => $x)): endforeach;" => "(foreach $a as (list \"k\"=>$x) {})",
        "switch ($a) { case 1: b(); break; default: c(); }" => "(switch $a [case 1: (call b); (break)] [default: (call c);])",
        "switch ($a): case 1; endswitch;" => "(switch $a [case 1: ])",
        "break 2; continue; return;" => "(break 2)\n(continue)\n(return)",
        "global $a, $$b;" => "(global $a ($$b))",
        "static $a = 1, $b;" => "(static $a=1 $b)",
        "unset($a, $b[1],);" => "(unset $a ([] $b 1))",
        "try { a(); } catch (A|B $e) { } catch (C) { } finally { }" => "(try {(call a);} catch(A|B $e) {} catch(C) {} finally {})",
        "goto end; end: echo 1;" => "(goto end)\n(label end)\n(echo 1)",
        "declare(strict_types=1);" => "(declare strict_types=1)",
        "declare(ticks=1) { a(); }" => "(declare ticks=1 {{(call a);}})",
        "declare(ticks=1): a(); enddeclare;" => "(declare ticks=1 {(call a);})",
        "namespace A\\B; use C\\D as E, F; use function g; use const H;" => "(namespace A\\B)\n(use C\\D as E, F)\n(use function  g)\n(use const  H)",
        "namespace { } namespace A { }" => "(namespace  {})\n(namespace A {})",
        "use A\\{B, function c as d, const E};" => "(use A\\{B, function c as d, const E})",
        "const A = 1, B = A + 1;" => "(const A=1 B=(+ A 1))",
        "echo 1, 2; print 3;" => "(echo 1 2)\n(print 3);",
        "?>x<?php echo 1 ?>y<?= 2 ?>" => ";\n(html \"x\")\n(echo 1)\n(html \"y\")\n(echo 2)",
        "(void) f();" => "(cast:void (call f));",
        "__halt_compiler(); junk" => "(halt 24)",
        ";" => ";",
    }
}

#[test]
fn declarations() {
    check! {
        "function &f(int $a, ?B &...$c): static {}" => "(function &f(int $a, ?B &...$c):static {})",
        "function &f(int $a = 1, ?B &...$c): ?array { return 1; }" => "(function &f(int $a=1, ?B &...$c):?array {(return 1)})",
        "function f(A|B $a, A&B $b, (A&B)|null $c): void {}" => "(function f(A|B $a, (A&B) $b, (A&B)|null $c):void {})",
        "#[A, B(1)] #[C] function f() {}" => "#[A, B(1)] #[C] (function f() {})",
        "$f = static function &($a) use ($b, &$c): int {};" => "(= $f (static function&($a) use($b, &$c):int {}));",
        "$f = #[A] static fn&(int ...$a) => 1;" => "(= $f (static fn&(int ...$a) => 1));",
        "abstract class A extends B implements C, D { }" => "(abstract Class A extends B implements C,D { })",
        "final readonly class A {}" => "(final readonly Class A { })",
        "interface I extends A, B { const X = 1; public function f(); }" => "(Interface I implements A,B { (const X=1) (method public f();) })",
        "trait T { use U; abstract protected static function f(): int; }" => "(Trait T { (use U) (method protected static abstract f():int;) })",
        "enum E: string implements I { case A = 'a'; case B; const C = self::A; }" => "(Enum E:string implements I { (case A=\"a\") (case B) (const C=(:: self A)) })",
        "class A { public $a, $b = 1; var $c; public static ?int $d = null; private readonly array $e; }" => "(Class A { (prop public $a $b=1) (prop var $c) (prop public static ?int $d=null) (prop private readonly array $e) })",
        "class A { const X = 1, Y = 2; final public const int Z = 3; private const static|B W = B; }" => "(Class A { (const X=1 Y=2) (const public final int Z=3) (const private static|B W=B) })",
        "class A { public function __construct(private readonly int $x = 0, public private(set) string $y = '') {} }" => "(Class A { (method public __construct(private readonly int $x=0, public private(set) string $y=\"\") {}) })",
        "class A { public int $x { get => 1; set(int $v) { $this->x = $v; } } }" => "(Class A { (prop public int $x {get => 1 set(int $v) {(= (-> $this x) $v);}}) })",
        "class A { public $x { final get; &set; } }" => "(Class A { (prop public $x {final get; &set;}) })",
        "class A { use T, U { T::f insteadof U; U::f as g; f as protected; f as private h; } }" => "(Class A { (use T,U [T::f insteadof U] [U::f as g] [f as protected] [f as private h]) })",
        "class A { function list() {} function class() {} const FOREACH = 1; }" => "(Class A { (method  list() {}) (method  class() {}) (const FOREACH=1) })",
        "$o = new class(1) extends A implements B { public $x; };" => "(= $o (new (class Class extends A implements B { (prop public $x) }) 1));",
        "$o = new readonly class {};" => "(= $o (new (class readonly Class { })));",
        "match($a) { 1, 2 => 'x', default => 'y', };" => "(match $a [1, 2 => \"x\"] [default => \"y\"]);",
        "match(true) {};" => "(match true);",
    }
}

#[test]
fn spans_and_lines() {
    let src = b"<?php\nfunction f(\n  $a\n) {\n  return 1;\n}\n$x = 1 +\n  2;\n";
    let arena = goro_syntax::Arena::new();
    let p = parse(&arena, src).unwrap();
    let f = &p.stmts[0];
    assert_eq!((f.span.line, f.span.end_line), (2, 6));
    let x = &p.stmts[1];
    assert_eq!((x.span.line, x.span.end_line), (7, 8));
    assert_eq!(
        &src[x.span.start as usize..x.span.end as usize],
        b"$x = 1 +\n  2;"
    );
}

#[test]
fn compile_errors_and_notices() {
    let arena = goro_syntax::Arena::new();
    let p = parse(&arena, b"<?php $a = 1 ? 2 : 3 ? 4 : 5;").unwrap();
    assert_eq!(
        p.compile_errors[0].message,
        "Unparenthesized `a ? b : c ? d : e` is not supported. Use either `(a ? b : c) ? d : e` or `a ? b : (c ? d : e)`"
    );
    let p = parse(&arena, b"<?php $a = (1 ? 2 : 3) ? 4 : 5; $b = 1 ?: 2 ?: 3;").unwrap();
    assert!(p.compile_errors.is_empty());
    let p = parse(&arena, b"<?php $a = 1 |> fn($x) => $x;").unwrap();
    assert_eq!(
        p.compile_errors[0].message,
        "Arrow functions on the right hand side of |> must be parenthesized"
    );
    let p = parse(&arena, b"<?php $a = 1 |> (fn($x) => $x);").unwrap();
    assert!(p.compile_errors.is_empty());
    let p = parse(&arena, b"<?php\n$a = (integer) $b;").unwrap();
    assert_eq!(
        p.notices[0].message,
        "Non-canonical cast (integer) is deprecated, use the (int) cast instead"
    );
    assert_eq!(p.notices[0].line, 2);
    let e = parse(&arena, b"<?php class A { public private $x; }").unwrap_err();
    assert_eq!(
        e.to_string(),
        "Fatal error: Multiple access type modifiers are not allowed on line 1"
    );
    let e = parse(&arena, b"<?php abstract final class A {}").unwrap_err();
    assert_eq!(
        e.message,
        "Cannot use the final modifier on an abstract class"
    );
}
