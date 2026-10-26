--TEST--
SPL: a cloned SplDoublyLinkedList starts traversal at its head
--FILE--
<?php
$l = new SplDoublyLinkedList; $l->push(1); $l->push(2); $l->push(3);
$l->rewind(); $l->next(); $l->next();
$d = clone $l; var_dump($d->valid(), $d->key(), $d->current()); $d->next(); var_dump($d->current());
$s = new SplStack; $s->push(1); $s->push(2); $s->rewind();
$d = clone $s; var_dump($d->valid(), $d->key(), $d->current()); $d->next(); var_dump($d->valid(), $d->current());
$q = new SplQueue; $d = clone $q; var_dump($d->valid(), $d->current());
$q->push('a'); $q->rewind(); $q->shift(); $d = clone $q; var_dump($d->valid(), $d->current());
class L extends SplQueue {} $l = new L; $l[] = 5; $l[] = 6; $l->rewind(); $l->next(); $d = clone $l; var_dump($d->current());
?>
--EXPECT--
bool(true)
int(0)
int(1)
int(2)
bool(true)
int(0)
int(1)
bool(false)
NULL
bool(false)
NULL
bool(false)
NULL
int(5)
