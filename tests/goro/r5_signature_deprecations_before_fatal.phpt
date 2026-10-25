--TEST--
Compile-time deprecations are shown before the compile error ending the file
--FILE--
<?php
function f(int $a = null, $z = 1, $y) {}
class C { function m(int $q = null) {} }
function g(int $b = "x") {}
--EXPECTF--

Deprecated: f(): Implicitly marking parameter $a as nullable is deprecated, the explicit nullable type must be used instead in %s on line 2

Deprecated: f(): Optional parameter $z declared before required parameter $y is implicitly treated as a required parameter in %s on line 2

Deprecated: C::m(): Implicitly marking parameter $q as nullable is deprecated, the explicit nullable type must be used instead in %s on line 3

Fatal error: Cannot use string as default value for parameter $b of type int in %s on line 4
