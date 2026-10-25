--TEST--
rewind() / fseek() of a pipe or socket: forward moves within the buffer, else "Stream does not support seeking"
--FILE--
<?php
$p = popen('echo hello world', 'r');
var_dump(fread($p, 2));
var_dump(fseek($p, 1, SEEK_CUR), fread($p, 2));
var_dump(rewind($p));
var_dump(fread($p, 100));
pclose($p);
$s = stream_socket_pair(STREAM_PF_UNIX, STREAM_SOCK_STREAM, 0);
fwrite($s[0], "0123456789");
var_dump(fread($s[1], 2), ftell($s[1]));
var_dump(fseek($s[1], 5), ftell($s[1]), fread($s[1], 2));
var_dump(rewind($s[1]));
var_dump(stream_get_contents($s[1], 2, 1));
var_dump(stream_get_contents($s[1], -1, 0));
?>
--EXPECTF--
string(2) "he"
int(0)
string(2) "lo"

Warning: rewind(): Stream does not support seeking in %s on line %d
bool(false)
string(7) " world
"
string(2) "01"
int(2)
int(0)
int(5)
string(2) "56"

Warning: rewind(): Stream does not support seeking in %s on line %d
bool(false)

Warning: stream_get_contents(): Stream does not support seeking in %s on line %d

Warning: stream_get_contents(): Failed to seek to position 1 in the stream in %s on line %d
bool(false)

Warning: stream_get_contents(): Stream does not support seeking in %s on line %d

Warning: stream_get_contents(): Failed to seek to position 0 in the stream in %s on line %d
bool(false)
