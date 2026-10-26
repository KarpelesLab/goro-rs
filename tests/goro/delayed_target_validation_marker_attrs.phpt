--TEST--
#[\DelayedTargetValidation]: newInstance() of a marker attribute checks its target; get_class() in the dummy Closure scope
--FILE--
<?php
#[\DelayedTargetValidation]
#[\Override]
function f() {}
#[\DelayedTargetValidation] #[\NoDiscard] class X {}
#[\DelayedTargetValidation] #[\Attribute] abstract class AB {}
#[\DelayedTargetValidation] #[\Deprecated] class Y {}
f();
foreach ([(new ReflectionFunction('f'))->getAttributes(), (new ReflectionClass('X'))->getAttributes(), (new ReflectionClass('AB'))->getAttributes()] as $attrs)
foreach ($attrs as $a) { echo $a->getName(), ": "; try { var_dump($a->newInstance()); } catch (Throwable $e) { echo get_class($e), ': ', $e->getMessage(), "\n"; } }
var_dump(new DelayedTargetValidation);
$f = function() { return [self::class, static::class, get_class(), get_called_class()]; };
var_dump(Closure::bind($f, new stdClass, null)());
$g = function() { return self::class; };
var_dump(Closure::bind($g, null, "ArrayObject"));
?>
--EXPECTF--
DelayedTargetValidation: object(DelayedTargetValidation)#1 (0) {
}
Override: Error: Attribute "Override" cannot target function (allowed targets: method, property)
DelayedTargetValidation: object(DelayedTargetValidation)#8 (0) {
}
NoDiscard: Error: Attribute "NoDiscard" cannot target class (allowed targets: function, method)
DelayedTargetValidation: object(DelayedTargetValidation)#1 (0) {
}
Attribute: Error: Cannot apply #[\Attribute] to abstract class AB
object(DelayedTargetValidation)#5 (0) {
}

Deprecated: Calling get_class() without arguments is deprecated in %s on line 12
array(4) {
  [0]=>
  string(7) "Closure"
  [1]=>
  string(8) "stdClass"
  [2]=>
  string(7) "Closure"
  [3]=>
  string(8) "stdClass"
}

Warning: Cannot bind closure to scope of internal class ArrayObject, this will be an error in PHP 9 in %s on line 15
NULL
