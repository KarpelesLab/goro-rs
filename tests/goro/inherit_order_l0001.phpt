--TEST--
Inheritance check order: final parent constant reported before its access level
--FILE--
<?php
trait T { abstract protected function n(int $x): void; }
interface I { public function n(int $x): int; public static function m($x = 1): int; public const L = 2; public const K = 1; }
abstract class A { private function n(string $x, $y = 2): static { return $this; } final public const L = 1; }
class B extends A { private const L = 1; public const K = 2; }
--EXPECTF--

Fatal error: B::L cannot override final constant A::L in %s on line 5
