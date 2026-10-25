--TEST--
Inheritance check order: interface method implemented by an inherited method: the method's class in the error
--FILE--
<?php
interface I { public function m(): void; public function n(int $x): static; public const K = 2; }
class A { public int $a = 1; private function m(string $x, $y = 2): string { return 's'; } public static function n($x = 1): static { return new static; } final public const L = 2; public const K = 1; }
class B extends A implements I { protected static $b = 1; public const K = 2; }
--EXPECTF--

Fatal error: Access level to A::m() must be public (as in class I) in %s on line 3
