<?php
// Deterministic test inputs shared by the fixture drivers. tests/common/mod.rs
// builds the same byte strings in Rust, so the fixture only records outputs.

function lcg_bytes(int $seed, int $n): string {
    $x = $seed & 0x7fffffff;
    $out = '';
    for ($i = 0; $i < $n; $i++) {
        $x = ($x * 1103515245 + 12345) & 0x7fffffff;
        $out .= chr(($x >> 16) & 0xff);
    }
    return $out;
}

class Lcg {
    public int $x;
    function __construct(int $seed) { $this->x = $seed & 0x7fffffff; }
    function next(): int {
        $this->x = ($this->x * 1103515245 + 12345) & 0x7fffffff;
        return $this->x >> 16; // 0..32767
    }
}

const WORDS = ['the', 'of', 'and', 'a', 'to', 'in', 'is', 'you', 'that', 'it',
    'he', 'was', 'for', 'on', 'are', 'as', 'with', 'his', 'they', 'I', 'at',
    'be', 'this', 'have', 'from', 'or', 'one', 'had', 'by', 'word', 'but',
    'not', 'what', 'all', 'were', 'we', 'when', 'your', 'can', 'said',
    'there', 'use', 'an', 'each', 'which', 'she', 'do', 'how', 'their', 'if',
    'compression', 'deflate', 'window', 'huffman', 'literal', 'distance',
    'PHP', 'zlib', 'stream', 'buffer'];

function text_data(int $seed, int $n): string {
    $r = new Lcg($seed);
    $out = '';
    while (strlen($out) < $n) {
        $w = WORDS[$r->next() % count(WORDS)];
        $k = $r->next() % 16;
        $sep = $k == 0 ? ".\n" : ($k == 1 ? ', ' : ' ');
        $out .= $w . $sep;
    }
    return substr($out, 0, $n);
}

function mixed_data(int $seed, int $n): string {
    $r = new Lcg($seed);
    $out = '';
    while (strlen($out) < $n) {
        $len = 1 + $r->next() % 3000;
        if ($r->next() % 3 == 0) {
            $out .= lcg_bytes($r->next(), $len);
        } else {
            $out .= text_data($r->next(), $len);
        }
    }
    return substr($out, 0, $n);
}

function runs_data(int $seed, int $n): string {
    $r = new Lcg($seed);
    $out = '';
    while (strlen($out) < $n) {
        $out .= str_repeat(chr($r->next() & 0xff), 1 + $r->next() % 600);
    }
    return substr($out, 0, $n);
}

function alpha_data(int $seed, int $n, string $alpha): string {
    $r = new Lcg($seed);
    $out = '';
    $k = strlen($alpha);
    for ($i = 0; $i < $n; $i++) $out .= $alpha[$r->next() % $k];
    return $out;
}

function fib_data(int $seed): string {
    // Byte i appears fib(i) times, shuffled: skewed frequencies that need
    // Huffman codes longer than 15 bits before length limiting.
    $a = 1; $b = 1; $s = '';
    for ($i = 0; $i < 25; $i++) { $s .= str_repeat(chr($i + 65), $a); [$a, $b] = [$b, $a + $b]; }
    $r = new Lcg($seed);
    for ($i = strlen($s) - 1; $i > 0; $i--) {
        $j = (($r->next() << 15) | $r->next()) % ($i + 1);
        $t = $s[$i]; $s[$i] = $s[$j]; $s[$j] = $t;
    }
    return $s;
}

function numbers_data(int $n): string {
    $s = '';
    for ($i = 0; $i < $n; ++$i) $s .= $i . ' ';
    return $s;
}

function struct_data(int $seed, int $n): string {
    $r = new Lcg($seed);
    $out = '';
    while (strlen($out) < $n) $out .= pack('V', $r->next() % 1000) . pack('v', strlen($out) & 0xffff);
    return substr($out, 0, $n);
}

function inputs(): array {
    $all = '';
    for ($i = 0; $i < 256; $i++) $all .= chr($i);
    $p1 = lcg_bytes(7, 32505);
    $p2 = lcg_bytes(8, 32507);
    $p3 = text_data(9, 32700);
    return [
        'empty' => '',
        'a' => 'a',
        'hello' => 'hello world',
        'a15' => str_repeat('a', 15),
        'abc100' => str_repeat('abc', 100),
        'allbytes' => $all,
        'zeros100k' => str_repeat("\0", 100000),
        'rand1000' => lcg_bytes(1, 1000),
        'rand70000' => lcg_bytes(2, 70000),
        'text5000' => text_data(3, 5000),
        'text300k' => text_data(4, 300000),
        'mixed400k' => mixed_data(5, 400000),
        'numbers' => numbers_data(30000),
        'period32505' => $p1 . $p1 . $p1,
        'period32507' => $p2 . $p2 . $p2 . substr($p2, 0, 1000),
        'period_text' => $p3 . $p3 . substr($p3, 0, 20000),
        'runs200k' => runs_data(10, 200000),
        'alpha4' => alpha_data(11, 100000, 'ACGT'),
        'alpha2' => alpha_data(12, 50000, 'ab'),
        'textend' => text_data(13, 100000) . str_repeat('abcabc', 50),
        'struct' => struct_data(14, 80000),
        'x258' => str_repeat('x', 258),
        'x259' => str_repeat('x', 259),
        'x262' => str_repeat('x', 262),
        'x263' => str_repeat('x', 263),
        'x1000' => str_repeat('x', 1000),
        'fib' => fib_data(15),
        'text70k' => text_data(16, 70000),
    ];
}

function out_repr(string $s): string {
    if (strlen($s) <= 400) return bin2hex($s);
    return strlen($s) . ':' . md5($s);
}
