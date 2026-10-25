--TEST--
Inheritance check order: abstract trait methods of two traits are checked against each other
--FILE--
<?php
trait T1 { private int $c = 1; abstract protected function o(): void; public static function m(int $x): int { return 1; } }
trait T2 { private function n(...$a): ?int { return null; } abstract protected function o($x = 1): ?int; }
interface J {  }
interface I extends J {  }
abstract class A implements J { protected const K = 2; }
class B extends A implements I { use T1, T2; private function m(): ?int { return null; } public const L = 1; }
class C extends B { protected $a = 1; protected function o(int $x) { } }
--EXPECTF--

Fatal error: Declaration of T1::o(): void must be compatible with T2::o($x = 1): ?int in %s on line 2
