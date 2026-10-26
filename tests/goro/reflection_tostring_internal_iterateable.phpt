--TEST--
Reflection __toString: <iterateable> and internal virtual properties
--FILE--
<?php
foreach (['DOMAttr', 'DOMDocument', 'Dom\Element', 'Dom\Node', 'DOMXPath', 'XMLReader', 'OuterIterator', 'RecursiveIterator', 'SeekableIterator', 'DatePeriod', 'Dom\TokenList', 'DOMNodeList', 'ArrayObject', 'Iterator', 'IteratorAggregate', 'Generator', 'SplFixedArray', 'DOMNamedNodeMap', 'Dom\HTMLCollection', 'SimpleXMLElement', 'WeakMap', 'PDOStatement'] as $c) {
  $s = (string) new ReflectionClass($c);
  $first = strtok($s, "\n");
  echo $first, "\n";
  preg_match_all('/Property \[.*\]/', $s, $m);
  foreach ($m[0] as $l) if (str_contains($l, 'get;') || str_contains($l, 'set;')) echo "  ", $l, "\n";
}
--EXPECT--
Class [ <internal:dom> class DOMAttr extends DOMNode ] {
Class [ <internal:dom> class DOMDocument extends DOMNode implements DOMParentNode ] {
Class [ <internal:dom> class Dom\Element extends Dom\Node implements Dom\ParentNode, Dom\ChildNode ] {
Class [ <internal:dom> class Dom\Node ] {
Class [ <internal:dom> class DOMXPath ] {
Class [ <internal:xmlreader> class XMLReader ] {
Interface [ <internal:SPL> interface OuterIterator extends Iterator, Traversable ] {
Interface [ <internal:SPL> interface RecursiveIterator extends Iterator, Traversable ] {
Interface [ <internal:SPL> interface SeekableIterator extends Iterator, Traversable ] {
Class [ <internal:date> <iterateable> class DatePeriod implements IteratorAggregate, Traversable ] {
Class [ <internal:dom> <iterateable> final class Dom\TokenList implements IteratorAggregate, Traversable, Countable ] {
Class [ <internal:dom> <iterateable> class DOMNodeList implements IteratorAggregate, Traversable, Countable ] {
Class [ <internal:SPL> <iterateable> class ArrayObject implements IteratorAggregate, Traversable, ArrayAccess, Serializable, Countable ] {
Interface [ <internal:Core> interface Iterator extends Traversable ] {
Interface [ <internal:Core> interface IteratorAggregate extends Traversable ] {
Class [ <internal:Core> <iterateable> final class Generator implements Iterator, Traversable ] {
Class [ <internal:SPL> <iterateable> class SplFixedArray implements IteratorAggregate, Traversable, ArrayAccess, Countable, JsonSerializable ] {
Class [ <internal:dom> <iterateable> class DOMNamedNodeMap implements IteratorAggregate, Traversable, Countable ] {
Class [ <internal:dom> <iterateable> class Dom\HTMLCollection implements IteratorAggregate, Traversable, Countable ] {
Class [ <internal:SimpleXML> <iterateable> class SimpleXMLElement implements Stringable, Countable, RecursiveIterator, Traversable, Iterator ] {
Class [ <internal:Core> <iterateable> final class WeakMap implements ArrayAccess, Countable, IteratorAggregate, Traversable ] {
Class [ <internal:PDO> <iterateable> class PDOStatement implements IteratorAggregate, Traversable ] {
