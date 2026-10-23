--TEST--
htmlentities()/html_entity_decode(): charset map, get_next_char() and single-byte/multi-byte mappings
--FILE--
<?php
function h($s) { echo bin2hex($s), "\n"; }
// 3: single-byte mapping in htmlentities
h(htmlentities("\x80\xe6\xf2", ENT_QUOTES, 'MacRoman'));
h(htmlentities("\x9a\xbf", ENT_QUOTES, 'KOI8-R'));
h(htmlentities("\xff\xf8", ENT_QUOTES, 'cp866'));
h(htmlentities("\xa4\xbc\xa6", ENT_QUOTES, 'ISO-8859-15'));
h(htmlentities("\xf0\xfd\xa1", ENT_QUOTES, 'ISO-8859-5'));
h(htmlentities("\x80\x9f\xe9", ENT_QUOTES, 'cp1251'));
h(htmlentities("\x80\x99\xe9", ENT_QUOTES, 'cp1252'));
// 4: unmapped bytes pass through
foreach (['cp1252' => "\x81\x8d\x8f\x90\x9d", 'cp1251' => "\x98", 'MacRoman' => "\x01\x1f\x7f\x00\t"] as $cs => $s) {
  foreach ([ENT_QUOTES, ENT_QUOTES|ENT_SUBSTITUTE, ENT_QUOTES|ENT_DISALLOWED, ENT_QUOTES|ENT_DISALLOWED|ENT_HTML5, ENT_QUOTES|ENT_IGNORE] as $f) {
    h(htmlspecialchars("a{$s}b", $f, $cs));
    h(htmlentities("a{$s}b", $f, $cs));
  }
}
h(htmlspecialchars("a\tb\x01", ENT_QUOTES, 'MacRoman'));
h(htmlspecialchars("a\tb\x01\n\r", ENT_QUOTES|ENT_DISALLOWED, 'MacRoman'));
// 5: multibyte decode
foreach (['BIG5', 'GB2312', 'Shift_JIS', 'EUC-JP', 'BIG5-HKSCS', 'SJIS', '936'] as $cs) {
  h(html_entity_decode("&amp;&lt;&#65;&#x42;&#20013;&#92;&#126;&#x7d;&eacute;&nbsp;&#31;", ENT_QUOTES, $cs));
}
foreach (['MacRoman', 'cp1252', 'KOI8-R', 'cp866', 'ISO-8859-5', 'ISO-8859-15', 'cp1251', 'ISO-8859-1'] as $cs) {
  h(html_entity_decode("&#9;&#127;&#128;&#x20ac;&euro;&eacute;&nbsp;&#x2116;&sect;&#xa4;&#x160;&copy;&Auml;&#x0x41;&#x41", ENT_QUOTES, $cs));
}
// 12: charset names
foreach (['utf8', 'latin1', '1251', 'latin9', 'iso_8859-1', 'iso_8859_1', 'UTF-8', 'iso-8859-1', 'KOI8-RU', 'macroman', 'ibm866', 'windows-1251', 'eucjp-win', 'koi8-r'] as $cs) {
  echo $cs, ": "; h(htmlentities("\xe9<", ENT_QUOTES, $cs));
}
echo "decode:"; h(html_entity_decode("&eacute;", ENT_QUOTES, 'latin1'));
echo "decode noamp:"; h(html_entity_decode("x", ENT_QUOTES, 'bogus'));
echo "empty:"; h(htmlentities("", ENT_QUOTES, 'bogus'));
echo "empty:"; h(htmlspecialchars("", ENT_QUOTES, 'bogus'));
// 13: utf-8 invalid sequences
foreach (["\xf4\xf5\xf6\xf7", "\xe0\xf5x", "\xc3", "\xc3\xc3\xa9", "\xe2\x82", "\xe2\x82x", "\xf0\x9f\x98", "\xf0\x9f\x98x", "\xed\xa0\x80", "\xc0\x80", "\xf4\x90\x80\x80", "\xe0\x80\x80", "\xf0\x80\x80\x80", "\x80\xbf", "\xf5\x80", "\xe2\xc3\xa9"] as $s) {
  echo bin2hex(htmlspecialchars($s, ENT_QUOTES|ENT_SUBSTITUTE)), " ", bin2hex(htmlspecialchars($s . "z", ENT_QUOTES|ENT_IGNORE)), " ", bin2hex(htmlspecialchars($s)), " ", bin2hex(htmlspecialchars($s, ENT_QUOTES)), "\n";
}
// 16: empty
var_dump(htmlentities("", ENT_QUOTES, "GB2312"));
var_dump(htmlentities("a<\x81\x40", ENT_QUOTES, "Shift_JIS"));
// misc: double encode
foreach (["&amp;", "&#65;", "&#x41;", "&#x0x41;", "&#1114112;", "&#0;", "&#1;", "&apos;", "&bogus;", "&;", "&", "&#;", "&#x;", "&Eacute;", "&#99999999999999999999;"] as $s) {
  foreach ([ENT_QUOTES, ENT_QUOTES|ENT_XHTML, ENT_QUOTES|ENT_HTML5|ENT_DISALLOWED, ENT_QUOTES|ENT_XML1|ENT_DISALLOWED] as $f) {
    echo htmlspecialchars($s, $f, 'UTF-8', false), " ", htmlentities($s, $f, 'UTF-8', false), " ", html_entity_decode($s, $f), " ", htmlspecialchars_decode($s, $f), "\n";
  }
}
echo htmlentities("fj\u{2242}\u{338}<\u{20D2}", ENT_HTML5), "\n";
echo htmlentities("\xe9", ENT_HTML5, 'ISO-8859-1'), "\n";
echo html_entity_decode("&NotEqualTilde;&nGt;&lt;", ENT_HTML5), "\n";
echo bin2hex(html_entity_decode("&NotEqualTilde;&nGt;&lt;", ENT_HTML5, 'cp1252')), "\n";
echo html_entity_decode("&#39;&#x27;&quot;&#34;", ENT_NOQUOTES), "|", html_entity_decode("&#39;&#x27;&quot;&#34;", ENT_COMPAT), "\n";
echo htmlspecialchars_decode("&#39;&#x27;&quot;&#34;&#65;&apos;", ENT_QUOTES), "|", htmlspecialchars_decode("&apos;", ENT_QUOTES|ENT_HTML5), "\n";
echo html_entity_decode("&#13;&#x0D;", ENT_QUOTES|ENT_HTML5) === "&#13;&#x0D;" ? "ok\n" : "no\n";
--EXPECTF--
2641756d6c3b2645636972633b265561637574653b
266e6273703b26636f70793b
266e6273703b266465673b
266575726f3b264f456c69673b26536361726f6e3b
f026736563743ba1
809fe9
266575726f3b2674726164653b266561637574653b
61818d8f909d62
61818d8f909d62
61818d8f909d62
61818d8f909d62
61818d8f909d62
61818d8f909d62
61818d8f909d62
61818d8f909d62
61818d8f909d62
61818d8f909d62
619862
619862
619862
619862
619862
619862
619862
619862
619862
619862
61011f7f000962
61011f7f000962
61011f7f000962
61011f7f000962
61262378464646443b262378464646443b7f262378464646443b0962
61011f7f000962
61262378464646443b262378464646443b7f262378464646443b0962
61011f7f000962
61011f7f000962
61011f7f000962
61096201
610962262378464646443b0a0d
263c4142262332303031333b5c26233132363b7d266561637574653b266e6273703b262333313b
263c4142262332303031333b5c26233132363b7d266561637574653b266e6273703b262333313b
263c4142262332303031333b262339323b26233132363b7d266561637574653b266e6273703b262333313b
263c4142262332303031333b262339323b26233132363b7d266561637574653b266e6273703b262333313b
263c4142262332303031333b5c26233132363b7d266561637574653b266e6273703b262333313b
263c4142262332303031333b262339323b26233132363b7d266561637574653b266e6273703b262333313b
263c4142262332303031333b5c26233132363b7d266561637574653b266e6273703b262333313b
0926233132373b26233132383bdbdb8eca262378323131363ba426237861343b2623783136303ba980412623783431
0926233132373b26233132383b8080e9a0262378323131363ba7a48aa9c4412623783431
0926233132373b26233132383b262378323061633b266575726f3b266561637574653b9a262378323131363b26736563743b26237861343b2623783136303bbf2641756d6c3b412623783431
0926233132373b26233132383b262378323061633b266575726f3b266561637574653bfffc26736563743bfd2623783136303b26636f70793b2641756d6c3b412623783431
0926233132373b26233132383b262378323061633b266575726f3b266561637574653ba0f0fd26237861343b2623783136303b26636f70793b2641756d6c3b412623783431
0926233132373b26233132383ba4a4e9a0262378323131363ba726237861343ba6a9c4412623783431
0926233132373b26233132383b8888266561637574653ba0b9a7a42623783136303ba92641756d6c3b412623783431
0926233132373b26233132383b262378323061633b266575726f3be9a0262378323131363ba7a42623783136303ba9c4412623783431
utf8: 
Warning: htmlentities(): Charset "utf8" is not supported, assuming UTF-8 in %s on line 29

latin1: 
Warning: htmlentities(): Charset "latin1" is not supported, assuming UTF-8 in %s on line 29

1251: 
Warning: htmlentities(): Charset "1251" is not supported, assuming UTF-8 in %s on line 29

latin9: 
Warning: htmlentities(): Charset "latin9" is not supported, assuming UTF-8 in %s on line 29

iso_8859-1: 
Warning: htmlentities(): Charset "iso_8859-1" is not supported, assuming UTF-8 in %s on line 29

iso_8859_1: 
Warning: htmlentities(): Charset "iso_8859_1" is not supported, assuming UTF-8 in %s on line 29

UTF-8: 
iso-8859-1: 266561637574653b266c743b
KOI8-RU: e9266c743b
macroman: 264567726176653b266c743b
ibm866: e9266c743b
windows-1251: e9266c743b
eucjp-win: 
Notice: htmlentities(): Only basic entities substitution is supported for multi-byte encodings other than UTF-8; functionality is equivalent to htmlspecialchars in %s on line 29

koi8-r: e9266c743b
decode:
Warning: html_entity_decode(): Charset "latin1" is not supported, assuming UTF-8 in %s on line 31
c3a9
decode noamp:78
empty:
empty:
efbfbd 7a efbfbd 
efbfbd78 787a efbfbd78 
efbfbd 7a efbfbd 
efbfbdc3a9 c3a97a efbfbdc3a9 
efbfbd 7a efbfbd 
efbfbd78 787a efbfbd78 
efbfbd 7a efbfbd 
efbfbd78 787a efbfbd78 
efbfbd 7a efbfbd 
efbfbdefbfbd 7a efbfbdefbfbd 
efbfbd 7a efbfbd 
efbfbd 7a efbfbd 
efbfbd 7a efbfbd 
efbfbdefbfbd 7a efbfbdefbfbd 
efbfbdefbfbd 7a efbfbdefbfbd 
efbfbdc3a9 c3a97a efbfbdc3a9 
string(0) ""

Notice: htmlentities(): Only basic entities substitution is supported for multi-byte encodings other than UTF-8; functionality is equivalent to htmlspecialchars in %s on line 41
string(7) "a&lt;�@"
&amp; &amp; & &
&amp; &amp; & &
&amp; &amp; & &
&amp; &amp; & &
&#65; &#65; A &#65;
&#65; &#65; A &#65;
&#65; &#65; A &#65;
&#65; &#65; A &#65;
&#x41; &#x41; A &#x41;
&#x41; &#x41; A &#x41;
&#x41; &#x41; A &#x41;
&#x41; &#x41; A &#x41;
&#x0x41; &#x0x41; A &#x0x41;
&#x0x41; &#x0x41; A &#x0x41;
&#x0x41; &#x0x41; A &#x0x41;
&#x0x41; &#x0x41; A &#x0x41;
&amp;#1114112; &amp;#1114112; &#1114112; &#1114112;
&amp;#1114112; &amp;#1114112; &#1114112; &#1114112;
&amp;#1114112; &amp;&num;1114112&semi; &#1114112; &#1114112;
&amp;#1114112; &amp;#1114112; &#1114112; &#1114112;
&#0; &#0; &#0; &#0;
&#0; &#0; &#0; &#0;
&amp;#0; &amp;&num;0&semi; &#0; &#0;
&amp;#0; &amp;#0; &#0; &#0;
&#1; &#1; &#1; &#1;
&#1; &#1; &#1; &#1;
&amp;#1; &amp;&num;1&semi; &#1; &#1;
&amp;#1; &amp;#1; &#1; &#1;
&amp;apos; &amp;apos; &apos; &apos;
&apos; &apos; ' '
&apos; &apos; ' '
&apos; &apos; ' '
&amp;bogus; &amp;bogus; &bogus; &bogus;
&amp;bogus; &amp;bogus; &bogus; &bogus;
&amp;bogus; &amp;bogus&semi; &bogus; &bogus;
&amp;bogus; &amp;bogus; &bogus; &bogus;
&amp;; &amp;; &; &;
&amp;; &amp;; &; &;
&amp;; &amp;&semi; &; &;
&amp;; &amp;; &; &;
&amp; &amp; & &
&amp; &amp; & &
&amp; &amp; & &
&amp; &amp; & &
&amp;#; &amp;#; &#; &#;
&amp;#; &amp;#; &#; &#;
&amp;#; &amp;&num;&semi; &#; &#;
&amp;#; &amp;#; &#; &#;
&amp;#x; &amp;#x; &#x; &#x;
&amp;#x; &amp;#x; &#x; &#x;
&amp;#x; &amp;&num;x&semi; &#x; &#x;
&amp;#x; &amp;#x; &#x; &#x;
&Eacute; &Eacute; É &Eacute;
&Eacute; &Eacute; É &Eacute;
&Eacute; &Eacute; É &Eacute;
&amp;Eacute; &amp;Eacute; &Eacute; &Eacute;
&amp;#99999999999999999999; &amp;#99999999999999999999; &#99999999999999999999; &#99999999999999999999;
&amp;#99999999999999999999; &amp;#99999999999999999999; &#99999999999999999999; &#99999999999999999999;
&amp;#99999999999999999999; &amp;&num;99999999999999999999&semi; &#99999999999999999999; &#99999999999999999999;
&amp;#99999999999999999999; &amp;#99999999999999999999; &#99999999999999999999; &#99999999999999999999;
&fjlig;&nesim;&nvlt;
&eacute;
≂̸≫⃒<
264e6f74457175616c54696c64653b266e47743b3c
&#39;&#x27;&quot;&#34;|&#39;&#x27;""
''""&#65;&apos;|'
ok
