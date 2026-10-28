<?php
// Throughput reference for examples/json_bench.rs:
//   php json_bench.php <prefix>
// Builds deterministic documents (mixed records with strings, unicode,
// escapes, ints, floats, nested objects and lists; a float-heavy one; a
// string-heavy one), writes them to <prefix>.<name>.json, and reports MB/s
// of json_decode/json_validate/json_encode on each.
mt_srand(7);
$words = ['alpha', 'beta', 'gamma', 'delta', 'epsilon', 'zeta', 'eta', 'theta', 'iota', 'kappa', 'lambda',
          'café', 'naïve', 'Ωmega', '日本語', 'данные', 'path/to/file', 'quote"d', 'tab	bed', "line\nbreak"];
$recs = [];
for ($i = 0; $i < 16000; $i++) {
    $tags = [];
    for ($k = mt_rand(0, 6); $k > 0; $k--) {
        $tags[] = $words[mt_rand(0, count($words) - 1)];
    }
    $desc = '';
    for ($k = mt_rand(3, 25); $k > 0; $k--) {
        $desc .= $words[mt_rand(0, count($words) - 1)] . ' ';
    }
    $recs[] = [
        'id' => $i,
        'uuid' => sprintf('%08x-%04x-%04x', mt_rand(), mt_rand(0, 65535), mt_rand(0, 65535)),
        'name' => ucfirst($words[mt_rand(0, 10)]) . ' ' . ucfirst($words[mt_rand(0, 10)]),
        'email' => "user$i@example.com",
        'active' => (bool)mt_rand(0, 1),
        'score' => mt_rand(0, 1000000) / 1000,
        'ratio' => mt_rand() / mt_getrandmax(),
        'count' => mt_rand(-100000, 100000),
        'tags' => $tags,
        'geo' => ['lat' => mt_rand(-90000000, 90000000) / 1e6, 'lng' => mt_rand(-180000000, 180000000) / 1e6],
        'url' => "https://example.com/items/$i?ref=" . mt_rand(),
        'description' => $desc,
        'parent' => mt_rand(0, 3) ? null : mt_rand(0, $i),
        'history' => array_map(fn($x) => ['t' => 1700000000 + $x * 3600, 'v' => $x * 1.5], range(0, mt_rand(0, 4))),
    ];
}
$docs = ['mixed' => json_encode($recs)];
$nums = [];
for ($i = 0; $i < 300000; $i++) {
    $nums[] = [mt_rand() / mt_getrandmax() * 1000, mt_rand(-1000000, 1000000), 1.5e-7 * mt_rand()];
}
$docs['numbers'] = json_encode($nums);
$strs = [];
for ($i = 0; $i < 20000; $i++) {
    $s = '';
    for ($k = mt_rand(5, 60); $k > 0; $k--) {
        $s .= $words[mt_rand(0, count($words) - 1)] . ' ';
    }
    $strs[] = $s;
}
$docs['strings'] = json_encode($strs);

function bench(string $label, float $mb, callable $f): void {
    $f();
    $best = INF;
    for ($r = 0; $r < 5; $r++) {
        $t = hrtime(true);
        $f();
        $best = min($best, (hrtime(true) - $t) / 1e9);
    }
    printf("%-28s %8.1f MB/s (%.2f ms)\n", $label, $mb / $best, $best * 1e3);
}

foreach ($docs as $name => $doc) {
    file_put_contents("$argv[1].$name.json", $doc);
    $mb = strlen($doc) / 1e6;
    printf("%s: %.2f MB\n", $name, $mb);
    $arr = json_decode($doc, true);
    $obj = json_decode($doc);
    bench('decode assoc', $mb, fn() => json_decode($doc, true));
    bench('decode objects', $mb, fn() => json_decode($doc));
    bench('validate', $mb, fn() => json_validate($doc));
    bench('encode arrays', $mb, fn() => json_encode($arr));
    bench('encode objects', $mb, fn() => json_encode($obj));
    bench('encode unescaped', $mb, fn() => json_encode($arr, JSON_UNESCAPED_UNICODE | JSON_UNESCAPED_SLASHES));
    bench('encode pretty', $mb, fn() => json_encode($arr, JSON_PRETTY_PRINT));
}
