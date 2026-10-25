--TEST--
A by-reference set hook returns by reference from a void function
--FILE--
<?php
class A { public $x { &set {} } }
echo "ok\n";
--EXPECTF--

Deprecated: A::$x::set(): Returning by reference from a void function is deprecated in %s on line 2
ok
