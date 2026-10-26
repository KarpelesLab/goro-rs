--TEST--
getAttributes(): the flags error names the declaring class; stdClass is #[AllowDynamicProperties]
--FILE--
<?php
#[Attribute] class A {}
class C { #[A] public $p; #[A] const K = 1; #[A] function m(#[A] $x) {} }
enum E { #[A] case X; }
foreach ([new ReflectionClass("C"), new ReflectionProperty("C", "p"), new ReflectionClassConstant("C", "K"), new ReflectionMethod("C", "m"), new ReflectionParameter(["C", "m"], 0), new ReflectionFunction("strlen"), new ReflectionEnumUnitCase("E", "X"), new ReflectionObject(new C)] as $r) {
  try { $r->getAttributes(null, 5); } catch (ValueError $e) { echo $e->getMessage(), "\n"; }
}
var_dump((new ReflectionClass("stdClass"))->getAttributes());
var_dump(count((new ReflectionClass("Attribute"))->getAttributes()), count((new ReflectionClass("SensitiveParameter"))->getAttributes()));

$a = (new ReflectionClass("stdClass"))->getAttributes()[0]; var_dump($a->getName(), $a->getArguments(), $a->getTarget(), $a->isRepeated(), get_class($a->newInstance()));
--EXPECT--
ReflectionClass::getAttributes(): Argument #2 ($flags) must be a valid attribute filter flag
ReflectionProperty::getAttributes(): Argument #2 ($flags) must be a valid attribute filter flag
ReflectionClassConstant::getAttributes(): Argument #2 ($flags) must be a valid attribute filter flag
ReflectionFunctionAbstract::getAttributes(): Argument #2 ($flags) must be a valid attribute filter flag
ReflectionParameter::getAttributes(): Argument #2 ($flags) must be a valid attribute filter flag
ReflectionFunctionAbstract::getAttributes(): Argument #2 ($flags) must be a valid attribute filter flag
ReflectionClassConstant::getAttributes(): Argument #2 ($flags) must be a valid attribute filter flag
ReflectionClass::getAttributes(): Argument #2 ($flags) must be a valid attribute filter flag
array(1) {
  [0]=>
  object(ReflectionAttribute)#6 (1) {
    ["name"]=>
    string(22) "AllowDynamicProperties"
  }
}
int(1)
int(1)
string(22) "AllowDynamicProperties"
array(0) {
}
int(1)
bool(false)
string(22) "AllowDynamicProperties"
