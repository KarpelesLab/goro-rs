--TEST--
fwrite(STDOUT) on a full non-blocking pipe returns the partial count
--FILE--
<?php
$code = 'stream_set_blocking(STDOUT, false); $n = fwrite(STDOUT, str_repeat("*", 200000)); fwrite(STDERR, "wrote $n\n");';
$p = proc_open([PHP_BINARY, '-r', $code], [1 => ['pipe', 'w'], 2 => ['pipe', 'w']], $pipes);
usleep(300000);
$out = stream_get_contents($pipes[1]);
$err = stream_get_contents($pipes[2]);
proc_close($p);
var_dump(strlen($out), $err === "wrote " . strlen($out) . "\n", strlen($out) < 200000);

?>
--EXPECT--
int(65536)
bool(true)
bool(true)
