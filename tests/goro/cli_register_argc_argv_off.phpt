--TEST--
CLI: the arguments are registered ($argv, $argc, $_SERVER) even with register_argc_argv=0
--FILE--
<?php
$p = proc_open([PHP_BINARY, '-n', '-d', 'register_argc_argv=0', '-r', 'var_dump($argc, count($argv), $_SERVER["argc"], $_SERVER["argv"][1]);', 'x'], [1 => ['pipe', 'w'], 2 => ['pipe', 'w']], $pipes);
echo stream_get_contents($pipes[1]), stream_get_contents($pipes[2]), "rc=", proc_close($p), "\n";
?>
--EXPECT--
int(2)
int(2)
int(2)
string(1) "x"
rc=0
