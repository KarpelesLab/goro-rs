--TEST--
call_user_func() / call_user_func_array() arguments sent by fast runs
--FILE--
<?php
function add($a, $b = 10) { return $a + $b; }
function byref(&$x) { $x++; return $x; }
function makes() { return [3, 4]; }
function show(...$a) { return json_encode($a); }
class K { public $v = 1; function m($a) { return $this->v + $a; } static function s($a, $b) { return "$a/$b"; } }
function run() {
    $s = 0;
    $k = new K;
    $arr = [1, 2];
    for ($i = 0; $i < 3; $i++) {
        $s += call_user_func('add', $i);
        $s += call_user_func_array('add', [$i, 1]);
        $s += call_user_func_array('add', makes());
        $s += call_user_func([$k, 'm'], $i);
        $s += call_user_func_array([$k, 'm'], [$i]);
        echo call_user_func_array('K::s', $arr), ' ', call_user_func('show', $arr, [$i]), "\n";
    }
    echo $s, "\n";
    $x = 5;
    echo call_user_func('byref', $x), ' ', $x, "\n";
    echo call_user_func_array('byref', [&$x]), ' ', $x, "\n";
    echo call_user_func(fn($a) => $a * 2, 21), "\n";
    $arr2 = $arr;
    echo call_user_func_array('show', $arr2), json_encode($arr), "\n";
}
run();
?>
--EXPECTF--
1/2 [[1,2],[0]]
1/2 [[1,2],[1]]
1/2 [[1,2],[2]]
72

Warning: byref(): Argument #1 ($x) must be passed by reference, value given in %s on line 21
6 5
6 6
42
[1,2][1,2]
