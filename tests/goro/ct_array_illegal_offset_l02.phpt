--TEST--
A constant array literal with an array key is a compile-time 'Illegal offset type' (l02)
--FILE--
<?php
$x = 1;
$a = [$x, [[1] => 2]];
--EXPECTF--

Fatal error: Illegal offset type in %s on line 3
