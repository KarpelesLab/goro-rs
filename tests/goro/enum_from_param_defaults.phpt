--TEST--
ReflectionParameter::getDefaultValue() of enum/constant-expression class constants; from()/tryFrom() of pure enums and with null; method_exists() of enum methods
--FILE--
<?php
enum En: int { case A = 1; case B = 2; }
enum P { case X; }
class Q {
    const K = En::A;
    const L = [En::B, 3];
    const M = self::K;
    function m(En $e = En::A, $f = self::K, $g = self::L, $h = Q::M, $i = P::X, $j = [En::A]) {}
}
foreach ((new ReflectionMethod('Q', 'm'))->getParameters() as $p) {
    var_dump($p->getDefaultValue());
    echo $p->isDefaultValueConstant() ? $p->getDefaultValueConstantName() : '-', "\n";
}
function t($l, $f) { try { $r = $f(); echo "$l: "; var_dump($r); } catch (\Throwable $e) { echo "$l: EXC ", get_class($e), ": ", $e->getMessage(), "\n"; } }
t(1, fn() => P::from('X'));
t(2, fn() => P::tryFrom('X'));
t(3, fn() => [method_exists(P::X, 'cases'), method_exists('P', 'from'), method_exists('En', 'from'), method_exists(En::A, 'tryFrom')]);
t(4, fn() => En::tryFrom(null));
t(5, fn() => En::from(null));
t(6, fn() => En::tryFrom('1'));
t(7, fn() => En::from(3));
--EXPECTF--
enum(En::A)
En::A
enum(En::A)
self::K
array(2) {
  [0]=>
  enum(En::B)
  [1]=>
  int(3)
}
self::L
enum(En::A)
Q::M
enum(P::X)
P::X
array(1) {
  [0]=>
  enum(En::A)
}
-
1: EXC Error: Call to undefined method P::from()
2: EXC Error: Call to undefined method P::tryFrom()
3: array(4) {
  [0]=>
  bool(true)
  [1]=>
  bool(false)
  [2]=>
  bool(true)
  [3]=>
  bool(true)
}

Deprecated: En::tryFrom(): Passing null to parameter #1 ($value) of type string|int is deprecated in %s on line 18
4: NULL

Deprecated: En::from(): Passing null to parameter #1 ($value) of type string|int is deprecated in %s on line 19
5: EXC ValueError: 0 is not a valid backing value for enum En
6: enum(En::A)
7: EXC ValueError: 3 is not a valid backing value for enum En
