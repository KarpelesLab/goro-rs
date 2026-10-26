--TEST--
A typed plain property redeclaring an untyped set-hooked one
--FILE--
<?php
class A { public $x { set {} } } class B extends A { public int $x; }
--EXPECTF--

Fatal error: Type of B::$x must be omitted to match the parent definition in class A in %s on line 2
