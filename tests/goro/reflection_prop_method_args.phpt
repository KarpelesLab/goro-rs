--TEST--
ReflectionProperty getValue/getRawValue/setValue and ReflectionMethod::getClosure argument checks
--FILE--
<?php
class P { public $a = 1; public static $s = 2; }
class Q extends P {}
$r = new ReflectionProperty("P", "a");
$rs = new ReflectionProperty("P", "s");
foreach ([
  fn() => $r->getValue(new stdClass), fn() => $r->getValue([]), fn() => $r->getValue(), fn() => $r->getValue(new Q),
  fn() => $r->getRawValue(new stdClass), fn() => $r->getRawValue(new Q), fn() => $r->setRawValue(new stdClass, 1),
  fn() => $r->setValue(new stdClass, 1), fn() => $r->isInitialized(new stdClass), fn() => $r->isInitialized(),
  fn() => $rs->getValue(), fn() => $rs->getValue(new stdClass), fn() => $rs->getRawValue(new P), fn() => $rs->isInitialized(),
  fn() => $r->setValue(5), fn() => $rs->setValue(5), fn() => $rs->getValue(),
  fn() => (new ReflectionMethod("P", "nope")),
  fn() => (new ReflectionMethod("Exception", "getMessage"))->getClosure([]),
  fn() => (new ReflectionMethod("Exception", "getMessage"))->getClosure("x"),
  fn() => (new ReflectionMethod("Exception", "getMessage"))->getClosure(new stdClass),
  fn() => (new ReflectionMethod("Exception", "getMessage"))->getClosure(),
  fn() => (new ReflectionMethod("Exception", "getMessage"))->invoke(new stdClass),
  fn() => (new ReflectionMethod("Exception", "getMessage"))->invoke(null),
] as $i => $f) {
  echo "$i: "; try { var_dump($f()); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
}
--EXPECTF--
0: ReflectionException: Given object is not an instance of the class this property was declared in
1: TypeError: ReflectionProperty::getValue(): Argument #1 ($object) must be of type ?object, array given
2: TypeError: ReflectionProperty::getValue(): Argument #1 ($object) must be provided for instance properties
3: int(1)
4: ReflectionException: Given object is not an instance of the class this property was declared in
5: int(1)
6: NULL
7: NULL
8: ReflectionException: Given object is not an instance of the class this property was declared in
9: TypeError: ReflectionProperty::isInitialized(): Argument #1 ($object) must be provided for instance properties
10: int(2)
11: int(2)
12: ReflectionException: May not use getRawValue on static properties
13: bool(true)
14: ArgumentCountError: ReflectionProperty::setValue() expects exactly 2 arguments, 1 given
15: 
Deprecated: Calling ReflectionProperty::setValue() with a single argument is deprecated in %s on line 11
NULL
16: int(5)
17: ReflectionException: Method P::nope() does not exist
18: TypeError: ReflectionMethod::getClosure(): Argument #1 ($object) must be of type ?object, array given
19: TypeError: ReflectionMethod::getClosure(): Argument #1 ($object) must be of type ?object, string given
20: ReflectionException: Given object is not an instance of the class this method was declared in
21: ValueError: ReflectionMethod::getClosure(): Argument #1 ($object) cannot be null for non-static methods
22: ReflectionException: Given object is not an instance of the class this method was declared in
23: ReflectionException: Trying to invoke non static method Exception::getMessage() without an object
