--TEST--
A class early-bound at compile time reports its linking error before a later compile error
--FILE--
<?php
const X = 1;
function f() { return 1; }
class P { public function m(int $a) {} }
class Q extends P { public function m(string $a) {} }
interface I { function g(); }
class R implements I { function g(int $a) {} }
class S { public int $x = "s"; }
?>
--EXPECTF--
Fatal error: Declaration of Q::m(string $a) must be compatible with P::m(int $a) in %s on line 5
