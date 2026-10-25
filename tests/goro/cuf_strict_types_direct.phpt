--TEST--
call_user_func() compiled into strict_types code checks its callee's arguments strictly
--FILE--
<?php
declare(strict_types=1);
try { var_dump(call_user_func('strlen', 5)); } catch (TypeError $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { var_dump(call_user_func_array('strlen', [5])); } catch (TypeError $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { var_dump(array_map('strlen', [5])); } catch (TypeError $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
function u(string $s) { return $s; }
try { var_dump(call_user_func('u', 5)); } catch (TypeError $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
--EXPECTF--
TypeError: strlen(): Argument #1 ($string) must be of type string, int given
TypeError: strlen(): Argument #1 ($string) must be of type string, int given
array(1) {
  [0]=>
  int(1)
}
TypeError: u(): Argument #1 ($s) must be of type string, int given, called in %s on line 7
