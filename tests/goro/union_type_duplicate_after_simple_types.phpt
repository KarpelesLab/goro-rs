--TEST--
Duplicate union member after signatures with simple and nullable types
--FILE--
<?php
class C { function m(?int $a, Foo $b, ?Foo $c, ?self $d = null): ?static { return null; } }
function ok(int $a, ?string $b, iterable $c, mixed $d, int|string $e, (A&B)|null $f): void {}
echo "ok\n";
function f(Foo|Bar|foo $x) {}
--EXPECTF--

Fatal error: Duplicate type foo is redundant in %s on line 5
