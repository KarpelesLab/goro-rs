--TEST--
Names read the complex way skip the CR of a CR LF pair (byte index after a failed entity name)
--FILE--
<?php
foreach (["<a>\xc3\xa9&lt;&\r\n</a>", "<a>\xc3\xa9&\r\n</a>", "<a>\xc3\xa9&lt;&</a>", "<a>&lt;&\r\n</a>", "<a>\xc3\xa9x&</a>", "<a>\xc3\xa9&lt;<</a>", "<a>\xc3\xa9\r\n&</a>", "<a>x\r\n&</a>", "<a>\xc3\xa9<!-- -->&</a>"] as $x) {
  $p = xml_parser_create();
  xml_set_character_data_handler($p, function($p, $d) { echo "[", json_encode($d), "@", xml_get_current_byte_index($p), "]"; });
  xml_parse($p, $x, true);
  echo json_encode($x), " ", xml_get_error_code($p), " ", xml_get_current_column_number($p), " ", xml_get_current_byte_index($p), "\n";
}
?>
--EXPECT--
["\u00e9"@5]["<"@9]"<a>\u00e9&lt;&\r\n<\/a>" 68 10 11
["\u00e9"@5]"<a>\u00e9&\r\n<\/a>" 68 6 7
["\u00e9"@5]["<"@9]"<a>\u00e9&lt;&<\/a>" 68 10 10
["<"@7]"<a>&lt;&\r\n<\/a>" 68 9 9
["\u00e9x"@6]"<a>\u00e9x&<\/a>" 68 7 7
["\u00e9"@5]["<"@9]"<a>\u00e9&lt;<<\/a>" 68 10 10
["\u00e9\n"@7]"<a>\u00e9\r\n&<\/a>" 68 2 8
["x"@4]["\n"@6]"<a>x\r\n&<\/a>" 68 2 7
["\u00e9"@5]"<a>\u00e9<!-- -->&<\/a>" 68 14 14
