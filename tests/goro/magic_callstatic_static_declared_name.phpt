--TEST--
Magic method signature errors use the declared method name
--FILE--
<?php
class A {
    function __CALLSTATIC($a, $b) {}
}
--EXPECTF--

Fatal error: Method A::__CALLSTATIC() must be static in %s on line 3
