--TEST--
ext/xml (push parser) CDATA sections: raw bytes, 300-byte pieces, encoding check, a column per byte
--FILE--
<?php
libxml_use_internal_errors(true);
foreach (["<a><![CDATA[\0]]></a>", "<a><![CDATA[x\x01y]]></a>", "<a><![CDATA[\xe9]]></a>", "<a><![CDATA[ok\xc3\xa9]]></a>", "<a><![CDATA[x\xff\xfe\xfd\xfc]]></a>", "<a><![CDATA[x]]></a>\x01", "<a>\x01</a>"] as $x) {
  foreach ([1, 1000] as $ch) {
    $p = xml_parser_create();
    xml_set_character_data_handler($p, function ($p, $d) { echo "[", bin2hex($d), "]"; });
    foreach (str_split($x, $ch) as $c) if (!xml_parse($p, $c, false)) break;
    xml_parse($p, '', true);
    echo json_encode(bin2hex($x)), " $ch: ", xml_get_error_code($p), " ", xml_get_current_column_number($p), "@", xml_get_current_byte_index($p), "\n";
    foreach (libxml_get_errors() as $e) echo "  L{$e->level} C{$e->code} {$e->line}:{$e->column} ", json_encode($e->message), "\n";
    libxml_clear_errors();
  }
}
foreach (["<a><![CDATA[x\r\ny\rz\n]]>\r\n<b/></a>", "<a><![CDATA[" . str_repeat("abcdefghij", 70) . "]]></a>", "<a><![CDATA[\xc3\xa9\xc3\xa9]]><b/></a>", "<a><![CDATA[]]></a>", "<a><![CDATA[x]]]]></a>"] as $x) {
  foreach ([1, 7, 100000] as $ch) {
    $p = xml_parser_create();
    $ev = [];
    xml_set_character_data_handler($p, function ($p, $d) use (&$ev) { $ev[] = strlen($d) > 20 ? strlen($d) . 'b' : bin2hex($d); });
    xml_set_element_handler($p, function ($p, $n) use (&$ev) { $ev[] = "S$n@" . xml_get_current_line_number($p) . ':' . xml_get_current_column_number($p) . '@' . xml_get_current_byte_index($p); }, function () {});
    foreach (str_split($x, $ch) as $c) if (!xml_parse($p, $c, false)) break;
    xml_parse($p, '', true);
    echo "$ch: ", implode(' ', $ev), " | ", xml_get_error_code($p), " ", xml_get_current_line_number($p), ':', xml_get_current_column_number($p), "@", xml_get_current_byte_index($p), "\n";
  }
  $d = new DOMDocument; $d->loadXML($x); echo "  dom ", bin2hex(substr($d->documentElement->textContent, 0, 20)), "\n";
}
?>
--EXPECT--
"3c613e3c215b43444154415b005d5d3e3c2f613e" 1: 81 13@12
  L3 C81 1:13 "Invalid bytes in character encoding\n"
"3c613e3c215b43444154415b005d5d3e3c2f613e" 1000: 81 13@12
  L3 C81 1:13 "Invalid bytes in character encoding\n"
"3c613e3c215b43444154415b7801795d5d3e3c2f613e" 1: 81 13@13
  L3 C81 1:13 "Invalid bytes in character encoding\n"
"3c613e3c215b43444154415b7801795d5d3e3c2f613e" 1000: 81 13@13
  L3 C81 1:13 "Invalid bytes in character encoding\n"
"3c613e3c215b43444154415be95d5d3e3c2f613e" 1: 81 13@12
  L3 C81 1:13 "Invalid bytes in character encoding\n"
"3c613e3c215b43444154415be95d5d3e3c2f613e" 1000: 81 13@12
  L3 C81 1:13 "Invalid bytes in character encoding\n"
[6f6bc3a9]"3c613e3c215b43444154415b6f6bc3a95d5d3e3c2f613e" 1: 0 24@23
[6f6bc3a9]"3c613e3c215b43444154415b6f6bc3a95d5d3e3c2f613e" 1000: 0 24@23
"3c613e3c215b43444154415b78fffefdfc5d5d3e3c2f613e" 1: 81 13@13
  L3 C81 1:13 "Invalid bytes in character encoding\n"
"3c613e3c215b43444154415b78fffefdfc5d5d3e3c2f613e" 1000: 81 13@13
  L3 C81 1:13 "Invalid bytes in character encoding\n"
[78]"3c613e3c215b43444154415b785d5d3e3c2f613e01" 1: 5 21@20
  L3 C5 1:21 "Extra content at the end of the document\n"
[78]"3c613e3c215b43444154415b785d5d3e3c2f613e01" 1000: 5 21@20
  L3 C5 1:21 "Extra content at the end of the document\n"
"3c613e013c2f613e" 1: 9 5@4
  L3 C9 1:4 "PCDATA invalid Char value 1\n"
"3c613e013c2f613e" 1000: 9 5@4
  L3 C9 1:4 "PCDATA invalid Char value 1\n"
1: SA@1:3@2 780d0a790d7a0a 0a SB@4:3@26 | 0 4:9@32
7: SA@1:3@2 780d0a790d7a0a 0a SB@4:3@26 | 0 4:9@32
100000: SA@1:3@2 780d0a790d7a0a 0a SB@4:3@26 | 0 4:9@32
  dom 780a790a7a0a0a
1: SA@1:3@2 300b 300b 100b | 0 1:720@719
7: SA@1:3@2 300b 300b 100b | 0 1:720@719
100000: SA@1:3@2 700b | 0 1:720@719
  dom 6162636465666768696a6162636465666768696a
1: SA@1:3@2 c3a9c3a9 SB@1:22@21 | 0 1:28@27
7: SA@1:3@2 c3a9c3a9 SB@1:22@21 | 0 1:28@27
100000: SA@1:3@2 c3a9c3a9 SB@1:22@21 | 0 1:28@27
  dom c3a9c3a9
1: SA@1:3@2  | 0 1:20@19
7: SA@1:3@2  | 0 1:20@19
100000: SA@1:3@2  | 0 1:20@19
  dom 
1: SA@1:3@2 785d5d | 0 1:23@22
7: SA@1:3@2 785d5d | 0 1:23@22
100000: SA@1:3@2 785d5d | 0 1:23@22
  dom 785d5d
