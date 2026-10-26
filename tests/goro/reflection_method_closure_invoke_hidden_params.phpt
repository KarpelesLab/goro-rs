--TEST--
ReflectionMethod of the __invoke of a method closure hides its $this
--FILE--
<?php
class A { private $x = 'ax'; function getC() { return function() { return $this->x; }; } }
class B extends A {}
$c = (new B)->getC();
$r = new ReflectionFunction($c);
var_dump($r->getClosureScopeClass()->name, $r->getClosureCalledClass()->name, get_class($r->getClosureThis()));
$bound = Closure::bind(function() { return static::class; }, null, B::class);
$r = new ReflectionFunction($bound);
var_dump($r->getClosureScopeClass()->name, $r->getClosureThis(), $r->getClosureCalledClass()->name);
$fc = Closure::fromCallable([new B, 'getC']);
$r = new ReflectionFunction($fc);
var_dump($r->getName(), $r->isClosure(), $r->getClosureScopeClass()->name, get_class($r->getClosureThis()));
echo $r;
$m = new ReflectionMethod($fc, '__invoke');
var_dump($m->class, $m->name);
echo $m;
$m = new ReflectionMethod(function($a, $b = 1) {}, '__invoke');
echo $m;
var_dump($m->getNumberOfParameters());
var_dump((new ReflectionClass('Closure'))->hasMethod('__invoke'));
var_dump((new ReflectionObject(function() {}))->hasMethod('__invoke'));
--EXPECTF--
string(1) "A"
string(1) "B"
string(1) "B"
string(1) "B"
NULL
string(1) "B"
string(4) "getC"
bool(true)
string(1) "A"
string(1) "B"
Closure [ <user> public method getC ] {
  @@ %s 2 - 2
}
string(7) "Closure"
string(8) "__invoke"
Method [ <internal> public method __invoke ] {
}
Method [ <internal> public method __invoke ] {

  - Parameters [2] {
    Parameter #0 [ <required> $a ]
    Parameter #1 [ <optional> $b = <default> ]
  }
}
int(2)
bool(true)
bool(true)
