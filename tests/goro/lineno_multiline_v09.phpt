--TEST--
Multi-line compound assignments / logical operators / arrow functions: opline lines (v09)
--FILE--
<?php
$x = 5;
$y = $x <=>
    $u;
$z = $x ==
    $u;
$w = $x &&
    $u;
$v = $x and
    $u;
--EXPECTF--

Warning: Undefined variable $u in %s on line 4

Warning: Undefined variable $u in %s on line 6

Warning: Undefined variable $u in %s on line 8

Warning: Undefined variable $u in %s on line 10
