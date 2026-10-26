--TEST--
Temporary objects passed to internal methods are released when the call returns
--FILE--
<?php
class A { public $a; public function __destruct() { echo "dtor\n"; } }
$r = new ReflectionClass('A');
var_dump($r->isUninitializedLazyObject(new A));
echo "1\n";
var_dump($r->isInstance(new A));
echo "2\n";
$s = new SplObjectStorage;
var_dump($s->contains(new A));
echo "3\n";
var_dump($r->getLazyInitializer(new A));
echo "4\n";
var_dump($r->initializeLazyObject(new A) instanceof A);
echo "5\n";
var_dump($r->markLazyObjectAsInitialized(new A) instanceof A);
echo "6\n";
var_dump($r->getLazyInitializer($r->newLazyGhost(fn() => 1)) instanceof Closure);
echo "7\n";
$rp = new ReflectionProperty('A', 'a');
var_dump($rp->isLazy(new A));
echo "8\n";
var_dump($rp->getValue(new A));
echo "9\n";
var_dump(get_class(new A));
echo "end\n";
--EXPECTF--
dtor
bool(false)
1
dtor
bool(true)
2

Deprecated: Method SplObjectStorage::contains() is deprecated since 8.5, use method SplObjectStorage::offsetExists() instead in %s on line 9
dtor
bool(false)
3
dtor
NULL
4
dtor
bool(true)
5
dtor
bool(true)
6
bool(true)
7
dtor
bool(false)
8
dtor
NULL
9
dtor
string(1) "A"
end
