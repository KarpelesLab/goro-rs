--TEST--
Int compared with an integer string compares exactly
--FILE--
<?php
// An int compared with an integer string compares exactly (no float rounding).
var_dump(PHP_INT_MAX == "9223372036854775806", PHP_INT_MAX <=> "9223372036854775806", "9223372036854775806" < PHP_INT_MAX, 5 == " 5", 5 == "5 ", 5 == "5x", 5 <=> "abc", "abc" <=> 5, 5 <=> "\xff", "1e1" == 10, 10 <=> "1e1", PHP_INT_MAX == "9.2233720368547758e18", 0 == "", 0 == "0.0", in_array("9223372036854775806", [PHP_INT_MAX]), 100 == "1e2", -0 == "-0");
$a = [PHP_INT_MAX, "9223372036854775806", 3, "10", "9"]; sort($a); var_dump($a);
switch (PHP_INT_MAX) { case "9223372036854775806": echo "wrong\n"; break; case "9223372036854775807": echo "right\n"; }
--EXPECT--
bool(false)
int(1)
bool(true)
bool(true)
bool(true)
bool(false)
int(-1)
int(1)
int(-1)
bool(true)
int(0)
bool(true)
bool(false)
bool(true)
bool(false)
bool(true)
bool(true)
array(5) {
  [0]=>
  int(3)
  [1]=>
  string(1) "9"
  [2]=>
  string(2) "10"
  [3]=>
  string(19) "9223372036854775806"
  [4]=>
  int(9223372036854775807)
}
right
