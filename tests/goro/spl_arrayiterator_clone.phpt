--TEST--
A cloned ArrayIterator starts at the beginning and compares with the storage it shares
--FILE--
<?php
$x=new ArrayIterator([1,2]); $x->next(); $c=clone $x; var_dump($c->current(),$c->key(), $x->key());
$y=new ArrayIterator([]); var_dump(clone $y == $y);
var_dump(clone $x == $x);
$x=new RecursiveArrayIterator([1,[2]]); $x->next(); $c=clone $x; var_dump($c->current(),$c->key());
$y=new RecursiveArrayIterator([]); var_dump(clone $y == $y);
$o = new ArrayObject([1,2,3]); $it = $o->getIterator(); $it->next(); $c = clone $it; var_dump($c->key());
$c[] = 4; var_dump(count($o), count($c));
var_dump($c == $it, $c == $o);
$z = new ArrayIterator([1,2,3]); var_dump($z == clone $x);
--EXPECT--
int(1)
int(0)
int(1)
bool(true)
bool(true)
int(1)
int(0)
bool(true)
int(0)
int(4)
int(4)
bool(true)
bool(false)
bool(false)
