--TEST--
An array variable sent by value is copied when code runs before the call
--FILE--
<?php
$c = [1]; function f2($a, $b) { return count($a); } echo f2($c, $c[] = 2), "\n";
$a = [3, 1, 2]; var_dump($a, sort($a));
function g($x, $y) { var_dump($x, $y); }
$d = ['k' => 1]; g($d, $d['k'] = 5);
$e = [1]; g($e, array_push($e, 9));
function h() { $v = [1]; return f2($v, $v[] = 3); } echo h(), "\n";
function m($x, $y) { return $x; }
$s = "ab"; var_dump(m($s, $s .= "c"));
$o = [[1]]; var_dump(m($o[0], $o[0][] = 2));
$p = new stdClass; $p->arr = [1]; var_dump(m($p->arr, $p->arr[] = 2));
function run() { $c = [1]; echo f2($c, $c[] = 2), "\n"; $a = [3, 1, 2]; var_dump($a, sort($a)); var_dump(strlen("x"), $a); }
run();
--EXPECTF--
1
array(3) {
  [0]=>
  int(3)
  [1]=>
  int(1)
  [2]=>
  int(2)
}
bool(true)
array(1) {
  ["k"]=>
  int(1)
}
int(5)
array(1) {
  [0]=>
  int(1)
}
int(2)
1
string(2) "ab"
array(1) {
  [0]=>
  int(1)
}
array(1) {
  [0]=>
  int(1)
}
1
array(3) {
  [0]=>
  int(3)
  [1]=>
  int(1)
  [2]=>
  int(2)
}
bool(true)
int(1)
array(3) {
  [0]=>
  int(1)
  [1]=>
  int(2)
  [2]=>
  int(3)
}
