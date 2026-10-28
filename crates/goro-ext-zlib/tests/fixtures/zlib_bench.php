<?php
// Throughput reference for examples/zlib_bench.rs:
//   php zlib_bench.php
// Builds the same deterministic inputs as the Rust bench (gen_data.php:
// 4 MB of mixed text and binary, 4 MB of text) and reports the best MB/s
// (of uncompressed bytes) of gzcompress() at levels 1, 6 and 9 and of
// gzuncompress() on each result.
require __DIR__ . '/gen_data.php';

$sets = [
    'mixed' => mixed_data(5, 4000000),
    'text' => text_data(4, 4000000),
];
foreach ($sets as $name => $data) {
    $mb = strlen($data) / 1e6;
    foreach ([1, 6, 9] as $level) {
        $best = INF;
        for ($i = 0; $i < 5; $i++) {
            $t = hrtime(true);
            $c = gzcompress($data, $level);
            $best = min($best, (hrtime(true) - $t) / 1e9);
        }
        $bestd = INF;
        for ($i = 0; $i < 10; $i++) {
            $t = hrtime(true);
            $d = gzuncompress($c);
            $bestd = min($bestd, (hrtime(true) - $t) / 1e9);
        }
        if ($d !== $data) die("mismatch\n");
        printf("%-6s level %d  ratio %5.3f  compress %7.1f MB/s  uncompress %7.1f MB/s\n",
            $name, $level, strlen($c) / strlen($data), $mb / $best, $mb / $bestd);
    }
}
