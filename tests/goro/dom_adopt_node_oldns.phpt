--TEST--
adoptNode() leaves out-of-scope namespaces undeclared; insertion reconciles them
--FILE--
<?php
$d = new DOMDocument;
$d->loadXML('<root xmlns:b="urn:b"><b:x/></root>');
$e = $d->createElementNS('urn:b', 'b:new');
$d->documentElement->appendChild($e);
echo $d->saveXML();
$d2 = new DOMDocument;
$d2->loadXML('<other xmlns:b="urn:other"/>');
$a = $d2->adoptNode($e);
echo $d2->saveXML($a), "\n";
$d2->documentElement->appendChild($a);
echo $d2->saveXML();
$a2 = $d2->adoptNode($d->documentElement->firstChild);
echo $d2->saveXML($a2), "\n";
$d2->documentElement->appendChild($a2);
echo $d2->saveXML();
$d3 = new DOMDocument;
$d3->loadXML('<o xmlns:b="urn:other"/>');
$el = $d3->createElementNS('urn:b', 'b:q');
$d3->documentElement->appendChild($el);
echo $d3->saveXML();
$d4 = new DOMDocument;
$d4->loadXML('<root xmlns:b="urn:b"><b:x/></root>');
$c = $d4->documentElement->firstChild;
$d4->documentElement->removeChild($c);
echo $d4->saveXML($c), "\n";
$d3->documentElement->appendChild($d3->importNode($c));
echo $d3->saveXML();
--EXPECT--
<?xml version="1.0"?>
<root xmlns:b="urn:b"><b:x/><b:new/></root>
<b:new/>
<?xml version="1.0"?>
<other xmlns:b="urn:other"><b1:new xmlns:b1="urn:b"/></other>
<b:x/>
<?xml version="1.0"?>
<other xmlns:b="urn:other"><b1:new xmlns:b1="urn:b"/><b1:x xmlns:b1="urn:b"/></other>
<?xml version="1.0"?>
<o xmlns:b="urn:other"><b:q xmlns:b="urn:b"/></o>
<b:x/>
<?xml version="1.0"?>
<o xmlns:b="urn:other"><b:q xmlns:b="urn:b"/><b:x xmlns:b="urn:b"/></o>
