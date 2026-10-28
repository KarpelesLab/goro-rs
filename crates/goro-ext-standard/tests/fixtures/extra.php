<?php
// Second-round probes for html.rs, url.rs and encoding.rs, run once on
// PHP 8.5 (observed behaviour):
//   php85 -d error_reporting=-1 -d display_errors=1 -d log_errors=0 -d html_errors=0 extra.php > extra_full.txt
// Each fixture test also checks the lines of its functions in extra.txt.
require __DIR__ . '/common.inc';
$GLOBALS['rng'] = 0x9e3779b9;

// strip_tags: escapes, nesting with declarations and PHP blocks
$st = ['<x<!a>b>c', '<x<!a>b', '<x<?a?>b>c', '<a href="\"">x</a>', "<a href='\\'>'>x</a>", '<?php "\\"?>" ?>y',
    "<!a '>' b>c", '<!-- a <!-- b --> c', '<![CDATA[ <b> ]]>x', '<a <b> c>d', '<a <!-- x --> b>c', '<? "?>" ?>z',
    "<a\x0bb>c", "a<\x0cb", "<?xml:x>y", "<x<?xml:y>z>w", "<?XmL:a\"b>\"c>d"];
foreach ($st as $s) foreach ([null, '<a>', '<x>', '<a><b>'] as $al) run('strip_tags', $s, $al);
$salpha = ['<', '<', '>', '>', '!', '-', '?', '"', "'", '\\', 'a', 'b', ' ', '<!', '<?', '<!--', '-->', '?>', 'x', 'm', 'l', ':'];
for ($i = 0; $i < 3000; $i++) run('strip_tags', rnd_str($salpha, 10), [null, '<a>', '<b>'][rnd(3)]);

// html: CJK decoding of 0x5C / 0x7E, other charsets
foreach (['Shift_JIS', 'EUC-JP', 'BIG5', 'GB2312', 'BIG5-HKSCS'] as $cs) {
    run('html_entity_decode', '&#92;&#126;&bsol;&#x5c;&#x7E;&#127;&#31;&#32;&excl;&tilde;&circ;', 3 | 48, $cs);
    run('html_entity_decode', '&#92;&#126;&#127;&#31;&#32;&quot;&#34;&apos;', 3, $cs);
}
foreach (['UTF-8', 'ISO-8859-1', 'cp1252', 'KOI8-R'] as $cs) {
    run('htmlentities', "a&b&amp;&#xZ;&#x;&#65;&Amp;", 3, $cs, false);
    run('htmlspecialchars', "\"'<>&", 0, $cs);
}

// parse_url: ports and odd hosts
foreach (['http://h:00080', 'http://h:000080', 'http://h:8x', 'http://h:+8', 'http://h:-0', 'http://h:	8',
    'http://h:  8', 'http://h:8 ', 'http://h:65535x', 'http://h:99999', 'http:// h/', 'http://h /', 'h:00080',
    'h:000080', 'h:8/x', 'h:8?x', 'h:8#x', 'a/b:8', 'a?b:8', '//h:8x', '//:8', 'http://[::1]:8/', 'http://[::1]x:8/',
    'http://[::1', 'http://u@[::1]:8', 'http://h:8@x:9/', 'http://a:b@c:d@e:1/', 'FILE:///x', 'file:///C|/x',
    'file://localhost/x', 'file:/x', 'file:x', 'x:/', 'x:?', 'x:#', 'x://?', 'x://#', 'x://h?', '?', '#', '?#',
    "\x00:8", "a\x00:8", "http://\x00/", "http://h\x00:8/"] as $u) {
    run('parse_url', $u);
}

// encoding
foreach (['', ' ', 'a', 'ab', 'abc', "\r\n", "=\r\n"] as $s) {
    run('hex2bin', $s);
    run('base64_decode', $s);
    run('base64_decode', $s, true);
    run('quoted_printable_decode', $s);
    run('quoted_printable_encode', $s);
}
$n45 = str_repeat('x', 45);
run('convert_uuencode', $n45);
run('convert_uuencode', $n45 . $n45);
run('convert_uudecode', convert_uuencode($n45 . 'yz'));
run('convert_uudecode', str_replace("\n", "\r\n", convert_uuencode($n45 . 'yz')));
run('convert_uudecode', substr(convert_uuencode($n45 . $n45), 0, 62));
run('convert_uudecode', substr(convert_uuencode($n45 . $n45), 0, 61));
run('convert_uudecode', substr(convert_uuencode($n45), 0, -2));
run('convert_uudecode', "M" . str_repeat('A', 60));
run('convert_uudecode', "M" . str_repeat('A', 59));
$u8 = ["\u{10348}", "\u{20ac}", "\u{e9}", "\xf5\x80", "\xff", 'a', ' ', "\t", '='];
for ($i = 0; $i < 400; $i++) run('quoted_printable_encode', rnd_str($u8, 40));
echo "# done\n";
