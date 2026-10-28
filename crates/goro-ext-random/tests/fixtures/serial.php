<?php
// Driver for serial.txt (one PHP 8.5 run): engine state serialization.
//   ser <engine> | <state items>          __serialize()[1] after seeding
//   jump <engine> <op> | <state items>    state after jump()/jumpLong()
//   unser <class> <items> | ok <next generate() hex> | E <class>: <message>
// State items: s:<string> for strings, i:<n> for ints. In unser lines the
// tokens are s:<hex string>, i:<int>, n (null), f (float 1.5), a (empty
// array), and "-" for a missing key (the key is skipped); "~" in a string is a space.
error_reporting(E_ALL & ~E_DEPRECATED);

use Random\Engine\{Mt19937, PcgOneseq128XslRr64, Xoshiro256StarStar};

// unserialize() of an object whose __unserialize() data is $data.
function unser(string $class, array $data): object {
    $a = serialize($data);
    $o = unserialize('O:' . strlen($class) . ':"' . $class . '"' . substr($a, 1));
    if (!is_object($o)) throw new Exception('unserialize() returned ' . var_export($o, true));
    return $o;
}

function items(array $a): string {
    $o = [];
    foreach ($a as $k => $v) $o[] = (is_int($v) ? "i:$v" : "s:$v");
    return implode(' ', $o);
}

foreach ([0, 1, 1234, -1, PHP_INT_MAX, PHP_INT_MIN, 4294967296 + 5, 12345678] as $s) {
    foreach ([0, 1] as $m) {
        echo "ser mt $s $m | ", items((new Mt19937($s, $m))->__serialize()[1]), "\n";
    }
    echo "ser pcg $s | ", items((new PcgOneseq128XslRr64($s))->__serialize()[1]), "\n";
    echo "ser xo $s | ", items((new Xoshiro256StarStar($s))->__serialize()[1]), "\n";
}
foreach (['00000000000000000000000000000000', '0102030405060708090a0b0c0d0e0f10', 'ffffffffffffffffffffffffffffffff', '00000000000000000000000000000001'] as $h) {
    echo "ser pcgs $h | ", items((new PcgOneseq128XslRr64(hex2bin($h)))->__serialize()[1]), "\n";
}
foreach (['0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f20', '0000000000000000000000000000000000000000000000000000000000000001', str_repeat('ff', 32)] as $h) {
    echo "ser xos $h | ", items((new Xoshiro256StarStar(hex2bin($h)))->__serialize()[1]), "\n";
}
// The MT state while generating: index advances, reload at 624.
foreach ([1, 623, 624, 625, 1300] as $n) {
    $e = new Mt19937(99);
    for ($i = 0; $i < $n; $i++) $e->generate();
    echo "ser mtgen 99 $n | ", items($e->__serialize()[1]), "\n";
}
foreach ([0, 1, 2, 3, 1000, 1234567, PHP_INT_MAX, 2 ** 40 + 3] as $j) {
    $e = new PcgOneseq128XslRr64(1234);
    $e->jump($j);
    echo "jump pcg 1234 $j | ", items($e->__serialize()[1]), "\n";
}
foreach ([0, 1, 1234] as $s) {
    foreach (['jump', 'jumpLong'] as $op) {
        $e = new Xoshiro256StarStar($s);
        $e->$op();
        echo "jump xo $s $op | ", items($e->__serialize()[1]), "\n";
    }
}

// Diagnostics while unserializing show up inline.
set_error_handler(function ($no, $str) { echo "[diag $no: $str] "; return true; });
$mt = (new Mt19937(1234))->__serialize()[1];
$mtTok = array_map(fn($v) => is_int($v) ? "i:$v" : "s:$v", $mt);
$classes = ['mt' => Mt19937::class, 'pcg' => PcgOneseq128XslRr64::class, 'xo' => Xoshiro256StarStar::class];
$cases = [
    ['pcg', 's:c6d571c37c41a8d1 s:345e7e82265d6e27'],
    ['pcg', 's:C6D571C37C41A8D1 s:345E7E82265D6E27'],
    ['pcg', 's:c6d571c37c41a8d1'],
    ['pcg', 's:c6d571c37c41a8d1 s:345e7e82265d6e27 s:00'],
    ['pcg', 's:c6d571c37c41a8d1 s:345e7e82265d6e2'],
    ['pcg', 's:c6d571c37c41a8d1 s:345e7e82265d6e270'],
    ['pcg', 's:c6d571c37c41a8d1 s:345e7e82265d6e2g'],
    ['pcg', 's:c6d571c37c41a8d1 i:5'],
    ['pcg', 's:c6d571c37c41a8d1 n'],
    ['pcg', '- s:345e7e82265d6e27 s:c6d571c37c41a8d1'],
    ['pcg', 's:0000000000000000 s:0000000000000000'],
    ['pcg', 's:ffffffffffffffff s:ffffffffffffffff'],
    ['pcg', 's:+6d571c37c41a8d1 s:345e7e82265d6e27'],
    ['pcg', 's:~6d571c37c41a8d1 s:345e7e82265d6e27'],
    ['xo', 's:db1c182f1bf60cbb s:2465f04d36a1c797 s:da25c09be4fabe33 s:33a0d052f241624e'],
    ['xo', 's:0000000000000000 s:0000000000000000 s:0000000000000000 s:0000000000000000'],
    ['xo', 's:db1c182f1bf60cbb s:2465f04d36a1c797 s:da25c09be4fabe33'],
    ['xo', 's:db1c182f1bf60cbb s:2465f04d36a1c797 s:da25c09be4fabe33 s:33a0d052f241624e n'],
    ['xo', 's:db1c182f1bf60cbb s:2465f04d36a1c797 s:da25c09be4fabe33 i:1'],
    ['xo', 's:db1c182f1bf60cbb s:2465f04d36a1c797 s:da25c09be4fabe33 s:33a0d052f241624g'],
    ['mt', implode(' ', $mtTok)],
    ['mt', implode(' ', array_slice($mtTok, 0, 624)) . ' i:624 i:0'],
    ['mt', implode(' ', array_slice($mtTok, 0, 624)) . ' i:623 i:1'],
    ['mt', implode(' ', array_slice($mtTok, 0, 624)) . ' i:625 i:0'],
    ['mt', implode(' ', array_slice($mtTok, 0, 624)) . ' i:-1 i:0'],
    ['mt', implode(' ', array_slice($mtTok, 0, 624)) . ' s:5 i:0'],
    ['mt', implode(' ', array_slice($mtTok, 0, 624)) . ' i:5 s:0'],
    ['mt', implode(' ', array_slice($mtTok, 0, 624)) . ' i:5 i:2'],
    ['mt', implode(' ', array_slice($mtTok, 0, 624)) . ' i:5 i:-1'],
    ['mt', implode(' ', array_slice($mtTok, 0, 624)) . ' i:5'],
    ['mt', implode(' ', array_slice($mtTok, 0, 624)) . ' i:5 i:0 i:0'],
    ['mt', implode(' ', array_slice($mtTok, 0, 624)) . ' f i:0'],
    ['mt', 's:00000000 ' . implode(' ', array_slice($mtTok, 1, 625))],
    ['mt', 's:5aa6b98 ' . implode(' ', array_slice($mtTok, 1, 625))],
    ['mt', 's:5aa6b9860 ' . implode(' ', array_slice($mtTok, 1, 625))],
    ['mt', 'i:5 ' . implode(' ', array_slice($mtTok, 1, 625))],
    ['mt', 's:5AA6B986 ' . implode(' ', array_slice($mtTok, 1, 625))],
    ['mt', '- ' . implode(' ', array_slice($mtTok, 1, 625)) . ' s:5aa6b986'],
];
foreach ($cases as [$c, $tok]) {
    $arr = [];
    $k = 0;
    foreach (explode(' ', $tok) as $t) {
        if ($t === '-') { $k++; continue; }
        $arr[$k++] = match (true) {
            str_starts_with($t, 's:') => str_replace('~', ' ', substr($t, 2)),
            str_starts_with($t, 'i:') => (int) substr($t, 2),
            $t === 'n' => null,
            $t === 'f' => 1.5,
            $t === 'a' => [],
        };
    }
    try {
        $e = unser($classes[$c], [[], $arr]);
        $res = 'ok ' . bin2hex($e->generate());
    } catch (Throwable $t) {
        $res = 'E ' . get_class($t) . ': ' . $t->getMessage();
    }
    echo "unser $c $tok | $res\n";
}
// Outer shape of the __unserialize() data (documented, checked by hand).
$good = (new Xoshiro256StarStar(1))->__serialize();
foreach ([
    'good' => $good,
    'props-not-array' => [1, $good[1]],
    'state-not-array' => [[], 'x'],
    'extra' => [[], $good[1], null],
    'one' => [[]],
    'empty' => [],
    'swapped-keys' => [1 => $good[1], 0 => []],
    'string-keys' => ['a' => [], 'b' => $good[1]],
    'props' => [['foo' => 1], $good[1]],
] as $label => $data) {
    try {
        $e = unser(Xoshiro256StarStar::class, $data);
        $res = 'ok ' . bin2hex($e->generate()) . ' ' . json_encode(get_object_vars($e));
    } catch (Throwable $t) {
        $res = 'E ' . get_class($t) . ': ' . $t->getMessage();
    }
    echo "outer $label | $res\n";
}
foreach ([
    'good' => [['engine' => new Xoshiro256StarStar(1)]],
    'extra-prop' => [['engine' => new Xoshiro256StarStar(1), 'foo' => 1]],
    'no-engine' => [[]],
    'engine-not-object' => [['engine' => 5]],
    'engine-not-engine' => [['engine' => new stdClass]],
    'two' => [['engine' => new Xoshiro256StarStar(1)], []],
    'none' => [],
] as $label => $data) {
    try {
        $r = unser(Random\Randomizer::class, $data);
        $res = 'ok ' . get_class($r->engine);
    } catch (Throwable $t) {
        $res = 'E ' . get_class($t) . ': ' . $t->getMessage();
    }
    echo "router $label | $res\n";
}
foreach (['Random\Engine\Secure', 'Random\Randomizer'] as $c) {
    try { echo "serialize $c | ", serialize($c === 'Random\Randomizer' ? new Random\Randomizer(new Random\Engine\Secure) : new $c), "\n"; }
    catch (Throwable $t) { echo "serialize $c | E ", get_class($t), ': ', $t->getMessage(), "\n"; }
}
