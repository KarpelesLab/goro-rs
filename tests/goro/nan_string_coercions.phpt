--TEST--
NAN coerced to string warns (sort flags, conversions)
--FILE--
<?php
$a = ["b", NAN, "a"]; sort($a, SORT_STRING); var_dump($a);
$a = ["b", NAN, "a"]; rsort($a, SORT_STRING | SORT_FLAG_CASE); var_dump($a);
$a = ["b", NAN, "a"]; sort($a, SORT_NATURAL); echo count($a), "\n";
$a = ["b", NAN, "a"]; sort($a, SORT_LOCALE_STRING); echo count($a), "\n";
var_dump(array_unique([NAN, "x", NAN]));
echo implode(",", [NAN, 1]), "\n";
echo strval(NAN), (string)NAN, "\n";
var_dump(str_contains("NAN", NAN));
var_dump(array_search("NAN", [NAN]));
var_dump(in_array("NAN", [NAN]));
echo strlen(NAN), "\n";
$k = [NAN => 1];
var_dump(array_flip([NAN]));
var_dump(settype($n, "string"));
$x = NAN; settype($x, "string"); var_dump($x);
var_dump(json_encode([NAN]));
printf("%s\n", NAN);
echo sprintf("%.2s", NAN), "\n";
echo str_pad(NAN, 5, "-"), "\n";
var_dump(NAN == "NAN");
var_dump(array_sum(["NAN"]));
--EXPECTF--

Warning: unexpected NAN value was coerced to string in %s on line 2

Warning: unexpected NAN value was coerced to string in %s on line 2
array(3) {
  [0]=>
  float(NAN)
  [1]=>
  string(1) "a"
  [2]=>
  string(1) "b"
}

Warning: unexpected NAN value was coerced to string in %s on line 3

Warning: unexpected NAN value was coerced to string in %s on line 3
array(3) {
  [0]=>
  float(NAN)
  [1]=>
  string(1) "b"
  [2]=>
  string(1) "a"
}

Warning: unexpected NAN value was coerced to string in %s on line 4

Warning: unexpected NAN value was coerced to string in %s on line 4
3

Warning: unexpected NAN value was coerced to string in %s on line 5

Warning: unexpected NAN value was coerced to string in %s on line 5
3

Warning: unexpected NAN value was coerced to string in %s on line 6

Warning: unexpected NAN value was coerced to string in %s on line 6
array(2) {
  [0]=>
  float(NAN)
  [1]=>
  string(1) "x"
}

Warning: unexpected NAN value was coerced to string in %s on line 7
NAN,1

Warning: unexpected NAN value was coerced to string in %s on line 8
NAN
Warning: unexpected NAN value was coerced to string in %s on line 8
NAN

Warning: unexpected NAN value was coerced to string in %s on line 9
bool(true)
bool(false)
bool(false)

Warning: unexpected NAN value was coerced to string in %s on line 12
3

Warning: The float NAN is not representable as an int, cast occurred in %s on line 13

Deprecated: Implicit conversion from float NAN to int loses precision in %s on line 13

Warning: array_flip(): Can only flip string and integer values, entry skipped in %s on line 14
array(0) {
}
bool(true)

Warning: unexpected NAN value was coerced to string in %s on line 16
string(3) "NAN"
bool(false)

Warning: unexpected NAN value was coerced to string in %s on line 18
NAN

Warning: unexpected NAN value was coerced to string in %s on line 19
NA

Warning: unexpected NAN value was coerced to string in %s on line 20
NAN--
bool(false)

Warning: array_sum(): Addition is not supported on type string in %s on line 22
int(0)
