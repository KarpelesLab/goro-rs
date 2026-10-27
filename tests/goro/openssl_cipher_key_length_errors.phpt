--TEST--
openssl_encrypt() with DONT_ZERO_PAD_KEY and a short key: per-cipher key-length errors
--EXTENSIONS--
openssl
--FILE--
<?php
// EVP_CIPHER_CTX_set_key_length() refusals: provider error, EVP error or silent
foreach (['aes-128-cbc-hmac-sha1', 'aes-256-gcm-siv', 'aes-128-siv', 'sm4-xts', 'aes-128-xts', 'aes-128-cbc', 'chacha20'] as $c) {
  $r = @openssl_encrypt("data", $c, "short", OPENSSL_DONT_ZERO_PAD_KEY, str_repeat("i", openssl_cipher_iv_length($c)));
  $e = []; while ($x = openssl_error_string()) $e[] = $x;
  echo "$c: ", var_export($r, true), " [", implode(" | ", $e), "]\n";
}
?>
--EXPECT--
aes-128-cbc-hmac-sha1: false [error:1C800069:Provider routines::invalid key length]
aes-256-gcm-siv: false [error:1C800069:Provider routines::invalid key length]
aes-128-siv: false []
sm4-xts: false [error:03000082:digital envelope routines::invalid key length]
aes-128-xts: false []
aes-128-cbc: false [error:03000082:digital envelope routines::invalid key length]
chacha20: false [error:1C800069:Provider routines::invalid key length]
