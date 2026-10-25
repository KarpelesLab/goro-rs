--TEST--
A trait property is checked against the inherited one it replaces
--FILE--
<?php
trait T { protected $c = "t"; }
class A { public $c = 5; }
class B extends A { use T; }
echo "unreachable\n";
--EXPECTF--

Fatal error: Access level to B::$c must be public (as in class A) in %s on line 4
