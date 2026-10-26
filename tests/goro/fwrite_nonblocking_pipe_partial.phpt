--TEST--
fwrite() to a full non-blocking pipe reports the partial write
--FILE--
<?php
$p = proc_open([PHP_BINARY, '-r', 'usleep(200000); $in = file_get_contents("php://stdin"); echo strlen($in);'], [0 => ['pipe', 'r'], 1 => ['pipe', 'w']], $pipes);
stream_set_blocking($pipes[0], false);
$buf = str_repeat('*', 100000);
$w = fwrite($pipes[0], $buf);
var_dump($w);
$buf = substr($buf, $w);
while ($buf !== '') {
    $r = $e = []; $ww = [$pipes[0]];
    stream_select($r, $ww, $e, 1);
    $w = fwrite($pipes[0], $buf);
    if ($w === false) { echo "false\n"; break; }
    $buf = substr($buf, $w);
}
fclose($pipes[0]);
var_dump(stream_get_contents($pipes[1]));
proc_close($p);

?>
--EXPECT--
int(65536)
string(6) "100000"
