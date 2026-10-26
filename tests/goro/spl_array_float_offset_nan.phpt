--TEST--
ArrayObject/ArrayIterator/SplFixedArray offsets: NAN and INF floats warn as for plain arrays
--FILE--
<?php
$a = [3, 1];
var_dump($a[NAN] ?? 'u');
var_dump(isset($a[NAN]));
$o = new ArrayObject([3, 1]);
var_dump($o[NAN]);
var_dump(isset($o[NAN]));
var_dump($o->offsetExists(NAN));
var_dump($o->offsetGet(1.5));
var_dump($o->offsetExists(INF));
$o->offsetSet(NAN, 5);
$o[NAN] = 6;
unset($o[NAN]);
var_dump($o->getArrayCopy());
$f = new SplFixedArray(2);
var_dump($f[NAN]);
var_dump($f->offsetGet(NAN));
--EXPECTF--

Warning: The float NAN is not representable as an int, cast occurred in %s on line 3

Deprecated: Implicit conversion from float NAN to int loses precision in %s on line 3
int(3)

Warning: The float NAN is not representable as an int, cast occurred in %s on line 4

Deprecated: Implicit conversion from float NAN to int loses precision in %s on line 4
bool(true)

Warning: The float NAN is not representable as an int, cast occurred in %s on line 6

Deprecated: Implicit conversion from float NAN to int loses precision in %s on line 6
int(3)

Warning: The float NAN is not representable as an int, cast occurred in %s on line 7

Deprecated: Implicit conversion from float NAN to int loses precision in %s on line 7
bool(true)

Warning: The float NAN is not representable as an int, cast occurred in %s on line 8

Deprecated: Implicit conversion from float NAN to int loses precision in %s on line 8
bool(true)

Deprecated: Implicit conversion from float 1.5 to int loses precision in %s on line 9
int(1)

Warning: The float INF is not representable as an int, cast occurred in %s on line 10
bool(true)

Warning: The float NAN is not representable as an int, cast occurred in %s on line 11

Deprecated: Implicit conversion from float NAN to int loses precision in %s on line 11

Warning: The float NAN is not representable as an int, cast occurred in %s on line 12

Deprecated: Implicit conversion from float NAN to int loses precision in %s on line 12

Warning: The float NAN is not representable as an int, cast occurred in %s on line 13

Deprecated: Implicit conversion from float NAN to int loses precision in %s on line 13
array(1) {
  [1]=>
  int(1)
}

Warning: The float NAN is not representable as an int, cast occurred in %s on line 16

Deprecated: Implicit conversion from float NAN to int loses precision in %s on line 16
NULL

Warning: The float NAN is not representable as an int, cast occurred in %s on line 17

Deprecated: Implicit conversion from float NAN to int loses precision in %s on line 17
NULL
