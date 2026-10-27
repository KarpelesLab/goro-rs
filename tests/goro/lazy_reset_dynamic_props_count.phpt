--TEST--
var_dump() count of a lazy object reset after it had dynamic properties
--FILE--
<?php
#[\AllowDynamicProperties]
class C { public $b; public $c = 1; }
$r = new ReflectionClass(C::class);
$o = $r->newLazyGhost(function ($o) {});
var_dump($o);
$o = new C; $r->resetAsLazyGhost($o, function ($o) {});
var_dump($o);
$o = new C; $o->d = 1; $r->resetAsLazyGhost($o, function ($o) {});
var_dump($o);
$o = new C; $o->d = 1; $r->resetAsLazyProxy($o, function ($o) { return new C; });
var_dump($o);
$o = new C; $o->d = 1; $r->resetAsLazyGhost($o, function ($o) {}); $r->getProperty('b')->setRawValueWithoutLazyInitialization($o, 5);
var_dump($o);
var_dump($o->c);
var_dump($o);
--EXPECT--
lazy ghost object(C)#3 (0) {
}
lazy ghost object(C)#4 (0) {
}
lazy ghost object(C)#2 (2) {
}
lazy proxy object(C)#3 (2) {
}
lazy ghost object(C)#4 (2) {
  ["b"]=>
  int(5)
}
int(1)
object(C)#4 (2) {
  ["b"]=>
  int(5)
  ["c"]=>
  int(1)
}
