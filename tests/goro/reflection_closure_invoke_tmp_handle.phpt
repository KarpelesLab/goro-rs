--TEST--
ReflectionClass(Closure)::getMethod(s)('__invoke') uses a temporary Closure handle
--FILE--
<?php
$rc = new ReflectionClass(Closure::class);
foreach ($rc->getMethods() as $method) {
    echo spl_object_id($method), " ", $method->name, "\n";
}
$closure = function($a) { echo $a; };
$r = new ReflectionClass('closure');
var_dump(spl_object_id($r));
var_dump($r->hasMethod('__invoke'));
$r2 = new ReflectionClass($closure);
var_dump(spl_object_id($r2));
var_dump($h = $r2->getMethod('__invoke'));
$o = new stdClass;
var_dump(spl_object_id($o));
--EXPECT--
2 __construct
3 bind
4 bindTo
5 call
6 fromCallable
7 getCurrent
9 __invoke
int(6)
bool(true)
int(5)
object(ReflectionMethod)#3 (2) {
  ["name"]=>
  string(8) "__invoke"
  ["class"]=>
  string(7) "Closure"
}
int(4)
