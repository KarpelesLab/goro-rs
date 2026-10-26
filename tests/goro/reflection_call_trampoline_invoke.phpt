--TEST--
ReflectionFunction/ReflectionMethod(__invoke) of a fromCallable() __call trampoline
--FILE--
<?php
class A { function __call($n, $a) { return $n; } static function __callStatic($n, $a) { return $n; } }
$f = Closure::fromCallable([new A, 'undefinedMethod']);
$r = new ReflectionFunction($f);
var_dump($r->getName(), $r->getNumberOfParameters(), $r->getNumberOfRequiredParameters(), $r->isVariadic(), $r->isInternal(), $r->isUserDefined(), $r->returnsReference(), $r->getFileName(), $r->getStartLine());
foreach ($r->getParameters() as $p) { var_dump($p->getName(), $p->isVariadic(), $p->isOptional(), (string)$p); }
var_dump($r->getClosureThis() instanceof A, $r->getClosureScopeClass()?->name, $r->getClosureCalledClass()?->name);
echo $r;
var_dump($r->invoke(1,2));
$f2 = Closure::fromCallable('A::stat');
$r2 = new ReflectionFunction($f2);
var_dump($r2->getName(), $r2->getClosureThis());
var_dump($f, $f2);
$m = new ReflectionMethod($f, '__invoke'); var_dump($m->getNumberOfParameters()); echo $m;
--EXPECT--
string(15) "undefinedMethod"
int(0)
int(0)
bool(false)
bool(true)
bool(false)
bool(false)
bool(false)
bool(false)
bool(true)
string(1) "A"
string(1) "A"
Closure [ <internal> public method undefinedMethod ] {
}
string(15) "undefinedMethod"
string(4) "stat"
NULL
object(Closure)#2 (2) {
  ["function"]=>
  string(18) "A::undefinedMethod"
  ["this"]=>
  object(A)#1 (0) {
  }
}
object(Closure)#4 (1) {
  ["function"]=>
  string(7) "A::stat"
}
int(0)
Method [ <internal> public method __invoke ] {
}
