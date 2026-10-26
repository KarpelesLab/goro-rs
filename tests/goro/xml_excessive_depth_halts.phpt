--TEST--
Excessive element depth: the parse halts (no further errors), the tree builder's limit applies to XMLReader
--FILE--
<?php
libxml_use_internal_errors(true);
$deep = str_repeat('<a>', 300) . str_repeat('</a>', 300);
$d = new DOMDocument;
var_dump($d->loadXML($deep));
foreach (libxml_get_errors() as $e) echo $e->code, ' ', trim($e->message), "\n";
libxml_clear_errors();
$r = XMLReader::XML($deep);
$n = 0;
while (@$r->read()) $n++;
var_dump($n);
foreach (libxml_get_errors() as $e) echo $e->code, ' ', trim($e->message), "\n";
libxml_clear_errors();
foreach ([1, 1000000] as $ch) {
    $p = xml_parser_create();
    $max = 0; $depth = 0;
    xml_set_element_handler($p, function () use (&$depth, &$max) { $depth++; $max = max($max, $depth); }, function () use (&$depth) { $depth--; });
    foreach (str_split($deep, $ch) as $c) if (!xml_parse($p, $c, false)) break;
    xml_parse($p, '', true);
    var_dump($max, xml_get_error_code($p), xml_get_current_column_number($p));
}
$x = @simplexml_load_string($deep);
var_dump($x);
?>
--EXPECT--
bool(false)
114 Excessive depth in document: 257 use XML_PARSE_HUGE option
int(172)
114 Excessive depth in document: 257 use XML_PARSE_HUGE option
int(300)
int(0)
int(2101)
int(300)
int(0)
int(2101)
bool(false)
