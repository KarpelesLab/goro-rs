--TEST--
PKCS#7: content written before signature checks, empty pkcs7_read input, key/cert mismatch on signing, EC recipients and decrypt keys
--EXTENSIONS--
openssl
--FILE--
<?php
function errs() { $o = []; while ($e = openssl_error_string()) $o[] = $e; return $o ? " [" . implode(" | ", $o) . "]" : ""; }
function t($l, $f) { try { $r = $f(); echo "$l: "; var_export($r); } catch (Throwable $e) { echo "$l: ", get_class($e), ": ", $e->getMessage(); } echo errs(), "\n"; }
$tmp = sys_get_temp_dir() . "/openssl_pkcs7_error_paths_" . getmypid();
@mkdir($tmp);
file_put_contents("$tmp/rsa.crt", <<<PEM
-----BEGIN CERTIFICATE-----
MIIEQzCCA6ygAwIBAgIJEjRWeJCrze8SMA0GCSqGSIb3DQEBCwUAMIHIMQswCQYD
VQQGEwJVSzERMA8GA1UECBMIU29tZXJzZXQxFDASBgNVBAcTC0dsYXN0b25idXJ5
MR8wHQYDVQQKExZUaGUgQnJhaW4gUm9vbSBMaW1pdGVkMR8wHQYDVQQLExZQSFAg
RG9jdW1lbnRhdGlvbiBUZWFtMRQwEgYDVQQDEwtXZXogRnVybG9uZzEeMBwGCSqG
SIb3DQEJARYPd2V6QGV4YW1wbGUuY29tMRgwFgYKCZImiZPyLGQBARMIc29tZSB1
aWQwHhcNMjYxMDI2MjAzMTM2WhcNMjcxMDI2MjAzMTM2WjCByDELMAkGA1UEBhMC
VUsxETAPBgNVBAgTCFNvbWVyc2V0MRQwEgYDVQQHEwtHbGFzdG9uYnVyeTEfMB0G
A1UEChMWVGhlIEJyYWluIFJvb20gTGltaXRlZDEfMB0GA1UECxMWUEhQIERvY3Vt
ZW50YXRpb24gVGVhbTEUMBIGA1UEAxMLV2V6IEZ1cmxvbmcxHjAcBgkqhkiG9w0B
CQEWD3dlekBleGFtcGxlLmNvbTEYMBYGCgmSJomT8ixkAQETCHNvbWUgdWlkMIGf
MA0GCSqGSIb3DQEBAQUAA4GNADCBiQKBgQDFjgGKbNCfeEV2N3mKEN+zX1ioZH15
6/72MnFooCptOUmHTqzzyASuG9pzB1hvUBDrLTB/z0CeY4HOMdfLDQ6tWsvwWCnF
NpWp8TotsyBgjdDfLkGlp75PIivuE6A/qDlt4PpXsihIVQDxzfp1ABu2byQxxkV5
8hPxcvIWUoCi6QIDAQABo4IBMTCCAS0wHQYDVR0OBBYEFHIl3qQ7/XZMaarmOA2N
Vt8A720WMIH9BgNVHSMEgfUwgfKAFHIl3qQ7/XZMaarmOA2NVt8A720WoYHOpIHL
MIHIMQswCQYDVQQGEwJVSzERMA8GA1UECBMIU29tZXJzZXQxFDASBgNVBAcTC0ds
YXN0b25idXJ5MR8wHQYDVQQKExZUaGUgQnJhaW4gUm9vbSBMaW1pdGVkMR8wHQYD
VQQLExZQSFAgRG9jdW1lbnRhdGlvbiBUZWFtMRQwEgYDVQQDEwtXZXogRnVybG9u
ZzEeMBwGCSqGSIb3DQEJARYPd2V6QGV4YW1wbGUuY29tMRgwFgYKCZImiZPyLGQB
ARMIc29tZSB1aWSCCRI0VniQq83vEjAMBgNVHRMEBTADAQH/MA0GCSqGSIb3DQEB
CwUAA4GBADt2pz3XNcevwq6sLqm6so208raQrmgwWxkFaVZ2Ivu52NekyWlO89dw
HVeNP0bWHy/vuATAdmHJGSLpg7N2dAwZmXx2xC/rWWwTNWZESS5o/N3Ag6yQ1vbI
KaTcJbZPDcI/igbs+4+b4n6WvUN48Rx3JVh4GMblwm428qDZZRYB
-----END CERTIFICATE-----
PEM);
file_put_contents("$tmp/rsa.key", <<<PEM
-----BEGIN PRIVATE KEY-----
MIICdgIBADANBgkqhkiG9w0BAQEFAASCAmAwggJcAgEAAoGBAMWOAYps0J94RXY3
eYoQ37NfWKhkfXnr/vYycWigKm05SYdOrPPIBK4b2nMHWG9QEOstMH/PQJ5jgc4x
18sNDq1ay/BYKcU2lanxOi2zIGCN0N8uQaWnvk8iK+4ToD+oOW3g+leyKEhVAPHN
+nUAG7ZvJDHGRXnyE/Fy8hZSgKLpAgMBAAECgYBlkfhq0j2E8BJOkIEVok1RbOEp
WtlEeTiSHic+0rJdi8mpHqM/h4QYfEuZPpqSSIkaURaRb95aUPDpMakwRZtYTxEC
BgAlu9KW/gbe3TT0n4zVIGpSvdYsKpuePsie3MFqX1Ny12A1p/o3IfOcEN5jAaVa
56iQafBSTKCzZmlVgQJBAPu7GL48nu0LG16QielELjP7cvhiE2B1ST1Zm8G1KEHo
2F/DBDapvcp86SziPfwB19i2rRquVSPAy/mf2quGojkCQQDI57NgQnEdrZNU9r+S
fY4aVl0LN5h6E1yD7nLpFo9NNdbv3F/WXk/i7fsemcqQkas6Udvh9HynHaKJr7aY
YEYxAkBd1jq8/4AGwPMs5xhhuia2/e63pGewt0fb9MFZDhy5pQCzgdDaR6AWEUYi
kxukC1PY1w/kOERIlZ/zp6Et+3wRAkAWUrh5r6hF9e1y+V/b9Rt/Da2U+35zwgb7
mdkm0RznJV5WlJQJi70wp+TJR6lcWvM9ra7S8omPQBm5Vs7hK4FhAkEA74AL8/5G
hCgZCzC0Wa9CDEQAe3JpDd5b2jVbrbj2ltYr13VMLsK70ptnDfpFLkdgmTe844US
2plbFgmBKUq4Ew==
-----END PRIVATE KEY-----
PEM);
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
$crt = "file://$tmp/rsa.crt"; $key = "file://$tmp/rsa.key";
$eccrt = "file://$tmp/ec.crt"; $eckey = "file://$tmp/ec.key";
file_put_contents("$tmp/in.txt", "Hello world\r\nline 2\n");
t("detached binary sign, verify text", function() use ($tmp, $crt, $key) {
    openssl_pkcs7_sign("$tmp/in.txt", "$tmp/s.eml", $crt, $key, [], PKCS7_DETACHED | PKCS7_BINARY);
    $v = openssl_pkcs7_verify("$tmp/s.eml", PKCS7_NOVERIFY, null, [], null, "$tmp/content.txt");
    return [$v, bin2hex(file_get_contents("$tmp/content.txt"))];
});
t("pkcs7_read empty", fn() => openssl_pkcs7_read("", $c));
t("sign with another key", fn() => openssl_pkcs7_sign("$tmp/in.txt", "$tmp/s3.eml", $crt, $eckey, []));
t("cms sign with another key", fn() => openssl_cms_sign("$tmp/in.txt", "$tmp/s4.eml", $crt, $eckey, []));
t("encrypt to EC cert", fn() => openssl_pkcs7_encrypt("$tmp/in.txt", "$tmp/e.eml", $eccrt, []));
t("decrypt with EC key", function() use ($tmp, $crt, $eckey, $eccrt) {
    openssl_pkcs7_encrypt("$tmp/in.txt", "$tmp/e.eml", $crt, []);
    return openssl_pkcs7_decrypt("$tmp/e.eml", "$tmp/d.txt", $eccrt, $eckey);
});
array_map('unlink', glob("$tmp/*"));
rmdir($tmp);
?>
--EXPECTF--
detached binary sign, verify text: array (
  0 => false,
  1 => '48656c6c6f20776f726c640d0a6c696e6520320d0a',
) [error:10800065:PKCS7 routines::digest failure | error:10800069:PKCS7 routines::signature failure]
pkcs7_read empty: false

Warning: openssl_pkcs7_sign(): Error creating PKCS7 structure! in %s on line 94
sign with another key: false [error:03000065:digital envelope routines::different key types | error:05800073:x509 certificate routines::key type mismatch | error:1080007F:PKCS7 routines::private key does not match certificate | error:10800099:PKCS7 routines::pkcs7 add signer error]

Warning: openssl_cms_sign(): Error creating CMS structure! in %s on line 95
cms sign with another key: false [error:03000065:digital envelope routines::different key types | error:05800073:x509 certificate routines::key type mismatch | error:17000088:CMS routines::private key does not match certificate | error:17000063:CMS routines::add signer error]
encrypt to EC cert: false [error:10800096:PKCS7 routines::encryption not supported for this key type | error:10800078:PKCS7 routines::error adding recipient]
decrypt with EC key: false [error:03000096:digital envelope routines::operation not supported for this keytype | error:10800077:PKCS7 routines::decrypt error]
