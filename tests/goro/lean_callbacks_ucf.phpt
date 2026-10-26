--TEST--
call_user_func() / call_user_func_array() called directly, from lean callees too
--INI--
zend.exception_ignore_args=0
--FILE--
<?php
// call_user_func() / call_user_func_array() called directly: their callee's
// frame is at the call site (php-src compiles them away).
function where($x) { $t = debug_backtrace(); return json_encode([count($t), $t[0]['line'] ?? null, $t[0]['function'], $t[0]['args'], $t[1]['function'] ?? null]); }
function thrower($x) { throw new RuntimeException("no $x"); }
function wrap($cb, $args) { return call_user_func_array($cb, $args); }
function wrap1($cb, $a) { return call_user_func($cb, $a); }
function add1($x) { return $x + 1; }
function refinc(&$x) { $x++; return $x; }
class H {
    public $callbacks = [];
    public function add($cb) { $this->callbacks[] = $cb; }
    public function apply($value, ...$args) {
        foreach ($this->callbacks as $cb) {
            $value = call_user_func_array($cb, array_merge([$value], $args));
        }
        return $value;
    }
    public function m($v) { return $v . 'm'; }
    private function priv($v) { return $v . 'p'; }
    public function callPriv($v) { return call_user_func([$this, 'priv'], $v); }
}
var_dump(call_user_func('where', 1));
var_dump(call_user_func_array('where', [2]));
var_dump(wrap('where', [3]));
var_dump(wrap1('where', 4));
for ($i = 0; $i < 3; $i++) { echo wrap('add1', [$i]), wrap1('add1', $i), "\n"; }
try {
    wrap('thrower', ['x']);
} catch (RuntimeException $e) {
    echo $e->getMessage(), "\n", $e->getTraceAsString(), "\n";
}
try {
    call_user_func('thrower', 'y');
} catch (RuntimeException $e) {
    echo $e->getMessage(), "\n", $e->getTraceAsString(), "\n";
}
$h = new H;
$h->add('add1');
$h->add([$h, 'm']);
$h->add(fn($v, $extra) => "$v+$extra");
$h->add(function ($v) { return "[$v]"; });
for ($i = 0; $i < 3; $i++) var_dump($h->apply($i, 'e'));
var_dump($h->callPriv('q'));
$v = 1;
$arr = [&$v];
var_dump(call_user_func_array('refinc', $arr), $v);
var_dump(call_user_func_array('add1', ['x' => 5]));
$o = new H;
var_dump(call_user_func_array([$o, 'm'], ['z']));
var_dump(call_user_func('str_repeat', 'ab', 2));
function gen() { yield 1; }
var_dump(get_class(call_user_func('gen')));
function deep_trace($n) { return $n ? call_user_func('deep_trace', $n - 1) : (new Exception)->getTraceAsString(); }
echo deep_trace(3), "\n";
function cb_static() { static $calls = 0; return ++$calls; }
for ($i = 0; $i < 3; $i++) echo call_user_func('cb_static');
echo "\n";
$counter = 0;
$inc = function () use (&$counter) { return ++$counter; };
for ($i = 0; $i < 3; $i++) call_user_func($inc);
var_dump($counter);
function too_few2($a, $b) { return $a; }
try { call_user_func('too_few2', 1); } catch (ArgumentCountError $e) { echo $e->getMessage(), "\n"; }
function typed2(int $a) { return $a; }
try { call_user_func('typed2', 'abc'); } catch (TypeError $e) { echo $e->getMessage(), "\n"; }
var_dump(call_user_func('typed2', '5'));
function dtor_cb() { return new class { function __destruct() { echo "gone\n"; } }; }
call_user_func('dtor_cb');
echo "after\n";
$x = call_user_func('dtor_cb');
echo "kept\n";
unset($x);
echo "end\n";
--EXPECTF--
string(23) "[1,23,"where",[1],null]"
string(23) "[1,24,"where",[2],null]"
string(24) "[2,6,"where",[3],"wrap"]"
string(25) "[2,7,"where",[4],"wrap1"]"
11
22
33
no x
#0 %s(6): thrower('x')
#1 %s(29): wrap('thrower', Array)
#2 {main}
no y
#0 %s(34): thrower('y')
#1 {main}
string(6) "[1m+e]"
string(6) "[2m+e]"
string(6) "[3m+e]"
string(2) "qp"
int(2)
int(2)
int(6)
string(2) "zm"
string(4) "abab"
string(9) "Generator"
#0 %s(54): deep_trace(0)
#1 %s(54): deep_trace(1)
#2 %s(54): deep_trace(2)
#3 %s(55): deep_trace(3)
#4 {main}
123
int(3)
Too few arguments to function too_few2(), 1 passed in %s on line 64 and exactly 2 expected
typed2(): Argument #1 ($a) must be of type int, string given, called in %s on line 66
int(5)
gone
after
kept
gone
end
