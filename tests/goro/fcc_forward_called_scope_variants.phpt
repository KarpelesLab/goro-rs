--TEST--
first-class callables and callbacks of parent::/self:: static methods from instance and static contexts
--FILE--
<?php
class P { public static function s() { return static::class; } public function i() { return static::class; } }
class C extends P {
  public function inst() {
    $r = [];
    $r[] = (parent::s(...))();
    $r[] = (self::s(...))();
    $r[] = (static::s(...))();
    $r[] = Closure::fromCallable('parent::s')();
    $r[] = Closure::fromCallable([$this, 'parent::s'])();
    $r[] = Closure::fromCallable([$this, 'parent::i'])();
    $r[] = (parent::i(...))();
    $r[] = call_user_func('parent::s');
    $r[] = call_user_func([$this, 'parent::s']);
    $r[] = (P::s(...))();
    $r[] = (C::s(...))();
    return $r;
  }
  public static function stat() {
    return [(parent::s(...))(), Closure::fromCallable(['parent', 's'])(), (new ReflectionFunction(parent::s(...)))->getClosureCalledClass()->name];
  }
}
class D extends C {}
var_dump((new D)->inst(), D::stat(), C::stat());
$f = (function() { return parent::s(...); })->bindTo(new D, C::class)();
var_dump($f());
--EXPECTF--

Deprecated: Use of "parent" in callables is deprecated in %s on line 9

Deprecated: Callables of the form ["D", "parent::s"] are deprecated in %s on line 10

Deprecated: Callables of the form ["D", "parent::i"] are deprecated in %s on line 11

Deprecated: Use of "parent" in callables is deprecated in %s on line 13

Deprecated: Callables of the form ["D", "parent::s"] are deprecated in %s on line 14

Deprecated: Use of "parent" in callables is deprecated in %s on line 20

Deprecated: Use of "parent" in callables is deprecated in %s on line 20
array(11) {
  [0]=>
  string(1) "D"
  [1]=>
  string(1) "D"
  [2]=>
  string(1) "D"
  [3]=>
  string(1) "D"
  [4]=>
  string(1) "D"
  [5]=>
  string(1) "D"
  [6]=>
  string(1) "D"
  [7]=>
  string(1) "D"
  [8]=>
  string(1) "D"
  [9]=>
  string(1) "P"
  [10]=>
  string(1) "C"
}
array(3) {
  [0]=>
  string(1) "D"
  [1]=>
  string(1) "D"
  [2]=>
  string(1) "D"
}
array(3) {
  [0]=>
  string(1) "C"
  [1]=>
  string(1) "C"
  [2]=>
  string(1) "C"
}
string(1) "D"
