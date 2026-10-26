--TEST--
A call's discarded result is released before the callee's locals and $this (ZEND_RETURN without return_value)
--FILE--
<?php
class A { function __destruct() { echo "A dtor\n"; } function m() { return new B; } function n() { return $this; } function o() { $b = new B; return $b; } }
class B { function __destruct() { echo "B dtor\n"; } }
function f($x) { return new B; }
function g($x) { $b = new B; return $b; }
function h() { $a = new A; return new B; }
(new A)->m(); echo "--\n";
$a = new A; $a->m(); echo "--\n"; unset($a);
(new A)->n()->m(); echo "--\n";
(new A)->o(); echo "--\n";
f(new A); echo "--\n";
g(new A); echo "--\n";
h(); echo "--\n";
$x = f(new A); echo "-- kept\n"; unset($x);
echo "end\n";
--EXPECTF--
B dtor
A dtor
--
B dtor
--
A dtor
B dtor
A dtor
--
B dtor
A dtor
--
B dtor
A dtor
--
A dtor
B dtor
--
B dtor
A dtor
--
A dtor
-- kept
B dtor
end
