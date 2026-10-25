--TEST--
Inheritance check order: trait constants are bound before trait properties
--FILE--
<?php
trait T { private readonly string $a; protected string $b = 's'; private const K = 2; public const L = 1; }
interface I { public function n($x = 1): int; public function m(): int; public const K = 1; final public const L = 1; }
class A { protected int $a = 1; protected function m(string $x, $y = 2) { } }
class B extends A { use T; protected $a = 1; final protected readonly string $b; public function m(string $x, $y = 2): void { } protected const L = 1; }
--EXPECTF--

Fatal error: B and T define the same constant (L) in the composition of B. However, the definition differs and is considered incompatible. Class was composed in %s on line 5
