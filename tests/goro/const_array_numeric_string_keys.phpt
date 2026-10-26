--TEST--
Numeric string keys in constant array expressions (class constants, property defaults) are integer keys; spread renumbers integer keys
--FILE--
<?php
class O {
    public $a = ['defined' => 'd', '1' => 1, '09' => '09'];
    public $b = ['1' => 1];
    const C = ['x' => 1, '2' => 2, '-3' => 3, '+4' => 4, '-0' => 5, '0' => 6];
    const D = [...[5 => 'a', 'k' => 'b'], ...['7' => 'c', 'k' => 'd']];
    public static $s = ['x' => 1, '5' => 2];
}
$o = new O;
var_dump($o->a, array_key_exists(1, $o->a), $o->b, array_key_exists(1, $o->b), O::C, O::D, O::$s);
var_dump(array_key_exists(1, ['defined' => 'd', '1' => 1]));
function g($p = ['k' => 1, '7' => 2]) { return $p; }
var_dump(g());
const E = ['3' => 1, '9223372036854775808' => 2];
var_dump(E);
class P {
    const K = '5';
    const A = [self::K => 'k', '6' => 'six', ...O::D];
    public $p = [self::K => 'k', '8' => 'e'];
    public static $q = [...O::C, '10' => 'ten'];
}
var_dump(P::A, (new P)->p, P::$q, O::C['2'], E['3']);

?>
--EXPECT--
array(3) {
  ["defined"]=>
  string(1) "d"
  [1]=>
  int(1)
  ["09"]=>
  string(2) "09"
}
bool(true)
array(1) {
  [1]=>
  int(1)
}
bool(true)
array(6) {
  ["x"]=>
  int(1)
  [2]=>
  int(2)
  [-3]=>
  int(3)
  ["+4"]=>
  int(4)
  ["-0"]=>
  int(5)
  [0]=>
  int(6)
}
array(3) {
  [0]=>
  string(1) "a"
  ["k"]=>
  string(1) "d"
  [1]=>
  string(1) "c"
}
array(2) {
  ["x"]=>
  int(1)
  [5]=>
  int(2)
}
bool(true)
array(2) {
  ["k"]=>
  int(1)
  [7]=>
  int(2)
}
array(2) {
  [3]=>
  int(1)
  ["9223372036854775808"]=>
  int(2)
}
array(5) {
  [5]=>
  string(1) "k"
  [6]=>
  string(3) "six"
  [7]=>
  string(1) "a"
  ["k"]=>
  string(1) "d"
  [8]=>
  string(1) "c"
}
array(2) {
  [5]=>
  string(1) "k"
  [8]=>
  string(1) "e"
}
array(7) {
  ["x"]=>
  int(1)
  [0]=>
  int(2)
  [1]=>
  int(3)
  ["+4"]=>
  int(4)
  ["-0"]=>
  int(5)
  [2]=>
  int(6)
  [10]=>
  string(3) "ten"
}
int(2)
int(1)
