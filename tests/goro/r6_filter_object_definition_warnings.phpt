--TEST--
filter definitions that are objects warn when read as int
--FILE--
<?php
// zval_get_long() of definitions that are objects warns (twice for a
// filter_var_array() element: the ID check, then php_filter_call()).
$data = ['a' => '5'];
var_dump(filter_var_array($data, ['a' => new stdClass]));
var_dump(filter_var_array($data, ['a' => ['filter' => new stdClass]]));
var_dump(filter_var_array($data, ['a' => ['filter' => FILTER_VALIDATE_INT, 'flags' => new stdClass]]));
var_dump(filter_var('5', FILTER_VALIDATE_INT, ['flags' => new stdClass]));
var_dump(filter_input(INPUT_GET, 'x', FILTER_VALIDATE_INT, ['flags' => new stdClass]));
--EXPECTF--

Warning: Object of class stdClass could not be converted to int in %s on line 5

Warning: filter_var_array(): Unknown filter with ID 1 in %s on line 5

Warning: Object of class stdClass could not be converted to int in %s on line 5
array(1) {
  ["a"]=>
  string(1) "5"
}

Warning: Object of class stdClass could not be converted to int in %s on line 6
array(1) {
  ["a"]=>
  string(1) "5"
}

Warning: Object of class stdClass could not be converted to int in %s on line 7
array(1) {
  ["a"]=>
  int(5)
}

Warning: Object of class stdClass could not be converted to int in %s on line 8
int(5)

Warning: Object of class stdClass could not be converted to int in %s on line 9
NULL
