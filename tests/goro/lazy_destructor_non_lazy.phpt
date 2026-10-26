--TEST--
Lazy objects: the destructor runs once marked initialized, or when the class has no properties
--FILE--
<?php
class A { public $a; public function __destruct() { echo "dtor A\n"; } }
class B { public function __destruct() { echo "dtor B\n"; } }
$r = new ReflectionClass('A');
$o = $r->newLazyGhost(function ($o) { echo "init\n"; });
$r->markLazyObjectAsInitialized($o);
$o = null;
echo "1\n";
$r = new ReflectionClass('B');
$o = $r->newLazyGhost(function ($o) { echo "init\n"; });
$r->initializeLazyObject($o);
$o = null;
echo "2\n";
$o = $r->newLazyGhost(function ($o) { echo "init\n"; });
var_dump($r->isUninitializedLazyObject($o));
$o = null;
echo "3\n";
$o = $r->newLazyProxy(function ($o) { echo "init\n"; return new B; });
var_dump($r->isUninitializedLazyObject($o));
$o = null;
echo "4\n";
$r = new ReflectionClass('A');
$o = $r->newLazyGhost(function ($o) { echo "init\n"; });
$o->dyn = 1;
var_dump($o);
$o = null;
echo "5\n";
$o = $r->newLazyProxy(function ($o) { echo "init\n"; return new A; });
$r->markLazyObjectAsInitialized($o);
var_dump($o);
$o = null;
echo "6\n";
$o = $r->newLazyGhost(function ($o) { echo "init\n"; });
$o = null;
echo "end\n";
--EXPECTF--
dtor A
1
dtor B
2
bool(false)
dtor B
3
bool(false)
dtor B
4
init

Deprecated: Creation of dynamic property A::$dyn is deprecated in %s on line 24
object(A)#1 (2) {
  ["a"]=>
  NULL
  ["dyn"]=>
  int(1)
}
dtor A
5
object(A)#3 (1) {
  ["a"]=>
  NULL
}
dtor A
6
end
