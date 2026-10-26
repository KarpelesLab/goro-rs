--TEST--
NAN callback results tested by array_filter()/array_any()/array_find() and usort() comparison results warn
--FILE--
<?php
// zend_is_true() of callback results and array elements
var_dump(array_filter([NAN, 0.0, 1.0]));
var_dump(array_filter([1], fn($x) => NAN));
var_dump(array_filter([1], fn($k) => NAN, ARRAY_FILTER_USE_KEY));
var_dump(array_any([1], fn() => NAN), array_all([1], fn() => NAN));
var_dump(array_find([1], fn() => NAN), array_find_key([1], fn() => NAN));
// php_get_long() of a comparison result
foreach ([1.5, 1e30, INF, NAN] as $r) {
    $a = [3, 1];
    usort($a, fn($x, $y) => $r);
    echo json_encode($a), "\n";
}
--EXPECTF--

Warning: unexpected NAN value was coerced to bool in %s on line 3
array(2) {
  [0]=>
  float(NAN)
  [2]=>
  float(1)
}

Warning: unexpected NAN value was coerced to bool in %s on line 4
array(1) {
  [0]=>
  int(1)
}

Warning: unexpected NAN value was coerced to bool in %s on line 5
array(1) {
  [0]=>
  int(1)
}

Warning: unexpected NAN value was coerced to bool in %s on line 6

Warning: unexpected NAN value was coerced to bool in %s on line 6
bool(true)
bool(true)

Warning: unexpected NAN value was coerced to bool in %s on line 7

Warning: unexpected NAN value was coerced to bool in %s on line 7
int(1)
int(0)
[1,3]

Warning: The float 1.0E+30 is not representable as an int, cast occurred in %s on line 11
[1,3]

Warning: The float INF is not representable as an int, cast occurred in %s on line 11
[3,1]

Warning: The float NAN is not representable as an int, cast occurred in %s on line 11
[3,1]
