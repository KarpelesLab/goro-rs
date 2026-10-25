--TEST--
Interface property hooks cannot have a body
--FILE--
<?php
interface I { public $x { get => 1; } }
echo "ok\n";
--EXPECTF--

Fatal error: Abstract property hook cannot have body in %s on line 2
