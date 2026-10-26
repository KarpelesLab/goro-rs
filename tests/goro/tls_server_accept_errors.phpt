--TEST--
TLS server accept errors: plain HTTP on the TLS port, a client below the server's minimum version
--EXTENSIONS--
openssl
pcntl
--FILE--
<?php
$key = openssl_pkey_new(['private_key_type' => OPENSSL_KEYTYPE_EC, 'curve_name' => 'prime256v1']);
$crt = openssl_csr_sign(openssl_csr_new(['commonName' => 'localhost'], $key), null, $key, 30, [], 1);
openssl_x509_export($crt, $pem);
openssl_pkey_export($key, $kpem);
$file = sys_get_temp_dir() . '/goro_tls_accept_' . getmypid() . '.pem';
file_put_contents($file, $pem . $kpem);
$scenarios = [
    'GET' => function ($p) { $c = stream_socket_client("tcp://127.0.0.1:$p"); fwrite($c, "GET / HTTP/1.0\r\n\r\n"); usleep(300000); },
    'POST' => function ($p) { $c = stream_socket_client("tcp://127.0.0.1:$p"); fwrite($c, "POST /x HTTP/1.1\r\n\r\n"); usleep(300000); },
    'PUT' => function ($p) { $c = stream_socket_client("tcp://127.0.0.1:$p"); fwrite($c, "PUT / HTTP/1.0\r\n\r\n"); usleep(300000); },
    'garbage' => function ($p) { $c = stream_socket_client("tcp://127.0.0.1:$p"); fwrite($c, "\x16\x09\x09\x00\x05hello"); usleep(300000); },
    'oversized' => function ($p) { $c = stream_socket_client("tcp://127.0.0.1:$p"); fwrite($c, "\x16\x03\x01\x40\x05hello"); usleep(300000); },
    'CONNECT' => function ($p) { $c = stream_socket_client("tcp://127.0.0.1:$p"); fwrite($c, "CONNECT host:443 HTTP/1.1\r\n\r\n"); usleep(300000); },
    'tls12-vs-13' => function ($p) { @stream_socket_client("ssl://127.0.0.1:$p", $en, $es, 3, STREAM_CLIENT_CONNECT, stream_context_create(['ssl' => ['verify_peer' => false, 'crypto_method' => STREAM_CRYPTO_METHOD_TLSv1_2_CLIENT]])); },
];
foreach ($scenarios as $label => $client) {
    $o = ['local_cert' => $file];
    if ($label === 'tls12-vs-13') $o['crypto_method'] = STREAM_CRYPTO_METHOD_TLSv1_3_SERVER;
    $srv = stream_socket_server('ssl://127.0.0.1:0', $en, $es, STREAM_SERVER_BIND | STREAM_SERVER_LISTEN, stream_context_create(['ssl' => $o]));
    $port = (int)substr(strrchr(stream_socket_get_name($srv, false), ':'), 1);
    $pid = pcntl_fork();
    if ($pid === 0) { fclose($srv); $client($port); usleep(200000); exit(0); }
    echo "$label:\n";
    set_error_handler(function ($no, $str) { echo "  W: $str\n"; return true; });
    var_dump(stream_socket_accept($srv, 3));
    restore_error_handler();
    pcntl_waitpid($pid, $st);
    fclose($srv);
}
unlink($file);
?>
--EXPECT--
GET:
  W: stream_socket_accept(): SSL operation failed with code 1. OpenSSL Error messages:
error:0A00009C:SSL routines::http request
  W: stream_socket_accept(): Failed to enable crypto
  W: stream_socket_accept(): Accept failed: Cannot enable crypto
bool(false)
POST:
  W: stream_socket_accept(): SSL operation failed with code 1. OpenSSL Error messages:
error:0A0000C6:SSL routines::packet length too long
error:0A000139:SSL routines::record layer failure
  W: stream_socket_accept(): Failed to enable crypto
  W: stream_socket_accept(): Accept failed: Cannot enable crypto
bool(false)
PUT:
  W: stream_socket_accept(): SSL operation failed with code 1. OpenSSL Error messages:
error:0A00009C:SSL routines::http request
  W: stream_socket_accept(): Failed to enable crypto
  W: stream_socket_accept(): Accept failed: Cannot enable crypto
bool(false)
garbage:
  W: stream_socket_accept(): SSL operation failed with code 1. OpenSSL Error messages:
error:0A00010B:SSL routines::wrong version number
  W: stream_socket_accept(): Failed to enable crypto
  W: stream_socket_accept(): Accept failed: Cannot enable crypto
bool(false)
oversized:
  W: stream_socket_accept(): SSL operation failed with code 1. OpenSSL Error messages:
error:0A000092:SSL routines::data length too long
error:0A000139:SSL routines::record layer failure
  W: stream_socket_accept(): Failed to enable crypto
  W: stream_socket_accept(): Accept failed: Cannot enable crypto
bool(false)
CONNECT:
  W: stream_socket_accept(): SSL operation failed with code 1. OpenSSL Error messages:
error:0A0000C6:SSL routines::packet length too long
error:0A000139:SSL routines::record layer failure
  W: stream_socket_accept(): Failed to enable crypto
  W: stream_socket_accept(): Accept failed: Cannot enable crypto
bool(false)
tls12-vs-13:
  W: stream_socket_accept(): SSL operation failed with code 1. OpenSSL Error messages:
error:0A000102:SSL routines::unsupported protocol
  W: stream_socket_accept(): Failed to enable crypto
  W: stream_socket_accept(): Accept failed: Cannot enable crypto
bool(false)
