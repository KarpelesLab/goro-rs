--TEST--
file_get_contents(): a negative offset before the start fails to seek; file(): empty and NUL paths; file_get_contents() of a directory asks for its stat size plus a chunk
--FILE--
<?php
$d = sys_get_temp_dir() . "/fgc_neg_" . getmypid();
@mkdir($d);
file_put_contents("$d/f", "frommem");
var_dump(file_get_contents("$d/f", false, null, -3));
var_dump(file_get_contents("$d/f", false, null, -7));
var_dump(file_get_contents("$d/f", false, null, -100));
var_dump(file_get_contents("$d/f", false, null, 100));
try { file(""); } catch (ValueError $e) { echo $e->getMessage(), "\n"; }
try { file("a\0b"); } catch (ValueError $e) { echo $e->getMessage(), "\n"; }
$size = filesize($d);
ob_start();
var_dump(file_get_contents($d), file($d));
$out = ob_get_clean();
echo str_replace((string)($size > 0 ? $size + 8192 : 8192), "N", $out);
unlink("$d/f");
rmdir($d);
?>
--EXPECTF--
string(3) "mem"
string(7) "frommem"

Warning: file_get_contents(): Failed to seek to position -100 in the stream in %s on line %d
bool(false)
string(0) ""
Path must not be empty
file(): Argument #1 ($filename) must not contain any null bytes

Notice: file_get_contents(): Read of N bytes failed with errno=21 Is a directory in %s on line %d

Notice: file(): Read of 8192 bytes failed with errno=21 Is a directory in %s on line %d
string(0) ""
array(0) {
}
