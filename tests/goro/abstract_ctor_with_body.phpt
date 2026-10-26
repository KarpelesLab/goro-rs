--TEST--
An abstract constructor cannot contain a body
--FILE--
<?php
abstract class A { abstract function __construct() {} }
--EXPECTF--

Fatal error: Abstract function A::__construct() cannot contain body in %s on line 2
