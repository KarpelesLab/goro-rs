--TEST--
First-class callables of __call()/__callStatic() methods are trampoline closures
--FILE--
<?php
class K { function i() {} function __call($n, $a) { return "call:$n"; } static function __callStatic($n, $a) { return "cs:$n"; } }
$o = new K;
$a = $o->zz(...);
var_dump($a, $a(1));
$b = K::yy(...);
var_dump($b, $b());
$c = Closure::fromCallable([$o, 'ww']);
var_dump($c, (new ReflectionFunction($c))->getName());
$d = Closure::fromCallable([$o, 'i']);
var_dump((new ReflectionFunction($d))->getName());
$e = [$o, 'vv'](...);
var_dump($e, (new ReflectionFunction($e))->getName());
$f = 'K::uu'(...);
var_dump($f, (new ReflectionFunction($f))->getName());
class Test { public function __call($n, $a) {} public static function __callStatic($n, $a) {} }
foreach ([Test::fail(...), (new Test)->x(...)] as $c) { $r = new ReflectionFunction($c); $p = $r->getParameters(); var_dump(count($p), $p[0]->isVariadic(), (string)$p[0]->getType(), $p[0]->getName(), $r->getNumberOfParameters(), $r->isVariadic()); }
foreach ([Closure::fromCallable("Test::fail"), Closure::fromCallable([new Test, "y"]), [new Test, "z"](...)] as $c) { $r = new ReflectionFunction($c); var_dump(count($r->getParameters()), $r->getNumberOfParameters(), $r->getNumberOfRequiredParameters(), $r->isVariadic()); }
?>
--EXPECT--
object(Closure)#2 (3) {
  ["function"]=>
  string(5) "K::zz"
  ["this"]=>
  object(K)#1 (0) {
  }
  ["parameter"]=>
  array(1) {
    ["$arguments"]=>
    string(10) "<optional>"
  }
}
string(7) "call:zz"
object(Closure)#3 (2) {
  ["function"]=>
  string(5) "K::yy"
  ["parameter"]=>
  array(1) {
    ["$arguments"]=>
    string(10) "<optional>"
  }
}
string(5) "cs:yy"
object(Closure)#4 (2) {
  ["function"]=>
  string(5) "K::ww"
  ["this"]=>
  object(K)#1 (0) {
  }
}
string(2) "ww"
string(1) "i"
object(Closure)#6 (3) {
  ["function"]=>
  string(5) "K::vv"
  ["this"]=>
  object(K)#1 (0) {
  }
  ["parameter"]=>
  array(1) {
    ["$arguments"]=>
    string(10) "<optional>"
  }
}
string(2) "vv"
object(Closure)#7 (2) {
  ["function"]=>
  string(5) "K::uu"
  ["parameter"]=>
  array(1) {
    ["$arguments"]=>
    string(10) "<optional>"
  }
}
string(2) "uu"
int(1)
bool(true)
string(5) "mixed"
string(9) "arguments"
int(1)
bool(true)
int(1)
bool(true)
string(5) "mixed"
string(9) "arguments"
int(1)
bool(true)
int(0)
int(0)
int(0)
bool(false)
int(0)
int(0)
int(0)
bool(false)
int(1)
int(1)
int(0)
bool(true)
