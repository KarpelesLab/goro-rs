--TEST--
SplDoublyLinkedList offset TypeErrors name the parameter type
--FILE--
<?php
foreach ([fn($l) => $l["a"] = 1, fn($l) => $l["a"], fn($l) => isset($l["a"]), fn($l) => $l->offsetSet("a", 1), fn($l) => $l->offsetGet("a"), fn($l) => $l->offsetExists("a"), fn($l) => $l->offsetUnset("a"), fn($l) => $l->add("a", 1), fn($l) => $l[1.5] = 1, fn($l) => $l[null] = 7] as $f) {
    $l = new SplDoublyLinkedList; $l->push(0);
    try { var_dump($f($l)); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
}
--EXPECTF--
TypeError: SplDoublyLinkedList::offsetSet(): Argument #1 ($index) must be of type ?int, string given
TypeError: SplDoublyLinkedList::offsetGet(): Argument #1 ($index) must be of type int, string given
TypeError: SplDoublyLinkedList::offsetExists(): Argument #1 ($index) must be of type int, string given
TypeError: SplDoublyLinkedList::offsetSet(): Argument #1 ($index) must be of type ?int, string given
TypeError: SplDoublyLinkedList::offsetGet(): Argument #1 ($index) must be of type int, string given
TypeError: SplDoublyLinkedList::offsetExists(): Argument #1 ($index) must be of type int, string given
TypeError: SplDoublyLinkedList::offsetUnset(): Argument #1 ($index) must be of type int, string given
TypeError: SplDoublyLinkedList::add(): Argument #1 ($index) must be of type int, string given

Deprecated: Implicit conversion from float 1.5 to int loses precision in %s on line 2
OutOfRangeException: SplDoublyLinkedList::offsetSet(): Argument #1 ($index) is out of range
int(7)
