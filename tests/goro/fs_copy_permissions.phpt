--TEST--
copy() does not copy the source's permissions
--FILE--
<?php
// copy(): a new destination gets 0666 & ~umask, an existing one keeps its permissions.
$d = sys_get_temp_dir() . "/r3cp_" . getmypid(); @mkdir($d);
file_put_contents("$d/a", "x"); chmod("$d/a", 0700); touch("$d/a", 1700000000, 1700000100);
var_dump(copy("$d/a", "$d/b")); clearstatcache();
var_dump(decoct(fileperms("$d/b") & 0777), filemtime("$d/b") == 1700000000);
file_put_contents("$d/c", "old"); chmod("$d/c", 0600); copy("$d/a", "$d/c"); clearstatcache(); var_dump(decoct(fileperms("$d/c") & 0777));
unlink("$d/a"); unlink("$d/b"); unlink("$d/c"); rmdir($d);
--EXPECT--
bool(true)
string(3) "644"
bool(false)
string(3) "600"
