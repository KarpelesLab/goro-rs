--TEST--
Integer operators convert a non-object left operand before the right operand's handler
--FILE--
<?php
// An integer operator (% | & ^ << >>) converts a non-object left operand
// to int before the right operand's do_operation handler runs.
function t($f) { try { $r = $f(); echo $r instanceof GMP ? "GMP($r)" : $r, "\n"; } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; } }
$n = new BcMath\Number("7");
$g = gmp_init(7);
$abc = "abc"; $arr = []; $s5 = "5abc"; $f = 2.5; $fs = "1.5"; $o = new stdClass;
foreach ([$n, $g] as $x) {
  t(fn() => $abc % $x);
  t(fn() => $arr % $x);
  t(fn() => $s5 % $x);
  t(fn() => $f % $x);
  t(fn() => $fs >> $x);
  t(fn() => $o % $x);
  t(fn() => $abc + $x);
}
t(fn() => $f | $g);
t(fn() => $f ^ $g);
t(fn() => $f << $g);
t(fn() => $abc & $g);
// BcMath\Number comparisons take a float operand as an int
var_dump($n == 7.5, $n <=> 6.5, 7.5 > $n);
--EXPECTF--
TypeError: Unsupported operand types: string % BcMath\Number
TypeError: Unsupported operand types: array % BcMath\Number

Warning: A non-numeric value encountered in %s on line 11
ValueError: Left string operand cannot be converted to BcMath\Number

Deprecated: Implicit conversion from float 2.5 to int loses precision in %s on line 12

Deprecated: Implicit conversion from float 2.5 to int loses precision in %s on line 12
2

Deprecated: Implicit conversion from float-string "1.5" to int loses precision in %s on line 13
TypeError: Unsupported operand types: string >> BcMath\Number
TypeError: Unsupported operand types: stdClass % BcMath\Number
ValueError: Left string operand cannot be converted to BcMath\Number
TypeError: Unsupported operand types: string % GMP
TypeError: Unsupported operand types: array % GMP

Warning: A non-numeric value encountered in %s on line 11
ValueError: Number is not an integer string

Deprecated: Implicit conversion from float 2.5 to int loses precision in %s on line 12

Deprecated: Implicit conversion from float 2.5 to int loses precision in %s on line 12
GMP(2)

Deprecated: Implicit conversion from float-string "1.5" to int loses precision in %s on line 13
TypeError: Unsupported operand types: string >> GMP
TypeError: Unsupported operand types: stdClass % GMP
ValueError: Number is not an integer string

Deprecated: Implicit conversion from float 2.5 to int loses precision in %s on line 17

Deprecated: Implicit conversion from float 2.5 to int loses precision in %s on line 17
GMP(7)

Deprecated: Implicit conversion from float 2.5 to int loses precision in %s on line 18

Deprecated: Implicit conversion from float 2.5 to int loses precision in %s on line 18
GMP(5)

Deprecated: Implicit conversion from float 2.5 to int loses precision in %s on line 19

Deprecated: Implicit conversion from float 2.5 to int loses precision in %s on line 19
GMP(256)
TypeError: Unsupported operand types: string & GMP

Deprecated: Implicit conversion from float 7.5 to int loses precision in %s on line 22

Deprecated: Implicit conversion from float 6.5 to int loses precision in %s on line 22

Deprecated: Implicit conversion from float 7.5 to int loses precision in %s on line 22
bool(true)
int(1)
bool(false)
