--TEST--
Failed opens report their errno (Permission denied, Is a directory); fstat() of memory, temp and data: streams
--SKIPIF--
<?php if (function_exists('posix_geteuid') && posix_geteuid() == 0) die("skip not for root"); ?>
--FILE--
<?php
$d = sys_get_temp_dir() . "/open_errno_" . getmypid();
@mkdir($d);
var_dump(file_put_contents($d, "x"));
file_put_contents("$d/f", "abc");
chmod("$d/f", 0);
var_dump(file_get_contents("$d/f"), file("$d/f"), readfile("$d/f"));
chmod("$d/f", 0644);
unlink("$d/f");
rmdir($d);
$m = fopen("php://memory", "w+");
fwrite($m, "hello");
$st = fstat($m);
echo json_encode(array_slice($st, 13)), "\n";
$st = fstat(fopen("php://memory", "r"));
var_dump($st["mode"], $st["size"]);
$st = fstat(fopen("data:,abc", "r"));
var_dump($st["mode"], $st["size"], $st["dev"]);
$t = fopen("php://temp", "w+");
fwrite($t, "12345678");
var_dump(fstat($t)["size"], fstat($t)["blksize"]);
?>
--EXPECTF--
Warning: file_put_contents(%s): Failed to open stream: Is a directory in %s on line %d
bool(false)

Warning: file_get_contents(%s): Failed to open stream: Permission denied in %s on line %d

Warning: file(%s): Failed to open stream: Permission denied in %s on line %d

Warning: readfile(%s): Failed to open stream: Permission denied in %s on line %d
bool(false)
bool(false)
bool(false)
{"dev":12,"ino":0,"mode":33206,"nlink":1,"uid":0,"gid":0,"rdev":-1,"size":5,"atime":0,"mtime":0,"ctime":0,"blksize":-1,"blocks":-1}
int(33060)
int(0)
int(33206)
int(3)
int(12)
int(8)
int(-1)
