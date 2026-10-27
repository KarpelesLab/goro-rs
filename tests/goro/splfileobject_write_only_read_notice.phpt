--TEST--
SplFileObject::fgets()/fgetc() on a write-only file: the read notice names the method
--FILE--
<?php
$fn = __DIR__ . '/t26.tmp';
$o = new SplFileObject($fn, 'w');
var_dump($o->fgetc());
try { var_dump($o->fgets()); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
$f = fopen($fn, 'w'); var_dump(fgetc($f), fgets($f), fread($f, 3)); fclose($f);
unset($o);
unlink($fn);
--EXPECTF--

Notice: SplFileObject::fgetc(): Read of 8192 bytes failed with errno=9 Bad file descriptor in %s on line 4
bool(false)

Notice: SplFileObject::fgets(): Read of 8192 bytes failed with errno=9 Bad file descriptor in %s on line 5
string(0) ""

Notice: fgetc(): Read of 8192 bytes failed with errno=9 Bad file descriptor in %s on line 6

Notice: fgets(): Read of 8192 bytes failed with errno=9 Bad file descriptor in %s on line 6

Notice: fread(): Read of 8192 bytes failed with errno=9 Bad file descriptor in %s on line 6
bool(false)
bool(false)
bool(false)
