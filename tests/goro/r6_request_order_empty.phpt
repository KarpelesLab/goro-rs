--TEST--
An empty request_order leaves $_REQUEST empty; an empty variables_order keeps EGPCS
--INI--
request_order=
variables_order=
--GET--
g=1&a=get
--COOKIE--
c=1; a=cookie
--FILE--
<?php
var_dump(ini_get('variables_order'), ini_get('request_order'));
var_dump($_GET, $_COOKIE, $_REQUEST);
?>
--EXPECT--
string(5) "EGPCS"
string(0) ""
array(2) {
  ["g"]=>
  string(1) "1"
  ["a"]=>
  string(3) "get"
}
array(2) {
  ["c"]=>
  string(1) "1"
  ["a"]=>
  string(6) "cookie"
}
array(0) {
}
