<?php
// Second batch of PHP 8.5 observations, sodium_php85_2.txt
// (same line format as sodium_driver.php): Ed25519 signatures with the
// identity as R, the Argon2 string length limit, Argon2 strings with
// several lanes (made by password_hash()), and scrypt limits.
require __DIR__ . '/sodium_driver_lib.php';
mt_srand(20261029);

// Ed25519: R = identity, S = k·a (passes the cofactorless equation).
for ($i = 0; $i < 3; $i++) {
    $seed = rb(32);
    $kp = sodium_crypto_sign_seed_keypair($seed);
    $pk = sodium_crypto_sign_publickey($kp);
    $h = hash('sha512', $seed, true);
    $a = substr($h, 0, 32);
    $a[0] = chr(ord($a[0]) & 248);
    $a[31] = chr((ord($a[31]) & 127) | 64);
    foreach (["\x01" . str_repeat("\0", 31), str_repeat("\0", 32)] as $R) {
        $m = rb(10);
        $k = sodium_crypto_core_ristretto255_scalar_reduce(hash('sha512', $R . $pk . $m, true));
        $s = sodium_crypto_core_ristretto255_scalar_mul($k, $a);
        c('sodium_crypto_sign_verify_detached', $R . $s, $m, $pk);
    }
}

// Argon2 string lengths around 128 bytes.
function b($n) { return rtrim(base64_encode(str_repeat("\x11", $n)), '='); }
foreach ([[8, 64], [8, 65], [9, 66], [8, 66], [9, 67], [12, 66], [8, 70]] as [$sl, $hl]) {
    $s = '$argon2id$v=19$m=8,t=1,p=1$' . b($sl) . '$' . b($hl);
    c('sodium_crypto_pwhash_str_needs_rehash', $s, 1, 8192);
    c('sodium_crypto_pwhash_str_verify', $s, 'x');
}

// Several lanes.
foreach ([[PASSWORD_ARGON2ID, 2, 64, 1], [PASSWORD_ARGON2I, 3, 96, 3], [PASSWORD_ARGON2ID, 4, 128, 2], [PASSWORD_ARGON2ID, 1, 64, 1]] as [$alg, $threads, $mem, $time]) {
    $pw = rb(12);
    $h = password_hash($pw, $alg, ['threads' => $threads, 'memory_cost' => $mem, 'time_cost' => $time]);
    c('sodium_crypto_pwhash_str_verify', $h, $pw);
    c('sodium_crypto_pwhash_str_verify', $h, $pw . 'x');
    c('sodium_crypto_pwhash_str_needs_rehash', $h, $time, $mem * 1024);
}

// scrypt: limits around the opslimit < memlimit / 32 switch.
foreach ([[524288, 16777216], [524289, 16777216], [524287 + 1, 16777248], [600000, 19200001], [600000, 19199999], [9000000, 100000000], [2000000, 50000000]] as [$o, $m]) {
    $h = sodium_crypto_pwhash_scryptsalsa208sha256_str('pw', $o, $m);
    c('sodium_crypto_pwhash_scryptsalsa208sha256_str_verify', $h, 'pw');
    echo "#scrypt\t$o $m\t=>\t", substr($h, 0, 14), "\n";
    c('sodium_crypto_pwhash_scryptsalsa208sha256', 32, 'pw', str_repeat("\x07", 32), $o, $m);
}
