--TEST--
Inheritance check order: unimplemented abstract method named after the interface declaring it
--FILE--
<?php
trait T1 { public $a = 1; }
trait T2 { private function m(int $x): void { } protected function n(): ?int { return null; } }
interface J { public function o(string $x, $y = 2): mixed; }
interface I extends J {  }
abstract class A implements J { private readonly string $a; private const K = 2; private const L = 2; }
class B extends A implements I { use T1, T2 { T2::m as protected ; } public $b = 1; private const K = 1; }
class C extends B { protected function n(int $x): int { return 1; } }
--EXPECTF--

Fatal error: Class B contains 1 abstract method and must therefore be declared abstract or implement the remaining method (J::o) in %s on line 7
