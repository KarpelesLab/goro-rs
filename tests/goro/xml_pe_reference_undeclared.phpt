--TEST--
Undeclared parameter entities: xmlHandleUndeclaredEntity(); ext/xml never resolves them
--FILE--
<?php
libxml_use_internal_errors(true);
$docs = ['<!DOCTYPE a [%q;]><a/>', '<!DOCTYPE a [<!ENTITY % pe "<!ELEMENT a ANY>"> %pe;]><a/>', '<!DOCTYPE a [<!ENTITY % pe "x"> %pe; %q;]><a>&e;</a>', '<!DOCTYPE a [<!ENTITY % p SYSTEM "nope.ent"> %p;]><a/>', '<!DOCTYPE a SYSTEM "x.dtd" [%q;]><a/>', "<?xml version='1.0' standalone='yes'?><!DOCTYPE a [%q;]><a/>"];
foreach ($docs as $x) {
  echo json_encode($x), "\n";
  foreach ([0, LIBXML_NOENT, LIBXML_DTDLOAD, LIBXML_DTDVALID] as $o) {
    $d = new DOMDocument; $d->loadXML($x, $o);
    foreach (libxml_get_errors() as $e) echo "  $o L{$e->level} C{$e->code} {$e->line}:{$e->column} ", str_replace([__DIR__ . "/", getcwd() . "/"], "DIR/", trim($e->message)), "\n";
    libxml_clear_errors();
  }
  $p = xml_parser_create(); $r = xml_parse($p, $x, true); echo "  xml r=$r e=", xml_get_error_code($p), "\n";
  foreach (libxml_get_errors() as $e) echo "  x L{$e->level} C{$e->code} {$e->line}:{$e->column} ", str_replace([__DIR__ . "/", getcwd() . "/"], "DIR/", trim($e->message)), "\n";
  libxml_clear_errors();
}
?>
--EXPECT--
"<!DOCTYPE a [%q;]><a\/>"
  0 L1 C27 1:17 Entity 'q' not defined
  2 L2 C27 1:17 Entity 'q' not defined
  4 L2 C27 1:17 Entity 'q' not defined
  16 L2 C26 1:17 Entity 'q' not defined
  16 L2 C522 1:21 Validation failed: no DTD found !
  xml r=0 e=27
  x L2 C27 1:17 Entity 'q' not defined
"<!DOCTYPE a [<!ENTITY % pe \"<!ELEMENT a ANY>\"> %pe;]><a\/>"
  xml r=0 e=27
  x L2 C27 1:51 Entity 'pe' not defined
"<!DOCTYPE a [<!ENTITY % pe \"x\"> %pe; %q;]><a>&e;<\/a>"
  0 L3 C118 1:36 Content error in the internal subset
  2 L3 C118 1:36 Content error in the internal subset
  4 L3 C118 1:36 Content error in the internal subset
  16 L3 C118 1:36 Content error in the internal subset
  xml r=0 e=27
  x L2 C27 1:36 Entity 'pe' not defined
  x L2 C27 1:40 Entity 'q' not defined
  x L2 C27 1:48 Entity 'e' not defined
"<!DOCTYPE a [<!ENTITY % p SYSTEM \"nope.ent\"> %p;]><a\/>"
  2 L1 C1524 1:49 failed to load "DIR/nope.ent": No such file or directory
  4 L1 C1524 1:49 failed to load "DIR/nope.ent": No such file or directory
  16 L2 C1524 1:49 failed to load "DIR/nope.ent": No such file or directory
  16 L2 C522 1:53 Validation failed: no DTD found !
  xml r=0 e=27
  x L2 C27 1:49 Entity 'p' not defined
"<!DOCTYPE a SYSTEM \"x.dtd\" [%q;]><a\/>"
  0 L1 C27 1:32 Entity 'q' not defined
  2 L2 C27 1:32 Entity 'q' not defined
  4 L2 C27 1:32 Entity 'q' not defined
  4 L1 C1524 1:34 failed to load "DIR/x.dtd": No such file or directory
  16 L2 C26 1:32 Entity 'q' not defined
  16 L2 C1524 1:34 failed to load "DIR/x.dtd": No such file or directory
  16 L2 C522 1:36 Validation failed: no DTD found !
  xml r=0 e=27
  x L2 C27 1:32 Entity 'q' not defined
"<?xml version='1.0' standalone='yes'?><!DOCTYPE a [%q;]><a\/>"
  0 L3 C26 1:55 Entity 'q' not defined
  2 L3 C26 1:55 Entity 'q' not defined
  4 L3 C26 1:55 Entity 'q' not defined
  16 L3 C26 1:55 Entity 'q' not defined
  xml r=0 e=26
  x L3 C26 1:55 Entity 'q' not defined
