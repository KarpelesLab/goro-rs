--TEST--
Inheritance check order: trait methods are checked against the parent's in the parent's method order
--FILE--
<?php
trait T { protected string $b = 's'; public function n(int $x): int { return 1; } final public static function m(int $x): static { return new static; } }
interface I {  }
class A { public function n($x = 1): void { } protected function m(string $x, $y = 2): static { return $this; } }
class B extends A implements I { use T; public $a = 1; protected function m($x = 1): string { return 's'; } final public const K = 1; public const L = 2; }
--EXPECTF--

Fatal error: Declaration of B::n(int $x): int must be compatible with A::n($x = 1): void in %s on line 2
