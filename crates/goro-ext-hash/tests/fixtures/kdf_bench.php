<?php
// PHP side of examples/kdf_bench.rs.
foreach (['md5', 'sha1', 'sha256', 'sha512'] as $a) {
    $t = hrtime(true);
    $out = hash_pbkdf2($a, 'password', 'salt', 100000);
    $p = (hrtime(true) - $t) / 1e6;
    $t = hrtime(true);
    for ($i = 0; $i < 200000; $i++) hash_hmac($a, pack('V', $i), 'key');
    $h = (hrtime(true) - $t) / 1e6;
    printf("%-8s pbkdf2 100k: %8.1f ms   200k short hmacs: %8.1f ms   (%s)\n", $a, $p, $h, substr($out, 0, 8));
}
