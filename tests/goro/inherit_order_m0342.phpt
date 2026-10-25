--TEST--
Inheritance check order: trait method collisions before trait constants and properties
--FILE--
<?php
trait T1 { abstract public function o(...$a); public function m($x = 1): void { } public const L = 2; private const K = 2; }
trait T2 { protected static int $a = 1; protected function m(string $x, $y = 2): int { return 1; } abstract protected function n(): int; }
interface J {  }
interface I extends J { public function o(string $x, $y = 2): mixed; }
abstract class A implements J { private string $a = 's'; public const L = 2; }
abstract class B extends A implements I { use T1, T2; public const K = 2; private const L = 2; }
class C extends B { private function o(...$a): void { } }
--EXPECTF--

Fatal error: Trait method T2::m has not been applied as B::m, because of collision with T1::m in %s on line 7
