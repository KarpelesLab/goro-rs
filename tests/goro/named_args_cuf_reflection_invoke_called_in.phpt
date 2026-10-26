--TEST--
No 'called in' for a TypeError of a function invoked by Reflection or a named-argument call_user_func() (a real call with its own frame)
--FILE--
<?php
function f(int $z, ...$rest) { debug_print_backtrace(); return $z; }
function g() { throw new Exception("x"); }
class A { function m(int $z, int $y = 0) {} }
$t = function ($c) { try { var_dump($c()); } catch (Throwable $e) { echo $e->getMessage(), "\n", $e->getTraceAsString(), "\n"; } };
$t(fn() => call_user_func('f', z: 'x'));
$t(fn() => call_user_func('f', 1, q: 2));
$t(fn() => call_user_func('f', 2));
$t(fn() => call_user_func(...['f', 'x']));
$t(fn() => call_user_func_array('f', ['z' => 'x']));
$t(fn() => call_user_func(callback: 'g'));
$t(fn() => (new ReflectionMethod('A', 'm'))->invokeArgs(new A, ['z' => 'x']));
$t(fn() => (new ReflectionMethod('A', 'm'))->invoke(new A, 'x'));
$t(fn() => (new ReflectionMethod('A', 'm'))->invokeArgs(new A, [1, 'y' => 'x']));
--EXPECTF--
f(): Argument #1 ($z) must be of type int, string given
#0 [internal function]: f('x')
#1 %s: call_user_func('f', z: 'x')
#2 %s: {closure:%s:6}()
#3 %s: {closure:%s:5}(Object(Closure))
#4 {main}
#0 [internal function]: f(1, q: 2)
#1 %s: call_user_func('f', 1, q: 2)
#2 %s: {closure:%s:7}()
#3 %s: {closure:%s:5}(Object(Closure))
int(1)
#0 %s: f(2)
#1 %s: {closure:%s:8}()
#2 %s: {closure:%s:5}(Object(Closure))
int(2)
f(): Argument #1 ($z) must be of type int, string given
#0 [internal function]: f('x')
#1 %s: call_user_func('f', 'x')
#2 %s: {closure:%s:9}()
#3 %s: {closure:%s:5}(Object(Closure))
#4 {main}
f(): Argument #1 ($z) must be of type int, string given, called in %s on line 10
#0 %s: f('x')
#1 %s: {closure:%s:10}()
#2 %s: {closure:%s:5}(Object(Closure))
#3 {main}
x
#0 [internal function]: g()
#1 %s: call_user_func('g')
#2 %s: {closure:%s:11}()
#3 %s: {closure:%s:5}(Object(Closure))
#4 {main}
A::m(): Argument #1 ($z) must be of type int, string given
#0 [internal function]: A->m('x')
#1 %s: ReflectionMethod->invokeArgs(Object(A), Array)
#2 %s: {closure:%s:12}()
#3 %s: {closure:%s:5}(Object(Closure))
#4 {main}
A::m(): Argument #1 ($z) must be of type int, string given
#0 [internal function]: A->m('x')
#1 %s: ReflectionMethod->invoke(Object(A), 'x')
#2 %s: {closure:%s:13}()
#3 %s: {closure:%s:5}(Object(Closure))
#4 {main}
A::m(): Argument #2 ($y) must be of type int, string given
#0 [internal function]: A->m(1, 'x')
#1 %s: ReflectionMethod->invokeArgs(Object(A), Array)
#2 %s: {closure:%s:14}()
#3 %s: {closure:%s:5}(Object(Closure))
#4 {main}
