--TEST--
Class constant fetches by scope, deprecated constants, loose comparisons of scalars, returns
--FILE--
<?php
class A {
    const X = 'A::X';
    const Y = self::X . '+';
    protected const P = 'A::P';
    private const Q = 'A::Q';
    #[\Deprecated("use X")]
    const D = 'old';
    static function get() { return [self::X, self::Y, static::X, self::P, self::Q]; }
    function bound() { return fn() => self::X; }
}
class B extends A {
    const X = 'B::X';
    static function getB() { return [self::X, parent::X, self::P, A::X]; }
}
enum E: string { case One = 'one'; case Two = 'two'; const DEF = self::One; }
for ($i = 0; $i < 3; $i++) {
    print_r(A::get());
    print_r(B::get());
    print_r(B::getB());
    var_dump(E::DEF === E::One, E::Two->value, A::Y, B::Y);
    echo A::D, "\n";
}
$f = (new A)->bound();
echo $f(), "\n";
$g = Closure::bind(function () { return self::X; }, null, B::class);
for ($i = 0; $i < 2; $i++) { echo $g(), " ", (function () { return self::X; })->bindTo(null, A::class)(), "\n"; }
try { echo A::Q; } catch (Error $e) { echo $e->getMessage(), "\n"; }
// loose comparisons of scalars
$vals = ['1', '01', '1.0', 'abc', '', '0', null, false, true, 0, 1, 1.0, '1e0', ' 1', '1 '];
$n = 0;
foreach ($vals as $a) { foreach ($vals as $b) { if ($a == $b) $n++; if ($a != $b) $n += 100; } }
echo $n, "\n";
// returns
function r1($a) { return $a; }
function &r2(array &$a) { return $a[0]; }
function r3(): int { return '5'; }
function r4() { $x = [1, 2]; return $x; }
function r5($o) { return $o; }
$arr = [1];
$ref = &r2($arr); $ref = 9;
var_dump(r1('s'), $arr, r3(), r4(), r5(new stdClass) instanceof stdClass);
function gen() { yield 1; return 2; }
$gg = gen(); foreach ($gg as $v) {} var_dump($gg->getReturn());
function opt($a, $b = 5, $c = null) { return [$a, $b, $c]; }
var_dump(opt(1), opt(1, 2), opt(1, 2, 3), opt(1, c: 7));
--EXPECTF--
Array
(
    [0] => A::X
    [1] => A::X+
    [2] => A::X
    [3] => A::P
    [4] => A::Q
)
Array
(
    [0] => A::X
    [1] => A::X+
    [2] => B::X
    [3] => A::P
    [4] => A::Q
)
Array
(
    [0] => B::X
    [1] => A::X
    [2] => A::P
    [3] => A::X
)
bool(true)
string(3) "two"
string(5) "A::X+"
string(5) "A::X+"

Deprecated: Constant A::D is deprecated, use X in %s on line 22
old
Array
(
    [0] => A::X
    [1] => A::X+
    [2] => A::X
    [3] => A::P
    [4] => A::Q
)
Array
(
    [0] => A::X
    [1] => A::X+
    [2] => B::X
    [3] => A::P
    [4] => A::Q
)
Array
(
    [0] => B::X
    [1] => A::X
    [2] => A::P
    [3] => A::X
)
bool(true)
string(3) "two"
string(5) "A::X+"
string(5) "A::X+"

Deprecated: Constant A::D is deprecated, use X in %s on line 22
old
Array
(
    [0] => A::X
    [1] => A::X+
    [2] => A::X
    [3] => A::P
    [4] => A::Q
)
Array
(
    [0] => A::X
    [1] => A::X+
    [2] => B::X
    [3] => A::P
    [4] => A::Q
)
Array
(
    [0] => B::X
    [1] => A::X
    [2] => A::P
    [3] => A::X
)
bool(true)
string(3) "two"
string(5) "A::X+"
string(5) "A::X+"

Deprecated: Constant A::D is deprecated, use X in %s on line 22
old
A::X
B::X A::X
B::X A::X
Cannot access private constant A::Q
12303
string(1) "s"
array(1) {
  [0]=>
  &int(9)
}
int(5)
array(2) {
  [0]=>
  int(1)
  [1]=>
  int(2)
}
bool(true)
int(2)
array(3) {
  [0]=>
  int(1)
  [1]=>
  int(5)
  [2]=>
  NULL
}
array(3) {
  [0]=>
  int(1)
  [1]=>
  int(2)
  [2]=>
  NULL
}
array(3) {
  [0]=>
  int(1)
  [1]=>
  int(2)
  [2]=>
  int(3)
}
array(3) {
  [0]=>
  int(1)
  [1]=>
  int(5)
  [2]=>
  int(7)
}
