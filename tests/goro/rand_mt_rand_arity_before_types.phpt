--TEST--
rand() / mt_rand(): the argument count (0 or 2) is checked before the int parameters
--FILE--
<?php
function t($f) { try { $f(); } catch (\Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; } }
foreach (['rand', 'mt_rand'] as $f) {
    t(fn() => $f(null));
    t(fn() => $f(1.5));
    t(fn() => $f([]));
    t(fn() => var_dump($f(1, null)));
    t(fn() => var_dump($f(1, 1.5)));
    t(fn() => var_dump($f(1, "1")));
    t(fn() => var_dump($f(1, " 1")));
    t(fn() => $f(1, "1x"));
    t(fn() => $f(1, 1e30));
    t(fn() => var_dump($f(true, true)));
    t(fn() => $f(1, 2, 3));
}
--EXPECTF--
ArgumentCountError: rand() expects exactly 2 arguments, 1 given
ArgumentCountError: rand() expects exactly 2 arguments, 1 given
ArgumentCountError: rand() expects exactly 2 arguments, 1 given

Deprecated: rand(): Passing null to parameter #2 ($max) of type int is deprecated in %s on line 7
int(1)

Deprecated: Implicit conversion from float 1.5 to int loses precision in %s on line 8
int(1)
int(1)
int(1)
TypeError: rand(): Argument #2 ($max) must be of type int, string given
TypeError: rand(): Argument #2 ($max) must be of type int, float given
int(1)
ArgumentCountError: rand() expects exactly 2 arguments, 3 given
ArgumentCountError: mt_rand() expects exactly 2 arguments, 1 given
ArgumentCountError: mt_rand() expects exactly 2 arguments, 1 given
ArgumentCountError: mt_rand() expects exactly 2 arguments, 1 given

Deprecated: mt_rand(): Passing null to parameter #2 ($max) of type int is deprecated in %s on line 7
ValueError: mt_rand(): Argument #2 ($max) must be greater than or equal to argument #1 ($min)

Deprecated: Implicit conversion from float 1.5 to int loses precision in %s on line 8
int(1)
int(1)
int(1)
TypeError: mt_rand(): Argument #2 ($max) must be of type int, string given
TypeError: mt_rand(): Argument #2 ($max) must be of type int, float given
int(1)
ArgumentCountError: mt_rand() expects exactly 2 arguments, 3 given
