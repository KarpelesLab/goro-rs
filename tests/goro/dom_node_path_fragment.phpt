--TEST--
getNodePath() of a fragment and its contents is NULL
--FILE--
<?php
$d = new DOMDocument; $f = $d->createDocumentFragment(); $e = $d->createElement("a"); $f->appendChild($e); $e->appendChild($d->createElement("b"));
var_dump($f->getNodePath(), $e->getNodePath(), $e->firstChild->getNodePath(), $d->createElement("z")->getNodePath());
--EXPECT--
NULL
NULL
NULL
string(2) "/z"
