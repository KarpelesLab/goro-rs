--TEST--
new ReflectionMethod(method closure, '__invoke') is the internal Closure::__invoke
--FILE--
<?php
class A { function getC() { return 1; } }
$c = Closure::fromCallable([new A, 'getC']);
$m = new ReflectionMethod($c, '__invoke');
var_dump($m->class, $m->name, $m->isInternal(), $m->getNumberOfParameters());
$c2 = function($x) {};
$m = new ReflectionMethod($c2, '__invoke');
var_dump($m->class, $m->name, $m->isInternal(), $m->getNumberOfParameters());
$c3 = strlen(...);
$m = new ReflectionMethod($c3, '__invoke');
var_dump($m->class, $m->name, $m->isInternal(), $m->getNumberOfParameters());
$m = new ReflectionMethod('Closure::__invoke');
var_dump($m->class, $m->name);
--EXPECTF--
string(7) "Closure"
string(8) "__invoke"
bool(true)
int(0)
string(7) "Closure"
string(8) "__invoke"
bool(true)
int(1)
string(7) "Closure"
string(8) "__invoke"
bool(true)
int(1)

Deprecated: Calling ReflectionMethod::__construct() with 1 argument is deprecated, use ReflectionMethod::createFromMethodName() instead in %s on line 12

Fatal error: Uncaught ReflectionException: Method Closure::__invoke() does not exist in %s:12
Stack trace:
#0 %s(12): ReflectionMethod->__construct('Closure::__invo...')
#1 {main}
  thrown in %s on line 12
