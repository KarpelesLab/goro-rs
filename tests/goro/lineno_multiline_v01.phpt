--TEST--
Multi-line compound assignments / logical operators / arrow functions: opline lines (v01)
--FILE--
<?php
$a = 1;
$a
    += $u1;
$b = "x";
$b
    .= $u2;
$c = null;
$c
    ??= $u3;
--EXPECTF--

Warning: Undefined variable $u1 in %s on line 4

Warning: Undefined variable $u2 in %s on line 7

Warning: Undefined variable $u3 in %s on line 9
