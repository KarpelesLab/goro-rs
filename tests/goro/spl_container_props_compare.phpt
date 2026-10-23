--TEST--
SPL lists, heaps and object storages: (array)/json show real properties, == compares properties only
--FILE--
<?php
$l = new SplDoublyLinkedList; $l->push('a');
$q = new SplQueue; $q->enqueue(1);
$h = new SplMinHeap; $h->insert(2);
$p = new SplPriorityQueue; $p->insert('x', 1);
$s = new SplObjectStorage; $s[new stdClass] = 1;
$f = SplFixedArray::fromArray([1, 2]);
$ao = new ArrayObject([1, 2]);
foreach ([$l, $q, $h, $p, $s, $f, $ao] as $o) {
    echo get_class($o), ' ', json_encode((array) $o), ' ', json_encode($o), ' ', count(get_object_vars($o)), "\n";
}
class MyList extends SplDoublyLinkedList { public $pub = 1; }
$m = new MyList; $m->push(5);
var_dump((array) $m, json_encode($m));
var_dump(new SplDoublyLinkedList == new SplDoublyLinkedList, SplFixedArray::fromArray([1]) == SplFixedArray::fromArray([2]));
$a = new SplDoublyLinkedList; $a->push(1); $b = new SplDoublyLinkedList; $b->push(2); var_dump($a == $b);
$a = new SplMinHeap; $a->insert(1); $b = new SplMinHeap; $b->insert(2); var_dump($a == $b);
var_dump(new ArrayObject([1]) == new ArrayObject([2]), new ArrayObject([1]) == new ArrayObject([1]));
--EXPECT--
SplDoublyLinkedList [] {} 0
SplQueue [] {} 0
SplMinHeap [] {} 0
SplPriorityQueue [] {} 0
SplObjectStorage [] {} 0
SplFixedArray [1,2] [1,2] 0
ArrayObject [1,2] {"0":1,"1":2} 0
array(1) {
  ["pub"]=>
  int(1)
}
string(9) "{"pub":1}"
bool(true)
bool(true)
bool(true)
bool(true)
bool(false)
bool(true)
