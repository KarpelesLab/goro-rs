--TEST--
openssl_x509_export()/openssl_csr_export() text as X509_print() (compat names, serials, signature dumps, DSA/EC keys, raw unknown/NUL extensions), codesign purpose, key type mismatch, sign-extended system errors
--EXTENSIONS--
openssl
--FILE--
<?php
function errs() { $o = []; while ($e = openssl_error_string()) $o[] = $e; return $o ? " [" . implode(" | ", $o) . "]" : ""; }
$certs = [];
$certs['unknown_ext'] = <<<PEM
-----BEGIN CERTIFICATE-----
MIIEoDCCBAmgAwIBAgIBJzANBgkqhkiG9w0BAQQFADCBkDELMAkGA1UEFhMCUk8x
EDAOBgNVBAgTB1JvbWFuaWExEDAOBgNVBAcTB0NyYWlvdmExDzANBgNVBAoTBlNl
cmdpdTETMBEGA1UECxMKU2VyZ2l1IFNSTDESMBAGA1UEAxMJU2VyZ2l1IENBMSMw
IQYJKoZIhvcNAQkBFhRuX3NlcmdpdUBob3RtYWlsLmNvbTAeFw0wNDA1MTQxMzM0
NTZaFw0wNTA1MTQxMzM0NTZaMIGaMQswCQYDVQQGEwJSTzEQMA4GA1UECBMHUm9t
YW5pYTEQMA4GA1UEBxMHQ3JhaW92YTETMBEGA1UEChMKU2VyZ2l1IFNSTDETMBEG
A1UECxMKU2VyZ2l1IFNSTDEYMBYGA1UEAxMPU2VyZ2l1IHBlcnNvbmFsMSMwIQYJ
KoZIhvcNAQkBFhRuX3NlcmdpdUBob3RtYWlsLmNvbTCBnzANBgkqhkiG9w0BAQEF
AAOBjQAwgYkCgYEApNj7XXz8T8FcLIWpBniPYom3QcT6T7u0xRPHqtqzj5oboBYp
DJe5d354/y0gJTpiLt8+fTrPgWXnbHm3pOHgXzTcX6Arani0GDU0/xDi4VkCRGcS
YqX2sJpcDzAbmK9UDMt3xf/O1B8AJan3RfO0Bm3ozTEPziLMkmsiYr5b/L4CAwEA
AaOCAfwwggH4MAkGA1UdEwQCMAAwNQYJYIZIAYb4QgENBCgWJkZvciBHcmlkIHVz
ZSBvbmx5OyByZXF1ZXN0IHRhZyB1c2VyVGFnMBEGCWCGSAGG+EIBAQQEAwIF4DA/
BgNVHR8EODA2MDSgMqAwhi5odHRwOi8vbW9iaWxlLmJsdWUtc29mdHdhcmUucm86
OTAvY2EvY3JsLnNodG1sMDUGCWCGSAGG+EIBCAQoFiZodHRwOi8vbW9iaWxlLmJs
dWUtc29mdHdhcmUucm86OTAvcHViLzAhBgNVHREEGjAYgRZzZXJnaXVAYmx1ZXNv
ZnR3YXJlLnJvMB0GA1UdDgQWBBSwp//5QRXeIzm93TEPl6CyonTg/DCBpwYDVR0j
BIGfMIGcoYGWpIGTMIGQMQswCQYDVQQGEwJSTzEQMA4GA1UECBMHUm9tYW5pYTEQ
MA4GA1UEBxMHQ3JhaW92YTEPMA0GA1UEChMGU2VyZ2l1MRMwEQYDVQQLEwpTZXJn
aXUgU1JMMRIwEAYDVQQDEwlTZXJnaXUgQ0ExIzAhBgkqhkiG9w0BCQEWFG5fc2Vy
Z2l1QGhvdG1haWwuY29tggEAMAsGA1UdDwQEAwIE8DAjBglghkgBhvhCAQIEFhYU
aHR0cDovLzYyLjIzMS45OC41Mi8wCwYDKgMEBAQ+52I0MA0GCSqGSIb3DQEBBAUA
A4GBAIBIOJ+iiLyQfNJEY+IMefayQea0nmuXYY+F+L1DFjSC7xChytgYoPNnKkhh
3dWPtxbswiqKYUnGi6y3Hi4UhDsOaDW29t2S305hSc2qgjOiNtRYQIVYQ8EHG1k7
Fl63S7uCOhnVJt+4MnUK1N6/pwgsp+Z2GvEsDG1qCKnvNpf6
-----END CERTIFICATE-----
PEM;
$certs['san_nul'] = <<<PEM
-----BEGIN CERTIFICATE-----
MIIE2DCCA8CgAwIBAgIBADANBgkqhkiG9w0BAQUFADCBxTELMAkGA1UEBhMCVVMx
DzANBgNVBAgMBk9yZWdvbjESMBAGA1UEBwwJQmVhdmVydG9uMSMwIQYDVQQKDBpQ
eXRob24gU29mdHdhcmUgRm91bmRhdGlvbjEgMB4GA1UECwwXUHl0aG9uIENvcmUg
RGV2ZWxvcG1lbnQxJDAiBgNVBAMMG251bGwucHl0aG9uLm9yZwBleGFtcGxlLm9y
ZzEkMCIGCSqGSIb3DQEJARYVcHl0aG9uLWRldkBweXRob24ub3JnMB4XDTEzMDgw
NzEzMTE1MloXDTEzMDgwNzEzMTI1MlowgcUxCzAJBgNVBAYTAlVTMQ8wDQYDVQQI
DAZPcmVnb24xEjAQBgNVBAcMCUJlYXZlcnRvbjEjMCEGA1UECgwaUHl0aG9uIFNv
ZnR3YXJlIEZvdW5kYXRpb24xIDAeBgNVBAsMF1B5dGhvbiBDb3JlIERldmVsb3Bt
ZW50MSQwIgYDVQQDDBtudWxsLnB5dGhvbi5vcmcAZXhhbXBsZS5vcmcxJDAiBgkq
hkiG9w0BCQEWFXB5dGhvbi1kZXZAcHl0aG9uLm9yZzCCASIwDQYJKoZIhvcNAQEB
BQADggEPADCCAQoCggEBALXq7cn7Rn1vO3aA3TrzA5QLp6bb7B3f/yN0CJ2XFj+j
pHs+Gw6WWSUDpybiiKnPec33BFawq3kyblnBMjBU61ioy5HwQqVkJ8vUVjGIUq3P
vX/wBmQfzCe4o4uM89gpHyUL9UYGG8oCRa17dgqcv7u5rg0Wq2B1rgY+nHwx3JIv
KRrgSwyRkGzpN8WQ1yrXlxWjgI9de0mPVDDUlywcWze1q2kwaEPTM3hLAmD1PESA
oY/n8A/RXoeeRs9i/Pm/DGUS8ZPINXk/yOzsR/XvvkTVroIeLZqfmFpnZeF0cHzL
08LODkVJJ9zjLdT7SA4vnne4FEbAxDbKAq5qkYzaL4UCAwEAAaOB0DCBzTAMBgNV
HRMBAf8EAjAAMB0GA1UdDgQWBBSIWlXAUv9hzVKjNQ/qWpwkOCL3XDALBgNVHQ8E
BAMCBeAwgZAGA1UdEQSBiDCBhYIeYWx0bnVsbC5weXRob24ub3JnAGV4YW1wbGUu
Y29tgSBudWxsQHB5dGhvbi5vcmcAdXNlckBleGFtcGxlLm9yZ4YpaHR0cDovL251
bGwucHl0aG9uLm9yZwBodHRwOi8vZXhhbXBsZS5vcmeHBMAAAgGHECABDbgAAAAA
AAAAAAAAAAEwDQYJKoZIhvcNAQEFBQADggEBAKxPRe99SaghcI6IWT7UNkJw9aO9
i9eo0Fj2MUqxpKbdb9noRDy2CnHWf7EIYZ1gznXPdwzSN4YCjV5d+Q9xtBaowT0j
HPERs1ZuytCNNJTmhyqZ8q6uzMLoht4IqH/FBfpvgaeC5tBTnTT0rD5A/olXeimk
kX4LxlEx5RAvpGB2zZVRGr6LobD9rVK91xuHYNIxxxfEGE8tCCWjp0+3ksri9SXx
VHWBnbM9YaL32u3hxm8sYB/Yb8WSBavJCWJJqRStVRHM1koZlJmXNx2BX4vPo6iW
RFEIPQsFZRLrtnCAiEhyT8bC2s/Njlu6ly9gtJZWSV46Q3ZjBL4q9sHKqZQ=
-----END CERTIFICATE-----
PEM;
$certs['bad_time'] = <<<PEM
-----BEGIN CERTIFICATE-----
MIIEpDCCA4ygAwIBAgIJAJzu8r6u6eBcMA0GCSqGSIb3DQEBBQUAMIHDMQswCQYD
VQQGEwJERTEcMBoGA1UECAwTTm9yZHJoZWluLVdlc3RmYWxlbjEQMA4GA1UEBwwH
S8ODwrZsbjEUMBIGA1UECgwLU2VrdGlvbkVpbnMxHzAdBgNVBAsMFk1hbGljaW91
cyBDZXJ0IFNlY3Rpb24xITAfBgNVBAMMGG1hbGljaW91cy5zZWt0aW9uZWlucy5k
ZTEqMCgGCSqGSIb3DQEJARYbc3RlZmFuLmVzc2VyQHNla3Rpb25laW5zLmRlMHUY
ZDE5NzAwMTAxMDAwMDAwWgAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA
AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA
AAAAAAAXDTE0MTEyODExMzkzNVowgcMxCzAJBgNVBAYTAkRFMRwwGgYDVQQIDBNO
b3JkcmhlaW4tV2VzdGZhbGVuMRAwDgYDVQQHDAdLw4PCtmxuMRQwEgYDVQQKDAtT
ZWt0aW9uRWluczEfMB0GA1UECwwWTWFsaWNpb3VzIENlcnQgU2VjdGlvbjEhMB8G
A1UEAwwYbWFsaWNpb3VzLnNla3Rpb25laW5zLmRlMSowKAYJKoZIhvcNAQkBFhtz
dGVmYW4uZXNzZXJAc2VrdGlvbmVpbnMuZGUwggEiMA0GCSqGSIb3DQEBAQUAA4IB
DwAwggEKAoIBAQDDAf3hl7JY0XcFniyEJpSSDqn0OqBr6QP65usJPRt/8PaDoqBu
wEYT/Na+6fsgPjC0uK9DZgWg2tHWWoanSblAMoz5PH6Z+S4SHRZ7e2dDIjPjdhjh
0mLg2UMO5yp0V797Ggs9lNt6JRfH81MN2obXWs4NtztLMuD6egqpr8dDbr34aOs8
pkdui5UawTZksy5pLPHq5cMhFGm06v65CLo0V2Pd9+KAokPrPcN5KLKebz7mLpk6
SMeEXOKP4idEqxyQ7O7fBuHMedsQhu+prY3si3BUyKfQtP5CZnX2bp0wKHxX12DX
1nfFIt9DbGvHTcyOuN+nZLPBm3vWxntyIIvVAgMBAAGjQjBAMAkGA1UdEwQCMAAw
EQYJYIZIAYb4QgEBBAQDAgeAMAsGA1UdDwQEAwIFoDATBgNVHSUEDDAKBggrBgEF
BQcDAjANBgkqhkiG9w0BAQUFAAOCAQEAG0fZYYCTbdj1XYc+1SnoaPR+vI8C8CaD
8+0UYhdnyU4gga0BAcDrY9e94eEAu6ZqycF6FjLqXXdAboppWocr6T6GD1x33Ckl
VArzG/KxQohGD2JeqkhIMlDomxHO7ka39+Oa8i2vWLVyjU8AZvWMAruHa4EENyG7
lW2AagaFKFCr9TnXTfrdxGVEbv7KVQ6bdhg5p5SjpWH1+Mq03uR3ZXPBYdyV8319
o0lVj1KFI2DCL/liWisJRoof+1cR35Ctd0wYBcpB6TZslMcOPl76dwKwJgeJo2Qg
Zsfmc2vC1/qOlNuNq/0TzzkVGv8ETT3CgaU+UXe4XOVvkccebJn2dg==
-----END CERTIFICATE-----
PEM;
$certs['dsa'] = <<<PEM
-----BEGIN CERTIFICATE-----
MIIFHDCCBMqgAwIBAgIJEjRWeJCrze8SMAsGCWCGSAFlAwQDAjCByDELMAkGA1UE
BhMCVUsxETAPBgNVBAgTCFNvbWVyc2V0MRQwEgYDVQQHEwtHbGFzdG9uYnVyeTEf
MB0GA1UEChMWVGhlIEJyYWluIFJvb20gTGltaXRlZDEfMB0GA1UECxMWUEhQIERv
Y3VtZW50YXRpb24gVGVhbTEUMBIGA1UEAxMLV2V6IEZ1cmxvbmcxHjAcBgkqhkiG
9w0BCQEWD3dlekBleGFtcGxlLmNvbTEYMBYGCgmSJomT8ixkAQETCHNvbWUgdWlk
MB4XDTI2MTAyNjIwMzEzNloXDTI3MTAyNjIwMzEzNlowgcgxCzAJBgNVBAYTAlVL
MREwDwYDVQQIEwhTb21lcnNldDEUMBIGA1UEBxMLR2xhc3RvbmJ1cnkxHzAdBgNV
BAoTFlRoZSBCcmFpbiBSb29tIExpbWl0ZWQxHzAdBgNVBAsTFlBIUCBEb2N1bWVu
dGF0aW9uIFRlYW0xFDASBgNVBAMTC1dleiBGdXJsb25nMR4wHAYJKoZIhvcNAQkB
Fg93ZXpAZXhhbXBsZS5jb20xGDAWBgoJkiaJk/IsZAEBEwhzb21lIHVpZDCCAb4w
ggEzBgcqhkjOOAQBMIIBJgKBgQCsu8+CA+0DImv9I2xjTtKtG5PLBmicQrRfMCyS
ejgoNC9qoh2Qb/6iDXVsHe5GMG1g12Mu5g2bc7uDdZMwXNpOtZpzJXNwn2KnEIhK
PUWyALH7q5Pv+eEXNavYtOEXK0qXWXb2AF8kWl/djjo/K/bL7Zunf6wXFXW74ay/
9F7qDQIdANa59cqNw18z17Gdzx/smmPtiHYoCa0nGmGKm7ECgYBrFs3lz3WKdLnq
ZvJLhdB9v/BjRo6B4mRYei+FbKK39ts74W+J5skg9vHs7UvQUNgbjb6jpVIbrJEF
uOkv0wezkg3i56wcwVa0NXzXuVNI05xeVxHEbUOE5xwZslUy3R9BtRuv9uaQYtHw
1DIj0zSIR7WuX/360yds6Cqtdmv0EwOBhAACgYBxPjTTsFBz5S8U9WC2uJfM/qPk
C7Mq9s7zTDOtMjcilZt288uQ8u4iuxVvVjDP+9Qu+ZL9D6jVPR93BR5z1fxBFAlH
3+mw1mc/J69iWctD2Lxk8UzSBeC+6C6B8Mi+tgpD6McG4EhgnTpGv4cqBWxd4cQ3
orzTwN6hMab4OgQtWqOCATEwggEtMB0GA1UdDgQWBBS8++ubEFEeHkrIpTzZFvQ/
t2waNTCB/QYDVR0jBIH1MIHygBS8++ubEFEeHkrIpTzZFvQ/t2waNaGBzqSByzCB
yDELMAkGA1UEBhMCVUsxETAPBgNVBAgTCFNvbWVyc2V0MRQwEgYDVQQHEwtHbGFz
dG9uYnVyeTEfMB0GA1UEChMWVGhlIEJyYWluIFJvb20gTGltaXRlZDEfMB0GA1UE
CxMWUEhQIERvY3VtZW50YXRpb24gVGVhbTEUMBIGA1UEAxMLV2V6IEZ1cmxvbmcx
HjAcBgkqhkiG9w0BCQEWD3dlekBleGFtcGxlLmNvbTEYMBYGCgmSJomT8ixkAQET
CHNvbWUgdWlkggkSNFZ4kKvN7xIwDAYDVR0TBAUwAwEB/zALBglghkgBZQMEAwID
PwAwPAIcJN+q+o7nBDVEyWKJ/nZnIDTMx0/L1uDjI/CbTQIcfWNTY8lrmQSat1so
/3baLnUx9CvdsH0DTknDYA==
-----END CERTIFICATE-----
PEM;
$certs['ec'] = <<<PEM
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
PEM;
$csr = <<<PEM
-----BEGIN CERTIFICATE REQUEST-----
MIIDDzCCArsCAQAwgcgxCzAJBgNVBAYTAlVLMREwDwYDVQQIEwhTb21lcnNldDEU
MBIGA1UEBxMLR2xhc3RvbmJ1cnkxHzAdBgNVBAoTFlRoZSBCcmFpbiBSb29tIExp
bWl0ZWQxHzAdBgNVBAsTFlBIUCBEb2N1bWVudGF0aW9uIFRlYW0xFDASBgNVBAMT
C1dleiBGdXJsb25nMR4wHAYJKoZIhvcNAQkBFg93ZXpAZXhhbXBsZS5jb20xGDAW
BgoJkiaJk/IsZAEBEwhzb21lIHVpZDCCAb4wggEzBgcqhkjOOAQBMIIBJgKBgQCs
u8+CA+0DImv9I2xjTtKtG5PLBmicQrRfMCySejgoNC9qoh2Qb/6iDXVsHe5GMG1g
12Mu5g2bc7uDdZMwXNpOtZpzJXNwn2KnEIhKPUWyALH7q5Pv+eEXNavYtOEXK0qX
WXb2AF8kWl/djjo/K/bL7Zunf6wXFXW74ay/9F7qDQIdANa59cqNw18z17Gdzx/s
mmPtiHYoCa0nGmGKm7ECgYBrFs3lz3WKdLnqZvJLhdB9v/BjRo6B4mRYei+FbKK3
9ts74W+J5skg9vHs7UvQUNgbjb6jpVIbrJEFuOkv0wezkg3i56wcwVa0NXzXuVNI
05xeVxHEbUOE5xwZslUy3R9BtRuv9uaQYtHw1DIj0zSIR7WuX/360yds6Cqtdmv0
EwOBhAACgYBxPjTTsFBz5S8U9WC2uJfM/qPkC7Mq9s7zTDOtMjcilZt288uQ8u4i
uxVvVjDP+9Qu+ZL9D6jVPR93BR5z1fxBFAlH3+mw1mc/J69iWctD2Lxk8UzSBeC+
6C6B8Mi+tgpD6McG4EhgnTpGv4cqBWxd4cQ3orzTwN6hMab4OgQtWqApMCcGCSqG
SIb3DQEJDjEaMBgwCQYDVR0TBAIwADALBgNVHQ8EBAMCBeAwCwYJYIZIAWUDBAMC
A0EAMD4CHQCHdgYXU+kDYkE2KR0T9YkSDW6KC+JaMzzLNwD0Ah0AiJEe1XnjF/Rv
pJLVeLfezy711WJDw9apaVUq+g==
-----END CERTIFICATE REQUEST-----
PEM;
$eckey = <<<PEM
-----BEGIN PRIVATE KEY-----
MIG2AgEAMBAGByqGSM49AgEGBSuBBAAiBIGeMIGbAgEBBDA1zB0U8oOv1iBCiHxZ
l2Z5d9KeOmXy3os+MUMbZyFVMHgKvqVrERw6VPWnx+zYk1ShZANiAAQihQsgSsX+
R6mdbPJPlHPryR5JAo8ZOesY+7PWE0zs3npD3od+rCN20youI886WFVIQWj/wS1t
oe4X3C+AnEi9419LL4zcb6QQStNPeWUgCvrkpQiEfzCk17soUXYzPSQ=
-----END PRIVATE KEY-----
PEM;
foreach ($certs as $n => $pem) {
    echo "=== $n\n";
    $c = @openssl_x509_read($pem);
    openssl_x509_export($c, $out, false);
    echo substr($out, 0, strpos($out, "-----BEGIN"));
    $p = openssl_x509_parse($c);
    echo "codesign: ", json_encode($p['purposes'][10] ?? null), "\n";
    echo "check ec key: ", var_export(openssl_x509_check_private_key($c, $eckey), true), errs(), "\n";
}
openssl_csr_export($csr, $out, false);
echo substr($out, 0, strpos($out, "-----BEGIN"));
echo "verify: ", openssl_x509_verify($certs['unknown_ext'], openssl_pkey_get_public($certs['san_nul'])), errs(), "\n";
var_dump(@openssl_x509_read('file:///nonexistent/x.pem'));
echo errs(), "\n";
?>
--EXPECTF--
=== unknown_ext
Certificate:
    Data:
        Version: 3 (0x2)
        Serial Number: 39 (0x27)
        Signature Algorithm: md5WithRSAEncryption
        Issuer: teletexTerminalIdentifier=RO, ST=Romania, L=Craiova, O=Sergiu, OU=Sergiu SRL, CN=Sergiu CA/emailAddress=n_sergiu@hotmail.com
        Validity
            Not Before: May 14 13:34:56 2004 GMT
            Not After : May 14 13:34:56 2005 GMT
        Subject: C=RO, ST=Romania, L=Craiova, O=Sergiu SRL, OU=Sergiu SRL, CN=Sergiu personal/emailAddress=n_sergiu@hotmail.com
        Subject Public Key Info:
            Public Key Algorithm: rsaEncryption
                Public-Key: (1024 bit)
                Modulus:
                    00:a4:d8:fb:5d:7c:fc:4f:c1:5c:2c:85:a9:06:78:
                    8f:62:89:b7:41:c4:fa:4f:bb:b4:c5:13:c7:aa:da:
                    b3:8f:9a:1b:a0:16:29:0c:97:b9:77:7e:78:ff:2d:
                    20:25:3a:62:2e:df:3e:7d:3a:cf:81:65:e7:6c:79:
                    b7:a4:e1:e0:5f:34:dc:5f:a0:2b:6a:78:b4:18:35:
                    34:ff:10:e2:e1:59:02:44:67:12:62:a5:f6:b0:9a:
                    5c:0f:30:1b:98:af:54:0c:cb:77:c5:ff:ce:d4:1f:
                    00:25:a9:f7:45:f3:b4:06:6d:e8:cd:31:0f:ce:22:
                    cc:92:6b:22:62:be:5b:fc:be
                Exponent: 65537 (0x10001)
        X509v3 extensions:
            X509v3 Basic Constraints: 
                CA:FALSE
            Netscape Comment: 
                For Grid use only; request tag userTag
            Netscape Cert Type: 
                SSL Client, SSL Server, S/MIME
            X509v3 CRL Distribution Points: 
                Full Name:
                  URI:http://mobile.blue-software.ro:90/ca/crl.shtml

            Netscape CA Policy Url: 
                http://mobile.blue-software.ro:90/pub/
            X509v3 Subject Alternative Name: 
                email:sergiu@bluesoftware.ro
            X509v3 Subject Key Identifier: 
                B0:A7:FF:F9:41:15:DE:23:39:BD:DD:31:0F:97:A0:B2:A2:74:E0:FC
            X509v3 Authority Key Identifier: 
                DirName:/C=RO/ST=Romania/L=Craiova/O=Sergiu/OU=Sergiu SRL/CN=Sergiu CA/emailAddress=n_sergiu@hotmail.com
                serial:00
            X509v3 Key Usage: 
                Digital Signature, Non Repudiation, Key Encipherment, Data Encipherment
            Netscape Base Url: 
                http://62.231.98.52/
            1.2.3.4: 
                >.b4
    Signature Algorithm: md5WithRSAEncryption
    Signature Value:
        80:48:38:9f:a2:88:bc:90:7c:d2:44:63:e2:0c:79:f6:b2:41:
        e6:b4:9e:6b:97:61:8f:85:f8:bd:43:16:34:82:ef:10:a1:ca:
        d8:18:a0:f3:67:2a:48:61:dd:d5:8f:b7:16:ec:c2:2a:8a:61:
        49:c6:8b:ac:b7:1e:2e:14:84:3b:0e:68:35:b6:f6:dd:92:df:
        4e:61:49:cd:aa:82:33:a2:36:d4:58:40:85:58:43:c1:07:1b:
        59:3b:16:5e:b7:4b:bb:82:3a:19:d5:26:df:b8:32:75:0a:d4:
        de:bf:a7:08:2c:a7:e6:76:1a:f1:2c:0c:6d:6a:08:a9:ef:36:
        97:fa
codesign: [false,false,"codesign"]
check ec key: false [error:03000065:digital envelope routines::different key types | error:05800073:x509 certificate routines::key type mismatch]
=== san_nul
Certificate:
    Data:
        Version: 3 (0x2)
        Serial Number: 0 (0x0)
        Signature Algorithm: sha1WithRSAEncryption
        Issuer: C=US, ST=Oregon, L=Beaverton, O=Python Software Foundation, OU=Python Core Development, CN=null.python.org\x00example.org/emailAddress=python-dev@python.org
        Validity
            Not Before: Aug  7 13:11:52 2013 GMT
            Not After : Aug  7 13:12:52 2013 GMT
        Subject: C=US, ST=Oregon, L=Beaverton, O=Python Software Foundation, OU=Python Core Development, CN=null.python.org\x00example.org/emailAddress=python-dev@python.org
        Subject Public Key Info:
            Public Key Algorithm: rsaEncryption
                Public-Key: (2048 bit)
                Modulus:
                    00:b5:ea:ed:c9:fb:46:7d:6f:3b:76:80:dd:3a:f3:
                    03:94:0b:a7:a6:db:ec:1d:df:ff:23:74:08:9d:97:
                    16:3f:a3:a4:7b:3e:1b:0e:96:59:25:03:a7:26:e2:
                    88:a9:cf:79:cd:f7:04:56:b0:ab:79:32:6e:59:c1:
                    32:30:54:eb:58:a8:cb:91:f0:42:a5:64:27:cb:d4:
                    56:31:88:52:ad:cf:bd:7f:f0:06:64:1f:cc:27:b8:
                    a3:8b:8c:f3:d8:29:1f:25:0b:f5:46:06:1b:ca:02:
                    45:ad:7b:76:0a:9c:bf:bb:b9:ae:0d:16:ab:60:75:
                    ae:06:3e:9c:7c:31:dc:92:2f:29:1a:e0:4b:0c:91:
                    90:6c:e9:37:c5:90:d7:2a:d7:97:15:a3:80:8f:5d:
                    7b:49:8f:54:30:d4:97:2c:1c:5b:37:b5:ab:69:30:
                    68:43:d3:33:78:4b:02:60:f5:3c:44:80:a1:8f:e7:
                    f0:0f:d1:5e:87:9e:46:cf:62:fc:f9:bf:0c:65:12:
                    f1:93:c8:35:79:3f:c8:ec:ec:47:f5:ef:be:44:d5:
                    ae:82:1e:2d:9a:9f:98:5a:67:65:e1:74:70:7c:cb:
                    d3:c2:ce:0e:45:49:27:dc:e3:2d:d4:fb:48:0e:2f:
                    9e:77:b8:14:46:c0:c4:36:ca:02:ae:6a:91:8c:da:
                    2f:85
                Exponent: 65537 (0x10001)
        X509v3 extensions:
            X509v3 Basic Constraints: critical
                CA:FALSE
            X509v3 Subject Key Identifier: 
                88:5A:55:C0:52:FF:61:CD:52:A3:35:0F:EA:5A:9C:24:38:22:F7:5C
            X509v3 Key Usage: 
                Digital Signature, Non Repudiation, Key Encipherment
            X509v3 Subject Alternative Name: 
                0....altnull.python.org.example.com. null@python.org.user@example.org.)http://null.python.org.http://example.org........ ..............
    Signature Algorithm: sha1WithRSAEncryption
    Signature Value:
        ac:4f:45:ef:7d:49:a8:21:70:8e:88:59:3e:d4:36:42:70:f5:
        a3:bd:8b:d7:a8:d0:58:f6:31:4a:b1:a4:a6:dd:6f:d9:e8:44:
        3c:b6:0a:71:d6:7f:b1:08:61:9d:60:ce:75:cf:77:0c:d2:37:
        86:02:8d:5e:5d:f9:0f:71:b4:16:a8:c1:3d:23:1c:f1:11:b3:
        56:6e:ca:d0:8d:34:94:e6:87:2a:99:f2:ae:ae:cc:c2:e8:86:
        de:08:a8:7f:c5:05:fa:6f:81:a7:82:e6:d0:53:9d:34:f4:ac:
        3e:40:fe:89:57:7a:29:a4:91:7e:0b:c6:51:31:e5:10:2f:a4:
        60:76:cd:95:51:1a:be:8b:a1:b0:fd:ad:52:bd:d7:1b:87:60:
        d2:31:c7:17:c4:18:4f:2d:08:25:a3:a7:4f:b7:92:ca:e2:f5:
        25:f1:54:75:81:9d:b3:3d:61:a2:f7:da:ed:e1:c6:6f:2c:60:
        1f:d8:6f:c5:92:05:ab:c9:09:62:49:a9:14:ad:55:11:cc:d6:
        4a:19:94:99:97:37:1d:81:5f:8b:cf:a3:a8:96:44:51:08:3d:
        0b:05:65:12:eb:b6:70:80:88:48:72:4f:c6:c2:da:cf:cd:8e:
        5b:ba:97:2f:60:b4:96:56:49:5e:3a:43:76:63:04:be:2a:f6:
        c1:ca:a9:94
codesign: [false,false,"codesign"]
check ec key: false [error:03000065:digital envelope routines::different key types | error:05800073:x509 certificate routines::key type mismatch]
=== bad_time
Certificate:
    Data:
        Version: 3 (0x2)
        Serial Number:
            9c:ee:f2:be:ae:e9:e0:5c
        Signature Algorithm: sha1WithRSAEncryption
        Issuer: C=DE, ST=Nordrhein-Westfalen, L=K\xC3\x83\xC2\xB6ln, O=SektionEins, OU=Malicious Cert Section, CN=malicious.sektioneins.de/emailAddress=stefan.esser@sektioneins.de
        Validity
            Not Before: Bad time value
            Not After : Nov 28 11:39:35 2014 GMT
        Subject: C=DE, ST=Nordrhein-Westfalen, L=K\xC3\x83\xC2\xB6ln, O=SektionEins, OU=Malicious Cert Section, CN=malicious.sektioneins.de/emailAddress=stefan.esser@sektioneins.de
        Subject Public Key Info:
            Public Key Algorithm: rsaEncryption
                Public-Key: (2048 bit)
                Modulus:
                    00:c3:01:fd:e1:97:b2:58:d1:77:05:9e:2c:84:26:
                    94:92:0e:a9:f4:3a:a0:6b:e9:03:fa:e6:eb:09:3d:
                    1b:7f:f0:f6:83:a2:a0:6e:c0:46:13:fc:d6:be:e9:
                    fb:20:3e:30:b4:b8:af:43:66:05:a0:da:d1:d6:5a:
                    86:a7:49:b9:40:32:8c:f9:3c:7e:99:f9:2e:12:1d:
                    16:7b:7b:67:43:22:33:e3:76:18:e1:d2:62:e0:d9:
                    43:0e:e7:2a:74:57:bf:7b:1a:0b:3d:94:db:7a:25:
                    17:c7:f3:53:0d:da:86:d7:5a:ce:0d:b7:3b:4b:32:
                    e0:fa:7a:0a:a9:af:c7:43:6e:bd:f8:68:eb:3c:a6:
                    47:6e:8b:95:1a:c1:36:64:b3:2e:69:2c:f1:ea:e5:
                    c3:21:14:69:b4:ea:fe:b9:08:ba:34:57:63:dd:f7:
                    e2:80:a2:43:eb:3d:c3:79:28:b2:9e:6f:3e:e6:2e:
                    99:3a:48:c7:84:5c:e2:8f:e2:27:44:ab:1c:90:ec:
                    ee:df:06:e1:cc:79:db:10:86:ef:a9:ad:8d:ec:8b:
                    70:54:c8:a7:d0:b4:fe:42:66:75:f6:6e:9d:30:28:
                    7c:57:d7:60:d7:d6:77:c5:22:df:43:6c:6b:c7:4d:
                    cc:8e:b8:df:a7:64:b3:c1:9b:7b:d6:c6:7b:72:20:
                    8b:d5
                Exponent: 65537 (0x10001)
        X509v3 extensions:
            X509v3 Basic Constraints: 
                CA:FALSE
            Netscape Cert Type: 
                SSL Client
            X509v3 Key Usage: 
                Digital Signature, Key Encipherment
            X509v3 Extended Key Usage: 
                TLS Web Client Authentication
    Signature Algorithm: sha1WithRSAEncryption
    Signature Value:
        1b:47:d9:61:80:93:6d:d8:f5:5d:87:3e:d5:29:e8:68:f4:7e:
        bc:8f:02:f0:26:83:f3:ed:14:62:17:67:c9:4e:20:81:ad:01:
        01:c0:eb:63:d7:bd:e1:e1:00:bb:a6:6a:c9:c1:7a:16:32:ea:
        5d:77:40:6e:8a:69:5a:87:2b:e9:3e:86:0f:5c:77:dc:29:25:
        54:0a:f3:1b:f2:b1:42:88:46:0f:62:5e:aa:48:48:32:50:e8:
        9b:11:ce:ee:46:b7:f7:e3:9a:f2:2d:af:58:b5:72:8d:4f:00:
        66:f5:8c:02:bb:87:6b:81:04:37:21:bb:95:6d:80:6a:06:85:
        28:50:ab:f5:39:d7:4d:fa:dd:c4:65:44:6e:fe:ca:55:0e:9b:
        76:18:39:a7:94:a3:a5:61:f5:f8:ca:b4:de:e4:77:65:73:c1:
        61:dc:95:f3:7d:7d:a3:49:55:8f:52:85:23:60:c2:2f:f9:62:
        5a:2b:09:46:8a:1f:fb:57:11:df:90:ad:77:4c:18:05:ca:41:
        e9:36:6c:94:c7:0e:3e:5e:fa:77:02:b0:26:07:89:a3:64:20:
        66:c7:e6:73:6b:c2:d7:fa:8e:94:db:8d:ab:fd:13:cf:39:15:
        1a:ff:04:4d:3d:c2:81:a5:3e:51:77:b8:5c:e5:6f:91:c7:1e:
        6c:99:f6:76

Warning: openssl_x509_parse(): Illegal length in timestamp in %s on line 184
codesign: [false,false,"codesign"]
check ec key: false [error:03000065:digital envelope routines::different key types | error:05800073:x509 certificate routines::key type mismatch]
=== dsa
Certificate:
    Data:
        Version: 3 (0x2)
        Serial Number:
            12:34:56:78:90:ab:cd:ef:12
        Signature Algorithm: dsa_with_SHA256
        Issuer: C=UK, ST=Somerset, L=Glastonbury, O=The Brain Room Limited, OU=PHP Documentation Team, CN=Wez Furlong/emailAddress=wez@example.com/UID=some uid
        Validity
            Not Before: Oct 26 20:31:36 2026 GMT
            Not After : Oct 26 20:31:36 2027 GMT
        Subject: C=UK, ST=Somerset, L=Glastonbury, O=The Brain Room Limited, OU=PHP Documentation Team, CN=Wez Furlong/emailAddress=wez@example.com/UID=some uid
        Subject Public Key Info:
            Public Key Algorithm: dsaEncryption
                Public-Key: (1024 bit)
                pub: 
                    71:3e:34:d3:b0:50:73:e5:2f:14:f5:60:b6:b8:97:
                    cc:fe:a3:e4:0b:b3:2a:f6:ce:f3:4c:33:ad:32:37:
                    22:95:9b:76:f3:cb:90:f2:ee:22:bb:15:6f:56:30:
                    cf:fb:d4:2e:f9:92:fd:0f:a8:d5:3d:1f:77:05:1e:
                    73:d5:fc:41:14:09:47:df:e9:b0:d6:67:3f:27:af:
                    62:59:cb:43:d8:bc:64:f1:4c:d2:05:e0:be:e8:2e:
                    81:f0:c8:be:b6:0a:43:e8:c7:06:e0:48:60:9d:3a:
                    46:bf:87:2a:05:6c:5d:e1:c4:37:a2:bc:d3:c0:de:
                    a1:31:a6:f8:3a:04:2d:5a
                P:   
                    00:ac:bb:cf:82:03:ed:03:22:6b:fd:23:6c:63:4e:
                    d2:ad:1b:93:cb:06:68:9c:42:b4:5f:30:2c:92:7a:
                    38:28:34:2f:6a:a2:1d:90:6f:fe:a2:0d:75:6c:1d:
                    ee:46:30:6d:60:d7:63:2e:e6:0d:9b:73:bb:83:75:
                    93:30:5c:da:4e:b5:9a:73:25:73:70:9f:62:a7:10:
                    88:4a:3d:45:b2:00:b1:fb:ab:93:ef:f9:e1:17:35:
                    ab:d8:b4:e1:17:2b:4a:97:59:76:f6:00:5f:24:5a:
                    5f:dd:8e:3a:3f:2b:f6:cb:ed:9b:a7:7f:ac:17:15:
                    75:bb:e1:ac:bf:f4:5e:ea:0d
                Q:   
                    00:d6:b9:f5:ca:8d:c3:5f:33:d7:b1:9d:cf:1f:ec:
                    9a:63:ed:88:76:28:09:ad:27:1a:61:8a:9b:b1
                G:   
                    6b:16:cd:e5:cf:75:8a:74:b9:ea:66:f2:4b:85:d0:
                    7d:bf:f0:63:46:8e:81:e2:64:58:7a:2f:85:6c:a2:
                    b7:f6:db:3b:e1:6f:89:e6:c9:20:f6:f1:ec:ed:4b:
                    d0:50:d8:1b:8d:be:a3:a5:52:1b:ac:91:05:b8:e9:
                    2f:d3:07:b3:92:0d:e2:e7:ac:1c:c1:56:b4:35:7c:
                    d7:b9:53:48:d3:9c:5e:57:11:c4:6d:43:84:e7:1c:
                    19:b2:55:32:dd:1f:41:b5:1b:af:f6:e6:90:62:d1:
                    f0:d4:32:23:d3:34:88:47:b5:ae:5f:fd:fa:d3:27:
                    6c:e8:2a:ad:76:6b:f4:13
        X509v3 extensions:
            X509v3 Subject Key Identifier: 
                BC:FB:EB:9B:10:51:1E:1E:4A:C8:A5:3C:D9:16:F4:3F:B7:6C:1A:35
            X509v3 Authority Key Identifier: 
                keyid:BC:FB:EB:9B:10:51:1E:1E:4A:C8:A5:3C:D9:16:F4:3F:B7:6C:1A:35
                DirName:/C=UK/ST=Somerset/L=Glastonbury/O=The Brain Room Limited/OU=PHP Documentation Team/CN=Wez Furlong/emailAddress=wez@example.com/UID=some uid
                serial:12:34:56:78:90:AB:CD:EF:12
            X509v3 Basic Constraints: 
                CA:TRUE
    Signature Algorithm: dsa_with_SHA256
    Signature Value:
        r:   
            24:df:aa:fa:8e:e7:04:35:44:c9:62:89:fe:76:67:
            20:34:cc:c7:4f:cb:d6:e0:e3:23:f0:9b:4d
        s:   
            7d:63:53:63:c9:6b:99:04:9a:b7:5b:28:ff:76:da:
            2e:75:31:f4:2b:dd:b0:7d:03:4e:49:c3:60
codesign: [false,true,"codesign"]
check ec key: false [error:03000065:digital envelope routines::different key types | error:05800073:x509 certificate routines::key type mismatch]
=== ec
Certificate:
    Data:
        Version: 3 (0x2)
        Serial Number:
            12:34:56:78:90:ab:cd:ef:12
        Signature Algorithm: ecdsa-with-SHA256
        Issuer: C=UK, ST=Somerset, L=Glastonbury, O=The Brain Room Limited, OU=PHP Documentation Team, CN=Wez Furlong/emailAddress=wez@example.com/UID=some uid
        Validity
            Not Before: Oct 26 20:31:36 2026 GMT
            Not After : Oct 26 20:31:36 2027 GMT
        Subject: C=UK, ST=Somerset, L=Glastonbury, O=The Brain Room Limited, OU=PHP Documentation Team, CN=Wez Furlong/emailAddress=wez@example.com/UID=some uid
        Subject Public Key Info:
            Public Key Algorithm: id-ecPublicKey
                Public-Key: (384 bit)
                pub:
                    04:22:85:0b:20:4a:c5:fe:47:a9:9d:6c:f2:4f:94:
                    73:eb:c9:1e:49:02:8f:19:39:eb:18:fb:b3:d6:13:
                    4c:ec:de:7a:43:de:87:7e:ac:23:76:d3:2a:2e:23:
                    cf:3a:58:55:48:41:68:ff:c1:2d:6d:a1:ee:17:dc:
                    2f:80:9c:48:bd:e3:5f:4b:2f:8c:dc:6f:a4:10:4a:
                    d3:4f:79:65:20:0a:fa:e4:a5:08:84:7f:30:a4:d7:
                    bb:28:51:76:33:3d:24
                ASN1 OID: secp384r1
                NIST CURVE: P-384
        X509v3 extensions:
            X509v3 Subject Key Identifier: 
                8F:3B:1D:74:82:88:14:19:1F:FB:1C:69:2E:40:19:6C:D1:4C:BA:A5
            X509v3 Authority Key Identifier: 
                keyid:8F:3B:1D:74:82:88:14:19:1F:FB:1C:69:2E:40:19:6C:D1:4C:BA:A5
                DirName:/C=UK/ST=Somerset/L=Glastonbury/O=The Brain Room Limited/OU=PHP Documentation Team/CN=Wez Furlong/emailAddress=wez@example.com/UID=some uid
                serial:12:34:56:78:90:AB:CD:EF:12
            X509v3 Basic Constraints: 
                CA:TRUE
    Signature Algorithm: ecdsa-with-SHA256
    Signature Value:
        30:65:02:30:55:e9:11:b3:da:5e:9f:8d:c6:3a:28:10:12:6f:
        8d:22:8e:95:33:94:17:da:f6:8c:d6:a7:8d:26:a5:53:1a:54:
        93:49:ef:e4:91:c2:da:a4:ef:dd:0e:0d:94:1a:70:bb:02:31:
        00:ec:08:1d:2c:72:65:02:36:24:19:89:9c:bd:14:3a:b4:c3:
        16:4d:cb:ae:d4:d3:89:59:4e:48:b4:58:11:8d:fd:1b:4c:7b:
        10:7a:ab:9f:f7:16:7f:07:06:61:1b:ef:74
codesign: [false,true,"codesign"]
check ec key: true
Certificate Request:
    Data:
        Version: 1 (0x0)
        Subject: C=UK, ST=Somerset, L=Glastonbury, O=The Brain Room Limited, OU=PHP Documentation Team, CN=Wez Furlong/emailAddress=wez@example.com/UID=some uid
        Subject Public Key Info:
            Public Key Algorithm: dsaEncryption
                Public-Key: (1024 bit)
                pub: 
                    71:3e:34:d3:b0:50:73:e5:2f:14:f5:60:b6:b8:97:
                    cc:fe:a3:e4:0b:b3:2a:f6:ce:f3:4c:33:ad:32:37:
                    22:95:9b:76:f3:cb:90:f2:ee:22:bb:15:6f:56:30:
                    cf:fb:d4:2e:f9:92:fd:0f:a8:d5:3d:1f:77:05:1e:
                    73:d5:fc:41:14:09:47:df:e9:b0:d6:67:3f:27:af:
                    62:59:cb:43:d8:bc:64:f1:4c:d2:05:e0:be:e8:2e:
                    81:f0:c8:be:b6:0a:43:e8:c7:06:e0:48:60:9d:3a:
                    46:bf:87:2a:05:6c:5d:e1:c4:37:a2:bc:d3:c0:de:
                    a1:31:a6:f8:3a:04:2d:5a
                P:   
                    00:ac:bb:cf:82:03:ed:03:22:6b:fd:23:6c:63:4e:
                    d2:ad:1b:93:cb:06:68:9c:42:b4:5f:30:2c:92:7a:
                    38:28:34:2f:6a:a2:1d:90:6f:fe:a2:0d:75:6c:1d:
                    ee:46:30:6d:60:d7:63:2e:e6:0d:9b:73:bb:83:75:
                    93:30:5c:da:4e:b5:9a:73:25:73:70:9f:62:a7:10:
                    88:4a:3d:45:b2:00:b1:fb:ab:93:ef:f9:e1:17:35:
                    ab:d8:b4:e1:17:2b:4a:97:59:76:f6:00:5f:24:5a:
                    5f:dd:8e:3a:3f:2b:f6:cb:ed:9b:a7:7f:ac:17:15:
                    75:bb:e1:ac:bf:f4:5e:ea:0d
                Q:   
                    00:d6:b9:f5:ca:8d:c3:5f:33:d7:b1:9d:cf:1f:ec:
                    9a:63:ed:88:76:28:09:ad:27:1a:61:8a:9b:b1
                G:   
                    6b:16:cd:e5:cf:75:8a:74:b9:ea:66:f2:4b:85:d0:
                    7d:bf:f0:63:46:8e:81:e2:64:58:7a:2f:85:6c:a2:
                    b7:f6:db:3b:e1:6f:89:e6:c9:20:f6:f1:ec:ed:4b:
                    d0:50:d8:1b:8d:be:a3:a5:52:1b:ac:91:05:b8:e9:
                    2f:d3:07:b3:92:0d:e2:e7:ac:1c:c1:56:b4:35:7c:
                    d7:b9:53:48:d3:9c:5e:57:11:c4:6d:43:84:e7:1c:
                    19:b2:55:32:dd:1f:41:b5:1b:af:f6:e6:90:62:d1:
                    f0:d4:32:23:d3:34:88:47:b5:ae:5f:fd:fa:d3:27:
                    6c:e8:2a:ad:76:6b:f4:13
        Attributes:
            Requested Extensions:
                X509v3 Basic Constraints: 
                    CA:FALSE
                X509v3 Key Usage: 
                    Digital Signature, Non Repudiation, Key Encipherment
    Signature Algorithm: dsa_with_SHA256
    Signature Value:
        r:   
            00:87:76:06:17:53:e9:03:62:41:36:29:1d:13:f5:
            89:12:0d:6e:8a:0b:e2:5a:33:3c:cb:37:00:f4
        s:   
            00:88:91:1e:d5:79:e3:17:f4:6f:a4:92:d5:78:b7:
            de:cf:2e:f5:d5:62:43:c3:d6:a9:69:55:2a:fa
verify: 0 [error:02000077:rsa routines::wrong signature length | error:1C880004:Provider routines::RSA lib | error:06880006:asn1 encoding routines::EVP lib]
bool(false)
 [error:FFFFFFFF80000002:system library::No such file or directory | error:10000080:BIO routines::no such file]
