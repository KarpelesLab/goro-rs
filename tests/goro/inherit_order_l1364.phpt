--TEST--
Inheritance check order: abstract trait method bound before interfaces are checked
--FILE--
<?php
trait T { public string $a = 's'; public $b = 1; protected function n(): string { return 's'; } abstract public function m(); }
interface I { public static function m(int $x): string; final public const L = 1; public const K = 2; }
class A { private string $b = 's'; }
class B extends A implements I { use T; public static function n($x = 1): static { return new static; } }
--EXPECTF--

Fatal error: Cannot make static method I::m() non static in class B in %s on line 2
