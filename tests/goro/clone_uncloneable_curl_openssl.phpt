--TEST--
curl multi/share handles and OpenSSL key/CSR/certificate objects are uncloneable
--EXTENSIONS--
curl
openssl
--FILE--
<?php
$objs = [];
$objs['cm'] = curl_multi_init(); $objs['cs'] = curl_share_init();
$k = openssl_pkey_new(['private_key_bits' => 1024, 'private_key_type' => OPENSSL_KEYTYPE_RSA]);
$objs['pkey'] = $k;
$csr = openssl_csr_new(['commonName' => 'x'], $k);
$objs['csr'] = $csr;
$objs['cert'] = openssl_csr_sign($csr, null, $k, 1);
$objs['curl'] = curl_init();
foreach ($objs as $k => $o) {
    try { clone $o; echo "$k cloned\n"; } catch (Error $e) { echo "$k: ", $e->getMessage(), "\n"; }
}
?>
--EXPECT--
cm: Trying to clone an uncloneable object of class CurlMultiHandle
cs: Trying to clone an uncloneable object of class CurlShareHandle
pkey: Trying to clone an uncloneable object of class OpenSSLAsymmetricKey
csr: Trying to clone an uncloneable object of class OpenSSLCertificateSigningRequest
cert: Trying to clone an uncloneable object of class OpenSSLCertificate
curl cloned
