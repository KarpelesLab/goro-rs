--TEST--
Interface get-only property: the implementing property's type must be a subtype
--FILE--
<?php
interface I { public int $x { get; } } class C implements I { public string $x = ""; }
echo "ok\n";
--EXPECTF--

Fatal error: Type of C::$x must be subtype of int (as in class I) in %s on line 2
