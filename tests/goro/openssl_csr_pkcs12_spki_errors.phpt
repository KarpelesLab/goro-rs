--TEST--
CSR/PKCS#12/SPKI/seal error paths: unknown names and config attributes, serial_hex parse errors, EVP_DecodeBlock SPKAC decoding, AEAD seal/open
--EXTENSIONS--
openssl
--FILE--
<?php
function errs() { $o = []; while ($e = openssl_error_string()) $o[] = $e; return $o ? " [" . implode(" | ", $o) . "]" : ""; }
function t($l, $f) { try { $r = $f(); echo "$l: "; var_export($r); } catch (Throwable $e) { echo "$l: ", get_class($e), ": ", $e->getMessage(); } echo errs(), "\n"; }
$cnfFile = __DIR__ . "/openssl_csr_pkcs12_spki_errors.cnf";
file_put_contents($cnfFile, <<<'CNF'
oid_section             = new_oids

[ new_oids ]
# Example policies to test their
tsa_policy1 = 1.2.3.4.1
tsa_policy2 = 1.2.3.4.5.6

[ req ]
default_bits		= 1024
default_md          = sha256
default_keyfile 	= privkey.pem
distinguished_name	= req_distinguished_name
attributes		= req_attributes
x509_extensions	= v3_ca	# The extensions to add to the self signed cert
string_mask = MASK:4294967295


[ req_distinguished_name ]
countryName			= Country Name (2 letter code)
countryName_default		= AU
countryName_min			= 2
countryName_max			= 2
stateOrProvinceName		= State or Province Name (full name)
stateOrProvinceName_default	= Some-State
localityName			= Locality Name (eg, city)
0.organizationName		= Organization Name (eg, company)
0.organizationName_default	= Internet Widgits Pty Ltd
organizationalUnitName		= Organizational Unit Name (eg, section)
commonName			= Common Name (eg, YOUR name)
commonName_max			= 64
emailAddress			= Email Address
emailAddress_max		= 64

[ req_attributes ]
challengePassword		= A challenge password
challengePassword_min		= 4
challengePassword_max		= 20
unstructuredName		= An optional company name

[ v3_req ]
basicConstraints = CA:FALSE
keyUsage = nonRepudiation, digitalSignature, keyEncipherment

[ v3_ca ]
subjectKeyIdentifier=hash
authorityKeyIdentifier=keyid:always,issuer:always
basicConstraints = CA:true

[ usr_cert ]
basicConstraints=CA:FALSE
CNF);
$cnf = ["config" => $cnfFile];
$k = openssl_pkey_get_private(<<<PEM
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
$crt = <<<PEM
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
PEM;
$eckey = openssl_pkey_get_private(<<<PEM
-----BEGIN PRIVATE KEY-----
MIG2AgEAMBAGByqGSM49AgEGBSuBBAAiBIGeMIGbAgEBBDA1zB0U8oOv1iBCiHxZ
l2Z5d9KeOmXy3os+MUMbZyFVMHgKvqVrERw6VPWnx+zYk1ShZANiAAQihQsgSsX+
R6mdbPJPlHPryR5JAo8ZOesY+7PWE0zs3npD3od+rCN20youI886WFVIQWj/wS1t
oe4X3C+AnEi9419LL4zcb6QQStNPeWUgCvrkpQiEfzCk17soUXYzPSQ=
-----END PRIVATE KEY-----
PEM);
errs();
t("dn unknown field", fn() => is_object(openssl_csr_new(["nope" => "x", "CN" => "y"], $k, $cnf)));
t("config attribute", fn() => openssl_csr_new(["CN" => "x"], $k, $cnf, ["challengePassword" => "pw"]));
t("unknown digest", fn() => is_object(openssl_csr_new(["CN" => "x"], $k, ["digest_alg" => "nope"] + $cnf)));
$csr = openssl_csr_new(["CN" => "sub"], $k, $cnf);
errs();
t("serial_hex XYZ", fn() => openssl_csr_sign($csr, null, $k, 1, $cnf, 0, "XYZ"));
t("serial_hex odd", fn() => openssl_csr_sign($csr, null, $k, 1, $cnf, 0, "ABC"));
t("serial_hex ok", fn() => openssl_x509_parse(openssl_csr_sign($csr, null, $k, 1, $cnf, 0, "00FF"))['serialNumberHex']);
t("p12 garbage", fn() => openssl_pkcs12_read("garbage", $c, "pw"));
t("p12 key mismatch", fn() => openssl_pkcs12_export($crt, $out, $eckey, "pw"));
$spkac = openssl_spki_new($k, "challenge", OPENSSL_ALGO_SHA256);
t("spki with SPKAC= prefix", fn() => openssl_spki_verify($spkac));
t("spki without prefix", fn() => openssl_spki_verify(substr($spkac, 6)));
t("spki challenge", fn() => openssl_spki_export_challenge(substr($spkac, 6)));
$pub = openssl_pkey_get_public($crt);
foreach (["aes-256-gcm", "chacha20-poly1305", "nope"] as $m) {
    t("seal/open $m", function() use ($pub, $k, $m) {
        $r = openssl_seal("secret data", $sealed, $ek, [$pub], $m, $iv);
        if ($r === false) return $r;
        return [$r, strlen($iv), openssl_open($sealed, $out, $ek[0], $k, $m, $iv), $out];
    });
}
@unlink($cnfFile);
?>
--EXPECTF--

Warning: openssl_csr_new(): dn: nope is not a recognized name in %s on line 112
dn unknown field: true [error:04000067:object identifier routines::unknown object name]

Warning: openssl_csr_new(): add1_attr_by_txt challengePassword_min -> 4 (failed; check error queue and value of string_mask OpenSSL option if illegal characters are reported) in %s on line 113
config attribute: false [error:04000067:object identifier routines::unknown object name | error:04000067:object identifier routines::unknown object name | error:05800077:x509 certificate routines::invalid field name]
unknown digest: true [error:0308010C:digital envelope routines::unsupported]

Warning: openssl_csr_sign(): Error parsing serial number in %s on line 117
serial_hex XYZ: false [error:06800096:asn1 encoding routines::short line]

Warning: openssl_csr_sign(): Error parsing serial number in %s on line 118
serial_hex odd: false [error:06800091:asn1 encoding routines::odd number of chars]
serial_hex ok: 'FF'
p12 garbage: false [error:0680008E:asn1 encoding routines::not enough data]

Warning: openssl_pkcs12_export(): Private key does not correspond to cert in %s on line 121
p12 key mismatch: false [error:03000065:digital envelope routines::different key types | error:05800073:x509 certificate routines::key type mismatch]

Warning: openssl_spki_verify(): Unable to decode supplied SPKAC in %s on line 123
spki with SPKAC= prefix: false [error:05800076:x509 certificate routines::base64 decode error]
spki without prefix: true
spki challenge: 'challenge'
seal/open aes-256-gcm: array (
  0 => 11,
  1 => 12,
  2 => false,
  3 => NULL,
)
seal/open chacha20-poly1305: array (
  0 => 11,
  1 => 12,
  2 => true,
  3 => 'secret data',
)

Warning: openssl_seal(): Unknown cipher algorithm in %s on line 129
seal/open nope: false [error:0308010C:digital envelope routines::unsupported]
