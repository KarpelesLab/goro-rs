--TEST--
A class constant / property default reading a missing array key or string offset warns (not folded)
--FILE--
<?php
const G = [1, 2][5];
var_dump(G);
class Z {
 const C = [1, 2][7];
 const D = "abc"[9];
 const E = ["a"=>1]["b"];
 static $s = [1][3];
 public $p = [1][4];
}
var_dump(Z::C);
var_dump(Z::D);
var_dump(Z::E);
var_dump(Z::$s);
var_dump((new Z)->p);
function f($x = [1][8]) { var_dump($x); }
f();
const H = null[1];
var_dump(H);
--EXPECTF--

Warning: Undefined array key 5 in %s on line 2
NULL

Warning: Undefined array key 7 in %s on line 5
NULL

Warning: Uninitialized string offset 9 in %s on line 6
string(0) ""

Warning: Undefined array key "b" in %s on line 7
NULL

Warning: Undefined array key 4 in %s on line 9

Warning: Undefined array key 3 in %s on line 8
NULL
NULL

Warning: Undefined array key 8 in %s on line 16
NULL

Warning: Trying to access array offset on null in %s on line 18
NULL
