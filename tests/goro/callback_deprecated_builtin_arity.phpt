--TEST--
A deprecated builtin called as a callback with a wrong argument count: the deprecation, then the ArgumentCountError
--FILE--
<?php function t($f) { try { var_dump($f()); } catch (\Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; } } t(fn() => call_user_func("utf8_decode")); t(fn() => call_user_func("utf8_encode", "a", "b")); t(fn() => call_user_func_array("utf8_decode", ["a", 1, 2]));
--EXPECTF--

Deprecated: Function utf8_decode() is deprecated since 8.2, visit the php.net documentation for various alternatives in %s on line 1
ArgumentCountError: utf8_decode() expects exactly 1 argument, 0 given

Deprecated: Function utf8_encode() is deprecated since 8.2, visit the php.net documentation for various alternatives in %s on line 1
ArgumentCountError: utf8_encode() expects exactly 1 argument, 2 given

Deprecated: Function utf8_decode() is deprecated since 8.2, visit the php.net documentation for various alternatives in %s on line 1
ArgumentCountError: utf8_decode() expects exactly 1 argument, 3 given
