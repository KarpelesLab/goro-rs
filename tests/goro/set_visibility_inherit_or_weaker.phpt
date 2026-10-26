--TEST--
A set visibility stricter than the parent set visibility
--FILE--
<?php
class A { public protected(set) int $x; } class B extends A { public private(set) int $x; }
--EXPECTF--

Fatal error: Set access level of B::$x must be protected(set) (as in class A) or weaker in %s on line 2
