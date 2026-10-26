--TEST--
iterator_apply() of an IteratorAggregate (ArrayObject, DOMNodeList, DatePeriod)
--FILE--
<?php
$d = new DOMDocument; $d->loadXML("<r><a/><b/><c/></r>");
$l = $d->documentElement->childNodes;
var_dump(iterator_apply($l, function () { echo "x\n"; return true; }));
var_dump(iterator_apply(new ArrayIterator([1, 2]), function () { echo "y\n"; return true; }));
var_dump(iterator_apply(new ArrayObject([1, 2]), function () { echo "z\n"; return true; }));
$p = new DatePeriod(new DateTimeImmutable('2024-01-01'), new DateInterval('P1D'), 2);
var_dump(iterator_apply($p, function () { echo "w\n"; return true; }));
var_dump(iterator_apply($p, function ($a) { echo "v $a\n"; return false; }, [5]));
--EXPECT--
x
x
x
int(3)
y
y
int(2)
z
z
int(2)
w
w
w
int(3)
v 5
int(1)
