--TEST--
Without a compile-time known callee, a variable argument is sent on the argument list's line (a24)
--FILE--
<?php
class A { function m($a, $b = 0) {} static function s($a, $b = 0) {} }
$o = new A;
$o->m(
    1,
    $u1
);
$o->m(
    $u2
);
A::s(
    1,
    $u3
);
later(
    1,
    $u4
);
later(
    $u5,
    2
);
function later($a, $b) {}
$o->m(
    $u6 + 1,
    $u7
);
--EXPECTF--

Warning: Undefined variable $u1 in %s on line 5

Warning: Undefined variable $u2 in %s on line 9

Warning: Undefined variable $u3 in %s on line 13

Warning: Undefined variable $u4 in %s on line 16

Warning: Undefined variable $u5 in %s on line 20

Warning: Undefined variable $u6 in %s on line 25

Warning: Undefined variable $u7 in %s on line 25
