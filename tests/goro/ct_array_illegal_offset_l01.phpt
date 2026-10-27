--TEST--
A constant array literal with an array key is a compile-time 'Illegal offset type' (l01)
--FILE--
<?php
$a = [
    'a' => 1,
    [] => 2,
];
--EXPECTF--

Fatal error: Illegal offset type in %s on line 3
