--TEST--
pow() non-numeric warnings, round() mode type name, intval() C-int base, range() keeps -0.0
--FILE--
<?php
set_error_handler(function ($no, $msg) { echo "[$msg]\n"; return true; });
var_dump(pow("1x", 3), pow(2, "1x"), pow("1x", "2y"), pow(" 2 ", 2));
try { pow("1x", []); } catch (Error $e) { echo $e->getMessage(), "\n"; }
try { pow([], "1x"); } catch (Error $e) { echo $e->getMessage(), "\n"; }
try { pow("abc", 2); } catch (Error $e) { echo $e->getMessage(), "\n"; }
try { var_dump(round(1.55, 1, "abc")); } catch (Error $e) { echo $e->getMessage(), "\n"; }
try { var_dump(round(1.55, 1, null)); } catch (Error $e) { echo $e->getMessage(), "\n"; }
try { var_dump(round(1.55, 1, [])); } catch (Error $e) { echo $e->getMessage(), "\n"; }
try { var_dump(round(1.55, 1, 1e30)); } catch (Error $e) { echo $e->getMessage(), "\n"; }
try { var_dump(round(1.55, 1, "3")); } catch (Error $e) { echo $e->getMessage(), "\n"; }
try { var_dump(round(1.55, "x")); } catch (Error $e) { echo $e->getMessage(), "\n"; }
var_dump(intval("12", PHP_INT_MIN), intval("12", 4294967306), intval("z", 4294967332), intval("0x1A", 4294967296), intval("12", 37), intval("0b11", 2), intval("12", 10));
var_dump(range(-0.0, 2));
var_dump(range(-0.0, 2, 0.5)[0]);
var_dump(range(2, -0.0)[2]);
var_dump(range(-0.0, -0.0));
var_dump(range(-0.0, -1.0));
--EXPECT--
[A non-numeric value encountered]
[A non-numeric value encountered]
[A non-numeric value encountered]
[A non-numeric value encountered]
int(1)
int(2)
int(1)
int(4)
[A non-numeric value encountered]
Unsupported operand types: string ** array
Unsupported operand types: array ** string
Unsupported operand types: string ** int
round(): Argument #3 ($mode) must be of type RoundingMode|int, string given
[round(): Passing null to parameter #3 ($mode) of type RoundingMode|int is deprecated]
round(): Argument #3 ($mode) must be a valid rounding mode (RoundingMode::*)
round(): Argument #3 ($mode) must be of type RoundingMode|int, array given
round(): Argument #3 ($mode) must be of type RoundingMode|int, float given
float(1.6)
round(): Argument #2 ($precision) must be of type int, string given
int(12)
int(12)
int(35)
int(26)
int(0)
int(3)
int(12)
array(3) {
  [0]=>
  float(-0)
  [1]=>
  float(1)
  [2]=>
  float(2)
}
float(-0)
float(0)
array(1) {
  [0]=>
  float(-0)
}
array(2) {
  [0]=>
  float(-0)
  [1]=>
  float(-1)
}
