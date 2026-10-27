--TEST--
A fatal error inside an SPL method called through ReflectionMethod::invokeArgs() ends the script
--INI--
memory_limit=128M
fatal_error_backtraces=1
--FILE--
<?php
$f = new SplTempFileObject(); $f->fwrite("abc"); $f->rewind();
$m = new ReflectionMethod($f, 'fread');
try {
  $r = $m->invokeArgs($f, [PHP_INT_MAX]);
  echo "after: "; var_dump($r);
} catch (Throwable $e) { echo "caught ", get_class($e), "\n"; }
echo "still running\n";
--EXPECTF--

Fatal error: Allowed memory size of 134217728 bytes exhausted (tried to allocate 9223372036854775832 bytes) in %s on line 5
Stack trace:
#0 [internal function]: SplFileObject->fread(9223372036854775807)
#1 %s(5): ReflectionMethod->invokeArgs(Object(SplTempFileObject), Array)
#2 {main}
