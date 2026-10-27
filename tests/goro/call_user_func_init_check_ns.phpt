--TEST--
ZEND_INIT_USER_CALL in a namespace: only for a fully qualified call_user_func(); autoload failure chains
--FILE--
<?php
namespace N;
class P { private function priv() {} }
function a() { echo "N\\a()\n"; return 1; }
try { call_user_func([new P,'priv'], a()); } catch (\Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { \call_user_func([new P,'priv'], a()); } catch (\Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
spl_autoload_register(function ($c) { echo "load $c\n"; throw new \Exception("nope $c"); });
try { \call_user_func(['Missing', 'm'], a()); } catch (\Throwable $e) { echo get_class($e), ": ", $e->getMessage(), " / ", get_class($e->getPrevious()), ": ", $e->getPrevious()->getMessage(), "\n"; }
try { \call_user_func_array('Missing2::m', [a()]); } catch (\Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
var_dump(\call_user_func('strtoupper', 'ok'), \call_user_func_array('max', [1, 3]));
--EXPECT--
N\a()
TypeError: call_user_func(): Argument #1 ($callback) must be a valid callback, cannot access private method N\P::priv()
TypeError: call_user_func(): Argument #1 ($callback) must be a valid callback, cannot access private method N\P::priv()
load Missing
TypeError: call_user_func(): Argument #1 ($callback) must be a valid callback, class "Missing" not found / Exception: nope Missing
load Missing2
TypeError: call_user_func_array(): Argument #1 ($callback) must be a valid callback, class "Missing2" not found
string(2) "OK"
int(3)
