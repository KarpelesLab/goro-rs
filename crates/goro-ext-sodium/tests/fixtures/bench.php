<?php
// Timing of the operations `cargo run --release --example bench` measures,
// for comparison on the reference PHP binary (libsodium). Each line is
// "name  microseconds-per-call" (best of 5 runs).
function bench($name, $iters, $f) {
    $best = INF;
    for ($r = 0; $r < 5; $r++) {
        $t = hrtime(true);
        for ($i = 0; $i < $iters; $i++) $f();
        $best = min($best, (hrtime(true) - $t) / $iters / 1000);
    }
    printf("%-40s %12.2f\n", $name, $best);
}
$mib = str_repeat("\x5a", 1 << 20);
$k = str_repeat("k", 32); $n24 = str_repeat("n", 24); $n12 = str_repeat("n", 12); $n8 = str_repeat("n", 8);
bench('secretbox 1MiB', 50, fn() => sodium_crypto_secretbox($mib, $n24, $k));
$c = sodium_crypto_secretbox($mib, $n24, $k);
bench('secretbox_open 1MiB', 50, fn() => sodium_crypto_secretbox_open($c, $n24, $k));
bench('stream_xor (xsalsa20) 1MiB', 50, fn() => sodium_crypto_stream_xor($mib, $n24, $k));
bench('stream_xchacha20_xor 1MiB', 50, fn() => sodium_crypto_stream_xchacha20_xor($mib, $n24, $k));
bench('aead chacha20poly1305 1MiB', 50, fn() => sodium_crypto_aead_chacha20poly1305_encrypt($mib, 'ad', $n8, $k));
bench('aead chacha20poly1305_ietf 1MiB', 50, fn() => sodium_crypto_aead_chacha20poly1305_ietf_encrypt($mib, 'ad', $n12, $k));
bench('aead xchacha20poly1305_ietf 1MiB', 50, fn() => sodium_crypto_aead_xchacha20poly1305_ietf_encrypt($mib, 'ad', $n24, $k));
$c = sodium_crypto_aead_xchacha20poly1305_ietf_encrypt($mib, 'ad', $n24, $k);
bench('aead xchacha20poly1305_ietf dec 1MiB', 50, fn() => sodium_crypto_aead_xchacha20poly1305_ietf_decrypt($c, 'ad', $n24, $k));
bench('aead aes256gcm 1MiB', 50, fn() => sodium_crypto_aead_aes256gcm_encrypt($mib, 'ad', $n12, $k));
if (function_exists('sodium_crypto_aead_aegis128l_encrypt')) {
    bench('aead aegis128l 1MiB', 50, fn() => sodium_crypto_aead_aegis128l_encrypt($mib, 'ad', str_repeat('n', 16), str_repeat('k', 16)));
    bench('aead aegis256 1MiB', 50, fn() => sodium_crypto_aead_aegis256_encrypt($mib, 'ad', $k, $k));
}
bench('generichash 1MiB', 50, fn() => sodium_crypto_generichash($mib));
bench('generichash 64B', 100000, fn() => sodium_crypto_generichash('0123456789012345678901234567890123456789012345678901234567890123'));
$st = sodium_crypto_generichash_init();
bench('generichash_update 1MiB', 50, function () use (&$st, $mib) { sodium_crypto_generichash_update($st, $mib); });
bench('auth 1MiB', 50, fn() => sodium_crypto_auth($mib, $k));
bench('shorthash 64B', 100000, fn() => sodium_crypto_shorthash('0123456789012345678901234567890123456789012345678901234567890123', '0123456789abcdef'));
$kp = sodium_crypto_sign_seed_keypair($k); $sk = sodium_crypto_sign_secretkey($kp); $pk = sodium_crypto_sign_publickey($kp);
bench('sign_detached 64B', 5000, fn() => sodium_crypto_sign_detached('message', $sk));
$sig = sodium_crypto_sign_detached('message', $sk);
bench('sign_verify_detached 64B', 5000, fn() => sodium_crypto_sign_verify_detached($sig, 'message', $pk));
bench('sign_seed_keypair', 5000, fn() => sodium_crypto_sign_seed_keypair($k));
$a = sodium_crypto_box_seed_keypair($k); $b = sodium_crypto_box_seed_keypair($n24 . 'xxxxxxxx');
$ab = sodium_crypto_box_keypair_from_secretkey_and_publickey(substr($a, 0, 32), substr($b, 32));
bench('box 64B', 5000, fn() => sodium_crypto_box('0123456789012345678901234567890123456789012345678901234567890123', $n24, $ab));
bench('box_seal 64B', 5000, fn() => sodium_crypto_box_seal('0123456789012345678901234567890123456789012345678901234567890123', substr($a, 32)));
bench('scalarmult', 5000, fn() => sodium_crypto_scalarmult($k, substr($b, 32)));
bench('scalarmult_base', 5000, fn() => sodium_crypto_scalarmult_base($k));
bench('scalarmult_ristretto255', 5000, fn() => sodium_crypto_scalarmult_ristretto255($k, sodium_crypto_scalarmult_ristretto255_base($k)));
bench('pwhash argon2id interactive', 3, fn() => sodium_crypto_pwhash(32, 'password', str_repeat('s', 16), SODIUM_CRYPTO_PWHASH_OPSLIMIT_INTERACTIVE, SODIUM_CRYPTO_PWHASH_MEMLIMIT_INTERACTIVE));
bench('pwhash argon2i interactive', 3, fn() => sodium_crypto_pwhash(32, 'password', str_repeat('s', 16), 4, SODIUM_CRYPTO_PWHASH_MEMLIMIT_INTERACTIVE, SODIUM_CRYPTO_PWHASH_ALG_ARGON2I13));
bench('pwhash scrypt interactive', 3, fn() => sodium_crypto_pwhash_scryptsalsa208sha256(32, 'password', $k, SODIUM_CRYPTO_PWHASH_SCRYPTSALSA208SHA256_OPSLIMIT_INTERACTIVE, SODIUM_CRYPTO_PWHASH_SCRYPTSALSA208SHA256_MEMLIMIT_INTERACTIVE));
$h = sodium_hex2bin(str_repeat('ab', 32));
bench('bin2hex 1MiB', 50, fn() => sodium_bin2hex($mib));
$hx = sodium_bin2hex($mib);
bench('hex2bin 1MiB', 50, fn() => sodium_hex2bin($hx));
bench('bin2base64 1MiB', 50, fn() => sodium_bin2base64($mib, SODIUM_BASE64_VARIANT_ORIGINAL));
$bx = sodium_bin2base64($mib, SODIUM_BASE64_VARIANT_ORIGINAL);
bench('base642bin 1MiB', 50, fn() => sodium_base642bin($bx, SODIUM_BASE64_VARIANT_ORIGINAL));
