--TEST--
php://filter with an empty chain reads the remainder as filter names
--FILE--
<?php
var_dump(file_get_contents("php://filter/resource=php://filter/read=string.rot13/resource=data:,abc"));
var_dump(file_get_contents("php://filter/read=string.toupper/resource=php://filter/read=string.rot13/resource=data:,abc"));
--EXPECTF--

Warning: file_get_contents(): Unable to locate filter "resource=php:" in %s on line 2

Warning: file_get_contents(): Unable to create filter (resource=php:) in %s on line 2

Warning: file_get_contents(): Unable to locate filter "filter" in %s on line 2

Warning: file_get_contents(): Unable to create filter (filter) in %s on line 2

Warning: file_get_contents(): Unable to locate filter "resource=data:,abc" in %s on line 2

Warning: file_get_contents(): Unable to create filter (resource=data:,abc) in %s on line 2
string(3) "abc"
string(3) "NOP"
