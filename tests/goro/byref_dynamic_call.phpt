--TEST--
By-reference arguments of a call through a variable function name or a callable array
--FILE--
<?php
$f = 'preg_match'; $r = $f('/(b)/', 'abc', $m); var_dump($r, $m);
$g = 'sort'; $a = [3,1,2]; $g($a); var_dump($a);
$h = 'str_replace'; $h('a','b','aaa', $cnt); var_dump($cnt);
function byr(&$x) { $x = 5; } $u = 'byr'; $u($z); var_dump($z);
$arr = ['preg_match', 'x']; $fn = $arr[0]; $fn('/x/', 'x', $mm); var_dump($mm);
$c = 'array_push'; $st = [1]; $c($st, 2); var_dump($st);
$d = 'preg_match_all'; $d('/a/', 'aa', $all); var_dump($all);
class K { static function s(&$x) { $x = 1; } function m(&$x) { $x = 2; } }
$sm = 'K::s'; $sm($v1); var_dump($v1);
$sa = ['K', 's']; $sa($v2); var_dump($v2);
$oa = [new K, 'm']; $oa($v3); var_dump($v3);
$ns = '\preg_match'; $ns('/q/', 'q', $v4); var_dump($v4);
$up = 'PREG_MATCH'; $up('/q/', 'q', $v5); var_dump($v5);
$e = 'end'; $arr2 = [1, 2]; var_dump($e($arr2));
--EXPECT--
int(1)
array(2) {
  [0]=>
  string(1) "b"
  [1]=>
  string(1) "b"
}
array(3) {
  [0]=>
  int(1)
  [1]=>
  int(2)
  [2]=>
  int(3)
}
int(3)
int(5)
array(1) {
  [0]=>
  string(1) "x"
}
array(2) {
  [0]=>
  int(1)
  [1]=>
  int(2)
}
array(1) {
  [0]=>
  array(2) {
    [0]=>
    string(1) "a"
    [1]=>
    string(1) "a"
  }
}
int(1)
int(1)
int(2)
array(1) {
  [0]=>
  string(1) "q"
}
array(1) {
  [0]=>
  string(1) "q"
}
int(2)
