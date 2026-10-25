--TEST--
Inheritance check order: abstract trait methods are checked even when excluded by insteadof
--FILE--
<?php
trait T1 { abstract public static function n($x = 1); }
trait T2 { public function n(string $x, $y = 2): mixed { return 1; } public function m(...$a): ?int { return null; } }
interface J { public function m(int $x); }
interface I extends J { public function o(...$a): ?int; }
abstract class A implements J { protected const K = 1; }
abstract class B extends A implements I { use T1, T2 { T2::n insteadof T1; } private $a = 1; public const K = 1; final protected const L = 2; }
class C extends B { public ?int $c = null; }
--EXPECTF--

Fatal error: Cannot make static method T1::n() non static in class B in %s on line 3
