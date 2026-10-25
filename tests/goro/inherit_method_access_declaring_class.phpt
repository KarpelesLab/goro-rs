--TEST--
Method access-level error names the declaring class
--FILE--
<?php
class A { public static function x() {} }
class B extends A { }
class C extends B { private static function x() {} }
--EXPECTF--

Fatal error: Access level to C::x() must be public (as in class A) in %s on line 4
