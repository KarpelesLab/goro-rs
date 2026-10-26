--TEST--
stream_wrapper_register(): the class name argument is converted with zval_try_get_string()
--FILE--
<?php
function t($f) { try { $f(); } catch (\Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; } }
t(fn() => var_dump(stream_wrapper_register("x", [])));
t(fn() => var_dump(stream_wrapper_register("x", new stdClass)));
t(fn() => var_dump(stream_register_wrapper("x", new stdClass)));
--EXPECTF--

Warning: Array to string conversion in %s on line 3
TypeError: stream_wrapper_register(): Argument #2 ($class) must be a valid class name, Array given
Error: Object of class stdClass could not be converted to string
Error: Object of class stdClass could not be converted to string
