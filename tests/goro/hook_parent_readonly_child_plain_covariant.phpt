--TEST--
A plain property may narrow a get-only virtual property type
--FILE--
<?php
class A { public int|string $x { get => 1; } } class B extends A { public int $x; } echo "ok\n";
--EXPECT--
ok
