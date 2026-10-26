--TEST--
Namespaced fallback calls of builtins: frames named after the builtin, real calls (zend_compile_ns_call)
--INI--
zend.exception_ignore_args=0
--FILE--
<?php
namespace foo;
function t($f) { try { $f(); } catch (\Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n", $e->getTraceAsString(), "\n"; } }
function cb($x) { throw new \Exception("boom $x"); }
t(function () { return count(1); });
t(function () { return \count(1); });
t(function () { return strlen([]); });
t(function () { return \strlen([]); });
t(function () { return array_map(1, []); });
t(function () { return str_repeat("x", -1); });
t(function () { return iterator_to_array(new \ArrayIterator([1]), true, 5); });
t(function () { return call_user_func_array('foo\nope', []); });
t(function () { return array_filter([1], function ($x) { throw new \Exception("in cb"); }); });
t(function () { return \array_filter([1], function ($x) { throw new \Exception("in cb"); }); });
t(function () { return preg_replace_callback('/./', function ($m) { throw new \Exception("in cb"); }, "a"); });
t(function () { return \preg_replace_callback('/./', function ($m) { throw new \Exception("in cb"); }, "a"); });
try {
    array_map('foo\cb', [1, 2]);
} catch (\Exception $e) {
    echo $e->getTraceAsString(), "\n";
    var_dump($e->getTrace()[1]['function']);
}
function bt($x) { debug_print_backtrace(); return $x; }
array_map('foo\bt', [1]);
$f = fn($x) => strlen($x) + count(debug_backtrace());
echo implode(",", array_map($f, ["ab"])), "\n";
$a = [2, 1]; usort($a, function ($x, $y) { debug_print_backtrace(); return $x <=> $y; });
echo strlen(...)("abc"), "\n";
$g = array_map(...);
try { $g(function ($x) { throw new \Exception('x'); }, [1]); } catch (\Exception $e) { echo $e->getTraceAsString(), "\n"; }
try { \call_user_func('foo\cb', 3); } catch (\Exception $e) { echo $e->getTraceAsString(), "\n"; }
try { call_user_func('foo\cb', 3); } catch (\Exception $e) { echo $e->getTraceAsString(), "\n"; }
--EXPECTF--
TypeError: count(): Argument #1 ($value) must be of type Countable|array, int given
#0 %s(5): count(1)
#1 %s(3): {closure:%s:5}()
#2 %s(5): foo\t(Object(Closure))
#3 {main}
TypeError: count(): Argument #1 ($value) must be of type Countable|array, int given
#0 %s(3): {closure:%s:6}()
#1 %s(6): foo\t(Object(Closure))
#2 {main}
TypeError: strlen(): Argument #1 ($string) must be of type string, array given
#0 %s(7): strlen(Array)
#1 %s(3): {closure:%s:7}()
#2 %s(7): foo\t(Object(Closure))
#3 {main}
TypeError: strlen(): Argument #1 ($string) must be of type string, array given
#0 %s(3): {closure:%s:8}()
#1 %s(8): foo\t(Object(Closure))
#2 {main}
TypeError: array_map(): Argument #1 ($callback) must be a valid callback or null, no array or string given
#0 %s(9): array_map(1, Array)
#1 %s(3): {closure:%s:9}()
#2 %s(9): foo\t(Object(Closure))
#3 {main}
ValueError: str_repeat(): Argument #2 ($times) must be greater than or equal to 0
#0 %s(10): str_repeat('x', -1)
#1 %s(3): {closure:%s:10}()
#2 %s(10): foo\t(Object(Closure))
#3 {main}
ArgumentCountError: iterator_to_array() expects at most 2 arguments, 3 given
#0 %s(11): iterator_to_array(Object(ArrayIterator), true, 5)
#1 %s(3): {closure:%s:11}()
#2 %s(11): foo\t(Object(Closure))
#3 {main}
TypeError: call_user_func_array(): Argument #1 ($callback) must be a valid callback, function "foo\nope" not found or invalid function name
#0 %s(12): call_user_func_array('foo\\nope', Array)
#1 %s(3): {closure:%s:12}()
#2 %s(12): foo\t(Object(Closure))
#3 {main}
Exception: in cb
#0 [internal function]: {closure:{closure:%s:13}:13}(1)
#1 %s(13): array_filter(Array, Object(Closure))
#2 %s(3): {closure:%s:13}()
#3 %s(13): foo\t(Object(Closure))
#4 {main}
Exception: in cb
#0 [internal function]: {closure:{closure:%s:14}:14}(1)
#1 %s(14): array_filter(Array, Object(Closure))
#2 %s(3): {closure:%s:14}()
#3 %s(14): foo\t(Object(Closure))
#4 {main}
Exception: in cb
#0 [internal function]: {closure:{closure:%s:15}:15}(Array)
#1 %s(15): preg_replace_callback('/./', Object(Closure), 'a')
#2 %s(3): {closure:%s:15}()
#3 %s(15): foo\t(Object(Closure))
#4 {main}
Exception: in cb
#0 [internal function]: {closure:{closure:%s:16}:16}(Array)
#1 %s(16): preg_replace_callback('/./', Object(Closure), 'a')
#2 %s(3): {closure:%s:16}()
#3 %s(16): foo\t(Object(Closure))
#4 {main}
#0 [internal function]: foo\cb(1)
#1 %s(18): array_map('foo\\cb', Array)
#2 {main}
string(9) "array_map"
#0 [internal function]: foo\bt(1)
#1 %s(24): array_map('foo\\bt', Array)
4
#0 [internal function]: {closure:%s:27}(2, 1)
#1 %s(27): usort(Array, Object(Closure))
3
#0 [internal function]: {closure:%s:30}(1)
#1 %s(30): array_map(Object(Closure), Array)
#2 {main}
#0 %s(31): foo\cb(3)
#1 {main}
#0 [internal function]: foo\cb(3)
#1 %s(32): call_user_func('foo\\cb', 3)
#2 {main}
