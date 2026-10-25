--TEST--
libxml2 column quirk of internal entity values
--FILE--
<?php
foreach (['<!DOCTYPE r [<!ENTITY e "ee">]><r/>', '<!DOCTYPE r [<!ENTITY e "x"><!ENTITY f "y">]><r/>', '<!DOCTYPE r [<!ENTITY % pe "x">]><r/>', "<!DOCTYPE r [<!ENTITY e \"a\xe2\x82\xacb\">]><r/>", "<!DOCTYPE r [<!ENTITY e \"a\nb\">]><r/>", '<!DOCTYPE r [<!ENTITY e SYSTEM "x">]><r/>'] as $s) {
    $p = xml_parser_create();
    xml_parse($p, $s, true);
    echo strlen($s), " ", xml_get_current_line_number($p), " ", xml_get_current_column_number($p), " ", xml_get_current_byte_index($p), "\n";
}
libxml_use_internal_errors(true);
$d = new DOMDocument;
$d->loadXML("<!DOCTYPE r [<!ENTITY e \"x\">]><r><a></b></r>");
$d->loadXML("<!DOCTYPE r [<!ENTITY e \"\xc3\xa9\xc3\xa9\">]><r><a></b></r>");
foreach (libxml_get_errors() as $e) echo $e->line, ":", $e->column, " ", $e->message;
--EXPECT--
35 1 35 35
49 1 48 49
37 1 37 37
38 1 38 38
36 2 10 36
41 1 42 41
1:40 Opening and ending tag mismatch: a line 1 and b
1:43 Opening and ending tag mismatch: a line 1 and b
