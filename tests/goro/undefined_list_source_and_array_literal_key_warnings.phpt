--TEST--
list() of an undefined variable warns; an array literal reads its value before its key, an undefined key variable being the empty string
--FILE--
<?php
list($l1, $l2) = $lsrc;
[$a, $b] = $src2;
[$q] = null;
function f() { [$a, $b] = $undefined; ['k' => $c] = $alsoundef; var_dump($a, $c); }
f();
$n = null;
$c = [$c1 => $c2, $c3, $n => 'null key'];
var_dump($c);
$d = [$k => f2()];
function f2() { echo "f2\n"; return 1; }
var_dump($d);
--EXPECTF--

Warning: Undefined variable $lsrc in %s on line 2

Warning: Undefined variable $src2 in %s on line 3

Warning: Undefined variable $undefined in %s on line 5

Warning: Undefined variable $alsoundef in %s on line 5
NULL
NULL

Warning: Undefined variable $c2 in %s on line 8

Warning: Undefined variable $c1 in %s on line 8

Warning: Undefined variable $c3 in %s on line 8

Deprecated: Using null as an array offset is deprecated, use an empty string instead in %s on line 8
array(2) {
  [""]=>
  string(8) "null key"
  [0]=>
  NULL
}
f2

Warning: Undefined variable $k in %s on line 10
array(1) {
  [""]=>
  int(1)
}
