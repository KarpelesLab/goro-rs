--TEST--
The xml prefix is bound on element names too
--FILE--
<?php
// the xml prefix is always bound, on elements too
$d = new DOMDocument;
$d->loadXML('<xml:e xml:lang="x"><xml:f/></xml:e>');
echo $d->saveXML();
var_dump($d->documentElement->namespaceURI, $d->documentElement->prefix);
$s = simplexml_load_string('<xml:e/>');
var_dump($s->getName());
$r = XMLReader::fromString('<xml:e/>');
$r->read();
var_dump($r->namespaceURI);
$m = Dom\XMLDocument::createFromString('<xml:e/>');
var_dump($m->documentElement->namespaceURI);
--EXPECT--
<?xml version="1.0"?>
<xml:e xml:lang="x"><xml:f/></xml:e>
string(36) "http://www.w3.org/XML/1998/namespace"
string(3) "xml"
string(1) "e"
string(36) "http://www.w3.org/XML/1998/namespace"
string(36) "http://www.w3.org/XML/1998/namespace"
