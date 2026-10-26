--TEST--
var_dump() of a method closure marks its by-reference parameters; debug_zval_dump() of a closure's debug arrays
--FILE--
<?php
class K { function m($a, &$b, ...$c) {} static function s(&$x, $y = 1) {} }
var_dump(Closure::fromCallable([new K, 'm']), K::s(...));
$x = 5;
$cl = function (int $a, &$b = 2) use ($x) { static $st = [1]; };
debug_zval_dump($cl);
--EXPECTF--
object(Closure)#2 (3) {
  ["function"]=>
  string(4) "K::m"
  ["this"]=>
  object(K)#1 (0) {
  }
  ["parameter"]=>
  array(3) {
    ["$a"]=>
    string(10) "<required>"
    ["&$b"]=>
    string(10) "<required>"
    ["$c"]=>
    string(10) "<optional>"
  }
}
object(Closure)#3 (2) {
  ["function"]=>
  string(4) "K::s"
  ["parameter"]=>
  array(2) {
    ["&$x"]=>
    string(10) "<required>"
    ["$y"]=>
    string(10) "<optional>"
  }
}
object(Closure)#3 (5) refcount(2){
  ["name"]=>
  string(%d) "{closure:%s:5}" refcount(%d)
  ["file"]=>
  string(%d) "%s" refcount(%d)
  ["line"]=>
  int(5)
  ["static"]=>
  array(2) refcount(1){
    ["x"]=>
    int(5)
    ["st"]=>
    array(1) packed refcount(3){
      [0]=>
      int(1)
    }
  }
  ["parameter"]=>
  array(2) refcount(1){
    ["$a"]=>
    string(10) "<required>" refcount(1)
    ["&$b"]=>
    string(10) "<optional>" refcount(1)
  }
}
