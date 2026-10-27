--TEST--
RSA-PSS keys: type -1 and no rsa details, PSS-only signing, no encryption
--EXTENSIONS--
openssl
--FILE--
<?php
function errs() { $o = []; while ($e = openssl_error_string()) $o[] = $e; return $o ? " [" . implode(" | ", $o) . "]" : ""; }
// RSASSA-PSS (EVP_PKEY_RSA_PSS) keys
$k = openssl_pkey_get_private("-----BEGIN PRIVATE KEY-----
MIICdQIBADALBgkqhkiG9w0BAQoEggJhMIICXQIBAAKBgQDKsGV17wvrOahRRC0Z
ozs2AiMKOOjJjeJ1WVF8kHDgbIItBPRmf1ShDUVi5JwU7H/JQx1bYre4aJ7uUyi9
N2zEI8klWNSwhhOyZtO1vXn9wZGK/HeG46eq/m42UL7AHo1THXkGEQJqHYA6wzx8
/6d5Osd34jVjhoPCtCJIgA4f+wIDAQABAoGAP51XaslJC13ARylHomlJ1yyahMXR
whim/7+1xIus8bL8tFZ+woCqNhXtNhmcJzEG9kr7Nshn8xfa8d/lxGVV3DaCVN8L
84qtz7GH31XIa6bTo4KBhjJiFQlDl7Rue5+xZ//Ar5mG7UNjl5gxRO8c1LyZzK9F
tM2/V94LIoLG70kCQQDnJxdA4YTrlIS6GvJNoMT6HRChpF7heWJAfH8O/iSRMl/c
N0C3cENvzUFm3dBUnE+aDIU7VqhpIaTLRS+i+fYHAkEA4HoJZZCXY8SI7IMveMNJ
ZC7KAb++jjGgeznta9E1tH9O/eQfmuFoxls0p+SwxUHzjrOwNvOW3duE4d54wqzp
bQJBAKRQonQJnA5G7i7JVvMd1Mgi/x6YlKXdPp2uojeLK31XfNMzJ7r+f8Gq+9He
3eg3kVJlkhQDXwYU12tzzBAjdeMCQDTfCY+OetnY1JdAXtIGzSZ3jsTSsMM1FSNx
BWa7o/abdbN/I6KLLO5rVP+mcaxEWGVoFT0cKpCev7GjgLX5nBECQQC8J6fTF/1x
e8NCNXsZR/j6prsQVwi3EsmLvJOFwtawrtWmbwKOfPGtNW23Ha9anRGDWXQX3rWi
NaczdjOD1woU
-----END PRIVATE KEY-----");
$d = openssl_pkey_get_details($k);
var_dump(array_keys($d), $d['bits'], $d['type'], $d['key']);
$pub = openssl_pkey_get_public($d['key']);
var_dump(openssl_pkey_get_details($pub)['type']);
foreach ([OPENSSL_ALGO_SHA256, OPENSSL_ALGO_SHA1] as $a) {
  echo "sign $a: ", var_export(openssl_sign("data", $sig, $k, $a), true), " len=", strlen($sig), " verify=", openssl_verify("data", $sig, $pub, $a), " other=", openssl_verify("datA", $sig, $pub, $a), errs(), "\n";
}
echo "public_encrypt: ", var_export(openssl_public_encrypt("x", $c, $pub), true), errs(), "\n";
echo "private_decrypt: ", var_export(openssl_private_decrypt(str_repeat("x", 128), $c, $k), true), errs(), "\n";
$cert = openssl_x509_read("-----BEGIN CERTIFICATE-----
MIICXjCCAZOgAwIBAgIUaSa52Xl9lIb8EfhUDMaMUURi0qEwQQYJKoZIhvcNAQEK
MDSgDzANBglghkgBZQMEAgEFAKEcMBoGCSqGSIb3DQEBCDANBglghkgBZQMEAgEF
AKIDAgEgMA4xDDAKBgNVBAMMA3BzczAeFw0yNjEwMjcxNjIxMjNaFw0yNjExMDYx
NjIxMjNaMA4xDDAKBgNVBAMMA3BzczCBnTALBgkqhkiG9w0BAQoDgY0AMIGJAoGB
AMqwZXXvC+s5qFFELRmjOzYCIwo46MmN4nVZUXyQcOBsgi0E9GZ/VKENRWLknBTs
f8lDHVtit7honu5TKL03bMQjySVY1LCGE7Jm07W9ef3BkYr8d4bjp6r+bjZQvsAe
jVMdeQYRAmodgDrDPHz/p3k6x3fiNWOGg8K0IkiADh/7AgMBAAGjUzBRMB0GA1Ud
DgQWBBSMZevq376QPxA9FOwfzv5xYd/4sDAfBgNVHSMEGDAWgBSMZevq376QPxA9
FOwfzv5xYd/4sDAPBgNVHRMBAf8EBTADAQH/MEEGCSqGSIb3DQEBCjA0oA8wDQYJ
YIZIAWUDBAIBBQChHDAaBgkqhkiG9w0BAQgwDQYJYIZIAWUDBAIBBQCiAwIBIAOB
gQAoYzqziYBAhzHOvRYzxM4+46if8gJwp9zp2/D+MnmV0dhfjKIoNYxDpJvZmi2P
WykJRR2WnRcvAtyLi2H8p6zb/A+YaMAgwtv3TKE8pg7IzgQIZhr9cwFn8WslaQRW
GdmN0yEdUtsMFrBe7NdZiTUhfx2Wz/K0FoNH8q+N63o8Rw==
-----END CERTIFICATE-----");
echo "cert: ", openssl_x509_parse($cert)['signatureTypeSN'], " verify=", openssl_x509_verify($cert, $pub), " type=", openssl_pkey_get_details(openssl_pkey_get_public($cert))['type'], errs(), "\n";
?>
--EXPECT--
array(3) {
  [0]=>
  string(4) "bits"
  [1]=>
  string(3) "key"
  [2]=>
  string(4) "type"
}
int(1024)
int(-1)
string(272) "-----BEGIN PUBLIC KEY-----
MIGdMAsGCSqGSIb3DQEBCgOBjQAwgYkCgYEAyrBlde8L6zmoUUQtGaM7NgIjCjjo
yY3idVlRfJBw4GyCLQT0Zn9UoQ1FYuScFOx/yUMdW2K3uGie7lMovTdsxCPJJVjU
sIYTsmbTtb15/cGRivx3huOnqv5uNlC+wB6NUx15BhECah2AOsM8fP+neTrHd+I1
Y4aDwrQiSIAOH/sCAwEAAQ==
-----END PUBLIC KEY-----
"
int(-1)
sign 7: true len=128 verify=1 other=0 [error:02000068:rsa routines::bad signature | error:1C880004:Provider routines::RSA lib]
sign 1: true len=128 verify=1 other=0 [error:02000068:rsa routines::bad signature | error:1C880004:Provider routines::RSA lib]
public_encrypt: false [error:1C8000B2:Provider routines::operation not supported for this keytype]
private_decrypt: false [error:1C8000B2:Provider routines::operation not supported for this keytype]
cert: RSASSA-PSS verify=1 type=-1
