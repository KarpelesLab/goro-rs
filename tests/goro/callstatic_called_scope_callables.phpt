--TEST--
__callStatic() reached through callables (call_user_func, array_map, fromCallable, $class::m(...)) keeps the called scope
--FILE--
<?php
class A {
    function __call($n, $a) { return "call:$n:" . static::class . ":" . get_class($this); }
    static function __callStatic($n, $a) { return "cs:$n:" . static::class . ":" . get_called_class() . ":" . self::class; }
}
class B extends A {
    static function go() { return [call_user_func(['static', 'g1']), call_user_func('static::g2'), call_user_func([static::class, 'g3'])]; }
    static function cmp($a, $b) { return 0; }
}
class C extends B {}
var_dump(call_user_func(['B', 'zzz']));
var_dump(call_user_func('B::zzz'));
var_dump(call_user_func_array('B::zzz', []));
var_dump(call_user_func_array(['B', 'zzz'], [1]));
var_dump(array_map('B::q', [1]));
var_dump(array_map(['B', 'q'], [1]));
var_dump(array_map(['C', 'q'], [1]));
$f = Closure::fromCallable(['B', 'fc']); var_dump($f());
$f = Closure::fromCallable('C::fc2'); var_dump($f());
$f = B::fcc(...); var_dump($f());
$f = C::fcc(...); var_dump($f());
$x = 'C'; $f = $x::fcc2(...); var_dump($f());
$cb = ['C', 'dyn']; var_dump($cb());
$cb = 'C::dyn2'; var_dump($cb());
var_dump(C::go());
var_dump(call_user_func([new C, 'inst']));
var_dump(call_user_func('C::inst2'));
$a = [1]; array_walk($a, ['C', 'walk']);
var_dump(is_callable(['B', 'zzz']));
--EXPECTF--
string(12) "cs:zzz:B:B:A"
string(12) "cs:zzz:B:B:A"
string(12) "cs:zzz:B:B:A"
string(12) "cs:zzz:B:B:A"
array(1) {
  [0]=>
  string(10) "cs:q:B:B:A"
}
array(1) {
  [0]=>
  string(10) "cs:q:B:B:A"
}
array(1) {
  [0]=>
  string(10) "cs:q:C:C:A"
}
string(11) "cs:fc:B:B:A"
string(12) "cs:fc2:C:C:A"
string(12) "cs:fcc:B:B:A"
string(12) "cs:fcc:C:C:A"
string(13) "cs:fcc2:C:C:A"
string(12) "cs:dyn:C:C:A"
string(13) "cs:dyn2:C:C:A"

Deprecated: Use of "static" in callables is deprecated in %s on line 7

Deprecated: Use of "static" in callables is deprecated in %s on line 7
array(3) {
  [0]=>
  string(11) "cs:g1:C:C:A"
  [1]=>
  string(11) "cs:g2:C:C:A"
  [2]=>
  string(11) "cs:g3:C:C:A"
}
string(13) "call:inst:C:C"
string(14) "cs:inst2:C:C:A"
bool(true)
