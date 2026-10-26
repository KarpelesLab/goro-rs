--TEST--
Equivalent set visibilities are accepted when redeclaring
--FILE--
<?php
class A { protected int $x; } class B extends A { protected protected(set) int $x; } echo "ok\n";
--EXPECT--
ok
