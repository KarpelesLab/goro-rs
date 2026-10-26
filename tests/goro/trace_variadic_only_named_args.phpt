--TEST--
A variadic function called with only extra named arguments shows them in backtraces
--FILE--
<?php
function t(...$a) { return (new Exception)->getTraceAsString(); }
echo t(...['a' => 1, 'b' => 2]), "\n";
echo t(a: 1, b: "two"), "\n";
function v(...$a) { var_dump((new Exception)->getTrace()[0]['args']); debug_print_backtrace(); }
v(a: 1);
--EXPECTF--
#0 %s(3): t(a: 1, b: 2)
#1 {main}
#0 %s(4): t(a: 1, b: 'two')
#1 {main}
array(1) {
  ["a"]=>
  int(1)
}
#0 %s(6): v(a: 1)
