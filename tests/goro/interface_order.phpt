--TEST--
class_implements() and getInterfaceNames() order of internal and user classes
--FILE--
<?php
class MyIt extends ArrayIterator {}
class MyIt2 extends ArrayIterator implements JsonSerializable { function jsonSerialize(): mixed { return 1; } }
interface I1 {} interface I2 extends I1 {} interface I3 {}
class Y implements I2, I3 {} class Z extends Y {} class W extends Y implements Countable { function count(): int { return 0; } }
foreach (['ArrayIterator', 'ArrayObject', 'RecursiveArrayIterator', 'SplObjectStorage', 'SplDoublyLinkedList', 'SplQueue', 'SplFixedArray', 'SplMinHeap', 'SplPriorityQueue', 'CachingIterator', 'RecursiveCachingIterator', 'RecursiveIteratorIterator', 'RecursiveDirectoryIterator', 'SplTempFileObject', 'Generator', 'WeakMap', 'DatePeriod', 'MyIt', 'MyIt2', 'Y', 'Z', 'W', 'I2', 'RecursiveRegexIterator', 'ParentIterator', 'PDOStatement', 'SimpleXMLElement', 'DOMNodeList'] as $c) {
    if (!class_exists($c) && !interface_exists($c)) continue;
    echo $c, ': ', implode(',', class_implements($c)), ' | ', implode(',', (new ReflectionClass($c))->getInterfaceNames()), "\n";
}
--EXPECT--
ArrayIterator: SeekableIterator,Traversable,Iterator,ArrayAccess,Serializable,Countable | SeekableIterator,Traversable,Iterator,ArrayAccess,Serializable,Countable
ArrayObject: IteratorAggregate,Traversable,ArrayAccess,Serializable,Countable | IteratorAggregate,Traversable,ArrayAccess,Serializable,Countable
RecursiveArrayIterator: Countable,Serializable,ArrayAccess,Iterator,Traversable,SeekableIterator,RecursiveIterator | Countable,Serializable,ArrayAccess,Iterator,Traversable,SeekableIterator,RecursiveIterator
SplObjectStorage: Countable,SeekableIterator,Traversable,Iterator,Serializable,ArrayAccess | Countable,SeekableIterator,Traversable,Iterator,Serializable,ArrayAccess
SplDoublyLinkedList: Iterator,Traversable,Countable,ArrayAccess,Serializable | Iterator,Traversable,Countable,ArrayAccess,Serializable
SplQueue: Serializable,ArrayAccess,Countable,Traversable,Iterator | Serializable,ArrayAccess,Countable,Traversable,Iterator
SplFixedArray: IteratorAggregate,Traversable,ArrayAccess,Countable,JsonSerializable | IteratorAggregate,Traversable,ArrayAccess,Countable,JsonSerializable
SplMinHeap: Countable,Traversable,Iterator | Countable,Traversable,Iterator
SplPriorityQueue: Iterator,Traversable,Countable | Iterator,Traversable,Countable
CachingIterator: Stringable,Iterator,Traversable,OuterIterator,ArrayAccess,Countable | Stringable,Iterator,Traversable,OuterIterator,ArrayAccess,Countable
RecursiveCachingIterator: Countable,ArrayAccess,OuterIterator,Traversable,Iterator,Stringable,RecursiveIterator | Countable,ArrayAccess,OuterIterator,Traversable,Iterator,Stringable,RecursiveIterator
RecursiveIteratorIterator: OuterIterator,Traversable,Iterator | OuterIterator,Traversable,Iterator
RecursiveDirectoryIterator: Stringable,SeekableIterator,Traversable,Iterator,RecursiveIterator | Stringable,SeekableIterator,Traversable,Iterator,RecursiveIterator
SplTempFileObject: SeekableIterator,Iterator,Traversable,RecursiveIterator,Stringable | SeekableIterator,Iterator,Traversable,RecursiveIterator,Stringable
Generator: Iterator,Traversable | Iterator,Traversable
WeakMap: ArrayAccess,Countable,IteratorAggregate,Traversable | ArrayAccess,Countable,IteratorAggregate,Traversable
DatePeriod: IteratorAggregate,Traversable | IteratorAggregate,Traversable
MyIt: Countable,Serializable,ArrayAccess,Iterator,Traversable,SeekableIterator | Countable,Serializable,ArrayAccess,Iterator,Traversable,SeekableIterator
MyIt2: SeekableIterator,Traversable,Iterator,ArrayAccess,Serializable,Countable,JsonSerializable | SeekableIterator,Traversable,Iterator,ArrayAccess,Serializable,Countable,JsonSerializable
Y: I2,I3,I1 | I2,I3,I1
Z: I1,I3,I2 | I1,I3,I2
W: I2,I3,I1,Countable | I2,I3,I1,Countable
I2: I1 | I1
RecursiveRegexIterator: Iterator,Traversable,OuterIterator,RecursiveIterator | Iterator,Traversable,OuterIterator,RecursiveIterator
ParentIterator: RecursiveIterator,Iterator,Traversable,OuterIterator | RecursiveIterator,Iterator,Traversable,OuterIterator
PDOStatement: IteratorAggregate,Traversable | IteratorAggregate,Traversable
SimpleXMLElement: Stringable,Countable,RecursiveIterator,Traversable,Iterator | Stringable,Countable,RecursiveIterator,Traversable,Iterator
DOMNodeList: IteratorAggregate,Traversable,Countable | IteratorAggregate,Traversable,Countable
