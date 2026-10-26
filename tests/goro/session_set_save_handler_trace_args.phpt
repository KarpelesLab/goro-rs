--TEST--
session_set_save_handler()'s frame in exception traces has its arguments
--FILE--
<?php
try { session_set_save_handler(1, 2); } catch (Throwable $e) { echo $e->getMessage(), "\n", $e->getTraceAsString(), "\n"; }
try { session_set_save_handler('a', 'b', 'c', 'd', 'e', 'f'); } catch (Throwable $e) { echo $e->getMessage(), "\n", $e->getTraceAsString(), "\n"; }
try { session_set_save_handler('strlen', 'b', 'c', 'd', 'e', 'f'); } catch (Throwable $e) { echo $e->getMessage(), "\n", $e->getTraceAsString(), "\n"; }
try { session_set_cookie_params([], 5); } catch (Throwable $e) { echo $e->getMessage(), "\n", $e->getTraceAsString(), "\n"; }
--EXPECTF--
session_set_save_handler(): Argument #1 ($open) must be of type SessionHandlerInterface, int given
#0 %s(2): session_set_save_handler(1, 2)
#1 {main}

Deprecated: session_set_save_handler(): Providing individual callbacks instead of an object implementing SessionHandlerInterface is deprecated in %s on line 3
session_set_save_handler(): Argument #1 ($open) must be a valid callback, function "a" not found or invalid function name
#0 %s(3): session_set_save_handler('a', 'b', 'c', 'd', 'e', 'f')
#1 {main}

Deprecated: session_set_save_handler(): Providing individual callbacks instead of an object implementing SessionHandlerInterface is deprecated in %s on line 4
session_set_save_handler(): Argument #2 ($close) must be a valid callback, function "b" not found or invalid function name
#0 %s(4): session_set_save_handler('strlen', 'b', 'c', 'd', 'e', 'f')
#1 {main}

Warning: session_set_cookie_params(): Session cookie parameters cannot be changed after headers have already been sent (sent from %s on line 2) in %s on line 5
