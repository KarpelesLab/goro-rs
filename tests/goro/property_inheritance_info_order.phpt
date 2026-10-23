--TEST--
Property inheritance errors follow the parent's properties_info order
--FILE--
<?php
class A { public int $c = 0; private array $x = []; }
class B extends A { protected $x = null; }
class C extends B { protected $c = 1; public int $x = 0; }
--EXPECTF--

Fatal error: Type of C::$x must be omitted to match the parent definition in class B in %s on line 4
