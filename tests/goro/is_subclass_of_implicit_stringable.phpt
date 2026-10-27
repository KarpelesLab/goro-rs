--TEST--
is_subclass_of() / is_a(): a class inheriting __toString() implements Stringable
--FILE--
<?php
class P { function __toString(): string { return ''; } }
class Q extends P {}
class R extends Exception {}
var_dump(is_subclass_of('P', 'Stringable'), is_subclass_of('Q', 'Stringable'), is_subclass_of('R', 'Stringable'));
var_dump(is_a('Q', 'Stringable', true), (new Q) instanceof Stringable, is_subclass_of(new Q, 'Stringable'));
var_dump((new ReflectionClass('Q'))->implementsInterface('Stringable'), (new ReflectionClass('Q'))->getInterfaceNames());
var_dump(in_array('Stringable', class_implements(new Q)));

?>
--EXPECT--
bool(true)
bool(true)
bool(true)
bool(true)
bool(true)
bool(true)
bool(true)
array(1) {
  [0]=>
  string(10) "Stringable"
}
bool(true)
