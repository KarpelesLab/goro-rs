--TEST--
openssl_x509_parse(): invalid or duplicated extensions report invalid certificate; bad BIT STRING padding
--EXTENSIONS--
openssl
--FILE--
<?php
// an undecodable keyUsage and a repeated extension make the certificate
// invalid for the purpose checks; the key usage cannot be printed
foreach (['badext' => "-----BEGIN CERTIFICATE-----
MIIBtTCCAR6gAwIBAgIBDTANBgkqhkiG9w0BAQsFADAZMRcwFQYDVQQDDA5pc3N1
ZXIgY3JhZnRlZDAeFw0yMDAxMDEwMDAwMDBaFw0zMDAxMDEwMDAwMDBaMBIxEDAO
BgNVBAMMB2NyYWZ0ZWQwgZ8wDQYJKoZIhvcNAQEBBQADgY0AMIGJAoGBAKiEw2We
s3ZCqhTefhFnxn84MCqLcRGXe1mHg8YGO/u6ZjYz6nyhOkimbU5Ua6YvnPhoVENn
WIfGcJaaOtjL3IOIyi4Uc4AwuoVFLkVGPl5bqXk9ljjzIiAiiQUnLC1DZy+/qKTE
1pc2U+LP1tw4lqtbmU110gGNwdaZZpvhqHYHAgMBAAGjFDASMBAGA1UdDwQJAwVn
YXJiYWdlMA0GCSqGSIb3DQEBCwUAA4GBAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA
AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA
AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA
AAAAAAAAAAAA
-----END CERTIFICATE-----", 'dupext' => "-----BEGIN CERTIFICATE-----
MIIBvDCCASWgAwIBAgIBDDANBgkqhkiG9w0BAQsFADAZMRcwFQYDVQQDDA5pc3N1
ZXIgY3JhZnRlZDAeFw0yMDAxMDEwMDAwMDBaFw0zMDAxMDEwMDAwMDBaMBIxEDAO
BgNVBAMMB2NyYWZ0ZWQwgZ8wDQYJKoZIhvcNAQEBBQADgY0AMIGJAoGBAKiEw2We
s3ZCqhTefhFnxn84MCqLcRGXe1mHg8YGO/u6ZjYz6nyhOkimbU5Ua6YvnPhoVENn
WIfGcJaaOtjL3IOIyi4Uc4AwuoVFLkVGPl5bqXk9ljjzIiAiiQUnLC1DZy+/qKTE
1pc2U+LP1tw4lqtbmU110gGNwdaZZpvhqHYHAgMBAAGjGzAZMAkGA1UdEwQCMAAw
DAYDVR0TBAUwAwEB/zANBgkqhkiG9w0BAQsFAAOBgQAAAAAAAAAAAAAAAAAAAAAA
AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA
AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA
AAAAAAAAAAAAAAAAAAAAAA==
-----END CERTIFICATE-----"] as $n => $pem) {
  $c = openssl_x509_read($pem);
  $p = openssl_x509_parse($c);
  echo "$n: ", json_encode($p['extensions']), " ", json_encode($p['purposes'][2]), "\n";
  $e = []; while ($x = openssl_error_string()) $e[] = $x;
  echo "  [", implode(" | ", $e), "]\n";
}
?>
--EXPECT--
badext: {"keyUsage":"\u0003\u0005garbage"} [true,true,"sslserver"]
  [error:1100009E:X509 V3 routines::invalid certificate | error:068000DC:asn1 encoding routines::invalid bit string bits left]
dupext: {"basicConstraints":"CA:TRUE"} [true,true,"sslserver"]
  [error:1100009E:X509 V3 routines::invalid certificate]
