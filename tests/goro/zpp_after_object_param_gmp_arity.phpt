--TEST--
zpp: parameters after an object one are checked; GMP argument counts
--FILE--
<?php
function t($f) { try { $r = $f(); var_dump($r); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; } }
// parameters after an object-typed one are checked too
t(fn() => hash_final(hash_init("md5"), []));
t(fn() => hash_update(hash_init("md5"), []));
t(fn() => hash_copy(hash_init("md5"), 1));
// GMP functions check their argument count
t(fn() => gmp_add(1));
t(fn() => gmp_add(1, 2, 3));
t(fn() => gmp_neg());
t(fn() => gmp_abs(1, 2));
t(fn() => gmp_setbit(gmp_init(1)));
t(fn() => gmp_setbit(gmp_init(1), 1, true, 4));
t(fn() => gmp_clrbit("1"));
t(fn() => gmp_nextprime(1, 2));
t(fn() => hash_update_file(hash_init("md5"), ""));
--EXPECT--
TypeError: hash_final(): Argument #2 ($binary) must be of type bool, array given
TypeError: hash_update(): Argument #2 ($data) must be of type string, array given
ArgumentCountError: hash_copy() expects exactly 1 argument, 2 given
ArgumentCountError: gmp_add() expects exactly 2 arguments, 1 given
ArgumentCountError: gmp_add() expects exactly 2 arguments, 3 given
ArgumentCountError: gmp_neg() expects exactly 1 argument, 0 given
ArgumentCountError: gmp_abs() expects exactly 1 argument, 2 given
ArgumentCountError: gmp_setbit() expects at least 2 arguments, 1 given
ArgumentCountError: gmp_setbit() expects at most 3 arguments, 4 given
ArgumentCountError: gmp_clrbit() expects exactly 2 arguments, 1 given
ArgumentCountError: gmp_nextprime() expects exactly 1 argument, 2 given
ValueError: Path must not be empty
