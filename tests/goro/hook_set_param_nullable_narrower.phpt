--TEST--
A ?int set hook parameter does not accept an int|string property
--FILE--
<?php
class A { public int|string $x { set(?int $v) { } } }
--EXPECTF--

Fatal error: Type of parameter $v of hook A::$x::set must be compatible with property type in %s on line 2
