--TEST--
number_format() arguments are parsed like Z_PARAM_NUMBER / LONG / STRING_OR_NULL
--FILE--
<?php
echo number_format(null), "\n";
echo number_format(PHP_INT_MAX), "\n";
echo number_format("12.5"), "\n";
try { number_format(1.5, "x"); } catch (Error $e) { echo $e->getMessage(), "\n"; }
try { number_format(1.5, "1x"); } catch (Error $e) { echo $e->getMessage(), "\n"; }
try { number_format(1.5, 1e19); } catch (Error $e) { echo $e->getMessage(), "\n"; }
try { number_format(1.5, 2, []); } catch (Error $e) { echo $e->getMessage(), "\n"; }
try { number_format(1.5, 2, ".", STDIN); } catch (Error $e) { echo $e->getMessage(), "\n"; }
try { number_format([]); } catch (Error $e) { echo $e->getMessage(), "\n"; }
try { number_format("abc"); } catch (Error $e) { echo $e->getMessage(), "\n"; }
class T { function __toString(): string { return "7"; } }
echo number_format(1234.567,2,'.',new T), "\n";
echo number_format(1234.567,2.0, null, null), "\n";
echo number_format(1234.567,true), "\n";
echo number_format(1234.567,2.5), "\n";
echo number_format(num: 1234.567, thousands_separator: ' '), "\n";
function f() { return number_format(1.5, NAN); }
try { f(); } catch (Error $e) { echo $e, "\n"; }
function g() { return number_format("1x"); }
try { g(); } catch (Error $e) { echo $e, "\n"; }
--EXPECTF--

Deprecated: number_format(): Passing null to parameter #1 ($num) of type float is deprecated in %s on line 2
0
9,223,372,036,854,775,807
13
number_format(): Argument #2 ($decimals) must be of type int, string given
number_format(): Argument #2 ($decimals) must be of type int, string given
number_format(): Argument #2 ($decimals) must be of type int, float given
number_format(): Argument #3 ($decimal_separator) must be of type ?string, array given
number_format(): Argument #4 ($thousands_separator) must be of type ?string, resource given
number_format(): Argument #1 ($num) must be of type int|float, array given
number_format(): Argument #1 ($num) must be of type int|float, string given
17234.57
1,234.57
1,234.6

Deprecated: Implicit conversion from float 2.5 to int loses precision in %s on line 16
1,234.57
1 235
TypeError: number_format(): Argument #2 ($decimals) must be of type int, float given in %s:18
Stack trace:
#0 %s(18): number_format(1.5, NAN)
#1 %s(19): f()
#2 {main}
TypeError: number_format(): Argument #1 ($num) must be of type int|float, string given in %s:20
Stack trace:
#0 %s(20): number_format('1x')
#1 %s(21): g()
#2 {main}
