--TEST--
-n together with -c still loads the -c ini file
--FILE--
<?php
$ini = tempnam(sys_get_temp_dir(), 'ini');
file_put_contents($ini, "memory_limit=\"123M\"\nprecision=13\n");
$p = proc_open([PHP_BINARY, '-n', '-c', $ini, '-r', 'var_dump(ini_get("memory_limit"), ini_get("precision"), php_ini_loaded_file() === $argv[1]);', $ini], [1 => ['pipe', 'w']], $pipes);
echo stream_get_contents($pipes[1]);
proc_close($p);
unlink($ini);

?>
--EXPECT--
string(4) "123M"
string(2) "13"
bool(true)
