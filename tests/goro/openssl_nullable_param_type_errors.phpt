--TEST--
TypeErrors for nullable openssl parameters name ?string / ?array
--EXTENSIONS--
openssl
--FILE--
<?php
$calls = [
    fn() => openssl_cms_sign("in", "out", "c", "k", new stdClass),
    fn() => openssl_cms_sign("in", "out", "c", "k", [], 0, 1, []),
    fn() => openssl_cms_verify("in", 0, []),
    fn() => openssl_cms_verify("in", 0, null, [], [], new stdClass),
    fn() => openssl_csr_sign("csr", null, "k", 1, "opts"),
    fn() => openssl_csr_sign("csr", null, "k", 1, null, 0, []),
    fn() => openssl_pkcs7_sign("in", "out", "c", "k", "hdr"),
    fn() => openssl_pkcs7_sign("in", "out", "c", "k", [], 0, new stdClass),
    fn() => openssl_x509_checkpurpose("c", 1, [], []),
    fn() => openssl_decrypt("d", "aes-128-gcm", "k", 0, "", []),
];
foreach ($calls as $c) {
    try { $c(); echo "no error\n"; } catch (TypeError $e) { echo $e->getMessage(), "\n"; }
}
?>
--EXPECT--
openssl_cms_sign(): Argument #5 ($headers) must be of type ?array, stdClass given
openssl_cms_sign(): Argument #8 ($untrusted_certificates_filename) must be of type ?string, array given
openssl_cms_verify(): Argument #3 ($certificates) must be of type ?string, array given
openssl_cms_verify(): Argument #5 ($untrusted_certificates_filename) must be of type ?string, array given
openssl_csr_sign(): Argument #5 ($options) must be of type ?array, string given
openssl_csr_sign(): Argument #7 ($serial_hex) must be of type ?string, array given
openssl_pkcs7_sign(): Argument #5 ($headers) must be of type ?array, string given
openssl_pkcs7_sign(): Argument #7 ($untrusted_certificates_filename) must be of type ?string, stdClass given
openssl_x509_checkpurpose(): Argument #4 ($untrusted_certificates_file) must be of type ?string, array given
openssl_decrypt(): Argument #6 ($tag) must be of type ?string, array given
