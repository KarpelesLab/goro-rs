--TEST--
private(set) readonly overriding public readonly
--FILE--
<?php
class A { public readonly int $x; } class B extends A { public private(set) readonly int $x; }
--EXPECTF--

Fatal error: Set access level of B::$x must be protected(set) (as in class A) or weaker in %s on line 2
