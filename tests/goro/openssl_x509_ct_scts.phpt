--TEST--
openssl_x509_parse()/x509_export text: CT precertificate SCT lists
--EXTENSIONS--
openssl
--FILE--
<?php
// a Let's Encrypt leaf: embedded SCT list (one SCT carrying extensions)
$pem = "-----BEGIN CERTIFICATE-----
MIIGYTCCBeegAwIBAgISBtsVhlLzKTCahufQ6d3+2aSyMAoGCCqGSM49BAMDMDMx
CzAJBgNVBAYTAlVTMRYwFAYDVQQKEw1MZXQncyBFbmNyeXB0MQwwCgYDVQQDEwNZ
RTIwHhcNMjYxMDA0MTgzNDUxWhcNMjcwMTAyMTgzNDUwWjAaMRgwFgYDVQQDDA8q
Lndpa2lwZWRpYS5vcmcwWTATBgcqhkjOPQIBBggqhkjOPQMBBwNCAAQsr9HaeYX/
7Dxego0rApiGbU7kt92SpFWgmdCXaU58wFVjOeF0ZXUCcAQ/q/horD2UxXhHgsn5
dHTtwPs/HV/no4IE8jCCBO4wDgYDVR0PAQH/BAQDAgeAMBMGA1UdJQQMMAoGCCsG
AQUFBwMBMAwGA1UdEwEB/wQCMAAwHQYDVR0OBBYEFOX5/ctVbKfIDnXF+nUAELvp
xOh/MB8GA1UdIwQYMBaAFLlZ8o7PIvCG0zdI/3YUGLqC2FWHMDMGCCsGAQUFBwEB
BCcwJTAjBggrBgEFBQcwAoYXaHR0cDovL3llMi5pLmxlbmNyLm9yZy8wggLtBgNV
HREEggLkMIIC4IIRKi5tLm1lZGlhd2lraS5vcmeCESoubS53aWtpYm9va3Mub3Jn
ghAqLm0ud2lraWRhdGEub3JnghEqLm0ud2lraW1lZGlhLm9yZ4IQKi5tLndpa2lu
ZXdzLm9yZ4IRKi5tLndpa2lwZWRpYS5vcmeCESoubS53aWtpcXVvdGUub3JnghIq
Lm0ud2lraXNvdXJjZS5vcmeCEyoubS53aWtpdmVyc2l0eS5vcmeCEioubS53aWtp
dm95YWdlLm9yZ4ISKi5tLndpa3Rpb25hcnkub3Jngg8qLm1lZGlhd2lraS5vcmeC
FioucGxhbmV0Lndpa2ltZWRpYS5vcmeCDyoud2lraWJvb2tzLm9yZ4IOKi53aWtp
ZGF0YS5vcmeCEyoud2lraWZ1bmN0aW9ucy5vcmeCDyoud2lraW1lZGlhLm9yZ4IZ
Ki53aWtpbWVkaWFmb3VuZGF0aW9uLm9yZ4IOKi53aWtpbmV3cy5vcmeCDyoud2lr
aXBlZGlhLm9yZ4IPKi53aWtpcXVvdGUub3JnghAqLndpa2lzb3VyY2Uub3JnghEq
Lndpa2l2ZXJzaXR5Lm9yZ4IQKi53aWtpdm95YWdlLm9yZ4IQKi53aWt0aW9uYXJ5
Lm9yZ4IUKi53bWZ1c2VyY29udGVudC5vcmeCDW1lZGlhd2lraS5vcmeCBncud2lr
aYINd2lraWJvb2tzLm9yZ4IMd2lraWRhdGEub3JnghF3aWtpZnVuY3Rpb25zLm9y
Z4INd2lraW1lZGlhLm9yZ4IXd2lraW1lZGlhZm91bmRhdGlvbi5vcmeCDHdpa2lu
ZXdzLm9yZ4INd2lraXBlZGlhLm9yZ4INd2lraXF1b3RlLm9yZ4IOd2lraXNvdXJj
ZS5vcmeCD3dpa2l2ZXJzaXR5Lm9yZ4IOd2lraXZveWFnZS5vcmeCDndpa3Rpb25h
cnkub3JnghJ3bWZ1c2VyY29udGVudC5vcmcwEwYDVR0gBAwwCjAIBgZngQwBAgEw
LgYDVR0fBCcwJTAjoCGgH4YdaHR0cDovL3llMi5jLmxlbmNyLm9yZy81MS5jcmww
ggEMBgorBgEEAdZ5AgQCBIH9BIH6APgAdQAcn2gs6frwRWlQ+BuWiofd2zIQ2Ezm
yLLjglJKxM9ZnwAAAaEIaBT8AAAEAwBGMEQCIDICIpr2qKXvLUtcy8q5Y95W/xfS
/mEYtUVqx5qpLPdEAiAwJq5i0xJP4Upg8j5l87ic+4gHKTajU+xXfsvhvVHneQB/
ABqLnWuN15HRzQVJ7bYDVdYGtk+tMNtx/niPD8fI+8SxAAABoQhoGhMACAAABQAF
SvNDBAMASDBGAiEArTwYScUlx8lSb7gDekcvY5Yd66eQQ3ND3fytcKRni/MCIQDK
f8xZqYFuz2x+qyr72dztoUW5avZqz+rt84JFZ6mPOjAKBggqhkjOPQQDAwNoADBl
AjAWj16M6XZ6waQhhz97PRlDAAOZZqQ6OhugSUoyOVdSsCujqx4/TaSJF3EymmTn
tiUCMQDcwGedOzdKAinMp49KLae5U1UvHJCP3SKI/TorL2U/3XtQlHPYq/fZbJqF
5+NMQQE=
-----END CERTIFICATE-----";
$p = openssl_x509_parse($pem);
echo $p['extensions']['ct_precert_scts'], "\n";
openssl_x509_export($pem, $txt, false);
$on = false;
foreach (explode("\n", $txt) as $l) {
  if (str_contains($l, 'CT Precertificate SCTs')) $on = true;
  elseif (str_contains($l, 'Signature Algorithm')) $on = false;
  if ($on) echo $l, "\n";
}
?>
--EXPECT--
Signed Certificate Timestamp:
    Version   : v1 (0x0)
    Log ID    : 1C:9F:68:2C:E9:FA:F0:45:69:50:F8:1B:96:8A:87:DD:
                DB:32:10:D8:4C:E6:C8:B2:E3:82:52:4A:C4:CF:59:9F
    Timestamp : Oct  4 19:33:21.276 2026 GMT
    Extensions: none
    Signature : ecdsa-with-SHA256
                30:44:02:20:32:02:22:9A:F6:A8:A5:EF:2D:4B:5C:CB:
                CA:B9:63:DE:56:FF:17:D2:FE:61:18:B5:45:6A:C7:9A:
                A9:2C:F7:44:02:20:30:26:AE:62:D3:12:4F:E1:4A:60:
                F2:3E:65:F3:B8:9C:FB:88:07:29:36:A3:53:EC:57:7E:
                CB:E1:BD:51:E7:79
Signed Certificate Timestamp:
    Version   : v1 (0x0)
    Log ID    : 1A:8B:9D:6B:8D:D7:91:D1:CD:05:49:ED:B6:03:55:D6:
                06:B6:4F:AD:30:DB:71:FE:78:8F:0F:C7:C8:FB:C4:B1
    Timestamp : Oct  4 19:33:22.579 2026 GMT
    Extensions: 00:00:05:00:05:4A:F3:43
    Signature : ecdsa-with-SHA256
                30:46:02:21:00:AD:3C:18:49:C5:25:C7:C9:52:6F:B8:
                03:7A:47:2F:63:96:1D:EB:A7:90:43:73:43:DD:FC:AD:
                70:A4:67:8B:F3:02:21:00:CA:7F:CC:59:A9:81:6E:CF:
                6C:7E:AB:2A:FB:D9:DC:ED:A1:45:B9:6A:F6:6A:CF:EA:
                ED:F3:82:45:67:A9:8F:3A
            CT Precertificate SCTs: 
                Signed Certificate Timestamp:
                    Version   : v1 (0x0)
                    Log ID    : 1C:9F:68:2C:E9:FA:F0:45:69:50:F8:1B:96:8A:87:DD:
                                DB:32:10:D8:4C:E6:C8:B2:E3:82:52:4A:C4:CF:59:9F
                    Timestamp : Oct  4 19:33:21.276 2026 GMT
                    Extensions: none
                    Signature : ecdsa-with-SHA256
                                30:44:02:20:32:02:22:9A:F6:A8:A5:EF:2D:4B:5C:CB:
                                CA:B9:63:DE:56:FF:17:D2:FE:61:18:B5:45:6A:C7:9A:
                                A9:2C:F7:44:02:20:30:26:AE:62:D3:12:4F:E1:4A:60:
                                F2:3E:65:F3:B8:9C:FB:88:07:29:36:A3:53:EC:57:7E:
                                CB:E1:BD:51:E7:79
                Signed Certificate Timestamp:
                    Version   : v1 (0x0)
                    Log ID    : 1A:8B:9D:6B:8D:D7:91:D1:CD:05:49:ED:B6:03:55:D6:
                                06:B6:4F:AD:30:DB:71:FE:78:8F:0F:C7:C8:FB:C4:B1
                    Timestamp : Oct  4 19:33:22.579 2026 GMT
                    Extensions: 00:00:05:00:05:4A:F3:43
                    Signature : ecdsa-with-SHA256
                                30:46:02:21:00:AD:3C:18:49:C5:25:C7:C9:52:6F:B8:
                                03:7A:47:2F:63:96:1D:EB:A7:90:43:73:43:DD:FC:AD:
                                70:A4:67:8B:F3:02:21:00:CA:7F:CC:59:A9:81:6E:CF:
                                6C:7E:AB:2A:FB:D9:DC:ED:A1:45:B9:6A:F6:6A:CF:EA:
                                ED:F3:82:45:67:A9:8F:3A
