--TEST--
openssl_digest()/openssl_pbkdf2(): XOF and null digests, empty digest name, iteration count errors, fetch errors on the queue
--EXTENSIONS--
openssl
--FILE--
<?php
function errs() { $o = []; while ($e = openssl_error_string()) $o[] = $e; return $o ? " [" . implode(" | ", $o) . "]" : ""; }
function t($f) { try { var_dump($f()); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; } echo errs(), "\n"; }
t(fn() => openssl_digest("abc", "nope"));
t(fn() => openssl_digest("abc", "shake128"));
t(fn() => openssl_digest("abc", "null"));
t(fn() => openssl_digest("abc", "SHA-256"));
t(fn() => openssl_cipher_iv_length("nope"));
t(fn() => openssl_cipher_key_length("nope"));
t(fn() => bin2hex(openssl_pbkdf2("pw", "salt", 8, 1, "")));
t(fn() => openssl_pbkdf2("pw", "salt", 8, 0));
t(fn() => openssl_pbkdf2("pw", "salt", 8, -1));
t(fn() => openssl_pbkdf2("pw", "salt", 8, 1, "shake256"));
t(fn() => openssl_pbkdf2("pw", "salt", 8, 1, "nope"));
t(fn() => openssl_pbkdf2("pw", "salt", PHP_INT_MAX, 1));
t(fn() => openssl_pbkdf2("pw", "salt", 8, PHP_INT_MIN));
t(fn() => openssl_pbkdf2("pw", "salt", 0, 1));
?>
--EXPECTF--

Warning: openssl_digest(): Unknown digest algorithm in %s on line 4
bool(false)
 [error:0308010C:digital envelope routines::unsupported]
bool(false)

string(0) ""

string(64) "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"


Warning: openssl_cipher_iv_length(): Unknown cipher algorithm in %s on line 8
bool(false)
 [error:0308010C:digital envelope routines::unsupported]

Warning: openssl_cipher_key_length(): Unknown cipher algorithm in %s on line 9
bool(false)
 [error:0308010C:digital envelope routines::unsupported]
string(16) "95c9338ea3a049dc"

bool(false)
 [error:1C80007B:Provider routines::invalid iteration count]
bool(false)
 [error:0780007D:common libcrypto routines::param unsigned integer negative value unsupported]
bool(false)
 [error:1C8000B7:Provider routines::xof digests not allowed]

Warning: openssl_pbkdf2(): Unknown digest algorithm in %s on line 14
bool(false)
 [error:0308010C:digital envelope routines::unsupported]
ValueError: openssl_pbkdf2(): Argument #3 ($key_length) key is too long

ValueError: openssl_pbkdf2(): Argument #4 ($iterations) iterations is too long

ValueError: openssl_pbkdf2(): Argument #3 ($key_length) must be greater than 0
