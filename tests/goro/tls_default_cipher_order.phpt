--TEST--
TLS streams offer OpenSSL's default TLS 1.3 suite order (AES-256-GCM first) and PHP's default stream cipher list
--EXTENSIONS--
openssl
pcntl
--FILE--
<?php
$key = openssl_pkey_new(['private_key_type' => OPENSSL_KEYTYPE_EC, 'curve_name' => 'prime256v1']);
$crt = openssl_csr_sign(openssl_csr_new(['commonName' => 'localhost'], $key), null, $key, 30, [], 1);
openssl_x509_export($crt, $pem);
openssl_pkey_export($key, $kpem);
$file = sys_get_temp_dir() . '/goro_tls_order_' . getmypid() . '.pem';
file_put_contents($file, $pem . $kpem);

$srv = stream_socket_server('ssl://127.0.0.1:0', $en, $es, STREAM_SERVER_BIND | STREAM_SERVER_LISTEN,
    stream_context_create(['ssl' => ['local_cert' => $file]]));
$port = (int)substr(strrchr(stream_socket_get_name($srv, false), ':'), 1);
$pid = pcntl_fork();
if ($pid === 0) {
    for ($i = 0; $i < 3; $i++) {
        $c = @stream_socket_accept($srv, 10);
        if ($c) { fwrite($c, "x"); fgets($c); fclose($c); }
    }
    exit(0);
}
fclose($srv);
foreach ([null, STREAM_CRYPTO_METHOD_TLSv1_2_CLIENT, STREAM_CRYPTO_METHOD_TLSv1_3_CLIENT] as $m) {
    $o = ['verify_peer' => false, 'verify_peer_name' => false];
    if ($m) $o['crypto_method'] = $m;
    $c = stream_socket_client("ssl://127.0.0.1:$port", $en, $es, 5, STREAM_CLIENT_CONNECT, stream_context_create(['ssl' => $o]));
    fread($c, 1);
    $md = stream_get_meta_data($c)['crypto'];
    echo $md['protocol'], ' ', $md['cipher_name'], ' ', $md['cipher_bits'], "\n";
    fwrite($c, "bye\n");
    fclose($c);
}
pcntl_waitpid($pid, $st);
unlink($file);
?>
--EXPECT--
TLSv1.3 TLS_AES_256_GCM_SHA384 256
TLSv1.2 ECDHE-ECDSA-AES128-GCM-SHA256 128
TLSv1.3 TLS_AES_256_GCM_SHA384 256
