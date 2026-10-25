--TEST--
Builtin calls of nested lean calls
--FILE--
<?php
function lvl3($s) { return str_repeat($s, 2) . json_encode(array_map(fn($f) => $f['function'], debug_backtrace())); }
function lvl2($s) { return lvl3(strtoupper($s)); }
function lvl1($s) { return lvl2($s) . "!"; }
function thr3($n) { return str_repeat("x", $n); }
function thr2($n) { return thr3($n); }
function thr1($n) { return thr2($n) . "?"; }
class C { public $v = "c"; function a($x) { return $this->b($x); } function b($x) { return strtolower($x) . $this->v . count(debug_backtrace()); } }
for ($i = 0; $i < 3; $i++) {
    echo lvl1("ab"), "\n";
    try { echo thr1(-1), "\n"; } catch (ValueError $e) { echo $e->getMessage(), " ", $e->getLine(), "\n", $e->getTraceAsString(), "\n"; }
    echo (new C)->a("XY"), "\n";
}
set_error_handler(function ($n, $s) { echo "W: $s ", count(debug_backtrace()), "\n"; return true; });
function w2($a) { return array_combine([1], [1, 2]); }
function w1($a) { $r = w2($a); return $r; }
var_dump(w1(1));
--EXPECTF--
ABAB["lvl3","lvl2","lvl1"]!
str_repeat(): Argument #2 ($times) must be greater than or equal to 0 5
#0 %s(5): str_repeat('x', -1)
#1 %s(6): thr3(-1)
#2 %s(7): thr2(-1)
#3 %s(11): thr1(-1)
#4 {main}
xyc2
ABAB["lvl3","lvl2","lvl1"]!
str_repeat(): Argument #2 ($times) must be greater than or equal to 0 5
#0 %s(5): str_repeat('x', -1)
#1 %s(6): thr3(-1)
#2 %s(7): thr2(-1)
#3 %s(11): thr1(-1)
#4 {main}
xyc2
ABAB["lvl3","lvl2","lvl1"]!
str_repeat(): Argument #2 ($times) must be greater than or equal to 0 5
#0 %s(5): str_repeat('x', -1)
#1 %s(6): thr3(-1)
#2 %s(7): thr2(-1)
#3 %s(11): thr1(-1)
#4 {main}
xyc2

Fatal error: Uncaught ValueError: array_combine(): Argument #1 ($keys) and argument #2 ($values) must have the same number of elements in %s:15
Stack trace:
#0 %s(15): array_combine(Array, Array)
#1 %s(16): w2(1)
#2 %s(17): w1(1)
#3 {main}
  thrown in %s on line 15
