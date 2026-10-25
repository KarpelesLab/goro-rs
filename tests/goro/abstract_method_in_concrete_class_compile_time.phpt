--TEST--
An abstract method in a class not declared abstract is a compile error
--FILE--
<?php
echo "never printed\n";
class P { final public function f() {} }
class C extends P { public function f() {} abstract function g(); }
--EXPECTF--

Fatal error: Class C declares abstract method g() and must therefore be declared abstract in %s on line 4
