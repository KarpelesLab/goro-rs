--TEST--
With LIBXML_NOENT the entity declarations still get their parsed content
--FILE--
<?php
foreach ([0, LIBXML_NOENT] as $o) {
  $d = new DOMDocument;
  $d->loadXML('<!DOCTYPE a [<!ENTITY e "x<b>y</b>z"><!ENTITY f "plain"><!ENTITY g "unused">]><a>&e;&f;</a>', $o);
  foreach (['e', 'f', 'g'] as $n) {
    $ent = $d->doctype->entities->getNamedItem($n);
    var_dump($ent->textContent, $ent->nodeValue, $ent->childNodes->length, $ent->firstChild?->nodeName, $ent->hasChildNodes());
  }
  echo $d->saveXML();
  $ref = $d->documentElement->firstChild;
  var_dump(get_class($ref), $ref->nodeName, $ref->childNodes->length, $ref->textContent);
}
?>
--EXPECT--
string(3) "xyz"
NULL
int(3)
string(5) "#text"
bool(true)
string(5) "plain"
NULL
int(1)
string(5) "#text"
bool(true)
string(0) ""
NULL
int(0)
NULL
bool(false)
<?xml version="1.0"?>
<!DOCTYPE a [
<!ENTITY e "x<b>y</b>z">
<!ENTITY f "plain">
<!ENTITY g "unused">
]>
<a>&e;&f;</a>
string(18) "DOMEntityReference"
string(1) "e"
int(3)
string(3) "xyz"
string(3) "xyz"
NULL
int(3)
string(5) "#text"
bool(true)
string(5) "plain"
NULL
int(1)
string(5) "#text"
bool(true)
string(0) ""
NULL
int(0)
NULL
bool(false)
<?xml version="1.0"?>
<!DOCTYPE a [
<!ENTITY e "x<b>y</b>z">
<!ENTITY f "plain">
<!ENTITY g "unused">
]>
<a>x<b>y</b>zplain</a>
string(7) "DOMText"
string(5) "#text"
int(0)
string(1) "x"
