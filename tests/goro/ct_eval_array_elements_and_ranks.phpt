--TEST--
zend_eval_const_expr() pass over array literal elements and sprintf formats; commutative operand ranks
--FILE--
<?php
$a = 5; $b = "7"; $s = "str"; $f = 1.5; $n = null; $arr = [1, "x" => 2];
class OO { public $p = 1; function __toString(): string { return "OO"; } }
$o = new OO;
function t($f) { try { var_dump($f()); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; } }
// zend_try_ct_eval_array(): array elements go through zend_eval_const_expr()
t(fn() => ["abc"["0x10"], "abc"["1x"]]);
t(fn() => [1 + "abc"["0x10"]]);
t(fn() => [NAN ? 1 : 2]);
t(fn() => [NAN || $a]);
t(fn() => [NAN && $a]);
t(fn() => [(int)(NAN || $a)]);
t(fn() => [!NAN]);
t(fn() => [(NAN || [1, 2]) <= 1]);
t(fn() => [NAN ?? 5, null ?? "d"]);
t(fn() => sprintf((NAN ? "%d" : "%s"), 5));
// operand ranks of commutative operators follow zend_compile's folding
t(fn() => [" 4", ((null & true) || ([1, 2] ? "" : [1, 2]))] & ((![1] ? (array)$a : "3"[$f]) ?: ($a != 1)));
t(fn() => [[[], "1e3"], (NAN === "1e3")] ^ (($n ? true : []) === [$o, PHP_INT_MAX]));
t(fn() => [("3" * true), !""] ^ [null, " 4"][~""]);
t(fn() => (@2.5 ^ [false, $o]));
t(fn() => [[[1, 2], INF], ("0x10" > true)] * (($f ** true) ? 2.5 : 1));
t(fn() => (("" ? INF : " 4")[+PHP_INT_MAX] & (($n << true) < [PHP_INT_MAX, 1])));
t(fn() => true / (true[PHP_INT_MAX] * [NAN, $b]));
// a constant array operand of `.` is cast before the concatenation
t(fn() => new stdClass . []);
t(fn() => [] . new stdClass);
--EXPECTF--

Warning: unexpected NAN value was coerced to bool in %s on line 9

Warning: unexpected NAN value was coerced to bool in %s on line 10

Warning: unexpected NAN value was coerced to bool in %s on line 11

Warning: unexpected NAN value was coerced to bool in %s on line 11

Warning: unexpected NAN value was coerced to bool in %s on line 12

Warning: unexpected NAN value was coerced to bool in %s on line 14

Warning: unexpected NAN value was coerced to bool in %s on line 16
array(2) {
  [0]=>
  string(1) "a"
  [1]=>
  string(1) "b"
}
TypeError: Unsupported operand types: int + string
array(1) {
  [0]=>
  int(1)
}
array(1) {
  [0]=>
  bool(true)
}
array(1) {
  [0]=>
  bool(true)
}
array(1) {
  [0]=>
  int(1)
}

Warning: unexpected NAN value was coerced to bool in %s on line 13
array(1) {
  [0]=>
  bool(false)
}
array(1) {
  [0]=>
  bool(true)
}
array(2) {
  [0]=>
  float(NAN)
  [1]=>
  string(1) "d"
}
string(1) "5"

Warning: String offset cast occurred in %s on line 18

Warning: Uninitialized string offset 1 in %s on line 18
TypeError: Unsupported operand types: bool & array
TypeError: Unsupported operand types: bool ^ array

Warning: Undefined array key "" in %s on line 20
TypeError: Unsupported operand types: null ^ array
TypeError: Unsupported operand types: array ^ float
TypeError: Unsupported operand types: float * array

Warning: Uninitialized string offset 9223372036854775807 in %s on line 23
TypeError: Unsupported operand types: string & bool

Warning: Trying to access array offset on true in %s on line 24
TypeError: Unsupported operand types: null * array

Warning: Array to string conversion in %s on line 26
Error: Object of class stdClass could not be converted to string

Warning: Array to string conversion in %s on line 27
Error: Object of class stdClass could not be converted to string
