--TEST--
ReflectionParameter::isDefaultValueAvailable() is false for a variadic parameter (nette/di DependencyChecker in phpstan)
--FILE--
<?php
class B { function useTrait(...$traits) {} function f($a, int ...$b) {} }
foreach ([["B","useTrait",0], ["B","f",1]] as [$c,$m,$i]) { $p = (new ReflectionMethod($c, $m))->getParameters()[$i];
var_dump($p->isDefaultValueAvailable(), $p->isOptional(), $p->isVariadic());
try { var_dump($p->isDefaultValueConstant()); } catch (ReflectionException $e) { echo $e->getMessage(), "\n"; }
try { var_dump($p->getDefaultValue()); } catch (ReflectionException $e) { echo $e->getMessage(), "\n"; } }
function g($a, $b = 1, $c = PHP_INT_MAX, ...$d) {}
function h($x = null, $y = [1]) {} $f = "h";
foreach ([new ReflectionFunction("g"), new ReflectionFunction($f), new ReflectionMethod("ArrayObject", "__construct"), new ReflectionFunction("str_pad")] as $rf) foreach ($rf->getParameters() as $p) echo $rf->getName(), " ", $p->getName(), " ", var_export($p->isDefaultValueAvailable(), true), " ", var_export($p->isOptional(), true), "\n";

?>
--EXPECT--
bool(false)
bool(true)
bool(true)
Internal error: Failed to retrieve the default value
Internal error: Failed to retrieve the default value
bool(false)
bool(true)
bool(true)
Internal error: Failed to retrieve the default value
Internal error: Failed to retrieve the default value
g a false false
g b true true
g c true true
g d false true
h x true true
h y true true
__construct array true true
__construct flags true true
__construct iteratorClass true true
str_pad string false false
str_pad length false false
str_pad pad_string true true
str_pad pad_type true true
