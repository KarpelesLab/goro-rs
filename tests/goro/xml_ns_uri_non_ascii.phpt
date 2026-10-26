--TEST--
Namespace names with non-ASCII bytes are invalid URIs (xmlns warnings, C14N error with node line/file)
--FILE--
<?php
libxml_use_internal_errors(true);
foreach (["<r xmlns:p='\xc3\xa9'/>", "<r xmlns='a b'/>", "<r xmlns:p='rel'/>", "<?xml version='1.0'?>\n<r>\n<x xmlns:p=' sp '/></r>", "<r xmlns:p='urn:x'/>"] as $x) {
  $d = new DOMDocument; $d->loadXML($x);
  libxml_clear_errors();
  var_dump($d->C14N());
  foreach (libxml_get_errors() as $e) echo "  L{$e->level} C{$e->code} {$e->line}:{$e->column} ", json_encode($e->message), " ", var_export($e->file, true), "\n";
  libxml_clear_errors();
  var_dump($d->documentElement->C14N(true));
  foreach (libxml_get_errors() as $e) echo "  L{$e->level} C{$e->code} {$e->line}:{$e->column} ", json_encode($e->message), "\n";
  libxml_clear_errors();
}
?>
--EXPECTF--
bool(false)
  L2 C91 1:0 "parsing namespace uri" '%s'
bool(false)
  L2 C91 1:0 "parsing namespace uri"
bool(false)
  L2 C91 1:0 "parsing namespace uri" '%s'
bool(false)
  L2 C91 1:0 "parsing namespace uri"
bool(false)
  L2 C1955 0:0 "Relative namespace UR is invalid here : (null)\n" ''
bool(false)
  L2 C1955 0:0 "Relative namespace UR is invalid here : (null)\n"
bool(false)
  L2 C91 3:0 "parsing namespace uri" '%s'
bool(false)
  L2 C91 3:0 "parsing namespace uri"
string(23) "<r xmlns:p="urn:x"></r>"
string(7) "<r></r>"
