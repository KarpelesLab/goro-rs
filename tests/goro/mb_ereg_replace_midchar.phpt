--TEST--
mb_ereg_replace(): empty matches inside a multibyte EUC-JP character
--EXTENSIONS--
mbstring
--FILE--
<?php
mb_regex_encoding('EUC-JP');
$s = mb_convert_encoding("sS>iä", 'EUC-JP', 'UTF-8');
echo bin2hex($s), "\n";
foreach (['\B', '(?!x)\B', '', 'x*', '\b', '(?=)', '$'] as $p) {
  $r = mb_ereg_replace($p, '[]', $s, 'pz'); var_dump($r === null ? null : bin2hex($r));
  $r = mb_ereg_replace($p, '[]', $s); var_dump($r === null ? null : bin2hex($r));
}
$r = mb_ereg_replace('\B', '[]', "\x8f\xab", 'pz'); var_dump($r === null ? null : bin2hex($r));
$r = mb_ereg_replace('\B', '[]', "a\xa4", 'pz'); var_dump($r === null ? null : bin2hex($r));
$r = mb_ereg_replace('', '[]', "a\x8f\xab", 'pz'); var_dump($r === null ? null : bin2hex($r));
--EXPECT--
73533e698faba3
string(34) "735b5d5b5d533e695b5d5b5d8f5b5daba3"
string(34) "735b5d5b5d533e695b5d5b5d8f5b5daba3"
string(34) "735b5d5b5d533e695b5d5b5d8f5b5daba3"
string(34) "735b5d5b5d533e695b5d5b5d8f5b5daba3"
string(46) "5b5d735b5d535b5d3e5b5d695b5d8f5b5dab5b5da35b5d"
string(46) "5b5d735b5d535b5d3e5b5d695b5d8f5b5dab5b5da35b5d"
string(46) "5b5d735b5d535b5d3e5b5d695b5d8f5b5dab5b5da35b5d"
string(46) "5b5d735b5d535b5d3e5b5d695b5d8f5b5dab5b5da35b5d"
string(38) "5b5d73535b5d5b5d3e5b5d698faba35b5d5b5d"
string(38) "5b5d73535b5d5b5d3e5b5d698faba35b5d5b5d"
string(46) "5b5d735b5d535b5d3e5b5d695b5d8f5b5dab5b5da35b5d"
string(46) "5b5d735b5d535b5d3e5b5d695b5d8f5b5dab5b5da35b5d"
string(22) "73533e698faba35b5d5b5d"
string(22) "73533e698faba35b5d5b5d"
NULL
NULL
NULL
