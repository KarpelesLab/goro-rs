--TEST--
Extending a class by another case or an alias, and an alias-typed parameter given the aliased class itself
--FILE--
<?php
class TC {}
interface I {}
class_alias('TC', 'X_TC');
class_alias('I', 'X_I');
class T extends X_TC implements X_I {}
class U extends tc {}
function f(TC $x) { echo get_class($x), "\n"; }
function g(I $x) { echo get_class($x), "\n"; }
function h(X_TC $x) { echo get_class($x), "\n"; }
function k(X_I $x): X_I { return $x; }
var_dump(new T instanceof TC, is_subclass_of(new T, 'TC'), get_parent_class(new T), get_parent_class(new U));
var_dump(class_implements(new T), (new ReflectionClass('T'))->getParentClass()->name);
f(new T);
g(new T);
h(new T);
h(new U);
h(new TC);
echo get_class(k(new T)), "\n";
var_dump(new T instanceof X_TC, new TC instanceof X_TC, new T instanceof X_I, is_a(new T, 'X_TC'), is_subclass_of(new T, 'X_I'));

?>
--EXPECT--
bool(true)
bool(true)
string(2) "TC"
string(2) "TC"
array(1) {
  ["I"]=>
  string(1) "I"
}
string(2) "TC"
T
T
T
U
TC
T
bool(true)
bool(true)
bool(true)
bool(true)
bool(true)
