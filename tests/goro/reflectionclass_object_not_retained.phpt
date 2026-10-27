--TEST--
ReflectionClass constructed from an object does not keep it alive (ReflectionObject does)
--FILE--
<?php
class A {}
$a = new A;
$w = WeakReference::create($a);
$r = new ReflectionClass($a);
var_dump($r->isUninitializedLazyObject($a));
unset($a);
var_dump($w->get() === null);
$a = new A;
$w = WeakReference::create($a);
$r2 = new ReflectionObject($a);
unset($a);
var_dump($w->get() === null);
unset($r2);
var_dump($w->get() === null);

?>
--EXPECT--
bool(false)
bool(true)
bool(false)
bool(true)
