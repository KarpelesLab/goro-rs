--TEST--
GMP: weak scalar arguments, int parameters, non-exact gmp_divexact()
--FILE--
<?php
// GMP function arguments: other scalars are accepted weakly (zend_parse_arg_long_weak)
function t($f) { try { $r = $f(); var_dump($r instanceof GMP ? "GMP:$r" : $r); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; } }
t(fn() => gmp_add(1.5, 1));
t(fn() => gmp_add(2.0, 1));
t(fn() => gmp_add(1e30, 1));
t(fn() => gmp_add(NAN, 1));
t(fn() => gmp_add(true, 1));
t(fn() => gmp_add(false, 1));
t(fn() => gmp_add(null, 1));
t(fn() => gmp_cmp(5, 4.5));
t(fn() => gmp_strval(2.0));
t(fn() => gmp_pow(2, 3.5));
t(fn() => gmp_pow(2, "3"));
t(fn() => gmp_pow(2, "3.5"));
t(fn() => gmp_pow(2, 1e30));
t(fn() => gmp_pow(2, "x"));
t(fn() => gmp_setbit(gmp_init(1), 2.5));
t(fn() => gmp_setbit("1", []));
t(fn() => gmp_setbit(gmp_init(1), PHP_INT_MAX));
t(fn() => gmp_testbit(gmp_init(1), -1));
t(fn() => gmp_strval(255, 16.0));
t(fn() => gmp_div_q(7, 2, 1.0));
// mpz_divexact() on a non-divisible pair: GMP's 2-adic quotient
foreach ([[1, 3], [7, -3], [-256, 3], [255, 7], [1, "12345678901"], ["-98765432109876543210123", 7],
          ["12345678901234567890", "12345678901"], [10, 4], [7, "12345678901234567890"], [12, 4], [0, 5]] as [$x, $y]) {
  echo gmp_divexact($x, $y), "\n";
}
--EXPECTF--

Deprecated: Implicit conversion from float 1.5 to int loses precision in %s on line 4
string(5) "GMP:2"
string(5) "GMP:3"
TypeError: gmp_add(): Argument #1 ($num1) must be of type GMP|string|int, float given
TypeError: gmp_add(): Argument #1 ($num1) must be of type GMP|string|int, float given
string(5) "GMP:2"
string(5) "GMP:1"

Deprecated: gmp_add(): Passing null to parameter #1 ($num1) of type GMP|string|int is deprecated in %s on line 10
string(5) "GMP:1"

Deprecated: Implicit conversion from float 4.5 to int loses precision in %s on line 11
int(1)
string(1) "2"

Deprecated: Implicit conversion from float 3.5 to int loses precision in %s on line 13
string(5) "GMP:8"
string(5) "GMP:8"

Deprecated: Implicit conversion from float-string "3.5" to int loses precision in %s on line 15
string(5) "GMP:8"
TypeError: gmp_pow(): Argument #2 ($exponent) must be of type int, float given
TypeError: gmp_pow(): Argument #2 ($exponent) must be of type int, string given

Deprecated: Implicit conversion from float 2.5 to int loses precision in %s on line 18
NULL
TypeError: gmp_setbit(): Argument #1 ($num) must be of type GMP, string given
ValueError: gmp_setbit(): Argument #2 ($index) must be between 0 and 2147483647 * 64
ValueError: gmp_testbit(): Argument #2 ($index) must be between 0 and 2147483647 * 64
string(2) "ff"
string(5) "GMP:4"
12297829382473034411
-12297829382473034413
-12297829382473034496
5270498306774157641
104386087570069021
-243058833514956059440329306719340609629
6519034618300909002
2
10980206599865110667
3
0
