--TEST--
Generator objects in class/object introspection builtins, callables, and conversions
--FILE--
<?php
function g() { yield 1; }
$t = function($name, $f) { try { $r = $f(); echo "$name: "; var_dump($r); } catch (Throwable $e) { echo "$name: ", get_class($e), ": ", $e->getMessage(), "\n"; } };
$gen = g();
$t('get_object_vars', fn() => get_object_vars($gen));
$t('get_mangled', fn() => get_mangled_object_vars($gen));
$t('method_exists', fn() => method_exists($gen, 'send'));
$t('method_exists2', fn() => method_exists($gen, 'nope'));
$t('property_exists', fn() => property_exists($gen, 'x'));
$t('is_a', fn() => is_a($gen, 'Iterator'));
$t('is_subclass_of', fn() => is_subclass_of($gen, 'Traversable'));
$t('get_parent_class', fn() => get_parent_class($gen));
$t('class_implements', fn() => class_implements($gen));
$t('get_class_methods', fn() => count(get_class_methods($gen)));
$t('call_user_func', fn() => call_user_func([$gen, 'current']));
$t('call_user_func_arr', fn() => call_user_func_array([g(), 'key'], []));
$t('class const', fn() => $gen::class);
$t('debugInfo', fn() => $gen->__debugInfo());
$t('property', fn() => $gen->x);
$t('strcast', fn() => (string)$gen);
$t('strval', fn() => strval($gen));
$t('settype', function() use ($gen) { $x = $gen; settype($x, 'string'); return $x; });
$t('intcast', fn() => (int)$gen);
$t('intval', fn() => intval($gen));
$t('eq int', fn() => $gen == 1);
$t('isset key', fn() => isset([1][$gen]));
--EXPECTF--
get_object_vars: array(0) {
}
get_mangled: array(0) {
}
method_exists: bool(true)
method_exists2: bool(false)
property_exists: bool(false)
is_a: bool(true)
is_subclass_of: bool(true)
get_parent_class: bool(false)
class_implements: array(2) {
  ["Iterator"]=>
  string(8) "Iterator"
  ["Traversable"]=>
  string(11) "Traversable"
}
get_class_methods: int(9)
call_user_func: int(1)
call_user_func_arr: int(0)
class const: string(9) "Generator"
debugInfo: array(1) {
  ["function"]=>
  string(1) "g"
}

Warning: Undefined property: Generator::$x in %s on line 19
property: NULL
strcast: Error: Object of class Generator could not be converted to string
strval: Error: Object of class Generator could not be converted to string
settype: Error: Object of class Generator could not be converted to string

Warning: Object of class Generator could not be converted to int in %s on line 23
intcast: int(1)

Warning: Object of class Generator could not be converted to int in %s on line 24
intval: int(1)

Notice: Object of class Generator could not be converted to int in %s on line 25
eq int: bool(true)
isset key: TypeError: Cannot access offset of type Generator in isset or empty
