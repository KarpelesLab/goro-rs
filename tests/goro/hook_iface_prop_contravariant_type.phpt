--TEST--
Interface set-only property: the implementing property's type must be a supertype
--FILE--
<?php
interface I { public int|string $x { set; } } class C implements I { public int $x = 0; }
echo "ok\n";
--EXPECTF--

Fatal error: Type of C::$x must be supertype of string|int (as in class I) in %s on line 2
