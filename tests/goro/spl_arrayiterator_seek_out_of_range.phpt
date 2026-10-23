--TEST--
ArrayIterator::seek() past the end leaves the iterator at the end
--FILE--
<?php
$x = new ArrayIterator([1,2,3]);
$x->seek(1); var_dump($x->key());
try { $x->seek(10); } catch (OutOfBoundsException $e) { echo $e->getMessage(), "\n"; }
var_dump($x->key(), $x->valid(), $x->current());
$x->seek(1);
try { $x->seek(-1); } catch (OutOfBoundsException $e) { echo $e->getMessage(), "\n"; }
var_dump($x->key(), $x->valid());
$x->seek(2); var_dump($x->key());
try { $x->seek(3); } catch (OutOfBoundsException $e) { echo $e->getMessage(), "\n"; }
var_dump($x->key(), $x->valid());
$x[] = 4; var_dump($x->key(), $x->current());
--EXPECT--
int(1)
Seek position 10 is out of range
NULL
bool(false)
NULL
Seek position -1 is out of range
int(1)
bool(true)
int(2)
Seek position 3 is out of range
NULL
bool(false)
int(3)
int(4)
