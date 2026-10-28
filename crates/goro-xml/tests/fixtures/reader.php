<?php
// Fixture driver: XMLReader node stream on PHP 8.5.
//   C <base64 input> <parser properties json>
//   N <type> <depth> <empty> <hasValue> <attrCount> <b64 name> <b64 localName> <b64 prefix> <b64 ns> <b64 value> <b64 inner> <b64 outer> <b64 string>
//   A <b64 name> <b64 value> <b64 ns>      (attributes of the current element)
//   E <level> <code> <line> <col> <b64 msg>  (errors collected while reading)
//   F <read() result false>
$cases = [
 ["<root a='1' b='2'><child>text</child><empty/><e2></e2><!-- c --><?pi data?><![CDATA[cd]]>\n  <x:y xmlns:x='urn:x' x:z='3'/></root>", []],
 ["<?xml version='1.0'?>\n<!DOCTYPE r [<!ENTITY e 'ent'>]>\n<r>&e;<s>  </s>\n</r>", []],
 ["<!DOCTYPE r [<!ENTITY e 'ent<b/>'>]><r>&e;</r>", [XMLReader::SUBST_ENTITIES => true]],
 ["<!DOCTYPE r [<!ENTITY e 'ent<b/>'>]><r>&e;</r>", []],
 ["<!DOCTYPE r [<!ATTLIST r d CDATA 'def'>]><r/>", [XMLReader::DEFAULTATTRS => true]],
 ["<r xml:lang='en' xml:space='preserve'><a xmlns='urn:d'><b/></a></r>", []],
 ["<r><a>1</a><b>2</b></r>junk", []],
 ["<r><a>1</a><b>2</c></r>", []],
 ["<r>\n  <a>x</a>\n</r>", []],
 ["<r><a><b>t</b>u</a></r>", []],
];
foreach ($cases as [$xml, $props]) {
  libxml_use_internal_errors(true); libxml_clear_errors();
  echo "C ", base64_encode($xml), " ", json_encode((object)$props), "\n";
  $r = XMLReader::XML($xml);
  foreach ($props as $k => $v) $r->setParserProperty($k, $v);
  $n = 0;
  while (true) {
    $ok = @$r->read();
    foreach (libxml_get_errors() as $e) echo "E {$e->level} {$e->code} {$e->line} {$e->column} ", base64_encode($e->message), "\n";
    libxml_clear_errors();
    if (!$ok) { echo "F\n"; break; }
    if (++$n > 100) break;
    $inner = @$r->readInnerXml(); $outer = @$r->readOuterXml(); $str = @$r->readString();
    echo "N {$r->nodeType} {$r->depth} ", (int)$r->isEmptyElement, " ", (int)$r->hasValue, " {$r->attributeCount} ",
      base64_encode($r->name), " ", base64_encode($r->localName), " ", base64_encode($r->prefix), " ", base64_encode($r->namespaceURI), " ",
      base64_encode($r->value), " ", base64_encode((string)$inner), " ", base64_encode((string)$outer), " ", base64_encode((string)$str), "\n";
    if ($r->nodeType == XMLReader::ELEMENT && $r->moveToFirstAttribute()) {
      do { echo "A ", base64_encode($r->name), " ", base64_encode($r->value), " ", base64_encode($r->namespaceURI), "\n"; } while ($r->moveToNextAttribute());
      $r->moveToElement();
    }
  }
}
// next()
$r = XMLReader::XML("<r><a><x/></a><b/><a/><c>t</c></r>");
$r->read(); $r->read();
echo "NEXT ", $r->name; while ($r->next()) echo " ", $r->name, ":", $r->nodeType; echo "\n";
$r = XMLReader::XML("<r><a><x/></a><b/><a/><c>t</c></r>");
$r->read(); $r->read();
echo "NEXTA ", $r->name; while ($r->next('a')) echo " ", $r->name, ":", $r->nodeType; echo "\n";
$r = XMLReader::XML("<r a='1' xmlns:p='urn:p' p:b='2'/>"); $r->read();
var_dump($r->getAttribute('a'), $r->getAttribute('p:b'), $r->getAttributeNs('b', 'urn:p'), $r->getAttributeNo(1), $r->getAttribute('zz'), $r->lookupNamespace('p'));
var_dump($r->moveToAttribute('p:b'), $r->name, $r->nodeType, $r->depth, $r->moveToAttributeNo(0), $r->name, $r->moveToElement(), $r->name);
$r = XMLReader::XML("<r><a b='1'>t<c/></a></r>"); $r->read(); $r->read(); $n = $r->expand(); var_dump(get_class($n), $n->nodeName, $n->ownerDocument->saveXML($n));
