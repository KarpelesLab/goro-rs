--TEST--
By-reference send diagnostics and array elements on the argument's / element's line
--FILE--
<?php
$x = [
  "a" => 1,
  STDERR => 2,
  3,
  [] ];
function t(&$r) {}
t(
  strlen("x")
);
array_walk(
  get_object_vars(new stdClass),
  "t"
);
$y = array(1,
  1.5 => 2,
  true => 3);
--EXPECTF--

Warning: Resource ID#3 used as offset, casting to integer (3) in %s on line 4

Fatal error: Uncaught Error: t(): Argument #1 ($r) could not be passed by reference in %s:9
Stack trace:
#0 {main}
  thrown in %s on line 9
