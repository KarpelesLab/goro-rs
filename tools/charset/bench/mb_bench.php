<?php
// Times mbstring operations on 1 MiB inputs (best of 20 rounds), in the same
// cases as crates/goro-charset/tests/bench.rs.
// Usage: php -d memory_limit=-1 mb_bench.php

// Whole repetitions of $unit, at least 1 MiB.
function input(string $unit): string {
    return str_repeat($unit, intdiv((1 << 20) + strlen($unit) - 1, strlen($unit)));
}
$in = [];
$in['ascii'] = input("The quick brown fox jumps over the lazy dog. 0123456789\n");
$in['mixed'] = input("Hello, world! Déjà vu — naïve café. 日本語のテキスト。Ελληνικά κείμενα. ");
$in['cjk'] = input("日本語のテキストです。漢字とかなカナ、");
$in['sjis'] = mb_convert_encoding($in['cjk'], 'SJIS', 'UTF-8');
$in['latin1'] = mb_convert_encoding(input("Déjà vu, naïve café, Ærøskøbing. "), 'ISO-8859-1', 'UTF-8');
$in['utf16'] = mb_convert_encoding($in['mixed'], 'UTF-16LE', 'UTF-8');
$in['bad'] = input("abc\xffdef\xe3\x81ghi");
$cases = [
    'check_encoding UTF-8 ascii' => fn($i) => mb_check_encoding($i['ascii'], 'UTF-8'),
    'check_encoding UTF-8 mixed' => fn($i) => mb_check_encoding($i['mixed'], 'UTF-8'),
    'strlen UTF-8 mixed' => fn($i) => mb_strlen($i['mixed'], 'UTF-8'),
    'substr UTF-8 mixed (middle 1000)' => fn($i) => mb_substr($i['mixed'], 300000, 1000, 'UTF-8'),
    'strpos UTF-8 mixed (miss)' => fn($i) => mb_strpos($i['mixed'], 'zzz', 0, 'UTF-8'),
    'strpos UTF-8 mixed (near end)' => fn($i) => mb_strpos($i['mixed'], 'κείμενα. ', 640000, 'UTF-8'),
    'convert UTF-8 -> UTF-16LE' => fn($i) => mb_convert_encoding($i['mixed'], 'UTF-16LE', 'UTF-8'),
    'convert UTF-16LE -> UTF-8' => fn($i) => mb_convert_encoding($i['utf16'], 'UTF-8', 'UTF-16LE'),
    'convert SJIS -> UTF-8' => fn($i) => mb_convert_encoding($i['sjis'], 'UTF-8', 'SJIS'),
    'convert UTF-8 -> SJIS' => fn($i) => mb_convert_encoding($i['cjk'], 'SJIS', 'UTF-8'),
    'convert ISO-8859-1 -> UTF-8' => fn($i) => mb_convert_encoding($i['latin1'], 'UTF-8', 'ISO-8859-1'),
    'scrub UTF-8 (errors)' => fn($i) => mb_scrub($i['bad'], 'UTF-8'),
    'strtoupper UTF-8 ascii' => fn($i) => mb_strtoupper($i['ascii'], 'UTF-8'),
    'strtoupper UTF-8 mixed' => fn($i) => mb_strtoupper($i['mixed'], 'UTF-8'),
    'strwidth UTF-8 mixed' => fn($i) => mb_strwidth($i['mixed'], 'UTF-8'),
    'detect [ASCII,UTF-8,SJIS,EUC-JP] mixed' => fn($i) => mb_detect_encoding($i['mixed'], ['ASCII', 'UTF-8', 'SJIS', 'EUC-JP']),
    'detect [UTF-8,SJIS,EUC-JP] sjis' => fn($i) => mb_detect_encoding($i['sjis'], ['UTF-8', 'SJIS', 'EUC-JP']),
];
foreach ($cases as $name => $f) {
    $best = INF;
    for ($r = 0; $r < 20; $r++) {
        // Fresh copies: PHP caches a string's UTF-8 validity.
        $i = array_map(fn($s) => strrev(strrev($s)), $in);
        $t = hrtime(true);
        $v = $f($i);
        $best = min($best, hrtime(true) - $t);
    }
    $sig = is_string($v) ? (strlen($v) < 20 ? $v : strlen($v) . ':' . crc32($v)) : var_export($v, true);
    printf("%-40s %8.3f ms  %s\n", $name, $best / 1e6, $sig);
}
