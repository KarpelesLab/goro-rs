--TEST--
call_user_func() of array_udiff()/array_diff_ukey()...: their own argument count check
--FILE--
<?php
function t($f) { try { var_dump($f()); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; } }
foreach (['array_udiff', 'array_diff_ukey', 'array_uintersect_uassoc', 'array_diff_uassoc'] as $fn) {
  t(fn() => $fn());
  t(fn() => $fn([1]));
  t(fn() => call_user_func($fn));
  t(fn() => call_user_func($fn, [1]));
  t(fn() => call_user_func_array($fn, []));
  t(fn() => call_user_func_array($fn, [[1]]));
  t(fn() => array_map($fn, [1]));
}
t(fn() => array_udiff());
t(fn() => array_udiff([1]));
--EXPECTF--
ArgumentCountError: array_udiff() expects at least 2 arguments, 0 given
ArgumentCountError: array_udiff() expects at least 2 arguments, 1 given
ArgumentCountError: array_udiff() expects at least 2 arguments, 0 given
ArgumentCountError: array_udiff() expects at least 2 arguments, 1 given
ArgumentCountError: array_udiff() expects at least 2 arguments, 0 given
ArgumentCountError: array_udiff() expects at least 2 arguments, 1 given
ArgumentCountError: array_udiff() expects at least 2 arguments, 1 given
ArgumentCountError: array_diff_ukey() expects at least 2 arguments, 0 given
ArgumentCountError: array_diff_ukey() expects at least 2 arguments, 1 given
ArgumentCountError: array_diff_ukey() expects at least 2 arguments, 0 given
ArgumentCountError: array_diff_ukey() expects at least 2 arguments, 1 given
ArgumentCountError: array_diff_ukey() expects at least 2 arguments, 0 given
ArgumentCountError: array_diff_ukey() expects at least 2 arguments, 1 given
ArgumentCountError: array_diff_ukey() expects at least 2 arguments, 1 given
ArgumentCountError: array_uintersect_uassoc() expects at least 3 arguments, 0 given
ArgumentCountError: array_uintersect_uassoc() expects at least 3 arguments, 1 given
ArgumentCountError: array_uintersect_uassoc() expects at least 3 arguments, 0 given
ArgumentCountError: array_uintersect_uassoc() expects at least 3 arguments, 1 given
ArgumentCountError: array_uintersect_uassoc() expects at least 3 arguments, 0 given
ArgumentCountError: array_uintersect_uassoc() expects at least 3 arguments, 1 given
ArgumentCountError: array_uintersect_uassoc() expects at least 3 arguments, 1 given
ArgumentCountError: array_diff_uassoc() expects at least 2 arguments, 0 given
ArgumentCountError: array_diff_uassoc() expects at least 2 arguments, 1 given
ArgumentCountError: array_diff_uassoc() expects at least 2 arguments, 0 given
ArgumentCountError: array_diff_uassoc() expects at least 2 arguments, 1 given
ArgumentCountError: array_diff_uassoc() expects at least 2 arguments, 0 given
ArgumentCountError: array_diff_uassoc() expects at least 2 arguments, 1 given
ArgumentCountError: array_diff_uassoc() expects at least 2 arguments, 1 given
ArgumentCountError: array_udiff() expects at least 2 arguments, 0 given
ArgumentCountError: array_udiff() expects at least 2 arguments, 1 given
