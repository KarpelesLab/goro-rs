--TEST--
Closure::fromCallable() errors are thrown from its frame
--FILE--
<?php
try { Closure::fromCallable("nope"); } catch (Error $e) { echo $e->getTraceAsString(), "\n"; }
try { Closure::fromCallable("ArrayObject::count"); } catch (Error $e) { echo $e->getMessage(), $e->getTraceAsString(), "\n"; }
try { Closure::fromCallable(["ArrayObject", "count"]); } catch (Error $e) { echo $e->getMessage(), $e->getTraceAsString(), "\n"; }
class A { function m() {} }
try { Closure::fromCallable("A::m"); } catch (Error $e) { echo $e->getMessage(), $e->getTraceAsString(), "\n"; }
function f() { try { Closure::fromCallable("nope"); } catch (Error $e) { echo $e->getTraceAsString(), "\n"; } }
f();
try { call_user_func("Closure::fromCallable", "nope"); } catch (Error $e) { echo $e->getTraceAsString(), "\n"; }
try { array_map("Closure::fromCallable", ["nope"]); } catch (Error $e) { echo $e->getTraceAsString(), "\n"; }
try { Closure::fromCallable(...)("nope"); } catch (Error $e) { echo $e->getTraceAsString(), "\n"; }
--EXPECTF--
#0 %s(2): Closure::fromCallable('nope')
#1 {main}
Failed to create closure from callable: non-static method ArrayObject::count() cannot be called statically#0 %s(3): Closure::fromCallable('ArrayObject::co...')
#1 {main}
Failed to create closure from callable: non-static method ArrayObject::count() cannot be called statically#0 %s(4): Closure::fromCallable(Array)
#1 {main}
Failed to create closure from callable: non-static method A::m() cannot be called statically#0 %s(6): Closure::fromCallable('A::m')
#1 {main}
#0 %s(7): Closure::fromCallable('nope')
#1 %s(8): f()
#2 {main}
#0 %s(9): Closure::fromCallable('nope')
#1 {main}
#0 [internal function]: Closure::fromCallable('nope')
#1 %s(10): array_map('Closure::fromCa...', Array)
#2 {main}
#0 %s(11): Closure::fromCallable('nope')
#1 {main}
