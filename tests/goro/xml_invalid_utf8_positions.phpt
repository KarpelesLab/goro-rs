--TEST--
Invalid UTF-8 input: libxml2 reads each bad byte as one char (U+FFFD), byte index stays raw
--FILE--
<?php
foreach (["<a>\x80</a>", "<a>\xe9\xe9<b></a>", "<a>\x80\x81\x82<b></a>", "<a>\xc3\xa9\x80xyz\xe9\xe9<b></a>", "<a>x\xc3", "<a>x\xe2\x82", "<\xe9/>"] as $x) {
    foreach ([1, 100] as $ch) {
        $p = xml_parser_create();
        $o = [];
        xml_set_character_data_handler($p, function ($p, $d) use (&$o) { $o[] = bin2hex($d) . "@" . xml_get_current_byte_index($p); });
        foreach (str_split($x, $ch) as $c) { if (!xml_parse($p, $c, false)) break; }
        xml_parse($p, '', true);
        echo bin2hex($x), " $ch: ", xml_get_error_code($p), " ", xml_get_current_column_number($p), " ", xml_get_current_byte_index($p), " ", implode(",", $o), "\n";
    }
}
libxml_use_internal_errors(true);
foreach (["<a>\xe9</a>", "<a>\xe2\x82</a>", "<a b='\xe9'/>", "<!-- \xe9 --><a/>", "<a>\xc0\x80</a>", "<a>x\xe2\x82", "<\xe9/>"] as $x) {
    $d = new DOMDocument;
    $d->loadXML($x, LIBXML_RECOVER);
    foreach (libxml_get_errors() as $e) echo "  C{$e->code} col{$e->column}: ", trim($e->message), "\n";
    libxml_clear_errors();
    echo bin2hex($x), " => ", bin2hex($d->saveXML($d->documentElement)), "\n";
}
?>
--EXPECT--
3c613e803c2f613e 1: 81 5 4 
3c613e803c2f613e 100: 81 5 4 
3c613ee9e93c623e3c2f613e 1: 81 6 5 
3c613ee9e93c623e3c2f613e 100: 81 6 5 
3c613e8081823c623e3c2f613e 1: 81 7 6 
3c613e8081823c623e3c2f613e 100: 81 7 6 
3c613ec3a98078797ae9e93c623e3c2f613e 1: 81 11 11 
3c613ec3a98078797ae9e93c623e3c2f613e 100: 81 11 11 
3c613e78c3 1: 9 6 5 78@4
3c613e78c3 100: 9 6 5 78@4
3c613e78e282 1: 9 6 5 78@4
3c613e78e282 100: 9 6 5 78@4
3ce92f3e 1: 68 2 1 
3ce92f3e 100: 68 2 1 
  C81 col4: Invalid bytes in character encoding
3c613ee93c2f613e => 3c613eefbfbd3c2f613e
  C81 col4: Invalid bytes in character encoding
3c613ee2823c2f613e => 3c613eefbfbdefbfbd3c2f613e
  C81 col7: Invalid bytes in character encoding
3c6120623d27e9272f3e => 3c6120623d22efbfbd222f3e
  C81 col6: Invalid bytes in character encoding
3c212d2d20e9202d2d3e3c612f3e => 3c612f3e
  C81 col4: Invalid bytes in character encoding
3c613ec0803c2f613e => 3c613eefbfbdefbfbd3c2f613e
  C9 col5: Incomplete UTF-8 sequence starting with E2
  C9 col6: Incomplete UTF-8 sequence starting with 82
3c613e78e282 => 3c613e783c2f613e
  C81 col2: Invalid bytes in character encoding
  C68 col2: StartTag: invalid element name
3ce92f3e => 3c3f786d6c2076657273696f6e3d22312e30223f3e0a
