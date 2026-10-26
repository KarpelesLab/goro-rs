--TEST--
A public readonly property cannot implement an abstract property with a set hook
--FILE--
<?php
abstract class A { abstract public int $x { set; } } class C extends A { public function __construct() {} public readonly int $x; }
--EXPECTF--

Fatal error: Set access level of C::$x must be omitted (as in class A) in %s on line 2
