--TEST--
Object handles taken by SPL iterators
--FILE--
<?php
$l=new SplDoublyLinkedList;$l->push(1);foreach($l as $v){ var_dump(new stdClass); }var_dump(new stdClass);
var_dump(iterator_to_array($l)); var_dump(new stdClass);
$ii = new IteratorIterator($l); var_dump(new stdClass); foreach ($ii as $v) { var_dump(new stdClass); }
unset($ii); var_dump(new stdClass);
$f = new SplFixedArray(2); $it = $f->getIterator(); var_dump($it, new stdClass);
unset($it); var_dump(new stdClass);
foreach ($f as $v) { var_dump(new stdClass); }
var_dump(new stdClass);
--EXPECT--
object(stdClass)#3 (0) {
}
object(stdClass)#2 (0) {
}
array(1) {
  [0]=>
  int(1)
}
object(stdClass)#2 (0) {
}
object(stdClass)#4 (0) {
}
object(stdClass)#5 (0) {
}
object(stdClass)#2 (0) {
}
object(InternalIterator)#4 (0) {
}
object(stdClass)#5 (0) {
}
object(stdClass)#4 (0) {
}
object(stdClass)#3 (0) {
}
object(stdClass)#3 (0) {
}
object(stdClass)#4 (0) {
}
