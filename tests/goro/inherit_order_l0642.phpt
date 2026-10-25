--TEST--
Inheritance check order: trait constants are checked against the parent's constants
--FILE--
<?php
trait T { private static function m(): string { return 's'; } private const K = 1; }
class A { public $b = 1; private function m($x = 1): static { return $this; } public static function n(string $x, $y = 2): int { return 1; } protected const K = 1; public const L = 1; }
class B extends A { use T; protected int $a = 1; public function m(string $x, $y = 2): void { } }
--EXPECTF--

Fatal error: Access level to B::K must be protected (as in class A) or weaker in %s on line 4
