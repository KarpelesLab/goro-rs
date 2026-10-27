--TEST--
WeakReference::create() of an object reusing a freed object's handle is a new WeakReference
--FILE--
<?php
class A {}
$a = new A;
$w = WeakReference::create($a);
unset($a);
var_dump($w->get());
$a = new A;
$w = WeakReference::create($a);
var_dump($w->get() === $a);
$b = new A;
$w2 = WeakReference::create($b);
unset($b);
$b = new A;
$w3 = WeakReference::create($b);
unset($w2);
var_dump($w3->get() === $b);

?>
--EXPECT--
NULL
bool(true)
bool(true)
