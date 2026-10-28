<?php
// Records mb_* string operations on sample inputs: one JSON array per line,
// [function, [arguments], result]. Strings are {"s": text} when printable
// ASCII, {"h": hex} otherwise; exceptions
// {"e": class, "m": message}. "subst" lines set mb_substitute_character().
// Usage: php mb_ops.php > mb_ops.jsonl
error_reporting(E_ALL & ~E_DEPRECATED);
ini_set('display_errors', '0');

function enc($v) {
    if (is_string($v)) return preg_match('/^[\x20-\x7e]*$/', $v) ? ['s' => $v] : ['h' => bin2hex($v)];
    if (is_array($v)) return array_map('enc', $v);
    return $v;
}
function rec($f, ...$args) {
    try {
        $r = enc($f(...$args));
    } catch (Throwable $e) {
        $r = ['e' => get_class($e), 'm' => $e->getMessage()];
    }
    echo json_encode([$f, enc($args), $r], JSON_UNESCAPED_SLASHES), "\n";
}
function subst($v) {
    mb_substitute_character($v);
    echo json_encode(['subst', [$v], null]), "\n";
}

$utf8 = ["", "a", "abc", "héllo wörld", "日本語テキストです", "a\xffb\xe3\x81", "ΣΑΣ ΣΑΣ. Σ ΑΣ'Σ",
    "ǆa o'neil x-y ŉ 1st 2ND x1y", "İstanbul ıi", "Straße ﬁ ǰ", "  \u{3000}x\u{a0} \n", "🐘a🐘b",
    "\xed\xa0\x80z", "aAbBaAbB", "ﾊﾟﾝｶﾞ", "ǅungla", "x\u{301}y", "ΌΣΟΣ ἈΣ", "ab\x00cd", "αβγ ΑΒΓ σς"];
$others = [
    ['SJIS', "\x82\xa0\x82\xa2abc\x81\x40\xb1"], ['SJIS', "\x82\xa0\x81"], ['EUC-JP', "\xa4\xa2\xa4\xa4abc\x8e\xb1\x8f\xb0\xa1"],
    ['UTF-16BE', "\x00a\x30\x42\xd8\x3d\xde\x00\x00b"], ['UTF-16LE', "a\x00\x42\x30\x3d\xd8"], ['UCS-2', "\x00a\x30\x42\x00"],
    ['UTF-32', "\x00\x00\x00a\x00\x01\xf4\x18"], ['ISO-2022-JP', "\x1b\$B\$\"\$\$\x1b(Babc"], ['8bit', "a\x80\xff"],
    ['ASCII', "ab\xffc"], ['ISO-8859-1', "h\xe9llo"], ['UTF-7', "a+ZeVnLIqe-b"], ['GB18030', "\x81\x30\x81\x30ab\xb0\xa1"],
    ['ISO-8859-7', "\xd3\xc1\xd3 \xd3"], ['Windows-1252', "\x80abc\x9f"], ['HZ', "a~{VP~}b"],
];

foreach ([0x3F, 0x25, "none", "long", "entity", 0x3013] as $sm) {
    subst($sm);
    foreach (["a\xffb\xe3\x81", "日本\xff", "\xed\xa0\x80"] as $s) {
        rec('mb_substr', $s, 1, null, 'UTF-8');
        rec('mb_strtoupper', $s, 'UTF-8');
        rec('mb_str_split', $s, 1, 'UTF-8');
        rec('mb_scrub', $s, 'UTF-8');
        rec('mb_convert_encoding', $s, 'SJIS', 'UTF-8');
        rec('mb_strimwidth', $s, 0, 3, '…', 'UTF-8');
        rec('mb_str_pad', $s, 8, '-', STR_PAD_BOTH, 'UTF-8');
        rec('mb_trim', $s, null, 'UTF-8');
        rec('mb_ucfirst', $s, 'UTF-8');
    }
}
subst(0x3F);

$all = [];
foreach ($utf8 as $s) $all[] = ['UTF-8', $s];
foreach ($others as $o) $all[] = $o;

foreach ($all as [$e, $s]) {
    rec('mb_strlen', $s, $e);
    rec('mb_strwidth', $s, $e);
    foreach ([0, 1, 2, -1, -3, 5, 100, -100] as $st) {
        foreach ([null, 0, 1, 2, -1, -2, 100] as $ln) {
            rec('mb_substr', $s, $st, $ln, $e);
            rec('mb_strcut', $s, $st, $ln, $e);
        }
    }
    foreach ([1, 2, 3] as $n) rec('mb_str_split', $s, $n, $e);
    foreach ([0, 1, 2, 3, 4, 5, 6, 7] as $m) rec('mb_convert_case', $s, $m, $e);
    rec('mb_strtoupper', $s, $e);
    rec('mb_strtolower', $s, $e);
    rec('mb_ucfirst', $s, $e);
    rec('mb_lcfirst', $s, $e);
    foreach ([[0, 5], [0, 3], [1, 4], [-3, 10], [0, 0], [2, -1], [100, 3], [-100, 2]] as [$st, $w]) {
        rec('mb_strimwidth', $s, $st, $w, mb_convert_encoding('..', $e, 'UTF-8'), $e);
        rec('mb_strimwidth', $s, $st, $w, '', $e);
    }
    foreach ([[10, ' ', STR_PAD_RIGHT], [12, 'ab', STR_PAD_LEFT], [11, 'xyz', STR_PAD_BOTH], [2, '-', STR_PAD_LEFT], [10, '', STR_PAD_LEFT], [10, '-', 5]] as [$l, $p, $t]) {
        rec('mb_str_pad', $s, $l, mb_convert_encoding($p, $e, 'UTF-8'), $t, $e);
    }
    rec('mb_trim', $s, null, $e);
    rec('mb_ltrim', $s, null, $e);
    rec('mb_rtrim', $s, null, $e);
    rec('mb_trim', $s, mb_convert_encoding('a ', $e, 'UTF-8'), $e);
    rec('mb_ord', $s, $e);
    rec('mb_check_encoding', $s, $e);
    rec('mb_scrub', $s, $e);
}

// Searches.
$hay = ["abcabcABC", "日本語の日本語", "aaa", "", "héllo HÉLLO", "a\xffb\xffc", "Straße STRASSE", "İi ıI", "ﬁ fi FI", "x"];
$needles = ["", "a", "bc", "BC", "日本", "\xff", "l", "É", "ss", "SS", "ß", "i", "I", "fi", "x", "zz", "abcabcABCd"];
foreach ($hay as $h) {
    foreach ($needles as $n) {
        foreach (['mb_strpos', 'mb_stripos', 'mb_strrpos', 'mb_strripos'] as $f) {
            foreach ([0, 1, 3, -1, -2, -4, 20, -20] as $off) rec($f, $h, $n, $off, 'UTF-8');
        }
        foreach (['mb_strstr', 'mb_stristr', 'mb_strrchr', 'mb_strrichr'] as $f) {
            rec($f, $h, $n, false, 'UTF-8');
            rec($f, $h, $n, true, 'UTF-8');
        }
        rec('mb_substr_count', $h, $n, 'UTF-8');
    }
}
foreach ([['SJIS', "\x82\xa0a\x82\xa0b", "\x82\xa0"], ['UTF-16BE', "\x00a\x00b\x00a", "\x00a"], ['EUC-JP', "\xa4\xa2\xa4\xa2", "\xa4"], ['ISO-8859-1', "A\xe9a\xc9", "\xe9"]] as [$e, $h, $n]) {
    foreach (['mb_strpos', 'mb_stripos', 'mb_strrpos', 'mb_strripos'] as $f) {
        foreach ([0, 1, -1] as $off) rec($f, $h, $n, $off, $e);
    }
    rec('mb_substr_count', $h, $n, $e);
    rec('mb_strstr', $h, $n, false, $e);
    rec('mb_strrchr', $h, $n, true, $e);
}

// Numeric entities.
$maps = [[0, 0x2FFFF, 0, 0xFFFF], [0x80, 0x10FFFF, 0, 0x1FFFFF], [0, 10, 1000, 0xFFFF], [0, 10, -100000, 0xFFFFFF], [0, 1000, 0, 0x123], [0x41, 0x5A, 0x20, 0xFF]];
foreach (["abc", "héllo €", "日本", "\x00\x01", "a\xffb"] as $s) {
    foreach ($maps as $m) {
        rec('mb_encode_numericentity', $s, $m, 'UTF-8', false);
        rec('mb_encode_numericentity', $s, $m, 'UTF-8', true);
    }
}
foreach (["&#65;&#x42;&#X43;", "&#1001;&#1002;", "&#x3E9;", "&#0;", "&#;&#x;&#", "&#65&#66", "&#123a;", "&#0000000065;", "&#00000000165;", "&#x00000041;", "&#x000000141;", "&#4294967295;", "&#4294967296;", "&#x110000;", "&&#65,", "&#&#x32;", "&#97;&#98;", "a&#xDC00;b", "&#9999;"] as $s) {
    foreach ($maps as $m) rec('mb_decode_numericentity', $s, $m, 'UTF-8');
}

// Kana.
$ks = ["ｶﾞｷﾞｸﾞ ﾊﾟﾋﾟ ｳﾞ ﾜ ｦ ｰ ｡｢｣､･ﾞﾟ", "がぎぐ ぱぴ ゔ ゎ を ー 、。「」゛゜・", "ガギグ パピ ヴ ヮ ヲ ヰヱ ヵヶ ヷヸヹヺ ヽヾ", "ＡＢＣａｂｃ０１２！＂＃＇＼～　−", "ABCabc012!\"#'\\~ ¥‾", "‘’“”", "ｱｲｳｴｵ"];
foreach ($ks as $s) {
    foreach (["r", "R", "n", "N", "a", "A", "s", "S", "k", "K", "h", "H", "c", "C", "V", "KV", "HV", "m", "M", "rnaskh", "KVC", "KVc", "AKV", "HVa", "", "sS", "Z", "AS", "kH", "hK"] as $m) {
        rec('mb_convert_kana', $s, $m, 'UTF-8');
    }
}
foreach ([['SJIS', "\xb6\xde\xca\xdf"], ['EUC-JP', "\x8e\xb6\x8e\xde"], ['JIS', "\xb9\xde\xde"], ['ASCII', "abc\x00abc"]] as [$e, $s]) {
    foreach (["KV", "HV", "K", "c", "A"] as $m) rec('mb_convert_kana', $s, $m, $e);
}

// MIME headers.
mb_internal_encoding('UTF-8');
foreach (["This is an English string. 0123456789", "日本語テキストです。", "Αυτό είναι ελληνικό κείμενο. 0123456789.", "abc dé fgh", "?", "=", "_", " ", "   ", "ab  ab  ", "o \x00", "hello wörld foo bar baz qux quux corge grault garply waldo fred plugh xyzzy thud", "x"] as $s) {
    foreach (['UTF-8', 'ISO-8859-1', 'SJIS', 'ISO-2022-JP', 'ASCII'] as $cs) {
        foreach (['B', 'Q'] as $t) {
            rec('mb_encode_mimeheader', $s, $cs, $t);
            rec('mb_encode_mimeheader', $s, $cs, $t, "\n", 10);
        }
    }
}
foreach (["=?UTF-8?B?5pel5pys6Kqe?=", "=?UTF-8?Q?abc_def?=", "_=?UTF-8?Q?abc_def?=_", "=?UTF-8?Q?=5F?=", "a =?UTF-8?B?w6k=?= =?UTF-8?B?w6k=?= b",
    "=?UTF-8?B?w6k=?=\r\n =?UTF-8?B?w6k=?=", "=?ISO-8859-1?Q?caf=E9?=", "=?bogus?Q?x?=", "=?UTF-8?X?x?=", "=?UTF-8?Q?unterminated", "plain\r\n text", "a\r\n\tb", "=?Shift_JIS?B?gqA=?=x", "=?UTF-8?b?w6k=?=", "=?utf-8?q?=C3=A9?="] as $s) {
    rec('mb_decode_mimeheader', $s);
}

// Detection.
$det = ["", "abc", "\xDD\x92", "\xD6\x8A\x8A", "\xEF\xBB\xBF", "\xFE\xFF", "\xFF\xFE", "h\xe9llo", "héllo", "\x82\xa0\x82\xa2", "\xa4\xa2\xa4\xa4", "\x1b\$B\$\"\x1b(B", "Zażółć gęślą jaźń.", "\x80\x81", "abc + abc"];
$lists = [['ASCII', 'UTF-8'], ['UTF-8', 'ASCII'], ['UTF-8', 'ISO-8859-1'], ['ISO-8859-1', 'UTF-8'], ['UTF-8', 'SJIS', 'EUC-JP', 'JIS'], ['UTF-8', 'UTF-16BE', 'UTF-16LE'], ['UTF-8', 'Windows-1252', 'ISO-8859-1'], ['SJIS', 'EUC-JP'], ['UTF-7', 'UTF-8'], ['UTF-8', 'Windows-1251', 'KOI8-R']];
foreach ($det as $s) {
    foreach ($lists as $l) {
        rec('mb_detect_encoding', $s, $l, false);
        rec('mb_detect_encoding', $s, $l, true);
    }
}

// mb_chr / mb_ord.
foreach ([0x41, 0xE9, 0x3042, 0x1F418, 0xD800, 0x110000, 0x80, 0xFF61, 0x20AC] as $cp) {
    foreach (['UTF-8', 'SJIS', 'ISO-8859-1', 'UTF-16BE', 'ASCII', 'EUC-JP', 'Windows-1252'] as $e) rec('mb_chr', $cp, $e);
}
