--TEST--
NUL bytes in XML input: xmlCurrentChar error, comments/PI/XML decl/start tag handling
--FILE--
<?php
libxml_use_internal_errors(true);
foreach (["<a>\0</a>", "<a>x\0y</a>", "<a>\0\0</a>", "<a x='\0'/>", "<a x='a\0b'/>", "<!--\0--><a/>", "<?p \0?><a/>", "<a><![CDATA[\0]]></a>", "<a \0/>", "<a>&\0;</a>", "<a/>\0", "<a/> \0", "<a>\xc3\xa9\0</a>", "\0<a/>", "<?xml version='1.0'\0?><a/>", "<!DOCTYPE a [\0]><a/>", "<a></a\0>", "<a>\0</a>\0"] as $x) {
  echo json_encode($x), "\n";
  foreach ([0, 1] as $rec) {
    $d = new DOMDocument; $d->loadXML($x, $rec);
    foreach (libxml_get_errors() as $e) echo "  $rec L{$e->level} C{$e->code} {$e->line}:{$e->column} ", trim($e->message), "\n";
    libxml_clear_errors();
    if ($rec) echo "  ", json_encode($d->saveXML()), "\n";
  }
  $p = xml_parser_create(); xml_parse($p, $x, true); echo "  xml ", xml_get_error_code($p), " ", xml_get_current_column_number($p), " ", xml_get_current_byte_index($p), "\n";
  libxml_clear_errors();
}
?>
--EXPECT--
"<a>\u0000<\/a>"
  0 L3 C9 1:4 Invalid character: Char 0x0 out of allowed range
  0 L3 C9 1:4 PCDATA invalid Char value 0
  1 L3 C9 1:4 Invalid character: Char 0x0 out of allowed range
  1 L3 C9 1:4 PCDATA invalid Char value 0
  "<?xml version=\"1.0\"?>\n<a\/>\n"
  xml 9 5 4
"<a>x\u0000y<\/a>"
  0 L3 C9 1:5 Invalid character: Char 0x0 out of allowed range
  0 L3 C9 1:5 PCDATA invalid Char value 0
  1 L3 C9 1:5 Invalid character: Char 0x0 out of allowed range
  1 L3 C9 1:5 PCDATA invalid Char value 0
  "<?xml version=\"1.0\"?>\n<a>xy<\/a>\n"
  xml 9 6 5
"<a>\u0000\u0000<\/a>"
  0 L3 C9 1:4 Invalid character: Char 0x0 out of allowed range
  0 L3 C9 1:4 PCDATA invalid Char value 0
  0 L3 C9 1:5 Invalid character: Char 0x0 out of allowed range
  0 L3 C9 1:5 PCDATA invalid Char value 0
  1 L3 C9 1:4 Invalid character: Char 0x0 out of allowed range
  1 L3 C9 1:4 PCDATA invalid Char value 0
  1 L3 C9 1:5 Invalid character: Char 0x0 out of allowed range
  1 L3 C9 1:5 PCDATA invalid Char value 0
  "<?xml version=\"1.0\"?>\n<a\/>\n"
  xml 9 5 4
"<a x='\u0000'\/>"
  0 L3 C9 1:7 invalid character in attribute value
  1 L3 C9 1:7 invalid character in attribute value
  "<?xml version=\"1.0\"?>\n<a x=\"&#xFFFD;\"\/>\n"
  xml 9 11 10
"<a x='a\u0000b'\/>"
  0 L3 C9 1:8 invalid character in attribute value
  1 L3 C9 1:8 invalid character in attribute value
  "<?xml version=\"1.0\"?>\n<a x=\"a&#xFFFD;b\"\/>\n"
  xml 9 13 12
"<!--\u0000--><a\/>"
  0 L3 C9 1:5 Invalid character: Char 0x0 out of allowed range
  0 L3 C45 1:5 Comment not terminated
  1 L3 C9 1:5 Invalid character: Char 0x0 out of allowed range
  1 L3 C45 1:5 Comment not terminated
  "<?xml version=\"1.0\"?>\n"
  xml 45 5 4
"<?p \u0000?><a\/>"
  0 L3 C9 1:5 Invalid character: Char 0x0 out of allowed range
  0 L3 C47 1:5 ParsePI: PI p never end ...
  1 L3 C9 1:5 Invalid character: Char 0x0 out of allowed range
  1 L3 C47 1:5 ParsePI: PI p never end ...
  "<?xml version=\"1.0\"?>\n"
  xml 47 5 4
"<a><![CDATA[\u0000]]><\/a>"
  0 L3 C9 1:13 Invalid character: Char 0x0 out of allowed range
  0 L3 C63 1:13 Unregistered error message
  0 L3 C9 1:13 Invalid character: Char 0x0 out of allowed range
  0 L3 C9 1:13 PCDATA invalid Char value 0
  0 L3 C62 1:14 Sequence ']]>' not allowed in content
  1 L3 C9 1:13 Invalid character: Char 0x0 out of allowed range
  1 L3 C63 1:13 Unregistered error message
  1 L3 C9 1:13 Invalid character: Char 0x0 out of allowed range
  1 L3 C9 1:13 PCDATA invalid Char value 0
  1 L3 C62 1:14 Sequence ']]>' not allowed in content
  "<?xml version=\"1.0\"?>\n<a>]]&gt;<\/a>\n"
  xml 63 13 12
"<a \u0000\/>"
  0 L3 C73 1:4 Couldn't find end of Start Tag a line 1
  1 L3 C73 1:4 Couldn't find end of Start Tag a line 1
  "<?xml version=\"1.0\"?>\n<a\/>\n"
  xml 73 4 3
"<a>&\u0000;<\/a>"
  0 L3 C9 1:5 Invalid character: Char 0x0 out of allowed range
  0 L3 C68 1:5 xmlParseEntityRef: no name
  0 L3 C9 1:5 Invalid character: Char 0x0 out of allowed range
  0 L3 C9 1:5 PCDATA invalid Char value 0
  1 L3 C9 1:5 Invalid character: Char 0x0 out of allowed range
  1 L3 C68 1:5 xmlParseEntityRef: no name
  1 L3 C9 1:5 Invalid character: Char 0x0 out of allowed range
  1 L3 C9 1:5 PCDATA invalid Char value 0
  "<?xml version=\"1.0\"?>\n<a>;<\/a>\n"
  xml 68 5 4
"<a\/>\u0000"
  0 L3 C5 1:5 Extra content at the end of the document
  1 L3 C5 1:5 Extra content at the end of the document
  "<?xml version=\"1.0\"?>\n<a\/>\n"
  xml 5 5 4
"<a\/> \u0000"
  0 L3 C5 1:6 Extra content at the end of the document
  1 L3 C5 1:6 Extra content at the end of the document
  "<?xml version=\"1.0\"?>\n<a\/>\n"
  xml 5 6 5
"<a>\u00e9\u0000<\/a>"
  0 L3 C9 1:5 Invalid character: Char 0x0 out of allowed range
  0 L3 C9 1:5 PCDATA invalid Char value 0
  1 L3 C9 1:5 Invalid character: Char 0x0 out of allowed range
  1 L3 C9 1:5 PCDATA invalid Char value 0
  "<?xml version=\"1.0\"?>\n<a>&#xE9;<\/a>\n"
  xml 9 6 6
"\u0000<a\/>"
  0 L3 C4 1:1 Document is empty
  1 L3 C4 1:1 Document is empty
  "<?xml version=\"1.0\"?>\n"
  xml 4 1 0
"<?xml version='1.0'\u0000?><a\/>"
  0 L3 C65 1:20 Blank needed here
  0 L3 C57 1:20 parsing XML declaration: '?>' expected
  1 L3 C65 1:20 Blank needed here
  1 L3 C57 1:20 parsing XML declaration: '?>' expected
  "<?xml version=\"1.0\"?>\n"
  xml 57 20 19
"<!DOCTYPE a [\u0000]><a\/>"
  0 L3 C118 1:14 Content error in the internal subset
  1 L3 C118 1:14 Content error in the internal subset
  "<?xml version=\"1.0\"?>\n<!DOCTYPE a>\n"
  xml 118 14 13
"<a><\/a\u0000>"
  0 L3 C9 1:7 Invalid character: Char 0x0 out of allowed range
  0 L3 C73 1:7 expected '>'
  1 L3 C9 1:7 Invalid character: Char 0x0 out of allowed range
  1 L3 C73 1:7 expected '>'
  "<?xml version=\"1.0\"?>\n<a\/>\n"
  xml 73 7 6
"<a>\u0000<\/a>\u0000"
  0 L3 C9 1:4 Invalid character: Char 0x0 out of allowed range
  0 L3 C9 1:4 PCDATA invalid Char value 0
  1 L3 C9 1:4 Invalid character: Char 0x0 out of allowed range
  1 L3 C9 1:4 PCDATA invalid Char value 0
  "<?xml version=\"1.0\"?>\n<a\/>\n"
  xml 9 5 4
