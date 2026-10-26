--TEST--
DOM / XMLReader / SimpleXML argument checks (registerNodeClass C, ?string, ?object, empty paths)
--FILE--
<?php
$d = new DOMDocument;
$d->loadXML("<a/>");
$x = new XMLReader;
$s = new SimpleXMLElement("<a/>");
foreach ([
  fn() => $d->registerNodeClass("abc", null), fn() => $d->registerNodeClass([], "x"), fn() => $d->registerNodeClass("DOMElement", "stdClass"),
  fn() => $d->registerNodeClass("DOMElement", "nope"), fn() => $d->registerNodeClass("abc", []),
  fn() => $d->contains([]), fn() => $d->contains("x"), fn() => $d->contains(new stdClass), fn() => $d->lookupNamespaceURI([]),
  fn() => $d->C14NFile(""), fn() => $d->C14NFile(null, null, null), fn() => $s->registerXPathNamespace("", ""),
  fn() => $x->next([]), fn() => $x->setRelaxNGSchema(new stdClass), fn() => $x->setRelaxNGSchemaSource([]),
] as $i => $f) {
  echo "$i: "; try { var_dump($f()); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
}
--EXPECTF--
0: TypeError: DOMDocument::registerNodeClass(): Argument #1 ($baseClass) must be a class name derived from DOMNode, abc given
1: 
Warning: Array to string conversion in %s on line 7
TypeError: DOMDocument::registerNodeClass(): Argument #1 ($baseClass) must be a class name derived from DOMNode, Array given
2: Error: DOMDocument::registerNodeClass(): Argument #2 ($extendedClass) must be a class name derived from DOMElement or null, stdClass given
3: TypeError: DOMDocument::registerNodeClass(): Argument #2 ($extendedClass) must be a valid class name or null, nope given
4: TypeError: DOMDocument::registerNodeClass(): Argument #1 ($baseClass) must be a class name derived from DOMNode, abc given
5: TypeError: DOMNode::contains(): Argument #1 ($other) must be of type ?object, array given
6: TypeError: DOMNode::contains(): Argument #1 ($other) must be of type ?object, string given
7: TypeError: DOMNode::contains(): Argument #1 ($other) must be of type DOMNode|DOMNameSpaceNode|null, stdClass given
8: TypeError: DOMNode::lookupNamespaceURI(): Argument #1 ($prefix) must be of type ?string, array given
9: ValueError: Path must not be empty
10: 
Deprecated: DOMNode::C14NFile(): Passing null to parameter #1 ($uri) of type string is deprecated in %s on line 10

Deprecated: DOMNode::C14NFile(): Passing null to parameter #2 ($exclusive) of type bool is deprecated in %s on line 10

Deprecated: DOMNode::C14NFile(): Passing null to parameter #3 ($withComments) of type bool is deprecated in %s on line 10
ValueError: Path must not be empty
11: bool(false)
12: TypeError: XMLReader::next(): Argument #1 ($name) must be of type ?string, array given
13: TypeError: XMLReader::setRelaxNGSchema(): Argument #1 ($filename) must be of type ?string, stdClass given
14: TypeError: XMLReader::setRelaxNGSchemaSource(): Argument #1 ($source) must be of type ?string, array given
