--TEST--
Live DOM node lists keep the object of the last item returned alive
--FILE--
<?php
$d = new DOMDocument; $d->loadXML('<r><a/><b/><c/></r>');
$l = $d->getElementsByTagName('*');
$l->item(1)->foo = 'kept';
var_dump($l->item(1)->foo ?? 'lost');
$l->item(2)->bar = 'x'; $l->item(0);
var_dump($d->getElementsByTagName('c')->item(0)->bar ?? 'lost');
$cn = $d->documentElement->childNodes;
$cn->item(0)->p = 1; var_dump($cn->item(0)->p ?? 'lost');
$cn[1]->q = 2; var_dump($cn[1]->q ?? 'lost');
foreach ($cn as $n) {}
var_dump($d->documentElement->lastChild->bar ?? 'lost');
$x = new DOMXPath($d); $ql = $x->query('//b'); $ql->item(0)->z = 3; var_dump($ql->item(0)->z ?? 'lost');
$m = Dom\XMLDocument::createFromString('<r><a/><b/></r>');
$ch = $m->documentElement->children; $ch->item(0)->w = 1; var_dump($ch->item(0)->w ?? 'lost');
$cnm = $m->documentElement->childNodes; $cnm->item(1)->w = 2; var_dump($cnm->item(1)->w ?? 'lost');
$gc = $d->documentElement->getElementsByTagName('a'); $gc->item(0)->y = 9; $d->documentElement->appendChild($d->createElement('a')); var_dump($gc->item(0)->y ?? 'lost');
class E extends DOMElement { function __destruct() { echo "destruct {$this->nodeName}\n"; } }
$d2 = new DOMDocument; $d2->registerNodeClass('DOMElement', 'E'); $d2->loadXML('<r><a/><b/></r>');
$l2 = $d2->getElementsByTagName('a');
$l2->item(0);
echo "after item\n";
unset($l2);
echo "after unset\n";
$l3 = $d2->documentElement->childNodes;
foreach ($l3 as $n) { echo "it {$n->nodeName}\n"; }
unset($n);
echo "after loop\n";
unset($l3);
echo "end\n";
?>
--EXPECTF--

Deprecated: Creation of dynamic property DOMElement::$foo is deprecated in %s on line 4
string(4) "kept"

Deprecated: Creation of dynamic property DOMElement::$bar is deprecated in %s on line 6
string(4) "lost"

Deprecated: Creation of dynamic property DOMElement::$p is deprecated in %s on line 9
int(1)

Deprecated: Creation of dynamic property DOMElement::$q is deprecated in %s on line 10
int(2)
string(4) "lost"

Deprecated: Creation of dynamic property DOMElement::$z is deprecated in %s on line 13
int(3)

Deprecated: Creation of dynamic property Dom\Element::$w is deprecated in %s on line 15
int(1)

Deprecated: Creation of dynamic property Dom\Element::$w is deprecated in %s on line 16
int(2)

Deprecated: Creation of dynamic property DOMElement::$y is deprecated in %s on line 17
int(9)
after item
destruct a
after unset
it a
it b
destruct b
after loop
destruct a
destruct r
end
