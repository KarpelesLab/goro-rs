--TEST--
Constant expressions: integer overflow becomes a float, out of range shifts/modulo/negation do not crash, numeric strings compare loosely
--FILE--
<?php
function t($f) { try { $f(); } catch (\Throwable $e) { echo get_class($e), ": ", $e->getMessage(), " @", $e->getLine(), "\n"; } }
class C {
  const W = PHP_INT_MAX + 1;
  const W2 = PHP_INT_MIN - 1;
  const W3 = PHP_INT_MAX * 2;
  const V = -PHP_INT_MIN;
  const U = 2 ** 64;
  const U2 = 2 ** 62;
  const T = "1e3" == "1000";
  const T2 = "1e3" != "1000";
  const T3 = 100 == "1e2";
  const T4 = "abc" == 0;
  const S1 = 1 << 64;
  const S2 = -8 >> 70;
  const M = PHP_INT_MIN % -1;
  const D = PHP_INT_MIN / -1;
}
var_dump(C::W, C::W2, C::W3, C::V, C::U, C::U2, C::T, C::T2, C::T3, C::T4, C::S1, C::S2, C::M, C::D);
class E1 { const E = 1 << -1; }
t(fn() => var_dump(E1::E));
class E2 { const E = 1 % 0; }
t(fn() => var_dump(E2::E));
--EXPECTF--
float(9.223372036854776E+18)
float(-9.223372036854776E+18)
float(1.8446744073709552E+19)
float(9.223372036854776E+18)
float(1.8446744073709552E+19)
int(4611686018427387904)
bool(true)
bool(false)
bool(true)
bool(false)
int(0)
int(-1)
int(0)
float(9.223372036854776E+18)
ArithmeticError: Bit shift by negative number @20
DivisionByZeroError: Modulo by zero @22
