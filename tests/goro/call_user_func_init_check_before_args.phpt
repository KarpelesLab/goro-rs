--TEST--
call_user_func()/call_user_func_array() check their callback before the other arguments are evaluated (ZEND_INIT_USER_CALL)
--FILE--
<?php
class P { private function priv() {} }
function a() { echo "a()\n"; return 1; }
try { call_user_func([new P,'priv'], a()); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { call_user_func_array([new P,'priv'], [a()]); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { call_user_func('nope', a()); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
--EXPECT--
TypeError: call_user_func(): Argument #1 ($callback) must be a valid callback, cannot access private method P::priv()
TypeError: call_user_func_array(): Argument #1 ($callback) must be a valid callback, cannot access private method P::priv()
TypeError: call_user_func(): Argument #1 ($callback) must be a valid callback, function "nope" not found or invalid function name
