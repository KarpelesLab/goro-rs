--TEST--
Processing instructions without data keep a NULL content (<?t?> vs <?t ?>, nodeValue NULL)
--FILE--
<?php
$p = new DOMProcessingInstruction('a'); var_dump($p->nodeValue, $p->data, $p->textContent);
$p = new DOMProcessingInstruction('a', ''); var_dump($p->nodeValue, $p->data);
$c = new DOMComment(); var_dump($c->nodeValue, $c->data, $c->textContent, $c->length);
$t = new DOMText(); var_dump($t->nodeValue, $t->data, $t->wholeText);
$d = new DOMDocument; $d->loadXML('<a/>');
$d->documentElement->append($p, $c, $t, new DOMProcessingInstruction('b'));
echo $d->saveXML(), $d->saveHTML(), "\n";
$m = Dom\XMLDocument::createFromString('<a><?p ?><?q?></a>');
foreach ($m->documentElement->childNodes as $n) var_dump($n->nodeValue, $n->data, $n->textContent);
$e = Dom\XMLDocument::createEmpty();
$e->append($e->createElement('r'));
$e->documentElement->append($e->createProcessingInstruction('z', ''), $e->createComment(''));
foreach ($e->documentElement->childNodes as $n) var_dump($n->nodeValue);
echo $e->saveXml(), "\n";
$dd = new DOMDocument; $dd->loadXML('<a><?q?><!----></a>');
foreach ($dd->documentElement->childNodes as $n) var_dump($n->nodeValue);
$cl = $dd->documentElement->cloneNode(true); echo $dd->saveXML($cl), "\n";
$x = simplexml_load_string('<a><?p ?><?q?><b/></a>'); echo $x->asXML(); var_dump((string)$x);
?>
--EXPECTF--
NULL
string(0) ""
string(0) ""
string(0) ""
string(0) ""
NULL
string(0) ""
string(0) ""
int(0)
NULL
string(0) ""
string(0) ""

Fatal error: Uncaught DOMException: Wrong Document Error in %s:7
Stack trace:
#0 %s(7): DOMElement->append(Object(DOMProcessingInstruction), Object(DOMComment), Object(DOMText), Object(DOMProcessingInstruction))
#1 {main}
  thrown in %s on line 7
