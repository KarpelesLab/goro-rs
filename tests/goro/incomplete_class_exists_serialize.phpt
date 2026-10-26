--TEST--
__PHP_Incomplete_Class: property_exists() warns and is false, method_exists() throws, serialize() counts the magic member
--FILE--
<?php
function t($f) { try { $f(); } catch (\Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; } }
$x = unserialize('O:3:"Foo":1:{s:1:"a";i:1;}');
t(fn() => var_dump(property_exists($x, 'a')));
t(fn() => var_dump(method_exists($x, 'foo')));
t(fn() => var_dump(method_exists($x, '__construct')));
var_dump(serialize(unserialize('O:22:"__PHP_Incomplete_Class":1:{s:1:"q";i:5;}')));
var_dump(serialize(unserialize('O:22:"__PHP_Incomplete_Class":0:{}')));
var_dump(serialize(new __PHP_Incomplete_Class));
var_dump(serialize($x));
--EXPECTF--

Warning: property_exists(): The script tried to access a property on an incomplete object. Please ensure that the class definition "Foo" of the object you are trying to operate on was loaded _before_ unserialize() gets called or provide an autoloader to load the class definition in %s on line %d
bool(false)
Error: The script tried to call a method on an incomplete object. Please ensure that the class definition "Foo" of the object you are trying to operate on was loaded _before_ unserialize() gets called or provide an autoloader to load the class definition
Error: The script tried to call a method on an incomplete object. Please ensure that the class definition "Foo" of the object you are trying to operate on was loaded _before_ unserialize() gets called or provide an autoloader to load the class definition
string(34) "O:22:"__PHP_Incomplete_Class":0:{}"
string(34) "O:22:"__PHP_Incomplete_Class":0:{}"
string(34) "O:22:"__PHP_Incomplete_Class":0:{}"
string(26) "O:3:"Foo":1:{s:1:"a";i:1;}"
