<?php
$p = stream_socket_pair(STREAM_PF_UNIX, STREAM_SOCK_STREAM, 0);
stream_set_blocking($p[1], false);
var_dump(fread($p[1], 10), fgets($p[1]));
fwrite($p[0], "line1\nline2");
var_dump(fgets($p[1]), fgets($p[1]), fgets($p[1]));
var_dump(stream_set_chunk_size($p[1], 4));
var_dump(stream_set_chunk_size($p[1], 3));
fwrite($p[0], "0123456789");
var_dump(fread($p[1], 100), fread($p[1], 100), fread($p[1], 100));
// (a fresh read buffer grows by a chunk at a time)
$q = stream_socket_pair(STREAM_PF_UNIX, STREAM_SOCK_STREAM, 0);
stream_set_chunk_size($q[1], 3);
fwrite($q[0], "0123456789");
var_dump(fread($q[1], 100), fread($q[1], 100), fread($q[1], 100));
$f = fopen(__FILE__, 'r');
var_dump(stream_set_chunk_size($f, 10), stream_set_chunk_size($f, 8192));
var_dump(strlen(fread($f, 100)));
// the first php://input creates the request body (a temp stream)
function next_id() { $x = fopen('php://memory', 'r'); return (int)$x; }
$n = next_id();
var_dump(file_get_contents('php://input'));
var_dump(next_id() - $n);
$n = next_id();
$i = fopen('php://input', 'r');
var_dump(next_id() - $n);
?>
