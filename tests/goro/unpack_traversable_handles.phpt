--TEST--
Unpacking takes an iterator handle like zend_object_iterator
--FILE--
<?php
function p($l) { $o = new stdClass; $p = new stdClass; echo "$l: ", spl_object_id($o), " ", spl_object_id($p), "\n"; }
$d = new stdClass;
p(0);
class G implements IteratorAggregate { function getIterator(): Generator { yield 'k' => 'v'; yield 7; } }
$r = [...new G]; p(1);
try { $r = [...new stdClass]; } catch (Throwable $e) { echo get_class($e), "\n"; }
p(2);
unset($e); p(3);
try { $five = 5; $r = [...$five]; } catch (Throwable $e) { echo get_class($e), "\n"; }
unset($e); p(4);
function kk() { yield 1.5 => 1; }
try { $r = [...kk()]; } catch (Throwable $e) { echo get_class($e), "\n"; }
unset($e); p(5);
class B implements IteratorAggregate { function getIterator(): Iterator { return new ArrayIterator(['x' => 1, '01' => 2, '1' => 3]); } }
$r = [...new B]; p(6);
$r = [...new ArrayObject([1, 'q' => 2])]; p(7);
$s = new SplDoublyLinkedList; $s->push(1); $s->push(2);
$r = [...$s]; p(8);
$r = [...new SplFixedArray(2)]; p(9);
--EXPECT--
0: 2 3
1: 3 4
TypeError
2: 4 2
3: 3 2
Error
4: 2 3
Error
5: 4 3
6: 3 2
7: 2 4
8: 2 3
9: 3 2
