--TEST--
ReflectionObject of a Generator
--FILE--
<?php
function g() { yield 1; yield 2; }
$gen = g();
$r = new ReflectionObject($gen);
var_dump($r->getName(), $r->isInternal(), $r->isFinal(), $r->isIterable(), $r->getProperties());
--EXPECT--
string(9) "Generator"
bool(true)
bool(true)
bool(true)
array(0) {
}
