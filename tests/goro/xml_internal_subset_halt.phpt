--TEST--
Internal subset errors: entity declarations not terminated halt the parser; the final NEXT after the subset
--FILE--
<?php
libxml_use_internal_errors(true);
foreach (['<!DOCTYPE a [<!ENTITY e "x" junk>]><a><b></a>', '<!DOCTYPE a [<!ENTITY e "x"<!ELEMENT a ANY>]><a><b></a>', '<!DOCTYPE a [<!ENTITY e SYSTEM "x" NDATA>]><a><b></a>', "<!DOCTYPE a [<!ENTITY e \"a\0b\">]><a><b></a>", '<!DOCTYPE a [<!ENTITY % e "x" y>]><a><b></a>'] as $x) {
  echo json_encode($x), "\n";
  $d = new DOMDocument; $d->loadXML($x);
  foreach (libxml_get_errors() as $e) echo "  L{$e->level} C{$e->code} {$e->line}:{$e->column} ", trim($e->message), "\n";
  libxml_clear_errors();
  $p = xml_parser_create(); xml_parse($p, $x, true); echo "  xml ", xml_get_error_code($p), " ", xml_get_current_column_number($p), "\n";
  libxml_clear_errors();
}
foreach (['<!DOCTYPE a [<!ENTITY e "x" junk>]><a/>', '<!DOCTYPE a [<!ENTITY e "x" junk', "<!DOCTYPE a [\n<!ENTITY e \"x\"\n junk>]><a/>", '<!DOCTYPE a [<!ATTLIST a x CDATA>]><a/>', '<a><b></a>'] as $x) {
  $p = xml_parser_create(); xml_parse($p, $x, true);
  echo json_encode($x), " ", xml_get_error_code($p), " ", xml_get_current_line_number($p), ":", xml_get_current_column_number($p), "@", xml_get_current_byte_index($p), "\n";
}
?>
--EXPECT--
"<!DOCTYPE a [<!ENTITY e \"x\" junk>]><a><b><\/a>"
  L3 C37 1:28 xmlParseEntityDecl: entity e not terminated
  xml 37 29
"<!DOCTYPE a [<!ENTITY e \"x\"<!ELEMENT a ANY>]><a><b><\/a>"
  L3 C37 1:27 xmlParseEntityDecl: entity e not terminated
  xml 37 28
"<!DOCTYPE a [<!ENTITY e SYSTEM \"x\" NDATA>]><a><b><\/a>"
  L3 C65 1:41 Space required after 'NDATA'
  L3 C76 1:54 Opening and ending tag mismatch: b line 1 and a
  xml 65 44
"<!DOCTYPE a [<!ENTITY e \"a\u0000b\">]><a><b><\/a>"
  L3 C9 1:26 invalid character in entity value
  L3 C37 1:26 xmlParseEntityDecl: entity e not terminated
  xml 37 27
"<!DOCTYPE a [<!ENTITY % e \"x\" y>]><a><b><\/a>"
  L3 C37 1:30 xmlParseEntityDecl: entity e not terminated
  xml 37 31
"<!DOCTYPE a [<!ENTITY e \"x\" junk>]><a\/>" 37 1:29@29
"<!DOCTYPE a [<!ENTITY e \"x\" junk" 37 1:29@29
"<!DOCTYPE a [\n<!ENTITY e \"x\"\n junk>]><a\/>" 37 3:3@31
"<!DOCTYPE a [<!ATTLIST a x CDATA>]><a\/>" 65 1:36@35
"<a><b><\/a>" 76 1:11@10
