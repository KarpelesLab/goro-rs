--TEST--
SplFileInfo::openFile(): NAN useIncludePath warns, the new object keeps the source's path; ArrayObject/ArrayIterator flags keep the low 16 bits (~SPL_ARRAY_INT_MASK)
--FILE--
<?php
$t = new SplTempFileObject(100);
try { var_dump(get_class($t->openFile('r', NAN))); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
foreach ([new SplTempFileObject(100), new SplTempFileObject(), new SplTempFileObject(0), new SplFileInfo('php://memory')] as $t) {
  var_dump($t->getFilename(), $t->getPathname(), $t->getPath(), $t->getBasename());
  try { $o = $t->openFile('r'); var_dump($o->getFilename(), $o->getPathname()); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
}
foreach ([PHP_INT_MIN, -1, 0x10000, 0x1FFFF, PHP_INT_MAX, 3] as $f) {
  $a = new ArrayIterator([1], $f); var_dump($a->getFlags());
  $a->setFlags($f); var_dump($a->getFlags());
  $o = new ArrayObject([1], $f); var_dump($o->getFlags());
  $o->setFlags($f); var_dump($o->getFlags());
  $r = new RecursiveArrayIterator([1], $f); var_dump($r->getFlags());
}
$a = new ArrayObject([1], -1); var_dump(serialize($a));
--EXPECTF--

Warning: unexpected NAN value was coerced to bool in %s on line 3
string(13) "SplFileObject"
string(24) "php://temp/maxmemory:100"
string(24) "php://temp/maxmemory:100"
string(0) ""
string(13) "maxmemory:100"
string(24) "php://temp/maxmemory:100"
string(24) "php://temp/maxmemory:100"
string(10) "php://temp"
string(10) "php://temp"
string(0) ""
string(4) "temp"
string(10) "php://temp"
string(10) "php://temp"
string(22) "php://temp/maxmemory:0"
string(22) "php://temp/maxmemory:0"
string(0) ""
string(11) "maxmemory:0"
string(22) "php://temp/maxmemory:0"
string(22) "php://temp/maxmemory:0"
string(6) "memory"
string(12) "php://memory"
string(5) "php:/"
string(6) "memory"
string(6) "memory"
string(12) "php://memory"
int(0)
int(0)
int(0)
int(0)
int(0)
int(65535)
int(65535)
int(65535)
int(65535)
int(65535)
int(0)
int(0)
int(0)
int(0)
int(0)
int(65535)
int(65535)
int(65535)
int(65535)
int(65535)
int(65535)
int(65535)
int(65535)
int(65535)
int(65535)
int(3)
int(3)
int(3)
int(3)
int(3)
string(69) "O:11:"ArrayObject":4:{i:0;i:65535;i:1;a:1:{i:0;i:1;}i:2;a:0:{}i:3;N;}"
