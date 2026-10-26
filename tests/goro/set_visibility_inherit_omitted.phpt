--TEST--
Adding set visibility stricter than the parent property visibility
--FILE--
<?php
class A { public int $x; } class B extends A { public protected(set) int $x; }
--EXPECTF--

Fatal error: Set access level of B::$x must be omitted (as in class A) in %s on line 2
