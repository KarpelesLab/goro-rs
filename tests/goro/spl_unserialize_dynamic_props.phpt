--TEST--
SPL unserialize() reports every dynamic member it loads
--FILE--
<?php
$s=new SplObjectStorage;$s->p0=1;$s->unserialize($s->serialize()); var_dump($s);
$a=new ArrayObject([1]);$a->p0=1;$a->unserialize($a->serialize()); var_dump($a);
$i=new ArrayIterator([1]);$i->p0=1;$i->unserialize($i->serialize()); var_dump($i);
$l=new SplDoublyLinkedList;$l->p0=1;$l->unserialize($l->serialize()); var_dump($l);
class S extends SplObjectStorage { public $d = 1; protected $e = 2; }
$x = new S; $x->d = 5; $x->unserialize($x->serialize()); var_dump($x);
var_dump(unserialize(serialize($s)));
$h = new SplMinHeap; $h->q = 1; var_dump(unserialize(serialize($h)));
--EXPECTF--

Deprecated: Creation of dynamic property SplObjectStorage::$p0 is deprecated in %s on line 2

Deprecated: Creation of dynamic property SplObjectStorage::$p0 is deprecated in %s on line 2
object(SplObjectStorage)#1 (2) {
  ["p0"]=>
  int(1)
  ["storage":"SplObjectStorage":private]=>
  array(0) {
  }
}

Deprecated: Creation of dynamic property ArrayObject::$p0 is deprecated in %s on line 3

Deprecated: Creation of dynamic property ArrayObject::$p0 is deprecated in %s on line 3
object(ArrayObject)#2 (2) {
  ["p0"]=>
  int(1)
  ["storage":"ArrayObject":private]=>
  array(1) {
    [0]=>
    int(1)
  }
}

Deprecated: Creation of dynamic property ArrayIterator::$p0 is deprecated in %s on line 4

Deprecated: Creation of dynamic property ArrayIterator::$p0 is deprecated in %s on line 4
object(ArrayIterator)#3 (2) {
  ["p0"]=>
  int(1)
  ["storage":"ArrayIterator":private]=>
  array(1) {
    [0]=>
    int(1)
  }
}

Deprecated: Creation of dynamic property SplDoublyLinkedList::$p0 is deprecated in %s on line 5
object(SplDoublyLinkedList)#4 (3) {
  ["p0"]=>
  int(1)
  ["flags":"SplDoublyLinkedList":private]=>
  int(0)
  ["dllist":"SplDoublyLinkedList":private]=>
  array(0) {
  }
}
object(S)#5 (3) {
  ["d"]=>
  int(5)
  ["e":protected]=>
  int(2)
  ["storage":"SplObjectStorage":private]=>
  array(0) {
  }
}

Deprecated: Creation of dynamic property SplObjectStorage::$p0 is deprecated in %s on line 8
object(SplObjectStorage)#6 (2) {
  ["p0"]=>
  int(1)
  ["storage":"SplObjectStorage":private]=>
  array(0) {
  }
}

Deprecated: Creation of dynamic property SplMinHeap::$q is deprecated in %s on line 9

Deprecated: Creation of dynamic property SplMinHeap::$q is deprecated in %s on line 9
object(SplMinHeap)#7 (4) {
  ["q"]=>
  int(1)
  ["flags":"SplHeap":private]=>
  int(0)
  ["isCorrupted":"SplHeap":private]=>
  bool(false)
  ["heap":"SplHeap":private]=>
  array(0) {
  }
}
