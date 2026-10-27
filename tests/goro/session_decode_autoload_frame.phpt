--TEST--
Session decoding runs under session_start()'s frame
--FILE--
<?php
class H implements SessionHandlerInterface {
	public function close(): bool { return true; }
	public function destroy(string $id): bool { return true; }
	public function gc(int $max_lifetime): int|false { return 0; }
	public function open(string $path, string $name): bool { return true; }
	public function read(string $id): string|false { return 'test|O:4:"Test":0:{}'; }
	public function write($id, $data): bool { return true; }
}
session_set_save_handler(new H());
spl_autoload_register(function($c) { throw new Exception("auto $c"); });
try { session_start(); } catch (Exception $e) { echo $e->getMessage(), "\n", $e->getTraceAsString(), "\n"; }
spl_autoload_register(function($c) { echo (new Exception)->getTraceAsString(), "\n"; }, true, true);
var_dump(unserialize('O:5:"Test2":0:{}'));
--EXPECTF--

Warning: session_start(): Failed to decode session object. Session has been destroyed in %s on line 12
auto Test
#0 [internal function]: {closure:%s:11}('Test')
#1 %s(12): session_start()
#2 {main}
#0 [internal function]: {closure:%s:13}('Test2')
#1 %s(14): unserialize('O:5:"Test2":0:{...')
#2 {main}

Fatal error: Uncaught Exception: auto Test2 in %s:11
Stack trace:
#0 [internal function]: {closure:%s:11}('Test2')
#1 %s(14): unserialize('O:5:"Test2":0:{...')
#2 {main}
  thrown in %s on line 11
