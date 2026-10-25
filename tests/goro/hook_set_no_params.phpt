--TEST--
A set hook must accept exactly one parameter (none given)
--FILE--
<?php
class A { public $x { set() {} } }
echo "ok\n";
--EXPECTF--

Fatal error: set hook of property A::$x must accept exactly one parameters in %s on line 2
