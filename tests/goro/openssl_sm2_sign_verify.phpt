--TEST--
SM2 signatures hash Z (empty ID) and allow SM3 only; SM2 certificates verify
--EXTENSIONS--
openssl
--FILE--
<?php
function errs() { $o = []; while ($e = openssl_error_string()) $o[] = $e; return $o ? " [" . implode(" | ", $o) . "]" : ""; }
// SM2 digest-sign hashes Z (empty distinguishing ID) with SM3, and only SM3
$k = openssl_pkey_get_private("-----BEGIN PRIVATE KEY-----
MIGHAgEAMBMGByqGSM49AgEGCCqBHM9VAYItBG0wawIBAQQgnaVmkalVM6P7LD3v
bpwa8FtqMZtEumMf0/u4e5sMGQShRANCAARt8EOcs63/V2bC24nJP6EnTcZSY9TP
P2guGJ2Ik8d1MY0BldevEpfAB1i6B0YVj+cb1YreVk3yHbVh+qqpNxSu
-----END PRIVATE KEY-----");
$pub = openssl_pkey_get_public(openssl_pkey_get_details($k)['key']);
// signed by OpenSSL 3.5: openssl_sign("data", $sig, $k, "sm3")
$ref = hex2bin("304402200cf0288ff98558ee1a79d5948677e611ddc05f790cfcda036d143fad94f5d7ab022016b144690e93e4c1084e57c44218cf4ed76225287a8db985a6ae12e1c2f18df9");
echo "reference signature: ", openssl_verify("data", $ref, $pub, "sm3"), errs(), "\n";
echo "other data: ", openssl_verify("datA", $ref, $pub, "sm3"), errs(), "\n";
echo "default digest: ", openssl_verify("data", $ref, $pub), errs(), "\n";
echo "sign sm3: ", var_export(openssl_sign("data", $sig, $k, "sm3"), true), " ", openssl_verify("data", $sig, $pub, "sm3"), errs(), "\n";
echo "sign sha256: ", var_export(openssl_sign("data", $sig, $k, OPENSSL_ALGO_SHA256), true), errs(), "\n";
echo "public_encrypt: ", var_export(openssl_public_encrypt("x", $c, $pub), true), errs(), "\n";
$cert = openssl_x509_read("-----BEGIN CERTIFICATE-----
MIIBcTCCARegAwIBAgIUTgniDv1JL5pLUezlIAYIj2lx/hAwCgYIKoEcz1UBg3Uw
DjEMMAoGA1UEAwwDc20yMB4XDTI2MTAyNzE2MjEyM1oXDTI2MTEwNjE2MjEyM1ow
DjEMMAoGA1UEAwwDc20yMFkwEwYHKoZIzj0CAQYIKoEcz1UBgi0DQgAEbfBDnLOt
/1dmwtuJyT+hJ03GUmPUzz9oLhidiJPHdTGNAZXXrxKXwAdYugdGFY/nG9WK3lZN
8h21YfqqqTcUrqNTMFEwHQYDVR0OBBYEFCcJ/+binmqFNkllf+9gdCcJI+Y2MB8G
A1UdIwQYMBaAFCcJ/+binmqFNkllf+9gdCcJI+Y2MA8GA1UdEwEB/wQFMAMBAf8w
CgYIKoEcz1UBg3UDSAAwRQIgcZBkbg3TYCiRRE7ppWF8J/D7C5kIlSIfIYultULB
84gCIQDzTt6f8L2oXIq7FMer8N5YIX5/0WFZRKVa/HnMo8oUGQ==
-----END CERTIFICATE-----");
echo "self-signed cert: ", openssl_x509_parse($cert)['signatureTypeSN'], " ", openssl_x509_verify($cert, $pub), errs(), "\n";
?>
--EXPECT--
reference signature: 1
other data: 0 [error:030000EA:digital envelope routines::provider signature failure]
default digest: -1 [error:1C80007A:Provider routines::invalid digest]
sign sm3: true 1
sign sha256: false [error:1C80007A:Provider routines::invalid digest]
public_encrypt: false
self-signed cert: SM2-SM3 1
