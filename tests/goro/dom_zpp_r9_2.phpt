--TEST--
DOM: ?bool / ?array argument types, selector number tokens, hasFeature(), Dom registerNodeClass() order
--FILE--
<?php
$d = new DOMDocument; $d->loadXML("<r><a x='1'/></r>");
$m = Dom\XMLDocument::createFromString("<r><a x='1'>t</a></r>");
$h = Dom\HTMLDocument::createFromString("<p class='a b'>x</p>", LIBXML_NOERROR);
foreach ([
  fn() => $d->documentElement->toggleAttribute("y", []), fn() => $m->documentElement->toggleAttribute("y", []),
  fn() => $h->body->firstChild->classList->toggle("c", []), fn() => $m->getRootNode("x"), fn() => $m->documentElement->getRootNode(1),
  fn() => $m->querySelector("9223372036854775807"), fn() => $m->querySelector("1e3"), fn() => $m->querySelector("1.50"),
  fn() => (new DOMImplementation)->hasFeature("", ""), fn() => (new DOMImplementation)->hasFeature("XML", "2.0"), fn() => (new DOMImplementation)->hasFeature("Core", "2.0"),
  fn() => $m->registerNodeClass("abc", []), fn() => $m->registerNodeClass([], "x"), fn() => $m->documentElement->setIdAttributeNS(null, "x", true),
] as $i => $f) {
  echo "$i: "; try { var_dump($f()); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
}
--EXPECTF--
0: TypeError: DOMElement::toggleAttribute(): Argument #2 ($force) must be of type ?bool, array given
1: TypeError: Dom\Element::toggleAttribute(): Argument #2 ($force) must be of type ?bool, array given
2: TypeError: Dom\TokenList::toggle(): Argument #2 ($force) must be of type ?bool, array given
3: TypeError: Dom\Node::getRootNode(): Argument #1 ($options) must be of type ?array, string given
4: TypeError: Dom\Node::getRootNode(): Argument #1 ($options) must be of type ?array, int given
5: DOMException: Invalid selector (Selectors. Unexpected token: 9223372036854776000)
6: DOMException: Invalid selector (Selectors. Unexpected token: 1000)
7: DOMException: Invalid selector (Selectors. Unexpected token: 1.5)
8: bool(false)
9: bool(true)
10: bool(false)
11: TypeError: Dom\Document::registerNodeClass(): Argument #1 ($baseClass) must be a class name derived from Dom\Node, abc given
12: 
Warning: Array to string conversion in %s on line 10
TypeError: Dom\Document::registerNodeClass(): Argument #1 ($baseClass) must be a class name derived from Dom\Node, Array given
13: 
Deprecated: Dom\Element::setIdAttributeNS(): Passing null to parameter #1 ($namespace) of type ?string is deprecated in %s on line 10
DOMException: Not Found Error
