--TEST--
array_fill_keys/array_column/array_keys/array_splice key and value conversions
--FILE--
<?php
// array_fill_keys(): non-int keys go through zval_get_string()
try { var_dump(array_fill_keys([new stdClass], 0)); } catch (Error $e) { echo $e->getMessage(), "\n"; }
var_dump(array_fill_keys([1.5, NAN, "05", true, null], 0));
// array_column(): string keys are looked up as symtable keys ("-0", "01" stay strings)
var_dump(array_column([[1], [2, 3]], 0, "-0"));
var_dump(array_column([["-0" => 'a', "01" => 'b', "1" => 'c']], "01", "-0"));
// array_column(): the loop goes on after an invalid index key
try { var_dump(array_column([['v' => 'c', 'id' => [1]], ['id' => 1.5, 'v' => 'd']], 'v', 'id')); } catch (Error $e) { echo $e->getMessage(), "\n"; }
// array_keys() with a search value compares like ==
$o = new stdClass;
var_dump(array_keys([-1 => 1, 5 => 2], $o));
class S { function __toString(): string { return "x"; } }
var_dump(array_keys(["x", "y", "x"], new S));
// array_splice(): a NAN replacement is converted to an array
$a = [1, 2, 3];
var_dump(array_splice($a, 1, 1, NAN), $a);
--EXPECTF--
Object of class stdClass could not be converted to string

Warning: unexpected NAN value was coerced to string in %s on line 4
array(5) {
  ["1.5"]=>
  int(0)
  ["NAN"]=>
  int(0)
  ["05"]=>
  int(0)
  [1]=>
  int(0)
  [""]=>
  int(0)
}
array(2) {
  [0]=>
  int(1)
  [1]=>
  int(2)
}
array(1) {
  ["a"]=>
  string(1) "b"
}

Deprecated: Implicit conversion from float 1.5 to int loses precision in %s on line 9
Cannot access offset of type array on array

Notice: Object of class stdClass could not be converted to int in %s on line 12

Notice: Object of class stdClass could not be converted to int in %s on line 12
array(1) {
  [0]=>
  int(-1)
}
array(2) {
  [0]=>
  int(0)
  [1]=>
  int(2)
}

Warning: unexpected NAN value was coerced to array in %s on line 17
array(1) {
  [0]=>
  int(2)
}
array(3) {
  [0]=>
  int(1)
  [1]=>
  float(NAN)
  [2]=>
  int(3)
}
