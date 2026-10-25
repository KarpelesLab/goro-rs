--TEST--
Enum name/value in get_class_vars() and property_exists()
--FILE--
<?php
enum P { case X; }
enum E: string { case X = 'x'; }
var_dump(get_class_vars('P'), get_class_vars('E'));
var_dump(property_exists('P', 'name'), property_exists('P', 'value'), property_exists('E', 'value'), property_exists(E::X, 'name'));
--EXPECT--
array(1) {
  ["name"]=>
  NULL
}
array(2) {
  ["name"]=>
  NULL
  ["value"]=>
  NULL
}
bool(true)
bool(false)
bool(true)
bool(true)
