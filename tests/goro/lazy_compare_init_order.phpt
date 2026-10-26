--TEST--
Comparing lazy objects initializes the right operand first (the left one for > and >=)
--FILE--
<?php
class A { public $a; }
class B { public $a; }
$r = new ReflectionClass('A');
$mk = fn($n, $v = 1) => $r->newLazyGhost(function ($x) use ($n, $v) { echo "init $n\n"; $x->a = $v; });
$o = $mk('o'); $p = $mk('p');
var_dump($o == $p);
$o = $mk('o'); $p = $mk('p', 2);
var_dump($o != $p);
$o = $mk('o'); $p = $mk('p', 2);
var_dump($o < $p);
$o = $mk('o'); $p = $mk('p', 2);
var_dump($o <= $p);
$o = $mk('o'); $p = $mk('p', 2);
var_dump($o > $p);
$o = $mk('o'); $p = $mk('p', 2);
var_dump($o >= $p);
$o = $mk('o'); $p = $mk('p', 2);
var_dump($o <=> $p);
echo "-- same object / different classes: no initialization\n";
$o = $mk('o');
var_dump($o == $o);
var_dump($o == new B);
var_dump($r->isUninitializedLazyObject($o));
echo "-- in arrays\n";
$o = $mk('o'); $p = $mk('p', 2);
var_dump([$o] == [$p]);
echo "-- proxies\n";
$o = $r->newLazyProxy(function () { echo "init o\n"; return new A; });
$p = $r->newLazyProxy(function () { echo "init p\n"; return new A; });
var_dump($o == $p);
--EXPECT--
init p
init o
bool(true)
init p
init o
bool(true)
init p
init o
bool(true)
init p
init o
bool(true)
init o
init p
bool(false)
init o
init p
bool(false)
init p
init o
int(-1)
-- same object / different classes: no initialization
bool(true)
bool(false)
bool(true)
-- in arrays
init p
init o
bool(false)
-- proxies
init p
init o
bool(true)
