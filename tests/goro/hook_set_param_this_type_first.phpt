--TEST--
A set hook parameter type error comes before the $this name error
--FILE--
<?php
class A { public $p { set(int $this) {} } }
--EXPECTF--

Fatal error: Type of parameter $this of hook A::$p::set must be compatible with property type in %s on line 2
