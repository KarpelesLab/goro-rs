--TEST--
SPL: sorting an ArrayObject keeps the bucket index of its iterators (foreach, getIterator())
--FILE--
<?php
$ao = new ArrayObject([3, 1, 2]);
foreach ($ao as $k => $v) { if ($k == 0) $ao->asort(); echo "$k=$v "; }
echo "|\n";
$ao = new ArrayObject(['c' => 3, 'a' => 1, 'b' => 2]);
foreach ($ao as $k => $v) { if ($k == 'c') $ao->ksort(); echo "$k=$v "; }
echo "|\n";
$ao = new ArrayObject([5, 4, 3, 2]);
$i1 = $ao->getIterator(); $i2 = $ao->getIterator();
$i1->next(); $i2->next(); $i2->next();
$ao->uasort(fn($a, $b) => $a <=> $b);
var_dump($i1->key(), $i2->key());
unset($ao[3]);
$i1->next(); var_dump($i1->key());
$it = new ArrayIterator([5, 4, 3, 2]);
foreach ($it as $k => $v) { echo "$k=$v "; if ($k == 1) { $it->asort(); } }
echo "|\n";
$a = new ArrayObject([1, 2]); $a->asort();
var_dump($a == new ArrayObject([1, 2]), serialize($a));
?>
--EXPECT--
0=3 2=2 0=3 |
c=3 b=2 c=3 |
int(2)
int(1)
int(1)
0=5 1=4 1=4 0=5 |
bool(true)
string(73) "O:11:"ArrayObject":4:{i:0;i:0;i:1;a:2:{i:0;i:1;i:1;i:2;}i:2;a:0:{}i:3;N;}"
