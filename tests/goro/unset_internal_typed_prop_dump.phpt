--TEST--
An unset typed property of ReflectionClass stays an uninitialized slot (var_dump, isset)
--FILE--
<?php
$r = new ReflectionClass('stdClass');
unset($r->name);
try { var_dump($r->name); } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
var_dump(isset($r->name));
var_dump($r);
$m = new ReflectionMethod('ArrayObject', 'count');
unset($m->class);
try { var_dump($m->class); } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { $r->name = 'x'; } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
$e = new Exception("m");
unset($e->message);
var_dump($e->getMessage());
--EXPECTF--
Error: Typed property ReflectionClass::$name must not be accessed before initialization
bool(false)
object(ReflectionClass)#1 (0) {
  ["name"]=>
  uninitialized(string)
}
Error: Typed property ReflectionMethod::$class must not be accessed before initialization

Fatal error: Uncaught ReflectionException: Cannot set read-only property ReflectionClass::$name in %s:10
Stack trace:
#0 {main}
  thrown in %s on line 10
