--TEST--
A #[\Deprecated] __call()/__callStatic() reached through callbacks deprecates the called method
--FILE--
<?php
class A { #[\Deprecated] function __call($n, $a) { echo "call $n\n"; return 1; } #[\Deprecated("why")] static function __callStatic($n, $a) { echo "cs $n\n"; return 1; } }
$a = new A;
array_map('A::sm', [1]);
array_map(['A', 'sm2'], [1]);
$x = [2, 1]; usort($x, [$a, 'cmp']);
call_user_func_array([$a, 'cufa'], [1]);
call_user_func_array('A::cufa2', [1]);
$r = new ReflectionMethod('A', '__call'); var_dump($r->isDeprecated());
set_error_handler(function($no, $s) { throw new Exception($s); });
try { call_user_func([$a, 'thrown']); } catch (Exception $e) { echo "caught: ", $e->getMessage(), "\n"; }
try { array_map([$a, 'thrown2'], [1]); } catch (Exception $e) { echo "caught: ", $e->getMessage(), "\n"; }
try { call_user_func('A::thrown3'); } catch (Exception $e) { echo "caught: ", $e->getMessage(), "\n"; }
restore_error_handler();
$b = new A;
$b->foo();
call_user_func([$b, 'bar']);
array_map([$b, 'baz'], [1]);
$f = Closure::fromCallable([$b, 'qux']); $f();
$g = $b->quux(...); $g();
A::st();
call_user_func('A::st2');
$h = A::st3(...); $h();
--EXPECTF--

Deprecated: Method A::sm() is deprecated, why in %s on line 4
cs sm

Deprecated: Method A::sm2() is deprecated, why in %s on line 5
cs sm2

Deprecated: Method A::cmp() is deprecated in %s on line 6
call cmp

Deprecated: Method A::cufa() is deprecated in %s on line 7
call cufa

Deprecated: Method A::cufa2() is deprecated, why in %s on line 8
cs cufa2
bool(true)
caught: Method A::thrown() is deprecated
caught: Method A::thrown2() is deprecated
caught: Method A::thrown3() is deprecated, why

Deprecated: Method A::foo() is deprecated in %s on line 16
call foo

Deprecated: Method A::bar() is deprecated in %s on line 17
call bar

Deprecated: Method A::baz() is deprecated in %s on line 18
call baz

Deprecated: Method A::qux() is deprecated in %s on line 19

Deprecated: Method A::__call() is deprecated in %s on line 19
call qux

Deprecated: Method A::quux() is deprecated in %s on line 20

Deprecated: Method A::__call() is deprecated in %s on line 20
call quux

Deprecated: Method A::st() is deprecated, why in %s on line 21
cs st

Deprecated: Method A::st2() is deprecated, why in %s on line 22
cs st2

Deprecated: Method A::st3() is deprecated, why in %s on line 23

Deprecated: Method A::__callStatic() is deprecated, why in %s on line 23
cs st3
