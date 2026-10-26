--TEST--
A public readonly property may redeclare a protected readonly one
--FILE--
<?php
class A { protected readonly int $x; } class B extends A { public readonly int $x; } echo "ok\n";
--EXPECT--
ok
