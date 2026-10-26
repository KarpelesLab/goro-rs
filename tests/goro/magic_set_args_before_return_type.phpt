--TEST--
Magic method argument count is checked before its return type
--FILE--
<?php
class A { function __set($n): int {} }
--EXPECTF--

Fatal error: Method A::__set() must take exactly 2 arguments in %s on line 2
