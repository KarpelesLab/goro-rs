--TEST--
A trait set hook parameter type must be compatible with the property type
--FILE--
<?php
trait T { public int $x { set(string $v) {} } }
--EXPECTF--

Fatal error: Type of parameter $v of hook T::$x::set must be compatible with property type in %s on line 2
