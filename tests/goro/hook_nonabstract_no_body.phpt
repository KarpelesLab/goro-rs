--TEST--
A non-abstract property hook must have a body
--FILE--
<?php
class A { public $x { get; } }
echo "ok\n";
--EXPECTF--

Fatal error: Non-abstract property hook must have a body in %s on line 2
