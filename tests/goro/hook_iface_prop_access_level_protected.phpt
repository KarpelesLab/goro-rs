--TEST--
Interface property: a protected implementing property
--FILE--
<?php
interface I { public $x { get; } } class A implements I { protected $x; }
echo "ok\n";
--EXPECTF--

Fatal error: Access level to A::$x must be public (as in class I) in %s on line 2
