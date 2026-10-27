--TEST--
debug_zval_dump() of $this in a method: the frame holds it once
--FILE--
<?php
class T { function d() { debug_zval_dump($this); } static function s($o) { debug_zval_dump($o); } function e() { $x = $this; debug_zval_dump($x); } }
$t = new T;
debug_zval_dump($t);
$t->d();
T::s($t);
$t->e();
function f($o) { debug_zval_dump($o); }
f($t);
(new T)->d();
--EXPECT--
object(T)#1 (0) refcount(2){
}
object(T)#1 (0) refcount(3){
}
object(T)#1 (0) refcount(3){
}
object(T)#1 (0) refcount(4){
}
object(T)#1 (0) refcount(3){
}
object(T)#2 (0) refcount(2){
}
