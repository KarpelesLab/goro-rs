--TEST--
openssl aes-*-cbc-cts / camellia-*-cbc-cts (CBC with ciphertext stealing, CS1)
--EXTENSIONS--
openssl
--FILE--
<?php
var_dump(openssl_cipher_iv_length('aes-128-cbc-cts'), openssl_cipher_key_length('aes-192-cbc-cts'));
var_dump(count(array_filter(openssl_get_cipher_methods(), fn($m) => str_ends_with($m, '-cbc-cts'))));
$iv = str_repeat("i", 16);
foreach (['aes-128-cbc-cts' => 16, 'aes-256-cbc-cts' => 32, 'camellia-192-cbc-cts' => 24] as $c => $kl) {
    $k = str_repeat("k", $kl);
    $all = '';
    $ok = true;
    for ($n = 16; $n <= 64; $n++) {
        $p = substr(str_repeat("The quick brown fox jumps over the lazy dog. ", 2), 0, $n);
        $e = openssl_encrypt($p, $c, $k, OPENSSL_RAW_DATA, $iv);
        $ok = $ok && strlen($e) === $n && openssl_decrypt($e, $c, $k, OPENSSL_RAW_DATA, $iv) === $p;
        $all .= $e;
    }
    echo $c, ' ', var_export($ok, true), ' ', md5($all), "\n";
}
$k = str_repeat("k", 16);
echo bin2hex(openssl_encrypt(str_repeat("A", 17), 'aes-128-cbc-cts', $k, OPENSSL_RAW_DATA, $iv)), "\n";
var_dump(openssl_encrypt("abc", 'aes-128-cbc-cts', $k, OPENSSL_RAW_DATA, $iv));
var_dump(openssl_encrypt("", 'aes-128-cbc-cts', $k, OPENSSL_RAW_DATA, $iv));
var_dump(openssl_decrypt("abc", 'aes-128-cbc-cts', $k, OPENSSL_RAW_DATA, $iv));
var_dump(openssl_encrypt(str_repeat("A", 20), 'aes-128-cbc-cts', $k, 0, $iv));
?>
--EXPECT--
int(16)
int(24)
int(6)
aes-128-cbc-cts true a501b240bb086e395be300ee648855eb
aes-256-cbc-cts true 5ae84d4176eb0df098be9f3933ada267
camellia-192-cbc-cts true 3c5615dd88fcc5847250894cb6995f76
1f71f91e0494775b4297a1d5412b6e1ac6
bool(false)
bool(false)
bool(false)
string(28) "Hz5rRF6A0deFDtNC2D7EAPfczdY="
