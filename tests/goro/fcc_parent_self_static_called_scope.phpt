--TEST--
parent::m(...) / self::m(...) / fromCallable('parent::m') of a static method keep the called scope
--FILE--
<?php
class P { public static function s($x = 1) { var_dump(static::class); return new Exception("t"); } }
class C extends P {
  public static function t() {
    $f = parent::s(...); $e = $f(5); echo $e->getTraceAsString(), "\n";
    var_dump($f);
    $r = new ReflectionFunction($f); var_dump($r->getName(), $r->getClosureScopeClass()->name, $r->getClosureCalledClass()->name, $r->getNumberOfParameters());
    $g = self::s(...); $g();
    var_dump($f == parent::s(...), $f === $f);
    $h = Closure::fromCallable(['parent', 's']); $h();
    $i = Closure::fromCallable('self::s'); $i();
    $j = Closure::fromCallable('C::s'); $j();
    $k = C::s(...); $k();
    $b = Closure::bind($f, null, 'C'); var_dump($b);
  }
  public static function s($x = 1) { echo "C::s\n"; return parent::s($x); }
}
class D extends C {}
D::t();
--EXPECTF--
string(1) "D"
#0 %s(5): P::s(5)
#1 %s(19): C::t()
#2 {main}
object(Closure)#1 (2) {
  ["function"]=>
  string(4) "P::s"
  ["parameter"]=>
  array(1) {
    ["$x"]=>
    string(10) "<optional>"
  }
}
string(1) "s"
string(1) "P"
string(1) "D"
int(1)
C::s
string(1) "D"
bool(true)
bool(true)

Deprecated: Use of "parent" in callables is deprecated in %s on line 10
string(1) "D"

Deprecated: Use of "self" in callables is deprecated in %s on line 11
C::s
string(1) "D"
C::s
string(1) "C"
C::s
string(1) "C"

Warning: Cannot rebind scope of closure created from method, this will be an error in PHP 9 in %s on line 14
NULL
