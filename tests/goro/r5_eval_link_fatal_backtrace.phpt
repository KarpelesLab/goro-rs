--TEST--
A fatal error linking a class in eval()'d code has the eval() frame
--FILE--
<?php
function f() { eval('class D extends C { use T2; }'); } trait T2 { public static $s = 1; } class C { public $s = 0; } f();
--EXPECTF--

Fatal error: Cannot redeclare non static C::$s as static D::$s in %s(2) : eval()'d code on line 1
