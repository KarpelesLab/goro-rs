--TEST--
ReflectionClass static property access, uninitialized typed statics, static defaults
--FILE--
<?php
class S { public static $a = 1; protected static $b = 2; private static $c = 3; public static int $typed; public $inst = 5; }
class S2 extends S { public static $a = 10; }
S::$a = 100;
$r = new ReflectionClass('S2');
var_dump($r->getStaticProperties());
var_dump((new ReflectionClass('S'))->getStaticProperties());
try { var_dump($r->getStaticPropertyValue('c')); } catch (Exception $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
var_dump($r->getStaticPropertyValue('c', 'dflt'));
var_dump($r->getStaticPropertyValue('b'));
var_dump((new ReflectionClass('S'))->getStaticPropertyValue('c'));
try { $r->setStaticPropertyValue('typed', 'str'); } catch (TypeError $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { $r->setStaticPropertyValue('c', 1); } catch (Exception $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { $r->getStaticPropertyValue('typed'); } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
var_dump($r->getDefaultProperties());
var_dump((new ReflectionClass('S'))->getDefaultProperties());
S2::$a = 77;
var_dump($r->getDefaultProperties()['a']);
var_dump((new ReflectionProperty('S', 'a'))->getDefaultValue());
--EXPECT--
array(2) {
  ["a"]=>
  int(10)
  ["b"]=>
  int(2)
}
array(3) {
  ["a"]=>
  int(100)
  ["b"]=>
  int(2)
  ["c"]=>
  int(3)
}
ReflectionException: Property S2::$c does not exist
string(4) "dflt"
int(2)
int(3)
TypeError: Cannot assign string to property S::$typed of type int
ReflectionException: Class S2 does not have a property named c
Error: Typed property S2::$typed must not be accessed before initialization
array(3) {
  ["a"]=>
  int(10)
  ["b"]=>
  int(2)
  ["inst"]=>
  int(5)
}
array(4) {
  ["a"]=>
  int(1)
  ["b"]=>
  int(2)
  ["c"]=>
  int(3)
  ["inst"]=>
  int(5)
}
int(10)
int(1)
