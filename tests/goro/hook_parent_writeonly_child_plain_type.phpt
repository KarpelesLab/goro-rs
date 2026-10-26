--TEST--
A plain property redeclaring a set-only virtual one must have a supertype
--FILE--
<?php
class A { public int $x { set {} } } class B extends A { public string $x; }
--EXPECTF--

Fatal error: Type of B::$x must be supertype of int (as in class A) in %s on line 2
