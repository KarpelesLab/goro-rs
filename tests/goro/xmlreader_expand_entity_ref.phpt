--TEST--
Copied entity references only resolve in the target document
--FILE--
<?php
// an expanded copy goes to another document: the entity reference is empty
$r = XMLReader::fromString('<!DOCTYPE r [<!ENTITY e "ent">]><r><x>text &e; <![CDATA[cd]]></x></r>');
while ($r->read()) {
    if ($r->name == 'x' && $r->nodeType == XMLReader::ELEMENT) {
        $n = $r->expand();
        var_dump($n->textContent, $n->childNodes->item(1)->nodeName, $n->childNodes->item(1)->hasChildNodes());
    }
}
$d = new DOMDocument;
$d->loadXML('<!DOCTYPE r [<!ENTITY e "ent">]><r>&e;</r>');
$d2 = new DOMDocument;
$d2->loadXML('<!DOCTYPE q [<!ENTITY e "other">]><q/>');
$d2->documentElement->appendChild($d2->importNode($d->documentElement, true));
var_dump($d2->documentElement->textContent);
$d3 = new DOMDocument;
$d3->loadXML('<q/>');
$d3->documentElement->appendChild($d3->importNode($d->documentElement, true));
var_dump($d3->documentElement->textContent);
echo $d3->saveXML();
--EXPECT--
string(8) "text  cd"
string(1) "e"
bool(false)
string(0) ""
string(0) ""
<?xml version="1.0"?>
<q><r>&e;</r></q>
