--TEST--
A CR LF pair followed by non-ASCII text stays in one character data chunk
--FILE--
<?php
libxml_use_internal_errors(true);
$cases = ["<a>\r\n\xc3\xa9</a>", "<a>x\r\n\xc3\xa9y</a>", "<a>\r\n\r\n\xc3\xa9</a>", "<a>\r\xc3\xa9</a>", "<a>\n\xc3\xa9</a>", "<a> \r\n \xc3\xa9</a>", "<a>\xc3\xa9\r\n\xc3\xa9</a>", "<a>\r\n<b/>\r\n</a>", "<a>x\r\ny</a>", "<a>\r\n&amp;</a>"];
foreach ($cases as $x) {
  echo json_encode($x), "\n ";
  $p = xml_parser_create();
  xml_set_character_data_handler($p, function ($p, $d) { echo json_encode($d), "@", xml_get_current_byte_index($p), " "; });
  xml_parse($p, $x, true);
  echo xml_get_error_code($p), "\n";
  $d = new DOMDocument; $d->loadXML($x);
  foreach (libxml_get_errors() as $e) echo "  L{$e->level} C{$e->code} {$e->line}:{$e->column} ", trim($e->message), "\n";
  libxml_clear_errors();
}
?>
--EXPECT--
"<a>\r\n\u00e9<\/a>"
 "\n\u00e9"@7 0
"<a>x\r\n\u00e9y<\/a>"
 "x"@4 "\n\u00e9y"@9 0
"<a>\r\n\r\n\u00e9<\/a>"
 "\n\n\u00e9"@9 0
"<a>\r\u00e9<\/a>"
 "\n\u00e9"@6 0
"<a>\n\u00e9<\/a>"
 "\n"@4 "\u00e9"@6 0
"<a> \r\n \u00e9<\/a>"
 " "@4 "\n "@7 "\u00e9"@9 0
"<a>\u00e9\r\n\u00e9<\/a>"
 "\u00e9\n\u00e9"@9 0
"<a>\r\n<b\/>\r\n<\/a>"
 "\n"@5 "\n"@11 0
"<a>x\r\ny<\/a>"
 "x"@4 "\ny"@7 0
"<a>\r\n&amp;<\/a>"
 "\n"@5 "&"@10 0
