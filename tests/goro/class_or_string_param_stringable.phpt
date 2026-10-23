--TEST--
A Class|string parameter of an internal function takes a Stringable object as a string in weak mode
--EXTENSIONS--
openssl
--FILE--
<?php
class S { function __toString(): string { return "not a cert"; } }
var_dump(@openssl_x509_parse(new S));
try {
    openssl_x509_parse(new stdClass);
} catch (TypeError $e) {
    echo $e->getMessage(), "\n";
}
$f = __DIR__ . '/class_or_string_param_stringable.inc';
file_put_contents($f, '<?php declare(strict_types=1);
try { openssl_x509_parse(new S); } catch (TypeError $e) { echo $e->getMessage(), "\n"; }');
include $f;
?>
--CLEAN--
<?php @unlink(__DIR__ . '/class_or_string_param_stringable.inc'); ?>
--EXPECT--
bool(false)
openssl_x509_parse(): Argument #1 ($certificate) must be of type OpenSSLCertificate|string, stdClass given
openssl_x509_parse(): Argument #1 ($certificate) must be of type OpenSSLCertificate|string, S given
