--TEST--
Legacy DOM *AttributeNS() keep an empty namespace name
--FILE--
<?php
// legacy DOM: "" is a namespace name no attribute is in (null is "no namespace")
$d = new DOMDocument;
$d->loadXML('<f x="1" xmlns:p="urn:p" p:y="2"/>');
$e = $d->documentElement;
var_dump($e->getAttributeNS('', 'x'), $e->hasAttributeNS('', 'x'), $e->getAttributeNodeNS('', 'x')?->name, $e->getAttributeNS(null, 'x'), $e->hasAttributeNS(null, 'x'));
$e->removeAttributeNS('', 'x');
echo $d->saveXML();
$e->removeAttributeNS(null, 'x');
echo $d->saveXML();
$e->setAttributeNS('', 'z', '3');
$e->setAttributeNS(null, 'w', '4');
$e->removeAttributeNS('', 'z');
echo $d->saveXML();
$m = Dom\XMLDocument::createFromString('<f x="1"/>');
var_dump($m->documentElement->getAttributeNS('', 'x'), $m->documentElement->hasAttributeNS('', 'x'));
$m->documentElement->removeAttributeNS('', 'x');
echo $m->saveXml(), "\n";
--EXPECT--
string(0) ""
bool(false)
NULL
string(1) "1"
bool(true)
<?xml version="1.0"?>
<f xmlns:p="urn:p" x="1" p:y="2"/>
<?xml version="1.0"?>
<f xmlns:p="urn:p" p:y="2"/>
<?xml version="1.0"?>
<f xmlns:p="urn:p" p:y="2" z="3" w="4"/>
string(1) "1"
bool(true)
<?xml version="1.0" encoding="UTF-8"?>
<f/>
