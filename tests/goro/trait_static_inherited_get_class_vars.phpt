--TEST--
Trait members bound into a class with a parent; get_class_vars() shows static defaults
--FILE--
<?php
trait T { public static $s = 't'; public $p = 'tp'; const C = 'tc'; public function who() { return static::class . '/' . self::class . '/' . __CLASS__ . '/' . get_class(); } public static function sw() { return static::$s; } }
class A { public static $s = 'a'; public $p = 'ap'; public function who() { return 'A'; } }
class B extends A { use T; }
class C extends B {}
$b = new B; $c = new C;
var_dump($b->who(), $c->who(), B::sw(), C::sw(), A::$s, B::$s, C::$s, B::C, C::C);
B::$s = 'changed'; var_dump(A::$s, B::$s, C::$s);
var_dump(get_class_vars('C'), get_object_vars($c));
foreach (['A', 'B', 'C'] as $k) { $r = new ReflectionClass($k); foreach ($r->getProperties() as $p) echo $k, ' ', $p->class, '::', $p->name, "\n"; foreach ($r->getMethods() as $m) echo $k, ' ', $m->class, '::', $m->name, "\n"; foreach ($r->getReflectionConstants() as $cc) echo $k, ' ', $cc->class, '::', $cc->name, "\n"; }
var_dump((new ReflectionProperty('B', 'p'))->getDeclaringClass()->name, (new ReflectionMethod('C', 'who'))->getDeclaringClass()->name);
--EXPECTF--

Deprecated: Calling get_class() without arguments is deprecated in %s on line 2

Deprecated: Calling get_class() without arguments is deprecated in %s on line 2
string(7) "B/B/B/B"
string(7) "C/B/B/B"
string(1) "t"
string(1) "t"
string(1) "a"
string(1) "t"
string(1) "t"
string(2) "tc"
string(2) "tc"
string(1) "a"
string(7) "changed"
string(7) "changed"
array(2) {
  ["p"]=>
  string(2) "tp"
  ["s"]=>
  string(1) "t"
}
array(1) {
  ["p"]=>
  string(2) "tp"
}
A A::s
A A::p
A A::who
B B::s
B B::p
B B::who
B B::sw
B B::C
C B::s
C B::p
C B::who
C B::sw
C B::C
string(1) "B"
string(1) "B"
