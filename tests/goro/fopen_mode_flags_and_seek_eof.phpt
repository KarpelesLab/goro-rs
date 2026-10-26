--TEST--
fopen(): only the first mode character and '+' matter ("rw" is read-only); a failed fseek() keeps EOF; php://filter/ without a resource
--FILE--
<?php
$f = tempnam(sys_get_temp_dir(), "rw");
file_put_contents($f, "hello");
$h = fopen($f, "rw");
var_dump(fwrite($h, "XY"));
var_dump(fread($h, 10));
fclose($h);
$h = fopen($f, "we");
var_dump(fwrite($h, "ab"), fread($h, 10));
fclose($h);
var_dump(file_get_contents($f));
unlink($f);

$m = fopen("php://memory", "w+");
fwrite($m, "abc");
rewind($m);
stream_get_contents($m);
var_dump(feof($m), fseek($m, -1), feof($m), fseek($m, 1), feof($m));

try { fopen("php://filter/", "r"); } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { fopen("", "r"); } catch (ValueError $e) { echo $e->getMessage(), "\n"; }
?>
--EXPECTF--
Notice: fwrite(): Write of 2 bytes failed with errno=9 Bad file descriptor in %s on line %d
bool(false)
string(5) "hello"

Notice: fread(): Read of 8192 bytes failed with errno=9 Bad file descriptor in %s on line %d
int(2)
bool(false)
string(2) "ab"
bool(true)
int(-1)
bool(true)
int(0)
bool(false)
Error: No URL resource specified
Path must not be empty
