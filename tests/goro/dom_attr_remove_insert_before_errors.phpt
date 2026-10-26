--TEST--
DOMNode::removeChild()/insertBefore() with attributes and with the reference node itself fail like php-src
--FILE--
<?php
$d = new DOMDocument;
$d->loadXML('<r><a k="1"/><b k="2">t</b><c xmlns:p="urn:p"><p:e p:k="v"/></c></r>');
$a = $d->getElementsByTagName('a')->item(0);
$b = $d->getElementsByTagName('b')->item(0);
$c = $d->getElementsByTagName('c')->item(0);
// removeChild() of an attribute: Not Found unless the element has children
try { $a->removeChild($a->getAttributeNode('k')); } catch (DOMException $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
var_dump($b->removeChild($b->getAttributeNode('k'))->name);
echo $d->saveXML($d->documentElement), "\n";
// insertBefore() of an attribute before an element child fails, after unlinking it
$e = $c->firstChild;
try { $c->insertBefore($e->getAttributeNodeNS('urn:p', 'k'), $e); } catch (Error $ex) { echo get_class($ex), ": ", $ex->getMessage(), "\n"; }
echo $d->saveXML($d->documentElement), "\n";
// insertBefore($node, $node): unlinked, then the same error
try { $c->insertBefore($e, $e); } catch (Error $ex) { echo get_class($ex), ": ", $ex->getMessage(), "\n"; }
echo $d->saveXML($d->documentElement), "\n";
var_dump($e->parentNode);
// attribute before an attribute is fine
$b->setAttribute('x', '1');
$at = $d->createAttribute('y');
var_dump($b->insertBefore($at, $b->getAttributeNode('x'))->name);
echo $d->saveXML($d->documentElement), "\n";
?>
--EXPECT--
DOMException: Not Found Error
string(1) "k"
<r><a k="1"/><b>t</b><c xmlns:p="urn:p"><p:e p:k="v"/></c></r>
Error: Cannot add newnode as the previous sibling of refnode
<r><a k="1"/><b>t</b><c xmlns:p="urn:p"><p:e/></c></r>
Error: Cannot add newnode as the previous sibling of refnode
<r><a k="1"/><b>t</b><c xmlns:p="urn:p"/></r>
NULL
string(1) "y"
<r><a k="1"/><b y="" x="1">t</b><c xmlns:p="urn:p"/></r>
