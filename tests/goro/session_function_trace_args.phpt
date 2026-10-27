--TEST--
Session functions' frames in exception traces show their arguments
--FILE--
<?php
function t($f) { try { $f(); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n", $e->getTraceAsString(), "\n"; } }
t(fn() => session_name("a\0b"));
t(fn() => session_save_path("a\0b"));
t(fn() => session_id("a\0b"));
t(fn() => session_name([]));
t(fn() => session_module_name("a\0b"));
t(fn() => session_cache_limiter([]));
t(fn() => session_set_cookie_params(['lifetime' => 1, 'bogus' => 2]));
t(fn() => session_create_id("a\0"));
t(fn() => session_decode([]));
t(fn() => session_start(['bogus_opt' => 1, 'read_and_close' => true]));
t(fn() => session_start(5));
?>
--EXPECTF--
ValueError: session_name(): Argument #1 ($name) must not contain any null bytes
#0 %s(3): session_name('a\x00b')
#1 %s(2): {closure:%s:3}()
#2 %s(3): t(Object(Closure))
#3 {main}
ValueError: session_save_path(): Argument #1 ($path) must not contain any null bytes
#0 %s(4): session_save_path('a\x00b')
#1 %s(2): {closure:%s:4}()
#2 %s(4): t(Object(Closure))
#3 {main}

Warning: session_id(): Session ID cannot be changed after headers have already been sent (sent from %s on line 2) in %s on line 5
TypeError: session_name(): Argument #1 ($name) must be of type ?string, array given
#0 %s(6): session_name(Array)
#1 %s(2): {closure:%s:6}()
#2 %s(6): t(Object(Closure))
#3 {main}

Warning: session_module_name(): Session save handler module cannot be changed after headers have already been sent (sent from %s on line 2) in %s on line 7
TypeError: session_cache_limiter(): Argument #1 ($value) must be of type ?string, array given
#0 %s(8): session_cache_limiter(Array)
#1 %s(2): {closure:%s:8}()
#2 %s(8): t(Object(Closure))
#3 {main}

Warning: session_set_cookie_params(): Session cookie parameters cannot be changed after headers have already been sent (sent from %s on line 2) in %s on line 9
ValueError: session_create_id(): Argument #1 ($prefix) must not contain any null bytes
#0 %s(10): session_create_id('a\x00')
#1 %s(2): {closure:%s:10}()
#2 %s(10): t(Object(Closure))
#3 {main}
TypeError: session_decode(): Argument #1 ($data) must be of type string, array given
#0 %s(11): session_decode(Array)
#1 %s(2): {closure:%s:11}()
#2 %s(11): t(Object(Closure))
#3 {main}

Warning: session_start(): Session cannot be started after headers have already been sent (sent from %s on line 2) in %s on line 12
TypeError: session_start(): Argument #1 ($options) must be of type array, int given
#0 %s(13): session_start(5)
#1 %s(2): {closure:%s:13}()
#2 %s(13): t(Object(Closure))
#3 {main}
