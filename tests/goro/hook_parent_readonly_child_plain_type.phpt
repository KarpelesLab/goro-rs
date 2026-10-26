--TEST--
A plain property redeclaring a get-only virtual one must have a subtype
--FILE--
<?php
class A { public int $x { get => 1; } } class B extends A { public string $x; }
--EXPECTF--

Fatal error: Type of B::$x must be subtype of int (as in class A) in %s on line 2
