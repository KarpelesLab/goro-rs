--TEST--
Extra named arguments collected by a typed variadic are type-checked and coerced
--FILE--
<?php
function g(int ...$n) { var_dump($n); }
g(x: 1, y: "2");
try { g(1, x: "no"); } catch (TypeError $e) { echo $e->getMessage(), "\n"; }
class C { function m(string $a, int ...$r) { var_dump($r); } }
try { (new C)->m("a", 5, k: []); } catch (TypeError $e) { echo $e->getMessage(), "\n"; }
--EXPECTF--
array(2) {
  ["x"]=>
  int(1)
  ["y"]=>
  int(2)
}
g(): Argument #2 must be of type int, string given, called in %s on line 4
C::m(): Argument #3 must be of type int, array given, called in %s on line 6
