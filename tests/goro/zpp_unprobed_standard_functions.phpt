--TEST--
zpp of ext/standard functions with side effects (sleep, handlers, output, dns...): argument count and types
--FILE--
<?php
function t($f) { try { $f(); } catch (\Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; } }
t(fn() => usleep("abc"));
t(fn() => usleep(1, 2));
t(fn() => time_nanosleep(0));
t(fn() => time_nanosleep(0, []));
t(fn() => time_sleep_until("x"));
t(fn() => set_time_limit([]));
t(fn() => var_dump(set_error_handler(null, "x")));
t(fn() => var_dump(set_error_handler("nope", "x")));
t(fn() => set_exception_handler(null, 1));
t(fn() => restore_error_handler(1));
t(fn() => unregister_tick_function());
t(fn() => header_register_callback());
t(fn() => error_reporting([]));
t(fn() => ini_restore([]));
t(fn() => ignore_user_abort("x"));
t(fn() => error_log([]));
t(fn() => error_log("x", "y"));
t(fn() => openlog("x", 1));
t(fn() => umask("x"));
t(fn() => var_dump(umask(null) === umask()));
t(fn() => chdir([]));
t(fn() => putenv([]));
t(fn() => printf([]));
t(fn() => header([]));
t(fn() => http_response_code("x"));
t(fn() => gc_enable(1));
t(fn() => gethostbyname([]));
t(fn() => debug_backtrace("x"));
t(fn() => debug_print_backtrace(0, "x"));
t(fn() => checkdnsrr([]));
t(fn() => dns_check_record("x", []));
t(fn() => getmxrr("x"));
t(fn() => var_dump(forward_static_call_array("strlen", ["abc"])));
t(fn() => forward_static_call_array("strlen", "abc"));
t(fn() => forward_static_call_array("nope", "abc"));
t(fn() => forward_static_call_array(nonexistent: 1));
t(fn() => spl_autoload_register(null, []));
t(fn() => spl_autoload_register("nope", []));
t(fn() => spl_autoload_register(null, true, true, 1));
--EXPECT--
TypeError: usleep(): Argument #1 ($microseconds) must be of type int, string given
ArgumentCountError: usleep() expects exactly 1 argument, 2 given
ArgumentCountError: time_nanosleep() expects exactly 2 arguments, 1 given
TypeError: time_nanosleep(): Argument #2 ($nanoseconds) must be of type int, array given
TypeError: time_sleep_until(): Argument #1 ($timestamp) must be of type float, string given
TypeError: set_time_limit(): Argument #1 ($seconds) must be of type int, array given
TypeError: set_error_handler(): Argument #2 ($error_levels) must be of type int, string given
TypeError: set_error_handler(): Argument #1 ($callback) must be a valid callback or null, function "nope" not found or invalid function name
ArgumentCountError: set_exception_handler() expects exactly 1 argument, 2 given
ArgumentCountError: restore_error_handler() expects exactly 0 arguments, 1 given
ArgumentCountError: unregister_tick_function() expects exactly 1 argument, 0 given
ArgumentCountError: header_register_callback() expects exactly 1 argument, 0 given
TypeError: error_reporting(): Argument #1 ($error_level) must be of type ?int, array given
TypeError: ini_restore(): Argument #1 ($option) must be of type string, array given
TypeError: error_log(): Argument #1 ($message) must be of type string, array given
TypeError: error_log(): Argument #2 ($message_type) must be of type int, string given
ArgumentCountError: openlog() expects exactly 3 arguments, 2 given
TypeError: umask(): Argument #1 ($mask) must be of type ?int, string given
bool(true)
TypeError: chdir(): Argument #1 ($directory) must be of type string, array given
TypeError: putenv(): Argument #1 ($assignment) must be of type string, array given
TypeError: printf(): Argument #1 ($format) must be of type string, array given
TypeError: header(): Argument #1 ($header) must be of type string, array given
TypeError: http_response_code(): Argument #1 ($response_code) must be of type int, string given
ArgumentCountError: gc_enable() expects exactly 0 arguments, 1 given
TypeError: gethostbyname(): Argument #1 ($hostname) must be of type string, array given
TypeError: debug_backtrace(): Argument #1 ($options) must be of type int, string given
TypeError: debug_print_backtrace(): Argument #2 ($limit) must be of type int, string given
TypeError: checkdnsrr(): Argument #1 ($hostname) must be of type string, array given
TypeError: dns_check_record(): Argument #2 ($type) must be of type string, array given
ArgumentCountError: getmxrr() expects at least 2 arguments, 1 given
int(3)
TypeError: forward_static_call_array(): Argument #2 ($args) must be of type array, string given
TypeError: forward_static_call_array(): Argument #1 ($callback) must be a valid callback, function "nope" not found or invalid function name
Error: Unknown named parameter $nonexistent
TypeError: spl_autoload_register(): Argument #2 ($throw) must be of type bool, array given
TypeError: spl_autoload_register(): Argument #1 ($callback) must be a valid callback or null, function "nope" not found or invalid function name
ArgumentCountError: spl_autoload_register() expects at most 3 arguments, 4 given
