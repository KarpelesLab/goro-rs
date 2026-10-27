--TEST--
openssl_pkcs7_decrypt(): a key not matching the recipient certificate fails X509_check_private_key first
--EXTENSIONS--
openssl
--FILE--
<?php
function errs() { $o = []; while ($e = openssl_error_string()) $o[] = $e; return $o ? " [" . implode(" | ", $o) . "]" : ""; }
$cert = "-----BEGIN CERTIFICATE-----
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
$rsa = "-----BEGIN PRIVATE KEY-----
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
$ec = "-----BEGIN PRIVATE KEY-----
MIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQgMk8mx9hEpopQi5cC
ItZ5ekqkVukjUcrHeZZANRotYh6hRANCAATKl8fW8ZIEDYL0NjBSN7wpsI7uNGOF
8tS2G2qcAGyyK1n8rNbTmTwCniAaQ3odeIV8toL0xpjJG+yNYdClmnnK
-----END PRIVATE KEY-----";
$dsa = "-----BEGIN PRIVATE KEY-----
MIIBWgIBADCCATMGByqGSM44BAEwggEmAoGBAIqdCNrK7agQWR6aE80gVqjb02w/
WdY9qJeyn0PU4qRsu1VlBu1rGv0VcoVORyURonBJQnfsTx75TjBSoJOCc0/yklrl
QshMFsIPhWcPQ7Bu71D2wBpQ8djElHjr2KF+XUeFDjhpMO3csTIQG48/rgysLpN4
rQO2OunsHTpTpEU5Ah0A8LhV4aICwaXmmYoSFOEYuu9potu6vm1TjpZlBQKBgCog
mczNlcF6gQ5ojb6y7diDt7fmbzMV7YzPMg0Ze1wIeWrV0Pr9j7F7YpF8VMkm6M6z
1TPIN3lWELfGDjblw7ODFLncYrIGOVaALjBIFRMNVAPBfirx4plal44/l9s3CC2v
O/htRw57rHAaKozDqGgv7rQgW9r9JLjzcQBeOIw2BB4CHGVhqwZILkLaOfkacSHi
mdxEH11YOMdVzaC92F4=
-----END PRIVATE KEY-----";
$t = sys_get_temp_dir() . "/goro_p7dec_" . getmypid();
file_put_contents("$t.in", "Hello PKCS7\n");
var_dump(openssl_pkcs7_encrypt("$t.in", "$t.enc", $cert, [], 0, OPENSSL_CIPHER_AES_128_CBC));
errs();
echo "right key: ", var_export(openssl_pkcs7_decrypt("$t.enc", "$t.out", $cert, $rsa), true), errs(), " ", file_get_contents("$t.out");
echo "ec key: ", var_export(openssl_pkcs7_decrypt("$t.enc", "$t.out", $cert, $ec), true), errs(), "\n";
echo "dsa key: ", var_export(openssl_pkcs7_decrypt("$t.enc", "$t.out", $cert, $dsa), true), errs(), "\n";
foreach (["in", "enc", "out"] as $s) @unlink("$t.$s");
?>
--EXPECT--
bool(true)
right key: true Hello PKCS7
ec key: false [error:03000065:digital envelope routines::different key types | error:05800073:x509 certificate routines::key type mismatch | error:1080007F:PKCS7 routines::private key does not match certificate]
dsa key: false [error:03000065:digital envelope routines::different key types | error:05800073:x509 certificate routines::key type mismatch | error:1080007F:PKCS7 routines::private key does not match certificate]
