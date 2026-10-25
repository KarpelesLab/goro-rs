--TEST--
Static/non-static property redeclaration names the declaring class
--FILE--
<?php
class A { protected static int $a = 5; }
class B extends A { }
class C extends B { protected $a = [1]; }
--EXPECTF--

Fatal error: Cannot redeclare static A::$a as non static C::$a in %s on line 4
