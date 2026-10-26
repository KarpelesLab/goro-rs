--TEST--
A set hook parameter of a subclass type is incompatible
--FILE--
<?php
class P {} class C extends P {} class A { public P $x { set(C $v) { } } } echo "ok\n";
--EXPECTF--

Fatal error: Type of parameter $v of hook A::$x::set must be compatible with property type in %s on line 2
