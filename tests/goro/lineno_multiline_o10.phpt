--TEST--
Multi-line static call / property assignment / unary operators / sprintf(): opline lines (o10)
--FILE--
<?php
$x = 1;
$y = $x
    instanceof stdClass;
$z = !
    $u;
$w = -
    $u2;
$v = (string)
    $u3;
--EXPECTF--

Warning: Undefined variable $u in %s on line 6

Warning: Undefined variable $u2 in %s on line 8

Warning: Undefined variable $u3 in %s on line 10
