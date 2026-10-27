--TEST--
list() / [...] destructuring of an extension object with a read_dimension handler (DOMNodeList, DOMNamedNodeMap, Dom\HTMLCollection, SimpleXMLElement)
--EXTENSIONS--
dom
simplexml
--FILE--
<?php
$d = new DOMDocument();
$d->loadXML('<r><a x="1"/><b x="2"/><c/></r>');
$l = $d->documentElement->childNodes;
[$a, $b] = $l;
var_dump($a->nodeName, $b->nodeName);
[1 => $q] = $l;
var_dump($q->nodeName);
list(, , $c) = $l;
var_dump($c->nodeName);
foreach ([$l] as [$z]) var_dump($z->nodeName);
[, , , $dd] = $l;
var_dump($dd);
$m2 = $d->getElementsByTagName('a')[0]->attributes;
[$at] = $m2;
var_dump($at->nodeName);
['x' => $ax] = $m2;
var_dump($ax->value);
try { [$e] = $d; } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
$h = Dom\HTMLDocument::createFromString('<p>1</p><p>2</p>', LIBXML_NOERROR);
[$p1, $p2] = $h->getElementsByTagName('p');
var_dump($p2->textContent);
$s = simplexml_load_string('<r><a>1</a><a>2</a></r>');
[$s1] = $s->a;
var_dump((string)$s1);
?>
--EXPECT--
string(1) "a"
string(1) "b"
string(1) "b"
string(1) "c"
string(1) "a"
NULL
string(1) "x"
string(1) "1"
Error: Cannot use object of type DOMDocument as array
string(1) "2"
string(1) "1"
