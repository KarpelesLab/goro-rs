--TEST--
Implementing ArrayObject is a fatal error
--FILE--
<?php
class A implements ArrayObject {}
--EXPECTF--

Fatal error: A cannot implement ArrayObject - it is not an interface in %s on line 2
