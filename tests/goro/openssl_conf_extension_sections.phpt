--TEST--
openssl config extensions: @section SANs (DNS.1 ...), policies with userNotice, generic ASN1:/DER: values and their errors; CSR text always lists Requested Extensions
--EXTENSIONS--
openssl
--FILE--
<?php
function errs() { $o = []; while ($e = openssl_error_string()) if (!str_contains($e, 'such file') && !str_contains($e, 'configuration file')) $o[] = $e; return $o ? " [" . implode(" | ", $o) . "]" : ""; }
$cnf = sys_get_temp_dir() . "/goro_confext_" . getmypid() . ".cnf";
file_put_contents($cnf, <<<'CNF'
HOME = .
oid_section = new_oids
[ new_oids ]
o1 = 1.2.3.4.1
o2 = 1.2.3.4.2
[ req ]
distinguished_name = dn
[ dn ]
[ ext_san ]
subjectAltName = @alt
certificatePolicies = ia5org, @pol, 1.2.3
o1 = ASN1:SEQUENCE:seq
o2 = ASN1:EXPLICIT:0,UTF8:tagged
[ alt ]
DNS.1 = example.com
DNS.2 = *.example.com
IP.1 = 127.0.0.1
IP.2 = ::1
email.1 = admin@example.com
URI.1 = https://example.com/
otherName.1 = 1.3.6.1.4.1.311.20.2.3;UTF8:upn@example.com
[ pol ]
policyIdentifier = 1.3.6.1.4.1.99.1
CPS.1 = "http://cps.example/"
userNotice.1 = @notice
[ notice ]
explicitText = "Notice text"
organization = "Org"
noticeNumbers = 1, 2
[ seq ]
a = INTEGER:-5
b = PRINTABLESTRING:Printable
c = BOOLEAN:TRUE
[ lower ]
subjectAltName = dns:lower.example
[ badasn1 ]
o1 = ASN1:INTEGER:abc
[ badder ]
o1 = DER:ZZ
CNF);
$key = openssl_pkey_get_private("-----BEGIN PRIVATE KEY-----
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
$csr = openssl_csr_new(['CN' => 'confext'], $key, ['config' => $cnf]);
openssl_csr_export($csr, $txt, false);
echo substr($txt, strpos($txt, 'Attributes:'), 60), "\n";
foreach (['ext_san', 'lower', 'badasn1', 'badder'] as $sect) {
  $c = @openssl_csr_sign($csr, null, $key, 1, ['config' => $cnf, 'x509_extensions' => $sect, 'digest_alg' => 'sha256'], 1);
  echo "== $sect: ", $c ? "" : "false", errs(), "\n";
  if ($c) foreach (openssl_x509_parse($c)['extensions'] as $k => $v) echo "$k: ", addcslashes($v, "\0..\37\177..\377"), "\n";
}
unlink($cnf);
?>
--EXPECT--
Attributes:
            (none)
            Requested Extensi
== ext_san: 
subjectAltName: DNS:example.com, DNS:*.example.com, IP Address:127.0.0.1, IP Address:0:0:0:0:0:0:0:1, email:admin@example.com, URI:https://example.com/, othername:UPN:upn@example.com
certificatePolicies: Policy: 1.3.6.1.4.1.99.1\n  CPS: http://cps.example/\n  User Notice:\n    Organization: Org\n    Numbers: 1, 2\n    Explicit Text: Notice text\nPolicy: 1.2.3
o1: 0\021\002\001\373\023\tPrintable\001\001\377
o2: \240\b\f\006tagged
== lower: false [error:11000075:X509 V3 routines::unsupported option | error:11000080:X509 V3 routines::error in extension]
== badasn1: false [error:11000064:X509 V3 routines::bn dec2bn error | error:068000B4:asn1 encoding routines::illegal integer | error:11000074:X509 V3 routines::extension value error]
== badder: false [error:07800066:common libcrypto routines::illegal hex digit | error:11000074:X509 V3 routines::extension value error]
