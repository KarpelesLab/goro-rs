--TEST--
Interface get/set property: the implementing property's type is invariant
--FILE--
<?php
interface I { public int $x { get; set; } } class C implements I { public int|string $x = 0; }
echo "ok\n";
--EXPECTF--

Fatal error: Type of C::$x must be int (as in class I) in %s on line 2
