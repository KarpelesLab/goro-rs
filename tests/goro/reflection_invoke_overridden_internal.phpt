--TEST--
ReflectionMethod::invoke() of an internal method runs it, not a user override; prototypes of inherited internal methods
--FILE--
<?php
interface I { public function f(); }
class A implements I { public function f() {} }
class MR extends ReflectionMethod {
    #[\ReturnTypeWillChange]
    public function getPrototype($x = 1): array { return ['overridden']; }
    public function getName(): string { return 'over'; }
    public function p() { return parent::getPrototype(); }
}
class AO extends ArrayObject { public function count(): int { return 42; } }
$m = new MR('A', 'f');
var_dump(get_class($m->p()));
var_dump(get_class((new ReflectionMethod(ReflectionMethod::class, 'getPrototype'))->invoke($m)));
var_dump((new ReflectionMethod(ReflectionFunctionAbstract::class, 'getName'))->invoke($m));
var_dump((new ReflectionMethod(ReflectionMethod::class, 'getName'))->invokeArgs($m, []));
$ao = new AO([1, 2]);
var_dump((new ReflectionMethod('ArrayObject', 'count'))->invoke($ao));
var_dump((new ReflectionMethod('AO', 'count'))->invoke($ao));
try { (new ReflectionMethod('Countable', 'count'))->invoke($ao); } catch (ReflectionException $e) { echo $e->getMessage(), "\n"; }
var_dump(Closure::fromCallable([new ReflectionMethod('ArrayObject', 'count'), 'invoke'])($ao));
class B extends ArrayObject { }
class C extends ArrayObject { public function count(): int { return 1; } }
foreach ([['B', 'count'], ['B', 'getIterator'], ['ArrayObject', 'count'], ['C', 'count'], ['ArrayIterator', 'current'], ['B', 'offsetGet'], ['Exception', 'getMessage'], ['RuntimeException', 'getMessage'], ['RuntimeException', '__construct']] as [$c, $n]) {
    try { $p = (new ReflectionMethod($c, $n))->getPrototype(); echo "$c::$n -> {$p->class}::{$p->name}\n"; }
    catch (ReflectionException $e) { echo $e->getMessage(), "\n"; }
}

?>
--EXPECT--
string(16) "ReflectionMethod"
string(16) "ReflectionMethod"
string(1) "f"
string(1) "f"
int(2)
int(42)
Trying to invoke abstract method Countable::count()
int(2)
B::count -> Countable::count
B::getIterator -> IteratorAggregate::getIterator
ArrayObject::count -> Countable::count
C::count -> Countable::count
ArrayIterator::current -> Iterator::current
B::offsetGet -> ArrayAccess::offsetGet
Exception::getMessage -> Throwable::getMessage
RuntimeException::getMessage -> Throwable::getMessage
Method RuntimeException::__construct does not have a prototype
