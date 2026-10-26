--TEST--
Callbacks builtins call, run without a frame of their own while they may
--INI--
zend.exception_ignore_args=0
--FILE--
<?php
// Callbacks a builtin calls, run without a frame of their own while they
// may be: what they see of their frame, their arguments and their scope.
function plain($x) { return $x * 2; }
function bt($x) { $t = debug_backtrace(); return count($t) . ':' . $t[0]['function'] . ':' . json_encode($t[0]['args']) . ':' . ($t[1]['function'] ?? '-'); }
function args_of($a, $b = 5) { return json_encode(func_get_args()) . func_num_args(); }
function throws($x) { if ($x > 1) throw new Exception("big $x"); return $x; }
function inc(&$v, $k) { $v = $v + $k; }
function cmp_ref(&$a, &$b) { return $a <=> $b; }
function typed(int $x): int { return $x + 1; }
function typed_bad(int $x): string { return $x; }
function stat_count($x) { static $n = 0; $n++; return $n; }
function changes_param($x) { $x = $x + 100; return debug_backtrace()[0]['args'][0]; }
function deep($x) { return array_map('plain', [$x])[0] + 1; }
function trace_str($x) { return (new Exception)->getTraceAsString(); }

var_dump(array_map('plain', [1, 2, 3]));
var_dump(array_map('bt', [7]));
var_dump(array_map('args_of', [1, 2]));
var_dump(call_user_func('args_of', 1, 2, 3));
var_dump(call_user_func_array('args_of', [4]));
var_dump(array_map('stat_count', [1, 1, 1]), stat_count(0));
var_dump(array_map('changes_param', [1]));
var_dump(array_map('deep', [1, 2]));
echo array_map('trace_str', [1])[0], "\n";
try {
    array_map('throws', [1, 2, 3]);
} catch (Exception $e) {
    echo $e->getMessage(), "\n", $e->getTraceAsString(), "\n";
}
$a = [1, 2, 3];
array_walk($a, 'inc');
var_dump($a);
$b = [3, 1, 2];
usort($b, 'cmp_ref');
var_dump($b);
var_dump(array_map('typed', [1, 2]));
try {
    var_dump(array_map('typed', ['x']));
} catch (TypeError $e) {
    echo get_class($e), ': ', $e->getMessage(), "\n";
}
var_dump(array_map('typed_bad', [1]));

class C {
    public $p = 10;
    private $q = 3;
    public function m($x) { return $x + $this->p + $this->q; }
    public static function s($x) { return static::class . $x; }
    public function bt($x) { $t = debug_backtrace(); return $t[0]['class'] . $t[0]['type'] . $t[0]['function'] . (isset($t[0]['object']) ? '+obj' : ''); }
    public function getClosure() { return function ($x) { return $x + $this->q; }; }
    public function getStatic() { return static function ($x) { return static::class . $x; }; }
}
class D extends C {}
$o = new C;
var_dump(array_map([$o, 'm'], [1, 2]));
var_dump(array_map(['D', 's'], [1]));
var_dump(array_map('D::s', [2]));
var_dump(array_map([$o, 'bt'], [1]));
var_dump(call_user_func_array([$o, 'm'], [5]));
var_dump(array_map($o->getClosure(), [1]));
var_dump(array_map((new D)->getStatic(), [1]));

$k = 5;
$byval = function ($x) use ($k) { return $x + $k; };
$byref = function ($x) use (&$k) { $k++; return $x + $k; };
var_dump(array_map($byval, [1, 2]), array_map($byref, [1, 2]), $k);
$arrow = fn($x) => $x * $k;
var_dump(array_map($arrow, [1, 2]));
$cnt = function () { static $c = 0; return ++$c; };
var_dump(array_map($cnt, [0, 0, 0]));
$cnt2 = clone $cnt;
var_dump($cnt2(), $cnt());
$bound = Closure::bind(function ($x) { return $this->q + $x; }, $o, C::class);
var_dump(array_map($bound, [1]));
$cl = function ($x) { return debug_backtrace()[0]['function'] . json_encode(func_get_args()); };
var_dump(array_map($cl, [1]), call_user_func($cl, 2, 3));
var_dump(array_filter([1, 2, 3, 4], fn($v) => $v % 2 == 0));
var_dump(array_all([1, 2], fn($v, $k) => !is_string($k)));
var_dump(preg_replace_callback('/\d/', fn($m) => $m[0] * 2, 'a1b2'));
$named = function ($a, $b) { return "$a-$b"; };
var_dump(call_user_func_array($named, ['b' => 2, 'a' => 1]));
var_dump(call_user_func($named, b: 3, a: 4));
function gen($x) { yield $x; yield $x + 1; }
foreach (call_user_func('gen', 5) as $v) echo $v, "\n";
function dtor_order($x) { $o = new class { function __destruct() { echo "dtor\n"; } }; echo "in\n"; return $x; }
var_dump(array_map('dtor_order', [1]));
set_error_handler(function ($no, $str) { echo "handler: $str\n"; return true; });
function warns($x) { return $x + $undefined; }
var_dump(array_map('warns', [1]));
restore_error_handler();
function too_few($a, $b) { return $a; }
try { array_map('too_few', [1]); } catch (ArgumentCountError $e) { echo $e->getMessage(), "\n"; }
$f = new Fiber(function () { return array_map(function ($x) { Fiber::suspend($x); return $x * 3; }, [1, 2]); });
var_dump($f->start(), $f->resume(), $f->resume(), $f->getReturn());
function recurse($n) { return $n <= 0 ? 0 : array_map('recurse', [$n - 1])[0] + 1; }
var_dump(recurse(50));
--EXPECTF--
array(3) {
  [0]=>
  int(2)
  [1]=>
  int(4)
  [2]=>
  int(6)
}
array(1) {
  [0]=>
  string(18) "2:bt:[7]:array_map"
}
array(2) {
  [0]=>
  string(4) "[1]1"
  [1]=>
  string(4) "[2]1"
}
string(8) "[1,2,3]3"
string(4) "[4]1"
array(3) {
  [0]=>
  int(1)
  [1]=>
  int(2)
  [2]=>
  int(3)
}
int(4)
array(1) {
  [0]=>
  int(101)
}
array(2) {
  [0]=>
  int(3)
  [1]=>
  int(5)
}
#0 [internal function]: trace_str(1)
#1 %s(25): array_map('trace_str', Array)
#2 {main}
big 2
#0 [internal function]: throws(2)
#1 %s(27): array_map('throws', Array)
#2 {main}
array(3) {
  [0]=>
  int(1)
  [1]=>
  int(3)
  [2]=>
  int(5)
}

Warning: cmp_ref(): Argument #1 ($a) must be passed by reference, value given in %s on line 35

Warning: cmp_ref(): Argument #2 ($b) must be passed by reference, value given in %s on line 35

Warning: cmp_ref(): Argument #1 ($a) must be passed by reference, value given in %s on line 35

Warning: cmp_ref(): Argument #2 ($b) must be passed by reference, value given in %s on line 35

Warning: cmp_ref(): Argument #1 ($a) must be passed by reference, value given in %s on line 35

Warning: cmp_ref(): Argument #2 ($b) must be passed by reference, value given in %s on line 35
array(3) {
  [0]=>
  int(1)
  [1]=>
  int(2)
  [2]=>
  int(3)
}
array(2) {
  [0]=>
  int(2)
  [1]=>
  int(3)
}
TypeError: typed(): Argument #1 ($x) must be of type int, string given
array(1) {
  [0]=>
  string(1) "1"
}
array(2) {
  [0]=>
  int(14)
  [1]=>
  int(15)
}
array(1) {
  [0]=>
  string(2) "D1"
}
array(1) {
  [0]=>
  string(2) "D2"
}
array(1) {
  [0]=>
  string(9) "C->bt+obj"
}
int(18)
array(1) {
  [0]=>
  int(4)
}
array(1) {
  [0]=>
  string(2) "D1"
}
array(2) {
  [0]=>
  int(6)
  [1]=>
  int(7)
}
array(2) {
  [0]=>
  int(7)
  [1]=>
  int(9)
}
int(7)
array(2) {
  [0]=>
  int(7)
  [1]=>
  int(14)
}
array(3) {
  [0]=>
  int(1)
  [1]=>
  int(2)
  [2]=>
  int(3)
}
int(4)
int(4)
array(1) {
  [0]=>
  int(4)
}
array(1) {
  [0]=>
  string(%d) "{closure:%s:76}[1]"
}
string(%d) "{closure:%s:76}[2,3]"
array(2) {
  [1]=>
  int(2)
  [3]=>
  int(4)
}
bool(true)
string(4) "a2b4"
string(3) "1-2"
string(3) "4-3"
5
6
in
dtor
array(1) {
  [0]=>
  int(1)
}
handler: Undefined variable $undefined
array(1) {
  [0]=>
  int(1)
}
Too few arguments to function too_few(), 1 passed and exactly 2 expected
int(1)
int(2)
NULL
array(2) {
  [0]=>
  int(3)
  [1]=>
  int(6)
}
int(50)
