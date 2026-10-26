--TEST--
NAN comparisons of literals fold (as zend_compare); xor of constant arrays folds
--FILE--
<?php
function t($f) { try { var_dump($f()); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; } }
// (NAN comparisons of literals fold; `a > b` is `b < a`)
var_dump(NAN > 1, 1 > NAN, NAN >= 1, 1 >= NAN, NAN < 1, 1 < NAN, NAN == NAN, NAN != NAN, NAN <=> 1, 1 <=> NAN);
// A folded comparison is a CONST operand: no commutative swap
t(fn() => ["1e3", "0x10"] ^ (1 == NAN));
t(fn() => ["1e3"] ^ (1 > NAN));
// xor of a constant array and a NAN folds while compiling
t(fn() => ([1, 2] xor NAN) ? "y" : "n");
t(fn() => ([] xor 0) ? "y" : "n");
--EXPECTF--

Warning: unexpected NAN value was coerced to bool in %s on line 9
bool(false)
bool(false)
bool(false)
bool(false)
bool(false)
bool(false)
bool(false)
bool(true)
int(1)
int(1)
TypeError: Unsupported operand types: array ^ bool
TypeError: Unsupported operand types: array ^ bool
string(1) "n"
string(1) "n"
