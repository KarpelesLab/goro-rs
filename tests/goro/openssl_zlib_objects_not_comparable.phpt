--TEST--
OpenSSL and zlib context objects are not comparable; NUL bytes end digest/cipher names
--EXTENSIONS--
openssl
zlib
--FILE--
<?php
// OpenSSL and zlib context objects use zend_objects_not_comparable
$pem = "-----BEGIN CERTIFICATE-----
MIIC4jCCAkugAwIBAgIIEjRWeJCrze8wDQYJKoZIhvcNAQELBQAwgYAxCzAJBgNV
BAYTAkZSMQ4wDAYDVQQIDAVQYXJpczEOMAwGA1UEBwwFUGFyaXMxETAPBgNVBAoM
CFRlc3QgT3JnMQ0wCwYDVQQLDARVbml0MRAwDgYDVQQDDAdUZXN0IENBMR0wGwYJ
KoZIhvcNAQkBFg5jYUBleGFtcGxlLmNvbTAeFw0yNjEwMjcxNTQ2NDJaFw0zNjEw
MjQxNTQ2NDJaMIGAMQswCQYDVQQGEwJGUjEOMAwGA1UECAwFUGFyaXMxDjAMBgNV
BAcMBVBhcmlzMREwDwYDVQQKDAhUZXN0IE9yZzENMAsGA1UECwwEVW5pdDEQMA4G
A1UEAwwHVGVzdCBDQTEdMBsGCSqGSIb3DQEJARYOY2FAZXhhbXBsZS5jb20wgZ8w
DQYJKoZIhvcNAQEBBQADgY0AMIGJAoGBAKiEw2Wes3ZCqhTefhFnxn84MCqLcRGX
e1mHg8YGO/u6ZjYz6nyhOkimbU5Ua6YvnPhoVENnWIfGcJaaOtjL3IOIyi4Uc4Aw
uoVFLkVGPl5bqXk9ljjzIiAiiQUnLC1DZy+/qKTE1pc2U+LP1tw4lqtbmU110gGN
wdaZZpvhqHYHAgMBAAGjYzBhMB8GA1UdIwQYMBaAFCNceaLRwgdNrUJ2FtiEuNlx
Z135MA8GA1UdEwEB/wQFMAMBAf8wDgYDVR0PAQH/BAQDAgEGMB0GA1UdDgQWBBQj
XHmi0cIHTa1CdhbYhLjZcWdd+TANBgkqhkiG9w0BAQsFAAOBgQBt21jRoA2Lxu6n
XvHioJxnzriq85wD019PN52bBiFU5sQeC1lJWsX9thJVez8AIHSwGl1jGKW+VMg4
6Q2r6QSELuPuWIAm3Oz3dQnLCx5NN/WH3k9AYdUBy4mQjQ4dP4aqzyGB5S2xMlHx
Y/X77czvaaToU03jRqU8HRioy7zRiA==
-----END CERTIFICATE-----";
$key = "-----BEGIN PRIVATE KEY-----
MIICdgIBADANBgkqhkiG9w0BAQEFAASCAmAwggJcAgEAAoGBAKiEw2Wes3ZCqhTe
fhFnxn84MCqLcRGXe1mHg8YGO/u6ZjYz6nyhOkimbU5Ua6YvnPhoVENnWIfGcJaa
OtjL3IOIyi4Uc4AwuoVFLkVGPl5bqXk9ljjzIiAiiQUnLC1DZy+/qKTE1pc2U+LP
1tw4lqtbmU110gGNwdaZZpvhqHYHAgMBAAECgYBTlj7BWf2ULC7m2RFZGstXcQxb
Hf1Ah420c8ZF4/O+wc9PFWM63FHnlT1ke/VFYhqPEFizckB0YFaaLPgBbuT6PtzD
M/e2rig2uvowb9DMc0KJiQWWHBLl8/LpS6XtElraKTU9K5dT5d6s3/Xji2nV9G9W
JRPoVuZSlazW0YjYGQJBAN0Y/AOMCpEWwB/iLkcY0jtm2fsgZaeIgqpwUBy6Myh3
YMdVaDF19Adk1QFwjbRcqVBnVKwsfI2lG9BjyYxCqOsCQQDDHvO8WTZGurAaATmD
hP42xo6xkCCqCirq0uo9zkLWrslawSPrcTX/6AP3D7DkNvkedKmdYSbCVW9xY04U
+yBVAkAyUjFpjUIKa6cmmo/11oxiwLH0GheI3eT/bbiOEUL7ftAkDig8uZ/k3uor
/bYCyuQuTJZU6Xsfseh2sX950OIpAkACIDIzmR04RAxrHBxOTIKggiVH1fQlJhs1
l7qplbewsxLKhFQp2n/mG9b/OBKLSfYI2OiIKGjNsTSXnadaHypJAkEA0aOEAY1w
5UzCVeAw5yo/lF2cCTeDoaTdMiACUywZsZnRB2gG6yAiumQPWrBoR8O3+8t+As2b
AnR3SEKhEVgvGg==
-----END PRIVATE KEY-----";
$pairs = [
  'cert' => fn() => openssl_x509_read($pem),
  'key' => fn() => openssl_pkey_get_private($key),
  'deflate' => fn() => deflate_init(ZLIB_ENCODING_RAW),
  'inflate' => fn() => inflate_init(ZLIB_ENCODING_GZIP),
];
foreach ($pairs as $n => $f) {
  $a = $f(); $b = $f();
  echo $n, ": == ", var_export($a == $b, true), " != ", var_export($a != $b, true), " < ", var_export($a < $b, true), " > ", var_export($a > $b, true), " <=> ", $a <=> $b, " self ", var_export($a == $a, true), " ", $a <=> $a, "\n";
}
// names reach OpenSSL as C strings: a NUL byte ends them
var_dump(openssl_digest("abc", "sha256\0junk") === hash('sha256', 'abc'));
var_dump(bin2hex(openssl_pbkdf2("p", "s", 8, 1, "sha1\0x")) === bin2hex(openssl_pbkdf2("p", "s", 8, 1, "sha1")));
var_dump(openssl_encrypt("x", "aes-128-ecb\0x", "k") === openssl_encrypt("x", "aes-128-ecb", "k"));
var_dump(openssl_cipher_iv_length("aes-128-cbc\0"));
?>
--EXPECT--
cert: == false != true < false > false <=> 1 self true 0
key: == false != true < false > false <=> 1 self true 0
deflate: == false != true < false > false <=> 1 self true 0
inflate: == false != true < false > false <=> 1 self true 0
bool(true)
bool(true)
bool(true)
int(16)
