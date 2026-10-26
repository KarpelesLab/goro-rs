--TEST--
Too few arguments: a variadic parameter does not count, so 'exactly' when all others are required
--FILE--
<?php
function t($f) { try { $f(); } catch (\Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; } }
function v($a, ...$rest) {}
function w($a, $b = 1, ...$rest) {}
function x2($a, $b) {}
function y($a, $b = 2) {}
function z(...$r) {}
t(fn() => v());
t(fn() => v(x: 1));
t(fn() => w());
t(fn() => x2(1));
t(fn() => y());
t(fn() => z());
class K {
  function m($a, ...$r) {}
  static function s($a, $b, ...$r) {}
  function __construct($p, ...$q) {}
}
t(fn() => (new K(1))->m());
t(fn() => K::s(1));
t(fn() => new K());
t(fn() => (new ReflectionMethod('K', 'm'))->invoke(new K(1)));
t(fn() => (new ReflectionMethod('K', 's'))->invokeArgs(null, [1]));
t(fn() => (new ReflectionClass('K'))->newInstance());
t(fn() => (new ReflectionClass('K'))->newInstanceArgs([]));
t(fn() => call_user_func('v'));
t(fn() => call_user_func_array([new K(1), 'm'], []));
t(fn() => array_map('v', []) + array_map('v', [[]]));
$c = function ($a, ...$b) {};
t(fn() => $c());
t(fn() => (fn($a, ...$b) => 1)());
--EXPECTF--
ArgumentCountError: Too few arguments to function v(), 0 passed in %s on line 8 and exactly 1 expected
ArgumentCountError: Too few arguments to function v(), 0 passed in %s on line 9 and exactly 1 expected
ArgumentCountError: Too few arguments to function w(), 0 passed in %s on line 10 and at least 1 expected
ArgumentCountError: Too few arguments to function x2(), 1 passed in %s on line 11 and exactly 2 expected
ArgumentCountError: Too few arguments to function y(), 0 passed in %s on line 12 and at least 1 expected
ArgumentCountError: Too few arguments to function K::m(), 0 passed in %s on line 19 and exactly 1 expected
ArgumentCountError: Too few arguments to function K::s(), 1 passed in %s on line 20 and exactly 2 expected
ArgumentCountError: Too few arguments to function K::__construct(), 0 passed in %s on line 21 and exactly 1 expected
ArgumentCountError: Too few arguments to function K::m(), 0 passed and exactly 1 expected
ArgumentCountError: Too few arguments to function K::s(), 1 passed and exactly 2 expected
ArgumentCountError: Too few arguments to function K::__construct(), 0 passed and exactly 1 expected
ArgumentCountError: Too few arguments to function K::__construct(), 0 passed and exactly 1 expected
ArgumentCountError: Too few arguments to function v(), 0 passed in %s on line 26 and exactly 1 expected
ArgumentCountError: Too few arguments to function K::m(), 0 passed in %s on line 27 and exactly 1 expected
ArgumentCountError: Too few arguments to function {closure:%s:29}(), 0 passed in %s on line 30 and exactly 1 expected
ArgumentCountError: Too few arguments to function {closure:{closure:%s:31}:31}(), 0 passed in %s on line 31 and exactly 1 expected
