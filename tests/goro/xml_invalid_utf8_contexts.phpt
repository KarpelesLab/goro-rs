--TEST--
Invalid UTF-8 in the XML declaration and attribute values; %.50s message excerpts count bytes
--FILE--
<?php
libxml_use_internal_errors(true);
$cases = [
    "<?xml version\xe9\"1.0\"?>\n<a/>",
    "<?xml version=\"1.0\"\xe9encoding=\"UTF-8\"?><a/>",
    "<?xml version=\"1.0\" x\xe9\xe9?><a>\xe9</a>",
    "<a><b x=\"\xc3",
    "<a><b x=\"\xe2\x82",
    "<a><!--" . str_repeat("\xc3\xa9", 30) . "<b/></a>",
    "<a><![CDATA[" . str_repeat("\xc3\xa9", 30),
];
foreach ($cases as $x) {
    echo json_encode(bin2hex($x)), "\n";
    $d = new DOMDocument;
    $d->loadXML($x);
    foreach (libxml_get_errors() as $e) echo "  L{$e->level} C{$e->code} {$e->line}:{$e->column} ", json_encode($e->message), "\n";
    libxml_clear_errors();
    $p = xml_parser_create();
    xml_parse($p, $x, true);
    echo "  xml ", xml_get_error_code($p), " ", xml_get_current_column_number($p), " ", xml_get_current_byte_index($p), "\n";
    libxml_clear_errors();
}
?>
--EXPECT--
"3c3f786d6c2076657273696f6ee922312e30223f3e0a3c612f3e"
  L3 C75 1:14 "expected '='\n"
  L3 C96 1:14 "Malformed declaration expecting version\n"
  L3 C65 1:14 "Blank needed here\n"
  L3 C57 1:14 "parsing XML declaration: '?>' expected\n"
  L3 C81 1:15 "Invalid bytes in character encoding\n"
  xml 81 22 21
"3c3f786d6c2076657273696f6e3d22312e3022e9656e636f64696e673d225554462d38223f3e3c612f3e"
  L3 C65 1:20 "Blank needed here\n"
  L3 C57 1:20 "parsing XML declaration: '?>' expected\n"
  L3 C81 1:21 "Invalid bytes in character encoding\n"
  xml 81 39 38
"3c3f786d6c2076657273696f6e3d22312e30222078e9e93f3e3c613ee93c2f613e"
  L3 C57 1:21 "parsing XML declaration: '?>' expected\n"
  L3 C81 1:23 "Invalid bytes in character encoding\n"
  xml 81 26 25
"3c613e3c6220783d22c3"
  L3 C81 1:10 "Invalid bytes in character encoding\n"
  L3 C40 1:11 "AttValue: ' expected\n"
  L3 C65 1:11 "attributes construct error\n"
  L3 C73 1:11 "Couldn't find end of Start Tag b line 1\n"
  xml 73 11 10
"3c613e3c6220783d22e282"
  L3 C81 1:10 "Invalid bytes in character encoding\n"
  L3 C40 1:12 "AttValue: ' expected\n"
  L3 C65 1:12 "attributes construct error\n"
  L3 C73 1:12 "Couldn't find end of Start Tag b line 1\n"
  xml 73 12 11
"3c613e3c212d2dc3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a93c622f3e3c2f613e"
  L3 C45 1:46 "Comment not terminated \n<!--\u00e9\u00e9\u00e9\u00e9\u00e9\u00e9\u00e9\u00e9\u00e9\u00e9\u00e9\u00e9\u00e9\u00e9\u00e9\u00e9\u00e9\u00e9\u00e9\u00e9\u00e9\u00e9\u00e9\u00e9\u00e9\n"
  xml 45 46 75
"3c613e3c215b43444154415bc3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9"
  L3 C63 1:43 "CData section not finished\n\u00e9\u00e9\u00e9\u00e9\u00e9\u00e9\u00e9\u00e9\u00e9\u00e9\u00e9\u00e9\u00e9\u00e9\u00e9\u00e9\u00e9\u00e9\u00e9\u00e9\u00e9\u00e9\u00e9\u00e9\u00e9\n"
  xml 63 43 72
