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
