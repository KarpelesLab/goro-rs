--TEST--
A popped or shifted current SplDoublyLinkedList node stays current
--FILE--
<?php
$l=new SplDoublyLinkedList;$l->push(1);$l->rewind();$l->pop();var_dump($l->valid(),$l->current(), $l->key());
$l->next(); var_dump($l->valid(), $l->current());
$l=new SplDoublyLinkedList;$l->push(1);$l->push(2);$l->rewind();$l->shift();var_dump($l->valid(),$l->current(),$l->key());
$l->next(); var_dump($l->valid(),$l->current());
$l=new SplDoublyLinkedList;$l->push(1);$l->push(2);$l->rewind();$l->offsetUnset(0);var_dump($l->valid(),$l->current(),$l->key());
$l->next(); var_dump($l->valid(),$l->current());
$l=new SplDoublyLinkedList;$l->push(1);$l->push(2);$l->rewind();$l->next();$l->pop();var_dump($l->valid(),$l->current());
$l->push(3); var_dump($l->valid(),$l->current());
$l->prev(); var_dump($l->valid(),$l->current());
$l=new SplDoublyLinkedList;$l->push(1);$l->push(2);foreach ($l as $k => $v) { echo "$k=>$v\n"; $l->shift(); }
var_dump(count($l));
foreach ([fn($l) => $l["a"] = 1, fn($l) => $l["a"], fn($l) => isset($l["a"]), fn($l) => $l->offsetSet("a", 1), fn($l) => $l->offsetGet("a"), fn($l) => $l->offsetExists("a"), fn($l) => $l->offsetUnset("a"), fn($l) => $l->add("a", 1), fn($l) => $l[1.5] = 1, fn($l) => $l[null] = 7] as $f) {
--EXPECTF--

Parse error: Unclosed '{' on line 13 in %s on line 14
