--TEST--
Legacy DOMElement attribute getters see DTD-defaulted attributes (xmlHasNsProp)
--FILE--
<?php
$d = new DOMDocument;
$d->loadXML('<!DOCTYPE r [<!ATTLIST r def CDATA "dv" fx CDATA #FIXED "fixed" imp CDATA #IMPLIED p:q CDATA "pq" xmlns:p CDATA "urn:p"><!ATTLIST e x CDATA "ex">]><r><e/><p:e xmlns:p="urn:p"/></r>');
$r = $d->documentElement;
var_dump($r->getAttribute('def'), $r->hasAttribute('def'), $r->getAttribute('fx'), $r->hasAttribute('imp'), $r->getAttribute('p:q'), $r->hasAttribute('p:q'));
var_dump($r->getAttributeNS('', 'def'), $r->hasAttributeNS(null, 'def'), $r->hasAttributeNS('urn:p', 'q'));
var_dump($r->getAttributeNode('def'));
var_dump($r->attributes->length, $r->getAttributeNames());
$e = $r->firstChild; var_dump($e->getAttribute('x'), $e->hasAttribute('x'));
var_dump($r->lastChild->getAttribute('x'));
$r->setAttribute('def', 'set'); var_dump($r->getAttribute('def')); $r->removeAttribute('def'); var_dump($r->getAttribute('def'), $r->hasAttribute('def'));
$x = new DOMXPath($d); var_dump($x->evaluate('string(/r/@def)'));
$s = simplexml_import_dom($d); var_dump((string)$s['def']);
$m = Dom\XMLDocument::createFromString('<!DOCTYPE r [<!ATTLIST r def CDATA "dv">]><r/>');
var_dump($m->documentElement->getAttribute('def'), $m->documentElement->hasAttribute('def'));
?>
--EXPECTF--
string(2) "dv"
bool(true)
string(5) "fixed"
bool(false)
string(2) "pq"
bool(true)
string(0) ""
bool(true)
bool(true)

Fatal error: Uncaught Error: Unsupported node type: 16 in %s:7
Stack trace:
#0 %s(7): DOMElement->getAttributeNode('def')
#1 {main}
  thrown in %s on line 7
