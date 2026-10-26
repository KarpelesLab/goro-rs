--TEST--
first-class callables and fromCallable of parent:: static methods forward static::
--FILE--
<?php
class P { public static function s() { var_dump(static::class); } public function m() { var_dump(static::class, get_class($this)); } }
class C extends P {
  public static function t() { $f = parent::s(...); $f(); $g = Closure::fromCallable("parent::s"); $g(); $h = Closure::fromCallable([static::class, "parent::s"]); $h();
     call_user_func("parent::s"); call_user_func([static::class, "parent::s"]); }
  public function u() { $f = parent::m(...); $f(); $g = Closure::fromCallable("parent::m"); $g(); $r = new ReflectionFunction($f); var_dump($r->getClosureScopeClass()->name, $r->getClosureCalledClass()->name); }
}
class D extends C {}
D::t();
(new D)->u();
--EXPECTF--
string(1) "D"

Deprecated: Use of "parent" in callables is deprecated in %s on line 4
string(1) "D"

Deprecated: Callables of the form ["D", "parent::s"] are deprecated in %s on line 4
string(1) "D"

Deprecated: Use of "parent" in callables is deprecated in %s on line 5
string(1) "D"

Deprecated: Callables of the form ["D", "parent::s"] are deprecated in %s on line 5
string(1) "D"
string(1) "D"
string(1) "D"

Deprecated: Use of "parent" in callables is deprecated in %s on line 6
string(1) "D"
string(1) "D"
string(1) "P"
string(1) "D"
