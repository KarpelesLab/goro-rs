--TEST--
return; in a typed property get hook names a method
--FILE--
<?php
class A { public int $x { get { return; } } }
--EXPECTF--

Fatal error: A method with return type must return a value in %s on line 2
