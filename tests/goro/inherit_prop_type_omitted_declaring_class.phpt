--TEST--
Property type-omitted error names the declaring class
--FILE--
<?php
class A { protected static $x = null; }
class B extends A { }
class C extends B { protected static int $x = 5; }
--EXPECTF--

Fatal error: Type of C::$x must be omitted to match the parent definition in class A in %s on line 4
