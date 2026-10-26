--TEST--
func_get_args() & co through call_user_func(): a direct call sees the calling code's scope, a dynamic one goes through zend_forbid_dynamic_call()
--FILE--
<?php
// call_user_func()/call_user_func_array() called directly are compiled to
// ZEND_INIT_USER_CALL: func_*() see the calling code's scope
$fn = 'func_num_args';
try { var_dump(call_user_func_array($fn, [])); } catch (Error $e) { echo $e->getMessage(), "\n"; }
try { var_dump(call_user_func($fn)); } catch (Error $e) { echo $e->getMessage(), "\n"; }
$fn = 'func_get_arg';
try { var_dump(call_user_func($fn, -1)); } catch (Error $e) { echo $e->getMessage(), "\n"; }
try { var_dump(call_user_func($fn, 1)); } catch (Error $e) { echo $e->getMessage(), "\n"; }
function f($fn) {
  try { var_dump(call_user_func($fn, 0)); } catch (Error $e) { echo $e->getMessage(), "\n"; }
  try { var_dump(call_user_func_array($fn, [-1])); } catch (Error $e) { echo $e->getMessage(), "\n"; }
}
f('func_get_arg');
// through an internal function's frame: zend_forbid_dynamic_call()
$g = 'call_user_func';
try { $g('func_get_args'); } catch (Error $e) { echo $e->getMessage(), "\n"; }
try { $g('func_get_arg', -1); } catch (Error $e) { echo $e->getMessage(), "\n"; }
try { array_map('func_num_args', [1]); } catch (Error $e) { echo $e->getMessage(), "\n"; }
try { array_map('func_get_arg', [-1]); } catch (Error $e) { echo $e->getMessage(), "\n"; }
--EXPECTF--
func_num_args() must be called from a function context
func_num_args() must be called from a function context
func_get_arg(): Argument #1 ($position) must be greater than or equal to 0
func_get_arg() cannot be called from the global scope
Cannot call func_get_arg() dynamically
func_get_arg(): Argument #1 ($position) must be greater than or equal to 0
Cannot call func_get_args() dynamically
func_get_arg(): Argument #1 ($position) must be greater than or equal to 0
func_num_args() expects exactly 0 arguments, 1 given
func_get_arg(): Argument #1 ($position) must be greater than or equal to 0
