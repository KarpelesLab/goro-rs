--TEST--
Dynamic calls of compact()/extract()/func_get_args()...: argument count errors come first
--FILE--
<?php
function t($f, ...$a) {
    try { $f(...$a); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), " @", $e->getLine(), " ", $e->getTrace()[0]['function'] ?? '-', "\n"; }
}
t('compact');
t('compact', 'x');
t('extract');
t('extract', []);
t('extract', [], 1, "", 4);
t('func_get_args');
t('func_get_args', 1);
t('func_get_arg');
t('func_get_arg', 0);
t('func_num_args');
t('get_defined_vars');
t('\compact', 'x');
t('COMPACT');
try { array_map('compact', [1]); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { call_user_func('compact'); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
function g() { $f = 'func_get_args'; return $f(); }
try { g(); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
--EXPECT--
ArgumentCountError: compact() expects at least 1 argument, 0 given @3 compact
Error: Cannot call compact() dynamically @3 compact
ArgumentCountError: extract() expects at least 1 argument, 0 given @3 extract
Error: Cannot call extract() dynamically @3 extract
ArgumentCountError: extract() expects at most 3 arguments, 4 given @3 extract
Error: Cannot call func_get_args() dynamically @3 func_get_args
ArgumentCountError: func_get_args() expects exactly 0 arguments, 1 given @3 func_get_args
ArgumentCountError: func_get_arg() expects exactly 1 argument, 0 given @3 func_get_arg
Error: Cannot call func_get_arg() dynamically @3 func_get_arg
Error: Cannot call func_num_args() dynamically @3 func_num_args
Error: Cannot call get_defined_vars() dynamically @3 get_defined_vars
Error: Cannot call compact() dynamically @3 compact
ArgumentCountError: compact() expects at least 1 argument, 0 given @3 compact
Error: Cannot call compact() dynamically
ArgumentCountError: compact() expects at least 1 argument, 0 given
Error: Cannot call func_get_args() dynamically
