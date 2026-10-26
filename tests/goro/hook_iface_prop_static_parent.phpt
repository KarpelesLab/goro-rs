--TEST--
A parent's static property doesn't implement an interface's instance property
--FILE--
<?php
interface I { public $x { get; } } class P { public static $x; } class A extends P implements I { } echo "ok\n";
--EXPECTF--

Fatal error: Cannot redeclare non static I::$x as static A::$x in %s on line 2
