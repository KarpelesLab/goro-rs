--TEST--
Trait properties are checked against the parent's before methods
--FILE--
<?php
trait T { private $b = "t"; }
class A { protected int $b = 5; public static function run() {} }
class B extends A { use T; private function run() {} }
--EXPECTF--

Fatal error: Access level to B::$b must be protected (as in class A) or weaker in %s on line 4
