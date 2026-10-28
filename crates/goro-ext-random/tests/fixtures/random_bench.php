<?php
// PHP counterpart of examples/random_bench.rs: best of 5 runs, in
// nanoseconds per call (per byte for getBytes and str_shuffle). The first
// line is the cost of the bare loop, included in every function loop.
use Random\Engine\{Mt19937, PcgOneseq128XslRr64, Xoshiro256StarStar, Secure};
use Random\{Randomizer, IntervalBoundary};

function best(string $label, int $n, callable $f) {
    $best = PHP_FLOAT_MAX;
    for ($i = 0; $i < 5; $i++) {
        $t = hrtime(true);
        $f();
        $best = min($best, hrtime(true) - $t);
    }
    printf("%-36s %8.2f ns\n", $label, $best / $n);
}
$N = 10_000_000;
best('empty loop', $N, function () use ($N) { $x = 0; for ($i = 0; $i < $N; $i++) { $x ^= $i; } });
mt_srand(42);
best('mt_rand()', $N, function () use ($N) { $x = 0; for ($i = 0; $i < $N; $i++) { $x ^= mt_rand(); } });
best('mt_rand(1, 100)', $N, function () use ($N) { $x = 0; for ($i = 0; $i < $N; $i++) { $x ^= mt_rand(1, 100); } });
best('mt_rand(0, PHP_INT_MAX)', $N, function () use ($N) { $x = 0; for ($i = 0; $i < $N; $i++) { $x ^= mt_rand(0, PHP_INT_MAX); } });
foreach (['Mt19937' => new Mt19937(42), 'Pcg' => new PcgOneseq128XslRr64(42), 'Xoshiro' => new Xoshiro256StarStar(42)] as $name => $e) {
    $r = new Randomizer($e);
    best("$name getInt(1, 100)", $N, function () use ($N, $r) { $x = 0; for ($i = 0; $i < $N; $i++) { $x ^= $r->getInt(1, 100); } });
    best("$name nextInt()", $N, function () use ($N, $r) { $x = 0; for ($i = 0; $i < $N; $i++) { $x ^= $r->nextInt(); } });
    best("$name nextFloat()", $N, function () use ($N, $r) { $x = 0.0; for ($i = 0; $i < $N; $i++) { $x += $r->nextFloat(); } });
    best("$name getFloat(0, 1)", $N, function () use ($N, $r) { $x = 0.0; for ($i = 0; $i < $N; $i++) { $x += $r->getFloat(0, 1); } });
    best("$name getBytes(100 MiB) /byte", 100 << 20, function () use ($r) { $r->getBytes(100 << 20); });
}
$r = new Randomizer(new Secure());
best('Secure getBytes(100 MiB) /byte', 100 << 20, function () use ($r) { $r->getBytes(100 << 20); });
best('random_int(1, 100)', $N / 10, function () use ($N) { $x = 0; for ($i = 0; $i < $N / 10; $i++) { $x ^= random_int(1, 100); } });
$s = str_repeat("abcdefghij", 1_000_000);
best('str_shuffle(10 MB) /byte', strlen($s), function () use ($s) { str_shuffle($s); });
$a = range(0, 999_999);
best('shuffle(1e6 ints) /element', count($a), function () use ($a) { shuffle($a); });
