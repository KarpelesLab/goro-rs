--TEST--
Inheritance checks: trait property composition before parent checks
--FILE--
<?php
trait T { public $x = 2; }
class A { public int $x = 1; public static function m() {} }
class B extends A { use T; public $x = 1; public function m() {} }
--EXPECTF--

Fatal error: B and T define the same property ($x) in the composition of B. However, the definition differs and is considered incompatible. Class was composed in %s on line 4
