--TEST--
A NAN array offset warns and deprecates
--FILE--
<?php
$x = []; $x[NAN] = 1; var_dump($x);
$n = NAN; $y = [$n => 1]; var_dump($y);
var_dump(isset($x[NAN]), $x[NAN] ?? 'd');
$x[INF] = 2;
$x[1.5] = 3;
$x[-INF] = 4;
unset($x[NAN]);
var_dump($x);
$z = [NAN => 'c'];
var_dump(array_key_exists(NAN, [0 => 1]));
$a = [1, 2]; var_dump($a[NAN]);
--EXPECTF--

Warning: The float NAN is not representable as an int, cast occurred in %s on line 2

Deprecated: Implicit conversion from float NAN to int loses precision in %s on line 2
array(1) {
  [0]=>
  int(1)
}

Warning: The float NAN is not representable as an int, cast occurred in %s on line 3

Deprecated: Implicit conversion from float NAN to int loses precision in %s on line 3
array(1) {
  [0]=>
  int(1)
}

Warning: The float NAN is not representable as an int, cast occurred in %s on line 4

Deprecated: Implicit conversion from float NAN to int loses precision in %s on line 4

Warning: The float NAN is not representable as an int, cast occurred in %s on line 4

Deprecated: Implicit conversion from float NAN to int loses precision in %s on line 4
bool(true)
int(1)

Warning: The float INF is not representable as an int, cast occurred in %s on line 5

Deprecated: Implicit conversion from float 1.5 to int loses precision in %s on line 6

Warning: The float -INF is not representable as an int, cast occurred in %s on line 7

Warning: The float NAN is not representable as an int, cast occurred in %s on line 8

Deprecated: Implicit conversion from float NAN to int loses precision in %s on line 8
array(1) {
  [1]=>
  int(3)
}

Warning: The float NAN is not representable as an int, cast occurred in %s on line 10

Deprecated: Implicit conversion from float NAN to int loses precision in %s on line 10

Warning: The float NAN is not representable as an int, cast occurred in %s on line 11

Deprecated: Implicit conversion from float NAN to int loses precision in %s on line 11
bool(true)

Warning: The float NAN is not representable as an int, cast occurred in %s on line 12

Deprecated: Implicit conversion from float NAN to int loses precision in %s on line 12
int(1)
