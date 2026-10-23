--TEST--
Calls of plain functions run without a frame of their own until they need one: backtraces, exceptions, func_get_args(), copies of array arguments, references, defaults, releases
--FILE--
<?php
function add($a, $b) { return $a + $b; }
function bt($a, $b) { $a = $a * 2; $c = $a + 1; return debug_backtrace(); }
function inner($x) { $y = $x + 1; throw new Exception("boom $y"); }
function outer($x) { $z = $x * 3; return inner($z); }
function fga($a, $b = 5) { $a++; return func_get_args(); }
function deep1($n) { $m = $n + 1; return deep2($m); }
function deep2($n) { $k = $n * 2; return strlen("abc") + $k . strtoupper("x"); }
function fib($n) { if ($n < 2) { return $n; } return fib($n - 1) + fib($n - 2); }
function modarr($a) { $a[] = 99; return count($a); }
function byref(&$a) { $a = $a + 1; return $a; }
function defs($a, $b = 10, $c = null) { return $a + $b; }
function retres($r) { return 1; }
function gen() { try { yield 1; } finally { echo "gen finally\n"; } }
function takegen($g) { return 2; }
class D { public $n; function __construct($n) { $this->n = $n; } function __destruct() { echo "D{$this->n} destroyed\n"; } }
function taked($d) { return 3; }
function inloop($arr) { foreach ($arr as $k => $v) { if ($v == 3) { return $k; } } return -1; }
function undef($a) { $b = $a + 1; return $q; }
function warnline($a) {
    $b = $a + 1;
    $c = $b * 2;
    return $c . $undefinedvar;
}
function retarr($n) { $x = [1, 2, $n]; return $x; }
function nested($a) { return add($a, add($a, 1)); }
function nr($x) { $y = $x; }

echo add(1, 2), "\n";
$t = bt(3, 4);
echo $t[0]['function'], " ", json_encode($t[0]['args']), " line ", $t[0]['line'], "\n";
try { outer(2); } catch (Exception $e) {
    echo $e->getMessage(), " ", $e->getLine(), "\n";
    foreach ($e->getTrace() as $f) { echo "  ", $f['function'], "(", json_encode($f['args'] ?? null), ") @", $f['line'], "\n"; }
}
var_dump(fga(1));
var_dump(fga(1, 2));
echo deep1(5), "\n";
echo fib(15), "\n";
$arr = [1, 2, 3];
echo modarr($arr), " ", count($arr), "\n";
$v = 5; echo byref($v), " ", $v, "\n";
echo defs(1), " ", defs(1, 2), "\n";
echo retres(fopen('php://memory', 'r')), " ", count(get_resources()), "\n";
echo takegen(gen()), "\n";
echo "after takegen\n";
echo taked(new D(1)), "\n";
echo "after taked\n";
echo inloop([5, 4, 3, 2]), "\n";
echo var_export(undef(1), true), "\n";
echo warnline(2), "\n";
var_dump(retarr(7));
echo nested(3), "\n";
var_dump(nr(1));
$s = 0;
for ($i = 0; $i < 1000; $i++) { $s = add($s, $i); }
echo $s, "\n";
function cnt() { static $n = 0; return ++$n; }
cnt(); cnt(); echo cnt(), "\n";
function gl() { global $s; return $s + 1; }
echo gl(), "\n";
function sets() { $GLOBALS['zz'] = 4; return 1; }
sets(); echo $zz, "\n";
function rec_bt($n) { if ($n == 0) { return count(debug_backtrace()); } $m = $n - 1; return rec_bt($m); }
echo rec_bt(5), "\n";
function strict_arg($a) { return $a . "x"; }
echo strict_arg("a"), strict_arg(1), "\n";
function obj_ret() { $o = new stdClass; $o->a = 1; return $o; }
var_dump(obj_ret());
function arr_unused() { return [1, 2]; }
arr_unused();
function chain($a) { $b = $a; $c = [$b, $a]; return $c[1]; }
echo chain(9), "\n";
echo "done\n";
--EXPECTF--
3
bt [6,4] line 30
boom 7 4
  inner([6]) @5
  outer([2]) @32
array(1) {
  [0]=>
  int(2)
}
array(2) {
  [0]=>
  int(2)
  [1]=>
  int(2)
}
15X
610
4 3
6 6
11 3
1 4
2
after takegen
D1 destroyed
3
after taked
2

Warning: Undefined variable $q in %s on line 19
NULL

Warning: Undefined variable $undefinedvar in %s on line 23
6
array(3) {
  [0]=>
  int(1)
  [1]=>
  int(2)
  [2]=>
  int(7)
}
7
NULL
499500
3
499501
4
6
ax1x
object(stdClass)#2 (1) {
  ["a"]=>
  int(1)
}
9
done
