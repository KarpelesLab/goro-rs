--TEST--
new InternalIterator: private constructor of an internal class
--FILE--
<?php
try { new InternalIterator; } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { $r = new ReflectionClass('InternalIterator'); $r->newInstanceWithoutConstructor(); } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { $r->newInstance(); } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
$d = new DatePeriod(new DateTime('2020-01-01'), new DateInterval('P1D'), 2);
$ii = $d->getIterator(); var_dump(get_class($ii)); var_dump($ii->valid(), $ii->key()); $ii->next(); var_dump($ii->key());
try { clone $ii; } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { serialize($ii); } catch (Exception $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
var_dump((new ReflectionClass('InternalIterator'))->isFinal(), (new ReflectionClass('InternalIterator'))->isInstantiable());
--EXPECTF--
Error: Call to private InternalIterator::__construct() from global scope

Fatal error: Uncaught ReflectionException: Class InternalIterator is an internal class marked as final that cannot be instantiated without invoking its constructor in %s:3
Stack trace:
#0 %s(3): ReflectionClass->newInstanceWithoutConstructor()
#1 {main}
  thrown in %s on line 3
