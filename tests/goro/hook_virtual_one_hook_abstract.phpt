--TEST--
A virtual hooked property with one hook leaves the other abstract
--FILE--
<?php
interface I { public $x { get; set; } } class A implements I { public $x { get { return 5; } } } echo "ok\n";
--EXPECTF--

Fatal error: Class A contains 1 abstract method and must therefore be declared abstract or implement the remaining method (I::$x::set) in %s on line 2
