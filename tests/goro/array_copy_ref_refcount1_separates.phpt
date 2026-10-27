--TEST--
A copy of an array holding a reference only the array holds once the copy is made writes its own value, not through the reference
--FILE--
<?php
$s = ['a' => 1]; $t = &$s['a']; $u = $s; unset($t); $u['a'] = 2; var_dump($s);
$s = [1, 2]; $t = &$s[0]; $u = $s; unset($t); $u[0] = 'x'; var_dump($s, $u);
$g = ['x' => ['y' => 1]]; $ref = &$g['x']['y']; $h = $g; unset($ref); $h['x']['y'] = 'H'; var_dump($g['x']['y']);
function f($arr) { $arr['a'] = 'f'; return $arr; }
$s = ['a' => 1]; $t = &$s['a']; unset($t); $r = f($s); var_dump($s, $r);
$s = ['a' => 1]; $t = &$s['a']; $u = $s; unset($t); foreach ($u as &$v) { $v = 'fe'; } unset($v); var_dump($s);
--EXPECT--
array(1) {
  ["a"]=>
  int(1)
}
array(2) {
  [0]=>
  int(1)
  [1]=>
  int(2)
}
array(2) {
  [0]=>
  string(1) "x"
  [1]=>
  int(2)
}
int(1)
array(1) {
  ["a"]=>
  int(1)
}
array(1) {
  ["a"]=>
  string(1) "f"
}
array(1) {
  ["a"]=>
  int(1)
}
