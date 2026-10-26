--TEST--
Interfaces with parents and traits using traits are not early-bound (declared at their position)
--FILE--
<?php
var_dump(interface_exists('I2', false), interface_exists('J2', false), trait_exists('A', false), trait_exists('B2', false));
interface I2 extends J2 {} interface J2 {}
trait A { use B2; }
trait B2 { function b() { return 'b'; } }
var_dump(interface_exists('I2', false), trait_exists('A', false));
class C { use A; }
var_dump((new C)->b());
?>
--EXPECT--
bool(false)
bool(true)
bool(false)
bool(true)
bool(true)
bool(true)
string(1) "b"
