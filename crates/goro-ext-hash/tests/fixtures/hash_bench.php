<?php
// PHP side of examples/hash_bench.rs: hash() throughput on 1 MiB, best of 5.
$data = '';
for ($i = 0; $i < (1 << 20); $i++) $data .= chr((($i * 2654435761) & 0xffffffff) >> 13 & 255);
$algos = array_slice($argv, 1) ?: ['md5', 'sha1', 'sha256', 'sha512', 'sha3-256', 'crc32b', 'xxh3', 'xxh128', 'murmur3a', 'murmur3f', 'xxh64'];
foreach ($algos as $a) {
    $best = INF;
    for ($r = 0; $r < 5; $r++) {
        $t = hrtime(true);
        for ($i = 0; $i < 20; $i++) hash($a, $data);
        $best = min($best, (hrtime(true) - $t) / 1e9 / 20);
    }
    printf("%-10s hash(): %8.1f MB/s\n", $a, 1.048576 / $best);
}
