--TEST--
assert_options() converts the value with zval_try_get_string(); highlight_file("") / php_strip_whitespace("") throw "Path must not be empty"
--FILE--
<?php
function t($f) { try { $f(); } catch (\Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; } }
t(fn() => var_dump(assert_options(ASSERT_BAIL, new stdClass)));
t(fn() => var_dump(assert_options(ASSERT_BAIL, [])));
t(fn() => var_dump(highlight_file("")));
t(fn() => var_dump(show_source("", true)));
t(fn() => var_dump(php_strip_whitespace("")));
--EXPECTF--

Deprecated: Constant ASSERT_BAIL is deprecated since 8.3, as assert_options() is deprecated in %s on line 3

Deprecated: Function assert_options() is deprecated since 8.3 in %s on line 3
Error: Object of class stdClass could not be converted to string

Deprecated: Constant ASSERT_BAIL is deprecated since 8.3, as assert_options() is deprecated in %s on line 4

Deprecated: Function assert_options() is deprecated since 8.3 in %s on line 4

Warning: Array to string conversion in %s on line 4
int(0)

Warning: highlight_file(): Failed opening '' for highlighting in %s on line 5
ValueError: Path must not be empty

Warning: show_source(): Failed opening '' for highlighting in %s on line 6
ValueError: Path must not be empty
ValueError: Path must not be empty
