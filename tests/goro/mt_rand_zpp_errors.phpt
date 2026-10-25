--TEST--
mt_rand()/rand()/lcg_value(): argument count and int parameter errors
--FILE--
<?php
function t($f) { try { $r = $f(); var_dump($r); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; } }
mt_srand(3);
t(fn() => mt_rand("5abc", 10));
t(fn() => mt_rand(1, "10abc"));
t(fn() => mt_rand(1, "x"));
t(fn() => mt_rand([], 1));
t(fn() => mt_rand(null, 5));
t(fn() => mt_rand(1, true));
t(fn() => mt_rand(1, 1e30));
t(fn() => mt_rand(1, NAN));
t(fn() => rand("x", 1));
t(fn() => rand(1, 2, 3));
t(fn() => rand(1, "2.5"));
t(fn() => mt_rand(1, 2, 3, 4));
t(fn() => lcg_value(1, 2));
t(fn() => mt_rand(max: 5, min: 1));
t(fn() => mt_rand(min: 1));
t(fn() => rand(max: 5));
--EXPECTF--
TypeError: mt_rand(): Argument #1 ($min) must be of type int, string given
TypeError: mt_rand(): Argument #2 ($max) must be of type int, string given
TypeError: mt_rand(): Argument #2 ($max) must be of type int, string given
TypeError: mt_rand(): Argument #1 ($min) must be of type int, array given

Deprecated: mt_rand(): Passing null to parameter #1 ($min) of type int is deprecated in %s on line 8
int(4)
int(1)
TypeError: mt_rand(): Argument #2 ($max) must be of type int, float given
TypeError: mt_rand(): Argument #2 ($max) must be of type int, float given
TypeError: rand(): Argument #1 ($min) must be of type int, string given
ArgumentCountError: rand() expects exactly 2 arguments, 3 given

Deprecated: Implicit conversion from float-string "2.5" to int loses precision in %s on line 14
int(2)
ArgumentCountError: mt_rand() expects exactly 2 arguments, 4 given

Deprecated: Function lcg_value() is deprecated since 8.4, use \Random\Randomizer::getFloat() instead in %s on line 16
ArgumentCountError: lcg_value() expects exactly 0 arguments, 2 given
int(3)
ArgumentCountError: mt_rand() expects exactly 2 arguments, 1 given
ArgumentCountError: rand(): Argument #1 ($min) must be passed explicitly, because the default value is not known
