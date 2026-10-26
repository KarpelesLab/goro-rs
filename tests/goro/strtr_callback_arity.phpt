--TEST--
strtr() called as a callback: its own argument count message
--FILE--
<?php function t($f) { try { var_dump($f()); } catch (\Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; } } t(fn() => strtr("a")); t(fn() => call_user_func("strtr", "a")); t(fn() => call_user_func_array("strtr", ["a"])); t(fn() => array_map("strtr", ["a"])); t(fn() => call_user_func("strtr", "a", "b", "c", "d")); t(fn() => call_user_func("strtr", "abc", ["a" => "x"]));
--EXPECTF--
ArgumentCountError: strtr() expects exactly 2 arguments, 1 given
ArgumentCountError: strtr() expects exactly 2 arguments, 1 given
ArgumentCountError: strtr() expects exactly 2 arguments, 1 given
ArgumentCountError: strtr() expects exactly 2 arguments, 1 given
ArgumentCountError: strtr() expects exactly 3 arguments, 4 given
string(3) "xbc"
