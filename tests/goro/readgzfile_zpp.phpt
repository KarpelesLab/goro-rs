--TEST--
readgzfile(): argument count and types checked like gzfile()'s
--FILE--
<?php
function t($f) { try { var_dump($f()); } catch (\Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; } }
t(fn() => readgzfile());
t(fn() => readgzfile("a", 1, 2));
t(fn() => readgzfile([]));
t(fn() => readgzfile("/nonexistent/x"));
t(fn() => readgzfile(""));
t(fn() => readgzfile("a\0b"));
t(fn() => call_user_func("readgzfile"));
t(fn() => readgzfile("/nonexistent", "x"));
--EXPECTF--
ArgumentCountError: readgzfile() expects at least 1 argument, 0 given
ArgumentCountError: readgzfile() expects at most 2 arguments, 3 given
TypeError: readgzfile(): Argument #1 ($filename) must be of type string, array given

Warning: readgzfile(/nonexistent/x): Failed to open stream: No such file or directory in %s on line 6
bool(false)
ValueError: Path must not be empty
ValueError: readgzfile(): Argument #1 ($filename) must not contain any null bytes
ArgumentCountError: readgzfile() expects at least 1 argument, 0 given

Warning: readgzfile(/nonexistent): Failed to open stream: No such file or directory in %s on line 10
bool(false)
