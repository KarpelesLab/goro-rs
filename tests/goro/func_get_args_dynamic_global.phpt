--TEST--
func_get_args()/func_get_arg()/func_num_args() called dynamically check $position and the global scope first
--FILE--
<?php
$a = 'func_get_args'; $b = 'func_get_arg'; $c = 'func_num_args';
// global scope: the scope check comes before zend_forbid_dynamic_call()
try { $a(); } catch (Error $e) { echo $e->getMessage(), "\n"; }
try { $b(0); } catch (Error $e) { echo $e->getMessage(), "\n"; }
try { $b(-1); } catch (Error $e) { echo $e->getMessage(), "\n"; }
try { $b(1.5); } catch (Error $e) { echo $e->getMessage(), "\n"; }
try { $c(); } catch (Error $e) { echo $e->getMessage(), "\n"; }
try { $a(1); } catch (Error $e) { echo $e->getMessage(), "\n"; }
// in a function
function g() { $a = 'func_get_args'; $b = 'func_get_arg'; $c = 'func_num_args';
  try { $a(); } catch (Error $e) { echo $e->getMessage(), "\n"; }
  try { $b(0); } catch (Error $e) { echo $e->getMessage(), "\n"; }
  try { $b(-1); } catch (Error $e) { echo $e->getMessage(), "\n"; }
  try { $c(); } catch (Error $e) { echo $e->getMessage(), "\n"; }
}
g(1);
--EXPECTF--
func_get_args() cannot be called from the global scope
func_get_arg() cannot be called from the global scope
func_get_arg(): Argument #1 ($position) must be greater than or equal to 0

Deprecated: Implicit conversion from float 1.5 to int loses precision in %s on line 7
func_get_arg() cannot be called from the global scope
func_num_args() must be called from a function context
func_get_args() expects exactly 0 arguments, 1 given
Cannot call func_get_args() dynamically
Cannot call func_get_arg() dynamically
func_get_arg(): Argument #1 ($position) must be greater than or equal to 0
Cannot call func_num_args() dynamically
