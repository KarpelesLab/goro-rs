--TEST--
extract() checks its arguments before refusing a dynamic call; call_user_func_array() of it with a literal argument list
--FILE--
<?php
function t($f, ...$a) { try { $r = $f(...$a); echo $f, ': '; var_dump($r); } catch (Throwable $e) { echo $f, ': ', get_class($e), ': ', $e->getMessage(), "\n"; } }
t('extract', 'x');
t('extract', ['x'=>1], 99);
t('extract', ['x'=>1], EXTR_PREFIX_ALL);
t('extract', ['x'=>1], EXTR_PREFIX_ALL, '1bad');
t('extract', ['x'=>1], EXTR_PREFIX_ALL, '');
t('extract', ['x'=>1], EXTR_PREFIX_ALL | EXTR_REFS, 'ok');
t('extract', ['x'=>1], EXTR_REFS | 7);
t('extract', ['x'=>1], EXTR_SKIP, 'a-b');
t('extract', ['x'=>1], "abc");
t('extract');
t('extract', [], 0, 'p', 4);
function t2($c) { try { $c(); } catch (Throwable $e) { echo get_class($e), ': ', $e->getMessage(), "\n", $e->getTraceAsString(), "\n"; } }
t2(fn() => array_map('extract', [['a' => 1]]));
t2(fn() => array_map('extract', ['x']));
t2(fn() => array_map('compact', ['x']));
t2(fn() => array_map('func_get_args', ['x']));
t2(fn() => call_user_func_array('extract', [['a' => 1], 99]));
t2(fn() => call_user_func('extract', ['a' => 1], 99));
t2(fn() => call_user_func('extract', ['a' => 1]));
t2(fn() => array_walk($GLOBALS, 'extract'));
t2(fn() => usort($x, 'compact'));
$a = [[1], [2]];
t2(function() use ($a) { usort($a, 'compact'); });
t2(fn() => (new ReflectionFunction('extract'))->invoke(['a' => 1], 77));
try { call_user_func_array('extract', [['a' => 1], 99]); } catch (Throwable $e) { echo $e->getMessage(), "\n", $e->getTraceAsString(), "\n"; }
echo "--\n";
try { (new ReflectionFunction('extract'))->invoke(['a' => 1], 77); } catch (Throwable $e) { echo $e->getMessage(), "\n", $e->getTraceAsString(), "\n"; }
try { (new ReflectionFunction('extract'))->invoke(['a' => 1]); } catch (Throwable $e) { echo $e->getMessage(), "\n", $e->getTraceAsString(), "\n"; }
try { (new ReflectionFunction('compact'))->invokeArgs(['a']); } catch (Throwable $e) { echo $e->getMessage(), "\n", $e->getTraceAsString(), "\n"; }
try { call_user_func_array('compact', ['a']); } catch (Throwable $e) { echo $e->getMessage(), "\n", $e->getTraceAsString(), "\n"; }
var_dump($a ?? 'unset');
function f() {
  var_dump(call_user_func_array('extract', [['a' => 1]]));
  var_dump($a ?? 'unset');
  var_dump(call_user_func('extract', ['b' => 2]));
  var_dump($b ?? 'unset');
  var_dump(call_user_func_array('compact', ['a', 'b']));
  var_dump(call_user_func('get_defined_vars'));
}
f();
--EXPECTF--
extract: TypeError: extract(): Argument #1 ($array) must be of type array, string given
extract: ValueError: extract(): Argument #2 ($flags) must be a valid extract type
extract: ValueError: extract(): Argument #3 ($prefix) is required when using this extract type
extract: ValueError: extract(): Argument #3 ($prefix) must be a valid identifier
extract: Error: Cannot call extract() dynamically
extract: Error: Cannot call extract() dynamically
extract: ValueError: extract(): Argument #2 ($flags) must be a valid extract type
extract: ValueError: extract(): Argument #3 ($prefix) must be a valid identifier
extract: TypeError: extract(): Argument #2 ($flags) must be of type int, string given
extract: ArgumentCountError: extract() expects at least 1 argument, 0 given
extract: ArgumentCountError: extract() expects at most 3 arguments, 4 given
Error: Cannot call extract() dynamically
#0 [internal function]: extract(Array)
#1 %s(15): array_map('extract', Array)
#2 %s(14): {closure:%s:15}()
#3 %s(15): t2(Object(Closure))
#4 {main}
TypeError: extract(): Argument #1 ($array) must be of type array, string given
#0 [internal function]: extract('x')
#1 %s(16): array_map('extract', Array)
#2 %s(14): {closure:%s:16}()
#3 %s(16): t2(Object(Closure))
#4 {main}
Error: Cannot call compact() dynamically
#0 [internal function]: compact('x')
#1 %s(17): array_map('compact', Array)
#2 %s(14): {closure:%s:17}()
#3 %s(17): t2(Object(Closure))
#4 {main}
ArgumentCountError: func_get_args() expects exactly 0 arguments, 1 given
#0 [internal function]: func_get_args('x')
#1 %s(18): array_map('func_get_args', Array)
#2 %s(14): {closure:%s:18}()
#3 %s(18): t2(Object(Closure))
#4 {main}
ValueError: extract(): Argument #2 ($flags) must be a valid extract type
#0 %s(19): extract(Array, 99)
#1 %s(14): {closure:%s:19}()
#2 %s(19): t2(Object(Closure))
#3 {main}
ValueError: extract(): Argument #2 ($flags) must be a valid extract type
#0 %s(20): extract(Array, 99)
#1 %s(14): {closure:%s:20}()
#2 %s(20): t2(Object(Closure))
#3 {main}
Error: array_walk(): Argument #1 ($array) could not be passed by reference
#0 %s(14): {closure:%s:22}()
#1 %s(22): t2(Object(Closure))
#2 {main}
TypeError: usort(): Argument #1 ($array) must be of type array, null given
#0 %s(23): usort(NULL, 'compact')
#1 %s(14): {closure:%s:23}()
#2 %s(23): t2(Object(Closure))
#3 {main}
Error: Cannot call compact() dynamically
#0 [internal function]: compact(Array, Array)
#1 %s(25): usort(Array, 'compact')
#2 %s(14): {closure:%s:25}()
#3 %s(25): t2(Object(Closure))
#4 {main}
ValueError: extract(): Argument #2 ($flags) must be a valid extract type
#0 [internal function]: extract(Array, 77)
#1 %s(26): ReflectionFunction->invoke(Array, 77)
#2 %s(14): {closure:%s:26}()
#3 %s(26): t2(Object(Closure))
#4 {main}
extract(): Argument #2 ($flags) must be a valid extract type
#0 %s(27): extract(Array, 99)
#1 {main}
--
extract(): Argument #2 ($flags) must be a valid extract type
#0 [internal function]: extract(Array, 77)
#1 %s(29): ReflectionFunction->invoke(Array, 77)
#2 {main}
Cannot call extract() dynamically
#0 [internal function]: extract(Array)
#1 %s(30): ReflectionFunction->invoke(Array)
#2 {main}
Cannot call compact() dynamically
#0 [internal function]: compact('a')
#1 %s(31): ReflectionFunction->invokeArgs(Array)
#2 {main}
array(2) {
  [0]=>
  array(1) {
    [0]=>
    int(1)
  }
  [1]=>
  array(1) {
    [0]=>
    int(2)
  }
}
int(1)
int(1)
int(1)
int(2)
array(2) {
  ["a"]=>
  int(1)
  ["b"]=>
  int(2)
}
array(2) {
  ["a"]=>
  int(1)
  ["b"]=>
  int(2)
}
