--TEST--
Inheritance check order: compile errors in member order: a property error before a later method's warning
--FILE--
<?php
trait T { final private string $a = 's'; final private static function n(int $x) { } public function m(int $x): void { } }
interface I { public const K = 2; }
abstract class A { final private int $b = 1; final public static string $a = 's'; abstract protected static function m($x = 1); protected function n($x = 1): string { return 's'; } }
class B extends A { use T; protected static $a = 1; private static function n(): int { return 1; } }
--EXPECTF--

Fatal error: Property cannot be both final and private in %s on line 2
