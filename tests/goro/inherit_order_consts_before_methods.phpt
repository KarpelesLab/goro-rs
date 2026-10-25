--TEST--
Inheritance checks: constants before methods
--FILE--
<?php
class A { const C = 1; public function f() {} }
class B extends A { private const C = 2; private function f() {} }
--EXPECTF--

Fatal error: Access level to B::C must be public (as in class A) in %s on line 3
