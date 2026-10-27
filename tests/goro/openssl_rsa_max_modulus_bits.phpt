--TEST--
RSA keys over OPENSSL_RSA_MAX_MODULUS_BITS: generated, private ops work, public ops refused
--FILE--
<?php
// OPENSSL_RSA_MAX_MODULUS_BITS (16384) limits the public operations, not
// key generation: a bigger key signs and decrypts, its public half fails
// with "modulus too large".
$cnf = sys_get_temp_dir() . "/goro_rsabig_" . getmypid() . ".cnf";
file_put_contents($cnf, "HOME = .\n[ req ]\ndistinguished_name = dn\n[ dn ]\n");
function errs() { while ($e = openssl_error_string()) echo "  ", $e, "\n"; }
// Two primes of nbits/2 bits each from 2048 bits on (SP 800-56B): an odd
// size gives a modulus one bit shorter; below that the size is exact.
foreach ([1025, 2047, 2049, 2051, 3073] as $bits) {
    $k = openssl_pkey_new(['config' => $cnf, 'private_key_bits' => $bits, 'private_key_type' => OPENSSL_KEYTYPE_RSA]);
    while (openssl_error_string());
    $d = openssl_pkey_get_details($k);
    echo $bits, ": ", $d['bits'], " p=", strlen($d['rsa']['p']), " q=", strlen($d['rsa']['q']), "\n";
}
$pem = <<<PEM
-----BEGIN PRIVATE KEY-----
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
U5MakRE0sgZThVr3HqEWE7PZWot+v4SF
-----END PRIVATE KEY-----
PEM;
$k = openssl_pkey_get_private($pem);
var_dump(get_class($k)); errs();
$d = openssl_pkey_get_details($k);
// (n and d do not fit EVP_PKEY_get_bn_param()'s first 2048-byte buffer)
var_dump($d['bits'], strlen($d['rsa']['n']), strlen($d['rsa']['d'])); errs();
$pub = openssl_pkey_get_public($d['key']);
echo "sign: "; var_dump(openssl_sign("x", $sig, $k, OPENSSL_ALGO_SHA256)); errs();
echo "verify: "; var_dump(openssl_verify("x", $sig, $pub, OPENSSL_ALGO_SHA256)); errs();
echo "sign pss: "; var_dump(openssl_sign("x", $sig2, $k, "sha256", OPENSSL_PKCS1_PSS_PADDING)); errs();
echo "verify pss: "; var_dump(openssl_verify("x", $sig2, $pub, "sha256", OPENSSL_PKCS1_PSS_PADDING)); errs();
echo "pubenc: "; var_dump(openssl_public_encrypt("x", $c, $pub)); errs();
echo "pubenc too long: "; var_dump(openssl_public_encrypt(str_repeat("x", 3000), $c, $pub)); errs();
echo "privenc: "; var_dump(openssl_private_encrypt("x", $c2, $k)); errs();
echo "pubdec: "; var_dump(openssl_public_decrypt($c2, $p, $pub), $p); errs();
echo "seal: "; var_dump(openssl_seal("data", $sealed, $ek, [$pub], "AES-128-CBC", $iv)); errs();
echo "x509 check: ";
$csr = openssl_csr_new(['commonName' => 'x'], $k, ['config' => $cnf, 'digest_alg' => 'sha256']);
while (openssl_error_string());
var_dump(get_class($csr));
var_dump(@openssl_csr_sign($csr, null, $k, 1, ['config' => $cnf])); errs();
@unlink($cnf);
?>
--EXPECT--
1025: 1025 p=65 q=64
2047: 2047 p=128 q=128
2049: 2048 p=128 q=128
2051: 2050 p=129 q=129
3073: 3072 p=192 q=192
string(20) "OpenSSLAsymmetricKey"
int(16386)
int(2049)
int(2049)
  error:07800074:common libcrypto routines::too small buffer
  error:07800074:common libcrypto routines::too small buffer
sign: bool(true)
verify: int(0)
  error:02000069:rsa routines::modulus too large
  error:1C880004:Provider routines::RSA lib
sign pss: bool(true)
verify pss: int(0)
  error:02000069:rsa routines::modulus too large
  error:1C880004:Provider routines::RSA lib
pubenc: bool(false)
  error:02000069:rsa routines::modulus too large
pubenc too long: bool(false)
  error:02000069:rsa routines::modulus too large
privenc: bool(true)
pubdec: bool(false)
NULL
  error:02000069:rsa routines::modulus too large
  error:1C880004:Provider routines::RSA lib
  error:030000EA:digital envelope routines::provider signature failure
seal: bool(false)
  error:02000069:rsa routines::modulus too large
x509 check: string(32) "OpenSSLCertificateSigningRequest"
bool(false)
  error:02000069:rsa routines::modulus too large
  error:1C880004:Provider routines::RSA lib
  error:06880006:asn1 encoding routines::EVP lib
