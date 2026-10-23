--TEST--
SplDoublyLinkedList internal iterators do not move the list's cursor
--FILE--
<?php
foreach ([new SplDoublyLinkedList, new SplQueue, new SplStack] as $x) {
    echo get_class($x), "\n";
    $x[] = 5; $x[] = 6; $x[] = 7;
    var_dump(iterator_to_array($x));
    var_dump($x->key(), $x->valid());
    var_dump(iterator_count($x));
    var_dump($x->key());
    $x->rewind(); $x->next();
    var_dump($x->key(), $x->current());
    foreach ($x as $k => $v) { echo "$k=>$v "; }
    echo "\n";
    var_dump($x->key(), $x->current());
    var_dump(iterator_to_array(new IteratorIterator($x)));
    var_dump($x->key(), $x->current());
    // nested foreach
    foreach ($x as $k => $v) { foreach ($x as $k2 => $v2) echo "$k$k2 "; }
    echo "\n";
    var_dump($x->key());
    $x->setIteratorMode($x->getIteratorMode() | SplDoublyLinkedList::IT_MODE_DELETE);
    var_dump(iterator_to_array($x));
    var_dump(count($x), $x->key());
}
$l = new SplDoublyLinkedList; $l->push(1); $l->push(2);
foreach ($l as $k => $v) { echo "$k=>$v\n"; if ($k == 0) $l->push(3); }
class L extends SplDoublyLinkedList { function current(): mixed { return parent::current() * 10; } }
$l = new L; $l->push(1); $l->push(2);
var_dump(iterator_to_array($l), $l->key());
--EXPECT--
SplDoublyLinkedList
array(3) {
  [0]=>
  int(5)
  [1]=>
  int(6)
  [2]=>
  int(7)
}
int(0)
bool(false)
int(3)
int(0)
int(1)
int(6)
0=>5 1=>6 2=>7 
int(1)
int(6)
array(3) {
  [0]=>
  int(5)
  [1]=>
  int(6)
  [2]=>
  int(7)
}
int(1)
int(6)
00 01 02 10 11 12 20 21 22 
int(1)
array(1) {
  [0]=>
  int(7)
}
int(0)
int(1)
SplQueue
array(3) {
  [0]=>
  int(5)
  [1]=>
  int(6)
  [2]=>
  int(7)
}
int(0)
bool(false)
int(3)
int(0)
int(1)
int(6)
0=>5 1=>6 2=>7 
int(1)
int(6)
array(3) {
  [0]=>
  int(5)
  [1]=>
  int(6)
  [2]=>
  int(7)
}
int(1)
int(6)
00 01 02 10 11 12 20 21 22 
int(1)
array(1) {
  [0]=>
  int(7)
}
int(0)
int(1)
SplStack
array(3) {
  [2]=>
  int(7)
  [1]=>
  int(6)
  [0]=>
  int(5)
}
int(0)
bool(false)
int(3)
int(0)
int(1)
int(6)
2=>7 1=>6 0=>5 
int(1)
int(6)
array(3) {
  [2]=>
  int(7)
  [1]=>
  int(6)
  [0]=>
  int(5)
}
int(1)
int(6)
22 21 20 12 11 10 02 01 00 
int(1)
array(3) {
  [2]=>
  int(7)
  [1]=>
  int(6)
  [0]=>
  int(5)
}
int(0)
int(1)
0=>1
1=>2
2=>3
array(2) {
  [0]=>
  int(10)
  [1]=>
  int(20)
}
int(2)
