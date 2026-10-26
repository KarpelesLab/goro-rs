--TEST--
[$obj, 'parent::m']() looks the method name up as is and reaches __call()
--FILE--
<?php
class P { function m() { echo "P::m\n"; } }
class A extends P { function __call($n, $a) { echo "__call $n\n"; } static function __callStatic($n, $a) { echo "__callStatic $n\n"; }
  function t() { [$this, 'parent::m'](); [$this, 'parent::nope'](); call_user_func([$this, 'parent::nope']); call_user_func('parent::nope'); }
  static function st() { call_user_func('parent::nope'); call_user_func([static::class, 'parent::nope']); }
}
[new A,'parent::m']();
try { [new A,'parent::nope'](); } catch (Error $e) { echo $e->getMessage(), "\n"; }
(new A)->t();
A::st();
class B { function __call($n, $a) { echo "B__call $n\n"; } }
class B2 extends B { function t() { call_user_func('parent::zz'); [$this, 'parent::zz'](); } }
(new B2)->t();
try { [new B2, 'parent::zz'](); } catch (Error $e) { echo $e->getMessage(), "\n"; }
--EXPECTF--
__call parent::m
__call parent::nope
__call parent::m
__call parent::nope

Deprecated: Callables of the form ["A", "parent::nope"] are deprecated in %s on line 4

Fatal error: Uncaught TypeError: call_user_func(): Argument #1 ($callback) must be a valid callback, class P does not have a method "nope" in %s:4
Stack trace:
#0 %s(9): A->t()
#1 {main}
  thrown in %s on line 4
