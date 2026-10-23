--TEST--
print_r(): SPL containers show their get_debug_info table (declaring class in private names)
--FILE--
<?php
class MyStack extends SplStack { public $x = 1; }
class MyAO extends ArrayObject { public $y = 2; }
$s = new MyStack; $s->push(1);
print_r($s); var_dump($s);
print_r(new SplQueue);
print_r(new MyAO([1]));
var_dump(new MyAO([1]));
print_r(new RecursiveArrayIterator([1]));
var_dump(new RecursiveArrayIterator([1]));
$h = new SplMinHeap; $h->insert(3);
print_r($h); var_dump($h);
$o = new SplObjectStorage; $o[new stdClass] = 1;
print_r($o);
class MyOS extends SplObjectStorage {}
print_r(new MyOS);
var_dump(new MyOS);
print_r(new SplFixedArray(2));
print_r(new SplPriorityQueue);
--EXPECT--
MyStack Object
(
    [x] => 1
    [flags:SplDoublyLinkedList:private] => 6
    [dllist:SplDoublyLinkedList:private] => Array
        (
            [0] => 1
        )

)
object(MyStack)#1 (3) {
  ["x"]=>
  int(1)
  ["flags":"SplDoublyLinkedList":private]=>
  int(6)
  ["dllist":"SplDoublyLinkedList":private]=>
  array(1) {
    [0]=>
    int(1)
  }
}
SplQueue Object
(
    [flags:SplDoublyLinkedList:private] => 4
    [dllist:SplDoublyLinkedList:private] => Array
        (
        )

)
MyAO Object
(
    [y] => 2
    [storage:ArrayObject:private] => Array
        (
            [0] => 1
        )

)
object(MyAO)#2 (2) {
  ["y"]=>
  int(2)
  ["storage":"ArrayObject":private]=>
  array(1) {
    [0]=>
    int(1)
  }
}
RecursiveArrayIterator Object
(
    [storage:ArrayIterator:private] => Array
        (
            [0] => 1
        )

)
object(RecursiveArrayIterator)#2 (1) {
  ["storage":"ArrayIterator":private]=>
  array(1) {
    [0]=>
    int(1)
  }
}
SplMinHeap Object
(
    [flags:SplHeap:private] => 0
    [isCorrupted:SplHeap:private] => 
    [heap:SplHeap:private] => Array
        (
            [0] => 3
        )

)
object(SplMinHeap)#2 (3) {
  ["flags":"SplHeap":private]=>
  int(0)
  ["isCorrupted":"SplHeap":private]=>
  bool(false)
  ["heap":"SplHeap":private]=>
  array(1) {
    [0]=>
    int(3)
  }
}
SplObjectStorage Object
(
    [storage:SplObjectStorage:private] => Array
        (
            [0] => Array
                (
                    [obj] => stdClass Object
                        (
                        )

                    [inf] => 1
                )

        )

)
MyOS Object
(
    [storage:SplObjectStorage:private] => Array
        (
        )

)
object(MyOS)#5 (1) {
  ["storage":"SplObjectStorage":private]=>
  array(0) {
  }
}
SplFixedArray Object
(
    [0] => 
    [1] => 
)
SplPriorityQueue Object
(
    [flags:SplPriorityQueue:private] => 1
    [isCorrupted:SplPriorityQueue:private] => 
    [heap:SplPriorityQueue:private] => Array
        (
        )

)
