--TEST--
Request startup warnings are recorded for error_get_last(), and a displayed one stops $_POST parsing
--INI--
max_input_vars=3
display_errors=1
--GET--
a=1&b=2&c=3&d=4
--POST--
p=1&q=2
--FILE--
<?php
var_dump($_GET, $_POST, error_get_last());
var_dump(file_get_contents('php://input'));
?>
--EXPECT--
Warning: PHP Request Startup: Input variables exceeded 3. To increase the limit change max_input_vars in php.ini. in Unknown on line 0
array(3) {
  ["a"]=>
  string(1) "1"
  ["b"]=>
  string(1) "2"
  ["c"]=>
  string(1) "3"
}
array(0) {
}
array(4) {
  ["type"]=>
  int(2)
  ["message"]=>
  string(104) "PHP Request Startup: Input variables exceeded 3. To increase the limit change max_input_vars in php.ini."
  ["file"]=>
  string(7) "Unknown"
  ["line"]=>
  int(0)
}
string(7) "p=1&q=2"
