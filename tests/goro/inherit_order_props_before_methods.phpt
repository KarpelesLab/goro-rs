--TEST--
Inheritance checks: parent properties are checked before methods
--FILE--
<?php
class A { public readonly int $a; public function n() {} }
class B extends A { public $a; protected function n() {} }
--EXPECTF--

Fatal error: Cannot redeclare readonly property A::$a as non-readonly B::$a in %s on line 3
