--TEST--
request_parse_body() over post_max_size parses the request body php://input buffered
--INI--
post_max_size=5
--POST--
a=1&b=2
--FILE--
<?php
// Over post_max_size nothing was buffered at startup: the first call has
// nothing to parse, php://input then buffers the body, which the second
// call (still over the limit, so keeping that buffer) parses.
foreach ([1, 2] as $i) {
    var_dump(request_parse_body()[0]);
    var_dump(file_get_contents('php://input'));
}
?>
--EXPECTF--
Warning: PHP Request Startup: POST Content-Length of 7 bytes exceeds the limit of 5 bytes in Unknown on line 0

Warning: request_parse_body(): POST Content-Length of 7 bytes exceeds the limit of 5 bytes in %s on line %d
array(0) {
}
string(7) "a=1&b=2"

Warning: request_parse_body(): POST Content-Length of 7 bytes exceeds the limit of 5 bytes in %s on line %d
array(2) {
  ["a"]=>
  string(1) "1"
  ["b"]=>
  string(1) "2"
}
string(7) "a=1&b=2"
