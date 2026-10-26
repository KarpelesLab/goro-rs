--TEST--
openssl_cms_encrypt()/openssl_cms_decrypt() with EC recipients (ECDH KeyAgreeRecipientInfo, X9.63 KDF, AES key wrap), interoperable with OpenSSL
--EXTENSIONS--
openssl
--FILE--
<?php
$tmp = sys_get_temp_dir() . "/openssl_cms_ec_recipient_" . getmypid();
@mkdir($tmp);
file_put_contents("$tmp/ec.crt", <<<PEM
-----BEGIN CERTIFICATE-----
MIID+TCCA3+gAwIBAgIJEjRWeJCrze8SMAoGCCqGSM49BAMCMIHIMQswCQYDVQQG
EwJVSzERMA8GA1UECBMIU29tZXJzZXQxFDASBgNVBAcTC0dsYXN0b25idXJ5MR8w
HQYDVQQKExZUaGUgQnJhaW4gUm9vbSBMaW1pdGVkMR8wHQYDVQQLExZQSFAgRG9j
dW1lbnRhdGlvbiBUZWFtMRQwEgYDVQQDEwtXZXogRnVybG9uZzEeMBwGCSqGSIb3
DQEJARYPd2V6QGV4YW1wbGUuY29tMRgwFgYKCZImiZPyLGQBARMIc29tZSB1aWQw
HhcNMjYxMDI2MjAzMTM2WhcNMjcxMDI2MjAzMTM2WjCByDELMAkGA1UEBhMCVUsx
ETAPBgNVBAgTCFNvbWVyc2V0MRQwEgYDVQQHEwtHbGFzdG9uYnVyeTEfMB0GA1UE
ChMWVGhlIEJyYWluIFJvb20gTGltaXRlZDEfMB0GA1UECxMWUEhQIERvY3VtZW50
YXRpb24gVGVhbTEUMBIGA1UEAxMLV2V6IEZ1cmxvbmcxHjAcBgkqhkiG9w0BCQEW
D3dlekBleGFtcGxlLmNvbTEYMBYGCgmSJomT8ixkAQETCHNvbWUgdWlkMHYwEAYH
KoZIzj0CAQYFK4EEACIDYgAEIoULIErF/kepnWzyT5Rz68keSQKPGTnrGPuz1hNM
7N56Q96HfqwjdtMqLiPPOlhVSEFo/8EtbaHuF9wvgJxIveNfSy+M3G+kEErTT3ll
IAr65KUIhH8wpNe7KFF2Mz0ko4IBMTCCAS0wHQYDVR0OBBYEFI87HXSCiBQZH/sc
aS5AGWzRTLqlMIH9BgNVHSMEgfUwgfKAFI87HXSCiBQZH/scaS5AGWzRTLqloYHO
pIHLMIHIMQswCQYDVQQGEwJVSzERMA8GA1UECBMIU29tZXJzZXQxFDASBgNVBAcT
C0dsYXN0b25idXJ5MR8wHQYDVQQKExZUaGUgQnJhaW4gUm9vbSBMaW1pdGVkMR8w
HQYDVQQLExZQSFAgRG9jdW1lbnRhdGlvbiBUZWFtMRQwEgYDVQQDEwtXZXogRnVy
bG9uZzEeMBwGCSqGSIb3DQEJARYPd2V6QGV4YW1wbGUuY29tMRgwFgYKCZImiZPy
LGQBARMIc29tZSB1aWSCCRI0VniQq83vEjAMBgNVHRMEBTADAQH/MAoGCCqGSM49
BAMCA2gAMGUCMFXpEbPaXp+NxjooEBJvjSKOlTOUF9r2jNanjSalUxpUk0nv5JHC
2qTv3Q4NlBpwuwIxAOwIHSxyZQI2JBmJnL0UOrTDFk3LrtTTiVlOSLRYEY39G0x7
EHqrn/cWfwcGYRvvdA==
-----END CERTIFICATE-----
PEM);
file_put_contents("$tmp/ec.key", <<<PEM
-----BEGIN PRIVATE KEY-----
MIG2AgEAMBAGByqGSM49AgEGBSuBBAAiBIGeMIGbAgEBBDA1zB0U8oOv1iBCiHxZ
l2Z5d9KeOmXy3os+MUMbZyFVMHgKvqVrERw6VPWnx+zYk1ShZANiAAQihQsgSsX+
R6mdbPJPlHPryR5JAo8ZOesY+7PWE0zs3npD3od+rCN20youI886WFVIQWj/wS1t
oe4X3C+AnEi9419LL4zcb6QQStNPeWUgCvrkpQiEfzCk17soUXYzPSQ=
-----END PRIVATE KEY-----
PEM);
// enveloped by PHP 8.5 / OpenSSL 3.5 for this EC certificate
file_put_contents("$tmp/ref.pem", <<<PEM
-----BEGIN CMS-----
MIIB9QYJKoZIhvcNAQcDoIIB5jCCAeICAQIxggGNoYIBiQIBA6BxoW8wCQYHKoZI
zj0CAQNiAATYrQht9ELqoRtqvJKtGI+1ms7B9UYaioPaxHV6rEPaALvGrbwVLXh1
Zi3yfmEjzxkwS6cNZ5RNA55Tl6sOj8xL/J/9maLzrfvMgwIsRnKV0EUi6lCJAG1N
Z98CdP2ZQLQwGAYJK4EFEIZIPwACMAsGCWCGSAFlAwQBBTCB9jCB8zCB1jCByDEL
MAkGA1UEBhMCVUsxETAPBgNVBAgTCFNvbWVyc2V0MRQwEgYDVQQHEwtHbGFzdG9u
YnVyeTEfMB0GA1UEChMWVGhlIEJyYWluIFJvb20gTGltaXRlZDEfMB0GA1UECxMW
UEhQIERvY3VtZW50YXRpb24gVGVhbTEUMBIGA1UEAxMLV2V6IEZ1cmxvbmcxHjAc
BgkqhkiG9w0BCQEWD3dlekBleGFtcGxlLmNvbTEYMBYGCgmSJomT8ixkAQETCHNv
bWUgdWlkAgkSNFZ4kKvN7xIEGFhtbsx4CHbdZChO5X7NWEFltB8Y2rvr6TBMBgkq
hkiG9w0BBwEwHQYJYIZIAWUDBAECBBBcrMi/dHdg0G6rPwHONOymgCDcq86I+Wjc
DuBpEQMJ/fUpSHd2UzMXtPyGUrGOtlHuEA==
-----END CMS-----
PEM);
$r = openssl_cms_decrypt("$tmp/ref.pem", "$tmp/out.txt", "file://$tmp/ec.crt", "file://$tmp/ec.key", OPENSSL_ENCODING_PEM);
var_dump($r, file_get_contents("$tmp/out.txt"));
file_put_contents("$tmp/in.txt", "round trip\n");
foreach ([OPENSSL_CIPHER_AES_128_CBC, OPENSSL_CIPHER_AES_256_CBC] as $c) {
    var_dump(openssl_cms_encrypt("$tmp/in.txt", "$tmp/e.der", "file://$tmp/ec.crt", [], 0, OPENSSL_ENCODING_DER, $c));
    var_dump(openssl_cms_decrypt("$tmp/e.der", "$tmp/d.txt", "file://$tmp/ec.crt", "file://$tmp/ec.key", OPENSSL_ENCODING_DER), file_get_contents("$tmp/d.txt"));
}
while ($e = openssl_error_string()) echo $e, "\n";
array_map('unlink', glob("$tmp/*"));
rmdir($tmp);
?>
--EXPECT--
bool(true)
string(18) "Hello EC interop
"
bool(true)
bool(true)
string(12) "round trip
"
bool(true)
bool(true)
string(12) "round trip
"
