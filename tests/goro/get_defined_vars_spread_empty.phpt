--TEST--
get_defined_vars(...[]) reads the calling scope
--FILE--
<?php
function f() { $x = 1; var_dump(get_defined_vars(...[])); var_dump(get_defined_vars()); $g = get_defined_vars(...); var_dump($g()); var_dump(func_get_args(...[])); var_dump(compact(...['x'])); }
f(5);
--EXPECTF--
array(1) {
  ["x"]=>
  int(1)
}
array(1) {
  ["x"]=>
  int(1)
}

Fatal error: Uncaught Error: Cannot call get_defined_vars() dynamically in %s:2
Stack trace:
#0 %s(2): get_defined_vars()
#1 %s(3): f(5)
#2 {main}
  thrown in %s on line 2
