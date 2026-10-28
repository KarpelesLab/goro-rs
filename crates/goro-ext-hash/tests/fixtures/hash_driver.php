<?php
// Fixture driver for goro-ext-hash (observed behaviour of PHP 8.5). Run once:
//   php85 -d error_reporting=-1 -d display_errors=1 -d log_errors=0 -d html_errors=0 \
//       hash_driver.php > hash_php85.txt
// Line formats (all binary data hex or base64):
//   D algo input digest              hash()
//   C algo seq digest b64(serialize)  hash_init + chunked hash_update (+ serialize)
//   H algo key input mac             hash_hmac()
//   K algo len info salt okm         hash_hkdf() on key "input key material"
//   P algo iter len bin out          hash_pbkdf2("password", "salt")
//   O algo optid input digest        hash() with options
//   Q algo optid seq digest          hash_init() with options, chunked
//   E label message                  errors and diagnostics

// Deterministic input bytes: an LCG seeded by $seed.
function prng($seed, $n) {
    $x = $seed & 0x7fffffff;
    $s = '';
    for ($i = 0; $i < $n; $i++) {
        $x = ($x * 1103515245 + 12345) & 0x7fffffff;
        $s .= chr(($x >> 16) & 255);
    }
    return $s;
}

$lens = [0, 1, 2, 3, 4, 5, 7, 8, 9, 15, 16, 17, 31, 32, 33, 55, 56, 57, 63, 64, 65,
    111, 112, 113, 119, 127, 128, 129, 135, 136, 137, 143, 144, 145, 191, 192, 200,
    239, 240, 241, 255, 256, 257, 500, 1000, 1023, 1024, 1025, 2047, 2048, 2049, 4096, 5000];
$inputs = [];
foreach ($lens as $n) $inputs["r$n"] = prng($n, $n);
$inputs['abc'] = 'abc';
$inputs['fox'] = 'The quick brown fox jumps over the lazy dog';

$seqs = ['3,5', '60,10', '100,30,70', '1,63,1', '200,200', '130,500,7', '33,1,31,64,2',
    '64,64', '16,16,16', '7,7,7,7,7,7,7,7,7,7', '250,1', '1,250', '1100,3,1100'];

foreach (hash_algos() as $a) {
    foreach ($inputs as $id => $in) echo "D $a $id ", hash($a, $in), "\n";
    foreach ($seqs as $si => $seq) {
        $c = hash_init($a);
        $pos = 0;
        foreach (explode(',', $seq) as $k => $n) {
            hash_update($c, prng(1000 + $si * 10 + $k, (int)$n));
        }
        try { $ser = base64_encode(serialize($c)); } catch (Throwable $e) { $ser = '-'; }
        echo "C $a $seq ", hash_final($c), " $ser\n";
    }
}

$keys = ['' => '', 'k' => 'k', 'key16' => str_repeat('K', 16), 'key64' => prng(7, 64),
    'key65' => prng(8, 65), 'key200' => prng(9, 200)];
foreach (hash_hmac_algos() as $a) {
    foreach ($keys as $kid => $k) {
        foreach (['r0', 'r3', 'r64', 'r1000', 'fox'] as $id) {
            echo "H $a ", $kid === '' ? 'empty' : $kid, " $id ", hash_hmac($a, $inputs[$id], $k), "\n";
        }
    }
    foreach ([0, 1, 10, 32, 100] as $len) {
        foreach ([['', ''], ['info', ''], ['', 'salt'], ['info', 'salt']] as [$info, $salt]) {
            if ($len > 255 * strlen(hash($a, '', true))) continue;
            echo "K $a $len ", $info === '' ? '-' : $info, ' ', $salt === '' ? '-' : $salt, ' ',
                bin2hex(hash_hkdf($a, 'input key material', $len, $info, $salt)), "\n";
        }
    }
    foreach ([1, 2, 7] as $it) {
        foreach ([0, 5, 20, 65] as $len) {
            echo "P $a $it $len 0 ", hash_pbkdf2($a, 'password', 'salt', $it, $len), "\n";
            echo "P $a $it $len 1 ", bin2hex(hash_pbkdf2($a, 'password', 'salt', $it, $len, true)), "\n";
        }
    }
}

$optsets = [
    'seed0' => ['seed' => 0], 'seed1' => ['seed' => 1], 'seed42' => ['seed' => 42],
    'seedbig' => ['seed' => (1 << 32) + 5], 'seedneg' => ['seed' => -1],
    'seedmax' => ['seed' => PHP_INT_MAX],
];
$secrets = ['sec136' => prng(11, 136), 'sec192' => prng(12, 192), 'sec200' => prng(13, 200),
    'sec256' => prng(14, 256)];
foreach ($secrets as $sid => $s) $optsets[$sid] = ['secret' => $s];
foreach (['murmur3a', 'murmur3c', 'murmur3f', 'xxh32', 'xxh64', 'xxh3', 'xxh128'] as $a) {
    foreach ($optsets as $oid => $o) {
        if (isset($o['secret']) && !in_array($a, ['xxh3', 'xxh128'])) continue;
        foreach ($inputs as $id => $in) echo "O $a $oid $id ", hash($a, $in, false, $o), "\n";
        foreach ($seqs as $si => $seq) {
            $c = hash_init($a, 0, '', $o);
            foreach (explode(',', $seq) as $k => $n) hash_update($c, prng(1000 + $si * 10 + $k, (int)$n));
            echo "Q $a $oid $seq ", hash_final($c), "\n";
        }
    }
}
// xxh3 streams long enough to cross several blocks, in odd chunks.
foreach (['xxh3', 'xxh128'] as $a) {
    foreach (['def' => [], 'seed9' => ['seed' => 9], 'sec136' => ['secret' => $secrets['sec136']]] as $oid => $o) {
        foreach (['1000,1000,1000,1000', '64,64,64,64,64,64,64,64,64,64,64,64,64,64,64,64,64', '1024,1', '2048', '2049', '5,3000,999,1,4000'] as $seq) {
            $c = hash_init($a, 0, '', $o);
            foreach (explode(',', $seq) as $k => $n) hash_update($c, prng(2000 + $k, (int)$n));
            echo "Q $a $oid $seq ", hash_final($c), "\n";
        }
    }
}

// Unserialize errors not covered by the tests.
function unser_err($label, $arr) {
    $s = 'O:11:"HashContext"' . substr(serialize($arr), 1);
    try {
        $c = unserialize($s);
        echo "E $label ok ", hash_final($c), "\n";
    } catch (Throwable $e) {
        echo "E $label ", get_class($e), ': ', $e->getMessage(), "\n";
    }
}
$c = hash_init('md5'); hash_update($c, 'abc'); $md5 = $c->__serialize();
$m = $md5; $m[0] = 'xxh3'; unser_err('xxh3-algo', $m);
$m = $md5; $m[0] = 'nope'; $m[1] = 1; unser_err('unknown+hmac', $m);
$m = $md5; $m[0] = 'nope'; $m[3] = 7; unser_err('unknown+magic', $m);
$m = $md5; $m[2] = 'x'; $m[3] = 7; unser_err('notarray+magic', $m);
$m = $md5; $m[1] = 1; unset($m[2]); unser_err('hmac+nostate', $m);
$m = $md5; $m[0] = 'MD5'; unser_err('upper', $m);
$m = $md5; $m[0] = 'snefru256'; unser_err('md5-as-snefru256', $m);
$c = hash_init('sha3-256'); hash_update($c, 'abc'); $s3 = $c->__serialize();
$m = $s3; $m[3] = 2; unser_err('sha3-magic2', $m);
