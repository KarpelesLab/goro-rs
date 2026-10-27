--TEST--
Nested yield from whose delegate starts with a yield from of an empty generator yields no phantom value
--FILE--
<?php
function e() { return; yield; }
function t1() { yield from e(); yield 'a' => 1; }
function o1() { yield from t1(); }
echo "o1\n"; foreach (o1() as $k => $v) echo var_export($k, true), " => $v\n";
function t2() { foreach ([1] as $_) { yield from e(); yield 'a' => 1; } }
function o2() { yield from t2(); }
echo "o2\n"; foreach (o2() as $k => $v) echo var_export($k, true), " => $v\n";

?>
--EXPECT--
o1
'a' => 1
o2
'a' => 1
