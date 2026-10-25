--TEST--
Signature deprecations of conditional declarations and closures, before the script runs
--FILE--
<?php
echo "start\n";
if (false) { function f1($a = 1, $b) {} }
function f2(int $x = null) {}
class C { function m($a = 1, $b) {} }
if (false) { class D { function m(int $a = null) {} } }
$f = function ($a = 1, $b) {};
$g = fn (int $q = null) => 1;
function &v(): void {}
echo "end\n";
--EXPECTF--

Deprecated: f1(): Optional parameter $a declared before required parameter $b is implicitly treated as a required parameter in %s on line 3

Deprecated: f2(): Implicitly marking parameter $x as nullable is deprecated, the explicit nullable type must be used instead in %s on line 4

Deprecated: C::m(): Optional parameter $a declared before required parameter $b is implicitly treated as a required parameter in %s on line 5

Deprecated: D::m(): Implicitly marking parameter $a as nullable is deprecated, the explicit nullable type must be used instead in %s on line 6

Deprecated: {closure:%s:7}(): Optional parameter $a declared before required parameter $b is implicitly treated as a required parameter in %s on line 7

Deprecated: {closure:%s:8}(): Implicitly marking parameter $q as nullable is deprecated, the explicit nullable type must be used instead in %s on line 8

Deprecated: v(): Returning by reference from a void function is deprecated in %s on line 9
start
end
