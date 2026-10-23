--TEST--
Property inheritance errors name the class declaring the parent property
--FILE--
<?php
class A { protected $x = 1; }
class B extends A { }
class C extends B { private $x = 2; }
--EXPECTF--

Fatal error: Access level to C::$x must be protected (as in class A) or weaker in %s on line 4
