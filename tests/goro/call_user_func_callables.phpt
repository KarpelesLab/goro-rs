--TEST--
call_user_func() / call_user_func_array() of the callable forms, from inside and outside the class scope
--FILE--
<?php
namespace NS { function nf($a) { return "NS\\nf($a)"; } }
namespace {
function f($a, $b = 'B') { return "f($a,$b)" . count(func_get_args()); }
function byref(&$x) { $x = 'set'; return 'byref'; }
function tr() { return array_map(fn($fr) => $fr['function'], debug_backtrace()); }
class K {
    public function m($a) { return "K->m($a)"; }
    private function p($a) { return "K->p($a)"; }
    protected function q($a) { return "K->q($a)"; }
    public static function s($a) { return "K::s($a)"; }
    public function __call($n, $a) { return "__call($n," . implode(',', $a) . ")"; }
    public static function __callStatic($n, $a) { return "__callStatic($n)"; }
    public function inside() {
        return [call_user_func([$this, 'p'], 1), call_user_func_array([$this, 'q'], [2]), call_user_func('self::s', 3)];
    }
    public function arr(array $a) { $a[] = 'x'; return count($a); }
}
$k = new K;
$cbs = ['f', 'strtoupper', 'NS\nf', fn($v) => "arrow($v)", [$k, 'm'], [$k, 'p'], [$k, 'q'], ['K', 's'], 'K::s', [$k, 'nope'], ['K', 'nope']];
for ($i = 0; $i < 2; $i++) {
    foreach ($cbs as $cb) {
        var_dump(call_user_func($cb, 'x'));
        var_dump(call_user_func_array($cb, ['y']));
    }
    print_r($k->inside());
    var_dump(call_user_func_array('f', ['b' => 2, 'a' => 1]));
    var_dump(call_user_func('f', 1, 2, 3));
    $v = 'orig';
    var_dump(call_user_func('byref', $v), $v);
    var_dump(call_user_func_array('byref', [&$v]), $v);
    print_r(call_user_func('tr'));
    $arr = [1, 2];
    var_dump(call_user_func([$k, 'arr'], $arr), $arr);
}
foreach (['nofunc', ['K'], [$k], ['NoClass', 'm'], 42, null] as $bad) {
    try { call_user_func($bad); } catch (TypeError $e) { echo $e->getMessage(), "\n"; }
    try { call_user_func_array($bad, []); } catch (TypeError $e) { echo $e->getMessage(), "\n"; }
}
try { call_user_func_array('f', 'notarray'); } catch (TypeError $e) { echo $e->getMessage(), "\n"; }
}
--EXPECTF--
string(7) "f(x,B)1"
string(7) "f(y,B)1"
string(1) "X"
string(1) "Y"
string(8) "NS\nf(x)"
string(8) "NS\nf(y)"
string(8) "arrow(x)"
string(8) "arrow(y)"
string(7) "K->m(x)"
string(7) "K->m(y)"
string(11) "__call(p,x)"
string(11) "__call(p,y)"
string(11) "__call(q,x)"
string(11) "__call(q,y)"
string(7) "K::s(x)"
string(7) "K::s(y)"
string(7) "K::s(x)"
string(7) "K::s(y)"
string(14) "__call(nope,x)"
string(14) "__call(nope,y)"
string(18) "__callStatic(nope)"
string(18) "__callStatic(nope)"

Deprecated: Use of "self" in callables is deprecated in %s on line 15
Array
(
    [0] => K->p(1)
    [1] => K->q(2)
    [2] => K::s(3)
)
string(7) "f(1,2)2"
string(7) "f(1,2)3"

Warning: byref(): Argument #1 ($x) must be passed by reference, value given in %s on line 30
string(5) "byref"
string(4) "orig"
string(5) "byref"
string(3) "set"
Array
(
    [0] => tr
)
int(3)
array(2) {
  [0]=>
  int(1)
  [1]=>
  int(2)
}
string(7) "f(x,B)1"
string(7) "f(y,B)1"
string(1) "X"
string(1) "Y"
string(8) "NS\nf(x)"
string(8) "NS\nf(y)"
string(8) "arrow(x)"
string(8) "arrow(y)"
string(7) "K->m(x)"
string(7) "K->m(y)"
string(11) "__call(p,x)"
string(11) "__call(p,y)"
string(11) "__call(q,x)"
string(11) "__call(q,y)"
string(7) "K::s(x)"
string(7) "K::s(y)"
string(7) "K::s(x)"
string(7) "K::s(y)"
string(14) "__call(nope,x)"
string(14) "__call(nope,y)"
string(18) "__callStatic(nope)"
string(18) "__callStatic(nope)"

Deprecated: Use of "self" in callables is deprecated in %s on line 15
Array
(
    [0] => K->p(1)
    [1] => K->q(2)
    [2] => K::s(3)
)
string(7) "f(1,2)2"
string(7) "f(1,2)3"

Warning: byref(): Argument #1 ($x) must be passed by reference, value given in %s on line 30
string(5) "byref"
string(4) "orig"
string(5) "byref"
string(3) "set"
Array
(
    [0] => tr
)
int(3)
array(2) {
  [0]=>
  int(1)
  [1]=>
  int(2)
}
call_user_func(): Argument #1 ($callback) must be a valid callback, function "nofunc" not found or invalid function name
call_user_func_array(): Argument #1 ($callback) must be a valid callback, function "nofunc" not found or invalid function name
call_user_func(): Argument #1 ($callback) must be a valid callback, array callback must have exactly two members
call_user_func_array(): Argument #1 ($callback) must be a valid callback, array callback must have exactly two members
call_user_func(): Argument #1 ($callback) must be a valid callback, array callback must have exactly two members
call_user_func_array(): Argument #1 ($callback) must be a valid callback, array callback must have exactly two members
call_user_func(): Argument #1 ($callback) must be a valid callback, class "NoClass" not found
call_user_func_array(): Argument #1 ($callback) must be a valid callback, class "NoClass" not found
call_user_func(): Argument #1 ($callback) must be a valid callback, no array or string given
call_user_func_array(): Argument #1 ($callback) must be a valid callback, no array or string given
call_user_func(): Argument #1 ($callback) must be a valid callback, no array or string given
call_user_func_array(): Argument #1 ($callback) must be a valid callback, no array or string given
call_user_func_array(): Argument #2 ($args) must be of type array, string given
