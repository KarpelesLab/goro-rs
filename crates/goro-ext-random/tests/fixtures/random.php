<?php
// Driver for random.txt (one PHP 8.5 run). Each line is
//   <engine> | <operation> | <result> ; <state after>
// "state after" is the hex of the engine's next generate() for native
// engines and the number of generate() calls for user engines, so the
// fixture also pins how much randomness each operation consumes.
// Floats are written as the hex of their IEEE-754 bits.
error_reporting(E_ALL & ~E_DEPRECATED);

use Random\Engine\{Mt19937, PcgOneseq128XslRr64, Xoshiro256StarStar};
use Random\{Randomizer, IntervalBoundary};

// A user engine returning lens[i % count] bytes per call, cut from a
// Xoshiro256** stream (whole outputs concatenated, then truncated).
final class LenEngine implements Random\Engine {
    private Xoshiro256StarStar $x;
    private int $i = 0;
    public int $calls = 0;
    public function __construct(int $seed, private array $lens) { $this->x = new Xoshiro256StarStar($seed); }
    public function generate(): string {
        $this->calls++;
        $l = $this->lens[$this->i++ % count($this->lens)];
        $s = '';
        while (strlen($s) < $l) $s .= $this->x->generate();
        return substr($s, 0, $l);
    }
}
// A user engine forwarding to a native Mt19937.
final class MtUser implements Random\Engine {
    private Mt19937 $m;
    public int $calls = 0;
    public function __construct(int $seed, int $mode) { $this->m = new Mt19937($seed, $mode); }
    public function generate(): string { $this->calls++; return $this->m->generate(); }
}

function fh(float $f): string { return bin2hex(pack('E', $f)); }
function src(int $n): string { $s = ''; for ($i = 0; $i < $n; $i++) $s .= chr(($i * 37 + 11) % 256); return $s; }

$engines = [];
foreach ([0, 1, 1234, -1, PHP_INT_MAX, 4294967296 + 5] as $s) {
    foreach ([0, 1] as $m) $engines["mt $s $m"] = fn() => new Mt19937($s, $m);
}
foreach ([0, 1, -1, 1234, PHP_INT_MIN] as $s) $engines["pcg $s"] = fn() => new PcgOneseq128XslRr64($s);
foreach (['0102030405060708090a0b0c0d0e0f10', 'ffffffffffffffffffffffffffffffff'] as $h) $engines["pcgs $h"] = fn() => new PcgOneseq128XslRr64(hex2bin($h));
foreach ([0, 1, 1234, -1, PHP_INT_MAX] as $s) $engines["xo $s"] = fn() => new Xoshiro256StarStar($s);
foreach (['0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f20', '0000000000000000000000000000000000000000000000000000000000000001'] as $h) $engines["xos $h"] = fn() => new Xoshiro256StarStar(hex2bin($h));
foreach (['1', '3', '4', '5', '8', '12', '1,9,4', '2,7'] as $l) $engines["user 7 $l"] = fn() => new LenEngine(7, array_map('intval', explode(',', $l)));
foreach ([0, 1] as $m) $engines["mtu 1234 $m"] = fn() => new MtUser(1234, $m);

$ranges = [
    [0, 0], [0, 1], [0, 9], [1, 6], [-5, 5], [0, 255], [0, 256], [0, 1000], [0, 0x66666665],
    [0, 0x7fffffff], [0, 0xfffffffe], [0, 0xffffffff], [0, 0x100000000], [-2147483648, 2147483647],
    [0, 2 ** 53], [0, 3 * 2 ** 61], [0, PHP_INT_MAX], [PHP_INT_MIN, PHP_INT_MAX], [PHP_INT_MIN, 0],
    [PHP_INT_MIN, PHP_INT_MIN + 1], [PHP_INT_MAX - 1, PHP_INT_MAX], [-1, PHP_INT_MAX], [PHP_INT_MIN, -1],
    [-5000000000, 5000000000], [PHP_INT_MIN, PHP_INT_MIN], [-1000, 1000],
];
$fr = [
    [0.0, 1.0], [-1.0, 1.0], [0.0, 1e-300], [-1.6e308, 1.6e308], [1.99999999999999, 2.00000000000001],
    [-5.5, -1.25], [1e-320, 1e-310], [0.0, 1.5e-323], [-PHP_FLOAT_MAX, PHP_FLOAT_MAX], [100.0, 100.00000000001],
    [-1e10, 3.0], [0.1, 0.2], [-2.0, -1.0], [1.0, 1.0 + 2 * PHP_FLOAT_EPSILON], [-0.0, 0.0], [-3.0, 1e300],
    [-1e300, 3.0], [5.0, 5.0], [1e15, 1e16], [-1e-310, 1e-310], [0.0, PHP_FLOAT_MAX], [-PHP_FLOAT_MAX, 0.0],
];
$bounds = ['CC' => IntervalBoundary::ClosedClosed, 'CO' => IntervalBoundary::ClosedOpen,
    'OC' => IntervalBoundary::OpenClosed, 'OO' => IntervalBoundary::OpenOpen];

foreach ($engines as $name => $mk) {
    $native = !str_starts_with($name, 'user') && !str_starts_with($name, 'mtu');
    $run = function (string $op, callable $f) use ($name, $mk, $native) {
        $e = $mk();
        $r = new Randomizer($e);
        try {
            $res = $f($r, $e);
        } catch (Throwable $t) {
            $res = 'E ' . get_class($t) . ': ' . $t->getMessage();
        }
        $after = $native ? bin2hex($e->generate()) : $e->calls;
        echo "$name | $op | $res ; $after\n";
    };
    if ($native) {
        $run('gen', function ($r, $e) { $o = []; for ($i = 0; $i < 16; $i++) $o[] = bin2hex($e->generate()); return implode(' ', $o); });
    }
    $run('nextInt', function ($r) { $o = []; for ($i = 0; $i < 8; $i++) $o[] = $r->nextInt(); return implode(' ', $o); });
    $run('nextFloat', function ($r) { $o = []; for ($i = 0; $i < 8; $i++) $o[] = fh($r->nextFloat()); return implode(' ', $o); });
    foreach ($ranges as [$a, $b]) {
        $run("getInt $a $b", function ($r) use ($a, $b) { $o = []; for ($i = 0; $i < 10; $i++) $o[] = $r->getInt($a, $b); return implode(' ', $o); });
    }
    foreach ($fr as [$a, $b]) {
        foreach ($bounds as $bn => $bv) {
            $run('getFloat ' . fh($a) . ' ' . fh($b) . " $bn", function ($r) use ($a, $b, $bv) { $o = []; for ($i = 0; $i < 6; $i++) $o[] = fh($r->getFloat($a, $b, $bv)); return implode(' ', $o); });
        }
    }
    foreach ([1, 3, 4, 5, 7, 8, 9, 17, 100] as $n) {
        $run("getBytes $n", fn($r) => bin2hex($r->getBytes($n)));
    }
    foreach ([[1, 10], [2, 20], [3, 20], [5, 20], [17, 20], [64, 20], [100, 30], [128, 20], [200, 20], [255, 20], [256, 20], [257, 20], [300, 20], [1000, 10], [70000, 5]] as [$k, $n]) {
        $run("gbfs $k $n", fn($r) => bin2hex($r->getBytesFromString(src($k), $n)));
    }
    foreach ([0, 1, 2, 5, 20, 300] as $n) {
        $run("shuffleBytes $n", fn($r) => bin2hex($r->shuffleBytes(src($n))));
    }
    foreach ([0, 1, 2, 3, 10, 50] as $n) {
        $run("shuffleArray $n", fn($r) => implode(' ', $r->shuffleArray($n ? range(0, $n - 1) : [])));
    }
    foreach ([[1, 1], [5, 1], [5, 2], [5, 3], [5, 5], [10, 4], [10, 5], [10, 6], [10, 9], [100, 1], [100, 7], [100, 60], [1000, 10], [3, 2], [2, 1], [2, 2]] as [$n, $k]) {
        $run("pick $n $k", fn($r) => implode(' ', $r->pickArrayKeys(range(0, $n - 1), $k)));
    }
}

// Legacy functions on the global Mt19937: "legacy SEED MODE | op | result ; next mt_rand()".
foreach ([0, 1, 1234, -1, 4294967296 + 5] as $s) {
    foreach ([0, 1] as $m) {
        $leg = function (string $op, callable $f) use ($s, $m) {
            mt_srand($s, $m);
            $res = $f();
            echo "legacy $s $m | $op | $res ; ", mt_rand(), "\n";
        };
        foreach ([0, 1, 2, 3, 10, 50] as $n) {
            $leg("shuffle $n", function () use ($n) { $a = $n ? range(0, $n - 1) : []; shuffle($a); return implode(' ', $a); });
            $leg("str_shuffle $n", fn() => bin2hex(str_shuffle(src($n))));
        }
        foreach ([[1, 1], [5, 1], [5, 2], [5, 3], [5, 5], [10, 4], [10, 6], [100, 1], [100, 7], [100, 60], [3, 2]] as [$n, $k]) {
            $leg("array_rand $n $k", function () use ($n, $k) { $r = array_rand(range(0, $n - 1), $k); return is_array($r) ? implode(' ', $r) : $r; });
        }
        $leg('getrand', fn() => mt_getrandmax() . ' ' . getrandmax());
    }
}
