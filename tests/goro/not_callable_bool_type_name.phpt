--TEST--
Value of type bool is not callable (not true/false)
--FILE--
<?php
function t($f) { try { var_dump($f()); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; } }
foreach ([true, false] as $v) {
    t(fn() => $v());
    t(fn() => 1 |> $v);
    t(fn() => call_user_func($v));
    t(fn() => array_map($v, [1]));
}
--EXPECTF--
Error: Value of type bool is not callable
Error: Value of type bool is not callable
TypeError: call_user_func(): Argument #1 ($callback) must be a valid callback, no array or string given
TypeError: array_map(): Argument #1 ($callback) must be a valid callback or null, no array or string given
Error: Value of type bool is not callable
Error: Value of type bool is not callable
TypeError: call_user_func(): Argument #1 ($callback) must be a valid callback, no array or string given
TypeError: array_map(): Argument #1 ($callback) must be a valid callback or null, no array or string given
