<?php
// Make a self-signed certificate for the sample's HTTPS listener with
// ext/openssl: php mkcert.php CERT_FILE KEY_FILE [HOST...]
if ($argc < 3) {
    fwrite(STDERR, "usage: mkcert.php CERT_FILE KEY_FILE [HOST...]\n");
    exit(1);
}
[, $certFile, $keyFile] = $argv;
$hosts = array_slice($argv, 3) ?: ['localhost', '127.0.0.1'];

$san = implode(',', array_map(
    fn ($h) => (filter_var($h, FILTER_VALIDATE_IP) ? 'IP:' : 'DNS:') . $h,
    $hosts
));
$config = tempnam(sys_get_temp_dir(), 'mkcert');
file_put_contents($config, <<<CNF
[req]
distinguished_name = dn
[dn]
[v3]
basicConstraints = critical,CA:FALSE
keyUsage = critical,digitalSignature,keyEncipherment
extendedKeyUsage = serverAuth
subjectAltName = $san
CNF);
$opts = ['config' => $config, 'digest_alg' => 'sha256', 'x509_extensions' => 'v3'];

$key = openssl_pkey_new(['private_key_type' => OPENSSL_KEYTYPE_EC, 'curve_name' => 'prime256v1']);
$csr = openssl_csr_new(['commonName' => $hosts[0], 'organizationName' => 'goro sample'], $key, $opts);
$cert = openssl_csr_sign($csr, null, $key, 825, $opts, random_int(1, PHP_INT_MAX));
unlink($config);
if ($cert === false) {
    fwrite(STDERR, "cannot make the certificate: " . openssl_error_string() . "\n");
    exit(1);
}
openssl_x509_export($cert, $certPem);
openssl_pkey_export($key, $keyPem, null, ['config' => '/dev/null']);
file_put_contents($certFile, $certPem);
file_put_contents($keyFile, $keyPem);
chmod($keyFile, 0600);
$info = openssl_x509_parse($certPem);
printf("Made %s (%s, valid until %s) and %s\n", $certFile, $info['extensions']['subjectAltName'] ?? $hosts[0],
    date('Y-m-d', $info['validTo_time_t']), $keyFile);
