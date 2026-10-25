--TEST--
Inheritance check order: an inherited method cannot be made abstract
--FILE--
<?php
trait T { private static function m($x = 1): void { } }
class A { public static function m(int $x): static { return new static; } public function n($x = 1): void { } private const L = 2; }
abstract class B extends A { private string $a = 's'; protected int $b = 1; final private static function n($x = 1): void { } abstract public static function m(): void; public const L = 1; }
--EXPECTF--

Warning: Private methods cannot be final as they are never overridden by other classes in %s on line 4

Fatal error: Cannot make non abstract method A::m() abstract in class B in %s on line 4
