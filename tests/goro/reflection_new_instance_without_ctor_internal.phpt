--TEST--
newInstanceWithoutConstructor(): internal final classes, exceptions, SplFixedArray
--FILE--
<?php
foreach (['WeakReference', 'Generator', 'Closure', 'WeakMap', 'Fiber'] as $c) {
  try { var_dump((new ReflectionClass($c))->newInstanceWithoutConstructor()); }
  catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
}
function mk() { return (new ReflectionClass("Exception"))->newInstanceWithoutConstructor(); }
$e = mk();
var_dump($e->getLine(), $e->getFile() === __FILE__, count($e->getTrace()), $e->getTrace()[0]['function']);
$o = (new ReflectionClass("SplFixedArray"))->newInstanceWithoutConstructor();
var_dump($o);
var_dump($o->getSize());
$o = (new ReflectionClass("ArrayObject"))->newInstanceWithoutConstructor();
var_dump(count($o));
$o = (new ReflectionClass("SplObjectStorage"))->newInstanceWithoutConstructor();
var_dump(count($o));
$o = (new ReflectionClass("SplDoublyLinkedList"))->newInstanceWithoutConstructor();
var_dump(count($o));
class MyE extends Exception {}
$e = (new ReflectionClass("MyE"))->newInstanceWithoutConstructor();
var_dump($e->getLine());
--EXPECT--
ReflectionException: Class WeakReference is an internal class marked as final that cannot be instantiated without invoking its constructor
ReflectionException: Class Generator is an internal class marked as final that cannot be instantiated without invoking its constructor
ReflectionException: Class Closure is an internal class marked as final that cannot be instantiated without invoking its constructor
ReflectionException: Class WeakMap is an internal class marked as final that cannot be instantiated without invoking its constructor
ReflectionException: Class Fiber is an internal class marked as final that cannot be instantiated without invoking its constructor
int(6)
bool(true)
int(2)
string(29) "newInstanceWithoutConstructor"
object(SplFixedArray)#2 (0) {
}
int(0)
int(0)
int(0)
int(0)
int(19)
