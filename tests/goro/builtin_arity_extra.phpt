--TEST--
Argument counts of ctype_*, file stat, method_exists() and call_user_func() functions
--FILE--
<?php
// Argument counts of functions outside the generated zpp whitelist
foreach ([fn() => ctype_digit(), fn() => ctype_alpha('a', 'b'), fn() => is_file(), fn() => file_exists('a', 'b'),
          fn() => filesize(), fn() => method_exists('x'), fn() => property_exists(new stdClass), fn() => get_class(new stdClass, 1),
          fn() => call_user_func(), fn() => call_user_func_array('strlen'), fn() => is_dir(), fn() => filetype('a', 'b'),
          fn() => ctype_digit('12'), fn() => method_exists('stdClass', 'x'), fn() => call_user_func('strlen', 'abc')] as $f) {
    try { var_dump($f()); } catch (ArgumentCountError $e) { echo $e->getMessage(), "\n"; }
}
--EXPECT--
ctype_digit() expects exactly 1 argument, 0 given
ctype_alpha() expects exactly 1 argument, 2 given
is_file() expects exactly 1 argument, 0 given
file_exists() expects exactly 1 argument, 2 given
filesize() expects exactly 1 argument, 0 given
method_exists() expects exactly 2 arguments, 1 given
property_exists() expects exactly 2 arguments, 1 given
get_class() expects at most 1 argument, 2 given
call_user_func() expects at least 1 argument, 0 given
call_user_func_array() expects exactly 2 arguments, 1 given
is_dir() expects exactly 1 argument, 0 given
filetype() expects exactly 1 argument, 2 given
bool(true)
bool(false)
int(3)
