--TEST--
An abstract method cannot contain a body
--FILE--
<?php
abstract class A {
    public function g() {}
    abstract static
    function f()
    {
    }
}
--EXPECTF--

Fatal error: Abstract function A::f() cannot contain body in %s on line 5
