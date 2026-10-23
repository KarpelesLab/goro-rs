--TEST--
Failed compound assignment leaves typed slots with the coerced UNDEF result
--FILE--
<?php
// A compound assignment whose operation throws leaves a typed slot with the
// weakly coerced UNDEF result (int 0, float 0, "", false), an untyped one
// unchanged, and an undefined variable NULL.
class C {
    public int $t = 5; public ?int $n = 5; public float $f = 5.0; public string $s = "5";
    public int|string $u = 5; public bool $b = true; public $x = 5; public mixed $m = 5;
    public ?string $ns = "x"; public string|bool $sb = "q"; public array $a = [1]; public iterable $it = [];
}
foreach (['t', 'n', 'f', 's', 'u', 'b', 'x', 'm', 'ns', 'sb', 'a', 'it'] as $p) {
    $o = new C;
    try { $o->$p <<= -1; } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
    echo $p, ": "; var_dump($o->$p);
}
$o = new C;
try { $o->t %= 0; } catch (Error $e) { echo $e->getMessage(), "\n"; }
try { $o->f /= 0; } catch (Error $e) { echo $e->getMessage(), "\n"; }
var_dump($o->t, $o->f);
$o->s = "ab";
try { $o->s .= new class { function __toString(): string { throw new Exception("ts"); } }; } catch (Exception $e) { echo $e->getMessage(), "\n"; }
var_dump($o->s);

// typed reference and typed static property
$o = new C; $r = &$o->t; $o->t = 7;
try { $r <<= -1; } catch (Error $e) { echo $e->getMessage(), "\n"; }
var_dump($o->t);
$arr = [&$o->f];
try { $arr[0] %= 0; } catch (Error $e) { echo $e->getMessage(), "\n"; }
var_dump($o->f);
class S { public static int $s = 3; public static $u = 3; }
try { S::$s %= 0; } catch (Error $e) { echo $e->getMessage(), "\n"; }
try { S::$u %= 0; } catch (Error $e) { echo $e->getMessage(), "\n"; }
var_dump(S::$s, S::$u);

// thrown from a called function
function h(C $o) { $o->t >>= -1; }
$o = new C;
try { h($o); } catch (Error $e) { echo $e->getMessage(), "\n"; }
var_dump($o->t);

// undefined variables
try { $u %= 0; } catch (Error $e) { echo $e->getMessage(), "\n"; }
var_dump($u);
function k() { try { $w <<= -1; } catch (Error $e) { echo $e->getMessage(), "\n"; } var_dump($w); }
k();
--EXPECTF--
ArithmeticError: Bit shift by negative number
t: int(0)
ArithmeticError: Bit shift by negative number
n: int(0)
ArithmeticError: Bit shift by negative number
f: float(0)
ArithmeticError: Bit shift by negative number
s: string(0) ""
ArithmeticError: Bit shift by negative number
u: int(0)
ArithmeticError: Bit shift by negative number
b: bool(false)
ArithmeticError: Bit shift by negative number
x: int(5)
ArithmeticError: Bit shift by negative number
m: int(0)
TypeError: Unsupported operand types: string << int
ns: string(0) ""
TypeError: Unsupported operand types: string << int
sb: string(0) ""
TypeError: Unsupported operand types: array << int
a: array(1) {
  [0]=>
  int(1)
}
TypeError: Unsupported operand types: array << int
it: array(0) {
}
Modulo by zero
Division by zero
int(0)
float(0)
ts
string(2) "ab"
Bit shift by negative number
int(0)
Modulo by zero
float(0)
Modulo by zero
Modulo by zero
int(0)
int(3)
Bit shift by negative number
int(0)

Warning: Undefined variable $u in %s on line 42
Modulo by zero
NULL

Warning: Undefined variable $w in %s on line 44
Bit shift by negative number
NULL
