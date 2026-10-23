--TEST--
Object handles of foreach iterators over generators and finished generator frames
--FILE--
<?php
// foreach over a Generator takes an iterator object handle before the
// rewind runs the generator; a finished frame releases its locals.
function g() { $o = new stdClass; echo "o=", spl_object_id($o), "\n"; yield 1; }
$d = g();
var_dump(spl_object_id(new stdClass));
$d->current();
$e = g();
foreach ($e as $v) { var_dump(spl_object_id(new stdClass)); }
function &r() { $v = 1; $o = new stdClass; echo "ro=", spl_object_id($o), "\n"; yield $v; }
foreach (r() as &$x) { echo spl_object_id(new stdClass), "\n"; }
function inner() { yield 1; }
function outer() { yield from inner(); yield 2; }
$c = outer();
foreach ($c as $v) {}
var_dump(spl_object_id(new stdClass));
function outer2() { foreach (g() as $v) { echo "in ", spl_object_id(new stdClass), "\n"; yield $v; } }
foreach (outer2() as $v) { echo spl_object_id(new stdClass), "\n"; }
--EXPECT--
int(2)
o=2
o=5
int(6)
ro=6
7
int(4)
o=9
in 10
10
