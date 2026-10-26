--TEST--
print_r() / var_export(): $return is a bool parameter (zpp)
--FILE--
<?php
function t($f) { try { $f(); } catch (\Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; } }
t(fn() => var_dump(print_r(1, [])));
t(fn() => var_dump(var_export(1, [])));
t(fn() => var_dump(print_r(2, "1")));
t(fn() => var_dump(var_export(3, 1.0)));
t(fn() => var_dump(print_r(4, null)));
t(fn() => var_dump(var_export(5, new stdClass)));
t(fn() => var_dump(function_exists('debug_zval_refcount')));
--EXPECTF--
TypeError: print_r(): Argument #2 ($return) must be of type bool, array given
TypeError: var_export(): Argument #2 ($return) must be of type bool, array given
string(1) "2"
string(1) "3"

Deprecated: print_r(): Passing null to parameter #2 ($return) of type bool is deprecated in %s on line 7
4bool(true)
TypeError: var_export(): Argument #2 ($return) must be of type bool, stdClass given
bool(false)
