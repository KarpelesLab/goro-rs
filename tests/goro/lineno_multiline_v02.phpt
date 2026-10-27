--TEST--
Multi-line compound assignments / logical operators / arrow functions: opline lines (v02)
--FILE--
<?php
$arr = [];
$arr
    ['x']
    += 1;
$o = new stdClass;
$o
    ->p
    .= "x";
--EXPECTF--

Warning: Undefined array key "x" in %s on line 4

Warning: Undefined property: stdClass::$p in %s on line 8
