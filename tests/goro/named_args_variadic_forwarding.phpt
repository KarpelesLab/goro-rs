--TEST--
Named arguments: legacy-zpp variadics reject unknown ones first; call_user_func() places $callback; failed calls drop them
--FILE--
<?php
function t($f) { try { $f(); } catch (\Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; } }
foreach (['array_diff_key', 'array_udiff', 'array_diff', 'array_intersect', 'register_tick_function', 'register_shutdown_function', 'call_user_func', 'forward_static_call'] as $f) {
    t(fn() => $f(nonexistent: 1));
}
t(fn() => var_dump(call_user_func(callback: 'strlen', string: 'abc')));
t(fn() => var_dump(call_user_func(string: 'abc', callback: 'strlen')));
t(fn() => var_dump(call_user_func('strlen', callback: 'strlen')));
t(fn() => var_dump(call_user_func_array(callback: 'strlen', args: ['abc'])));
t(fn() => var_dump(call_user_func_array('strlen', foo: ['abc'])));
// (named arguments of a failed call are not kept for the next one)
t(fn() => forward_static_call(nonexistent: 1));
t(fn() => var_dump(call_user_func('strlen', 'abc')));
--EXPECT--
ArgumentCountError: array_diff_key() does not accept unknown named parameters
ArgumentCountError: array_udiff() does not accept unknown named parameters
ArgumentCountError: array_diff() expects at least 1 argument, 0 given
ArgumentCountError: array_intersect() does not accept unknown named parameters
ArgumentCountError: register_tick_function() does not accept unknown named parameters
ArgumentCountError: register_shutdown_function() does not accept unknown named parameters
ArgumentCountError: call_user_func() expects at least 1 argument, 0 given
ArgumentCountError: forward_static_call() expects at least 1 argument, 0 given
int(3)
int(3)
Error: Named parameter $callback overwrites previous argument
int(3)
Error: Unknown named parameter $foo
ArgumentCountError: forward_static_call() expects at least 1 argument, 0 given
int(3)
