--TEST--
By-reference foreach over a property, an element or a static property iterates the live array (made a reference)
--FILE--
<?php
class O { public $a = [1,2]; }
$o = new O;
$n = 0;
foreach ($o->a as $k => &$v) { echo "$k=$v\n"; if ($n++ < 3) $o->a[] = $v * 10; }
unset($v);
var_dump($o->a);
class S { static $a = [1,2]; }
$n = 0;
foreach (S::$a as $k => &$v) { echo "$k=$v\n"; if ($n++ < 3) S::$a[] = $v * 10; }
unset($v);
$arr = ['p' => [1,2]]; $n=0;
foreach ($arr['p'] as $k => &$v) { echo "$k=$v\n"; if ($n++ < 3) $arr['p'][] = $v * 10; }
unset($v);
$o2 = new O; $n = 0;
foreach ($o2->a as $k => &$v) { echo "$k=$v\n"; if ($n++ < 1) $o2->a = [5,6,7]; }
unset($v);
$o3 = new stdClass; $o3->q = [1,2]; $n = 0;
foreach ($o3->q as $k => &$v) { echo "$k=$v\n"; if ($n++ < 2) $o3->q[] = 'x'; }
$o = new stdClass;
foreach ($o->nope as &$v) {}
var_dump($o);
class T { public array $t = [1]; public int $i = 1; public readonly array $ro; function __construct() { $this->ro = [1]; } }
$t = new T;
foreach ($t->t as &$v) { $v = 2; }
unset($v);
var_dump($t);
foreach ($t->i as &$v) {}
try { foreach ($t->ro as &$v) {} } catch (Error $e) { echo $e->getMessage(), "\n"; }
$a = [];
foreach ($a['x'] as &$v) {}
var_dump($a);
$o2 = new stdClass; $o2->a = [1, 2];
foreach ($o2->a as &$v) { var_dump($o2); break; }
class S3 { static $a = [1,2]; }
$n = 0;
foreach (S3::$a as $k => &$v) { echo "$k=$v\n"; if ($n++ < 3) { S3::$a[] = $v * 10; } }
unset($v);
class S4 { static $a; }
S4::$a = [1, 2]; $n = 0;
foreach (S4::$a as $k => &$v) { echo "$k=$v\n"; if ($n++ < 3) { S4::$a[] = $v * 10; } }
--EXPECTF--
0=1
1=2
2=10
3=20
4=100
array(5) {
  [0]=>
  int(1)
  [1]=>
  int(2)
  [2]=>
  int(10)
  [3]=>
  int(20)
  [4]=>
  int(100)
}
0=1
1=2
2=10
3=20
4=100
0=1
1=2
2=10
3=20
4=100
0=1
0=5
1=6
2=7
0=1
1=2
2=x
3=x

Warning: foreach() argument must be of type array|object, null given in %s on line 21
object(stdClass)#4 (1) {
  ["nope"]=>
  NULL
}
object(T)#1 (3) {
  ["t"]=>
  array(1) {
    [0]=>
    int(2)
  }
  ["i"]=>
  int(1)
  ["ro"]=>
  array(1) {
    [0]=>
    int(1)
  }
}

Warning: foreach() argument must be of type array|object, int given in %s on line 28
Cannot indirectly modify readonly property T::$ro

Warning: foreach() argument must be of type array|object, null given in %s on line 31
array(1) {
  ["x"]=>
  NULL
}
object(stdClass)#6 (1) {
  ["a"]=>
  &array(2) {
    [0]=>
    &int(1)
    [1]=>
    int(2)
  }
}
0=1
1=2
2=10
3=20
4=100
0=1
1=2
2=10
3=20
4=100
