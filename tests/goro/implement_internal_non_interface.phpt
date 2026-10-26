--TEST--
Implementing an internal class that is not an interface
--FILE--
<?php
class A implements stdClass {}
--EXPECTF--

Fatal error: A cannot implement stdClass - it is not an interface in %s on line 2
