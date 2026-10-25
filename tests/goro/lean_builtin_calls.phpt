--TEST--
Builtin calls of functions called without a frame of their own
--FILE--
<?php
function low($s) { return strtolower($s); }
function bt($a) { return count(debug_backtrace()) . ":" . debug_backtrace()[0]['function'] . ":" . json_encode(debug_backtrace()[0]['args'] ?? null); }
function thr($s) { return str_repeat($s, -1); }
function args($a, $b) { return implode(",", func_get_args()); }
function warn($a) { return array_combine([1, 2], [1]); }
class K { public $p = 1; function m($x) { return strtoupper($x) . $this->p; } }
set_error_handler(function ($no, $str, $file, $line) {
    $t = debug_backtrace();
    echo "handler: $str at line $line; frames: ";
    foreach ($t as $f) echo ($f['function'] ?? '?'), "(", json_encode($f['args'] ?? []), ") ";
    echo "\n";
    return true;
});
for ($i = 0; $i < 3; $i++) {
    echo low("ABC"), "\n";
    echo bt(5), "\n";
    try { echo thr("x"), "\n"; } catch (ValueError $e) { echo get_class($e), ": ", $e->getMessage(), " @", $e->getLine(), "\n"; echo $e->getTraceAsString(), "\n"; }
    echo args(1, 2, 3), "\n";
    echo (new K)->m("q"), "\n";
    var_dump(@warn(1));
    var_dump(warn(2));
}
function cb($v) { return low($v) . strlen($v); }
print_r(array_map('cb', ["AA", "Bb"]));
function outer($x) { return inner($x) . "!"; }
function inner($x) { return trim($x); }
echo outer("  hi  "), "\n";
class D { public $n; function __construct($n) { $this->n = $n; } function __destruct() { echo "dtor {$this->n}\n"; } }
function mk($n) { $d = new D($n); return strlen("x"); }
echo mk(1), "\n";
echo "after mk\n";
function ex($o) { $o = null; return strlen(str_repeat("a", 3)); }
echo ex(new D(2)), "\n";
function thr2($a) { throw new Exception("boom"); }
function viaArr($a) { return array_map('thr2', [1]); }
try { viaArr(1); } catch (Exception $e) { echo "caught ", $e->getMessage(), " ", count($e->getTrace()), "\n"; }
function strictf($x) { return strlen($x); }
try { echo strictf([]), "\n"; } catch (TypeError $e) { echo "TE ", $e->getMessage(), " ", $e->getTrace()[0]['function'], " ", $e->getTrace()[1]['function'] ?? '-', "\n"; }
--EXPECTF--
abc
1:bt:[5]
ValueError: str_repeat(): Argument #2 ($times) must be greater than or equal to 0 @4
#0 %s(4): str_repeat('x', -1)
#1 %s(18): thr('x')
#2 {main}
1,2,3
Q1

Fatal error: Uncaught ValueError: array_combine(): Argument #1 ($keys) and argument #2 ($values) must have the same number of elements in %s:6
Stack trace:
#0 %s(6): array_combine(Array, Array)
#1 %s(21): warn(1)
#2 {main}
  thrown in %s on line 6
