--TEST--
A transport whose connect or bind fails has used up a resource id
--FILE--
<?php
function next_id() { $x = fopen('php://memory', 'r'); $id = (int)$x; fclose($x); return $id; }
$base = next_id();
$s = stream_socket_server("tcp://127.0.0.1:0");
$port = (int)substr(strrchr(stream_socket_get_name($s, false), ':'), 1);
fclose($s);
$n = next_id();
@stream_socket_client("tcp://127.0.0.1:$port");
echo next_id() - $n, "\n";
$n = next_id();
@stream_socket_server("tcp://127.0.0.1:{$port}x");
echo next_id() - $n, "\n";
$n = next_id();
@stream_socket_client("unix:///nonexistent/goro.sock");
echo next_id() - $n, "\n";
$n = next_id();
@fsockopen("127.0.0.1", $port);
echo next_id() - $n, "\n";
$n = next_id();
@stream_socket_client("foo://127.0.0.1:$port");
echo next_id() - $n, "\n";
?>
--EXPECT--
2
2
2
2
1
