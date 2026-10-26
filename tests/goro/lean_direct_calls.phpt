--TEST--
Plain function calls made whole at their INIT_FCALL (lean_direct): by-value arrays, references, backtraces, exceptions, generators, by-reference returns, variadics, silenced warnings, destructors, namespace fallback, functions declared later, error handlers
--FILE--
<?php
namespace Ns {
    function callfoo($x) { return foo($x); }
    function callbar($x) { return bar($x); }
}
namespace {
function foo($x) { return "global foo $x"; }
function bar($x) { return "global bar $x"; }
function add($a, $b) { return $a + $b; }
function arrmod($a, $k) { $a[$k] = 'changed'; return count($a); }
function arrsame($a, $b) { $a[] = 1; return count($a) . count($b); }
function bt($a, $b) { return json_encode(array_map(fn($f) => [$f['function'], $f['args'] ?? null], debug_backtrace())); }
function thrower($n) { if ($n % 3 == 0) { throw new Exception("t$n"); } return $n; }
function gen($n) { yield $n; }
function &refret($n) { static $r; $r = $n; return $r; }
function vari(...$xs) { return count($xs); }
function warns($n) { return $n + $undefined; }
function inner_builtin($s) { return strtoupper($s) . strlen($s); }
class Dt { function __destruct() { echo "Dt destroyed\n"; } }
function takes($o, $n) { return $n * 2; }
function later_defined_caller($i) { return later($i); }

for ($i = 0; $i < 3; $i++) { echo add($i, 10), " "; }
echo "\n";
$arr = ['a' => 1, 'b' => 2];
for ($i = 0; $i < 2; $i++) { echo arrmod($arr, 'a'), " ", $arr['a'], " | "; }
echo "\n";
echo arrsame($arr, $arr), " ", count($arr), "\n";
echo arrmod(array_merge($arr, ['c' => 3]), 'c'), "\n";
$r = &$arr; echo arrmod($r, 'z'), " ", count($arr), "\n";
unset($r);
echo bt(1, [2]), "\n";
for ($i = 1; $i <= 4; $i++) {
    try { echo thrower($i), " "; } catch (Exception $e) { echo "caught ", $e->getMessage(), " line ", $e->getLine(), " "; }
}
echo "\n";
for ($i = 0; $i < 2; $i++) { foreach (gen($i) as $v) { echo "gen $v "; } }
echo "\n";
for ($i = 0; $i < 2; $i++) { $x = &refret($i); $x++; echo refret(5), " "; }
echo "\n";
for ($i = 0; $i < 2; $i++) { echo vari($i, 2, 3), " "; }
echo "\n";
for ($i = 0; $i < 2; $i++) { echo @warns($i), " "; }
echo "\n";
for ($i = 0; $i < 2; $i++) { echo warns($i), "\n"; }
for ($i = 0; $i < 2; $i++) { echo inner_builtin("ab$i"), " "; }
echo "\n";
for ($i = 0; $i < 2; $i++) { echo takes(new Dt, $i), "\n"; }
echo Ns\callfoo(1), "\n";
function_exists('Ns\foo') or eval('namespace Ns; function foo($x) { return "ns foo $x"; }');
echo Ns\callfoo(2), "\n";
eval('namespace Ns; function bar($x) { return "ns bar $x"; }');
echo Ns\callbar(3), "\n";
for ($i = 0; $i < 3; $i++) {
    try { echo later_defined_caller($i), "\n"; } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
    if ($i == 0) { eval('function later($i) { return "later $i"; }'); }
}
set_error_handler(function ($no, $str) { echo "handler: $str\n"; return true; });
for ($i = 0; $i < 2; $i++) { echo warns($i), "\n"; }
restore_error_handler();
function counter_rec($n) { return $n <= 0 ? 0 : 1 + counter_rec($n - 1); }
echo counter_rec(50), "\n";
$s = 0;
for ($i = 0; $i < 1000; $i++) { $s += add($i, $i); }
echo $s, "\n";
echo "end\n";
}
--EXPECTF--
10 11 12 
2 1 | 2 1 | 
32 2
3
3 2
[["bt",[1,[2]]]]
1 2 caught t3 line 13 4 
gen 0 gen 1 
5 5 
3 3 
0 1 

Warning: Undefined variable $undefined in %s on line 17
0

Warning: Undefined variable $undefined in %s on line 17
1
AB03 AB13 
Dt destroyed
0
Dt destroyed
2
global foo 1
global foo 2
ns bar 3
Error: Call to undefined function later()
later 1
later 2
handler: Undefined variable $undefined
0
handler: Undefined variable $undefined
1
50
999000
end
