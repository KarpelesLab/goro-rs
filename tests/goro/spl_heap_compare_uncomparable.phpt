--TEST--
SplHeap/SplPriorityQueue compare() maps uncomparable values to 1
--FILE--
<?php
class Mi extends SplMinHeap{function c($a,$b){return $this->compare($a,$b);}}
class Ma extends SplMaxHeap{function c($a,$b){return $this->compare($a,$b);}}
$a=new stdClass;$b=new ArrayObject([5]);
var_dump((new Mi)->c($a,$b), (new Mi)->c($b,$a), (new Ma)->c($a,$b), (new Ma)->c($b,$a));
$q = new SplPriorityQueue; var_dump($q->compare($a,$b), $q->compare($b,$a));
$h=new SplMinHeap;$h->insert($a);$h->insert($b);echo get_class($h->top()),"\n";
$h=new SplMaxHeap;$h->insert($a);$h->insert($b);echo get_class($h->top()),"\n";
$q=new SplPriorityQueue;$q->insert(1,$a);$q->insert(2,$b);echo $q->top(),"\n";
--EXPECT--
int(1)
int(1)
int(1)
int(1)
int(1)
int(1)
stdClass
stdClass
1
