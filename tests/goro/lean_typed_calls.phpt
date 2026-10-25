--TEST--
Typed parameters and return types of plain functions and methods
--FILE--
<?php
function a(int $x): int { return $x + 1; }
function b(int $x): string { return $x; }
function c(?string $s = null): ?int { return $s === null ? null : strlen($s); }
function d(float $f): float { return $f * 2; }
function e(): void { }
function f2(): ?int { if (false) return 1; }
function g(array $a): array { return $a; }
function h(int ...$n): int { return array_sum($n); }
class P { function m(self $o): static { return $this; } function n(P $p): P { return $p; } }
function k(): never { throw new Exception("never"); }
for ($i = 0; $i < 3; $i++) {
    var_dump(a(1), b(2), c(), c("abc"), d(3), d(2.5), e(), g([1]), h(1, 2));
    try { var_dump(f2()); } catch (TypeError $e) { echo $e->getMessage(), "\n"; }
    try { var_dump(a("5")); } catch (TypeError $e) { echo $e->getMessage(), "\n"; }
    try { var_dump(a("x")); } catch (TypeError $e) { echo $e->getMessage(), "\n"; }
    try { var_dump(a(1.5)); } catch (TypeError $e) { echo $e->getMessage(), "\n"; }
    $p = new P; var_dump($p->m($p) === $p, $p->n($p) === $p);
    try { k(); } catch (Exception $e) { echo $e->getMessage(), "\n"; }
}
function fl(int $x): float { return $x; }
var_dump(fl(3));
--EXPECTF--
int(2)
string(1) "2"
NULL
int(3)
float(6)
float(5)
NULL
array(1) {
  [0]=>
  int(1)
}
int(3)
f2(): Return value must be of type ?int, none returned
int(6)
a(): Argument #1 ($x) must be of type int, string given, called in %s on line 16

Deprecated: Implicit conversion from float 1.5 to int loses precision in %s on line 2
int(2)
bool(true)
bool(true)
never
int(2)
string(1) "2"
NULL
int(3)
float(6)
float(5)
NULL
array(1) {
  [0]=>
  int(1)
}
int(3)
f2(): Return value must be of type ?int, none returned
int(6)
a(): Argument #1 ($x) must be of type int, string given, called in %s on line 16

Deprecated: Implicit conversion from float 1.5 to int loses precision in %s on line 2
int(2)
bool(true)
bool(true)
never
int(2)
string(1) "2"
NULL
int(3)
float(6)
float(5)
NULL
array(1) {
  [0]=>
  int(1)
}
int(3)
f2(): Return value must be of type ?int, none returned
int(6)
a(): Argument #1 ($x) must be of type int, string given, called in %s on line 16

Deprecated: Implicit conversion from float 1.5 to int loses precision in %s on line 2
int(2)
bool(true)
bool(true)
never
float(3)
