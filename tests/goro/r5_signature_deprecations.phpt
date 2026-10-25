--TEST--
Signature deprecations at compile time, naming the last required parameter
--FILE--
<?php
function a1($a = null, $b) {}
function a2(?int $a = null, $b) {}
function a3(mixed $a = null, $b) {}
function a4(int $a = null, $b) {}
function a5($a = 1, $b, $c = 2, $d, ...$e) {}
function a6(
    $a = 1,
    int $b = null,
    $c
) {}
function &a7(int $x = null): void {}
class K {
    function m(
        int $q = null
    ) {}
    function __construct(public $p = 1, $r) {}
}
$c = function (int $z = null, $y = 1, $w) {};
function nsf(int $a = null) {}
class N { static function s($a = [], $b) {} }
$x = new class { function an(int $a = null) {} };
echo "run\n";
--EXPECTF--

Deprecated: a1(): Optional parameter $a declared before required parameter $b is implicitly treated as a required parameter in %s on line 2

Deprecated: a2(): Optional parameter $a declared before required parameter $b is implicitly treated as a required parameter in %s on line 3

Deprecated: a3(): Optional parameter $a declared before required parameter $b is implicitly treated as a required parameter in %s on line 4

Deprecated: a4(): Implicitly marking parameter $a as nullable is deprecated, the explicit nullable type must be used instead in %s on line 5

Deprecated: a5(): Optional parameter $a declared before required parameter $d is implicitly treated as a required parameter in %s on line 6

Deprecated: a5(): Optional parameter $c declared before required parameter $d is implicitly treated as a required parameter in %s on line 6

Deprecated: a6(): Optional parameter $a declared before required parameter $c is implicitly treated as a required parameter in %s on line 8

Deprecated: a6(): Implicitly marking parameter $b as nullable is deprecated, the explicit nullable type must be used instead in %s on line 9

Deprecated: a7(): Returning by reference from a void function is deprecated in %s on line 12

Deprecated: a7(): Implicitly marking parameter $x as nullable is deprecated, the explicit nullable type must be used instead in %s on line 12

Deprecated: K::m(): Implicitly marking parameter $q as nullable is deprecated, the explicit nullable type must be used instead in %s on line 15

Deprecated: K::__construct(): Optional parameter $p declared before required parameter $r is implicitly treated as a required parameter in %s on line 17

Deprecated: {closure:%s:19}(): Implicitly marking parameter $z as nullable is deprecated, the explicit nullable type must be used instead in %s on line 19

Deprecated: {closure:%s:19}(): Optional parameter $y declared before required parameter $w is implicitly treated as a required parameter in %s on line 19

Deprecated: nsf(): Implicitly marking parameter $a as nullable is deprecated, the explicit nullable type must be used instead in %s on line 20

Deprecated: N::s(): Optional parameter $a declared before required parameter $b is implicitly treated as a required parameter in %s on line 21

Deprecated: class@anonymous(): Implicitly marking parameter $a as nullable is deprecated, the explicit nullable type must be used instead in %s on line 22
run
