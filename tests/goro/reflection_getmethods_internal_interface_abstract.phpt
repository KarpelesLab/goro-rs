--TEST--
ReflectionClass::getMethods() of an abstract class lists the methods of an extension interface it implements
--FILE--
<?php
abstract class B implements \SessionHandlerInterface {
    abstract public function create_sid(): string;
    abstract public function validateId(string $id): bool;
}
interface I { function foo(); }
abstract class C implements I {}
foreach ([B::class, C::class] as $c) {
$r = new ReflectionClass($c);
foreach ($r->getMethods() as $m) echo $m->class, "::", $m->name, " ", $m->isAbstract() ? "abs" : "", "\n";
var_dump($r->hasMethod('open'), $r->hasMethod('foo'));
}

?>
--EXPECT--
B::create_sid abs
B::validateId abs
SessionHandlerInterface::open abs
SessionHandlerInterface::close abs
SessionHandlerInterface::read abs
SessionHandlerInterface::write abs
SessionHandlerInterface::destroy abs
SessionHandlerInterface::gc abs
bool(true)
bool(false)
I::foo abs
bool(false)
bool(true)
