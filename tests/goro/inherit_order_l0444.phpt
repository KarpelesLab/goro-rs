--TEST--
Inheritance check order: interface static mismatch with an inherited method: "in class" of the method
--FILE--
<?php
trait T { private string $b = 's'; protected const K = 2; }
interface I { public function n(): void; }
class A { public static function n(string $x, $y = 2): int { return 1; } private const L = 2; }
class B extends A implements I { use T; private static function m(int $x): static { return new static; } }
--EXPECTF--

Fatal error: Cannot make non static method I::n() static in class A in %s on line 4
