--TEST--
A virtual parent property cannot be redeclared readonly
--FILE--
<?php
class C { public $x { get => 1; } } class D extends C { public readonly int $x; }
echo "ok\n";
--EXPECTF--

Fatal error: Cannot redeclare non-readonly property C::$x as readonly D::$x in %s on line 2
