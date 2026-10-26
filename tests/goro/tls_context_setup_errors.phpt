--TEST--
TLS client setup failures: local_cert/local_pk error queue, key mismatch, cipher strings, cafile warnings
--EXTENSIONS--
openssl
pcntl
--FILE--
<?php
$dir = sys_get_temp_dir() . '/goro_tls_setup_' . getmypid();
@mkdir($dir);
$k1 = openssl_pkey_new(['private_key_type' => OPENSSL_KEYTYPE_EC, 'curve_name' => 'prime256v1']);
$k2 = openssl_pkey_new(['private_key_type' => OPENSSL_KEYTYPE_EC, 'curve_name' => 'prime256v1']);
$crt = openssl_csr_sign(openssl_csr_new(['commonName' => 'localhost'], $k1), null, $k1, 30, [], 1);
openssl_x509_export($crt, $pem);
openssl_pkey_export($k2, $k2pem);
file_put_contents("$dir/crt.pem", $pem);
file_put_contents("$dir/k2.pem", $k2pem);

$cases = [
    'cert-missing' => ['local_cert' => "$dir/nonexistent.pem"],
    'cert-notpem' => ['local_cert' => __FILE__],
    'pk-missing' => ['local_cert' => "$dir/crt.pem", 'local_pk' => "$dir/nonexistent.key"],
    'no-key' => ['local_cert' => "$dir/crt.pem"],
    'key-mismatch' => ['local_cert' => "$dir/crt.pem", 'local_pk' => "$dir/k2.pem"],
    'ciphers-bogus' => ['ciphers' => 'BOGUS'],
    'ciphers-lowercase' => ['ciphers' => 'high'],
    'ciphers-none' => ['ciphers' => '!ALL'],
    'ciphers-empty' => ['ciphers' => ''],
    'ciphers-mixed' => ['ciphers' => 'BOGUS:ECDHE-RSA-AES128-GCM-SHA256'],
    'cafile-missing' => ['cafile' => "$dir/nonexistent-ca.pem"],
    'cafile-notpem' => ['cafile' => __FILE__],
];
$ls = stream_socket_server('tcp://127.0.0.1:0');
$port = (int)substr(strrchr(stream_socket_get_name($ls, false), ':'), 1);
$pid = pcntl_fork();
if ($pid === 0) {
    for ($i = 0; $i < count($cases); $i++) { $c = @stream_socket_accept($ls, 5); if ($c) fclose($c); }
    exit(0);
}
set_error_handler(function ($no, $str) use ($dir, $port) {
    $str = str_replace([$dir, __FILE__, (string)$port], ['DIR', 'FILE', 'PORT'], $str);
    if (!str_contains($str, 'Unable to connect') && !str_contains($str, 'Failed to enable crypto')) echo "  W: $str\n";
    return true;
});
foreach ($cases as $label => $o) {
    echo "$label:\n";
    $c = stream_socket_client("ssl://127.0.0.1:$port", $en, $es, 2, STREAM_CLIENT_CONNECT, stream_context_create(['ssl' => $o]));
    while ($m = openssl_error_string()) echo "  ssl: $m\n";
}
pcntl_waitpid($pid, $st);
unlink("$dir/crt.pem"); unlink("$dir/k2.pem"); rmdir($dir);
?>
--EXPECT--
cert-missing:
  W: stream_socket_client(): Unable to set local cert chain file `DIR/nonexistent.pem'; Check that your cafile/capath settings include details of your certificate and its issuer
  ssl: error:FFFFFFFF80000002:system library::No such file or directory
  ssl: error:10080002:BIO routines::system lib
  ssl: error:0A080002:SSL routines::system lib
cert-notpem:
  W: stream_socket_client(): Unable to set local cert chain file `FILE'; Check that your cafile/capath settings include details of your certificate and its issuer
  ssl: error:0480006C:PEM routines::no start line
  ssl: error:0A080009:SSL routines::PEM lib
pk-missing:
  W: stream_socket_client(): Unable to set private key file `DIR/nonexistent.key'
  ssl: error:FFFFFFFF80000002:system library::No such file or directory
  ssl: error:10080002:BIO routines::system lib
  ssl: error:0A080002:SSL routines::system lib
no-key:
  W: stream_socket_client(): Unable to set private key file `DIR/crt.pem'
  ssl: error:1E08010C:DECODER routines::unsupported
  ssl: error:0A080009:SSL routines::PEM lib
key-mismatch:
  W: stream_socket_client(): Unable to set private key file `DIR/k2.pem'
  ssl: error:05800074:x509 certificate routines::key values mismatch
ciphers-bogus:
  ssl: error:0A0000B9:SSL routines::no cipher match
ciphers-lowercase:
  ssl: error:0A0000B9:SSL routines::no cipher match
ciphers-none:
  ssl: error:0A0000B9:SSL routines::no cipher match
ciphers-empty:
  ssl: error:0A0000B9:SSL routines::no cipher match
ciphers-mixed:
cafile-missing:
  W: failed loading cafile stream: `DIR/nonexistent-ca.pem'
cafile-notpem:
  W: no valid certs found cafile stream: `FILE'
