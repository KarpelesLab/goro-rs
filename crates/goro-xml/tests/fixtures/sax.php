<?php
// Fixture driver: ext/xml (libxml SAX implementation) on PHP 8.5.
// Output lines: "S <case>" header, then one line per event, JSON-encoded.
echo "IMPL ", XML_SAX_IMPL, "\n";
for ($i = 0; $i < 120; $i++) { echo "ERRSTR $i ", json_encode(xml_error_string($i)), "\n"; }
foreach ([200, 201, 202, 203, 204, 205, 500, 1000, 1549] as $i) echo "ERRSTR $i ", json_encode(xml_error_string($i)), "\n";

function ev() { global $log; $a = func_get_args(); array_shift($a); $log[] = $a; }
function run($label, $chunks, $opts = [], $ns = false, $enc = null, $handlers = 'all') {
  global $log; $log = [];
  $p = $ns ? ($enc === null ? xml_parser_create_ns() : xml_parser_create_ns($enc, $ns === true ? ':' : $ns))
           : ($enc === null ? xml_parser_create() : xml_parser_create($enc));
  foreach ($opts as $k => $v) xml_parser_set_option($p, $k, $v);
  if ($handlers === 'all' || str_contains($handlers, 'e')) xml_set_element_handler($p, function($p, $n, $a) { ev(0, 'start', $n, $a); }, function($p, $n) { ev(0, 'end', $n); });
  if ($handlers === 'all' || str_contains($handlers, 'c')) xml_set_character_data_handler($p, function($p, $d) { ev(0, 'cdata', $d); });
  if ($handlers === 'all' || str_contains($handlers, 'p')) xml_set_processing_instruction_handler($p, function($p, $t, $d) { ev(0, 'pi', $t, $d); });
  if ($handlers === 'all' || str_contains($handlers, 'd')) xml_set_default_handler($p, function($p, $d) { ev(0, 'default', $d); });
  if ($handlers === 'all' || str_contains($handlers, 'x')) xml_set_external_entity_ref_handler($p, function($p, $n, $b, $s, $pub) { ev(0, 'extent', $n, $b, $s, $pub); return true; });
  if ($handlers === 'all' || str_contains($handlers, 'u')) xml_set_unparsed_entity_decl_handler($p, function($p, $n, $b, $s, $pub, $no) { ev(0, 'unparsed', $n, $b, $s, $pub, $no); });
  if ($handlers === 'all' || str_contains($handlers, 'n')) xml_set_notation_decl_handler($p, function($p, $n, $b, $s, $pub) { ev(0, 'notation', $n, $b, $s, $pub); });
  if ($handlers === 'all' || str_contains($handlers, 's')) { xml_set_start_namespace_decl_handler($p, function($p, $pf, $u) { ev(0, 'startns', $pf, $u); }); xml_set_end_namespace_decl_handler($p, function($p, $pf) { ev(0, 'endns', $pf); }); }
  echo "S ", json_encode($label), " ", implode(",", array_map("base64_encode", $chunks)), " ", json_encode((object)$opts), " ", json_encode($ns), " ", json_encode($enc), " ", $handlers, "\n";
  $n = count($chunks);
  foreach ($chunks as $i => $c) {
    $r = xml_parse($p, $c, $i == $n - 1);
    $log[] = ['parse', $r, xml_get_error_code($p), xml_get_current_line_number($p), xml_get_current_column_number($p), xml_get_current_byte_index($p)];
    if (!$r) break;
  }
  foreach ($log as $l) echo "  ", json_encode($l, JSON_INVALID_UTF8_SUBSTITUTE), "\n";
  // into_struct
  $p2 = $ns ? xml_parser_create_ns() : xml_parser_create();
  foreach ($opts as $k => $v) xml_parser_set_option($p2, $k, $v);
  $vals = $idx = null;
  $r = xml_parse_into_struct($p2, implode('', $chunks), $vals, $idx);
  echo "  STRUCT ", json_encode([$r, $vals, $idx], JSON_INVALID_UTF8_SUBSTITUTE), "\n";
}
run('basic', ["<?xml version='1.0'?>\n<root a='1' B='x'>\n text <child/>&amp;&lt;more<!-- c --><?pi data ?><![CDATA[cd<a>]]></root>\n"]);
run('nofold', ["<Root A='1'><Sub/></Root>"], [XML_OPTION_CASE_FOLDING => 0]);
run('chunks', ["<root>ab", "cd<x", " y='1'/>e", "f</root>"]);
run('err-mismatch', ["<a><b></a>"]);
run('err-eof', ["<a>"]);
run('err-empty', [""]);
run('err-decl', ["<?xml>"]);
run('err-entity', ["<a>&foo;</a>"]);
run('err-junk', ["<a/>junk"]);
run('err-attr', ["<a x=1/>"]);
run('lines', ["<a>\n<b>\n</c>\n</a>"]);
run('ent', ["<!DOCTYPE a [<!ENTITY e 'xyz'><!ENTITY f SYSTEM 'f.xml'><!NOTATION n SYSTEM 'nn'><!ENTITY u SYSTEM 'u.bin' NDATA n>]><a>&e;&f;</a>"]);
run('ns', ["<a xmlns='urn:d' xmlns:p='urn:p'><p:b p:x='1' y='2'/><c/></a>"], [], true);
run('ns-sep', ["<a xmlns:p='urn:p'><p:b/></a>"], [], '#');
run('skipwhite', ["<a>\n  <b> x </b>\n  \n</a>"], [XML_OPTION_SKIP_WHITE => 1]);
run('tagstart', ["<abcd><abef/></abcd>"], [XML_OPTION_SKIP_TAGSTART => 2]);
run('latin1-out', ["<a x='\u{e9}'>\u{e9}\u{20ac}</a>"], [XML_OPTION_TARGET_ENCODING => 'ISO-8859-1']);
run('ascii-out', ["<a>\u{e9}</a>"], [XML_OPTION_TARGET_ENCODING => 'US-ASCII']);
run('latin1-in', ["<a>\xe9</a>"], [], false, 'ISO-8859-1');
run('latin1-decl', ["<?xml version='1.0' encoding='ISO-8859-1'?><a>\xe9</a>"]);
run('default-only', ["<?xml version='1.0'?><!DOCTYPE a><a x='1'>t&amp;<!--c--><?p d?></a>"], [], false, null, 'd');
run('no-handlers', ["<a>t</a>"], [], false, null, '');
run('cdata-only', ["<a>t&amp;u<b/>v\n</a>"], [], false, null, 'c');
run('struct', ["<a><b>1</b><b>2</b><c><d/></c>t</a>"]);
run('struct2', ["<a> <b x='1'>t<c/>u</b> </a>"]);
run('attrs-dup', ["<a x='1' X='2'/>"]);
run('bom', ["\xef\xbb\xbf<a>x</a>"]);
run('utf8-invalid', ["<a>\xff</a>"]);
run('pi-xml', ["<a><?xml-stylesheet href='x'?></a>"]);
run('crlf', ["<a>x\r\ny\rz</a>"]);
run('charref', ["<a>&#65;&#x42;&#233;</a>"]);
run('entity-undefined-dtd', ["<!DOCTYPE a SYSTEM 'x.dtd'><a>&e;</a>"]);
run('chunk-split-utf8', ["<a>\xc3", "\xa9</a>"]);
run('huge-depth', [str_repeat('<a>', 300) . str_repeat('</a>', 300)]);
// Byte index details
$p = xml_parser_create();
xml_set_element_handler($p, function($p, $n) { echo "  idx start $n ", xml_get_current_byte_index($p), " ", xml_get_current_line_number($p), " ", xml_get_current_column_number($p), "\n"; }, function($p, $n) { echo "  idx end $n ", xml_get_current_byte_index($p), " ", xml_get_current_column_number($p), "\n"; });
xml_set_character_data_handler($p, function($p, $d) { echo "  idx cdata ", json_encode($d), " ", xml_get_current_byte_index($p), " ", xml_get_current_column_number($p), "\n"; });
echo "S \"byteidx\"\n";
xml_parse($p, "<root>\n  <a x='1'>text</a>\n  <b/>\n</root>", true);
// options get
$p = xml_parser_create();
foreach ([XML_OPTION_CASE_FOLDING, XML_OPTION_TARGET_ENCODING, XML_OPTION_SKIP_TAGSTART, XML_OPTION_SKIP_WHITE, XML_OPTION_PARSE_HUGE] as $o) echo "OPT $o ", json_encode(xml_parser_get_option($p, $o)), "\n";
$p = xml_parser_create('ISO-8859-1'); echo "OPT enc-latin1 ", json_encode(xml_parser_get_option($p, XML_OPTION_TARGET_ENCODING)), "\n";
$p = xml_parser_create('US-ASCII'); echo "OPT enc-ascii ", json_encode(xml_parser_get_option($p, XML_OPTION_TARGET_ENCODING)), "\n";
