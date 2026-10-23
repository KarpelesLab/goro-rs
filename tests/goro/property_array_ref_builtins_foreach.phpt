--TEST--
Array builtins and by-reference foreach fetch a property for writing (readonly errors, no auto-initialization)
--FILE--
<?php
class D { function __construct(public readonly array $arr = [3, 1, 2]) {}
  function inside() { try { sort($this->arr); } catch (Error $e) { echo get_class($e), ': ', $e->getMessage(), "\n"; } } }
function t($l, $f) { try { $f(); echo "$l: ok\n"; } catch (\Throwable $e) { echo "$l: ", get_class($e), ": ", $e->getMessage(), "\n"; } }
$d = new D;
t(1, fn() => array_push($d->arr, 4));
t(2, fn() => sort($d->arr));
t(3, function () use ($d) { foreach ($d->arr as &$v) $v = 0; });
t(4, fn() => preg_match('/x/', 'x', $d->arr));
t(5, function () use ($d) { $r = &$d->arr; });
t(6, function () use ($d) { $d->arr[] = 1; });
function byref(&$x) { $x[] = 1; }
t(7, fn() => byref($d->arr));
t(8, fn() => array_splice($d->arr, 0, 1));
t(9, fn() => end($d->arr));
t(10, fn() => current($d->arr));
$d->inside();
var_dump($d->arr);
class A { public $a = [1, 2]; public $o; private $p = [5]; public ?array $n = null;
  function __construct() { $this->o = new ArrayObject([7, 8]); }
  function pr() { foreach ($this->p as &$v) $v *= 2; unset($v); return $this->p; } }
$x = new A;
foreach ($x->a as &$v) $v *= 10; unset($v);
var_dump($x->a);
var_dump($x->a);
foreach ($x->o as &$v) {}
var_dump($x->pr());
foreach ($x->n as &$v) {} var_dump($x->n);
foreach ($x->undef as &$v) {} var_dump($x->undef);
class M { private $d = ['q' => [1]]; function __get($n) { echo "get $n\n"; return $this->d[$n]; } }
$m = new M; foreach ($m->q as &$v) $v = 3; unset($v);
$s = new stdClass; foreach ($s->dyn as &$v) {} var_dump(get_object_vars($s));
sort($x->a); var_dump($x->a);
array_push($x->a, 5); var_dump(count($x->a));
$y = $x->a; sort($x->a); var_dump($y === $x->a);
$arr = ['k' => new A]; sort($arr['k']->a); var_dump($arr['k']->a);
--EXPECTF--
1: Error: Cannot indirectly modify readonly property D::$arr
2: Error: Cannot indirectly modify readonly property D::$arr
3: Error: Cannot indirectly modify readonly property D::$arr
4: Error: Cannot indirectly modify readonly property D::$arr
5: Error: Cannot indirectly modify readonly property D::$arr
6: Error: Cannot indirectly modify readonly property D::$arr
7: Error: Cannot indirectly modify readonly property D::$arr
8: Error: Cannot indirectly modify readonly property D::$arr
9: Error: Cannot indirectly modify readonly property D::$arr
10: ok
Error: Cannot indirectly modify readonly property D::$arr
array(3) {
  [0]=>
  int(3)
  [1]=>
  int(1)
  [2]=>
  int(2)
}
array(2) {
  [0]=>
  int(10)
  [1]=>
  int(20)
}
array(2) {
  [0]=>
  int(10)
  [1]=>
  int(20)
}
array(1) {
  [0]=>
  int(10)
}

Warning: foreach() argument must be of type array|object, null given in %s on line 28
NULL

Deprecated: Creation of dynamic property A::$undef is deprecated in %s on line 29

Warning: foreach() argument must be of type array|object, null given in %s on line 29
NULL
get q

Notice: Indirect modification of overloaded property M::$q has no effect in %s on line 31

Warning: foreach() argument must be of type array|object, null given in %s on line 32
array(1) {
  ["dyn"]=>
  NULL
}
array(2) {
  [0]=>
  int(10)
  [1]=>
  int(20)
}
int(3)
bool(false)
array(2) {
  [0]=>
  int(1)
  [1]=>
  int(2)
}
