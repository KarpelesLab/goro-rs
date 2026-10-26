--TEST--
Reflection methods: argument counts from their stubs, lazy-object and ReflectionClass|string argument types
--FILE--
<?php
class P { public $a = 1; }
$rc = new ReflectionClass("stdClass");
$rp = new ReflectionProperty("P", "a");
$rm = new ReflectionMethod("Exception", "getMessage");
$rf = new ReflectionFunction("strlen");
foreach ([
  fn() => $rc->getName(1), fn() => $rc->hasMethod(), fn() => $rc->getLazyInitializer(null), fn() => $rc->initializeLazyObject([]),
  fn() => $rc->isUninitializedLazyObject("x"), fn() => $rc->markLazyObjectAsInitialized(new ArrayObject), fn() => $rc->implementsInterface([]),
  fn() => $rc->isSubclassOf([]), fn() => $rp->isInitialized([]), fn() => $rp->isInitialized(1, 2), fn() => $rp->isLazy(null),
  fn() => $rp->setValue(), fn() => $rp->setValue("a", 1), fn() => $rp->setValue(new P, 1, 2), fn() => $rp->getName(1),
  fn() => $rm->getClosure(1, 2), fn() => $rm->getName([]), fn() => $rf->getNumberOfParameters(1), fn() => $rf->isDisabled(1),
] as $i => $f) {
  echo "$i: "; try { var_dump($f()); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
}
--EXPECTF--
0: ArgumentCountError: ReflectionClass::getName() expects exactly 0 arguments, 1 given
1: ArgumentCountError: ReflectionClass::hasMethod() expects exactly 1 argument, 0 given
2: TypeError: ReflectionClass::getLazyInitializer(): Argument #1 ($object) must be of type stdClass, null given
3: TypeError: ReflectionClass::initializeLazyObject(): Argument #1 ($object) must be of type stdClass, array given
4: TypeError: ReflectionClass::isUninitializedLazyObject(): Argument #1 ($object) must be of type stdClass, string given
5: TypeError: ReflectionClass::markLazyObjectAsInitialized(): Argument #1 ($object) must be of type stdClass, ArrayObject given
6: TypeError: ReflectionClass::implementsInterface(): Argument #1 ($interface) must be of type ReflectionClass|string, array given
7: TypeError: ReflectionClass::isSubclassOf(): Argument #1 ($class) must be of type ReflectionClass|string, array given
8: TypeError: ReflectionProperty::isInitialized(): Argument #1 ($object) must be of type ?object, array given
9: ArgumentCountError: ReflectionProperty::isInitialized() expects at most 1 argument, 2 given
10: TypeError: ReflectionProperty::isLazy(): Argument #1 ($object) must be of type P, null given
11: ArgumentCountError: ReflectionProperty::setValue() expects exactly 2 arguments, 0 given
12: TypeError: ReflectionProperty::setValue(): Argument #1 ($objectOrValue) must be of type object, string given
13: ArgumentCountError: ReflectionProperty::setValue() expects exactly 2 arguments, 3 given
14: ArgumentCountError: ReflectionProperty::getName() expects exactly 0 arguments, 1 given
15: ArgumentCountError: ReflectionMethod::getClosure() expects at most 1 argument, 2 given
16: ArgumentCountError: ReflectionFunctionAbstract::getName() expects exactly 0 arguments, 1 given
17: ArgumentCountError: ReflectionFunctionAbstract::getNumberOfParameters() expects exactly 0 arguments, 1 given
18: 
Deprecated: Method ReflectionFunction::isDisabled() is deprecated since 8.0, as ReflectionFunction can no longer be constructed for disabled functions in %s on line 12
ArgumentCountError: ReflectionFunction::isDisabled() expects exactly 0 arguments, 1 given
