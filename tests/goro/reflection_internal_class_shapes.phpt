--TEST--
Internal class shapes: BcMath\Number's virtual properties, enum Pcntl\QosClass, no debug-only Dom\HTMLDocument::debugGetTemplateCount()
--EXTENSIONS--
bcmath
pcntl
dom
--FILE--
<?php
$p = new ReflectionProperty('BcMath\Number', 'value');
var_dump($p->isVirtual(), Reflection::getModifierNames($p->getModifiers()));
var_dump(Pcntl\QosClass::Default, count(Pcntl\QosClass::cases()), (new ReflectionEnum('Pcntl\QosClass'))->getExtensionName());
var_dump(method_exists('Dom\HTMLDocument', 'debugGetTemplateCount'));
?>
--EXPECT--
bool(true)
array(4) {
  [0]=>
  string(7) "virtual"
  [1]=>
  string(6) "public"
  [2]=>
  string(14) "protected(set)"
  [3]=>
  string(8) "readonly"
}
enum(Pcntl\QosClass::Default)
int(5)
string(5) "pcntl"
bool(false)
