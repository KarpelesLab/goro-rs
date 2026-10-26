--TEST--
An unset typed property of an internal class (ReflectionClass::$name) is uninitialized, not undefined
--FILE--
<?php
class R extends ReflectionClass {}
$r = new R('stdClass');
unset($r->name);
try { var_dump($r->name); } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
$x = new ReflectionProperty('Exception', 'message');
unset($x->name);
try { var_dump($x->name); } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
$p = new SplFileInfo(__FILE__);
$d = new DateInterval('P1D');
unset($d->y);
try { var_dump($d->y); } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
--EXPECT--
Error: Typed property ReflectionClass::$name must not be accessed before initialization
Error: Typed property ReflectionProperty::$name must not be accessed before initialization
int(0)
