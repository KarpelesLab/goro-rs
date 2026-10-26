--TEST--
A |> call is on the line of its callable
--FILE--
<?php
function t($x) { return debug_backtrace()[0]['line']; }
$r = 1
  |> t(...)
  |> t(...);
var_dump($r);
function u($x) { echo debug_backtrace()[0]['line'], "\n"; return $x; }
$r = 1
  |> u(...)
  |> u(...);
--EXPECT--
int(5)
9
10
