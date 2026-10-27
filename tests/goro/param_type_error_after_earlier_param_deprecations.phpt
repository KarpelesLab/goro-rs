--TEST--
A parameter's compile error comes after the deprecations of the parameters before it
--FILE--
<?php
function f(int $a = null, $b, string $c = null, self $x = null, float $after = null) {}
--EXPECTF--

Deprecated: f(): Implicitly marking parameter $a as nullable is deprecated, the explicit nullable type must be used instead in %s on line 2

Deprecated: f(): Implicitly marking parameter $c as nullable is deprecated, the explicit nullable type must be used instead in %s on line 2

Fatal error: Cannot use "self" when no class scope is active in %s on line 2
