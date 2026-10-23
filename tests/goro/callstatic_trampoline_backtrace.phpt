--TEST--
Backtraces of __call()/__callStatic() trampolines reached through callables
--FILE--
<?php
class A { static function __callStatic($n, $a) { debug_print_backtrace(); return static::class; } function __call($n, $a) { debug_print_backtrace(); return static::class; } }
class B extends A {}
call_user_func(['B', 'zzz'], 1);
call_user_func('B::yyy', 2);
array_map(['B', 'q'], [1]);
$cb = ['B', 'dyn']; $cb(3);
B::direct(4);
$x = 'B'; $x::dd(5);
call_user_func([new B, 'inst'], 6);
call_user_func_array([new B, 'inst'], [6]);
$o = new B; $m = 'mm'; $o->$m(7);
call_user_func_array(['B', 'zzz'], ['a' => 1]);
--EXPECTF--
#0 %s(4): A::__callStatic('zzz', Array)
#0 %s(5): A::__callStatic('yyy', Array)
#0 [internal function]: A::__callStatic('q', Array)
#1 %s(6): array_map(Array, Array)
#0 %s(7): A::__callStatic('dyn', Array)
#0 %s(8): A::__callStatic('direct', Array)
#0 %s(9): A::__callStatic('dd', Array)
#0 %s(10): A->__call('inst', Array)
#0 %s(11): A->__call('inst', Array)
#0 %s(12): A->__call('mm', Array)
#0 %s(13): A::__callStatic('zzz', Array)
