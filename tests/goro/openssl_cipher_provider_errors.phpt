--TEST--
openssl_encrypt()/openssl_decrypt(): AEAD IV/tag rules, untagged decryption, key length and wrap/XTS errors as OpenSSL 3 providers raise them
--EXTENSIONS--
openssl
--FILE--
<?php
function errs() { $o = []; while ($e = openssl_error_string()) $o[] = $e; return $o ? " [" . implode(" | ", $o) . "]" : ""; }
function show($r) { return $r === false ? "false" : bin2hex($r); }
$k = "0123456789abcdef";
$iv = "123456789012";
// IV length rules and errors
foreach (["aes-128-gcm" => [0, 129], "aes-128-ccm" => [6, 14], "aes-128-ocb" => [0, 16], "chacha20-poly1305" => [8, 16]] as $c => $ls)
    foreach ($ls as $l) echo "$c iv=$l: ", show(openssl_encrypt("x", $c, str_repeat("k", 32), 0, str_repeat("i", $l), $t)), errs(), "\n";
// tag lengths when encrypting
foreach (["aes-128-gcm", "aes-128-ccm", "aes-128-ocb", "chacha20-poly1305"] as $c)
    foreach ([-1, 0, 17] as $tl) echo "$c tl=$tl: ", show(openssl_encrypt("x", $c, $k, 0, $iv, $t, "", $tl)), errs(), "\n";
try { openssl_encrypt("x", "aes-128-gcm", $k, 0, $iv, $t, "", PHP_INT_MAX); } catch (ValueError $e) { echo $e->getMessage(), "\n"; }
// decryption without / with bad tags
foreach (["aes-128-gcm", "aes-128-ccm", "aes-128-ocb", "chacha20-poly1305"] as $c) {
    $ct = openssl_encrypt("data", $c, $k, OPENSSL_RAW_DATA, $iv, $tag, "aad", 16);
    echo "$c null tag: ", show(openssl_decrypt($ct, $c, $k, OPENSSL_RAW_DATA, $iv, null, "aad")), errs(), "\n";
    $bad = $tag; $bad[0] = chr(ord($bad[0]) ^ 1);
    echo "$c bad tag: ", show(openssl_decrypt($ct, $c, $k, OPENSSL_RAW_DATA, $iv, $bad, "aad")), errs(), "\n";
    echo "$c long tag: ", show(openssl_decrypt($ct, $c, $k, OPENSSL_RAW_DATA, $iv, $tag . "x", "aad")), errs(), "\n";
    echo "$c ok: ", show(openssl_decrypt($ct, $c, $k, OPENSSL_RAW_DATA, $iv, $tag, "aad")), errs(), "\n";
}
// key length errors depend on the provider
foreach (["aes-128-cbc", "aes-128-gcm", "aes-128-ocb", "chacha20", "aes-128-xts"] as $c) {
    echo "$c long key: ", show(openssl_encrypt(str_repeat("d", 16), $c, str_repeat("k", 70), OPENSSL_RAW_DATA, str_repeat("i", 16), $t)), errs(), "\n";
    echo "$c short key: ", show(openssl_encrypt(str_repeat("d", 16), $c, "k", OPENSSL_RAW_DATA | OPENSSL_DONT_ZERO_PAD_KEY, str_repeat("i", 16), $t)), errs(), "\n";
}
// XTS and key wrap failures
echo "xts short: ", show(openssl_encrypt("short", "aes-128-xts", str_repeat("k", 16) . str_repeat("j", 16), 0, str_repeat("i", 16))), errs(), "\n";
foreach ([8, 20] as $l) echo "wrap enc $l: ", show(openssl_encrypt(str_repeat("d", $l), "aes-128-wrap", $k, 0, "")), errs(), "\n";
foreach ([8, 16, 24] as $l) echo "wrap dec $l: ", show(openssl_decrypt(str_repeat("x", $l), "aes-128-wrap", $k, OPENSSL_RAW_DATA, "")), errs(), "\n";
echo "wrap-pad dec: ", show(openssl_decrypt(str_repeat("x", 16), "aes-128-wrap-pad", $k, OPENSSL_RAW_DATA, "")), errs(), "\n";
echo "unknown: ", show(openssl_encrypt("x", "nope", $k)), errs(), "\n";
?>
--EXPECTF--
aes-128-gcm iv=0: 
Warning: openssl_encrypt(): Setting of IV length for AEAD mode failed in %s on line 8
false [error:1C80006D:Provider routines::invalid iv length]
aes-128-gcm iv=129: 
Warning: openssl_encrypt(): Setting of IV length for AEAD mode failed in %s on line 8
false [error:1C80006D:Provider routines::invalid iv length]
aes-128-ccm iv=6: 
Warning: openssl_encrypt(): Setting of IV length for AEAD mode failed in %s on line 8
false [error:1C80006D:Provider routines::invalid iv length]
aes-128-ccm iv=14: 
Warning: openssl_encrypt(): Setting of IV length for AEAD mode failed in %s on line 8
false [error:1C80006D:Provider routines::invalid iv length]
aes-128-ocb iv=0: 
Warning: openssl_encrypt(): Setting of IV length for AEAD mode failed in %s on line 8
false
aes-128-ocb iv=16: 
Warning: openssl_encrypt(): Setting of IV length for AEAD mode failed in %s on line 8
false
chacha20-poly1305 iv=8: 
Warning: openssl_encrypt(): Setting of IV length for AEAD mode failed in %s on line 8
false [error:1C80006D:Provider routines::invalid iv length]
chacha20-poly1305 iv=16: 
Warning: openssl_encrypt(): Setting of IV length for AEAD mode failed in %s on line 8
false [error:1C80006D:Provider routines::invalid iv length]
aes-128-gcm tl=-1: 
Warning: openssl_encrypt(): Retrieving verification tag failed in %s on line 11
false [error:1C80006E:Provider routines::invalid tag]
aes-128-gcm tl=0: 
Warning: openssl_encrypt(): Retrieving verification tag failed in %s on line 11
false [error:1C80006E:Provider routines::invalid tag]
aes-128-gcm tl=17: 
Warning: openssl_encrypt(): Retrieving verification tag failed in %s on line 11
false [error:1C80006E:Provider routines::invalid tag]
aes-128-ccm tl=-1: 
Warning: openssl_encrypt(): Setting tag length for AEAD cipher failed in %s on line 11
false [error:1C800076:Provider routines::invalid tag length]
aes-128-ccm tl=0: 
Warning: openssl_encrypt(): Setting tag length for AEAD cipher failed in %s on line 11
false [error:1C800076:Provider routines::invalid tag length]
aes-128-ccm tl=17: 
Warning: openssl_encrypt(): Setting tag length for AEAD cipher failed in %s on line 11
false [error:1C800076:Provider routines::invalid tag length]
aes-128-ocb tl=-1: 
Warning: openssl_encrypt(): Setting tag length for AEAD cipher failed in %s on line 11
false [error:1C800076:Provider routines::invalid tag length]
aes-128-ocb tl=0: 
Warning: openssl_encrypt(): Setting of additional application data failed in %s on line 11
false
aes-128-ocb tl=17: 
Warning: openssl_encrypt(): Setting tag length for AEAD cipher failed in %s on line 11
false [error:1C800076:Provider routines::invalid tag length]
chacha20-poly1305 tl=-1: 
Warning: openssl_encrypt(): Retrieving verification tag failed in %s on line 11
false [error:1C800076:Provider routines::invalid tag length]
chacha20-poly1305 tl=0: 
Warning: openssl_encrypt(): Retrieving verification tag failed in %s on line 11
false [error:1C800076:Provider routines::invalid tag length]
chacha20-poly1305 tl=17: 
Warning: openssl_encrypt(): Retrieving verification tag failed in %s on line 11
false [error:1C800076:Provider routines::invalid tag length]
tag_len is too long
aes-128-gcm null tag: false
aes-128-gcm bad tag: false
aes-128-gcm long tag: 
Warning: openssl_decrypt(): Setting tag for AEAD cipher decryption failed in %s on line 19
false [error:07800074:common libcrypto routines::too small buffer | error:1C800067:Provider routines::failed to get parameter]
aes-128-gcm ok: 64617461
aes-128-ccm null tag: false [error:1C800066:Provider routines::cipher operation failed]
aes-128-ccm bad tag: false [error:1C800066:Provider routines::cipher operation failed]
aes-128-ccm long tag: 
Warning: openssl_decrypt(): Setting tag for AEAD cipher decryption failed in %s on line 19
false [error:1C800076:Provider routines::invalid tag length]
aes-128-ccm ok: 64617461
aes-128-ocb null tag: 
Warning: openssl_decrypt(): Setting of additional application data failed in %s on line 16
false
aes-128-ocb bad tag: false
aes-128-ocb long tag: 
Warning: openssl_decrypt(): Setting tag length for AEAD cipher failed in %s on line 19
false [error:1C800076:Provider routines::invalid tag length]
aes-128-ocb ok: 64617461
chacha20-poly1305 null tag: 64617461
chacha20-poly1305 bad tag: false
chacha20-poly1305 long tag: 
Warning: openssl_decrypt(): Setting tag for AEAD cipher decryption failed in %s on line 19
false [error:1C800076:Provider routines::invalid tag length]
chacha20-poly1305 ok: 64617461
aes-128-cbc long key: 252b2434dba11ac7a767d883b1ff72b48c93f138fb4d172c29eb36e9e1095650 [error:03000082:digital envelope routines::invalid key length]
aes-128-cbc short key: 
Warning: openssl_encrypt(): Key length cannot be set for the cipher algorithm in %s on line 25
false [error:03000082:digital envelope routines::invalid key length]
aes-128-gcm long key: 23581f5a46a8073c589ad1f6b3ae91c2 [error:03000082:digital envelope routines::invalid key length]
aes-128-gcm short key: 
Warning: openssl_encrypt(): Key length cannot be set for the cipher algorithm in %s on line 25
false [error:03000082:digital envelope routines::invalid key length]
aes-128-ocb long key: 
Warning: openssl_encrypt(): Setting of IV length for AEAD mode failed in %s on line 24
false
aes-128-ocb short key: 
Warning: openssl_encrypt(): Setting of IV length for AEAD mode failed in %s on line 25
false
chacha20 long key: ad86cb24d31a6fc09af3f08904cb3810 [error:1C800069:Provider routines::invalid key length]
chacha20 short key: 
Warning: openssl_encrypt(): Key length cannot be set for the cipher algorithm in %s on line 25
false [error:1C800069:Provider routines::invalid key length]
aes-128-xts long key: false [error:1C800095:Provider routines::xts duplicated keys]
aes-128-xts short key: 
Warning: openssl_encrypt(): Key length cannot be set for the cipher algorithm in %s on line 25
false
xts short: false [error:1C800066:Provider routines::cipher operation failed]
wrap enc 8: 
Warning: openssl_encrypt(): Using an empty Initialization Vector (iv) is potentially insecure and not recommended in %s on line 29
false [error:1C800066:Provider routines::cipher operation failed | error:030000BD:digital envelope routines::update error]
wrap enc 20: 
Warning: openssl_encrypt(): Using an empty Initialization Vector (iv) is potentially insecure and not recommended in %s on line 29
false [error:1C8000E6:Provider routines::invalid input length | error:030000BD:digital envelope routines::update error]
wrap dec 8: false [error:1C8000E6:Provider routines::invalid input length | error:030000BD:digital envelope routines::update error]
wrap dec 16: false [error:1C800066:Provider routines::cipher operation failed | error:030000BD:digital envelope routines::update error]
wrap dec 24: false [error:1C800066:Provider routines::cipher operation failed | error:030000BD:digital envelope routines::update error]
wrap-pad dec: false [error:1C800066:Provider routines::cipher operation failed | error:030000BD:digital envelope routines::update error]
unknown: 
Warning: openssl_encrypt(): Unknown cipher algorithm in %s on line 32
false [error:0308010C:digital envelope routines::unsupported]
