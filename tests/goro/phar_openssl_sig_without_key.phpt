--TEST--
Phar::setSignatureAlgorithm(Phar::OPENSSL) without a usable private key fails quietly
--EXTENSIONS--
phar
openssl
--INI--
phar.readonly=0
--FILE--
<?php
// php-src reads the key with libssl's PEM_read_bio_PrivateKey() itself
// (PHAR_HAVE_OPENSSL): no openssl_sign() warning; without a key the BIO
// fails ("unable to write to phar"), an unreadable key "unable to process"
$f = __DIR__ . "/phar_openssl_sig_without_key.phar";
@unlink($f);
$p = new Phar($f);
$p["a.txt"] = "a";
foreach ([null, "not a key", "file://" . __FILE__, ""] as $key) {
    try {
        $key === null ? $p->setSignatureAlgorithm(Phar::OPENSSL) : $p->setSignatureAlgorithm(Phar::OPENSSL_SHA256, $key);
        echo "signed\n";
    } catch (PharException $e) {
        echo str_replace($f, "F", $e->getMessage()), "\n";
    }
}
$p->setSignatureAlgorithm(Phar::SHA1);
var_dump($p->getSignature()["hash_type"]);
?>
--CLEAN--
<?php @unlink(__DIR__ . "/phar_openssl_sig_without_key.phar"); ?>
--EXPECT--
phar error: unable to write signature: unable to write to phar "F" with requested openssl signature
phar error: unable to write signature: unable to process private key
phar error: unable to write signature: unable to process private key
phar error: unable to write signature: unable to process private key
string(5) "SHA-1"
