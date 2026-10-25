--TEST--
socket_sendto(): a failed host lookup reports -10000 - h_errno (gethostbyname_r() leaves h_errno alone)
--EXTENSIONS--
sockets
--FILE--
<?php
$s = socket_create(AF_INET, SOCK_DGRAM, SOL_UDP);
var_dump(socket_sendto($s, "x", 1, 0, 'not an ip', 46021));
var_dump(socket_last_error($s));
?>
--EXPECTF--
Warning: socket_sendto(): Host lookup failed [-10000]: Unknown error -10000 in %s on line %d
bool(false)
int(-10000)
