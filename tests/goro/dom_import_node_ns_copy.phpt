--TEST--
importNode() namespace handling of elements and attributes
--FILE--
<?php
// xmlStaticCopyNode()/xmlCopyPropInternal() namespace handling
$d = new DOMDocument;
$d->loadXML('<b:g xmlns:b="urn:a" xmlns:default="urn:z"><g xmlns:a="urn:c"><b:f a:z="&lt;"><xml:h b:x="1"/><a:h xmlns="urn:a" y="2" a:y="3" default:x=""/></b:f></g></b:g>');
$f = $d->getElementsByTagName('f')->item(0);
$f->prefix = 'a';
echo $d->saveXML($f), "\n";
$d2 = new DOMDocument;
$d2->loadXML('<o xmlns:a="urn:other" xmlns="urn:dd"/>');
$i = $d2->importNode($f, true);
echo $d2->saveXML($i), "\n";
$d2->documentElement->appendChild($i);
echo $d2->saveXML();
foreach ($i->getElementsByTagName('*') as $e) {
    echo $e->nodeName, ' ', var_export($e->namespaceURI, true), "\n";
    foreach ($e->attributes as $at) echo '  ', $at->nodeName, ' ', var_export($at->namespaceURI, true), "\n";
}
--EXPECT--
<a:f xmlns:a="urn:a" a:z="&lt;"><xml:h b:x="1"/><a:h xmlns="urn:a" y="2" a:y="3" default:x=""/></a:f>
<a:f xmlns:a="urn:a" xmlns:a1="urn:c" xmlns:b="urn:a" xmlns:default="urn:z" a1:z="&lt;"><xml:h b:x="1"/><a:h xmlns="urn:a" y="2" a1:y="3" default:x=""/></a:f>
<?xml version="1.0"?>
<o xmlns:a="urn:other" xmlns="urn:dd"><a:f xmlns:a="urn:a" xmlns:a1="urn:c" xmlns:b="urn:a" xmlns:default="urn:z" a1:z="&lt;"><xml:h a:x="1"/><a:h xmlns="urn:a" y="2" a1:y="3" default:x=""/></a:f></o>
xml:h 'http://www.w3.org/XML/1998/namespace'
  a:x 'urn:a'
a:h 'urn:a'
  y NULL
  a1:y 'urn:c'
  default:x 'urn:z'
