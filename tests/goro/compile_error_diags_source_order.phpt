--TEST--
Diagnostics of code before a hoisted declaration's compile error come first (closure implicit nullable deprecations)
--FILE--
<?php
function f(int $x = null) {}
function g(?int $x = null, string $y = null, $z = null, int|string $w = null) {}
class C { function m(array $a = null) {} function __construct(public ?Foo $p = null, Bar $q = null) {} }
$c = function (int $a = null) {};
$d = fn (float $a = null) => 1;
function h(int $x = null, $y) {}
function k(mixed $m = null, null|int $n = null) {}
function j(self $s = null) {}
echo "done\n";
?>
--EXPECTF--

Deprecated: f(): Implicitly marking parameter $x as nullable is deprecated, the explicit nullable type must be used instead in %s on line 2

Deprecated: g(): Implicitly marking parameter $y as nullable is deprecated, the explicit nullable type must be used instead in %s on line 3

Deprecated: g(): Implicitly marking parameter $w as nullable is deprecated, the explicit nullable type must be used instead in %s on line 3

Deprecated: C::m(): Implicitly marking parameter $a as nullable is deprecated, the explicit nullable type must be used instead in %s on line 4

Deprecated: C::__construct(): Implicitly marking parameter $q as nullable is deprecated, the explicit nullable type must be used instead in %s on line 4

Deprecated: {closure:%s:5}(): Implicitly marking parameter $a as nullable is deprecated, the explicit nullable type must be used instead in %s on line 5

Deprecated: {closure:%s:6}(): Implicitly marking parameter $a as nullable is deprecated, the explicit nullable type must be used instead in %s on line 6

Deprecated: h(): Implicitly marking parameter $x as nullable is deprecated, the explicit nullable type must be used instead in %s on line 7

Fatal error: Cannot use "self" when no class scope is active in %s on line 9
