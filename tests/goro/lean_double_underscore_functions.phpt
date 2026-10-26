--TEST--
Fast and lean calls of user functions named with two leading underscores
--FILE--
<?php
// User functions whose names start with two underscores.
function __($text, $domain = 'default') { return translate($text, $domain); }
function translate($text, $domain) { return "[$domain]" . strtoupper($text); }
function __return_true() { return true; }
function __bt() {
    foreach (debug_backtrace() as $f) echo "  ", $f['function'], "@", $f['line'] ?? '-', " ", json_encode($f['args'] ?? null), "\n";
}
function __throws($x) { throw new RuntimeException("x=$x"); }
function __typed(int $x): int { return $x * 2; }
function __byref(&$a) { $a[] = count($a); }
function __static() { static $n = 0; return ++$n; }
function __variadic(...$a) { return count($a); }
function caller($t) { return __($t) . (__return_true() ? '+' : '-'); }
for ($i = 0; $i < 30; $i++) {
    $r = caller("hi$i");
}
echo $r, "\n";
function bt_caller() { __bt(); }
bt_caller();
try {
    __throws(3);
} catch (RuntimeException $e) {
    echo $e->getMessage(), "\n", $e->getTraceAsString(), "\n";
}
echo __typed(21), "\n";
try {
    echo __typed("x"), "\n";
} catch (TypeError $e) {
    echo $e->getMessage(), "\n";
}
$a = [];
for ($i = 0; $i < 3; $i++) __byref($a);
echo implode(',', $a), "\n";
for ($i = 0; $i < 3; $i++) echo __static(), "\n";
echo __variadic(1, 2, 3), "\n";
echo __('x', 'd'), "\n";
var_dump(function_exists('__'), is_callable('__return_true'));
echo implode(',', array_map('__', ['a', 'b'])), "\n";
$f = '__';
echo $f('dyn'), "\n";
echo call_user_func('__', 'cuf'), "\n";
echo "done\n";
--EXPECTF--
[default]HI29+
  __bt@19 []
  bt_caller@20 []
x=3
#0 %s(22): __throws(3)
#1 {main}
42
__typed(): Argument #1 ($x) must be of type int, string given, called in %s on line 28
0,1,2
1
2
3
3
[d]X
bool(true)
bool(true)
[default]A,[default]B
[default]DYN
[default]CUF
done
