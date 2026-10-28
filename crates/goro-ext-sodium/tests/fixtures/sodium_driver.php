<?php
// Generates sodium_php85.txt: run once on the reference PHP 8.5 binary
// (php -d error_reporting=-1 sodium_driver.php > sodium_php85.txt).
//
// Line format (tab separated):
//   function  args  =>  result
// args: space separated tokens, `s<hex>` (string), `i<int>`, `n` (null),
// `a` (array: a non-string by-reference value).
// result: `S<hex>`, `B1`/`B0`, `I<int>`, `N`, `V` (void), `A<r>,<r>,...`
// (list of results), `E<class>:<message>`, preceded by `W<message>|` for
// each warning raised during the call; functions taking a
// reference append ` R<token>` with the referenced value afterwards.
// Lines starting with `#len` give only the length of a random result:
//   #len  function  args  =>  length

mt_srand(20261028);
require __DIR__ . '/sodium_driver_lib.php';
$MSGLENS = [0, 1, 15, 16, 17, 63, 64, 65, 127, 128, 129, 255, 256, 300, 1000];
$INTS = [PHP_INT_MIN, -1, 0, 1, 15, 16, 17, 31, 32, 33, 63, 64, 65, 255, 256, 4294967295, 4294967296, PHP_INT_MAX];

// ---- utilities ----
foreach ([0, 1, 2, 5, 16, 33] as $l) c('sodium_bin2hex', rb($l));
$hex = ['', '0', '00', 'ab', 'AB', 'aB', 'abc', 'abcd', 'zz', 'a z', '0g', 'g0', ':ab', 'ab:', 'a:b', 'a::b', ' ab cd ', 'ab cd', 'ab  cd', '00ff', 'FFff', "ab\0cd", 'abcdx', 'xabcd'];
foreach ($hex as $h) foreach (['', ':', ' ', ': ', 'x', 'a', "\0"] as $ig) c('sodium_hex2bin', $h, $ig);
foreach ([0, 1, 2, 3, 4, 5, 6, 30, 31, 32] as $l) {
    $b = rb($l);
    foreach ([-1, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9] as $id) c('sodium_bin2base64', $b, $id);
}
$b64 = ['', 'A', 'AA', 'AAA', 'AAAA', 'AA==', 'AA=', 'AAA=', 'AB==', 'AAB=', 'YWJjZA', 'YWJjZA==', 'YWJjZA=', 'YWJjZA===', 'YWJj', 'YW Jj', ' YWJj', 'YWJj ', 'YWJjZA= =', 'YW=Jj', '+/+/', '-_-_', '+/-_', 'O1R', 'O1', 'O', 'O1Q=', 'O1R=', 'O1Q', '////', '____', '/w==', '_w==', '/w', '_w', 'AA==AA==', "YWJj\n", "YW\nJj", '=', '==', 'Zm9vYmFy', 'Zm9vYmE', 'Zm9vYg', 'Zm9vYmE=', 'Zm9vYg==', 'Zm9vYh==', 'Zm9vYmF='];
foreach ($b64 as $s) foreach ([1, 3, 5, 7, 0, 2, 9] as $id) foreach (['', ' ', "\n", '=', ' =', 'A'] as $ig) {
    if ($ig !== '' && $id != 1 && $id != 3) continue;
    c('sodium_base642bin', $s, $id, $ig);
}
$cmp = [['', ''], ['a', 'a'], ['a', 'b'], ['b', 'a'], ['ab', 'ba'], ['ba', 'ab'], ['abc', 'ab'], ['', 'a'], ["\x01\x00", "\x00\x01"], ["\x00\x01", "\x01\x00"], [rb(32), rb(32)], [str_repeat("\xff", 9), str_repeat("\xff", 9)]];
foreach ($cmp as [$a, $b]) { c('sodium_compare', $a, $b); c('sodium_memcmp', $a, $b); }
foreach (['', "\x00", "\xff", "\xff\xff", "\xff\x00", "\xfe\xff\xff", 'abc', str_repeat("\xff", 17), rb(20)] as $s) cr('sodium_increment', $s);
cr('sodium_increment', 123); cr('sodium_increment', null); cr('sodium_increment', [1]);
foreach ([['', ''], ["\x01", "\x02"], ["\xff", "\x01"], ["\xff\xff", "\x01\x00"], ["\xff\x7f", "\x01\x80"], ['abc', "\2\0\0"], ['ab', 'abc'], ['abc', 'ab'], [rb(16), rb(16)], [rb(33), rb(33)]] as [$a, $b]) cr('sodium_add', $a, $b);
cr('sodium_add', 123, 'abc'); cr('sodium_add', null, ''); cr('sodium_add', 'abc', 123);
foreach (['', 'a', 'test'] as $s) cr('sodium_memzero', $s);
cr('sodium_memzero', 42); cr('sodium_memzero', null); cr('sodium_memzero', [1]);
foreach (['', 'x', 'xyz', str_repeat('a', 15), str_repeat('a', 16), str_repeat('a', 17)] as $s)
    foreach ([PHP_INT_MIN, -1, 0, 1, 2, 3, 15, 16, 17, 255, 256, 1000] as $bs) c('sodium_pad', $s, $bs);
$unp = ['', "\x80", "a\x80", "\x80\x00", "a\x80\x00\x00", "ab\x80\x00", "\x00\x00", "a\x00", "a\x80\x01", "\x80\x80", "xyz\x80" . str_repeat("\0", 12), "xyz\x80" . str_repeat("\0", 13), "xyz" . str_repeat("\0", 13), str_repeat("\0", 15) . "\x80", "a\x81"];
foreach ($unp as $s) foreach ([PHP_INT_MIN, -1, 0, 1, 2, 3, 4, 16, 17, 32] as $bs) c('sodium_unpad', $s, $bs);

// ---- generichash ----
foreach ($MSGLENS as $l) {
    $m = rb($l);
    c('sodium_crypto_generichash', $m);
    c('sodium_crypto_generichash', $m, rb(32), 64);
}
foreach ([0, 1, 15, 16, 17, 32, 63, 64, 65] as $kl) foreach ([PHP_INT_MIN, -1, 0, 1, 15, 16, 17, 32, 63, 64, 65, PHP_INT_MAX] as $ol) c('sodium_crypto_generichash', 'msg', rb($kl), $ol);
foreach ([0, 1, 15, 16, 17, 32, 63, 64, 65] as $kl) foreach ([-1, 0, 15, 16, 17, 32, 64, 65] as $ol) c('sodium_crypto_generichash_init', rb($kl), $ol);
$chains = [[], [0], [1], [127], [128], [129], [1, 127], [128, 1], [200, 100], [256], [257], [255, 1, 1], [1000], [129, 129, 129], [300, 0, 5]];
foreach ([['', 32], [rb(16), 64], [rb(64), 16], ['', 64], [rb(32), 32]] as [$k, $ol]) {
    foreach ($chains as $ch) {
        $st = c('sodium_crypto_generichash_init', $k, $ol);
        foreach ($ch as $l) $st = cr('sodium_crypto_generichash_update', $st, rb($l));
        foreach ([$ol, 16, 64, 15, 65, 0, -1] as $fl) cr('sodium_crypto_generichash_final', $st, $fl);
    }
}
$st = sodium_crypto_generichash_init();
cr('sodium_crypto_generichash_update', substr($st, 1), 'x');
cr('sodium_crypto_generichash_update', $st . 'x', 'x');
cr('sodium_crypto_generichash_update', 5, 'x');
cr('sodium_crypto_generichash_update', null, 'x');
cr('sodium_crypto_generichash_update', [], 'x');
cr('sodium_crypto_generichash_final', substr($st, 1));
cr('sodium_crypto_generichash_final', 5);
cr('sodium_crypto_generichash_final', null);
cr('sodium_crypto_generichash_final', []);
len('sodium_crypto_generichash_keygen');

// ---- shorthash ----
foreach ($MSGLENS as $l) c('sodium_crypto_shorthash', rb($l), rb(16));
foreach (bad(16) as $kl) c('sodium_crypto_shorthash', 'm', rb($kl));
len('sodium_crypto_shorthash_keygen');

// ---- kdf ----
$key = rb(32);
foreach ([PHP_INT_MIN, -1, 0, 15, 16, 17, 32, 63, 64, 65, PHP_INT_MAX] as $sl) c('sodium_crypto_kdf_derive_from_key', $sl, 1, 'context!', $key);
foreach ([PHP_INT_MIN, -1, 0, 1, 2, 255, 256, 4294967295, 4294967296, PHP_INT_MAX] as $id) c('sodium_crypto_kdf_derive_from_key', 32, $id, 'context!', $key);
foreach ([0, 7, 9] as $cl) c('sodium_crypto_kdf_derive_from_key', 32, 1, rb($cl), $key);
foreach (bad(32) as $kl) c('sodium_crypto_kdf_derive_from_key', 32, 1, 'context!', rb($kl));
foreach ([16, 32, 64] as $sl) for ($i = 0; $i < 3; $i++) c('sodium_crypto_kdf_derive_from_key', $sl, mt_rand(), rb(8), rb(32));
len('sodium_crypto_kdf_keygen');

// ---- auth ----
foreach ($MSGLENS as $l) {
    $m = rb($l); $k = rb(32);
    $mac = c('sodium_crypto_auth', $m, $k);
    c('sodium_crypto_auth_verify', $mac, $m, $k);
    $bm = $mac; $bm[0] = chr(ord($bm[0]) ^ 1);
    c('sodium_crypto_auth_verify', $bm, $m, $k);
}
foreach (bad(32) as $l) { c('sodium_crypto_auth', 'm', rb($l)); c('sodium_crypto_auth_verify', rb(32), 'm', rb($l)); c('sodium_crypto_auth_verify', rb($l), 'm', rb(32)); }
len('sodium_crypto_auth_keygen');

// ---- secretbox / stream ----
foreach ($MSGLENS as $l) {
    $m = rb($l); $n = rb(24); $k = rb(32);
    $ct = c('sodium_crypto_secretbox', $m, $n, $k);
    c('sodium_crypto_secretbox_open', $ct, $n, $k);
    $b = $ct; $b[strlen($b) - 1] = chr(ord($b[strlen($b) - 1]) ^ 0x80);
    c('sodium_crypto_secretbox_open', $b, $n, $k);
    c('sodium_crypto_stream_xor', $m, $n, $k);
    c('sodium_crypto_stream_xchacha20_xor', $m, $n, $k);
    c('sodium_crypto_stream_xchacha20_xor_ic', $m, $n, mt_rand(0, 1000), $k);
}
foreach ([0, 15, 16, 17] as $l) c('sodium_crypto_secretbox_open', rb($l), rb(24), rb(32));
foreach (bad(24) as $l) { c('sodium_crypto_secretbox', 'm', rb($l), rb(32)); c('sodium_crypto_secretbox_open', rb(20), rb($l), rb(32)); c('sodium_crypto_stream', 10, rb($l), rb(32)); c('sodium_crypto_stream_xor', 'm', rb($l), rb(32)); c('sodium_crypto_stream_xchacha20', 10, rb($l), rb(32)); c('sodium_crypto_stream_xchacha20_xor', 'm', rb($l), rb(32)); c('sodium_crypto_stream_xchacha20_xor_ic', 'm', rb($l), 0, rb(32)); }
foreach (bad(32) as $l) { c('sodium_crypto_secretbox', 'm', rb(24), rb($l)); c('sodium_crypto_secretbox_open', rb(20), rb(24), rb($l)); c('sodium_crypto_stream', 10, rb(24), rb($l)); c('sodium_crypto_stream_xor', 'm', rb(24), rb($l)); c('sodium_crypto_stream_xchacha20', 10, rb(24), rb($l)); c('sodium_crypto_stream_xchacha20_xor', 'm', rb(24), rb($l)); c('sodium_crypto_stream_xchacha20_xor_ic', 'm', rb(24), 0, rb($l)); }
foreach ([PHP_INT_MIN, -1, 0, 1, 63, 64, 65, 200] as $l) { c('sodium_crypto_stream', $l, rb(24), rb(32)); c('sodium_crypto_stream_xchacha20', $l, rb(24), rb(32)); }
foreach ([PHP_INT_MIN, -1, 0, 1, 4294967295, 4294967296, PHP_INT_MAX] as $ic) c('sodium_crypto_stream_xchacha20_xor_ic', rb(130), rb(24), $ic, rb(32));
len('sodium_crypto_secretbox_keygen'); len('sodium_crypto_stream_keygen'); len('sodium_crypto_stream_xchacha20_keygen');

// ---- aead ----
$aeads = [
    ['chacha20poly1305', 8, 32], ['chacha20poly1305_ietf', 12, 32], ['xchacha20poly1305_ietf', 24, 32], ['aes256gcm', 12, 32],
];
foreach ($aeads as [$name, $nl, $kl]) {
    $e = "sodium_crypto_aead_{$name}_encrypt"; $d = "sodium_crypto_aead_{$name}_decrypt";
    foreach ($MSGLENS as $l) foreach ([0, 1, 16, 33] as $al) {
        if ($al == 33 && $l > 17) continue;
        $m = rb($l); $ad = rb($al); $n = rb($nl); $k = rb($kl);
        $ct = c($e, $m, $ad, $n, $k);
        c($d, $ct, $ad, $n, $k);
        c($d, $ct, $ad . 'x', $n, $k);
        $b = $ct; $b[0] = chr(ord($b[0]) ^ 4);
        c($d, $b, $ad, $n, $k);
    }
    foreach ([0, 1, 15, 16, 17] as $l) c($d, rb($l), '', rb($nl), rb($kl));
    foreach (bad($nl) as $l) { c($e, 'm', '', rb($l), rb($kl)); c($d, rb(20), '', rb($l), rb($kl)); }
    foreach (bad($kl) as $l) { c($e, 'm', '', rb($nl), rb($l)); c($d, rb(20), '', rb($nl), rb($l)); }
    len("sodium_crypto_aead_{$name}_keygen");
}
c('sodium_crypto_aead_aes256gcm_is_available');

// ---- scalarmult / box / kx ----
$low = [str_repeat("\0", 32), "\x01" . str_repeat("\0", 31), str_repeat("\xff", 32), sodium_hex2bin('e0eb7a7c3b41b8ae1656e3faf19fc46ada098deb9c32b1fd866205165f49b800'), sodium_hex2bin('5f9c95bca3508c24b1d0b1559c83ef5b04445cc4581c8e86d8224eddd09f1157'), sodium_hex2bin('ecffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff7f'), sodium_hex2bin('edffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff7f')];
for ($i = 0; $i < 6; $i++) { c('sodium_crypto_scalarmult', rb(32), rb(32)); c('sodium_crypto_scalarmult_base', rb(32)); }
foreach ($low as $p) c('sodium_crypto_scalarmult', rb(32), $p);
c('sodium_crypto_scalarmult_base', str_repeat("\0", 32));
foreach (bad(32) as $l) { c('sodium_crypto_scalarmult', rb($l), rb(32)); c('sodium_crypto_scalarmult', rb(32), rb($l)); c('sodium_crypto_scalarmult_base', rb($l)); }
for ($i = 0; $i < 3; $i++) {
    $kp = c('sodium_crypto_box_seed_keypair', rb(32));
    c('sodium_crypto_box_secretkey', $kp); c('sodium_crypto_box_publickey', $kp);
    $sk = substr($kp, 0, 32);
    c('sodium_crypto_box_publickey_from_secretkey', $sk);
    $kp2 = sodium_crypto_box_seed_keypair(rb(32));
    $pk2 = substr($kp2, 32);
    $kpx = c('sodium_crypto_box_keypair_from_secretkey_and_publickey', $sk, $pk2);
    $kpy = substr($kp2, 0, 32) . substr($kp, 32);
    foreach ([0, 1, 100] as $l) {
        $m = rb($l); $n = rb(24);
        $ct = c('sodium_crypto_box', $m, $n, $kpx);
        c('sodium_crypto_box_open', $ct, $n, $kpy);
        $b = $ct; $b[0] = chr(ord($b[0]) ^ 1);
        c('sodium_crypto_box_open', $b, $n, $kpy);
        $sealed = sodium_crypto_box_seal($m, substr($kp, 32));
        c('sodium_crypto_box_seal_open', $sealed, $kp);
        c('sodium_crypto_box_seal_open', $sealed, $kp2);
        len('sodium_crypto_box_seal', $m, substr($kp, 32));
    }
    $ckp = c('sodium_crypto_kx_seed_keypair', rb(32));
    $skp = c('sodium_crypto_kx_seed_keypair', rb(32));
    c('sodium_crypto_kx_publickey', $ckp); c('sodium_crypto_kx_secretkey', $ckp);
    c('sodium_crypto_kx_client_session_keys', $ckp, substr($skp, 32));
    c('sodium_crypto_kx_server_session_keys', $skp, substr($ckp, 32));
}
foreach ($low as $p) {
    c('sodium_crypto_box', 'm', rb(24), rb(32) . $p);
    c('sodium_crypto_box_open', rb(40), rb(24), rb(32) . $p);
    c('sodium_crypto_box_seal', 'm', $p);
    c('sodium_crypto_kx_client_session_keys', rb(64), $p);
    c('sodium_crypto_kx_server_session_keys', rb(64), $p);
}
foreach ([0, 15, 16, 31, 32, 47, 48, 49] as $l) { c('sodium_crypto_box_open', rb($l), rb(24), rb(64)); c('sodium_crypto_box_seal_open', rb($l), rb(64)); }
foreach (bad(32) as $l) {
    c('sodium_crypto_box_seed_keypair', rb($l)); c('sodium_crypto_box_publickey_from_secretkey', rb($l));
    c('sodium_crypto_box_keypair_from_secretkey_and_publickey', rb($l), rb(32)); c('sodium_crypto_box_keypair_from_secretkey_and_publickey', rb(32), rb($l));
    c('sodium_crypto_box_seal', 'm', rb($l)); c('sodium_crypto_kx_seed_keypair', rb($l));
    c('sodium_crypto_kx_client_session_keys', rb(64), rb($l)); c('sodium_crypto_kx_server_session_keys', rb(64), rb($l));
}
foreach (bad(64) as $l) {
    c('sodium_crypto_box', 'm', rb(24), rb($l)); c('sodium_crypto_box_open', rb(20), rb(24), rb($l));
    c('sodium_crypto_box_secretkey', rb($l)); c('sodium_crypto_box_publickey', rb($l)); c('sodium_crypto_box_seal_open', rb(50), rb($l));
    c('sodium_crypto_kx_publickey', rb($l)); c('sodium_crypto_kx_secretkey', rb($l));
    c('sodium_crypto_kx_client_session_keys', rb($l), rb(32)); c('sodium_crypto_kx_server_session_keys', rb($l), rb(32));
}
foreach (bad(24) as $l) { c('sodium_crypto_box', 'm', rb($l), rb(64)); c('sodium_crypto_box_open', rb(20), rb($l), rb(64)); }
len('sodium_crypto_box_keypair'); len('sodium_crypto_kx_keypair');

// ---- sign ----
for ($i = 0; $i < 4; $i++) {
    $kp = c('sodium_crypto_sign_seed_keypair', rb(32));
    $sk = c('sodium_crypto_sign_secretkey', $kp); $pk = c('sodium_crypto_sign_publickey', $kp);
    c('sodium_crypto_sign_publickey_from_secretkey', $sk);
    c('sodium_crypto_sign_keypair_from_secretkey_and_publickey', $sk, $pk);
    c('sodium_crypto_sign_ed25519_pk_to_curve25519', $pk);
    c('sodium_crypto_sign_ed25519_sk_to_curve25519', $sk);
    foreach ([0, 1, 64, 200] as $l) {
        $m = rb($l);
        $sm = c('sodium_crypto_sign', $m, $sk);
        c('sodium_crypto_sign_open', $sm, $pk);
        $sig = c('sodium_crypto_sign_detached', $m, $sk);
        c('sodium_crypto_sign_verify_detached', $sig, $m, $pk);
        c('sodium_crypto_sign_verify_detached', $sig, $m . 'x', $pk);
        $b = $sm; $b[$l + 63] = chr(ord($b[$l + 63]) ^ 1);
        c('sodium_crypto_sign_open', $b, $pk);
    }
    // Mismatched public half of the secret key.
    $sk2 = substr($sk, 0, 32) . rb(32);
    c('sodium_crypto_sign_detached', 'msg', $sk2);
    c('sodium_crypto_sign_publickey_from_secretkey', $sk2);
    // s + L (non-canonical s).
    $sig = sodium_crypto_sign_detached('msg', $sk);
    $s = substr($sig, 32); $L = sodium_hex2bin('edd3f55c1a631258d69cf7a2def9de1400000000000000000000000000000010');
    $sl = $s; sodium_add($sl, $L);
    c('sodium_crypto_sign_verify_detached', substr($sig, 0, 32) . $sl, 'msg', $pk);
    c('sodium_crypto_sign_verify_detached', substr($sig, 0, 32) . str_repeat("\xff", 32), 'msg', $pk);
}
$edlow = [str_repeat("\0", 32), "\x01" . str_repeat("\0", 31), sodium_hex2bin('ecffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff7f'), sodium_hex2bin('c7176a703d4dd84fba3c0b760d10670f2a2053fa2c39ccc64ec7fd7792ac037a'), sodium_hex2bin('26e8958fc2b227b045c3f489f2ef98f0d5dfac05d3c63339b13802886d53fc05'), sodium_hex2bin('0000000000000000000000000000000000000000000000000000000000000080'), str_repeat("\xff", 32), sodium_hex2bin('edffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff7f'), sodium_hex2bin('eeffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff7f'), sodium_hex2bin('0200000000000000000000000000000000000000000000000000000000000000'), sodium_hex2bin('0100000000000000000000000000000000000000000000000000000000000080')];
foreach ($edlow as $p) {
    c('sodium_crypto_sign_ed25519_pk_to_curve25519', $p);
    c('sodium_crypto_sign_verify_detached', rb(64), 'msg', $p);
    c('sodium_crypto_sign_verify_detached', $p . str_repeat("\0", 32), 'msg', sodium_crypto_sign_publickey(sodium_crypto_sign_seed_keypair(str_repeat('x', 32))));
    c('sodium_crypto_sign_open', rb(70), $p);
}
foreach ([0, 1, 63] as $l) c('sodium_crypto_sign_open', rb($l), rb(32));
foreach (bad(32) as $l) {
    c('sodium_crypto_sign_seed_keypair', rb($l)); c('sodium_crypto_sign_open', rb(80), rb($l));
    c('sodium_crypto_sign_verify_detached', rb(64), 'm', rb($l)); c('sodium_crypto_sign_ed25519_pk_to_curve25519', rb($l));
    c('sodium_crypto_sign_keypair_from_secretkey_and_publickey', rb(64), rb($l));
}
foreach (bad(64) as $l) {
    c('sodium_crypto_sign', 'm', rb($l)); c('sodium_crypto_sign_detached', 'm', rb($l));
    c('sodium_crypto_sign_verify_detached', rb($l), 'm', rb(32)); c('sodium_crypto_sign_ed25519_sk_to_curve25519', rb($l));
    c('sodium_crypto_sign_publickey_from_secretkey', rb($l)); c('sodium_crypto_sign_keypair_from_secretkey_and_publickey', rb($l), rb(32));
}
foreach (bad(96) as $l) { c('sodium_crypto_sign_publickey', rb($l)); c('sodium_crypto_sign_secretkey', rb($l)); }
len('sodium_crypto_sign_keypair');

// ---- ristretto255 ----
$pts = [];
for ($i = 0; $i < 6; $i++) $pts[] = c('sodium_crypto_core_ristretto255_from_hash', rb(64));
foreach (bad(64) as $l) c('sodium_crypto_core_ristretto255_from_hash', rb($l));
$rbad = [str_repeat("\0", 32), rb(32), rb(32), sodium_hex2bin('0100000000000000000000000000000000000000000000000000000000000000'), str_repeat("\xff", 32), sodium_hex2bin('e2f2ae0a6abc4e71a884a961c500515f58e30b6aa582dd8db6a65945e08d2d76'), sodium_hex2bin('edd3f55c1a631258d69cf7a2def9de1400000000000000000000000000000010')];
foreach (array_merge($pts, $rbad) as $p) c('sodium_crypto_core_ristretto255_is_valid_point', $p);
foreach ([0, 31, 33] as $l) c('sodium_crypto_core_ristretto255_is_valid_point', rb($l));
for ($i = 0; $i < 5; $i++) { c('sodium_crypto_core_ristretto255_add', $pts[$i], $pts[$i + 1]); c('sodium_crypto_core_ristretto255_sub', $pts[$i], $pts[$i + 1]); }
c('sodium_crypto_core_ristretto255_sub', $pts[0], $pts[0]); c('sodium_crypto_core_ristretto255_add', $pts[0], str_repeat("\0", 32));
foreach ($rbad as $p) { c('sodium_crypto_core_ristretto255_add', $pts[0], $p); c('sodium_crypto_core_ristretto255_sub', $p, $pts[0]); }
foreach (bad(32) as $l) { c('sodium_crypto_core_ristretto255_add', rb($l), $pts[0]); c('sodium_crypto_core_ristretto255_add', $pts[0], rb($l)); c('sodium_crypto_core_ristretto255_sub', rb($l), $pts[0]); c('sodium_crypto_core_ristretto255_sub', $pts[0], rb($l)); }
$L = sodium_hex2bin('edd3f55c1a631258d69cf7a2def9de1400000000000000000000000000000010');
$scal = [str_repeat("\0", 32), "\x01" . str_repeat("\0", 31), $L, str_repeat("\xff", 32), rb(32), rb(32), rb(32), rb(31) . "\x0f", rb(31) . "\x0f"];
foreach ($scal as $x) {
    foreach (['negate', 'complement', 'invert'] as $op) c("sodium_crypto_core_ristretto255_scalar_$op", $x);
    c('sodium_crypto_scalarmult_ristretto255_base', $x);
    c('sodium_crypto_scalarmult_ristretto255', $x, $pts[1]);
    foreach ([$scal[4], $scal[1], $L] as $y) foreach (['add', 'sub', 'mul'] as $op) c("sodium_crypto_core_ristretto255_scalar_$op", $x, $y);
}
foreach ($rbad as $p) c('sodium_crypto_scalarmult_ristretto255', rb(32), $p);
foreach ([rb(64), rb(64), str_repeat("\xff", 64), str_repeat("\0", 64)] as $x) c('sodium_crypto_core_ristretto255_scalar_reduce', $x);
foreach (bad(64) as $l) c('sodium_crypto_core_ristretto255_scalar_reduce', rb($l));
foreach (bad(32) as $l) {
    foreach (['negate', 'complement', 'invert'] as $op) c("sodium_crypto_core_ristretto255_scalar_$op", rb($l));
    foreach (['add', 'sub', 'mul'] as $op) { c("sodium_crypto_core_ristretto255_scalar_$op", rb($l), rb(32)); c("sodium_crypto_core_ristretto255_scalar_$op", rb(32), rb($l)); }
    c('sodium_crypto_scalarmult_ristretto255_base', rb($l)); c('sodium_crypto_scalarmult_ristretto255', rb($l), $pts[0]); c('sodium_crypto_scalarmult_ristretto255', rb(32), rb($l));
}
len('sodium_crypto_core_ristretto255_random'); len('sodium_crypto_core_ristretto255_scalar_random');

// ---- secretstream ----
foreach (bad(24) as $l) c('sodium_crypto_secretstream_xchacha20poly1305_init_pull', rb($l), rb(32));
foreach (bad(32) as $l) { c('sodium_crypto_secretstream_xchacha20poly1305_init_pull', rb(24), rb($l)); c('sodium_crypto_secretstream_xchacha20poly1305_init_push', rb($l)); }
len('sodium_crypto_secretstream_xchacha20poly1305_init_push', rb(32));
len('sodium_crypto_secretstream_xchacha20poly1305_keygen');
for ($round = 0; $round < 3; $round++) {
    $h = rb(24); $k = rb(32);
    $st = c('sodium_crypto_secretstream_xchacha20poly1305_init_pull', $h, $k);
    $pst = $st; $msgs = [];
    foreach ([[10, 0, 0], [0, 0, 5], [100, 1, 0], [64, 2, 3], [300, 0, 0], [1, 2, 0], [17, 0, 17], [5, 3, 0]] as [$ml, $tag, $al]) {
        $m = rb($ml); $ad = rb($al);
        $before = $pst;
        try { $ct = sodium_crypto_secretstream_xchacha20poly1305_push($pst, $m, $ad, $tag); } catch (Throwable $e) { $ct = null; }
        emit('sodium_crypto_secretstream_xchacha20poly1305_push', [$before, $m, $ad, $tag], ($ct === null ? 'E' : res($ct)) . ' R' . tok($pst));
        $msgs[] = [$ct, $ad];
    }
    $qst = $st;
    foreach ($msgs as $i => [$ct, $ad]) {
        if ($i == 3) { $b = $qst; $qst = cr('sodium_crypto_secretstream_xchacha20poly1305_pull', $qst, $ct, $ad . 'x'); }
        $before = $qst;
        try { $r = sodium_crypto_secretstream_xchacha20poly1305_pull($qst, $ct, $ad); $o = res($r); } catch (Throwable $e) { $o = 'E' . get_class($e) . ':' . $e->getMessage(); }
        emit('sodium_crypto_secretstream_xchacha20poly1305_pull', [$before, $ct, $ad], $o . ' R' . tok($qst));
    }
    $x = $st; cr('sodium_crypto_secretstream_xchacha20poly1305_rekey', $x);
}
$st = sodium_crypto_secretstream_xchacha20poly1305_init_pull(rb(24), rb(32));
// counter near wrap-around
$wrap = substr($st, 0, 32) . "\xfe\xff\xff\xff" . substr($st, 36);
$w = $wrap;
for ($i = 0; $i < 3; $i++) $w = cr('sodium_crypto_secretstream_xchacha20poly1305_push', $w, 'abc', '', 0);
$w = $wrap;
for ($i = 0; $i < 3; $i++) { $c = sodium_crypto_secretstream_xchacha20poly1305_push($w, 'abc'); }
foreach ([PHP_INT_MIN, -1, 4, 5, 255, 256] as $tag) cr('sodium_crypto_secretstream_xchacha20poly1305_push', $st, 'm', '', $tag);
foreach ([0, 1, 16, 17, 18] as $l) cr('sodium_crypto_secretstream_xchacha20poly1305_pull', $st, rb($l), '');
foreach ([0, 51, 53] as $l) { cr('sodium_crypto_secretstream_xchacha20poly1305_push', rb($l), 'm'); cr('sodium_crypto_secretstream_xchacha20poly1305_pull', rb($l), rb(20)); cr('sodium_crypto_secretstream_xchacha20poly1305_rekey', rb($l)); }
cr('sodium_crypto_secretstream_xchacha20poly1305_push', 5, 'm'); cr('sodium_crypto_secretstream_xchacha20poly1305_pull', null, 'm'); cr('sodium_crypto_secretstream_xchacha20poly1305_rekey', 5);
cr('sodium_crypto_secretstream_xchacha20poly1305_rekey', $wrap);

// ---- pwhash (argon2) ----
$salt = rb(16);
foreach ([0, 1, 2, 3, 4] as $ops) foreach ([0, 8191, 8192, 8193, 9216, 65536, 1048576] as $mem) foreach ([1, 2] as $alg) c('sodium_crypto_pwhash', 32, 'password', $salt, $ops, $mem, $alg);
foreach ([PHP_INT_MIN, -1, 0, 15, 16, 17, 64, 100, 1000] as $ol) c('sodium_crypto_pwhash', $ol, 'pw', $salt, 3, 16384, 1);
foreach ([PHP_INT_MIN, -1, 0, 3, 4, 100] as $alg) c('sodium_crypto_pwhash', 16, 'pw', $salt, 3, 16384, $alg);
foreach ([PHP_INT_MIN, -1] as $v) { c('sodium_crypto_pwhash', 16, 'pw', $salt, $v, 16384, 2); c('sodium_crypto_pwhash', 16, 'pw', $salt, 3, $v, 2); }
foreach (bad(16) as $l) c('sodium_crypto_pwhash', 16, 'pw', rb($l), 3, 16384, 2);
foreach (['', 'p', rb(100)] as $pw) foreach ([1, 2] as $alg) c('sodium_crypto_pwhash', 16, $pw, rb(16), 3, 32768, $alg);
foreach ([[1, 8192], [2, 8192], [3, 16384], [2, 1048576]] as [$ops, $mem]) {
    foreach (['', 'password', rb(40)] as $pw) {
        $h = c('sodium_crypto_pwhash_str', $pw, $ops, $mem);
        if ($h === null) continue;
        c('sodium_crypto_pwhash_str_verify', $h, $pw);
        c('sodium_crypto_pwhash_str_verify', $h, $pw . 'x');
        foreach ([[$ops, $mem], [$ops + 1, $mem], [$ops, $mem * 2], [$ops, $mem + 1], [$ops, $mem + 1024], [$ops - 1, $mem]] as [$o2, $m2]) c('sodium_crypto_pwhash_str_needs_rehash', $h, $o2, $m2);
    }
}
foreach ([PHP_INT_MIN, -1, 0] as $v) { c('sodium_crypto_pwhash_str', 'pw', $v, 8192); c('sodium_crypto_pwhash_str', 'pw', 2, $v); }
c('sodium_crypto_pwhash_str', 'pw', 1, 8191);
$known = [
    '$argon2i$v=19$m=4096,t=3,p=1$MzE4ODFiZWFlMjAzOWUAAA$FWUV6tsyJ32qThiLi1cCsLIbf3dIOG/RwXcTzt536KY',
    '$argon2i$v=19$m=4096,t=0,p=1$c29tZXNhbHQAAAAAAAAAAA$JTBozgKQiCn5yKAm3Hz0vUSX/XgfqhZloNCxDWmeDr0',
];
$base = sodium_crypto_pwhash_str('password', 2, 8192);
$hid = sodium_crypto_pwhash_str('password', 3, 16384);
$hidi = '$argon2i$' . substr($hid, 10);
$argonvar = array_merge($known, [$base, $hid, $hidi,
    str_replace('v=19', 'v=16', $base), str_replace('$v=19', '', $base), str_replace('p=1', 'p=2', $base), str_replace('t=2', 't=02', $base),
    str_replace('m=8', 'm=08', $base), substr($base, 0, -1), $base . 'A', $base . '=', substr($base, 0, -2) . '==', '$argon2d' . substr($base, 9), '$argon2' . substr($base, 9),
    str_replace('$argon2id$', '$ARGON2ID$', $base), str_replace(',t=2,', ',t=2 ,', $base), ' ' . $base, $base . "\0", '', 'password', '$argon2id$', '$argon2id$v=19$m=8,t=2,p=1$AAAAAAAAAAAAAAAAAAAAAA$AAAAAAAAAAAAAAAAAAAAAA',
    str_replace('m=8,', 'm=4294967296,', $base), str_replace('t=2,', 't=4294967296,', $base), str_replace(',p=1', ',p=0', $base),
    str_replace('m=8192,t=2', 't=2,m=8192', $base), '$7$C6..../....QQ.sGgUWYmlOVDS5MewlqahxY2ZONvLf7FSM0dhVvk7$hz8ylNDKMQ6xb2T6P8NfyahfwAfsIivRk6FDoRZ0OV/']);
foreach ($argonvar as $h) {
    c('sodium_crypto_pwhash_str_verify', $h, 'password');
    c('sodium_crypto_pwhash_str_needs_rehash', $h, 2, 8192);
    c('sodium_crypto_pwhash_str_needs_rehash', $h, 3, 16384);
}
foreach ([PHP_INT_MIN, -1, 0, 4294967296, PHP_INT_MAX] as $v) { c('sodium_crypto_pwhash_str_needs_rehash', $base, $v, 8192); c('sodium_crypto_pwhash_str_needs_rehash', $base, 2, $v); }

// ---- scrypt ----
$ssalt = rb(32);
foreach ([524288, 1048576, 2097152, 4194304, 33554432] as $ops) foreach ([16777216, 16777217, 33554432] as $mem) {
    $h = c('sodium_crypto_pwhash_scryptsalsa208sha256_str', 'pw', $ops, $mem);
    if ($h === null) continue;
    c('sodium_crypto_pwhash_scryptsalsa208sha256_str_verify', $h, 'pw');
    c('sodium_crypto_pwhash_scryptsalsa208sha256_str_verify', $h, 'px');
}
foreach ([524288, 1048576] as $ops) foreach ([16777216, 33554432] as $mem) c('sodium_crypto_pwhash_scryptsalsa208sha256', 64, 'password', $ssalt, $ops, $mem);
foreach ([PHP_INT_MIN, -1, 0, 1, 524287] as $v) { c('sodium_crypto_pwhash_scryptsalsa208sha256', 16, 'pw', $ssalt, $v, 16777216); c('sodium_crypto_pwhash_scryptsalsa208sha256', 16, 'pw', $ssalt, 524288, $v); c('sodium_crypto_pwhash_scryptsalsa208sha256_str', 'pw', $v, 16777216); c('sodium_crypto_pwhash_scryptsalsa208sha256_str', 'pw', 524288, $v); }
foreach ([16777215, 8388608] as $v) { c('sodium_crypto_pwhash_scryptsalsa208sha256', 16, 'pw', $ssalt, 524288, $v); c('sodium_crypto_pwhash_scryptsalsa208sha256_str', 'pw', 524288, $v); }
foreach ([PHP_INT_MIN, -1, 0, 1, 15, 16, 17] as $ol) c('sodium_crypto_pwhash_scryptsalsa208sha256', $ol, 'pw', $ssalt, 524288, 16777216);
foreach (bad(32) as $l) c('sodium_crypto_pwhash_scryptsalsa208sha256', 16, 'pw', rb($l), 524288, 16777216);
$sh = sodium_crypto_pwhash_scryptsalsa208sha256_str('pw', 524288, 16777216);
foreach (['', '$7$', substr($sh, 0, -1), $sh . 'x', str_replace('$7$', '$8$', $sh), $base, '$7$C6..../....$', '$7$B6..../....QQ.sGgUWYmlOVDS5MewlqahxY2ZONvLf7FSM0dhVvk7$hz8ylNDKMQ6xb2T6P8NfyahfwAfsIivRk6FDoRZ0OV/', '$7$C6..../....QQ.sGgUWYmlOVDS5MewlqahxY2ZONvLf7FSM0dhVvk7$hz8ylNDKMQ6xb2T6P8NfyahfwAfsIivRk6FDoRZ0OV/'] as $h)
    c('sodium_crypto_pwhash_scryptsalsa208sha256_str_verify', $h, 'pw');
