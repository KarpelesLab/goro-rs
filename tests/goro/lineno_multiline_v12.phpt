--TEST--
Multi-line compound assignments / logical operators / arrow functions: opline lines (v12)
--FILE--
<?php
$f = fn() =>
    $u;
$f();
$g = function () {
    return $u2
        ;
};
$g();
--EXPECTF--

Warning: Undefined variable $u in %s on line 3

Warning: Undefined variable $u2 in %s on line 6
