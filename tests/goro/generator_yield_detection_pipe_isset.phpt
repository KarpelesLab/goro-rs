--TEST--
yield inside a pipe, isset(), empty(), @ makes the function a generator
--FILE--
<?php
function g5() { $v = (yield 1) |> strval(...); return $v; }
var_dump(g5() instanceof Generator);
function g6() { return 1 |> (yield); }
var_dump(g6() instanceof Generator);
function g7() { $v = (yield) |> (fn($x) => $x); }
var_dump(g7() instanceof Generator);
function g8() { $a = []; return isset($a[yield]); }
var_dump(g8() instanceof Generator);
function g9() { return @(yield); }
var_dump(g9() instanceof Generator);
function g10() { return empty($a[yield]); }
var_dump(g10() instanceof Generator);
$g = g8(); $g->current(); $g->send(1); var_dump($g->getReturn());
--EXPECTF--
bool(true)
bool(true)
bool(true)
bool(true)
bool(true)
bool(true)
bool(false)
