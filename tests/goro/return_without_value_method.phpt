--TEST--
return; in a method with a return type names a method
--FILE--
<?php
class A { function f(): int { return; } }
--EXPECTF--

Fatal error: A method with return type must return a value in %s on line 2
