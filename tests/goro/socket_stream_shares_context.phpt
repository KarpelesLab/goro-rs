--TEST--
Socket streams refer to the context they were created with (options set through the stream are shared)
--FILE--
<?php
$fp = fopen(__FILE__, 'r');
stream_context_set_option(stream_context_get_default(), 'q', 'r', 1);
// a plain file stream has no context: it gets a private one
var_dump(stream_context_get_options($fp));

$s = stream_socket_server('tcp://127.0.0.1:0');
$addr = 'tcp://' . stream_socket_get_name($s, false);
stream_context_set_option(stream_context_get_default(), 'q', 's', 2);
echo json_encode(stream_context_get_options($s)), "\n";
stream_context_set_option($s, 'socket', 'z', 3);
echo json_encode(stream_context_get_options(stream_context_get_default())), "\n";

$c = stream_socket_client($addr);
echo json_encode(stream_context_get_options($c)), "\n";
$a = stream_socket_accept($s);
echo json_encode(stream_context_get_options($a)), "\n";

// fsockopen() passes no context
$f = fsockopen('127.0.0.1', (int)substr(strrchr($addr, ':'), 1));
echo json_encode(stream_context_get_options($f)), "\n";

$ctx = stream_context_create(['k' => ['v' => 1]]);
$c3 = stream_socket_client($addr, $e, $es, 1, STREAM_CLIENT_CONNECT, $ctx);
stream_context_set_option($ctx, 'k', 'w', 2);
echo json_encode(stream_context_get_options($c3)), "\n";
stream_context_set_option($c3, 'k', 'x', 3);
echo json_encode(stream_context_get_options($ctx)), "\n";
?>
--EXPECT--
array(0) {
}
{"q":{"r":1,"s":2}}
{"q":{"r":1,"s":2},"socket":{"z":3}}
{"q":{"r":1,"s":2},"socket":{"z":3}}
{"q":{"r":1,"s":2},"socket":{"z":3}}
[]
{"k":{"v":1,"w":2}}
{"k":{"v":1,"w":2,"x":3}}
