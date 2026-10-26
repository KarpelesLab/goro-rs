--TEST--
php://filter: a resource that cannot be opened makes the URL fail with "operation failed" (the inner open does not report)
--FILE--
<?php
var_dump(fopen("php://filter/read=string.toupper/resource=/nonexist/x", "r"));
var_dump(file_get_contents("php://filter/read=string.toupper/resource=/nonexist/x"));
var_dump(file("php://filter/resource=/nonexist/x"));
var_dump(fopen("php://filter/read=bogus/resource=/nonexist/x", "r"));
var_dump(file_get_contents("php://filter/read=string.toupper/resource=data:,abc"));
?>
--EXPECTF--
Warning: fopen(php://filter/read=string.toupper/resource=/nonexist/x): Failed to open stream: operation failed in %s on line %d
bool(false)

Warning: file_get_contents(php://filter/read=string.toupper/resource=/nonexist/x): Failed to open stream: operation failed in %s on line %d
bool(false)

Warning: file(php://filter/resource=/nonexist/x): Failed to open stream: operation failed in %s on line %d
bool(false)

Warning: fopen(php://filter/read=bogus/resource=/nonexist/x): Failed to open stream: operation failed in %s on line %d
bool(false)
string(3) "ABC"
