--TEST--
openssl config extensions: OpenSSL 3.5 error queues for invalid values; inhibitAnyPolicy, policyConstraints, nameConstraints
--EXTENSIONS--
openssl
--FILE--
<?php
// error queues of failing configuration extensions (OpenSSL 3.5 reason
// codes), and inhibitAnyPolicy / policyConstraints / nameConstraints
$cnf = sys_get_temp_dir() . "/goro_exterr_" . getmypid() . ".cnf";
file_put_contents($cnf, file_get_contents(__FILE__, false, null, __COMPILER_HALT_OFFSET__));
$k = openssl_pkey_get_private("-----BEGIN PRIVATE KEY-----
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
-----END PRIVATE KEY-----");
$ca = "-----BEGIN CERTIFICATE-----
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
$csr = openssl_csr_new(['CN' => 'x'], $k, ['config' => $cnf]);
while (openssl_error_string());
for ($i = 1; $i <= 26; $i++) {
  $c = @openssl_csr_sign($csr, ($i == 10 ? null : $ca), $k, 1, ['config' => $cnf, 'x509_extensions' => "e$i"]);
  $o = []; while ($e = openssl_error_string()) if (!str_contains($e, 'such file') && !str_contains($e, 'configuration file')) $o[] = $e;
  echo "e$i: ", $c ? "ok" : "false", " ", implode(" | ", $o), "\n";
  if ($c) echo "   ", json_encode(openssl_x509_parse($c)['extensions']), "\n";
}
unlink($cnf);
__halt_compiler();HOME = .
[ req ]
distinguished_name = dn
[ dn ]
[ e1 ]
basicConstraints = CA:TRUE, pathlen:abc
[ e2 ]
keyUsage = digitalSignature, notABit
[ e3 ]
authorityKeyIdentifier = keyid, bogus
[ e4 ]
1.2.3.x = DER:01
[ e5 ]
subjectAltName = otherName:nonsense
[ e6 ]
basicConstraints = CA:maybe
[ e7 ]
extendedKeyUsage = serverAuth, not.an.oid
[ e8 ]
nsCertType = client, notatype
[ e9 ]
subjectKeyIdentifier = zz
[ e10 ]
authorityKeyIdentifier = keyid:always
[ e11 ]
inhibitAnyPolicy = abc
[ e12 ]
policyConstraints = requireExplicitPolicy:abc
[ e13 ]
crlDistributionPoints = bogus:x
[ e14 ]
authorityInfoAccess = nonsense
[ e15 ]
nameConstraints = permitted;BOGUS:x
[ e16 ]
noSuchExtension = foo
[ e17 ]
basicConstraints = bogus:1
[ e18 ]
subjectAltName = IP:300.1.1.1
[ e19 ]
subjectAltName = RID:not.an.oid
[ e20 ]
subjectAltName = dirName:nosuchsection
[ e21 ]
inhibitAnyPolicy = 3
policyConstraints = requireExplicitPolicy:1, inhibitPolicyMapping:0x2
nameConstraints = critical, permitted;DNS:.example.com, permitted;IP:10.0.0.0/255.0.0.0, excluded;IP:2001:db8::/ffff:ffff::, excluded;email:bad.example, permitted;dirName:ncdir
[ ncdir ]
C = US
O = NC Org
[ e22 ]
policyConstraints = bogusName:1
[ e23 ]
policyConstraints = @emptysect
[ emptysect ]
[ e24 ]
nameConstraints = permitted;IP:10.0.0.0
[ e25 ]
subjectAltName = otherName:1.2.3.4;INTEGER:42, otherName:1.3.6.1.4.1.311.20.2.3;UTF8:u@x
[ e26 ]
inhibitAnyPolicy = -1
--EXPECT--
e1: false error:11000064:X509 V3 routines::bn dec2bn error | error:11000080:X509 V3 routines::error in extension
e2: false error:1100006F:X509 V3 routines::unknown bit string argument | error:11000080:X509 V3 routines::error in extension
e3: false error:110000AC:X509 V3 routines::unknown value | error:11000080:X509 V3 routines::error in extension
e4: false error:06800082:asn1 encoding routines::invalid digit | error:11000073:X509 V3 routines::extension name error
e5: false error:11000093:X509 V3 routines::othername error | error:11000080:X509 V3 routines::error in extension
e6: false error:11000068:X509 V3 routines::invalid boolean string | error:11000080:X509 V3 routines::error in extension
e7: false error:04000067:object identifier routines::unknown object name | error:1100006E:X509 V3 routines::invalid object identifier | error:11000080:X509 V3 routines::error in extension
e8: false error:1100006F:X509 V3 routines::unknown bit string argument | error:11000080:X509 V3 routines::error in extension
e9: false error:07800066:common libcrypto routines::illegal hex digit | error:11000080:X509 V3 routines::error in extension
e10: false error:1100007B:X509 V3 routines::unable to get issuer keyid | error:11000080:X509 V3 routines::error in extension
e11: false error:11000064:X509 V3 routines::bn dec2bn error | error:11000080:X509 V3 routines::error in extension
e12: false error:11000064:X509 V3 routines::bn dec2bn error | error:11000080:X509 V3 routines::error in extension
e13: false error:11000075:X509 V3 routines::unsupported option | error:11000080:X509 V3 routines::error in extension
e14: false error:1100008F:X509 V3 routines::invalid syntax | error:11000080:X509 V3 routines::error in extension
e15: false error:11000075:X509 V3 routines::unsupported option | error:11080022:X509 V3 routines::X509V3 lib | error:11000080:X509 V3 routines::error in extension
e16: false error:11000082:X509 V3 routines::unknown extension name | error:11000080:X509 V3 routines::error in extension
e17: false error:1100006A:X509 V3 routines::invalid name | error:11000080:X509 V3 routines::error in extension
e18: false error:11000076:X509 V3 routines::bad ip address | error:11000080:X509 V3 routines::error in extension
e19: false error:04000067:object identifier routines::unknown object name | error:11000077:X509 V3 routines::bad object | error:11000080:X509 V3 routines::error in extension
e20: false error:11000096:X509 V3 routines::section not found | error:11000095:X509 V3 routines::dirname error | error:11000080:X509 V3 routines::error in extension
e21: ok 
   {"inhibitAnyPolicy":"3","policyConstraints":"Require Explicit Policy:1, Inhibit Policy Mapping:2","nameConstraints":"Permitted:\n  DNS:.example.com\n  IP:10.0.0.0\/255.0.0.0\n  DirName:C = US, O = NC Org\nExcluded:\n  IP:2001:DB8:0:0:0:0:0:0\/FFFF:FFFF:0:0:0:0:0:0\n  email:bad.example"}
e22: false error:1100006A:X509 V3 routines::invalid name | error:11000080:X509 V3 routines::error in extension
e23: false error:11000069:X509 V3 routines::invalid extension string | error:11000080:X509 V3 routines::error in extension
e24: false error:11000076:X509 V3 routines::bad ip address | error:11080022:X509 V3 routines::X509V3 lib | error:11000080:X509 V3 routines::error in extension
e25: ok 
   {"subjectAltName":"othername:<unsupported>, othername:UPN:u@x"}
e26: ok 
   {"inhibitAnyPolicy":"-1"}
