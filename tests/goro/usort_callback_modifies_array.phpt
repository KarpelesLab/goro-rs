--TEST--
usort() & co. sort a copy and write it back to the variable, whatever the callbacks did to it
--FILE--
<?php
$a = [3,1,2];
usort($a, function($x, $y) use (&$a) { $a = 'gone'; return $x <=> $y; });
var_dump($a);
$b = [3,1,2];
usort($b, function($x, $y) use (&$b) { $b[] = 9; return $x <=> $y; });
var_dump($b);
$c = ['x'=>3,'y'=>1];
uasort($c, function($x, $y) use (&$c) { unset($c['x']); return $x <=> $y; });
var_dump($c);
$d = ['x'=>3,'y'=>1];
uksort($d, function($x, $y) use (&$d) { $d = null; return $x <=> $y; });
var_dump($d);
class P { public $arr = [3,1,2]; }
$p = new P;
usort($p->arr, function($x, $y) use ($p) { $p->arr = [7]; return $x <=> $y; });
var_dump($p->arr);
$e = [3,1,2];
usort($e, function($x, $y) use (&$e) { var_dump(count($e)); return $x <=> $y; });
function f2() { return [3, 1]; }
$cmp = fn($a, $b) => $a <=> $b;
var_dump(usort(f2(), $cmp));
$x = [3, 1, 2];
$fn = 'usort';
$fn($x, $cmp);
var_dump($x);
$y = [3, 1, 2];
call_user_func_array('usort', [&$y, $cmp]);
var_dump($y);
$o = new ArrayObject([3, 1]);
$o->uasort($cmp);
var_dump($o->getArrayCopy());
$z = ['b' => 1, 'a' => 2];
uksort($z, 'strcmp');
var_dump($z);
$r = [3, 1]; $ref = &$r;
usort($ref, $cmp);
var_dump($r);
$s = [3, 1]; $t = $s;
usort($s, $cmp);
var_dump($s, $t);
$n = null;
try { usort($n, $cmp); } catch (TypeError $e) { echo $e->getMessage(), "\n"; }
var_dump($n);
$arr = [[3, 1]];
usort($arr[0], $cmp);
var_dump($arr);
class P2 { public array $a = [2, 1]; public $u; }
$p = new P2;
usort($p->a, $cmp);
var_dump($p->a);
usort($p->u, $cmp);
--EXPECTF--
array(3) {
  [0]=>
  int(1)
  [1]=>
  int(2)
  [2]=>
  int(3)
}
array(3) {
  [0]=>
  int(1)
  [1]=>
  int(2)
  [2]=>
  int(3)
}
array(2) {
  ["y"]=>
  int(1)
  ["x"]=>
  int(3)
}
array(2) {
  ["x"]=>
  int(3)
  ["y"]=>
  int(1)
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
int(3)
int(3)

Notice: Only variables should be passed by reference in %s on line 22
bool(true)
array(3) {
  [0]=>
  int(1)
  [1]=>
  int(2)
  [2]=>
  int(3)
}
array(3) {
  [0]=>
  int(1)
  [1]=>
  int(2)
  [2]=>
  int(3)
}
array(2) {
  [1]=>
  int(1)
  [0]=>
  int(3)
}
array(2) {
  ["a"]=>
  int(2)
  ["b"]=>
  int(1)
}
array(2) {
  [0]=>
  int(1)
  [1]=>
  int(3)
}
array(2) {
  [0]=>
  int(1)
  [1]=>
  int(3)
}
array(2) {
  [0]=>
  int(3)
  [1]=>
  int(1)
}
usort(): Argument #1 ($array) must be of type array, null given
NULL
array(1) {
  [0]=>
  array(2) {
    [0]=>
    int(1)
    [1]=>
    int(3)
  }
}
array(2) {
  [0]=>
  int(1)
  [1]=>
  int(2)
}

Fatal error: Uncaught TypeError: usort(): Argument #1 ($array) must be of type array, null given in %s:52
Stack trace:
#0 %s(52): usort(NULL, Object(Closure))
#1 {main}
  thrown in %s on line 52
