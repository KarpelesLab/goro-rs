<?php
// Records mb_detect_encoding() results: one JSON array per line, in the
// format of mb_ops.php (["mb_detect_encoding", [string, list, strict], result]).
// Usage: php mb_detect.php > mb_detect.jsonl
error_reporting(E_ALL & ~E_DEPRECATED);
ini_set('display_errors', '0');

function enc($v) {
    if (is_string($v)) return preg_match('/^[\x20-\x7e]*$/', $v) ? ['s' => $v] : ['h' => bin2hex($v)];
    if (is_array($v)) return array_map('enc', $v);
    return $v;
}
function rec($s, $l, $strict) {
    try {
        $r = enc(mb_detect_encoding($s, $l, $strict));
    } catch (Throwable $e) {
        $r = ['e' => get_class($e), 'm' => $e->getMessage()];
    }
    echo json_encode(['mb_detect_encoding', enc([$s, $l, $strict]), $r], JSON_UNESCAPED_SLASHES), "\n";
}

mt_srand(20251028);
// A copy: PHP 8.5 takes another path for the very array mb_list_encodings()
// returns (observed), which the library does not emulate.
$all = array_map(fn($e) => $e, mb_list_encodings());
$text = array_values(array_filter($all, fn($e) => !in_array($e, ['BASE64', 'UUENCODE', 'HTML-ENTITIES', 'Quoted-Printable', '7bit', '8bit', 'pass'])));

// 1. Sample texts in every encoding that can represent them.
$samples = [
    "Hello, world! This is plain ASCII text.", "Café crème, naïve façade — déjà vu.", "Zażółć gęślą jaźń. Łódź",
    "Съешь же ещё этих мягких французских булок", "Ξεσκεπάζω την ψυχοφθόρα βδελυγμία", "日本語のテキストです。漢字とかなカナ",
    "ｶﾀｶﾅ ﾃｷｽﾄ", "中文文本，简体字。", "繁體中文的文字。", "한국어 텍스트입니다", "שלום עולם", "مرحبا بالعالم",
    "สวัสดีชาวโลก", "Emoji 🐘 and ❤", "Grüße aus Köln, Straße 5", "Ärger über Öl", "tab\tand\nnewline\r\n",
    "a+b=c (x-y) [z]", "€100 – £50", "İstanbul’da ılık", "Português: ação, coração", "Māori: whānau",
    "①②③ ㈱ №", "～〜−", "Ｆｕｌｌｗｉｄｔｈ", "ㄅㄆㄇ", "ア", "é", "日", "\u{FEFF}BOM text", "x",
];
$lists = [];
foreach ($samples as $u) {
    foreach ($text as $e) {
        $s = @mb_convert_encoding($u, $e, 'UTF-8');
        if (mb_convert_encoding($s, 'UTF-8', $e) !== $u) continue;
        $peers = ['UTF-8', 'ISO-8859-1', 'SJIS', 'EUC-JP', 'Windows-1252', 'UTF-16LE'];
        $l = [$e];
        foreach (array_rand(array_flip($peers), 3) as $p) if ($p !== $e) $l[] = $p;
        shuffle($l);
        rec($s, $l, false);
        rec($s, $l, true);
        $big = $text;
        shuffle($big);
        $big = array_slice($big, 0, 12);
        if (!in_array($e, $big)) $big[] = $e;
        rec($s, $big, false);
        rec($s, $big, true);
    }
}
foreach ($samples as $u) {
    foreach (['UTF-8', 'SJIS', 'EUC-JP', 'ISO-8859-1', 'Windows-1251', 'UTF-16BE', 'GB18030', 'BIG-5', 'UHC'] as $e) {
        $s = @mb_convert_encoding($u, $e, 'UTF-8');
        rec($s, $all, false);
        rec($s, $all, true);
        rec($s, $text, false);
    }
}

// 2. Random bytes with random candidate lists.
$alpha = [range(0, 255), array_merge(range(0x20, 0x7e), [0x1b, 0x0e, 0x0f, 0x2b, 0x2d, 0x7e, 0x7b, 0x7d, 0x24, 0x28, 0x42, 0x40]), array_merge(range(0x80, 0xff), range(0x40, 0x7e)), [0x00, 0x41, 0x4e, 0x30, 0x20, 0xd8, 0xdc, 0xfe, 0xff, 0x2e]];
for ($t = 0; $t < 6000; $t++) {
    $a = $alpha[$t % count($alpha)];
    $n = mt_rand(0, 14);
    $s = '';
    for ($i = 0; $i < $n; $i++) $s .= chr($a[mt_rand(0, count($a) - 1)]);
    $k = mt_rand(1, 4);
    $l = [];
    for ($i = 0; $i < $k; $i++) $l[] = $all[mt_rand(0, count($all) - 1)];
    rec($s, $l, (bool)($t & 1));
}

// 3. Thresholds of the scoring model.
$tr = function ($mk, $l, $max, $step = 1) {
    for ($k = 0; $k <= $max; $k += $step) {
        rec($mk($k), $l, false);
        rec($mk($k), array_reverse($l), false);
    }
};
foreach (['ISO-2022-JP', 'JIS', 'ISO-2022-JP-MS', 'CP50220', 'CP50222', 'ISO-2022-KR', 'UTF7-IMAP', 'UTF-7', 'ISO-2022-JP-2004', 'ISO-2022-JP-KDDI', 'HZ'] as $e) {
    $tr(fn($k) => "\x1b\$B" . str_repeat("\x30\x21", $k), [$e, 'UTF-8'], 120, 3);
    $tr(fn($k) => "\x1b(I\x31\x1b\$B" . str_repeat("\x30\x21", $k) . "\x1b(B", [$e, 'UTF-8'], 120, 3);
    $tr(fn($k) => "\x80\x1b\$B" . str_repeat("\x30\x21", $k) . "\x1b(B", [$e, 'UTF-8'], 120, 3);
    $tr(fn($k) => "\x0e\x31\x0f" . str_repeat("ab", $k), [$e, 'UTF-8'], 60, 3);
}
foreach ([1, 2, 5] as $m) {
    $tr(fn($k) => str_repeat("\x80", $m) . str_repeat("+AGE-", $k), ['UTF-7', 'UTF-8'], 100, 2);
    $tr(fn($k) => str_repeat("a+ ", $m) . str_repeat("+AGE-", $k), ['UTF-7', 'UTF-8'], 60);
    $tr(fn($k) => str_repeat("a&", $m) . str_repeat("&AGE-", $k), ['UTF7-IMAP', 'UTF-8'], 60);
    $tr(fn($k) => str_repeat("\x80", $m) . str_repeat("&AGE-", $k), ['UTF7-IMAP', 'UTF-8'], 100, 2);
    $tr(fn($k) => str_repeat("\x41\x00", 10 * $m) . str_repeat("\x00\x41", $k), ['UTF-16BE', 'UTF-16LE', 'ISO-8859-1'], 30 * $m, $m);
    $tr(fn($k) => str_repeat("\x41\x00", 10 * $m) . str_repeat("\x00\x41", $k), ['UTF-16BE', 'ISO-8859-5', 'UTF-16LE', 'ISO-8859-1'], 30 * $m, $m);
}
foreach (['SJIS-Mobile#DOCOMO', 'SJIS-Mobile#KDDI', 'SJIS-Mobile#SOFTBANK', 'UTF-8-Mobile#DOCOMO', 'UTF-8-Mobile#KDDI-A', 'UTF-8-Mobile#KDDI-B', 'UTF-8-Mobile#SOFTBANK', 'SJIS-win', 'CP932', 'SJIS-mac', 'SJIS-2004', 'eucJP-win', 'CP51932', 'EUC-JP-2004'] as $e) {
    foreach (["abc", "\x82\xa0\x82\xa2", "\xa4\xa2\xa4\xa4", "\xf8\x9f", "\xef\xbb\xbf", "\xee\x98\xbe", "\xf0\x9f\x90\x98"] as $s) {
        rec($s, [$e, 'UTF-8'], false);
        rec($s, ['UTF-8', $e], false);
        rec($s, [$e, 'SJIS'], false);
        rec($s, [$e, 'ASCII'], true);
    }
}
foreach (["", "abc", "\xc3\xa9", "\x80"] as $s) {
    rec($s, ['BASE64', 'UTF-8'], false);
    rec($s, ['UUENCODE', 'ASCII'], false);
    rec($s, ['HTML-ENTITIES', 'Quoted-Printable', 'ISO-8859-1'], false);
    rec($s, ['7bit', '8bit'], false);
    rec($s, ['8bit', 'UTF-8'], true);
    rec($s, ['UTF-8', 'UTF-8', 'ISO-8859-1'], false);
    rec($s, ['ISO-8859-1', 'UTF-8', 'UTF-8'], false);
    rec($s, ['BASE64'], true);
}
