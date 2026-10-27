--TEST--
is_a() / is_subclass_of() / instanceof / Reflection with a class_alias() of a parent or interface
--FILE--
<?php
interface BarInterface {}
interface Sub extends BarInterface {}
class Bar implements BarInterface {}
class Baz implements Sub {}
class P {}
class Q extends P {}
class_alias('BarInterface', 'AliasI');
class_alias('P', 'AliasP');
var_dump(is_subclass_of('Bar', 'AliasI'), is_subclass_of('Baz', 'AliasI'), is_a('Bar', 'AliasI', true));
var_dump(is_subclass_of('Q', 'AliasP'), is_subclass_of(new Bar, 'AliasI'), new Baz instanceof AliasI);
var_dump(is_subclass_of('AliasP', 'P'), is_subclass_of('Sub', 'AliasI'));
var_dump((new ReflectionClass('Baz'))->implementsInterface('AliasI'), (new ReflectionClass('Q'))->isSubclassOf('AliasP'));
var_dump(class_implements('Baz'));

?>
--EXPECT--
bool(true)
bool(true)
bool(true)
bool(true)
bool(true)
bool(true)
bool(false)
bool(true)
bool(true)
bool(true)
array(2) {
  ["Sub"]=>
  string(3) "Sub"
  ["BarInterface"]=>
  string(12) "BarInterface"
}
