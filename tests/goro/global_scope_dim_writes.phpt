--TEST--
Global scope: array appends and writes of bound variables
--FILE--
<?php
// Appends and element writes in the global scope (variables bound to the
// symbol table): the value stored is a copy of the variable's.
$a = [];
for ($i = 0; $i < 3; $i++) { $a[] = $i; $a["k$i"] = $i; }
$x = 1; $r = &$x;
$a[] = $x; $a[] = $r; $a['x'] = $r;
$x = 2;
var_dump($a);
$s = 'str'; $f = 1.5; $n = null; $t = true;
$b = [];
$b[] = $s; $b[] = $f; $b[] = $n; $b[] = $t;
$s .= '!';
var_dump($b);
$c = [];
$c[] = $undef;
var_dump($c);
$d = [PHP_INT_MAX => 1];
try { $d[] = $x; } catch (Error $e) { echo $e->getMessage(), "\n"; }
function g() { global $a, $x; $a[] = $x; return count($a); }
var_dump(g(), end($a));
$GLOBALS['a'][] = $x;
var_dump(count($a), $GLOBALS['a'][count($a) - 1]);
$o = new ArrayObject([]);
$o[] = $x;
var_dump(count($o));
--EXPECTF--
array(9) {
  [0]=>
  int(0)
  ["k0"]=>
  int(0)
  [1]=>
  int(1)
  ["k1"]=>
  int(1)
  [2]=>
  int(2)
  ["k2"]=>
  int(2)
  [3]=>
  int(1)
  [4]=>
  int(1)
  ["x"]=>
  int(1)
}
array(4) {
  [0]=>
  string(3) "str"
  [1]=>
  float(1.5)
  [2]=>
  NULL
  [3]=>
  bool(true)
}

Warning: Undefined variable $undef in %s on line 16
array(1) {
  [0]=>
  NULL
}
Cannot add element to the array as the next element is already occupied
int(10)
int(2)

Warning: Undefined array key 10 in %s on line 23
int(11)
NULL
int(1)
