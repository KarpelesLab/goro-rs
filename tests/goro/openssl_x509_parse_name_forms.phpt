--TEST--
openssl_x509_parse(): multi-valued RDNs in name, otherName (UPN) SANs and name constraints text
--EXTENSIONS--
openssl
--FILE--
<?php
// multi-valued RDN, otherName UPN, dirName/RID/IPv6 SAN, name constraints
$c = openssl_x509_read("-----BEGIN CERTIFICATE-----
MIIGDTCCBXagAwIBAgIKAP8A/wD/AP8A/zANBgkqhkiG9w0BAQ0FADCBgDELMAkG
A1UEBhMCRlIxDjAMBgNVBAgMBVBhcmlzMQ4wDAYDVQQHDAVQYXJpczERMA8GA1UE
CgwIVGVzdCBPcmcxDTALBgNVBAsMBFVuaXQxEDAOBgNVBAMMB1Rlc3QgQ0ExHTAb
BgkqhkiG9w0BCQEWDmNhQGV4YW1wbGUuY29tMB4XDTI2MTAyNzE2MjEyM1oXDTI2
MTEyNjE2MjEyM1owggGCMQswCQYDVQQGEwJVUzELMAkGA1UECAwCQ0ExCzAJBgNV
BAcMAlNGMQ0wCwYDVQQKDARPcmcxMQ0wCwYDVQQKDARPcmcyMRswCwYDVQQLDARV
bml0MAwGA1UEAwwFbXVsdGkxFDASBgNVBAMMC2JpZy5leGFtcGxlMRQwEgYJKoZI
hvcNAQkBFgVlQHgueTEMMAoGA1UEBRMDU04xMQswCQYDVQQMDAJEcjEKMAgGA1UE
KgwBRzEKMAgGA1UEBAwBUzEKMAgGA1UEKwwBSTEKMAgGA1UEQQwBUDEMMAoGA1UE
LAwDSUlJMQwwCgYDVQQuEwNkbnExFDASBgoJkiaJk/IsZAEBDAR1aWQxMRcwFQYK
CZImiZPyLGQBGRYHZXhhbXBsZTETMBEGCgmSJomT8ixkARkWA2NvbTEQMA4GA1UE
CQwHTWFpbiBTdDEOMAwGA1UEEQwFMTIzNDUxEDAOBgNVBA8MB1ByaXZhdGUxEzAR
BgsrBgEEAYI3PAIBAxMCVVMwgZ8wDQYJKoZIhvcNAQEBBQADgY0AMIGJAoGBAKiE
w2Wes3ZCqhTefhFnxn84MCqLcRGXe1mHg8YGO/u6ZjYz6nyhOkimbU5Ua6YvnPho
VENnWIfGcJaaOtjL3IOIyi4Uc4AwuoVFLkVGPl5bqXk9ljjzIiAiiQUnLC1DZy+/
qKTE1pc2U+LP1tw4lqtbmU110gGNwdaZZpvhqHYHAgMBAAGjggKHMIICgzCBjgYD
VR0RBIGGMIGDgglhLmV4YW1wbGWgGAYKKwYBBAGCNxQCA6AKDAh1c2VyQHVwbqQ0
MDIxCzAJBgNVBAYTAlVTMRAwDgYDVQQKDAdEaXIgT3JnMREwDwYDVQQDDAhEaXIg
TmFtZYgDKgMEhxAgAQ24AAAAAAAAAAAAAAABgQV4QHkueoYIaHR0cDovL3UwLAYD
VR0eAQH/BCIwIKAQMA6CDC5leGFtcGxlLmNvbaEMMAqHCMCoAAD//wAAMAwGA1Ud
JAQFMAOAAQMwCgYDVR02BAMCAQIwEgYDVR0TAQH/BAgwBgEB/wIBAjAMBgNVHQ8E
BQMDB/+AMEsGA1UdJQREMEIGCCsGAQUFBwMBBggrBgEFBQcDAgYIKwYBBQUHAwMG
CCsGAQUFBwMEBggrBgEFBQcDCAYIKwYBBQUHAwkGBCoDBAUwEQYJYIZIAYb4QgEB
BAQDAgD3MBUGA1UdEgQOMAyBCmlzc3VlckB4LnkwHQYDVR0OBBYEFCNceaLRwgdN
rUJ2FtiEuNlxZ135MIG0BgNVHSMEgawwgamAFCNceaLRwgdNrUJ2FtiEuNlxZ135
oYGGpIGDMIGAMQswCQYDVQQGEwJGUjEOMAwGA1UECAwFUGFyaXMxDjAMBgNVBAcM
BVBhcmlzMREwDwYDVQQKDAhUZXN0IE9yZzENMAsGA1UECwwEVW5pdDEQMA4GA1UE
AwwHVGVzdCBDQTEdMBsGCSqGSIb3DQEJARYOY2FAZXhhbXBsZS5jb22CCBI0VniQ
q83vMBEGCCsGAQUFBwEYBAUwAwIBBTAVBgUqAwQFBgQMDApjdXN0b20gZXh0MA8G
BSoDBAUHAQH/BAMBAgMwDQYJKoZIhvcNAQENBQADgYEAMwxBDwK62RtwLIrIADI3
qi33dQ/ecWOpMMStzGGyoxO4HbtneZ1VIKwsjBOWHMltEsSe9ZStFx7+0bE749Xq
zJ+69hSWN1fAi4XMqkHzZkShDwYa7WMzFMxgc5vswUNRn7h4IR0iBPbUY2bmHIT0
btBokOCQh9OFRduwj96H6F4=
-----END CERTIFICATE-----");
$p = openssl_x509_parse($c);
echo $p['name'], "\n";
var_dump($p['subject']['OU'], $p['subject']['CN']);
foreach (['subjectAltName', 'nameConstraints', 'policyConstraints', 'inhibitAnyPolicy', 'basicConstraints', 'keyUsage', 'extendedKeyUsage', 'nsCertType', 'issuerAltName', 'tlsfeature', '1.2.3.4.5.6', '1.2.3.4.5.7'] as $e) {
  echo "$e: ", var_export($p['extensions'][$e] ?? null, true), "\n";
}
echo openssl_x509_parse($c, false)['name'], "\n";
var_dump(openssl_csr_get_subject("-----BEGIN CERTIFICATE REQUEST-----
MIICxDCCAi0CAQAwggGCMQswCQYDVQQGEwJVUzELMAkGA1UECAwCQ0ExCzAJBgNV
BAcMAlNGMQ0wCwYDVQQKDARPcmcxMQ0wCwYDVQQKDARPcmcyMRswCwYDVQQLDARV
bml0MAwGA1UEAwwFbXVsdGkxFDASBgNVBAMMC2JpZy5leGFtcGxlMRQwEgYJKoZI
hvcNAQkBFgVlQHgueTEMMAoGA1UEBRMDU04xMQswCQYDVQQMDAJEcjEKMAgGA1UE
KgwBRzEKMAgGA1UEBAwBUzEKMAgGA1UEKwwBSTEKMAgGA1UEQQwBUDEMMAoGA1UE
LAwDSUlJMQwwCgYDVQQuEwNkbnExFDASBgoJkiaJk/IsZAEBDAR1aWQxMRcwFQYK
CZImiZPyLGQBGRYHZXhhbXBsZTETMBEGCgmSJomT8ixkARkWA2NvbTEQMA4GA1UE
CQwHTWFpbiBTdDEOMAwGA1UEEQwFMTIzNDUxEDAOBgNVBA8MB1ByaXZhdGUxEzAR
BgsrBgEEAYI3PAIBAxMCVVMwgZ8wDQYJKoZIhvcNAQEBBQADgY0AMIGJAoGBAKiE
w2Wes3ZCqhTefhFnxn84MCqLcRGXe1mHg8YGO/u6ZjYz6nyhOkimbU5Ua6YvnPho
VENnWIfGcJaaOtjL3IOIyi4Uc4AwuoVFLkVGPl5bqXk9ljjzIiAiiQUnLC1DZy+/
qKTE1pc2U+LP1tw4lqtbmU110gGNwdaZZpvhqHYHAgMBAAGgADANBgkqhkiG9w0B
AQsFAAOBgQAEGtoYHJK1bNgLNttCphLY2+Xkjju4qLmp44L6lBSvkVU7VKUawGc1
30DXC1VU8ogOR7L5vBgfEBgRFhyRRJPe8ZEC4dnF1Ur7PUUlz9pxww+uReOyS+TX
iYiBI8Oo0JNYWsY4ZtOYegIYZcUK7+lnr26hD1++t2KORniMwk7G1A==
-----END CERTIFICATE REQUEST-----")['OU']);
?>
--EXPECT--
/C=US/ST=CA/L=SF/O=Org1/O=Org2/OU=Unit+CN=multi/CN=big.example/emailAddress=e@x.y/serialNumber=SN1/title=Dr/GN=G/SN=S/initials=I/pseudonym=P/generationQualifier=III/dnQualifier=dnq/UID=uid1/DC=example/DC=com/street=Main St/postalCode=12345/businessCategory=Private/jurisdictionC=US
string(4) "Unit"
array(2) {
  [0]=>
  string(5) "multi"
  [1]=>
  string(11) "big.example"
}
subjectAltName: 'DNS:a.example, othername:UPN:user@upn, DirName:C = US, O = Dir Org, CN = Dir Name, Registered ID:1.2.3.4, IP Address:2001:DB8:0:0:0:0:0:1, email:x@y.z, URI:http://u'
nameConstraints: 'Permitted:
  DNS:.example.com
Excluded:
  IP:192.168.0.0/255.255.0.0'
policyConstraints: 'Require Explicit Policy:3'
inhibitAnyPolicy: '2'
basicConstraints: 'CA:TRUE, pathlen:2'
keyUsage: 'Digital Signature, Non Repudiation, Key Encipherment, Data Encipherment, Key Agreement, Certificate Sign, CRL Sign, Encipher Only, Decipher Only'
extendedKeyUsage: 'TLS Web Server Authentication, TLS Web Client Authentication, Code Signing, E-mail Protection, Time Stamping, OCSP Signing, 1.2.3.4.5'
nsCertType: 'SSL Client, SSL Server, S/MIME, Object Signing, SSL CA, S/MIME CA, Object Signing CA'
issuerAltName: 'email:issuer@x.y'
tlsfeature: 'status_request'
1.2.3.4.5.6: '
custom ext'
1.2.3.4.5.7: ''
/C=US/ST=CA/L=SF/O=Org1/O=Org2/OU=Unit+CN=multi/CN=big.example/emailAddress=e@x.y/serialNumber=SN1/title=Dr/GN=G/SN=S/initials=I/pseudonym=P/generationQualifier=III/dnQualifier=dnq/UID=uid1/DC=example/DC=com/street=Main St/postalCode=12345/businessCategory=Private/jurisdictionC=US
string(4) "Unit"
