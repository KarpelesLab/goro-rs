<?php
// Fixture driver for html.rs: run once on PHP 8.5 (observed behaviour).
//   php85 -d error_reporting=-1 -d display_errors=1 -d log_errors=0 -d html_errors=0 html.php entities.json > html_full.txt
// tools/fixture_hash.py then replaces long results with their md5 (see common.inc).
require __DIR__ . '/common.inc';

$charsets = ['UTF-8', 'ISO-8859-1', 'ISO-8859-5', 'ISO-8859-15', 'cp866', 'cp1251', 'cp1252',
    'KOI8-R', 'BIG5', 'GB2312', 'BIG5-HKSCS', 'Shift_JIS', 'EUC-JP', 'MacRoman'];
$single = ['ISO-8859-1', 'ISO-8859-5', 'ISO-8859-15', 'cp866', 'cp1251', 'cp1252', 'KOI8-R', 'MacRoman'];
$multi = ['BIG5', 'GB2312', 'BIG5-HKSCS', 'Shift_JIS', 'EUC-JP'];
$doctypes = [0, 16, 32, 48];
$quotes = [0, 2, 3];
$modes = [0, 4, 8, 128, 136];

function u8(int $cp): string {
    // Encodes any value up to 0x7fffffff, including surrogates (which are
    // then invalid UTF-8 input).
    if ($cp < 0x80) return chr($cp);
    if ($cp < 0x800) return chr(0xc0 | ($cp >> 6)) . chr(0x80 | ($cp & 0x3f));
    if ($cp < 0x10000) return chr(0xe0 | ($cp >> 12)) . chr(0x80 | (($cp >> 6) & 0x3f)) . chr(0x80 | ($cp & 0x3f));
    return chr(0xf0 | ($cp >> 18)) . chr(0x80 | (($cp >> 12) & 0x3f)) . chr(0x80 | (($cp >> 6) & 0x3f)) . chr(0x80 | ($cp & 0x3f));
}

// --- charset names and default_charset
$names = ['UTF-8', 'utf-8', 'utf8', 'UTF8', 'ISO-8859-1', 'iso-8859-1', 'ISO8859-1', 'latin1', 'ISO-8859-5',
    'ISO8859-5', 'ISO-8859-15', 'ISO8859-15', 'iso8859-15', 'cp866', 'CP866', 'ibm866', 'IBM866', '866',
    'cp1251', 'Windows-1251', 'win-1251', '1251', 'cp1252', 'Windows-1252', 'windows-1252', '1252', 'KOI8-R',
    'koi8-r', 'koi8-ru', 'koi8r', 'KOI8-U', 'BIG5', 'big5', '950', 'GB2312', 'gb2312', '936', 'BIG5-HKSCS',
    'big5-hkscs', 'Shift_JIS', 'shift_jis', 'SJIS', 'sjis', 'SJIS-win', 'cp932', '932', 'EUC-JP', 'euc-jp',
    'EUCJP', 'eucJP-win', 'MacRoman', 'macroman', 'MACROMAN', 'ISO-8859-2', 'bogus', ' UTF-8', 'UTF-8 ',
    'cp1250', 'UTF-16', 'ASCII', 'us-ascii', "UTF-8\0", 'x'];
$probe = "<\xe9\x80'\"&a\xc3\xa9\x82\xa0>";
foreach ($names as $n) {
    run('htmlentities', $probe, 3, $n);
    run('htmlspecialchars', $probe, 3, $n);
    run('html_entity_decode', "&eacute;&lt;&#233;&#x80;&euro;", 3, $n);
    run('htmlspecialchars_decode', "&eacute;&lt;&#233;", 3);
    run('get_html_translation_table', 0, 3, $n);
}
foreach (['ISO-8859-1', 'cp1251', 'bogus', '', 'SJIS', 'utf8', 'UTF-8'] as $dc) {
    ini_set('default_charset', $dc);
    echo "# default_charset=$dc\n";
    run('htmlentities', $probe, 3, '');
    run('htmlentities', $probe, 3, null);
    run('htmlentities', $probe);
    run('htmlspecialchars', $probe, 3, '');
    run('html_entity_decode', "&eacute;&lt;&#233;&#x80;&euro;", 3, '');
    run('html_entity_decode', "&eacute;&lt;&#233;&#x80;&euro;");
    run('get_html_translation_table', 1, 3, '');
}
ini_set('default_charset', 'UTF-8');

// --- translation tables
foreach ([0, 1] as $table) foreach ($quotes as $q) foreach ($doctypes as $d) foreach ($charsets as $cs)
    run('get_html_translation_table', $table, $q | $d, $cs);
run('get_html_translation_table');
run('get_html_translation_table', 1);
run('get_html_translation_table', 2);
run('get_html_translation_table', -1);
run('get_html_translation_table', 1, -1);
run('get_html_translation_table', 1, 3 | 4 | 8 | 128 | 48);
run('get_html_translation_table', 0, 64 | 3);
run('get_html_translation_table', 1, 0xffff);

// --- single-byte charsets: all 256 bytes
$all = '';
for ($i = 0; $i < 256; $i++) $all .= chr($i);
label($all, 'bytes:0-256');
foreach ($single as $cs) foreach ($quotes as $q) foreach ($doctypes as $d) foreach ($modes as $m) {
    run('htmlentities', $all, $q | $d | $m, $cs);
    run('htmlspecialchars', $all, $q | $d | $m, $cs);
}
foreach ($single as $cs) {
    run('htmlentities', "&amp;&eacute;&#233;&#xe9;&bogus;&apos;" . $all, 3, $cs, false);
}

// --- UTF-8: code point chunks
$ranges = [[0, 0x3000], [0xd7f0, 0xe010], [0xfdc0, 0xfe10], [0xfff0, 0x10010], [0x1d400, 0x1d800],
    [0x1fff0, 0x20010], [0x2fff0, 0x30010], [0x10fff0, 0x110010], [0x1fffff, 0x200001]];
$chunks = [];
foreach ($ranges as [$a, $b]) {
    for ($c = $a; $c < $b; $c += 1024) {
        $s = '';
        for ($i = $c; $i < min($b, $c + 1024); $i++) $s .= u8($i) . ($i % 16 == 15 ? "\n" : '');
        $chunks[] = label($s, sprintf('u8:%d-%d', $c, min($b, $c + 1024)));
    }
}
foreach ($chunks as $ci => $s) foreach ($quotes as $q) foreach ($doctypes as $d) foreach ($modes as $m) {
    run('htmlentities', $s, $q | $d | $m, 'UTF-8');
    if ($ci == 0 || $ci >= 12) run('htmlspecialchars', $s, $q | $d | $m, 'UTF-8');
}
// sequences that have multi-code-point HTML5 names, and their neighbours
$seqs = "\u{2242}\u{338}|\u{2242}|\u{338}|\u{226b}\u{20d2}|\u{3d}\u{20e5}|\u{66}\u{6a}|fj|\u{2267}\u{338}x"
    . "|\u{2282}\u{20d2}\u{338}|<\u{20d2}|>\u{20d2}|\u{2a6d}\u{338}|\u{2229}\u{fe00}|\u{2229}\u{fe01}";
foreach ($quotes as $q) foreach ($doctypes as $d) foreach ([0, 128] as $m) run('htmlentities', $seqs, $q | $d | $m, 'UTF-8');

// --- invalid UTF-8
$bytes = ["\x41", "\x80", "\x8f", "\x90", "\xa0", "\xbf", "\xc0", "\xc1", "\xc2", "\xdf", "\xe0", "\xe1",
    "\xed", "\xee", "\xef", "\xf0", "\xf4", "\xf5", "\xf8", "\xfc", "\xff", "&", "<"];
for ($i = 0; $i < 500; $i++) {
    $s = rnd_str($bytes, 5);
    foreach ([3, 3 | 4, 3 | 8, 3 | 128 | 48] as $f) run('htmlspecialchars', $s, $f, 'UTF-8');
    run('htmlentities', $s, 3 | 8, 'UTF-8');
}

// --- multibyte charsets
$mb = ["\x00", "\x22", "\x26", "\x27", "\x3c", "\x40", "\x41", "\x5c", "\x62", "\x7e", "\x7f", "\x80", "\x81",
    "\x8e", "\x8f", "\x9f", "\xa0", "\xa1", "\xa4", "\xc0", "\xdf", "\xe0", "\xef", "\xf0", "\xf9", "\xfc",
    "\xfe", "\xff"];
foreach ($multi as $cs) {
    for ($i = 0; $i < 250; $i++) {
        $s = rnd_str($mb, 6);
        foreach ([3, 3 | 4, 3 | 8] as $f) run('htmlspecialchars', $s, $f, $cs);
    }
    run('htmlentities', "<\xa4\xa1&eacute;", 3, $cs);
    run('htmlentities', "<\xa4\xa1&eacute;", 3 | 128 | 48, $cs);
    run('html_entity_decode', "&lt;&eacute;&#65;&#233;&#x3042;&amp;&quot;&#039;&apos;", 3, $cs);
    run('html_entity_decode', "&lt;&eacute;&#65;&#233;&#x3042;&amp;&quot;&#039;&apos;", 3 | 48, $cs);
}

// --- double_encode = false
$ents = '';
// argv[1]: the WHATWG entities.json (https://html.spec.whatwg.org/entities.json)
$json = json_decode(file_get_contents($argv[1]), true);
$entnames = $json ? array_keys($json) : [];
foreach ($entnames as $k) $ents .= $k . '|';
label($ents, 'ents');
foreach ($doctypes as $d) {
    run('htmlspecialchars', $ents, 3 | $d, 'UTF-8', false);
}
$num = '';
$nums = array_merge(range(0, 0x110), [0x7ff, 0x800, 0xd7ff, 0xd800, 0xdfff, 0xe000, 0xfdcf, 0xfdd0, 0xfdef,
    0xfdf0, 0xfffd, 0xfffe, 0xffff, 0x10000, 0x1fffe, 0x1ffff, 0x10fffe, 0x10ffff, 0x110000, 0x7fffffff,
    0x80000000, 0xffffffff]);
foreach ($nums as $n) $num .= "&#$n;|&#x" . dechex($n) . ";|&#X" . strtoupper(dechex($n)) . ";|";
$num .= "&#;|&#x;|&#X;|&#0000000065;|&#x00000041;|&#65|&#x41|&#99999999999999999999;|&#xffffffffffff;|&#-1;|&#+1;|&# 65;|&#a;|&#xg;|&;|&|&&amp;|&amp|&AMP;|&Amp;|&lt|&ltx;|&#x0041;"
    . "|&#00000000000000000000065;|&#x000000000000000000041;|&#0x41;|&#X27;|&#039|&#x27|&#1114111;|&#01114112;";
label($num, 'num');
foreach ($doctypes as $d) {
    run('htmlspecialchars', $num, 3 | $d, 'UTF-8', false);
    run('htmlentities', $num, 3 | $d, 'UTF-8', false);
}
$dalpha = ['&', '#', 'x', 'X', '0', '1', '9', 'a', 'f', 'A', 'm', 'p', ';', 'g', 't', 'l', 'o', 'q', 'u', 'n', 'b', 's', ' '];
for ($i = 0; $i < 600; $i++) {
    $s = '&' . rnd_str($dalpha, 9);
    run('htmlspecialchars', $s, 3 | [0, 16, 32, 48][rnd(4)], 'UTF-8', false);
}

// --- decoding
foreach ($doctypes as $d) foreach ($charsets as $cs) {
    run('html_entity_decode', $ents, 3 | $d, $cs);
}
foreach ($doctypes as $d) foreach (['UTF-8', 'ISO-8859-1', 'cp1252', 'KOI8-R', 'MacRoman', 'Shift_JIS'] as $cs) {
    run('html_entity_decode', $num, 3 | $d, $cs);
}
foreach ($doctypes as $d) {
    run('htmlspecialchars_decode', $ents, 3 | $d);
    run('htmlspecialchars_decode', $num, 3 | $d);
}
$qprobe = "&quot;&#34;&#x22;&#039;&#39;&#x27;&apos;&#0039;&lt;&gt;&amp;&amp;lt;&QUOT;&LT;&GT;&AMP;&lt&gt&amp&quot";
foreach ($quotes as $q) foreach ($doctypes as $d) {
    run('html_entity_decode', $qprobe, $q | $d, 'UTF-8');
    run('htmlspecialchars_decode', $qprobe, $q | $d);
    run('htmlspecialchars', "\"'<>&", $q | $d, 'UTF-8');
    run('htmlentities', "\"'<>&\u{a0}\u{e9}", $q | $d, 'UTF-8');
}
run('html_entity_decode', $qprobe);
run('htmlspecialchars_decode', $qprobe);
run('htmlspecialchars', "\"'<>&\x80");
run('htmlentities', "\"'<>&\x80\u{e9}");
foreach ([-1, 0xffff, 64, 3 | 64, 3 | 256] as $f) {
    run('htmlspecialchars', "\"'<>&\x80\u{1}\u{e9}", $f);
    run('htmlentities', "\"'<>&\x80\u{1}\u{e9}", $f);
    run('html_entity_decode', $qprobe . "&#1;&#x80;&eacute;", $f);
    run('htmlspecialchars_decode', $qprobe, $f);
}
for ($i = 0; $i < 1200; $i++) {
    $s = rnd_str($dalpha, 12);
    $d = [0, 16, 32, 48][rnd(4)];
    run('html_entity_decode', $s, 3 | $d, 'UTF-8');
    run('htmlspecialchars_decode', $s, 3 | $d);
}

// --- strip_tags
$st = [
    'NEAT <? cool < blah ?> STUFF', 'NEAT <? cool > blah ?> STUFF', 'NEAT <!-- cool < blah --> STUFF',
    'NEAT <!-- cool > blah --> STUFF', 'NEAT <? echo \"\\\"\"?> STUFF', 'NEAT <? echo \'\\\'\'?> STUFF',
    'TESTS ?!!?!?!!!?!!', '<span title="Bug \' Trigger">Text</span>', "<div>Bug ' Trigger</div> Missing Text",
    '<!-- testing I\'ve been to mars -->foobar', '<a alt="foobar">foo<!-- foo! --></a>bar',
    '<a alt="foobar"/>foo<?= foo! /* <!-- "cool" --> */ ?>bar', '< ax', '<! a>', '<? ax',
    '<?php $dom->test(); ?> this is a test', '<?xml $xml->test(); ?> this is a test',
    "<span class=sf-dump-> this is a test</span>", '<?= \'<?= 1 ?>\' ?>2',
    '<br /><br  />USD<input type="text"/><br/>CDN<br><input type="text" />', '<a/b>', "<s/trong>b</strong>",
    '<span title="test > all">this</span>', '<foo<>bar>', '<foo<!>bar>', '<foo<?>bar>',
    "1:<!-- abc -  -->\n2:<!doctype -- >\n3:\n4:<abc - def>\n5:abc - def\n6:</abc>\n",
    'This text is shown <?XML:NAMESPACE PREFIX = ST1 /><b>This Text disappears</b>',
    '<p>foo <b>bar</b> <a href="#">foobar</a></p>',
    "<html><p>hello</p><b>world</b><a href=\"#fragment\">Other text</a></html><?php echo hello ?>",
];
$allowed = [null, '', '<a>', '<b>', '<a><b>', '<p><a>', '<A>', 'a', '<a', '<abc>', '<input>', '<br><input>',
    '<strong>', '<html><p><a><?php', '<?php', '<!-->', '<a href>', '< a>', '<a/>', '<a><', 'a>b',
    ['a'], ['a', 'p'], ['A'], ['<a>'], [], ['b', 'abc'], ['a b']];
foreach ($st as $s) foreach ($allowed as $al) run('strip_tags', $s, $al);
$salpha = ['<', '<', '>', '>', '!', '-', '-', '?', '"', "'", '/', 'a', 'b', 'p', 'x', 'm', 'l', ' ', '=', "\n",
    '\\', "\0", 'A', 'B', 'h', '[', 'C', 'D', 'T', 'Y', '.'];
for ($i = 0; $i < 5000; $i++) {
    $s = rnd_str($salpha, 14);
    run('strip_tags', $s, $allowed[rnd(count($allowed))]);
}
// --- round 2 probes
run('htmlentities', "<\xd2>\xd2=\xe5<\xe5fj", 3 | 48, 'ISO-8859-1');
run('htmlentities', "<\xd2>\xd2=\xe5", 3 | 48, 'cp1252');
foreach ($doctypes as $d) {
    run('htmlspecialchars_decode', "&#X27;&#039|&#x27|&#39;&#x3C;&#X3e;&#060;&#0;&#x26;&#38;&#x22;", 3 | $d);
    run('html_entity_decode', "&#X27;&#039|&#x27|&#39;&#x3C;&#X3e;&#060;&#0;&#x26;&#38;&#x22;&#128512;&#xd800;", 3 | $d, 'UTF-8');
}
$salpha2 = ['<', '<', '>', '>', '!', '-', '-', '?', '"', "'", '/', 'a', 'b', 'x', 'm', 'l', 'X', 'M', 'L', ':', ' ',
    "\t", "\n", "\r", "\x0b", "\x0c", '=', '\\', "\0", 'p', 'h', '?>', '<?', '<!--', '-->', '<a', '</a', '<x'];
for ($i = 0; $i < 3000; $i++) {
    $s = rnd_str($salpha2, 12);
    run('strip_tags', $s, $allowed[rnd(count($allowed))]);
}
echo "# done\n";
