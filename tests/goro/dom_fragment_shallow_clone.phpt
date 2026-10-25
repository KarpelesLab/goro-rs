--TEST--
A shallow clone/import of a document fragment has no children
--FILE--
<?php $d = new DOMDocument; $f = $d->createDocumentFragment(); $f->appendXML("<fe>f</fe>ft"); $c = $f->cloneNode(false); var_dump($c->childNodes->length, $d->saveXML($c)); $c = $f->cloneNode(true); var_dump($c->childNodes->length); $d2 = new DOMDocument; var_dump($d2->importNode($f, false)->childNodes->length, $d2->importNode($f, true)->childNodes->length); $m = Dom\XMLDocument::createEmpty(); $f = $m->createDocumentFragment(); $f->append($m->createElement("a"), "t"); var_dump($f->cloneNode(false)->childNodes->length, $f->cloneNode(true)->childNodes->length, $m->importNode($f)->childNodes->length);
--EXPECT--
int(0)
string(0) ""
int(2)
int(0)
int(2)
int(0)
int(2)
int(2)
