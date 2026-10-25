--TEST--
Inheritance check order: abstract trait method checked against an inherited abstract method
--FILE--
<?php
trait T1 { private function o(int $x): ?int { return null; } }
trait T2 { abstract public function n(string $x, $y = 2): mixed; }
interface J { public function n($x = 1): mixed; public const K = 1; }
interface I extends J { public function m(): void; }
abstract class A implements J {  }
abstract class B extends A implements I { use T1, T2; public ?int $a = null; public ?int $b = null; protected function o(string $x, $y = 2): int { return 1; } }
class C extends B { protected $b = 1; private function m(): void { } }
--EXPECTF--

Fatal error: Declaration of J::n($x = 1): mixed must be compatible with T2::n(string $x, $y = 2): mixed in %s on line 4
