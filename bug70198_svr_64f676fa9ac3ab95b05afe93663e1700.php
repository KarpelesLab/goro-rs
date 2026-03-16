<?php
$socket = stream_socket_server('tcp://127.0.0.1:8964', $errno, $errstr);

if (!$socket) {
    echo "$errstr ($errno)\n";
} else {
    if ($conn = stream_socket_accept($socket, 3)) {
        sleep(1);
        /* just close the connection immediately after accepting,
            the client side will need wait a bit longer to realize it.*/
        fclose($conn);
    }
    fclose($socket);
}