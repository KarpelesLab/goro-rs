--TEST--
php://memory / php://temp: SEEK_CUR clamps to PHP_INT_MAX; reading past the end of an in-memory stream is EOF (a spilled temp file reports the read error, named after the SplFileObject method)
--FILE--
<?php
$t = new SplTempFileObject(); var_dump($t->fseek(PHP_INT_MAX), $t->ftell(), $t->fgetc(), $t->eof());
$t = new SplTempFileObject(0); $t->fwrite("abc"); var_dump($t->fseek(PHP_INT_MAX), $t->ftell(), $t->fgetc(), $t->eof());
$t = new SplTempFileObject(0); $t->fwrite("abc"); var_dump($t->fseek(100), $t->ftell(), $t->fgetc(), $t->eof(), $t->fwrite("x"), $t->ftell());
foreach (['php://temp', 'php://memory', 'php://temp/maxmemory:0'] as $u) {
  $f = fopen($u, 'w+'); fwrite($f, "abc");
  var_dump(fseek($f, PHP_INT_MAX), ftell($f), fgetc($f), fread($f, 10), feof($f), fseek($f, 10), ftell($f), fgetc($f), fwrite($f, "z"), ftell($f));
  rewind($f); var_dump(bin2hex(stream_get_contents($f)));
  var_dump(fseek($f, -1), fseek($f, -1, SEEK_END), ftell($f), fseek($f, PHP_INT_MAX, SEEK_CUR), ftell($f));
}
$f = fopen(__FILE__, 'r'); var_dump(fseek($f, PHP_INT_MAX), ftell($f), fgetc($f), feof($f));
--EXPECTF--
int(0)
int(9223372036854775807)
bool(false)
bool(true)

Notice: SplFileObject::fgetc(): Read of 8192 bytes failed with errno=22 Invalid argument in %s on line 3
int(0)
int(9223372036854775807)
bool(false)
bool(true)
int(0)
int(100)
bool(false)
bool(true)
int(1)
int(101)
int(0)
int(9223372036854775807)
bool(false)
string(0) ""
bool(true)
int(0)
int(10)
bool(false)
int(1)
int(11)
string(22) "616263000000000000007a"
int(-1)
int(0)
int(10)
int(0)
int(9223372036854775807)
int(0)
int(9223372036854775807)
bool(false)
string(0) ""
bool(true)
int(0)
int(10)
bool(false)
int(1)
int(11)
string(22) "616263000000000000007a"
int(-1)
int(0)
int(10)
int(0)
int(9223372036854775807)

Notice: fgetc(): Read of 8192 bytes failed with errno=22 Invalid argument in %s on line 7

Notice: fread(): Read of 8192 bytes failed with errno=22 Invalid argument in %s on line 7
int(0)
int(9223372036854775807)
bool(false)
bool(false)
bool(true)
int(0)
int(10)
bool(false)
int(1)
int(11)
string(22) "616263000000000000007a"
int(-1)
int(0)
int(10)
int(0)
int(9223372036854775807)

Notice: fgetc(): Read of 8192 bytes failed with errno=22 Invalid argument in %s on line 11
int(0)
int(9223372036854775807)
bool(false)
bool(true)
