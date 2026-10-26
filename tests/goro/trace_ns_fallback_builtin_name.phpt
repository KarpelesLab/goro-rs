--TEST--
A builtin called by an unqualified name in a namespace has its own name in backtraces
--FILE--
<?php
namespace App\Sub;
try { intdiv(1, 0); } catch (\Error $e) { echo $e->getTraceAsString(), "\n"; }
function f() { try { str_repeat("x", -1); } catch (\Error $e) { echo $e->getTraceAsString(), "\n"; } }
f();
try { array_map(fn($x) => throw new \Exception("x"), [1]); } catch (\Exception $e) { echo $e->getTraceAsString(), "\n"; }
function g() { debug_print_backtrace(); }
array_map('App\Sub\g', [1]);
--EXPECTF--
#0 %s(3): intdiv(1, 0)
#1 {main}
#0 %s(4): str_repeat('x', -1)
#1 %s(5): App\Sub\f()
#2 {main}
#0 [internal function]: {closure:%s:6}(1)
#1 %s(6): array_map(Object(Closure), Array)
#2 {main}
#0 [internal function]: App\Sub\g(1)
#1 %s(8): array_map('App\\Sub\\g', Array)
